//! Session-bound Codex V2 Agent Thread dispatch.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use subagents::{
    AgentControl, AgentPath, AgentStatusV2, AgentThread, AgentThreadMessage, AgentThreadStatus,
    AgentThreadStore, CloseAgentRequest, InterruptAgentRequest, InterruptAgentV2Request,
    InterruptAgentV2Result, ListAgentThreadsRequest, ListAgentsV2Request, MessageAgentV2Request,
    MessageAgentV2Result, ReadAgentThreadRequest, SendAgentMessageRequest, SpawnAgentV2Result,
    SpawnRuntimeV2Request, WaitAgentV2Request, WaitAgentV2Result, WaitOutcome,
};
use tools::{AgentThreadDispatch, SpawnAgentDispatchRequest};

use super::agent_runtime::{AgentRuntimeManager, RunAgentTurnRequest};
use crate::runtime::{Config, Session};

#[derive(Clone)]
struct StoredRuntimeRequest {
    runtime: SpawnRuntimeV2Request,
    memory_dir: PathBuf,
}

#[derive(Default)]
struct RuntimeRequestRegistry {
    requests: Mutex<HashMap<String, StoredRuntimeRequest>>,
}

impl RuntimeRequestRegistry {
    fn global() -> Arc<Self> {
        static REGISTRY: OnceLock<Arc<RuntimeRequestRegistry>> = OnceLock::new();
        Arc::clone(REGISTRY.get_or_init(|| Arc::new(Self::default())))
    }

    fn insert(&self, thread_id: &str, request: StoredRuntimeRequest) -> anyhow::Result<()> {
        let mut requests = self
            .requests
            .lock()
            .map_err(|_| anyhow::anyhow!("agent runtime request registry mutex is poisoned"))?;
        if requests.contains_key(thread_id) {
            anyhow::bail!("runtime request already exists for agent thread {thread_id:?}");
        }
        requests.insert(thread_id.to_string(), request);
        Ok(())
    }

    fn get(&self, thread_id: &str) -> anyhow::Result<Option<StoredRuntimeRequest>> {
        Ok(self
            .requests
            .lock()
            .map_err(|_| anyhow::anyhow!("agent runtime request registry mutex is poisoned"))?
            .get(thread_id)
            .cloned())
    }

    fn remove(&self, thread_id: &str) {
        if let Ok(mut requests) = self.requests.lock() {
            requests.remove(thread_id);
        }
    }
}

/// Dispatcher bound to exactly one current Agent Thread session.
pub struct DefaultAgentThreadDispatch {
    control: Arc<AgentControl>,
    current_path: AgentPath,
    current_thread_id: String,
    runtime_manager: Arc<AgentRuntimeManager>,
    runtime_requests: Arc<RuntimeRequestRegistry>,
    #[cfg(test)]
    chat_override: Option<crate::streaming::ChatOverride>,
}

impl DefaultAgentThreadDispatch {
    pub fn for_session(
        control: Arc<AgentControl>,
        current_path: AgentPath,
        current_thread_id: String,
    ) -> Self {
        Self {
            control,
            current_path,
            current_thread_id,
            runtime_manager: AgentRuntimeManager::global(),
            runtime_requests: RuntimeRequestRegistry::global(),
            #[cfg(test)]
            chat_override: None,
        }
    }

    #[cfg(test)]
    fn for_test(
        control: Arc<AgentControl>,
        current_path: AgentPath,
        current_thread_id: String,
        runtime_manager: Arc<AgentRuntimeManager>,
    ) -> Self {
        Self {
            control,
            current_path,
            current_thread_id,
            runtime_manager,
            runtime_requests: Arc::new(RuntimeRequestRegistry::default()),
            chat_override: None,
        }
    }

    async fn launch_turn(&self, request: RunAgentTurnRequest) -> anyhow::Result<()> {
        let thread_id = request.thread.thread_id.clone();
        let manager = Arc::clone(&self.runtime_manager);
        let run_manager = Arc::clone(&manager);
        let mut handle = tokio::spawn(async move { run_manager.start_turn(request).await });
        loop {
            if manager.is_running(&thread_id) && self.control.runtime_handle(&thread_id)?.is_some()
            {
                return Ok(());
            }
            tokio::select! {
                result = &mut handle => {
                    return result
                        .map_err(|error| anyhow::anyhow!("agent runtime task failed to start: {error}"))?;
                }
                _ = tokio::task::yield_now() => {}
            }
        }
    }

