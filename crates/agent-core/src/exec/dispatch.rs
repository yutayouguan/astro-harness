//! Session-bound Codex V2 Agent Thread dispatch.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use subagents::{
    AgentControl, AgentPath, AgentThread, AgentThreadMessage, AgentThreadStatus, AgentThreadStore,
    CloseAgentRequest, InterruptAgentRequest, InterruptAgentV2Request, InterruptAgentV2Result,
    ListAgentThreadsRequest, ListAgentsV2Request, MessageAgentV2Request, MessageAgentV2Result,
    ReadAgentThreadRequest, SendAgentMessageRequest, SpawnAgentV2Result, SpawnRuntimeV2Request,
    WaitAgentV2Request, WaitAgentV2Result, WaitOutcome,
};
use tools::{AgentThreadDispatch, SpawnAgentDispatchRequest};

use super::agent_runtime::{AgentRuntimeManager, FollowupAdmission, RunAgentTurnRequest};
use crate::runtime::{Config, Session};

#[derive(Clone)]
struct StoredRuntimeRequest {
    runtime: SpawnRuntimeV2Request,
    memory_dir: PathBuf,
}

struct ForkedSessionGuard {
    sessions_dir: PathBuf,
    child_session_id: String,
    parent_session_id: String,
    armed: bool,
}