    fn run_request(
        &self,
        thread: subagents::AgentThreadV2,
        stored: StoredRuntimeRequest,
    ) -> RunAgentTurnRequest {
        RunAgentTurnRequest {
            control: Arc::clone(&self.control),
            thread,
            runtime: stored.runtime,
            memory_dir: stored.memory_dir,
            #[cfg(test)]
            chat_override: self.chat_override.clone(),
            #[cfg(not(test))]
            chat_override: None,
        }
    }
}

#[async_trait]
impl AgentThreadDispatch for DefaultAgentThreadDispatch {
    async fn spawn_agent(
        &self,
        request: SpawnAgentDispatchRequest,
    ) -> anyhow::Result<SpawnAgentV2Result> {
        let settings =
            subagents::load_agents_settings(&request.memory_dir, request.project_root.as_deref());
        if !settings.enabled {
            anyhow::bail!("agent threads are disabled by Codex agent settings");
        }
        let agent_type = request.request.agent_type.as_deref().unwrap_or("default");
        let catalog =
            subagents::load_agent_catalog(&request.memory_dir, request.project_root.as_deref());
        let resolved = subagents::resolve_agent(
            &catalog,
            &settings,
            agent_type,
            request.request.model.as_deref(),
            request.request.reasoning_effort.as_deref(),
            request.parent_model.as_deref(),
            Some(&request.parent_sandbox_mode),
        )?;

        let reservation = self.control.reserve_spawn_typed(
            &self.current_path,
            &request.request.task_name,
            &resolved.definition.name,
        )?;
        let thread = reservation.thread().clone();
        let memory_dir = request.memory_dir.clone();
        let runtime = build_runtime_request(
            request,
            resolved,
            &self.current_path,
            &self.current_thread_id,
            self.control.root_thread_id(),
            settings.interrupt_message,
        );

        fork_parent_session(
            &memory_dir,
            &runtime,
            &thread.session_id,
            &runtime.model_request.fork_turns,
        )?;
        validate_runtime_setup(
            &memory_dir,
            &self.control,
            &thread,
            &runtime,
            &thread.session_id,
        )?;

        let stored = StoredRuntimeRequest {
            memory_dir,
            runtime,
        };
        self.runtime_requests
            .insert(&thread.thread_id, stored.clone())?;
        if let Err(error) = reservation.commit() {
            self.runtime_requests.remove(&thread.thread_id);
            return Err(error);
        }
        if let Err(start_error) = self
            .launch_turn(self.run_request(thread.clone(), stored))
            .await
        {
            self.runtime_requests.remove(&thread.thread_id);
            return match self.control.abort_committed_pending_spawn(&thread) {
                Ok(()) => Err(start_error.context("start agent runtime")),
                Err(rollback_error) => Err(anyhow::anyhow!(
                    "start agent runtime failed: {start_error:#}; pending spawn rollback failed: {rollback_error:#}"
                )),
            };
        }
        Ok(SpawnAgentV2Result { thread })
    }

    async fn list_agents(
        &self,
        request: ListAgentsV2Request,
    ) -> anyhow::Result<Vec<subagents::AgentThreadV2>> {
        self.control
            .list_agents(&self.current_path, request.path_prefix.as_deref())
    }

    async fn send_message(
        &self,
        request: MessageAgentV2Request,
    ) -> anyhow::Result<MessageAgentV2Result> {
        let message = self
            .control
            .enqueue_message(&self.current_path, request, false)?;
        Ok(MessageAgentV2Result {
            message_id: message.message_id,
            queued: true,
            turn_triggered: false,
        })
    }

    async fn followup_task(
        &self,
        request: MessageAgentV2Request,
    ) -> anyhow::Result<MessageAgentV2Result> {
        let target = self
            .control
            .resolve_target(&self.current_path, &request.target)?;
        if target.status == AgentStatusV2::Shutdown {
            anyhow::bail!(
                "cannot follow up a Shutdown agent: {}",
                target.canonical_path
            );
        }
        let followup_text = request.message.trim().to_string();
        let message = self
            .control
            .enqueue_message(&self.current_path, request, true)?;
        if !self.runtime_manager.is_running(&target.thread_id) {
            let mut stored = self
                .runtime_requests
                .get(&target.thread_id)?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "agent runtime configuration is unavailable for {}",
                        target.canonical_path
                    )
                })?;
            stored.runtime.model_request.message = followup_text;
            self.launch_turn(self.run_request(target, stored)).await?;
        }
        Ok(MessageAgentV2Result {
            message_id: message.message_id,
            queued: true,
            turn_triggered: true,
        })
    }

    async fn wait_agent(&self, request: WaitAgentV2Request) -> anyhow::Result<WaitAgentV2Result> {
        let timeout_ms = request.timeout_ms.unwrap_or(30_000);
        anyhow::ensure!(
            (10_000..=3_600_000).contains(&timeout_ms),
            "normalized timeout_ms must be between 10000 and 3600000"
        );
        let cursor = self.control.activity_cursor();
        let outcome = self
            .control
            .wait_activity(cursor, Duration::from_millis(timeout_ms as u64))
            .await;
        Ok(match outcome {
            WaitOutcome::MailboxActivity => WaitAgentV2Result {
                message: "Agent Thread activity is available.".into(),
                timed_out: false,
            },
            WaitOutcome::Steered => WaitAgentV2Result {
                message: "The main task received new input.".into(),
                timed_out: false,
            },
            WaitOutcome::TimedOut => WaitAgentV2Result {
                message: "Timed out waiting for Agent Thread activity.".into(),
                timed_out: true,
            },
        })
    }

    async fn interrupt_agent(
        &self,
        request: InterruptAgentV2Request,
    ) -> anyhow::Result<InterruptAgentV2Result> {
        let target = self
            .control
            .resolve_target(&self.current_path, &request.target)?;
        anyhow::ensure!(
            target.canonical_path != AgentPath::root(),
            "the root agent cannot be interrupted through model tools"
        );
        let previous_status = target.status.clone();
        self.runtime_manager.interrupt(&target.thread_id).await?;
        let thread = self
            .control
            .resolve_target(&self.current_path, target.canonical_path.as_str())?;
        Ok(InterruptAgentV2Result {
            thread,
            previous_status,
        })
    }

    fn notify_main_steer(&self) {
        self.control.notify_main_steer();
    }
}

fn build_runtime_request(
    request: SpawnAgentDispatchRequest,
    resolved: subagents::ResolvedAgent,
    parent_path: &AgentPath,
    parent_thread_id: &str,
    root_thread_id: &str,
    interrupt_message: bool,
) -> SpawnRuntimeV2Request {
    let mut model_request = request.request;
    model_request.agent_type = Some(resolved.definition.name.clone());
    model_request.model = resolved.model;
    model_request.reasoning_effort = resolved.model_reasoning_effort;

    let mut skills = request
        .inherited_skill_config
        .into_iter()
        .map(|(path, enabled)| subagents::SkillConfigEntry { path, enabled })
        .collect::<Vec<_>>();
    for overlay in &resolved.definition.skills.config {
        skills.retain(|entry| entry.path != overlay.path);
        skills.push(overlay.clone());
    }
    SpawnRuntimeV2Request {
        model_request,
        parent_thread_id: parent_thread_id.to_string(),
        parent_path: parent_path.clone(),
        root_thread_id: root_thread_id.to_string(),
        parent_session_id: parent_thread_id.to_string(),
        parent_agent_id: request.parent_agent_id,
        developer_instructions: resolved.definition.developer_instructions,
        context_snapshot: String::new(),
        sandbox_mode: resolved.sandbox_mode,
        mcp_servers: resolved.definition.mcp_servers,
        skills_config: skills,
        chat_targets: request.chat_targets,
        project_root: request.project_root,
        hook_bus: request.hook_bus,
        interrupt_message,
    }
}

fn fork_parent_session(
    memory_dir: &Path,
    runtime: &SpawnRuntimeV2Request,
    child_session_id: &str,
    fork_turns: &Option<String>,
) -> anyhow::Result<()> {
    let sessions = session::SessionStore::open_sessions_dir(&memory_dir.join("sessions"))?;
    let recent_turns = parse_fork_turns(fork_turns.as_deref())?;
    sessions.fork_session_recent_turns(&runtime.parent_session_id, child_session_id, recent_turns)
}