impl ForkedSessionGuard {
    fn rollback(&mut self) -> anyhow::Result<()> {
        if !self.armed {
            return Ok(());
        }
        let sessions = session::SessionStore::open_sessions_dir(&self.sessions_dir)?;
        let child = sessions
            .get_session(&self.child_session_id)?
            .ok_or_else(|| anyhow::anyhow!("forked child session disappeared before rollback"))?;
        anyhow::ensure!(
            child.parent_session_id.as_deref() == Some(self.parent_session_id.as_str()),
            "refusing to delete child session whose fork ownership changed"
        );
        sessions.delete_session_permanently(&self.child_session_id)?;
        self.armed = false;
        Ok(())
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ForkedSessionGuard {
    fn drop(&mut self) {
        if self.armed {
            if let Err(error) = self.rollback() {
                tracing::warn!(%error, child_session_id = %self.child_session_id, "failed to compensate forked child session");
            }
        }
    }
}

fn rollback_fork_error(
    guard: &mut ForkedSessionGuard,
    operation: &str,
    error: anyhow::Error,
) -> anyhow::Error {
    match guard.rollback() {
        Ok(()) => anyhow::anyhow!("{operation} failed: {error:#}"),
        Err(rollback_error) => anyhow::anyhow!(
            "{operation} failed: {error:#}; child session rollback failed: {rollback_error:#}"
        ),
    }
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

struct SpawnStartupGuard {
    control: Arc<AgentControl>,
    runtime_requests: Arc<RuntimeRequestRegistry>,
    thread: subagents::AgentThreadV2,
    forked_session: Option<ForkedSessionGuard>,
    armed: bool,
}

impl SpawnStartupGuard {
    fn cleanup(&mut self) -> anyhow::Result<()> {
        if !self.armed {
            return Ok(());
        }
        self.runtime_requests.remove(&self.thread.thread_id);
        let graph_result = self.control.abort_committed_pending_spawn(&self.thread);
        let session_result = self
            .forked_session
            .as_mut()
            .map(ForkedSessionGuard::rollback)
            .unwrap_or(Ok(()));
        self.armed = false;
        match (graph_result, session_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(graph), Ok(())) => Err(graph.context("spawn graph rollback")),
            (Ok(()), Err(session)) => Err(session.context("child session rollback")),
            (Err(graph), Err(session)) => Err(anyhow::anyhow!(
                "spawn graph rollback failed: {graph:#}; child session rollback failed: {session:#}"
            )),
        }
    }

    fn disarm(&mut self) {
        if let Some(forked_session) = self.forked_session.as_mut() {
            forked_session.disarm();
        }
        self.armed = false;
    }
}

impl Drop for SpawnStartupGuard {
    fn drop(&mut self) {
        if self.armed {
            if let Err(error) = self.cleanup() {
                // A running status here means the manager owns the second half
                // of the unaccepted-start rollback after observing accept drop.
                tracing::warn!(%error, thread_id = %self.thread.thread_id, "spawn caller cleanup deferred to runtime manager");
            }
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
    #[cfg(test)]
    before_followup_atomic_hook: Option<super::agent_runtime::AckSubscribeHook>,
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
            #[cfg(test)]
            before_followup_atomic_hook: None,
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
            before_followup_atomic_hook: None,
        }
    }

    async fn launch_turn(&self, mut request: RunAgentTurnRequest) -> anyhow::Result<()> {
        let (startup_tx, mut startup_rx) = tokio::sync::watch::channel(None);
        let (startup_accept_tx, startup_accept_rx) = tokio::sync::oneshot::channel();
        request.startup_tx = Some(startup_tx);
        request.startup_accept_rx = Some(startup_accept_rx);
        let run_manager = Arc::clone(&self.runtime_manager);
        let handle = tokio::spawn(async move { run_manager.start_turn(request).await });
        loop {
            if let Some(result) = startup_rx.borrow().clone() {
                result.map_err(anyhow::Error::msg)?;
                startup_accept_tx.send(()).map_err(|_| {
                    anyhow::anyhow!("agent runtime ended before startup acceptance")
                })?;
                return Ok(());
            }
            tokio::select! {
                changed = startup_rx.changed() => {
                    if changed.is_err() {
                        return handle.await
                            .map_err(|error| anyhow::anyhow!("agent runtime task failed to start: {error}"))?;
                    }
                }
            }
        }
    }

    fn run_request(
        &self,
        thread: subagents::AgentThreadV2,
        stored: StoredRuntimeRequest,
        consume_mailbox: bool,
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
            consume_mailbox,
            startup_tx: None,
            startup_accept_rx: None,
            followup_start_tx: None,
            start_token: None,
        }
    }

    async fn wait_for_shared_followup_start(
        mut result_rx: tokio::sync::watch::Receiver<Option<Result<(), String>>>,
    ) -> anyhow::Result<()> {
        loop {
            if let Some(result) = result_rx.borrow().clone() {
                return result.map_err(anyhow::Error::msg);
            }
            result_rx.changed().await.map_err(|_| {
                anyhow::anyhow!("follow-up handoff owner ended before starting the next turn")
            })?;
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

        let mut forked_session = fork_parent_session(
            &memory_dir,
            &runtime,
            &thread.session_id,
            &runtime.model_request.fork_turns,
        )?;
        if let Err(error) = validate_runtime_setup(
            &memory_dir,
            &self.control,
            &thread,
            &runtime,
            &thread.session_id,
        ) {
            return Err(rollback_fork_error(
                &mut forked_session,
                "validate agent runtime setup",
                error,
            ));
        }

        let stored = StoredRuntimeRequest {
            memory_dir,
            runtime,
        };
        if let Err(error) = self
            .runtime_requests
            .insert(&thread.thread_id, stored.clone())
        {
            return Err(rollback_fork_error(
                &mut forked_session,
                "register agent runtime request",
                error,
            ));
        }
        if let Err(error) = reservation.commit() {
            self.runtime_requests.remove(&thread.thread_id);
            return Err(rollback_fork_error(
                &mut forked_session,
                "commit agent thread reservation",
                error,
            ));
        }
        let mut startup_guard = SpawnStartupGuard {
            control: Arc::clone(&self.control),
            runtime_requests: Arc::clone(&self.runtime_requests),
            thread: thread.clone(),
            forked_session: Some(forked_session),
            armed: true,
        };
        if let Err(start_error) = self
            .launch_turn(self.run_request(thread.clone(), stored, false))
            .await
        {
            return match startup_guard.cleanup() {
                Ok(()) => Err(start_error.context("start agent runtime")),
                Err(cleanup_error) => Err(anyhow::anyhow!(
                    "start agent runtime failed: {start_error:#}; spawn cleanup failed: {cleanup_error:#}"
                )),
            };
        }
        startup_guard.disarm();
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
        let followup_text = request.message.trim().to_string();
        #[cfg(test)]
        if let Some(hook) = self.before_followup_atomic_hook.as_ref() {
            // Deliberately model a stale pre-check; the control-layer atomic
            // admission below must re-check after this race window.
            let _ = self
                .control
                .resolve_target(&self.current_path, &request.target)?;
            hook.entered.notify_one();
            hook.release.notified().await;
        }
        let (message, admission) = self.control.enqueue_followup_with_admission(
            &self.current_path,
            request,
            |target| {
                let mut stored =
                    self.runtime_requests
                        .get(&target.thread_id)?
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "agent runtime configuration is unavailable for {}",
                                target.canonical_path
                            )
                        })?;
                stored.runtime.model_request.message = followup_text;
                let run = self.run_request(target.clone(), stored, true);
                self.runtime_manager
                    .request_or_start_followup(&target.thread_id, run)
            },
        )?;
        match admission {
            FollowupAdmission::StartNow { request, result_rx } => {
                let start_tx = request
                    .followup_start_tx
                    .clone()
                    .expect("follow-up start admission owns its result channel");
                let manager = Arc::clone(&self.runtime_manager);
                tokio::spawn(async move {
                    manager.pause_after_followup_admission().await;
                    let start_result = manager.start_turn(*request).await;
                    if start_tx.borrow().is_none() {
                        let shared = start_result
                            .as_ref()
                            .map(|_| ())
                            .map_err(|error| format!("{error:#}"));
                        let _ = start_tx.send(Some(shared));
                    }
                });
                Self::wait_for_shared_followup_start(result_rx).await?
            }
            FollowupAdmission::AwaitStart { result_rx } => {
                self.runtime_manager.pause_after_followup_admission().await;
                Self::wait_for_shared_followup_start(result_rx).await?
            }
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
) -> anyhow::Result<ForkedSessionGuard> {
    let sessions_dir = memory_dir.join("sessions");
    let sessions = session::SessionStore::open_sessions_dir(&sessions_dir)?;
    let recent_turns = parse_fork_turns(fork_turns.as_deref())?;
    sessions.fork_session_recent_turns(
        &runtime.parent_session_id,
        child_session_id,
        recent_turns,
    )?;
    Ok(ForkedSessionGuard {
        sessions_dir,
        child_session_id: child_session_id.to_string(),
        parent_session_id: runtime.parent_session_id.clone(),
        armed: true,
    })
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
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use subagents::{AgentGraphStore, AgentStatusV2, Limits};

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

    fn immediate_error_chat() -> crate::streaming::ChatOverride {
        Arc::new(move |_messages, _tools, _config| {
            Box::pin(async move { anyhow::bail!("injected immediate provider failure") })
        })
    }

    fn capturing_chat(
        captured: Arc<Mutex<Vec<Vec<String>>>>,
        fail_call: Option<usize>,
    ) -> crate::streaming::ChatOverride {
        let calls = Arc::new(AtomicUsize::new(0));
        Arc::new(move |messages, _tools, _config| {
            let call = calls.fetch_add(1, Ordering::SeqCst);
            captured.lock().unwrap().push(
                messages
                    .iter()
                    .flat_map(|message| match message {
                        providers::Message::User { content } => content
                            .iter()
                            .filter_map(|part| match part {
                                providers::types::message::UserContent::Text { text } => {
                                    Some(text.clone())
                                }
                                _ => None,
                            })
                            .collect::<Vec<_>>(),
                        _ => Vec::new(),
                    })
                    .collect(),
            );
            Box::pin(async move {
                if fail_call == Some(call) {
                    anyhow::bail!("injected provider failure after mailbox preparation");
                }
                Ok(Box::pin(stream::iter(vec![
                    Ok(StreamChunk::Text("done".into())),
                    Ok(StreamChunk::Done {
                        finish_reason: "stop".into(),
                    }),
                ])) as CompletionStream)
            })
        })
    }

    fn gated_first_turn_chat(
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    ) -> crate::streaming::ChatOverride {
        let calls = Arc::new(AtomicUsize::new(0));
        Arc::new(move |_messages, _tools, _config| {
            let call = calls.fetch_add(1, Ordering::SeqCst);
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            Box::pin(async move {
                if call == 0 {
                    entered.notify_one();
                    release.notified().await;
                }
                Ok(Box::pin(stream::iter(vec![
                    Ok(StreamChunk::Text(format!("turn-{call}"))),
                    Ok(StreamChunk::Done {
                        finish_reason: "stop".into(),
                    }),
                ])) as CompletionStream)
            })
        })
    }

    fn gated_first_then_pending_chat(
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    ) -> crate::streaming::ChatOverride {
        let calls = Arc::new(AtomicUsize::new(0));
        Arc::new(move |_messages, _tools, _config| {
            let call = calls.fetch_add(1, Ordering::SeqCst);
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            Box::pin(async move {
                if call == 0 {
                    entered.notify_one();
                    release.notified().await;
                    return Ok(Box::pin(stream::iter(vec![
                        Ok(StreamChunk::Text("initial done".into())),
                        Ok(StreamChunk::Done {
                            finish_reason: "stop".into(),
                        }),
                    ])) as CompletionStream);
                }
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
        assert_eq!(
            sessions
                .list_sessions(session::SessionListFilter::Active, 10)
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn spawn_accepts_durable_start_even_when_provider_errors_immediately() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut dispatch = dispatch(&dir);
        dispatch.chat_override = Some(immediate_error_chat());

        let spawned = AgentThreadDispatch::spawn_agent(&dispatch, spawn_request(&memory_dir))
            .await
            .expect("durably started runtime must be accepted independently of completion");

        assert!(dispatch
            .runtime_requests
            .get(&spawned.thread.thread_id)
            .unwrap()
            .is_some());
        assert!(sessions
            .get_session(&spawned.thread.session_id)
            .unwrap()
            .is_some());
        while dispatch
            .runtime_manager
            .is_running(&spawned.thread.thread_id)
        {
            tokio::task::yield_now().await;
        }
        assert!(matches!(
            dispatch
                .control
                .resolve_target(&AgentPath::root(), "worker")
                .unwrap()
                .status,
            AgentStatusV2::Errored { .. }
        ));
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
        assert_eq!(
            sessions
                .list_sessions(session::SessionListFilter::Active, 10)
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn startup_status_failure_removes_forked_session_and_all_spawn_state() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        sessions
            .append_message(session::NewMessage {
                content: Some("parent history"),
                ..session::NewMessage::empty("root-session", "user")
            })
            .unwrap();
        let dispatch = dispatch(&dir);
        dispatch
            .runtime_manager
            .set_start_status_failure(Some("injected startup status failure"));
        let mut request = spawn_request(&memory_dir);
        request.request.fork_turns = Some("all".into());

        let error = AgentThreadDispatch::spawn_agent(&dispatch, request)
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("injected startup status failure"));
        assert_eq!(dispatch.control.identity_count().unwrap(), 0);
        assert!(dispatch
            .runtime_requests
            .requests
            .lock()
            .unwrap()
            .is_empty());
        assert_eq!(
            sessions
                .list_sessions(session::SessionListFilter::Active, 10)
                .unwrap()
                .len(),
            1
        );
        let parent = sessions.get_session("root-session").unwrap().unwrap();
        assert_eq!(parent.parent_session_id, None);
        assert_eq!(parent.message_count, 1);
        assert_eq!(
            sessions.get_messages("root-session").unwrap()[0]
                .content
                .as_deref(),
            Some("parent history")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancelling_spawn_before_startup_acceptance_rolls_back_every_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        sessions
            .append_message(session::NewMessage {
                content: Some("parent history"),
                ..session::NewMessage::empty("root-session", "user")
            })
            .unwrap();
        let mut dispatch = dispatch(&dir);
        dispatch.chat_override = Some(scripted_chat("must not survive cancellation"));
        let hook = super::super::agent_runtime::AckSubscribeHook {
            entered: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
        };
        dispatch
            .runtime_manager
            .set_before_startup_ack_hook(Some(hook.clone()));
        let dispatch = Arc::new(dispatch);
        let spawn = tokio::spawn({
            let dispatch = Arc::clone(&dispatch);
            let memory_dir = memory_dir.clone();
            async move { AgentThreadDispatch::spawn_agent(&*dispatch, spawn_request(&memory_dir)).await }
        });
        hook.entered.notified().await;
        let child = dispatch
            .control
            .list_agents(&AgentPath::root(), Some("worker"))
            .unwrap()
            .into_iter()
            .find(|thread| thread.canonical_path.as_str() == "/root/worker")
            .unwrap();

        spawn.abort();
        assert!(spawn.await.unwrap_err().is_cancelled());
        hook.release.notify_one();
        tokio::time::timeout(Duration::from_secs(2), async {
            while dispatch.runtime_manager.is_running(&child.thread_id) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

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
        assert!(dispatch
            .control
            .runtime_handle(&child.thread_id)
            .unwrap()
            .is_none());
        assert!(sessions.get_session(&child.session_id).unwrap().is_none());
        assert_eq!(sessions.get_messages("root-session").unwrap().len(), 1);

        let retried = AgentThreadDispatch::spawn_agent(&*dispatch, spawn_request(&memory_dir))
            .await
            .unwrap();
        assert_eq!(retried.thread.canonical_path.as_str(), "/root/worker");
        while dispatch
            .runtime_manager
            .is_running(&retried.thread.thread_id)
        {
            tokio::task::yield_now().await;
        }
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

    #[tokio::test(flavor = "current_thread")]
    async fn followup_retry_delivers_old_marker_and_new_message_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let captured = Arc::new(Mutex::new(Vec::new()));
        let mut dispatch = dispatch(&dir);
        dispatch.chat_override = Some(capturing_chat(Arc::clone(&captured), Some(1)));
        let spawned = AgentThreadDispatch::spawn_agent(&dispatch, spawn_request(&memory_dir))
            .await
            .unwrap();
        while dispatch
            .runtime_manager
            .is_running(&spawned.thread.thread_id)
        {
            tokio::task::yield_now().await;
        }

        let graph = rusqlite::Connection::open(dir.path().join("subagents-v2.db")).unwrap();
        graph
            .execute_batch(
                "CREATE TRIGGER fail_first_followup_ack
                 BEFORE UPDATE OF delivery_state ON agent_mailbox
                 WHEN NEW.delivery_state = 'delivered'
                 BEGIN
                   SELECT RAISE(ABORT, 'injected first followup ack failure');
                 END;",
            )
            .unwrap();
        let first = AgentThreadDispatch::followup_task(
            &dispatch,
            MessageAgentV2Request {
                target: spawned.thread.canonical_path.to_string(),
                message: "S1".into(),
            },
        )
        .await
        .unwrap_err();
        assert!(format!("{first:#}").contains("injected first followup ack failure"));
        graph
            .execute_batch("DROP TRIGGER fail_first_followup_ack;")
            .unwrap();

        AgentThreadDispatch::followup_task(
            &dispatch,
            MessageAgentV2Request {
                target: spawned.thread.canonical_path.to_string(),
                message: "S2".into(),
            },
        )
        .await
        .unwrap();
        while dispatch
            .runtime_manager
            .is_running(&spawned.thread.thread_id)
        {
            tokio::task::yield_now().await;
        }
        assert!(matches!(
            dispatch
                .control
                .resolve_target(&AgentPath::root(), "worker")
                .unwrap()
                .status,
            AgentStatusV2::Errored { ref message }
                if message.contains("injected provider failure")
        ));

        assert!(dispatch
            .control
            .drain_mailbox(&spawned.thread.canonical_path)
            .unwrap()
            .is_empty());
        let child_users = sessions
            .get_messages(&spawned.thread.session_id)
            .unwrap()
            .into_iter()
            .filter(|message| message.role == "user")
            .filter_map(|message| message.content)
            .collect::<Vec<_>>();
        assert_eq!(
            child_users
                .iter()
                .filter(|content| content.as_str() == "S1")
                .count(),
            1
        );
        assert_eq!(
            child_users
                .iter()
                .filter(|content| content.as_str() == "S2")
                .count(),
            1
        );
        AgentThreadDispatch::followup_task(
            &dispatch,
            MessageAgentV2Request {
                target: spawned.thread.canonical_path.to_string(),
                message: "S3".into(),
            },
        )
        .await
        .unwrap();
        while dispatch
            .runtime_manager
            .is_running(&spawned.thread.thread_id)
        {
            tokio::task::yield_now().await;
        }
        let captured = captured.lock().unwrap();
        let sampled = captured.last().unwrap();
        assert_eq!(
            sampled
                .iter()
                .filter(|content| content.as_str() == "S1")
                .count(),
            1
        );
        assert_eq!(
            sampled
                .iter()
                .filter(|content| content.as_str() == "S2")
                .count(),
            1
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn followup_at_terminal_cleanup_is_handed_off_to_a_new_turn() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut dispatch = dispatch(&dir);
        let entered_sampling = Arc::new(tokio::sync::Notify::new());
        let release_sampling = Arc::new(tokio::sync::Notify::new());
        dispatch.chat_override = Some(gated_first_turn_chat(
            Arc::clone(&entered_sampling),
            Arc::clone(&release_sampling),
        ));
        let dispatch = Arc::new(dispatch);

        let spawned = AgentThreadDispatch::spawn_agent(&*dispatch, spawn_request(&memory_dir))
            .await
            .unwrap();
        entered_sampling.notified().await;

        let cleanup_entered = Arc::new(AtomicBool::new(false));
        let cleanup_entered_for_hook = Arc::clone(&cleanup_entered);
        let (at_cleanup_tx, at_cleanup_rx) = std::sync::mpsc::sync_channel(1);
        let (release_cleanup_tx, release_cleanup_rx) = std::sync::mpsc::sync_channel(1);
        let release_cleanup_rx = Arc::new(Mutex::new(release_cleanup_rx));
        dispatch
            .runtime_manager
            .set_before_cleanup_hook(Some(Arc::new(move || {
                if !cleanup_entered_for_hook.swap(true, Ordering::SeqCst) {
                    at_cleanup_tx.send(()).unwrap();
                    release_cleanup_rx.lock().unwrap().recv().unwrap();
                }
            })));
        release_sampling.notify_one();
        tokio::task::spawn_blocking(move || at_cleanup_rx.recv().unwrap())
            .await
            .unwrap();

        let followup_dispatch = Arc::clone(&dispatch);
        let target = spawned.thread.canonical_path.to_string();
        let followup = tokio::spawn(async move {
            AgentThreadDispatch::followup_task(
                &*followup_dispatch,
                MessageAgentV2Request {
                    target,
                    message: "continue after cleanup".into(),
                },
            )
            .await
        });
        let joined_dispatch = Arc::clone(&dispatch);
        let joined_target = spawned.thread.canonical_path.to_string();
        let joined_followup = tokio::spawn(async move {
            AgentThreadDispatch::followup_task(
                &*joined_dispatch,
                MessageAgentV2Request {
                    target: joined_target,
                    message: "also continue after cleanup".into(),
                },
            )
            .await
        });
        let queued = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let queued = dispatch
                    .control
                    .drain_mailbox(&spawned.thread.canonical_path)
                    .unwrap();
                if queued.len() == 2 {
                    break queued;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let expected_combined_input = queued
            .iter()
            .map(|message| message.payload.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        release_cleanup_tx.send(()).unwrap();
        followup.await.unwrap().unwrap();
        joined_followup.await.unwrap().unwrap();

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let starts = dispatch
                    .control
                    .status_events(&spawned.thread.thread_id)
                    .unwrap()
                    .into_iter()
                    .filter(|event| {
                        matches!(event.event, subagents::RunnerEvent::TurnStarted { .. })
                    })
                    .count();
                if starts == 2 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("durable followup must start a second turn after cleanup handoff");
        assert!(dispatch
            .control
            .drain_mailbox(&spawned.thread.canonical_path)
            .unwrap()
            .is_empty());
        assert!(sessions
            .get_messages(&spawned.thread.session_id)
            .unwrap()
            .iter()
            .any(|message| message.content.as_deref() == Some(expected_combined_input.as_str())));
        dispatch.runtime_manager.set_before_cleanup_hook(None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn idle_followup_reports_runtime_start_failure_instead_of_triggered_success() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        drop(sessions);
        let mut dispatch = dispatch(&dir);
        dispatch.chat_override = Some(scripted_chat("done"));
        let spawned = AgentThreadDispatch::spawn_agent(&dispatch, spawn_request(&memory_dir))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while dispatch
                .runtime_manager
                .is_running(&spawned.thread.thread_id)
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        std::fs::remove_dir_all(&memory_dir).unwrap();
        std::fs::write(&memory_dir, "not a runtime directory").unwrap();
        let dispatch = Arc::new(dispatch);
        let admission = Arc::new(tokio::sync::Barrier::new(3));
        dispatch
            .runtime_manager
            .set_followup_admission_barrier(Some(Arc::clone(&admission)));
        let first_dispatch = Arc::clone(&dispatch);
        let first_target = spawned.thread.canonical_path.to_string();
        let first = tokio::spawn(async move {
            AgentThreadDispatch::followup_task(
                &*first_dispatch,
                MessageAgentV2Request {
                    target: first_target,
                    message: "cannot start one".into(),
                },
            )
            .await
        });
        let second_dispatch = Arc::clone(&dispatch);
        let second_target = spawned.thread.canonical_path.to_string();
        let second = tokio::spawn(async move {
            AgentThreadDispatch::followup_task(
                &*second_dispatch,
                MessageAgentV2Request {
                    target: second_target,
                    message: "cannot start two".into(),
                },
            )
            .await
        });
        admission.wait().await;
        dispatch
            .runtime_manager
            .set_followup_admission_barrier(None);
        let (first, second) = tokio::join!(first, second);
        let error = first.unwrap().unwrap_err();
        let joined_error = second.unwrap().unwrap_err();
        assert_eq!(format!("{error:#}"), format!("{joined_error:#}"));
        assert!(
            format!("{error:#}").contains("Not a directory")
                || format!("{error:#}").contains("not a directory")
                || format!("{error:#}").contains("File exists"),
            "{error:#}"
        );
        assert!(!dispatch
            .runtime_manager
            .is_running(&spawned.thread.thread_id));
        assert_eq!(
            dispatch
                .control
                .drain_mailbox(&spawned.thread.canonical_path)
                .unwrap()
                .len(),
            2,
            "failed start must leave durable follow-up retryable"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_idle_followups_share_one_starting_generation() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut dispatch = dispatch(&dir);
        dispatch.chat_override = Some(scripted_chat("done"));
        let dispatch = Arc::new(dispatch);
        let spawned = AgentThreadDispatch::spawn_agent(&*dispatch, spawn_request(&memory_dir))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while dispatch
                .runtime_manager
                .is_running(&spawned.thread.thread_id)
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        let admission = Arc::new(tokio::sync::Barrier::new(3));
        dispatch
            .runtime_manager
            .set_followup_admission_barrier(Some(Arc::clone(&admission)));
        let first_dispatch = Arc::clone(&dispatch);
        let first_target = spawned.thread.canonical_path.to_string();
        let first = tokio::spawn(async move {
            AgentThreadDispatch::followup_task(
                &*first_dispatch,
                MessageAgentV2Request {
                    target: first_target,
                    message: "idle one".into(),
                },
            )
            .await
        });
        let second_dispatch = Arc::clone(&dispatch);
        let second_target = spawned.thread.canonical_path.to_string();
        let second = tokio::spawn(async move {
            AgentThreadDispatch::followup_task(
                &*second_dispatch,
                MessageAgentV2Request {
                    target: second_target,
                    message: "idle two".into(),
                },
            )
            .await
        });
        admission.wait().await;
        dispatch
            .runtime_manager
            .set_followup_admission_barrier(None);
        let (first, second) = tokio::join!(first, second);
        first.unwrap().unwrap();
        second.unwrap().unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while dispatch
                .runtime_manager
                .is_running(&spawned.thread.thread_id)
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let starts = dispatch
            .control
            .status_events(&spawned.thread.thread_id)
            .unwrap()
            .into_iter()
            .filter(|event| matches!(event.event, subagents::RunnerEvent::TurnStarted { .. }))
            .count();
        assert_eq!(starts, 2);
        assert!(dispatch
            .control
            .drain_mailbox(&spawned.thread.canonical_path)
            .unwrap()
            .is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn post_claim_setup_failure_is_shared_cleaned_and_retryable() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut dispatch = dispatch(&dir);
        dispatch.chat_override = Some(scripted_chat("done"));
        let dispatch = Arc::new(dispatch);
        let spawned = AgentThreadDispatch::spawn_agent(&*dispatch, spawn_request(&memory_dir))
            .await
            .unwrap();
        while dispatch
            .runtime_manager
            .is_running(&spawned.thread.thread_id)
        {
            tokio::task::yield_now().await;
        }
        dispatch
            .runtime_manager
            .set_start_status_failure(Some("injected status_events failure"));
        let barrier = Arc::new(tokio::sync::Barrier::new(3));
        dispatch
            .runtime_manager
            .set_followup_admission_barrier(Some(Arc::clone(&barrier)));
        let make = |message: &'static str| {
            let dispatch = Arc::clone(&dispatch);
            let target = spawned.thread.canonical_path.to_string();
            tokio::spawn(async move {
                AgentThreadDispatch::followup_task(
                    &*dispatch,
                    MessageAgentV2Request {
                        target,
                        message: message.into(),
                    },
                )
                .await
            })
        };
        let first = make("first retained");
        let second = make("second retained");
        barrier.wait().await;
        dispatch
            .runtime_manager
            .set_followup_admission_barrier(None);
        let (first, second) = tokio::join!(first, second);
        let first = first.unwrap().unwrap_err();
        let second = second.unwrap().unwrap_err();
        assert_eq!(format!("{first:#}"), format!("{second:#}"));
        assert!(format!("{first:#}").contains("injected status_events failure"));
        assert!(!dispatch
            .runtime_manager
            .is_running(&spawned.thread.thread_id));
        assert!(dispatch
            .control
            .runtime_handle(&spawned.thread.thread_id)
            .unwrap()
            .is_none());
        assert_eq!(
            dispatch
                .control
                .drain_mailbox(&spawned.thread.canonical_path)
                .unwrap()
                .len(),
            2
        );
        dispatch.runtime_manager.set_start_status_failure(None);
        AgentThreadDispatch::followup_task(
            &*dispatch,
            MessageAgentV2Request {
                target: spawned.thread.canonical_path.to_string(),
                message: "retry".into(),
            },
        )
        .await
        .unwrap();
        while dispatch
            .runtime_manager
            .is_running(&spawned.thread.thread_id)
        {
            tokio::task::yield_now().await;
        }
        assert!(dispatch
            .control
            .drain_mailbox(&spawned.thread.canonical_path)
            .unwrap()
            .is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn canceling_idle_followup_caller_does_not_cancel_manager_owned_start() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut dispatch = dispatch(&dir);
        dispatch.chat_override = Some(scripted_chat("done"));
        let dispatch = Arc::new(dispatch);
        let spawned = AgentThreadDispatch::spawn_agent(&*dispatch, spawn_request(&memory_dir))
            .await
            .unwrap();
        while dispatch
            .runtime_manager
            .is_running(&spawned.thread.thread_id)
        {
            tokio::task::yield_now().await;
        }

        let start_entered = Arc::new(tokio::sync::Notify::new());
        let start_release = Arc::new(tokio::sync::Notify::new());
        dispatch
            .runtime_manager
            .set_before_followup_start_hook(Some(crate::exec::agent_runtime::AckSubscribeHook {
                entered: Arc::clone(&start_entered),
                release: Arc::clone(&start_release),
            }));
        let caller_dispatch = Arc::clone(&dispatch);
        let target = spawned.thread.canonical_path.to_string();
        let caller = tokio::spawn(async move {
            AgentThreadDispatch::followup_task(
                &*caller_dispatch,
                MessageAgentV2Request {
                    target,
                    message: "survive caller cancellation".into(),
                },
            )
            .await
        });
        start_entered.notified().await;
        caller.abort();
        start_release.notify_one();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let starts = dispatch
                    .control
                    .status_events(&spawned.thread.thread_id)
                    .unwrap()
                    .into_iter()
                    .filter(|event| {
                        matches!(event.event, subagents::RunnerEvent::TurnStarted { .. })
                    })
                    .count();
                if starts == 2
                    && !dispatch
                        .runtime_manager
                        .is_running(&spawned.thread.thread_id)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(dispatch
            .control
            .drain_mailbox(&spawned.thread.canonical_path)
            .unwrap()
            .is_empty());
        dispatch
            .runtime_manager
            .set_before_followup_start_hook(None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn terminal_handoff_keeps_starting_slot_visible_to_third_followup() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut dispatch = dispatch(&dir);
        let entered_sampling = Arc::new(tokio::sync::Notify::new());
        let release_sampling = Arc::new(tokio::sync::Notify::new());
        dispatch.chat_override = Some(gated_first_then_pending_chat(
            Arc::clone(&entered_sampling),
            Arc::clone(&release_sampling),
        ));
        let dispatch = Arc::new(dispatch);
        let spawned = AgentThreadDispatch::spawn_agent(&*dispatch, spawn_request(&memory_dir))
            .await
            .unwrap();
        entered_sampling.notified().await;

        let cleanup_once = Arc::new(AtomicBool::new(false));
        let cleanup_once_for_hook = Arc::clone(&cleanup_once);
        let (at_cleanup_tx, at_cleanup_rx) = std::sync::mpsc::sync_channel(1);
        let (release_cleanup_tx, release_cleanup_rx) = std::sync::mpsc::sync_channel(1);
        let release_cleanup_rx = Arc::new(Mutex::new(release_cleanup_rx));
        dispatch
            .runtime_manager
            .set_before_cleanup_hook(Some(Arc::new(move || {
                if !cleanup_once_for_hook.swap(true, Ordering::SeqCst) {
                    at_cleanup_tx.send(()).unwrap();
                    release_cleanup_rx.lock().unwrap().recv().unwrap();
                }
            })));
        release_sampling.notify_one();
        tokio::task::spawn_blocking(move || at_cleanup_rx.recv().unwrap())
            .await
            .unwrap();

        let second_dispatch = Arc::clone(&dispatch);
        let second_target = spawned.thread.canonical_path.to_string();
        let second = tokio::spawn(async move {
            AgentThreadDispatch::followup_task(
                &*second_dispatch,
                MessageAgentV2Request {
                    target: second_target,
                    message: "handoff owner".into(),
                },
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            while dispatch
                .control
                .drain_mailbox(&spawned.thread.canonical_path)
                .unwrap()
                .len()
                != 1
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        let start_entered = Arc::new(tokio::sync::Notify::new());
        let start_release = Arc::new(tokio::sync::Notify::new());
        dispatch
            .runtime_manager
            .set_before_followup_start_hook(Some(crate::exec::agent_runtime::AckSubscribeHook {
                entered: Arc::clone(&start_entered),
                release: Arc::clone(&start_release),
            }));
        release_cleanup_tx.send(()).unwrap();
        start_entered.notified().await;

        let third_dispatch = Arc::clone(&dispatch);
        let third_target = spawned.thread.canonical_path.to_string();
        let third = tokio::spawn(async move {
            AgentThreadDispatch::followup_task(
                &*third_dispatch,
                MessageAgentV2Request {
                    target: third_target,
                    message: "handoff joiner".into(),
                },
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            while dispatch
                .control
                .drain_mailbox(&spawned.thread.canonical_path)
                .unwrap()
                .len()
                != 2
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        start_release.notify_one();
        let (second, third) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(second, third)
        })
        .await
        .unwrap();
        second.unwrap().unwrap();
        third.unwrap().unwrap();
        let starts = dispatch
            .control
            .status_events(&spawned.thread.thread_id)
            .unwrap()
            .into_iter()
            .filter(|event| matches!(event.event, subagents::RunnerEvent::TurnStarted { .. }))
            .count();
        assert_eq!(starts, 2);
        AgentThreadDispatch::interrupt_agent(
            &*dispatch,
            InterruptAgentV2Request {
                target: spawned.thread.canonical_path.to_string(),
            },
        )
        .await
        .unwrap();
        dispatch.runtime_manager.set_before_cleanup_hook(None);
        dispatch
            .runtime_manager
            .set_before_followup_start_hook(None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_rejects_pending_followup_without_starting_a_new_generation() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut dispatch = dispatch(&dir);
        dispatch.chat_override = Some(pending_chat());
        let dispatch = Arc::new(dispatch);
        let spawned = AgentThreadDispatch::spawn_agent(&*dispatch, spawn_request(&memory_dir))
            .await
            .unwrap();

        let followup_dispatch = Arc::clone(&dispatch);
        let target = spawned.thread.canonical_path.to_string();
        let followup = tokio::spawn(async move {
            AgentThreadDispatch::followup_task(
                &*followup_dispatch,
                MessageAgentV2Request {
                    target,
                    message: "must not run after shutdown".into(),
                },
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            while dispatch
                .control
                .drain_mailbox(&spawned.thread.canonical_path)
                .unwrap()
                .is_empty()
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        dispatch
            .runtime_manager
            .terminate(&spawned.thread.thread_id)
            .await
            .unwrap();
        let error = followup.await.unwrap().unwrap_err();
        assert!(format!("{error:#}").contains("shutdown"));
        let starts = dispatch
            .control
            .status_events(&spawned.thread.thread_id)
            .unwrap()
            .into_iter()
            .filter(|event| matches!(event.event, subagents::RunnerEvent::TurnStarted { .. }))
            .count();
        assert_eq!(starts, 1);
        assert!(!dispatch
            .runtime_manager
            .is_running(&spawned.thread.thread_id));
        assert_eq!(
            dispatch
                .control
                .drain_mailbox(&spawned.thread.canonical_path)
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn followup_rechecks_shutdown_atomically_before_enqueue_and_admission() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut dispatch = dispatch(&dir);
        dispatch.chat_override = Some(pending_chat());
        let checked = Arc::new(tokio::sync::Notify::new());
        let resume = Arc::new(tokio::sync::Notify::new());
        dispatch.before_followup_atomic_hook = Some(crate::exec::agent_runtime::AckSubscribeHook {
            entered: Arc::clone(&checked),
            release: Arc::clone(&resume),
        });
        let dispatch = Arc::new(dispatch);
        let spawned = AgentThreadDispatch::spawn_agent(&*dispatch, spawn_request(&memory_dir))
            .await
            .unwrap();
        let followup_dispatch = Arc::clone(&dispatch);
        let target = spawned.thread.canonical_path.to_string();
        let followup = tokio::spawn(async move {
            AgentThreadDispatch::followup_task(
                &*followup_dispatch,
                MessageAgentV2Request {
                    target,
                    message: "racing followup".into(),
                },
            )
            .await
        });
        checked.notified().await;
        dispatch
            .runtime_manager
            .terminate(&spawned.thread.thread_id)
            .await
            .unwrap();
        resume.notify_one();
        let error = followup.await.unwrap().unwrap_err();
        assert!(format!("{error:#}").contains("Shutdown"));
        assert!(dispatch
            .control
            .drain_mailbox(&spawned.thread.canonical_path)
            .unwrap()
            .is_empty());
        let events = dispatch
            .control
            .status_events(&spawned.thread.thread_id)
            .unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event.event, subagents::RunnerEvent::TurnStarted { .. }))
                .count(),
            1
        );
        assert!(!events
            .iter()
            .any(|event| matches!(event.event, subagents::RunnerEvent::TurnErrored { .. })));
        assert_eq!(
            dispatch
                .control
                .resolve_target(&AgentPath::root(), spawned.thread.canonical_path.as_ref())
                .unwrap()
                .status,
            AgentStatusV2::Shutdown
        );
        assert!(dispatch
            .control
            .runtime_handle(&spawned.thread.thread_id)
            .unwrap()
            .is_none());
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
    fn custom_layers_preserve_unknown_parent_sandbox_and_merge_skills() {
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
            Some("locked"),
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
                parent_sandbox_mode: "locked".into(),
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
        assert_eq!(runtime.sandbox_mode.as_deref(), Some("locked"));
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