fn parse_fork_turns(value: Option<&str>) -> anyhow::Result<Option<usize>> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        None | Some("all") => Ok(None),
        Some("none") => Ok(Some(0)),
        Some(value) => {
            let turns = value.parse::<usize>().map_err(|_| {
                anyhow::anyhow!("fork_turns must be all, none, or a positive integer")
            })?;
            anyhow::ensure!(turns > 0, "fork_turns must be positive");
            Ok(Some(turns))
        }
    }
}

fn validate_runtime_setup(
    memory_dir: &Path,
    control: &Arc<AgentControl>,
    thread: &subagents::AgentThreadV2,
    runtime: &SpawnRuntimeV2Request,
    child_session_id: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !runtime.chat_targets.is_empty(),
        "agent turn has no chat target"
    );
    if let Some(model) = runtime.model_request.model.as_deref() {
        let _ = types::ModelSpec::parse(model)?;
    }
    let _ = mcp::decode_inline_mcp_servers(&runtime.mcp_servers)?;
    let config = Config::with_defaults(memory_dir.to_path_buf());
    let _session = Session::with_session_id_for_agent_thread(
        config,
        child_session_id.to_string(),
        &runtime.parent_agent_id,
        Arc::clone(control),
        thread.canonical_path.clone(),
    )?;
    Ok(())
}

/// Desktop-only legacy bridge retained until Task 7 rewrites Tauri commands.
/// It is not implemented by, or reachable through, [`AgentThreadDispatch`].
pub struct LegacyDesktopAgentThreadControl;

#[allow(non_upper_case_globals)]
pub const DefaultAgentThreadDispatch: LegacyDesktopAgentThreadControl =
    LegacyDesktopAgentThreadControl;

impl LegacyDesktopAgentThreadControl {
    pub async fn list_agents(
        &self,
        request: ListAgentThreadsRequest,
    ) -> anyhow::Result<Vec<AgentThread>> {
        AgentThreadStore::open_default()?
            .list(Some(&request.parent_session_id), request.include_closed)
    }

    pub async fn read_agent(
        &self,
        request: ReadAgentThreadRequest,
    ) -> anyhow::Result<(AgentThread, Vec<AgentThreadMessage>)> {
        let store = AgentThreadStore::open_default()?;
        let thread = require_legacy_owned(&store, &request.parent_session_id, &request.thread_id)?;
        Ok((thread, store.messages(&request.thread_id)?))
    }

    pub async fn send_message(
        &self,
        request: SendAgentMessageRequest,
    ) -> anyhow::Result<AgentThread> {
        let store = AgentThreadStore::open_default()?;
        let thread = require_legacy_owned(&store, &request.parent_session_id, &request.thread_id)?;
        anyhow::ensure!(
            thread.status != AgentThreadStatus::Closed,
            "agent thread is closed"
        );
        subagents::LiveAgentThreads::global()
            .send_follow_up(&request.thread_id, request.message.trim().to_string())?;
        Ok(thread)
    }

    pub async fn interrupt_agent(
        &self,
        request: InterruptAgentRequest,
    ) -> anyhow::Result<AgentThread> {
        let store = AgentThreadStore::open_default()?;
        let thread = require_legacy_owned(&store, &request.parent_session_id, &request.thread_id)?;
        if matches!(
            thread.status,
            AgentThreadStatus::Pending | AgentThreadStatus::Running
        ) {
            subagents::LiveAgentThreads::global().interrupt(&request.thread_id)?;
        }
        Ok(thread)
    }

    pub async fn close_agent(&self, request: CloseAgentRequest) -> anyhow::Result<AgentThread> {
        let store = AgentThreadStore::open_default()?;
        let thread = require_legacy_owned(&store, &request.parent_session_id, &request.thread_id)?;
        if subagents::LiveAgentThreads::global().is_live(&request.thread_id) {
            subagents::LiveAgentThreads::global().close(&request.thread_id)?;
        }
        Ok(thread)
    }
}

fn require_legacy_owned(
    store: &AgentThreadStore,
    parent_session_id: &str,
    thread_id: &str,
) -> anyhow::Result<AgentThread> {
    store
        .get(thread_id)?
        .filter(|thread| thread.parent_session_id == parent_session_id)
        .ok_or_else(|| anyhow::anyhow!("unknown historical desktop agent thread: {thread_id}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use providers::types::stream::StreamChunk;
    use providers::CompletionStream;
    use subagents::{AgentGraphStore, Limits};

    fn dispatch(dir: &tempfile::TempDir) -> DefaultAgentThreadDispatch {
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap();
        let control = AgentControl::open(
            "root-session".into(),
            store,
            Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .unwrap();
        DefaultAgentThreadDispatch::for_test(
            control,
            AgentPath::root(),
            "root-session".into(),
            Arc::new(AgentRuntimeManager::default()),
        )
    }

    fn committed_child(
        dispatch: &DefaultAgentThreadDispatch,
        name: &str,
    ) -> subagents::AgentThreadV2 {
        let reservation = dispatch
            .control
            .reserve_spawn(&AgentPath::root(), name)
            .unwrap();
        let thread = reservation.thread().clone();
        reservation.commit().unwrap();
        thread
    }

    fn scripted_chat(reply: &str) -> crate::streaming::ChatOverride {
        let reply = reply.to_string();
        Arc::new(move |_messages, _tools, _config| {
            let reply = reply.clone();
            Box::pin(async move {
                Ok(Box::pin(stream::iter(vec![
                    Ok(StreamChunk::Text(reply)),
                    Ok(StreamChunk::Done {
                        finish_reason: "stop".into(),
                    }),
                ])) as CompletionStream)
            })
        })
    }

    fn pending_chat() -> crate::streaming::ChatOverride {
        Arc::new(move |_messages, _tools, _config| {
            Box::pin(async move {
                Ok(Box::pin(stream::pending::<anyhow::Result<StreamChunk>>()) as CompletionStream)
            })
        })
    }

    fn spawn_request(memory_dir: &Path) -> SpawnAgentDispatchRequest {
        SpawnAgentDispatchRequest {
            request: subagents::SpawnAgentV2Request {
                task_name: "worker".into(),
                message: "do the work".into(),
                agent_type: None,
                model: None,
                reasoning_effort: None,
                fork_turns: Some("none".into()),
            },
            memory_dir: memory_dir.to_path_buf(),
            parent_agent_id: home::DEFAULT_AGENT_ID.into(),
            parent_model: Some("openai:test".into()),
            parent_sandbox_mode: "workspace-write".into(),
            inherited_skill_config: Vec::new(),
            chat_targets: vec![types::ChatTarget {
                provider_id: "test".into(),
                backend_id: "openai".into(),
                model: "test".into(),
                api_key: "test".into(),
                base_url: "http://127.0.0.1.invalid".into(),
            }],
            project_root: None,
            hook_bus: None,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn spawn_commits_only_after_preflight_and_starts_the_turn() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut dispatch = dispatch(&dir);
        dispatch.chat_override = Some(scripted_chat("done"));

        let result = AgentThreadDispatch::spawn_agent(&dispatch, spawn_request(&memory_dir))
            .await
            .unwrap();
        assert_eq!(result.thread.canonical_path.as_str(), "/root/worker");
        assert_eq!(result.thread.agent_type, "default");

        for _ in 0..100 {
            let current = dispatch
                .control
                .list_agents(&AgentPath::root(), Some("worker"))
                .unwrap()
                .into_iter()
                .find(|thread| thread.thread_id == result.thread.thread_id)
                .unwrap();
            if matches!(current.status, AgentStatusV2::Completed { .. }) {
                assert!(!dispatch.runtime_manager.is_running(&current.thread_id));
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("spawned turn did not complete");
    }

    #[tokio::test]
    async fn spawn_preflight_failure_rolls_back_path_row_edge_and_runtime_request() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let dispatch = dispatch(&dir);
        let mut request = spawn_request(&memory_dir);
        request.chat_targets.clear();

        let error = AgentThreadDispatch::spawn_agent(&dispatch, request)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("no chat target"));
        assert_eq!(dispatch.control.identity_count().unwrap(), 0);
        assert_eq!(
            dispatch
                .control
                .list_agents(&AgentPath::root(), None)
                .unwrap()
                .len(),
            1
        );
        let graph = rusqlite::Connection::open(dir.path().join("subagents-v2.db")).unwrap();
        let child_rows: i64 = graph
            .query_row(
                "SELECT COUNT(*) FROM agent_threads WHERE canonical_path <> '/root'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let edge_rows: i64 = graph
            .query_row("SELECT COUNT(*) FROM agent_spawn_edges", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(child_rows, 0);
        assert_eq!(edge_rows, 0);
        assert!(dispatch
            .runtime_requests
            .requests
            .lock()
            .unwrap()
            .is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_admission_failure_rolls_back_committed_pending_spawn() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let graph = AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap();
        let control = AgentControl::open(
            "root-session".into(),
            graph.clone(),
            Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 0,
            },
        )
        .unwrap();
        let mut dispatch = DefaultAgentThreadDispatch::for_test(
            Arc::clone(&control),
            AgentPath::root(),
            "root-session".into(),
            Arc::new(AgentRuntimeManager::default()),
        );
        dispatch.chat_override = Some(pending_chat());

        let error = AgentThreadDispatch::spawn_agent(&dispatch, spawn_request(&memory_dir))
            .await
            .unwrap_err();
        assert!(
            format!("{error:#}").contains("active agent execution limit"),
            "{error:#}"
        );
        assert_eq!(control.identity_count().unwrap(), 0);
        assert_eq!(graph.snapshot("root-session").unwrap().threads.len(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn interrupt_waits_for_runner_ack_and_returns_previous_status() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut dispatch = dispatch(&dir);
        dispatch.chat_override = Some(pending_chat());
        let spawned = AgentThreadDispatch::spawn_agent(&dispatch, spawn_request(&memory_dir))
            .await
            .unwrap();
        while !dispatch
            .runtime_manager
            .is_running(&spawned.thread.thread_id)
        {
            tokio::task::yield_now().await;
        }

        let result = AgentThreadDispatch::interrupt_agent(
            &dispatch,
            InterruptAgentV2Request {
                target: spawned.thread.canonical_path.to_string(),
            },
        )
        .await
        .unwrap();
        assert_eq!(result.previous_status, AgentStatusV2::Running);
        assert_eq!(result.thread.status, AgentStatusV2::Interrupted);
        assert!(!dispatch
            .runtime_manager
            .is_running(&spawned.thread.thread_id));
    }

    #[tokio::test]
    async fn send_message_is_queue_only_and_followup_uses_trigger_semantics() {
        let dir = tempfile::tempdir().unwrap();
        let dispatch = dispatch(&dir);
        let child = committed_child(&dispatch, "worker");

        let sent = AgentThreadDispatch::send_message(
            &dispatch,
            MessageAgentV2Request {
                target: child.canonical_path.to_string(),
                message: "note".into(),
            },
        )
        .await
        .unwrap();
        assert!(sent.queued);
        assert!(!sent.turn_triggered);
        assert!(!dispatch.runtime_manager.is_running(&child.thread_id));

        let followup = AgentThreadDispatch::followup_task(
            &dispatch,
            MessageAgentV2Request {
                target: child.canonical_path.to_string(),
                message: "continue".into(),
            },
        )
        .await
        .unwrap_err();
        assert!(followup.to_string().contains("runtime configuration"));
        let mailbox = dispatch
            .control
            .drain_mailbox(&child.canonical_path)
            .unwrap();
        assert_eq!(mailbox.len(), 2);
        assert!(!mailbox[0].trigger_turn);
        assert!(mailbox[1].trigger_turn);
    }

    #[tokio::test]
    async fn list_defaults_to_entire_root_and_resolves_relative_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let dispatch = dispatch(&dir);
        let parent = committed_child(&dispatch, "parent");
        let reservation = dispatch
            .control
            .reserve_spawn(&parent.canonical_path, "child")
            .unwrap();
        reservation.commit().unwrap();

        let all =
            AgentThreadDispatch::list_agents(&dispatch, ListAgentsV2Request { path_prefix: None })
                .await
                .unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].canonical_path, AgentPath::root());

        let nested = AgentThreadDispatch::list_agents(
            &dispatch,
            ListAgentsV2Request {
                path_prefix: Some("parent".into()),
            },
        )
        .await
        .unwrap();
        assert_eq!(nested.len(), 2);
        assert!(nested
            .iter()
            .all(|thread| thread.canonical_path.starts_with(&parent.canonical_path)));
    }

    #[tokio::test]
    async fn interrupt_rejects_root_and_self_before_runtime_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let dispatch = dispatch(&dir);
        let root_error = AgentThreadDispatch::interrupt_agent(
            &dispatch,
            InterruptAgentV2Request {
                target: "/root".into(),
            },
        )
        .await
        .unwrap_err();
        assert!(root_error.to_string().contains("itself"));
    }

    #[tokio::test]
    async fn wait_result_has_only_message_and_timed_out_fields() {
        let dir = tempfile::tempdir().unwrap();
        let dispatch = dispatch(&dir);
        let (result, ()) = tokio::join!(
            AgentThreadDispatch::wait_agent(
                &dispatch,
                WaitAgentV2Request {
                    timeout_ms: Some(10_000),
                },
            ),
            async {
                tokio::task::yield_now().await;
                AgentThreadDispatch::notify_main_steer(&dispatch);
            }
        );
        let value = serde_json::to_value(result.unwrap()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "message": "The main task received new input.",
                "timed_out": false
            })
        );
    }

    #[test]
    fn custom_layers_preserve_parent_sandbox_and_merge_skills() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join(".codex/agents")).unwrap();
        std::fs::write(
            project.join(".codex/agents/reviewer.toml"),
            r#"name = "reviewer"
description = "review"
developer_instructions = "review carefully"
sandbox_mode = "danger-full-access"

[mcp_servers.docs]
url = "https://example.invalid/mcp"

[[skills.config]]
path = "skills/review/SKILL.md"
enabled = true
"#,
        )
        .unwrap();
        let catalog = subagents::load_agent_catalog(dir.path(), Some(&project));
        let resolved = subagents::resolve_agent(
            &catalog,
            &subagents::AgentsSettings::default(),
            "reviewer",
            None,
            Some("high"),
            Some("openai:gpt-5.6"),
            Some("workspace-write"),
        )
        .unwrap();
        let runtime = build_runtime_request(
            SpawnAgentDispatchRequest {
                request: subagents::SpawnAgentV2Request {
                    task_name: "review".into(),
                    message: "review".into(),
                    agent_type: Some("reviewer".into()),
                    model: None,
                    reasoning_effort: Some("high".into()),
                    fork_turns: None,
                },
                memory_dir: dir.path().to_path_buf(),
                parent_agent_id: "parent-agent".into(),
                parent_model: Some("openai:gpt-5.6".into()),
                parent_sandbox_mode: "workspace-write".into(),
                inherited_skill_config: vec![(PathBuf::from("parent/SKILL.md"), true)],
                chat_targets: Vec::new(),
                project_root: Some(project),
                hook_bus: None,
            },
            resolved,
            &AgentPath::root(),
            "root",
            "root",
            true,
        );
        assert_eq!(runtime.sandbox_mode.as_deref(), Some("workspace-write"));
        assert_eq!(runtime.developer_instructions, "review carefully");
        assert!(runtime.mcp_servers.contains_key("docs"));
        assert_eq!(runtime.skills_config.len(), 2);
        assert_eq!(
            runtime.model_request.model.as_deref(),
            Some("openai:gpt-5.6")
        );
        assert_eq!(
            runtime.model_request.reasoning_effort.as_deref(),
            Some("high")
        );
    }

    #[test]
    fn fork_turns_maps_to_structured_session_store_contract() {
        assert_eq!(parse_fork_turns(None).unwrap(), None);
        assert_eq!(parse_fork_turns(Some("all")).unwrap(), None);
        assert_eq!(parse_fork_turns(Some("none")).unwrap(), Some(0));
        assert_eq!(parse_fork_turns(Some("3")).unwrap(), Some(3));
        assert!(parse_fork_turns(Some("0")).is_err());
        assert!(parse_fork_turns(Some("bad")).is_err());
    }
}
