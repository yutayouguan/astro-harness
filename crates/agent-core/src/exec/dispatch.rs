//! 绑定 Session 的 Codex V2 Agent Thread 分发。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use anyhow::Context;
use async_trait::async_trait;
use subagents::{
    ActivityCursor, AgentControl, AgentPath, AgentThreadDetailV2, AgentThreadMessageV2,
    AgentThreadV2, AgentTreeSnapshotV2, InterruptAgentV2Request, InterruptAgentV2Result,
    ListAgentsV2Request, MessageAgentV2Request, MessageAgentV2Result, SpawnAgentV2Result,
    SpawnRuntimeV2Request, WaitAgentV2Request, WaitAgentV2Result, WaitOutcome,
};
use tools::{
    AgentThreadDispatch, FollowupAgentDispatchRequest, ParentRuntimeMaterial,
    SpawnAgentDispatchRequest,
};

use super::agent_runtime::{
    resolve_chat_targets_for_model, AgentRuntimeManager, CloseThreadStart, FollowupAdmission,
    RunAgentTurnRequest, UnacceptedSpawnCleanup,
};
use crate::runtime::{Config, Session};

type ActiveRootSession = Session;

#[derive(Default)]
struct ActiveRootSessionRegistry {
    sessions: Mutex<HashMap<(PathBuf, String), Weak<ActiveRootSession>>>,
}

fn active_root_sessions() -> &'static ActiveRootSessionRegistry {
    static REGISTRY: OnceLock<ActiveRootSessionRegistry> = OnceLock::new();
    REGISTRY.get_or_init(ActiveRootSessionRegistry::default)
}

pub fn register_active_root_session(
    memory_dir: &Path,
    root_session_id: &str,
    session: &Arc<ActiveRootSession>,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !root_session_id.trim().is_empty(),
        "root session id must not be empty"
    );
    active_root_sessions()
        .sessions
        .lock()
        .map_err(|_| anyhow::anyhow!("active root session registry mutex is poisoned"))?
        .insert(
            (memory_dir.to_path_buf(), root_session_id.to_string()),
            Arc::downgrade(session),
        );
    Ok(())
}

pub fn unregister_active_root_session(
    memory_dir: &Path,
    root_session_id: &str,
    session: &Arc<ActiveRootSession>,
) {
    let Ok(mut sessions) = active_root_sessions().sessions.lock() else {
        return;
    };
    let key = (memory_dir.to_path_buf(), root_session_id.to_string());
    if sessions
        .get(&key)
        .is_some_and(|registered| Weak::ptr_eq(registered, &Arc::downgrade(session)))
    {
        sessions.remove(&key);
    }
}

pub async fn active_root_runtime_material(
    memory_dir: &Path,
    root_session_id: &str,
) -> anyhow::Result<Option<ParentRuntimeMaterial>> {
    active_root_runtime_material_with_upgrade_hook(memory_dir, root_session_id, || {}).await
}

/// 解析活跃的根 Session，用于仅桌面端的生命周期操作（如手动压缩）。
/// 注册表存储 Weak 引用，因此不会延长 Session 的生命周期。
pub fn active_root_session_for_hooks(
    memory_dir: &Path,
    root_session_id: &str,
) -> anyhow::Result<Option<Arc<Session>>> {
    let key = (memory_dir.to_path_buf(), root_session_id.to_string());
    let mut sessions = active_root_sessions()
        .sessions
        .lock()
        .map_err(|_| anyhow::anyhow!("active root session registry mutex is poisoned"))?;
    let session = sessions.get(&key).and_then(Weak::upgrade);
    if session.is_none() {
        sessions.remove(&key);
    }
    Ok(session)
}

async fn active_root_runtime_material_with_upgrade_hook(
    memory_dir: &Path,
    root_session_id: &str,
    mut after_upgrade: impl FnMut(),
) -> anyhow::Result<Option<ParentRuntimeMaterial>> {
    let key = (memory_dir.to_path_buf(), root_session_id.to_string());
    loop {
        let session = {
            let mut sessions = active_root_sessions()
                .sessions
                .lock()
                .map_err(|_| anyhow::anyhow!("active root session registry mutex is poisoned"))?;
            let Some(session) = sessions.get(&key).and_then(Weak::upgrade) else {
                sessions.remove(&key);
                return Ok(None);
            };
            session
        };
        after_upgrade();
        tokio::task::yield_now().await;
        let parent_model = session
            .chat_targets()
            .first()
            .map(|target| format!("{}:{}", target.backend_id.trim(), target.model.trim()));
        let material = ParentRuntimeMaterial {
            memory_dir: session.memory_dir().to_path_buf(),
            parent_agent_id: session.agent_id().to_string(),
            parent_model,
            parent_sandbox_mode: session
                .permission_profile()
                .unwrap_or_else(|| types::WORKSPACE_PROFILE.to_string()),
            inherited_skill_config: session.skill_config_overrides(),
            chat_targets: session.chat_targets(),
            project_root: session.project_root(),
            workspace_roots: session.workspace_roots(),
            hook_runtime: Some(session.hook_runtime()),
            hook_bus: Some(session.hook_bus()),
        };
        let remains_current = active_root_sessions()
            .sessions
            .lock()
            .map_err(|_| anyhow::anyhow!("active root session registry mutex is poisoned"))?
            .get(&key)
            .is_some_and(|registered| Weak::ptr_eq(registered, &Arc::downgrade(&session)));
        if !remains_current {
            continue;
        }
        return Ok(Some(material));
    }
}

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
    async fn rollback(&mut self) -> anyhow::Result<()> {
        if !self.armed {
            return Ok(());
        }
        let sessions = session::SessionStore::open_sessions_dir(&self.sessions_dir).await?;
        let Some(child) = sessions.get_session(&self.child_session_id).await? else {
            self.armed = false;
            return Ok(());
        };
        anyhow::ensure!(
            child.parent_session_id.as_deref() == Some(self.parent_session_id.as_str()),
            "refusing to delete child session whose fork ownership changed"
        );
        sessions.delete_session_permanently(&self.child_session_id).await?;
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
            self.armed = false;
            let sessions_dir = self.sessions_dir.clone();
            let child_session_id = self.child_session_id.clone();
            let parent_session_id = self.parent_session_id.clone();
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let result: anyhow::Result<()> = async {
                        let sessions =
                            session::SessionStore::open_sessions_dir(&sessions_dir).await?;
                        let Some(child) = sessions.get_session(&child_session_id).await? else {
                            return Ok(());
                        };
                        if child.parent_session_id.as_deref()
                            != Some(parent_session_id.as_str())
                        {
                            return Ok(());
                        }
                        sessions
                            .delete_session_permanently(&child_session_id)
                            .await?;
                        Ok(())
                    }
                    .await;
                    if let Err(error) = result {
                        tracing::warn!(%error, %child_session_id, "failed to compensate forked child session");
                    }
                });
            }
        }
    }
}

async fn rollback_fork_error(
    guard: &mut ForkedSessionGuard,
    operation: &str,
    error: anyhow::Error,
) -> anyhow::Error {
    match guard.rollback().await {
        Ok(()) => anyhow::anyhow!("{operation} failed: {error:#}"),
        Err(rollback_error) => anyhow::anyhow!(
            "{operation} failed: {error:#}; child session rollback failed: {rollback_error:#}"
        ),
    }
}

#[derive(Default)]
struct RuntimeRequestRegistry {
    requests: Mutex<HashMap<String, Arc<StoredRuntimeRequest>>>,
}

impl RuntimeRequestRegistry {
    fn global() -> Arc<Self> {
        static REGISTRY: OnceLock<Arc<RuntimeRequestRegistry>> = OnceLock::new();
        Arc::clone(REGISTRY.get_or_init(|| Arc::new(Self::default())))
    }

    fn insert(&self, thread_id: &str, request: Arc<StoredRuntimeRequest>) -> anyhow::Result<()> {
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

    fn get(&self, thread_id: &str) -> anyhow::Result<Option<Arc<StoredRuntimeRequest>>> {
        Ok(self
            .requests
            .lock()
            .map_err(|_| anyhow::anyhow!("agent runtime request registry mutex is poisoned"))?
            .get(thread_id)
            .cloned())
    }

    fn get_or_insert(
        &self,
        thread_id: &str,
        candidate: Arc<StoredRuntimeRequest>,
    ) -> anyhow::Result<Arc<StoredRuntimeRequest>> {
        let mut requests = self
            .requests
            .lock()
            .map_err(|_| anyhow::anyhow!("agent runtime request registry mutex is poisoned"))?;
        Ok(requests
            .entry(thread_id.to_string())
            .or_insert(candidate)
            .clone())
    }

    fn take(&self, thread_id: &str) -> anyhow::Result<Option<Arc<StoredRuntimeRequest>>> {
        Ok(self
            .requests
            .lock()
            .map_err(|_| anyhow::anyhow!("agent runtime request registry mutex is poisoned"))?
            .remove(thread_id))
    }

    fn remove(&self, thread_id: &str) {
        if let Ok(mut requests) = self.requests.lock() {
            requests.remove(thread_id);
        }
    }
}

struct SpawnStartupGuard {
    cleanup: Arc<UnacceptedSpawnCleanup>,
    armed: bool,
}

impl SpawnStartupGuard {
    fn cleanup(&mut self) -> anyhow::Result<()> {
        if !self.armed {
            return Ok(());
        }
        let result = self.cleanup.run();
        self.armed = false;
        result
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for SpawnStartupGuard {
    fn drop(&mut self) {
        if self.armed {
            if let Err(error) = self.cleanup() {
                // 此处 running 状态意味着 manager 在观察到 accept drop 后
                // 拥有未接受启动回滚的后半部分。
                tracing::warn!(%error, "spawn caller cleanup deferred to runtime manager");
            }
        }
    }
}

/// 绑定到当前单个 Agent Thread session 的分发器。
pub struct DefaultAgentThreadDispatch {
    control: Arc<AgentControl>,
    current_path: AgentPath,
    current_thread_id: String,
    runtime_manager: Arc<AgentRuntimeManager>,
    runtime_requests: Arc<RuntimeRequestRegistry>,
    wait_cursor: Arc<AtomicU64>,
    #[cfg(any(test, feature = "test-support"))]
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
        let wait_cursor = control.activity_cursor();
        Self {
            control,
            current_path,
            current_thread_id,
            runtime_manager: AgentRuntimeManager::global(),
            runtime_requests: RuntimeRequestRegistry::global(),
            wait_cursor: Arc::new(AtomicU64::new(wait_cursor.0)),
            #[cfg(any(test, feature = "test-support"))]
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
        let wait_cursor = control.activity_cursor();
        Self {
            control,
            current_path,
            current_thread_id,
            runtime_manager,
            runtime_requests: Arc::new(RuntimeRequestRegistry::default()),
            wait_cursor: Arc::new(AtomicU64::new(wait_cursor.0)),
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
            #[cfg(any(test, feature = "test-support"))]
            chat_override: self.chat_override.clone(),
            #[cfg(not(any(test, feature = "test-support")))]
            chat_override: None,
            consume_mailbox,
            startup_tx: None,
            startup_accept_rx: None,
            unaccepted_spawn_cleanup: None,
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

    async fn recover_runtime_request(
        &self,
        target: &AgentThreadV2,
        material: Option<&ParentRuntimeMaterial>,
    ) -> anyhow::Result<Arc<StoredRuntimeRequest>> {
        anyhow::ensure!(
            self.current_path == AgentPath::root()
                || target.canonical_path.starts_with(&self.current_path),
            "cold Agent Thread recovery must be initiated by the root or an ancestor of the target"
        );
        let material = material.ok_or_else(|| {
            anyhow::anyhow!(
                "agent runtime configuration is unavailable for {}; cold recovery requires an active parent runtime context",
                target.canonical_path
            )
        })?;
        let descriptor = self
            .control
            .runtime_descriptor(&target.thread_id).await?
            .with_context(|| {
                format!(
                    "runtime descriptor is unavailable for {}",
                    target.canonical_path
                )
            })?;
        let agent_configuration = subagents::load_agent_configuration(
            &material.memory_dir,
            material.project_root.as_deref(),
        )?;
        let settings = &agent_configuration.settings;
        anyhow::ensure!(
            settings.enabled,
            "agent threads are disabled by Codex agent settings"
        );
        let catalog = &agent_configuration.catalog;
        let mut recovered_material = material.clone();
        let mut ancestors = Vec::new();
        let mut cursor = target.canonical_path.parent();
        while let Some(path) = cursor {
            if path == AgentPath::root() {
                break;
            }
            cursor = path.parent();
            ancestors.push(path);
        }
        ancestors.reverse();
        for path in ancestors {
            let ancestor = self.control.resolve_desktop_target(path.as_str()).await?;
            let ancestor_descriptor = self
                .control
                .runtime_descriptor(&ancestor.thread_id).await?
                .with_context(|| {
                    format!("runtime descriptor is unavailable for ancestor {path}")
                })?;
            let ancestor_resolved = subagents::resolve_agent(
                catalog,
                settings,
                &ancestor.agent_type,
                ancestor_descriptor.model.as_deref(),
                ancestor_descriptor.reasoning_effort.as_deref(),
                recovered_material.parent_model.as_deref(),
                Some(&recovered_material.parent_sandbox_mode),
            )?;
            for overlay in ancestor_resolved.definition.skills.config {
                recovered_material
                    .inherited_skill_config
                    .retain(|(path, _)| path != &overlay.path);
                recovered_material
                    .inherited_skill_config
                    .push((overlay.path, overlay.enabled));
            }
            if let Some(sandbox_mode) = ancestor_resolved.sandbox_mode {
                recovered_material.parent_sandbox_mode = sandbox_mode;
            }
        }
        let mut resolved = subagents::resolve_agent(
            catalog,
            settings,
            &target.agent_type,
            descriptor.model.as_deref(),
            descriptor.reasoning_effort.as_deref(),
            recovered_material.parent_model.as_deref(),
            Some(&recovered_material.parent_sandbox_mode),
        )?;
        // 当前 catalog 内容提供行为/配置覆盖，但重启不应悄悄切换
        // spawn 时接受的模型契约。持久化 descriptor 对这两个
        // 非机密选项具有权威性。
        resolved.model = descriptor.model.clone();
        resolved.model_reasoning_effort = descriptor.reasoning_effort.clone();
        let parent_path = target
            .canonical_path
            .parent()
            .context("non-root Agent Thread is missing its parent path")?;
        let parent_thread_id = target
            .parent_thread_id
            .as_deref()
            .context("non-root Agent Thread is missing its parent thread id")?;
        let mut runtime = build_runtime_request(
            SpawnAgentDispatchRequest {
                request: subagents::SpawnAgentV2Request {
                    task_name: target.task_name.clone(),
                    message: target.task_name.clone(),
                    agent_type: Some(target.agent_type.clone()),
                    model: descriptor.model,
                    reasoning_effort: descriptor.reasoning_effort,
                    fork_turns: None,
                },
                runtime: recovered_material,
            },
            resolved,
            &parent_path,
            parent_thread_id,
            self.control.root_thread_id(),
            settings.interrupt_message,
        );
        runtime.chat_targets = resolve_chat_targets_for_model(
            &runtime.chat_targets,
            runtime.model_request.model.as_deref(),
        )?;
        validate_recovered_runtime_setup(&material.memory_dir, &self.control, target, &runtime).await?;
        Ok(Arc::new(StoredRuntimeRequest {
            runtime,
            memory_dir: material.memory_dir.clone(),
        }))
    }
}

#[async_trait]
impl AgentThreadDispatch for DefaultAgentThreadDispatch {
    async fn spawn_agent(
        &self,
        mut request: SpawnAgentDispatchRequest,
    ) -> anyhow::Result<SpawnAgentV2Result> {
        let agent_configuration = subagents::load_agent_configuration(
            &request.runtime.memory_dir,
            request.runtime.project_root.as_deref(),
        )?;
        let settings = &agent_configuration.settings;
        if !settings.enabled {
            anyhow::bail!("agent threads are disabled by Codex agent settings");
        }
        let agent_type = request.request.agent_type.as_deref().unwrap_or("default");
        let resolved = subagents::resolve_agent(
            &agent_configuration.catalog,
            settings,
            agent_type,
            request.request.model.as_deref(),
            request.request.reasoning_effort.as_deref(),
            request.runtime.parent_model.as_deref(),
            Some(&request.runtime.parent_sandbox_mode),
        )?;
        request.runtime.chat_targets = resolve_chat_targets_for_model(
            &request.runtime.chat_targets,
            resolved.model.as_deref(),
        )?;

        let reservation = self.control.reserve_spawn_typed(
            &self.current_path,
            &request.request.task_name,
            &resolved.definition.name,
        ).await?;
        let thread = reservation.thread().clone();
        let memory_dir = request.runtime.memory_dir.clone();
        let runtime = build_runtime_request(
            request,
            resolved,
            &self.current_path,
            &self.current_thread_id,
            self.control.root_thread_id(),
            settings.interrupt_message,
        );

        self.control
            .record_runtime_descriptor(&subagents::AgentRuntimeDescriptorV2 {
                thread_id: thread.thread_id.clone(),
                model: runtime.model_request.model.clone(),
                reasoning_effort: runtime.model_request.reasoning_effort.clone(),
            }).await?;

        let mut forked_session = fork_parent_session(
            &memory_dir,
            &runtime,
            &thread.session_id,
            &runtime.model_request.fork_turns,
        ).await?;
        if let Err(error) = validate_runtime_setup(
            &memory_dir,
            &self.control,
            &thread,
            &runtime,
            &thread.session_id,
        ).await {
            return Err(rollback_fork_error(
                &mut forked_session,
                "validate agent runtime setup",
                error,
            ).await);
        }

        let stored = Arc::new(StoredRuntimeRequest {
            memory_dir,
            runtime,
        });
        if let Err(error) = self
            .runtime_requests
            .insert(&thread.thread_id, stored.clone())
        {
            return Err(rollback_fork_error(
                &mut forked_session,
                "register agent runtime request",
                error,
            ).await);
        }
        if let Err(error) = reservation.commit().await {
            self.runtime_requests.remove(&thread.thread_id);
            return Err(rollback_fork_error(
                &mut forked_session,
                "commit agent thread reservation",
                error,
            ).await);
        }
        let sessions_dir = forked_session.sessions_dir.clone();
        let child_session_id = forked_session.child_session_id.clone();
        let parent_session_id = forked_session.parent_session_id.clone();
        forked_session.disarm();
        let cleanup_control = Arc::clone(&self.control);
        let cleanup_requests = Arc::clone(&self.runtime_requests);
        let cleanup_thread = thread.clone();
        let unaccepted_cleanup = Arc::new(UnacceptedSpawnCleanup::new(move |turn_id| {
            cleanup_requests.remove(&cleanup_thread.thread_id);
            let graph_result = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(
                    cleanup_control.finalize_unaccepted_spawn(&cleanup_thread, turn_id),
                )
            })
            .map_err(|error| error.context("spawn graph and identity rollback"));
            let mut owned_session = ForkedSessionGuard {
                sessions_dir: sessions_dir.clone(),
                child_session_id: child_session_id.clone(),
                parent_session_id: parent_session_id.clone(),
                armed: true,
            };
            let session_result = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(owned_session.rollback())
            })
            .map_err(|error| error.context("child session rollback"));
            match (graph_result, session_result) {
                (Ok(()), Ok(())) => Ok(()),
                (Err(graph), Ok(())) => Err(graph),
                (Ok(()), Err(session)) => Err(session),
                (Err(graph), Err(session)) => Err(anyhow::anyhow!(
                    "{graph:#}; child session rollback failed: {session:#}"
                )),
            }
        }));
        let mut startup_guard = SpawnStartupGuard {
            cleanup: Arc::clone(&unaccepted_cleanup),
            armed: true,
        };
        let mut run_request = self.run_request(thread.clone(), stored.as_ref().clone(), false);
        run_request.unaccepted_spawn_cleanup = Some(unaccepted_cleanup);
        if let Err(start_error) = self.launch_turn(run_request).await {
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
            .list_agents(&self.current_path, request.path_prefix.as_deref()).await
    }

    async fn send_message(
        &self,
        request: MessageAgentV2Request,
    ) -> anyhow::Result<MessageAgentV2Result> {
        let message = self
            .control
            .enqueue_message(&self.current_path, request, false).await?;
        Ok(MessageAgentV2Result {
            message_id: message.message_id,
            queued: true,
            turn_triggered: false,
        })
    }

    async fn followup_task(
        &self,
        request: FollowupAgentDispatchRequest,
    ) -> anyhow::Result<MessageAgentV2Result> {
        let resolved = self
            .control
            .resolve_target(&self.current_path, &request.request.target).await?;
        anyhow::ensure!(
            resolved.canonical_path != AgentPath::root(),
            "follow-up tasks cannot target the root agent"
        );
        let followup_text = request.request.message.trim().to_string();
        #[cfg(test)]
        if let Some(hook) = self.before_followup_atomic_hook.as_ref() {
            let _ = self
                .control
                .resolve_target(&self.current_path, &request.request.target).await?;
            hook.entered.notify_one();
            hook.release.notified().await;
        }
        let (message, admission) = self.control.enqueue_followup_with_admission(
            &self.current_path,
            request.request,
            |target| {
                let stored = match self.runtime_requests.get(&target.thread_id)? {
                    Some(stored) => stored,
                    None => {
                        let recovered = futures::executor::block_on(
                            self.recover_runtime_request(target, request.runtime.as_ref()),
                        )?;
                        self.runtime_requests
                            .get_or_insert(&target.thread_id, recovered)?
                    }
                };
                let mut turn_request = stored.as_ref().clone();
                turn_request.runtime.model_request.message = followup_text;
                let run = self.run_request(target.clone(), turn_request, true);
                self.runtime_manager
                    .request_or_start_followup(&target.thread_id, run)
            },
        ).await?;
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
            FollowupAdmission::QueuedForActive => {}
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
        let cursor = ActivityCursor(self.wait_cursor.load(Ordering::Acquire));
        let (outcome, next_cursor) = self
            .control
            .wait_model_activity(
                cursor,
                Duration::from_millis(timeout_ms as u64),
                &self.current_thread_id,
                self.current_path == AgentPath::root(),
            )
            .await?;
        self.wait_cursor.fetch_max(next_cursor.0, Ordering::AcqRel);
        Ok(match outcome {
            WaitOutcome::MailboxActivity => WaitAgentV2Result {
                message: "Wait completed.".into(),
                timed_out: false,
            },
            WaitOutcome::Steered => WaitAgentV2Result {
                message: "Wait interrupted by new input.".into(),
                timed_out: false,
            },
            WaitOutcome::TimedOut => WaitAgentV2Result {
                message: "Wait timed out.".into(),
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
            .resolve_target(&self.current_path, &request.target).await?;
        anyhow::ensure!(
            target.canonical_path != AgentPath::root(),
            "the root agent cannot be interrupted through model tools"
        );
        anyhow::ensure!(
            target.canonical_path != self.current_path,
            "an agent cannot interrupt itself"
        );
        let previous_status = target.status.clone();
        self.runtime_manager
            .interrupt_active_if_any(&target.thread_id)
            .await?;
        let thread = self
            .control
            .resolve_target(&self.current_path, target.canonical_path.as_str()).await?;
        Ok(InterruptAgentV2Result {
            thread,
            previous_status,
        })
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
    let material = request.runtime;
    let mut model_request = request.request;
    model_request.agent_type = Some(resolved.definition.name.clone());
    model_request.model = resolved.model;
    model_request.reasoning_effort = resolved.model_reasoning_effort;

    let mut skills = material
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
        parent_agent_id: material.parent_agent_id,
        developer_instructions: resolved.definition.developer_instructions,
        context_snapshot: String::new(),
        sandbox_mode: resolved.sandbox_mode,
        mcp_servers: resolved.definition.mcp_servers,
        skills_config: skills,
        chat_targets: material.chat_targets,
        project_root: material.project_root,
        workspace_roots: material.workspace_roots,
        hook_runtime: material.hook_runtime,
        hook_bus: material.hook_bus,
        interrupt_message,
    }
}

async fn fork_parent_session(
    memory_dir: &Path,
    runtime: &SpawnRuntimeV2Request,
    child_session_id: &str,
    fork_turns: &Option<String>,
) -> anyhow::Result<ForkedSessionGuard> {
    let sessions_dir = home::data_dir(memory_dir);
    let sessions = session::SessionStore::open_sessions_dir(&sessions_dir).await?;
    let recent_turns = parse_fork_turns(fork_turns.as_deref())?;
    sessions.fork_session_recent_turns(
        &runtime.parent_session_id,
        child_session_id,
        recent_turns,
    ).await?;
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

async fn validate_runtime_setup(
    memory_dir: &Path,
    control: &Arc<AgentControl>,
    thread: &subagents::AgentThreadV2,
    runtime: &SpawnRuntimeV2Request,
    child_session_id: &str,
) -> anyhow::Result<()> {
    let _ = resolve_chat_targets_for_model(
        &runtime.chat_targets,
        runtime.model_request.model.as_deref(),
    )?;
    let _ = mcp::decode_inline_mcp_servers(&runtime.mcp_servers)?;
    let config = Config::with_defaults(memory_dir.to_path_buf());
    let _session = Session::with_session_id_for_agent_thread(
        config,
        child_session_id.to_string(),
        &runtime.parent_agent_id,
        Arc::clone(control),
        thread.canonical_path.clone(),
    ).await?;
    Ok(())
}

async fn validate_recovered_runtime_setup(
    memory_dir: &Path,
    control: &Arc<AgentControl>,
    thread: &subagents::AgentThreadV2,
    runtime: &SpawnRuntimeV2Request,
) -> anyhow::Result<()> {
    let sessions = session::SessionStore::open_sessions_dir(&home::data_dir(memory_dir)).await?;
    let stored = sessions
        .get_session(&thread.session_id).await?
        .with_context(|| format!("child session {:?} is unavailable", thread.session_id))?;
    anyhow::ensure!(
        stored.parent_session_id.as_deref() == thread.parent_thread_id.as_deref(),
        "child session ownership no longer matches the durable Agent Thread parent"
    );
    validate_runtime_setup(memory_dir, control, thread, runtime, &thread.session_id).await
}

/// 仅桌面端操作。此 trait 有意与 [`AgentThreadDispatch`] 中的
/// 六个模型可见方法保持分离。
#[async_trait]
pub trait DesktopAgentThreadControl: Send + Sync {
    async fn snapshot(&self, root_session_id: &str) -> anyhow::Result<AgentTreeSnapshotV2>;
    async fn read_thread(
        &self,
        root_session_id: &str,
        target: &str,
    ) -> anyhow::Result<AgentThreadDetailV2>;
    async fn followup(
        &self,
        root_session_id: &str,
        target: &str,
        message: String,
    ) -> anyhow::Result<AgentThreadV2>;
    async fn interrupt(
        &self,
        root_session_id: &str,
        target: &str,
    ) -> anyhow::Result<InterruptAgentV2Result>;
    async fn close_subtree(
        &self,
        root_session_id: &str,
        target: &str,
    ) -> anyhow::Result<AgentTreeSnapshotV2>;
}

#[derive(Debug)]
pub struct DesktopFollowupContextUnavailable;

impl std::fmt::Display for DesktopFollowupContextUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            "the root task is not active; open or resume the root task before following up this Agent Thread",
        )
    }
}

impl std::error::Error for DesktopFollowupContextUnavailable {}

#[derive(Debug)]
pub struct CloseSubtreeError {
    failed_path: String,
    cause: String,
    snapshot: AgentTreeSnapshotV2,
}

impl CloseSubtreeError {
    pub fn failed_path(&self) -> &str {
        &self.failed_path
    }

    pub fn snapshot(&self) -> &AgentTreeSnapshotV2 {
        &self.snapshot
    }
}

impl std::fmt::Display for CloseSubtreeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "agent subtree close stopped at {}: {}",
            self.failed_path, self.cause
        )
    }
}

impl std::error::Error for CloseSubtreeError {}

async fn close_subtree_error(
    control: &AgentControl,
    failed_path: &AgentPath,
    cause: impl std::fmt::Display,
) -> anyhow::Error {
    match control.snapshot().await {
        Ok(snapshot) => CloseSubtreeError {
            failed_path: failed_path.to_string(),
            cause: cause.to_string(),
            snapshot,
        }
        .into(),
        Err(snapshot_error) => anyhow::anyhow!(
            "agent subtree close stopped at {failed_path}: {cause}; snapshot failed: {snapshot_error:#}"
        ),
    }
}

async fn finish_close_operation(
    runtime_manager: Arc<AgentRuntimeManager>,
    control: Arc<AgentControl>,
    runtime_requests: Arc<RuntimeRequestRegistry>,
    threads: Vec<AgentThreadV2>,
) -> anyhow::Result<AgentTreeSnapshotV2> {
    let mut index = 0;
    loop {
        let Some(thread) = threads.get(index) else {
            return control.snapshot().await;
        };
        match runtime_manager
            .begin_close_thread(control.as_ref(), thread)
            .await
        {
            Ok(CloseThreadStart::Complete) => {
                match runtime_requests.take(&thread.thread_id) {
                    Ok(Some(_stored)) => {}
                    Ok(None) => {}
                    Err(error) => tracing::warn!(
                        %error,
                        thread_id = %thread.thread_id,
                        "failed to resolve Agent Thread stop hook observer"
                    ),
                }
                index += 1;
            }
            // request_close_slot 原子移除并拒绝 Starting 槽位，
            // 因此立即重新读取；在此等待可能错过在结果返回之前
            // 已发出的移除通知。
            Ok(CloseThreadStart::Starting) => continue,
            Ok(CloseThreadStart::TerminationRequested(terminated)) => {
                // 成功的非 Shutdown 确认可能属于跨越关闭准入边界的
                // 代际。重新读取持久化/运行时状态，而非将其视为完成。
                let acknowledgement = runtime_manager.wait_for_close_ack(terminated).await;
                if acknowledgement.is_err() {
                    // 已关闭的确认通道可能在清理收敛期间
                    // 被紧密循环重复订阅。
                    tokio::select! {
                        () = runtime_manager.wait_for_runtime_change() => {}
                        () = tokio::time::sleep(Duration::from_millis(50)) => {}
                    }
                }
            }
            Err(error) => {
                let runtime_may_still_be_live = runtime_manager.is_running(&thread.thread_id)
                    || control
                        .runtime_handle(&thread.thread_id)
                        .map(|handle| handle.is_some())
                        .unwrap_or(true);
                if runtime_may_still_be_live {
                    tokio::select! {
                        () = runtime_manager.wait_for_runtime_change() => {}
                        () = tokio::time::sleep(Duration::from_millis(50)) => {}
                    }
                    continue;
                }
                runtime_manager.clear_close_intent(&thread.thread_id);
                return Err(close_subtree_error(
                    control.as_ref(),
                    &thread.canonical_path,
                    error,
                ).await);
            }
        }
    }
}

pub struct DefaultDesktopAgentThreadControl {
    memory_dir: PathBuf,
    runtime_manager: Arc<AgentRuntimeManager>,
    runtime_requests: Arc<RuntimeRequestRegistry>,
    control_override: Option<Arc<AgentControl>>,
    #[cfg(any(test, feature = "test-support"))]
    chat_override: Option<crate::streaming::ChatOverride>,
    #[cfg(test)]
    close_barrier_hook: Option<super::agent_runtime::AckSubscribeHook>,
}

impl DefaultDesktopAgentThreadControl {
    pub fn new(memory_dir: PathBuf) -> Self {
        Self {
            memory_dir,
            runtime_manager: AgentRuntimeManager::global(),
            runtime_requests: RuntimeRequestRegistry::global(),
            control_override: None,
            #[cfg(any(test, feature = "test-support"))]
            chat_override: None,
            #[cfg(test)]
            close_barrier_hook: None,
        }
    }

    #[cfg(test)]
    fn for_test(
        memory_dir: PathBuf,
        control: Arc<AgentControl>,
        runtime_manager: Arc<AgentRuntimeManager>,
        runtime_requests: Arc<RuntimeRequestRegistry>,
        chat_override: Option<crate::streaming::ChatOverride>,
    ) -> Self {
        Self {
            memory_dir,
            runtime_manager,
            runtime_requests,
            control_override: Some(control),
            chat_override,
            close_barrier_hook: None,
        }
    }

    #[cfg(feature = "test-support")]
    fn for_acceptance(
        memory_dir: PathBuf,
        control: Arc<AgentControl>,
        runtime_manager: Arc<AgentRuntimeManager>,
        runtime_requests: Arc<RuntimeRequestRegistry>,
        chat_override: crate::streaming::ChatOverride,
    ) -> Self {
        Self {
            memory_dir,
            runtime_manager,
            runtime_requests,
            control_override: Some(control),
            chat_override: Some(chat_override),
            #[cfg(test)]
            close_barrier_hook: None,
        }
    }

    #[cfg(test)]
    fn set_close_barrier_hook(&mut self, hook: Option<super::agent_runtime::AckSubscribeHook>) {
        self.close_barrier_hook = hook;
    }

    async fn control(&self, root_session_id: &str) -> anyhow::Result<Arc<AgentControl>> {
        anyhow::ensure!(
            !root_session_id.trim().is_empty(),
            "root session id must not be empty"
        );
        if let Some(control) = self.control_override.as_ref() {
            anyhow::ensure!(
                control.root_thread_id() == root_session_id,
                "desktop control root does not match requested root session"
            );
            return Ok(Arc::clone(control));
        }
        crate::exec::agent_control_directory::AgentControlDirectory::global()
            .open_root_at(root_session_id, &home::subagents_db_path(&self.memory_dir))
            .await
    }

    fn dispatch(&self, control: Arc<AgentControl>) -> DefaultAgentThreadDispatch {
        let wait_cursor = control.activity_cursor();
        DefaultAgentThreadDispatch {
            current_thread_id: control.root_thread_id().to_string(),
            control,
            current_path: AgentPath::root(),
            runtime_manager: Arc::clone(&self.runtime_manager),
            runtime_requests: Arc::clone(&self.runtime_requests),
            wait_cursor: Arc::new(AtomicU64::new(wait_cursor.0)),
            #[cfg(any(test, feature = "test-support"))]
            chat_override: self.chat_override.clone(),
            #[cfg(test)]
            before_followup_atomic_hook: None,
        }
    }
}

#[async_trait]
impl DesktopAgentThreadControl for DefaultDesktopAgentThreadControl {
    async fn snapshot(&self, root_session_id: &str) -> anyhow::Result<AgentTreeSnapshotV2> {
        self.control(root_session_id).await?.snapshot().await
    }

    async fn read_thread(
        &self,
        root_session_id: &str,
        target: &str,
    ) -> anyhow::Result<AgentThreadDetailV2> {
        let control = self.control(root_session_id).await?;
        let thread = control.resolve_desktop_target(target).await?;
        let messages = session::SessionStore::open_sessions_dir(&home::data_dir(&self.memory_dir)).await?
            .get_messages(&thread.session_id).await?
            .into_iter()
            .map(|message| AgentThreadMessageV2 {
                id: message.id,
                session_id: message.session_id,
                role: message.role,
                content: message.content,
                compressed_content: message.compressed_content,
                tool_call_id: message.tool_call_id,
                tool_calls: message.tool_calls,
                tool_name: message.tool_name,
                timestamp: message.timestamp,
                token_count: message.token_count,
                finish_reason: message.finish_reason,
                reasoning: message.reasoning,
                reasoning_content: message.reasoning_content,
                reasoning_details: message.reasoning_details,
                reasoning_items: message.reasoning_items,
                message_items: message.message_items,
                media_json: message.media_json,
            })
            .collect();
        Ok(AgentThreadDetailV2 { thread, messages })
    }

    async fn followup(
        &self,
        root_session_id: &str,
        target: &str,
        message: String,
    ) -> anyhow::Result<AgentThreadV2> {
        let control = self.control(root_session_id).await?;
        let target_thread = control.resolve_desktop_target(target).await?;
        anyhow::ensure!(
            target_thread.canonical_path != AgentPath::root(),
            "the root agent cannot receive a desktop subagent follow-up"
        );
        let dispatch = self.dispatch(Arc::clone(&control));
        let runtime = if self
            .runtime_requests
            .get(&target_thread.thread_id)?
            .is_some()
        {
            None
        } else {
            Some(
                active_root_runtime_material(&self.memory_dir, root_session_id)
                    .await?
                    .ok_or(DesktopFollowupContextUnavailable)?,
            )
        };
        dispatch
            .followup_task(FollowupAgentDispatchRequest {
                request: MessageAgentV2Request {
                    target: target_thread.canonical_path.to_string(),
                    message,
                },
                runtime,
            })
            .await?;
        control.resolve_desktop_target(target_thread.canonical_path.as_str()).await
    }

    async fn interrupt(
        &self,
        root_session_id: &str,
        target: &str,
    ) -> anyhow::Result<InterruptAgentV2Result> {
        let control = self.control(root_session_id).await?;
        let target_thread = control.resolve_desktop_target(target).await?;
        anyhow::ensure!(
            target_thread.canonical_path != AgentPath::root(),
            "the root agent cannot be interrupted through desktop subagent controls"
        );
        let previous_status = target_thread.status;
        self.runtime_manager
            .interrupt(&target_thread.thread_id)
            .await?;
        let thread = control.resolve_desktop_target(target_thread.canonical_path.as_str()).await?;
        Ok(InterruptAgentV2Result {
            thread,
            previous_status,
        })
    }

    async fn close_subtree(
        &self,
        root_session_id: &str,
        target: &str,
    ) -> anyhow::Result<AgentTreeSnapshotV2> {
        let response_timeout = self.runtime_manager.close_timeout();
        let response_deadline = tokio::time::Instant::now() + response_timeout;
        let control = self.control(root_session_id).await?;
        let target_thread = control.resolve_desktop_target(target).await?;
        let _close = match tokio::time::timeout_at(
            response_deadline,
            self.runtime_manager.lock_subtree_close(control.as_ref()),
        )
        .await
        {
            Ok(result) => result?,
            Err(_) => {
                return Err(close_subtree_error(
                    control.as_ref(),
                    &target_thread.canonical_path,
                    "timed out waiting for subtree close coordination; an earlier close remains active",
                ).await);
            }
        };
        let close_admission = control.begin_close(target_thread.canonical_path.clone())?;
        match tokio::time::timeout_at(
            response_deadline,
            close_admission.wait_for_inflight_spawns(),
        )
        .await
        {
            Ok(result) => result?,
            Err(_) => {
                return Err(close_subtree_error(
                    control.as_ref(),
                    &target_thread.canonical_path,
                    "timed out waiting for in-flight agent spawn reservations",
                ).await);
            }
        }
        #[cfg(test)]
        if let Some(hook) = self.close_barrier_hook.as_ref() {
            hook.entered.notify_one();
            hook.release.notified().await;
        }
        let mut threads = control
            .snapshot().await?
            .threads
            .into_iter()
            .filter(|thread| {
                thread.canonical_path != AgentPath::root()
                    && thread
                        .canonical_path
                        .starts_with(&target_thread.canonical_path)
            })
            .collect::<Vec<_>>();
        threads.sort_by(|left, right| {
            right
                .canonical_path
                .depth()
                .cmp(&left.canonical_path.depth())
                .then_with(|| right.canonical_path.cmp(&left.canonical_path))
        });

        let Some(first_thread) = threads.first() else {
            return control.snapshot().await;
        };
        let failed_path = first_thread.canonical_path.clone();
        let runtime_manager = Arc::clone(&self.runtime_manager);
        let runtime_requests = Arc::clone(&self.runtime_requests);
        let background_control = Arc::clone(&control);
        let mut completion = tokio::spawn(async move {
            // 协调和生命周期准入归 manager 拥有的收敛任务所有，
            // 在第一个 runtime close intent 创建之前执行。
            let _subtree_close = _close;
            let _close_admission = close_admission;
            finish_close_operation(
                runtime_manager,
                background_control,
                runtime_requests,
                threads,
            )
            .await
        });
        match tokio::time::timeout_at(response_deadline, &mut completion).await {
            Ok(Ok(result)) => result,
            Ok(Err(join_error)) => Err(close_subtree_error(
                control.as_ref(),
                &failed_path,
                format!("background close task failed: {join_error}"),
            ).await),
            Err(_) => Err(close_subtree_error(
                control.as_ref(),
                &failed_path,
                "timed out waiting for shutdown acknowledgement; subtree remains closing in background",
            ).await),
        }
    }
}

#[cfg(feature = "test-support")]
pub mod test_support;

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use providers::types::stream::StreamChunk;
    use providers::CompletionStream;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use subagents::{AgentGraphStore, AgentStatusV2, Limits, RunnerEvent};

    use agent_db::sqlx;

    fn dispatch(dir: &tempfile::TempDir) -> DefaultAgentThreadDispatch {
        dispatch_for_root(
            dir,
            "root-session",
            Arc::new(AgentRuntimeManager::default()),
        )
    }

    fn dispatch_for_root(
        dir: &tempfile::TempDir,
        root_thread_id: &str,
        runtime_manager: Arc<AgentRuntimeManager>,
    ) -> DefaultAgentThreadDispatch {
        let store = AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap();
        let control = AgentControl::open(
            root_thread_id.into(),
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
            root_thread_id.into(),
            runtime_manager,
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

    fn dispatch_for_thread(
        dispatch: &DefaultAgentThreadDispatch,
        thread: &subagents::AgentThreadV2,
    ) -> DefaultAgentThreadDispatch {
        DefaultAgentThreadDispatch {
            control: Arc::clone(&dispatch.control),
            current_path: thread.canonical_path.clone(),
            current_thread_id: thread.thread_id.clone(),
            runtime_manager: Arc::clone(&dispatch.runtime_manager),
            runtime_requests: Arc::clone(&dispatch.runtime_requests),
            wait_cursor: Arc::new(AtomicU64::new(dispatch.control.activity_cursor().0)),
            chat_override: dispatch.chat_override.clone(),
            before_followup_atomic_hook: None,
        }
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

    fn gated_scripted_chat(
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    ) -> crate::streaming::ChatOverride {
        Arc::new(move |_messages, _tools, _config| {
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            Box::pin(async move {
                entered.notify_one();
                release.notified().await;
                Ok(Box::pin(stream::iter(vec![
                    Ok(StreamChunk::Text("done".into())),
                    Ok(StreamChunk::Done {
                        finish_reason: "stop".into(),
                    }),
                ])) as CompletionStream)
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

    fn capturing_config_chat(
        captured: Arc<Mutex<Vec<providers::types::ProviderConfig>>>,
    ) -> crate::streaming::ChatOverride {
        Arc::new(move |_messages, _tools, config| {
            captured.lock().unwrap().push(config.clone());
            Box::pin(async move {
                Ok(Box::pin(stream::iter(vec![
                    Ok(StreamChunk::Text("recovered".into())),
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

    fn gated_tool_then_final_chat(
        entered: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
        saw_followup: Arc<AtomicBool>,
    ) -> crate::streaming::ChatOverride {
        let calls = Arc::new(AtomicUsize::new(0));
        Arc::new(move |messages, _tools, _config| {
            let call = calls.fetch_add(1, Ordering::SeqCst);
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            let saw_followup = Arc::clone(&saw_followup);
            let has_followup = messages.iter().any(|message| match message {
                providers::Message::User { content } => content.iter().any(|part| {
                    matches!(
                        part,
                        providers::types::message::UserContent::Text { text }
                            if text.contains("consume in current turn")
                    )
                }),
                _ => false,
            });
            Box::pin(async move {
                if call == 0 {
                    entered.notify_one();
                    release.notified().await;
                    return Ok(Box::pin(stream::iter(vec![
                        Ok(StreamChunk::ToolCallStart {
                            index: 0,
                            id: "list-agents-call".into(),
                            name: "list_agents".into(),
                        }),
                        Ok(StreamChunk::ToolCallDelta {
                            index: 0,
                            arguments: "{}".into(),
                        }),
                        Ok(StreamChunk::Done {
                            finish_reason: "tool_calls".into(),
                        }),
                    ])) as CompletionStream);
                }
                saw_followup.store(has_followup, Ordering::SeqCst);
                Ok(Box::pin(stream::iter(vec![
                    Ok(StreamChunk::Text("done after followup".into())),
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
            runtime: ParentRuntimeMaterial {
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
                    api_mode: String::new(),
                }],
                project_root: None,
                workspace_roots: Vec::new(),
                hook_runtime: None,
                hook_bus: None,
            },
        }
    }

    fn request_with_hook_bus(
        memory_dir: &Path,
        hook_bus: Arc<hooks::PluginHookBus>,
    ) -> SpawnAgentDispatchRequest {
        let mut request = spawn_request(memory_dir);
        request.runtime.hook_bus = Some(hook_bus);
        request
    }

    fn desktop_control(
        dispatch: &DefaultAgentThreadDispatch,
        memory_dir: &Path,
    ) -> DefaultDesktopAgentThreadControl {
        DefaultDesktopAgentThreadControl::for_test(
            memory_dir.to_path_buf(),
            Arc::clone(&dispatch.control),
            Arc::clone(&dispatch.runtime_manager),
            Arc::clone(&dispatch.runtime_requests),
            dispatch.chat_override.clone(),
        )
    }

    #[tokio::test]
    async fn desktop_read_returns_complete_session_store_timeline() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let dispatch = dispatch(&dir);
        let child = committed_child(&dispatch, "worker");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions
            .ensure_session(&child.session_id, "agent-thread")
            .unwrap();
        sessions
            .append_message(session::NewMessage {
                content: Some("user task"),
                ..session::NewMessage::empty(&child.session_id, "user")
            })
            .unwrap();
        sessions
            .append_message(session::NewMessage {
                content: Some("calling tool"),
                compressed_content: Some("compressed assistant"),
                tool_calls: Some(serde_json::json!([{"id":"call-1","name":"terminal"}])),
                reasoning: Some("summary"),
                reasoning_content: Some("private reasoning"),
                reasoning_details: Some(serde_json::json!({"phase":"analysis"})),
                reasoning_items: Some(serde_json::json!([{"type":"reasoning"}])),
                message_items: Some(serde_json::json!([{"type":"message"}])),
                media_json: Some(r#"[{"kind":"image","path":"artifact.png"}]"#),
                ..session::NewMessage::empty(&child.session_id, "assistant")
            })
            .unwrap();
        sessions
            .append_message(session::NewMessage {
                content: Some("tool output"),
                tool_call_id: Some("call-1"),
                tool_name: Some("terminal"),
                ..session::NewMessage::empty(&child.session_id, "tool")
            })
            .unwrap();

        let detail = desktop_control(&dispatch, &memory_dir)
            .read_thread("root-session", "/root/worker")
            .await
            .unwrap();

        assert_eq!(detail.thread.thread_id, child.thread_id);
        assert_eq!(detail.messages.len(), 3);
        assert_eq!(detail.messages[0].role, "user");
        assert_eq!(
            detail.messages[1].compressed_content.as_deref(),
            Some("compressed assistant")
        );
        assert!(detail.messages[1].tool_calls.is_some());
        assert!(detail.messages[1].reasoning_details.is_some());
        assert!(detail.messages[1].reasoning_items.is_some());
        assert!(detail.messages[1].message_items.is_some());
        assert!(detail.messages[1].media_json.is_some());
        assert_eq!(detail.messages[2].role, "tool");
        assert_eq!(detail.messages[2].tool_call_id.as_deref(), Some("call-1"));
    }

    #[tokio::test]
    async fn desktop_cold_followup_requires_active_parent_context_without_queuing() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let dispatch = dispatch(&dir);
        let child = committed_child(&dispatch, "worker");

        let error = desktop_control(&dispatch, &memory_dir)
            .followup(
                "root-session",
                child.canonical_path.as_str(),
                "continue".into(),
            )
            .await
            .unwrap_err();

        assert!(error
            .downcast_ref::<DesktopFollowupContextUnavailable>()
            .is_some());
        assert!(format!("{error:#}").contains("open or resume the root task"));
        assert!(dispatch
            .control
            .drain_mailbox(&child.canonical_path)
            .unwrap()
            .is_empty());
        assert!(dispatch
            .runtime_requests
            .requests
            .lock()
            .unwrap()
            .is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn desktop_cold_followup_recovers_from_exact_live_root_session() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut dispatch = dispatch(&dir);
        dispatch.chat_override = Some(scripted_chat("done"));
        let request = spawn_request(&memory_dir);
        let root_material = request.runtime.clone();
        let child = AgentThreadDispatch::spawn_agent(&dispatch, request)
            .await
            .unwrap()
            .thread;
        while dispatch.runtime_manager.is_running(&child.thread_id) {
            tokio::task::yield_now().await;
        }
        dispatch.runtime_requests.remove(&child.thread_id);

        let root_session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(memory_dir.clone()),
                "root-session".into(),
            )
            .unwrap(),
        );
        register_active_root_session(&memory_dir, "root-session", &root_session).unwrap();
        root_session.set_chat_targets(root_material.chat_targets);
        root_session.set_permission_profile(Some(root_material.parent_sandbox_mode));
        root_session.set_project_root(root_material.project_root);
        root_session.set_skill_config_overrides(root_material.inherited_skill_config);

        desktop_control(&dispatch, &memory_dir)
            .followup(
                "root-session",
                child.canonical_path.as_str(),
                "continue from desktop after restart".into(),
            )
            .await
            .unwrap();
        while dispatch.runtime_manager.is_running(&child.thread_id) {
            tokio::task::yield_now().await;
        }

        let messages = sessions.get_messages(&child.session_id).unwrap();
        assert_eq!(
            messages
                .iter()
                .filter(|message| {
                    message.content.as_deref() == Some("continue from desktop after restart")
                })
                .count(),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn live_root_lookup_revalidates_arc_identity_after_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let make_session = |api_key: &str, permission: &str| {
            let session = Session::with_session_id(
                Config::with_defaults(memory_dir.clone()),
                "root-session".into(),
            )
            .unwrap();
            session.set_chat_targets(vec![types::ChatTarget {
                provider_id: "openai".into(),
                backend_id: "openai".into(),
                model: "test".into(),
                api_key: api_key.into(),
                base_url: "https://openai.invalid".into(),
                api_mode: String::new(),
            }]);
            session.set_permission_profile(Some(permission.into()));
            Arc::new(session)
        };
        let old = make_session("old-key-must-not-win", types::DANGER_FULL_ACCESS_PROFILE);
        let replacement = make_session("replacement-key", types::READ_ONLY_PROFILE);
        register_active_root_session(&memory_dir, "root-session", &old).unwrap();

        let lookup_memory_dir = memory_dir.clone();
        let (upgraded_tx, upgraded_rx) = tokio::sync::oneshot::channel();
        let mut upgraded_tx = Some(upgraded_tx);
        let lookup = tokio::spawn(async move {
            active_root_runtime_material_with_upgrade_hook(
                &lookup_memory_dir,
                "root-session",
                || {
                    if let Some(upgraded_tx) = upgraded_tx.take() {
                        upgraded_tx.send(()).unwrap();
                    }
                },
            )
            .await
            .unwrap()
            .unwrap()
        });
        upgraded_rx.await.unwrap();
        register_active_root_session(&memory_dir, "root-session", &replacement).unwrap();
        let material = lookup.await.unwrap();
        assert_eq!(material.chat_targets[0].api_key, "replacement-key");
        assert_eq!(material.parent_sandbox_mode, types::READ_ONLY_PROFILE);
    }

    #[tokio::test]
    async fn desktop_close_subtree_is_leaf_first_idempotent_and_root_safe() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let dispatch = dispatch(&dir);
        let parent = committed_child(&dispatch, "parent");
        dispatch
            .control
            .record_runner_event(
                &parent.thread_id,
                RunnerEvent::TurnCompleted {
                    turn_id: "parent-turn".into(),
                    last_message: "parent done".into(),
                },
            )
            .unwrap();
        let reservation = dispatch
            .control
            .reserve_spawn(&parent.canonical_path, "child")
            .unwrap();
        let child = reservation.thread().clone();
        reservation.commit().unwrap();
        dispatch
            .control
            .record_runner_event(
                &child.thread_id,
                RunnerEvent::TurnInterrupted {
                    turn_id: "child-turn".into(),
                    reason: "stopped".into(),
                },
            )
            .unwrap();
        let desktop = desktop_control(&dispatch, &memory_dir);

        let first = desktop
            .close_subtree("root-session", "/root/parent")
            .await
            .unwrap();
        let second = desktop
            .close_subtree("root-session", "/root/parent")
            .await
            .unwrap();

        for path in ["/root/parent", "/root/parent/child"] {
            assert!(first
                .threads
                .iter()
                .any(|thread| thread.canonical_path.as_str() == path
                    && thread.status == AgentStatusV2::Shutdown));
        }
        assert_eq!(first.threads, second.threads);
        assert_eq!(
            dispatch
                .control
                .status_events(&parent.thread_id)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            dispatch
                .control
                .status_events(&child.thread_id)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(dispatch.control.active_execution_count().unwrap(), 0);
        assert_eq!(dispatch.runtime_manager.active_count(), 0);
        assert!(dispatch
            .control
            .runtime_handle(&parent.thread_id)
            .unwrap()
            .is_none());
        assert!(dispatch
            .control
            .runtime_handle(&child.thread_id)
            .unwrap()
            .is_none());

        let shutdown_followup = AgentThreadDispatch::followup_task(
            &dispatch,
            FollowupAgentDispatchRequest {
                request: MessageAgentV2Request {
                    target: child.canonical_path.to_string(),
                    message: "must stay closed".into(),
                },
                runtime: Some(spawn_request(&memory_dir).runtime),
            },
        )
        .await
        .unwrap_err();
        assert!(format!("{shutdown_followup:#}").contains("Shutdown"));
        assert!(dispatch
            .runtime_requests
            .requests
            .lock()
            .unwrap()
            .is_empty());
        assert!(dispatch
            .control
            .drain_mailbox(&child.canonical_path)
            .unwrap()
            .is_empty());

        let root_close = desktop
            .close_subtree("root-session", "/root")
            .await
            .unwrap();
        assert_eq!(
            root_close
                .threads
                .iter()
                .find(|thread| thread.canonical_path == AgentPath::root())
                .unwrap()
                .status,
            AgentStatusV2::Running
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn desktop_close_partial_failure_keeps_parent_open_and_retryable() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let bus = Arc::new(hooks::PluginHookBus::new());
        let stopped_paths = Arc::new(Mutex::new(Vec::<String>::new()));
        let observed = Arc::clone(&stopped_paths);
        bus.register(hooks::SUBAGENT_STOP, move |payload| {
            observed.lock().unwrap().push(payload.detail.clone());
            hooks::HookOutcome::Continue
        });
        let mut dispatch = dispatch(&dir);
        dispatch.chat_override = Some(scripted_chat("done"));
        let mut parent_request = request_with_hook_bus(&memory_dir, Arc::clone(&bus));
        parent_request.request.task_name = "parent".into();
        let parent = AgentThreadDispatch::spawn_agent(&dispatch, parent_request)
            .await
            .unwrap()
            .thread;
        while dispatch.runtime_manager.is_running(&parent.thread_id) {
            tokio::task::yield_now().await;
        }
        let child_dispatch = DefaultAgentThreadDispatch {
            control: Arc::clone(&dispatch.control),
            current_path: parent.canonical_path.clone(),
            current_thread_id: parent.thread_id.clone(),
            runtime_manager: Arc::clone(&dispatch.runtime_manager),
            runtime_requests: Arc::clone(&dispatch.runtime_requests),
            wait_cursor: Arc::clone(&dispatch.wait_cursor),
            chat_override: Some(scripted_chat("done")),
            before_followup_atomic_hook: None,
        };
        let mut leaf_request = request_with_hook_bus(&memory_dir, bus);
        leaf_request.request.task_name = "leaf".into();
        let leaf = AgentThreadDispatch::spawn_agent(&child_dispatch, leaf_request)
            .await
            .unwrap()
            .thread;
        while dispatch.runtime_manager.is_running(&leaf.thread_id) {
            tokio::task::yield_now().await;
        }
        let pool = agent_db::sqlx::SqlitePool::connect(
            &format!("sqlite:{}", dir.path().join("subagents-v2.db").display())
        ).await.unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "CREATE TRIGGER fail_parent_shutdown
             BEFORE UPDATE OF status_kind ON agent_threads
             WHEN NEW.thread_id = '{}' AND NEW.status_kind = 'shutdown'
             BEGIN
               SELECT RAISE(ABORT, 'injected parent shutdown failure');
             END;",
            parent.thread_id
        )))
        .execute(&pool).await.unwrap();
        let desktop = desktop_control(&dispatch, &memory_dir);

        let error = desktop
            .close_subtree("root-session", "/root/parent")
            .await
            .unwrap_err();
        let partial = error.downcast_ref::<CloseSubtreeError>().unwrap();
        assert_eq!(partial.failed_path(), "/root/parent");
        assert!(partial
            .snapshot().await
            .threads
            .iter()
            .any(|thread| thread.thread_id == leaf.thread_id
                && thread.status == AgentStatusV2::Shutdown));
        assert_ne!(
            partial
                .snapshot().await
                .threads
                .iter()
                .find(|thread| thread.thread_id == parent.thread_id)
                .unwrap()
                .status,
            AgentStatusV2::Shutdown
        );
        assert_eq!(stopped_paths.lock().unwrap().len(), 2);
        assert!(stopped_paths
            .lock()
            .unwrap()
            .iter()
            .any(|payload| payload.contains("path=/root/parent/leaf")));
        assert!(dispatch
            .runtime_requests
            .get(&leaf.thread_id)
            .unwrap()
            .is_none());
        assert!(dispatch
            .runtime_requests
            .get(&parent.thread_id)
            .unwrap()
            .is_some());

        graph
            .execute_batch("DROP TRIGGER fail_parent_shutdown;")
            .unwrap();
        let retried = desktop
            .close_subtree("root-session", "/root/parent")
            .await
            .unwrap();
        assert!(retried.threads.iter().any(|thread| {
            thread.thread_id == parent.thread_id && thread.status == AgentStatusV2::Shutdown
        }));
        let stopped_paths = stopped_paths.lock().unwrap();
        assert_eq!(stopped_paths.len(), 2);
        assert!(stopped_paths
            .iter()
            .any(|payload| payload.contains("path=/root/parent")));
        assert!(dispatch
            .runtime_requests
            .get(&parent.thread_id)
            .unwrap()
            .is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn desktop_close_active_runtime_waits_for_shutdown_ack_and_cleanup() {
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
        let desktop = desktop_control(&dispatch, &memory_dir);

        let snapshot = desktop
            .close_subtree("root-session", "/root/worker")
            .await
            .unwrap();

        assert_eq!(
            snapshot
                .threads
                .iter()
                .find(|thread| thread.thread_id == spawned.thread.thread_id)
                .unwrap()
                .status,
            AgentStatusV2::Shutdown
        );
        assert_eq!(dispatch.runtime_manager.active_count(), 0);
        assert_eq!(dispatch.control.active_execution_count().unwrap(), 0);
        assert!(dispatch
            .control
            .runtime_handle(&spawned.thread.thread_id)
            .unwrap()
            .is_none());
        assert_eq!(
            dispatch
                .control
                .status_events(&spawned.thread.thread_id)
                .unwrap()
                .into_iter()
                .map(|event| event.event)
                .filter(|event| matches!(event, RunnerEvent::RuntimeTerminated))
                .count(),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn close_timeout_after_terminate_keeps_admission_closed_until_runner_ack() {
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
        let terminal_hook = super::super::agent_runtime::AckSubscribeHook {
            entered: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
        };
        dispatch
            .runtime_manager
            .set_before_terminal_persist_hook(Some(terminal_hook.clone()));
        dispatch
            .runtime_manager
            .set_close_timeout(Duration::from_millis(20));
        let desktop = Arc::new(desktop_control(&dispatch, &memory_dir));
        let close = tokio::spawn({
            let desktop = Arc::clone(&desktop);
            async move { desktop.close_subtree("root-session", "/root/worker").await }
        });
        terminal_hook.entered.notified().await;

        let error = close.await.unwrap().unwrap_err();
        assert!(error.to_string().contains("remains closing in background"));
        let partial = error.downcast_ref::<CloseSubtreeError>().unwrap();
        assert_eq!(partial.failed_path(), "/root/worker");
        assert!(dispatch
            .control
            .is_path_closing(&spawned.thread.canonical_path)
            .unwrap());
        assert_close_admission_rejects_new_work(&dispatch, &spawned.thread);

        terminal_hook.release.notify_one();
        wait_for_shutdown_and_barrier_release(&dispatch, &spawned.thread).await;
        assert!(dispatch
            .control
            .reserve_spawn(&spawned.thread.canonical_path, "too_late")
            .is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn stuck_background_close_in_one_root_does_not_block_another_root() {
        let dir_a = tempfile::tempdir().unwrap();
        let dir_b = tempfile::tempdir().unwrap();
        let memory_a = dir_a.path().join("memory");
        let memory_b = dir_b.path().join("memory");
        let manager = Arc::new(AgentRuntimeManager::default());
        let mut dispatch_a = dispatch_for_root(&dir_a, "root-a", Arc::clone(&manager));
        let dispatch_b = dispatch_for_root(&dir_b, "root-b", Arc::clone(&manager));
        session::SessionStore::open_sessions_dir(&memory_a.join("sessions"))
            .unwrap()
            .ensure_session("root-a", "test")
            .unwrap();
        session::SessionStore::open_sessions_dir(&memory_b.join("sessions"))
            .unwrap()
            .ensure_session("root-b", "test")
            .unwrap();
        dispatch_a.chat_override = Some(pending_chat());
        let worker_a = AgentThreadDispatch::spawn_agent(&dispatch_a, spawn_request(&memory_a))
            .await
            .unwrap()
            .thread;
        let worker_b = committed_child(&dispatch_b, "worker");
        let terminal_hook = super::super::agent_runtime::AckSubscribeHook {
            entered: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
        };
        manager.set_before_terminal_persist_hook(Some(terminal_hook.clone()));
        manager.set_close_timeout(Duration::from_millis(20));
        let close_a = tokio::spawn({
            let desktop = desktop_control(&dispatch_a, &memory_a);
            async move { desktop.close_subtree("root-a", "/root/worker").await }
        });
        terminal_hook.entered.notified().await;
        assert!(close_a
            .await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("remains closing in background"));

        // Root A deliberately proves the 20ms timeout path. Give root B an
        // internal deadline that tolerates parallel-test scheduler jitter,
        // while the outer 500ms bound still proves A's live close lock does
        // not serialize an unrelated root.
        manager.set_close_timeout(Duration::from_secs(1));
        let close_b = tokio::time::timeout(
            Duration::from_millis(500),
            desktop_control(&dispatch_b, &memory_b).close_subtree("root-b", "/root/worker"),
        )
        .await;
        terminal_hook.release.notify_one();
        wait_for_shutdown_and_barrier_release(&dispatch_a, &worker_a).await;
        let snapshot_b = close_b
            .expect("root B must not wait for root A's close lock")
            .unwrap();
        assert_eq!(
            snapshot_b
                .threads
                .iter()
                .find(|thread| thread.thread_id == worker_b.thread_id)
                .unwrap()
                .status,
            AgentStatusV2::Shutdown
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn same_root_close_lock_wait_is_bounded_by_the_caller_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut dispatch = dispatch(&dir);
        dispatch.chat_override = Some(pending_chat());
        let worker = AgentThreadDispatch::spawn_agent(&dispatch, spawn_request(&memory_dir))
            .await
            .unwrap()
            .thread;
        let terminal_hook = super::super::agent_runtime::AckSubscribeHook {
            entered: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
        };
        dispatch
            .runtime_manager
            .set_before_terminal_persist_hook(Some(terminal_hook.clone()));
        dispatch
            .runtime_manager
            .set_close_timeout(Duration::from_millis(20));
        let desktop = Arc::new(desktop_control(&dispatch, &memory_dir));
        let first = tokio::spawn({
            let desktop = Arc::clone(&desktop);
            async move { desktop.close_subtree("root-session", "/root/worker").await }
        });
        terminal_hook.entered.notified().await;
        assert!(first.await.unwrap().is_err());

        let second = tokio::time::timeout(
            Duration::from_millis(100),
            desktop.close_subtree("root-session", "/root/worker"),
        )
        .await;
        terminal_hook.release.notify_one();
        wait_for_shutdown_and_barrier_release(&dispatch, &worker).await;
        let error = second
            .expect("same-root coordination wait must obey the caller deadline")
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("timed out waiting for subtree close coordination"));
        assert_eq!(
            error
                .downcast_ref::<CloseSubtreeError>()
                .unwrap()
                .failed_path(),
            "/root/worker"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn caller_cancellation_after_starting_close_keeps_background_convergence_owner() {
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
        let followup = tokio::spawn({
            let dispatch = Arc::clone(&dispatch);
            let target = spawned.thread.canonical_path.to_string();
            async move {
                AgentThreadDispatch::followup_task(
                    &*dispatch,
                    MessageAgentV2Request {
                        target,
                        message: "close must own this Starting slot".into(),
                    }
                    .into(),
                )
                .await
            }
        });
        start_entered.notified().await;
        let close_hook = crate::exec::agent_runtime::AckSubscribeHook {
            entered: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
        };
        dispatch
            .runtime_manager
            .set_after_starting_close_hook(Some(close_hook.clone()));
        let close = tokio::spawn({
            let desktop = desktop_control(&dispatch, &memory_dir);
            async move { desktop.close_subtree("root-session", "/root/worker").await }
        });
        close_hook.entered.notified().await;
        close.abort();
        assert!(dispatch
            .control
            .is_path_closing(&spawned.thread.canonical_path)
            .unwrap());
        close_hook.release.notify_one();
        start_release.notify_one();
        wait_for_shutdown_and_barrier_release(&dispatch, &spawned.thread).await;
        assert!(followup.await.unwrap().is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn close_retries_after_old_generation_completes_before_atomic_signal() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let provider_entered = Arc::new(tokio::sync::Notify::new());
        let provider_release = Arc::new(tokio::sync::Notify::new());
        let mut dispatch = dispatch(&dir);
        dispatch.chat_override = Some(gated_scripted_chat(
            Arc::clone(&provider_entered),
            Arc::clone(&provider_release),
        ));
        let spawned = AgentThreadDispatch::spawn_agent(&dispatch, spawn_request(&memory_dir))
            .await
            .unwrap();
        provider_entered.notified().await;
        let close_hook = super::super::agent_runtime::CloseAdmissionHook {
            entered: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new())),
        };
        dispatch
            .runtime_manager
            .set_close_admission_hook(Some(close_hook.clone()));
        let cleanup_entered = Arc::new(tokio::sync::Notify::new());
        dispatch
            .runtime_manager
            .set_before_cleanup_hook(Some(Arc::new({
                let cleanup_entered = Arc::clone(&cleanup_entered);
                move || cleanup_entered.notify_one()
            })));
        let desktop = Arc::new(desktop_control(&dispatch, &memory_dir));
        let close = tokio::spawn({
            let desktop = Arc::clone(&desktop);
            async move { desktop.close_subtree("root-session", "/root/worker").await }
        });
        close_hook.entered.notified().await;

        provider_release.notify_one();
        cleanup_entered.notified().await;
        assert!(dispatch
            .control
            .is_path_closing(&spawned.thread.canonical_path)
            .unwrap());
        let (released, wake) = &*close_hook.release;
        *released.lock().unwrap() = true;
        wake.notify_all();

        let snapshot = close.await.unwrap().unwrap();
        assert_eq!(
            snapshot
                .threads
                .iter()
                .find(|thread| thread.thread_id == spawned.thread.thread_id)
                .unwrap()
                .status,
            AgentStatusV2::Shutdown
        );
        assert!(!dispatch
            .control
            .is_path_closing(&spawned.thread.canonical_path)
            .unwrap());
        assert!(!dispatch
            .runtime_manager
            .is_running(&spawned.thread.thread_id));
        assert!(dispatch
            .control
            .runtime_handle(&spawned.thread.thread_id)
            .unwrap()
            .is_none());
        let pool = agent_db::sqlx::SqlitePool::connect(
            &format!("sqlite:{}", dir.path().join("subagents-v2.db").display())
        ).await.unwrap();
        let (edge_state,): (String,) = agent_db::sqlx::query_as(
            "SELECT edge_state FROM agent_spawn_edges WHERE child_thread_id = ?1",
        )
        .bind(&spawned.thread.thread_id)
        .fetch_one(&pool).await.unwrap();
        assert_eq!(edge_state, "closed");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn close_rejects_previously_admitted_handoff_without_starting_a_new_generation() {
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
        let followup = tokio::spawn({
            let dispatch = Arc::clone(&dispatch);
            let target = spawned.thread.canonical_path.to_string();
            async move {
                AgentThreadDispatch::followup_task(
                    &*dispatch,
                    MessageAgentV2Request {
                        target,
                        message: "must remain queued".into(),
                    }
                    .into(),
                )
                .await
            }
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

        let snapshot = desktop_control(&dispatch, &memory_dir)
            .close_subtree("root-session", spawned.thread.canonical_path.as_str())
            .await
            .unwrap();
        let followup_result = followup.await.unwrap().unwrap();
        assert!(followup_result.queued);
        assert!(followup_result.turn_triggered);
        assert_eq!(
            snapshot
                .threads
                .iter()
                .find(|thread| thread.thread_id == spawned.thread.thread_id)
                .unwrap()
                .status,
            AgentStatusV2::Shutdown
        );
        assert_eq!(
            dispatch
                .control
                .drain_mailbox(&spawned.thread.canonical_path)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(dispatch.runtime_manager.active_count(), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn close_cancels_starting_followup_without_missing_its_state_change() {
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
        let followup = tokio::spawn({
            let dispatch = Arc::clone(&dispatch);
            let target = spawned.thread.canonical_path.to_string();
            async move {
                AgentThreadDispatch::followup_task(
                    &*dispatch,
                    MessageAgentV2Request {
                        target,
                        message: "must be cancelled while Starting".into(),
                    }
                    .into(),
                )
                .await
            }
        });
        start_entered.notified().await;

        let snapshot = tokio::time::timeout(
            Duration::from_secs(1),
            desktop_control(&dispatch, &memory_dir)
                .close_subtree("root-session", spawned.thread.canonical_path.as_str()),
        )
        .await
        .expect("close must not wait for a notification emitted before Starting was returned")
        .unwrap();
        start_release.notify_one();
        assert!(followup.await.unwrap().is_err());
        assert_eq!(
            snapshot
                .threads
                .iter()
                .find(|thread| thread.thread_id == spawned.thread.thread_id)
                .unwrap()
                .status,
            AgentStatusV2::Shutdown
        );
        assert_eq!(dispatch.runtime_manager.active_count(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancelled_close_after_terminate_keeps_admission_closed_until_runner_ack() {
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
        let terminal_hook = super::super::agent_runtime::AckSubscribeHook {
            entered: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
        };
        dispatch
            .runtime_manager
            .set_before_terminal_persist_hook(Some(terminal_hook.clone()));
        let desktop = Arc::new(desktop_control(&dispatch, &memory_dir));
        let close = tokio::spawn({
            let desktop = Arc::clone(&desktop);
            async move { desktop.close_subtree("root-session", "/root/worker").await }
        });
        terminal_hook.entered.notified().await;

        close.abort();
        assert!(close.await.unwrap_err().is_cancelled());
        assert!(dispatch
            .control
            .is_path_closing(&spawned.thread.canonical_path)
            .unwrap());
        assert_close_admission_rejects_new_work(&dispatch, &spawned.thread);

        terminal_hook.release.notify_one();
        wait_for_shutdown_and_barrier_release(&dispatch, &spawned.thread).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn leaf_timeout_never_requests_parent_shutdown_before_leaf_ack() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut root_dispatch = dispatch(&dir);
        root_dispatch.chat_override = Some(pending_chat());
        let parent = AgentThreadDispatch::spawn_agent(&root_dispatch, spawn_request(&memory_dir))
            .await
            .unwrap()
            .thread;
        let mut child_dispatch = DefaultAgentThreadDispatch::for_test(
            Arc::clone(&root_dispatch.control),
            parent.canonical_path.clone(),
            parent.thread_id.clone(),
            Arc::clone(&root_dispatch.runtime_manager),
        );
        child_dispatch.chat_override = Some(pending_chat());
        let mut leaf_request = spawn_request(&memory_dir);
        leaf_request.request.task_name = "leaf".into();
        let leaf = AgentThreadDispatch::spawn_agent(&child_dispatch, leaf_request)
            .await
            .unwrap()
            .thread;
        let terminal_hook = super::super::agent_runtime::AckSubscribeHook {
            entered: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
        };
        root_dispatch
            .runtime_manager
            .set_before_terminal_persist_hook(Some(terminal_hook.clone()));
        root_dispatch
            .runtime_manager
            .set_close_timeout(Duration::from_millis(20));
        let desktop = desktop_control(&root_dispatch, &memory_dir);
        let close =
            tokio::spawn(
                async move { desktop.close_subtree("root-session", "/root/worker").await },
            );
        terminal_hook.entered.notified().await;

        let error = close.await.unwrap().unwrap_err();
        assert_eq!(
            error
                .downcast_ref::<CloseSubtreeError>()
                .unwrap()
                .failed_path(),
            leaf.canonical_path.as_str()
        );
        assert_ne!(
            root_dispatch
                .control
                .resolve_desktop_target(parent.canonical_path.as_str())
                .unwrap()
                .status,
            AgentStatusV2::Shutdown
        );
        assert!(root_dispatch
            .control
            .is_path_closing(&parent.canonical_path)
            .unwrap());

        root_dispatch
            .runtime_manager
            .set_before_terminal_persist_hook(None);
        terminal_hook.release.notify_one();
        wait_for_shutdown_and_barrier_release(&root_dispatch, &parent).await;
        assert_eq!(
            root_dispatch
                .control
                .resolve_desktop_target(leaf.canonical_path.as_str())
                .unwrap()
                .status,
            AgentStatusV2::Shutdown
        );
    }

    fn assert_close_admission_rejects_new_work(
        dispatch: &DefaultAgentThreadDispatch,
        thread: &AgentThreadV2,
    ) {
        assert!(dispatch
            .control
            .reserve_spawn(&thread.canonical_path, "blocked_child")
            .is_err());
        assert!(dispatch
            .control
            .enqueue_message(
                &AgentPath::root(),
                MessageAgentV2Request {
                    target: thread.canonical_path.to_string(),
                    message: "queue only".into(),
                },
                false,
            )
            .is_err());
        assert!(dispatch
            .control
            .enqueue_followup_with_admission(
                &AgentPath::root(),
                MessageAgentV2Request {
                    target: thread.canonical_path.to_string(),
                    message: "trigger turn".into(),
                },
                |_| Ok(()),
            )
            .is_err());
        assert!(dispatch
            .control
            .drain_mailbox(&thread.canonical_path)
            .unwrap()
            .is_empty());
    }

    async fn wait_for_shutdown_and_barrier_release(
        dispatch: &DefaultAgentThreadDispatch,
        thread: &AgentThreadV2,
    ) {
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let current = dispatch
                    .control
                    .resolve_desktop_target(thread.canonical_path.as_str())
                    .unwrap();
                if current.status == AgentStatusV2::Shutdown
                    && !dispatch
                        .control
                        .is_path_closing(&thread.canonical_path)
                        .unwrap()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn desktop_interrupt_is_ack_driven_and_reports_previous_status() {
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
        let desktop = desktop_control(&dispatch, &memory_dir);

        let result = desktop
            .interrupt("root-session", "/root/worker")
            .await
            .unwrap();

        assert_eq!(result.previous_status, AgentStatusV2::Running);
        assert_eq!(result.thread.status, AgentStatusV2::Interrupted);
        assert!(!dispatch
            .runtime_manager
            .is_running(&spawned.thread.thread_id));
        assert!(desktop.interrupt("root-session", "/root").await.is_err());
    }

    #[tokio::test]
    async fn concurrent_desktop_close_callers_share_idempotent_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let dispatch = dispatch(&dir);
        let child = committed_child(&dispatch, "worker");
        let desktop = Arc::new(desktop_control(&dispatch, &memory_dir));

        let (first, second) = tokio::join!(
            desktop.close_subtree("root-session", "/root/worker"),
            desktop.close_subtree("root-session", "/root/worker")
        );
        first.unwrap();
        second.unwrap();

        assert_eq!(
            dispatch
                .control
                .status_events(&child.thread_id)
                .unwrap()
                .into_iter()
                .filter(|event| matches!(event.event, RunnerEvent::RuntimeTerminated))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn close_barrier_rejects_spawn_send_and_followup_under_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let dispatch = dispatch(&dir);
        let parent = committed_child(&dispatch, "parent");
        let child_reservation = dispatch
            .control
            .reserve_spawn(&parent.canonical_path, "child")
            .unwrap();
        let child = child_reservation.thread().clone();
        child_reservation.commit().unwrap();
        let hook = super::super::agent_runtime::AckSubscribeHook {
            entered: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
        };
        let mut desktop = desktop_control(&dispatch, &memory_dir);
        desktop.set_close_barrier_hook(Some(hook.clone()));
        let desktop = Arc::new(desktop);
        let close = tokio::spawn({
            let desktop = Arc::clone(&desktop);
            async move { desktop.close_subtree("root-session", "/root/parent").await }
        });
        hook.entered.notified().await;

        assert!(dispatch
            .control
            .reserve_spawn(&parent.canonical_path, "late_child")
            .is_err());
        assert!(dispatch
            .control
            .reserve_spawn(&child.canonical_path, "late_grandchild")
            .is_err());
        assert!(dispatch
            .control
            .enqueue_message(
                &AgentPath::root(),
                MessageAgentV2Request {
                    target: parent.canonical_path.to_string(),
                    message: "queue only".into(),
                },
                false,
            )
            .is_err());
        assert!(dispatch
            .control
            .enqueue_followup_with_admission(
                &AgentPath::root(),
                MessageAgentV2Request {
                    target: parent.canonical_path.to_string(),
                    message: "trigger turn".into(),
                },
                |_| Ok(()),
            )
            .is_err());
        assert!(dispatch
            .control
            .drain_mailbox(&parent.canonical_path)
            .unwrap()
            .is_empty());
        assert!(!dispatch
            .control
            .snapshot().await
            .unwrap()
            .threads
            .iter()
            .any(|thread| {
                matches!(
                    thread.canonical_path.as_str(),
                    "/root/parent/late_child" | "/root/parent/child/late_grandchild"
                )
            }));

        hook.release.notify_one();
        close.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn close_waits_for_preexisting_spawn_reservation_then_closes_committed_child() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let dispatch = dispatch(&dir);
        let parent = committed_child(&dispatch, "parent");
        let reservation = dispatch
            .control
            .reserve_spawn(&parent.canonical_path, "late_child")
            .unwrap();
        let late_child = reservation.thread().clone();
        let desktop = Arc::new(desktop_control(&dispatch, &memory_dir));
        let close = tokio::spawn({
            let desktop = Arc::clone(&desktop);
            async move { desktop.close_subtree("root-session", "/root/parent").await }
        });

        tokio::time::timeout(Duration::from_secs(1), async {
            while !dispatch
                .control
                .is_path_closing(&parent.canonical_path)
                .unwrap()
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(!close.is_finished());

        reservation.commit().unwrap();
        let snapshot = close.await.unwrap().unwrap();
        assert_eq!(
            snapshot
                .threads
                .iter()
                .find(|thread| thread.thread_id == late_child.thread_id)
                .unwrap()
                .status,
            AgentStatusV2::Shutdown
        );
    }

    #[tokio::test]
    async fn shutdown_parent_rejects_new_spawn_without_pending_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let dispatch = dispatch(&dir);
        let parent = committed_child(&dispatch, "parent");
        desktop_control(&dispatch, &memory_dir)
            .close_subtree("root-session", "/root/parent")
            .await
            .unwrap();
        let before = dispatch.control.snapshot().unwrap();

        assert!(dispatch
            .control
            .reserve_spawn(&parent.canonical_path, "after_shutdown")
            .is_err());

        assert_eq!(dispatch.control.snapshot().unwrap(), before);
        let pool = agent_db::sqlx::SqlitePool::connect(
            &format!("sqlite:{}", dir.path().join("subagents-v2.db").display())
        ).await.unwrap();
        let (count,): (i64,) = agent_db::sqlx::query_as(
            "SELECT COUNT(*) FROM agent_threads WHERE status_kind = 'pending_init'",
        )
        .fetch_one(&pool).await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn cancelled_close_releases_prefix_admission_barrier() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let dispatch = dispatch(&dir);
        let parent = committed_child(&dispatch, "parent");
        let hook = super::super::agent_runtime::AckSubscribeHook {
            entered: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
        };
        let mut desktop = desktop_control(&dispatch, &memory_dir);
        desktop.set_close_barrier_hook(Some(hook.clone()));
        let desktop = Arc::new(desktop);
        let close = tokio::spawn({
            let desktop = Arc::clone(&desktop);
            async move { desktop.close_subtree("root-session", "/root/parent").await }
        });
        hook.entered.notified().await;

        close.abort();
        assert!(close.await.unwrap_err().is_cancelled());
        assert!(!dispatch
            .control
            .is_path_closing(&parent.canonical_path)
            .unwrap());
        dispatch
            .control
            .reserve_spawn(&parent.canonical_path, "allowed_after_cancel")
            .unwrap()
            .abort()
            .unwrap();
        assert_ne!(
            dispatch
                .control
                .resolve_desktop_target(parent.canonical_path.as_str())
                .unwrap()
                .status,
            AgentStatusV2::Shutdown
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn desktop_followup_reuses_runtime_manager_handoff() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut dispatch = dispatch(&dir);
        dispatch.chat_override = Some(scripted_chat("done"));
        let spawned = AgentThreadDispatch::spawn_agent(&dispatch, spawn_request(&memory_dir))
            .await
            .unwrap();
        while dispatch
            .runtime_manager
            .is_running(&spawned.thread.thread_id)
        {
            tokio::task::yield_now().await;
        }
        let desktop = desktop_control(&dispatch, &memory_dir);

        desktop
            .followup(
                "root-session",
                "/root/worker",
                "desktop continuation".into(),
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
                .resolve_desktop_target("/root/worker")
                .unwrap()
                .status,
            AgentStatusV2::Completed { .. }
        ));
        assert!(sessions
            .get_messages(&spawned.thread.session_id)
            .unwrap()
            .iter()
            .any(|message| message.content.as_deref() == Some("desktop continuation")));
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

    #[tokio::test(flavor = "current_thread")]
    async fn spawn_start_hook_fires_once_after_acceptance_and_not_for_followup() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut dispatch = dispatch(&dir);
        dispatch.chat_override = Some(scripted_chat("done"));
        let bus = Arc::new(hooks::PluginHookBus::new());
        bus.register(hooks::SUBAGENT_START, |_| panic!("injected hook panic"));
        let payloads = Arc::new(Mutex::new(Vec::<hooks::HookPayload>::new()));
        let observed = Arc::clone(&payloads);
        bus.register(hooks::SUBAGENT_START, move |payload| {
            observed.lock().unwrap().push(payload.clone());
            hooks::HookOutcome::Continue
        });

        let mut request = request_with_hook_bus(&memory_dir, Arc::clone(&bus));
        let long_task = "sensitive".repeat(40);
        request.request.message = long_task.clone();
        let spawned = AgentThreadDispatch::spawn_agent(&dispatch, request)
            .await
            .unwrap();
        while dispatch
            .runtime_manager
            .is_running(&spawned.thread.thread_id)
        {
            tokio::task::yield_now().await;
        }
        AgentThreadDispatch::followup_task(
            &dispatch,
            MessageAgentV2Request {
                target: spawned.thread.canonical_path.to_string(),
                message: "continue once".into(),
            }
            .into(),
        )
        .await
        .unwrap();
        while dispatch
            .runtime_manager
            .is_running(&spawned.thread.thread_id)
        {
            tokio::task::yield_now().await;
        }

        let payloads = payloads.lock().unwrap();
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0].session_id, spawned.thread.session_id);
        assert_eq!(
            payloads[0].agent_id.as_deref(),
            Some(spawned.thread.thread_id.as_str())
        );
        assert_eq!(payloads[0].agent_type.as_deref(), Some("default"));
        assert!(payloads[0].detail.contains("path=/root/worker"));
        assert!(!payloads[0].detail.contains(&long_task));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn subagent_stop_keep_going_continues_the_same_turn() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut dispatch = dispatch(&dir);
        dispatch.chat_override = Some(scripted_chat("done"));
        let bus = Arc::new(hooks::PluginHookBus::new());
        let stop_count = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&stop_count);
        bus.register(hooks::SUBAGENT_STOP, move |_| {
            if observed.fetch_add(1, Ordering::SeqCst) == 0 {
                hooks::HookOutcome::KeepGoing("verify once more".into())
            } else {
                hooks::HookOutcome::Continue
            }
        });

        let spawned =
            AgentThreadDispatch::spawn_agent(&dispatch, request_with_hook_bus(&memory_dir, bus))
                .await
                .unwrap();
        while dispatch
            .runtime_manager
            .is_running(&spawned.thread.thread_id)
        {
            tokio::task::yield_now().await;
        }

        assert_eq!(stop_count.load(Ordering::SeqCst), 2);
        let messages = sessions.get_messages(&spawned.thread.session_id).unwrap();
        assert_eq!(
            messages
                .iter()
                .map(|message| message.role.as_str())
                .collect::<Vec<_>>(),
            vec!["user", "assistant", "user", "assistant"]
        );
        assert!(messages[2]
            .content
            .as_deref()
            .unwrap()
            .contains("verify once more"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn stop_hook_observes_each_terminal_turn_and_close_does_not_duplicate_it() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut dispatch = dispatch(&dir);
        dispatch.chat_override = Some(scripted_chat("done"));
        let bus = Arc::new(hooks::PluginHookBus::new());
        bus.register(hooks::SUBAGENT_STOP, |_| panic!("injected hook panic"));
        let sensitive_probe = Arc::new(());
        let sensitive_probe_weak = Arc::downgrade(&sensitive_probe);
        let captured_probe = Arc::clone(&sensitive_probe);
        bus.register(hooks::SUBAGENT_STOP, move |_| {
            let _ = Arc::strong_count(&captured_probe);
            hooks::HookOutcome::Continue
        });
        drop(sensitive_probe);
        let payloads = Arc::new(Mutex::new(Vec::<hooks::HookPayload>::new()));
        let observed = Arc::clone(&payloads);
        bus.register(hooks::SUBAGENT_STOP, move |payload| {
            observed.lock().unwrap().push(payload.clone());
            hooks::HookOutcome::Block("observer outcomes are ignored".into())
        });
        let spawned =
            AgentThreadDispatch::spawn_agent(&dispatch, request_with_hook_bus(&memory_dir, bus))
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
                .resolve_desktop_target("/root/worker")
                .unwrap()
                .status,
            AgentStatusV2::Completed { .. }
        ));
        assert_eq!(payloads.lock().unwrap().len(), 1);

        let desktop = desktop_control(&dispatch, &memory_dir);
        let (first, second) = tokio::join!(
            desktop.close_subtree("root-session", "/root/worker"),
            desktop.close_subtree("root-session", "/root/worker")
        );
        first.unwrap();
        second.unwrap();
        desktop
            .close_subtree("root-session", "/root/worker")
            .await
            .unwrap();

        let payloads = payloads.lock().unwrap();
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0].session_id, spawned.thread.session_id);
        assert!(payloads[0].detail.contains("path=/root/worker"));
        assert!(dispatch
            .runtime_requests
            .get(&spawned.thread.thread_id)
            .unwrap()
            .is_none());
        assert!(sensitive_probe_weak.upgrade().is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn interrupted_and_errored_turns_emit_stop_at_turn_end_not_close() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut dispatch = dispatch(&dir);
        let bus = Arc::new(hooks::PluginHookBus::new());
        let stop_count = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&stop_count);
        bus.register(hooks::SUBAGENT_STOP, move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            hooks::HookOutcome::Continue
        });

        dispatch.chat_override = Some(immediate_error_chat());
        let errored = AgentThreadDispatch::spawn_agent(
            &dispatch,
            request_with_hook_bus(&memory_dir, Arc::clone(&bus)),
        )
        .await
        .unwrap();
        while dispatch
            .runtime_manager
            .is_running(&errored.thread.thread_id)
        {
            tokio::task::yield_now().await;
        }
        assert!(matches!(
            dispatch
                .control
                .resolve_desktop_target("/root/worker")
                .unwrap()
                .status,
            AgentStatusV2::Errored { .. }
        ));
        assert_eq!(stop_count.load(Ordering::SeqCst), 1);
        assert!(dispatch
            .runtime_requests
            .get(&errored.thread.thread_id)
            .unwrap()
            .is_some());

        let mut interrupted_request = request_with_hook_bus(&memory_dir, Arc::clone(&bus));
        interrupted_request.request.task_name = "interrupted".into();
        dispatch.chat_override = Some(pending_chat());
        let interrupted = AgentThreadDispatch::spawn_agent(&dispatch, interrupted_request)
            .await
            .unwrap();
        AgentThreadDispatch::interrupt_agent(
            &dispatch,
            InterruptAgentV2Request {
                target: interrupted.thread.canonical_path.to_string(),
            },
        )
        .await
        .unwrap();
        while dispatch
            .runtime_manager
            .is_running(&interrupted.thread.thread_id)
        {
            tokio::task::yield_now().await;
        }
        assert_eq!(stop_count.load(Ordering::SeqCst), 2);
        assert!(dispatch
            .runtime_requests
            .get(&interrupted.thread.thread_id)
            .unwrap()
            .is_some());

        let desktop = desktop_control(&dispatch, &memory_dir);
        desktop
            .close_subtree("root-session", errored.thread.canonical_path.as_str())
            .await
            .unwrap();
        desktop
            .close_subtree("root-session", interrupted.thread.canonical_path.as_str())
            .await
            .unwrap();
        assert_eq!(stop_count.load(Ordering::SeqCst), 2);
        assert!(dispatch
            .runtime_requests
            .get(&errored.thread.thread_id)
            .unwrap()
            .is_none());
        assert!(dispatch
            .runtime_requests
            .get(&interrupted.thread.thread_id)
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn spawn_preflight_failure_rolls_back_path_row_edge_and_runtime_request() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let dispatch = dispatch(&dir);
        let bus = Arc::new(hooks::PluginHookBus::new());
        let start_count = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&start_count);
        bus.register(hooks::SUBAGENT_START, move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            hooks::HookOutcome::Continue
        });
        let mut request = request_with_hook_bus(&memory_dir, bus);
        request.runtime.chat_targets.clear();

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
        let pool = agent_db::sqlx::SqlitePool::connect(
            &format!("sqlite:{}", dir.path().join("subagents-v2.db").display())
        ).await.unwrap();
        let (child_rows,): (i64,) = agent_db::sqlx::query_as(
            "SELECT COUNT(*) FROM agent_threads WHERE canonical_path <> '/root'",
        )
        .fetch_one(&pool).await.unwrap();
        let (edge_rows,): (i64,) = agent_db::sqlx::query_as(
            "SELECT COUNT(*) FROM agent_spawn_edges",
        )
        .fetch_one(&pool).await.unwrap();
        assert_eq!(child_rows, 0);
        assert_eq!(edge_rows, 0);
        assert!(dispatch
            .runtime_requests
            .requests
            .lock()
            .unwrap()
            .is_empty());
        assert_eq!(start_count.load(Ordering::SeqCst), 0);
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
        let bus = Arc::new(hooks::PluginHookBus::new());
        let start_count = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&start_count);
        bus.register(hooks::SUBAGENT_START, move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            hooks::HookOutcome::Continue
        });

        let error =
            AgentThreadDispatch::spawn_agent(&dispatch, request_with_hook_bus(&memory_dir, bus))
                .await
                .unwrap_err();
        assert!(
            format!("{error:#}").contains("active agent execution limit"),
            "{error:#}"
        );
        assert_eq!(control.identity_count().unwrap(), 0);
        assert_eq!(graph.snapshot("root-session").unwrap().threads.len(), 1);
        assert_eq!(start_count.load(Ordering::SeqCst), 0);
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
        let bus = Arc::new(hooks::PluginHookBus::new());
        let start_count = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&start_count);
        bus.register(hooks::SUBAGENT_START, move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            hooks::HookOutcome::Continue
        });
        let dispatch = Arc::new(dispatch);
        let spawn = tokio::spawn({
            let dispatch = Arc::clone(&dispatch);
            let memory_dir = memory_dir.clone();
            let bus = Arc::clone(&bus);
            async move {
                AgentThreadDispatch::spawn_agent(
                    &*dispatch,
                    request_with_hook_bus(&memory_dir, bus),
                )
                .await
            }
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
        let pool = agent_db::sqlx::SqlitePool::connect(
            &format!("sqlite:{}", dir.path().join("subagents-v2.db").display())
        ).await.unwrap();
        let (child_rows,): (i64,) = agent_db::sqlx::query_as(
            "SELECT COUNT(*) FROM agent_threads WHERE canonical_path <> '/root'",
        )
        .fetch_one(&pool).await.unwrap();
        let (edge_rows,): (i64,) = agent_db::sqlx::query_as(
            "SELECT COUNT(*) FROM agent_spawn_edges",
        )
        .fetch_one(&pool).await.unwrap();
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
        assert_eq!(start_count.load(Ordering::SeqCst), 0);

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
    async fn cancelling_spawn_after_permit_before_turn_started_releases_identity() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut dispatch = dispatch(&dir);
        dispatch.chat_override = Some(scripted_chat("must not survive cancellation"));
        let hook = super::super::agent_runtime::AckSubscribeHook {
            entered: Arc::new(tokio::sync::Notify::new()),
            release: Arc::new(tokio::sync::Notify::new()),
        };
        dispatch
            .runtime_manager
            .set_after_startup_permit_hook(Some(hook.clone()));
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
            loop {
                if !dispatch.runtime_manager.is_running(&child.thread_id)
                    && dispatch.control.identity_count().unwrap() == 0
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

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
        let pool = agent_db::sqlx::SqlitePool::connect(
            &format!("sqlite:{}", dir.path().join("subagents-v2.db").display())
        ).await.unwrap();
        let (thread_count,): (i64,) = agent_db::sqlx::query_as(
            "SELECT COUNT(*) FROM agent_threads WHERE canonical_path <> '/root'",
        )
        .fetch_one(&pool).await.unwrap();
        assert_eq!(thread_count, 0);
        let (edge_count,): (i64,) = agent_db::sqlx::query_as(
            "SELECT COUNT(*) FROM agent_spawn_edges",
        )
        .fetch_one(&pool).await.unwrap();
        assert_eq!(edge_count, 0);

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
    async fn interrupt_completed_thread_is_an_idempotent_noop() {
        let dir = tempfile::tempdir().unwrap();
        let dispatch = dispatch(&dir);
        let child = committed_child(&dispatch, "worker");
        dispatch
            .control
            .record_runner_event(
                &child.thread_id,
                RunnerEvent::TurnCompleted {
                    turn_id: "completed-turn".into(),
                    last_message: "done".into(),
                },
            )
            .unwrap();

        let result = AgentThreadDispatch::interrupt_agent(
            &dispatch,
            InterruptAgentV2Request {
                target: child.thread_id.clone(),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            result.previous_status,
            AgentStatusV2::Completed {
                last_message: "done".into()
            }
        );
        assert_eq!(result.thread.status, result.previous_status);
    }

    #[tokio::test]
    async fn wait_observes_mailbox_activity_already_pending_at_call_time() {
        let dir = tempfile::tempdir().unwrap();
        let dispatch = dispatch(&dir);
        let child = committed_child(&dispatch, "worker");
        let child_dispatch = dispatch_for_thread(&dispatch, &child);
        AgentThreadDispatch::send_message(
            &dispatch,
            MessageAgentV2Request {
                target: child.thread_id.clone(),
                message: "already queued".into(),
            },
        )
        .await
        .unwrap();

        let result = tokio::time::timeout(
            Duration::from_millis(100),
            AgentThreadDispatch::wait_agent(
                &child_dispatch,
                WaitAgentV2Request {
                    timeout_ms: Some(10_000),
                },
            ),
        )
        .await
        .expect("pending mailbox activity must complete wait immediately")
        .unwrap();
        assert_eq!(result.message, "Wait completed.");
        assert!(!result.timed_out);
    }

    #[tokio::test]
    async fn wait_mailbox_scope_is_the_recipient_not_root_or_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let dispatch = dispatch(&dir);
        let alpha = committed_child(&dispatch, "alpha");
        let beta = committed_child(&dispatch, "beta");
        let alpha_dispatch = dispatch_for_thread(&dispatch, &alpha);
        let beta_dispatch = dispatch_for_thread(&dispatch, &beta);

        AgentThreadDispatch::send_message(
            &dispatch,
            MessageAgentV2Request {
                target: beta.thread_id.clone(),
                message: "only beta".into(),
            },
        )
        .await
        .unwrap();

        let mut root_wait = Box::pin(AgentThreadDispatch::wait_agent(
            &dispatch,
            WaitAgentV2Request {
                timeout_ms: Some(10_000),
            },
        ));
        assert!(
            tokio::time::timeout(Duration::from_millis(20), root_wait.as_mut())
                .await
                .is_err()
        );
        dispatch
            .control
            .persist_main_steer(&AgentPath::root(), "new root input".into())
            .unwrap();
        dispatch.control.notify_main_steer();
        let root_result = tokio::time::timeout(Duration::from_millis(100), root_wait.as_mut())
            .await
            .expect("a later root input should wake the existing wait")
            .unwrap();
        assert_eq!(root_result.message, "Wait interrupted by new input.");

        let mut alpha_wait = Box::pin(AgentThreadDispatch::wait_agent(
            &alpha_dispatch,
            WaitAgentV2Request {
                timeout_ms: Some(10_000),
            },
        ));
        assert!(
            tokio::time::timeout(Duration::from_millis(20), alpha_wait.as_mut())
                .await
                .is_err()
        );
        AgentThreadDispatch::send_message(
            &dispatch,
            MessageAgentV2Request {
                target: alpha.thread_id.clone(),
                message: "now alpha".into(),
            },
        )
        .await
        .unwrap();
        let alpha_result = tokio::time::timeout(Duration::from_millis(100), alpha_wait.as_mut())
            .await
            .expect("a later matching mailbox should wake the existing wait")
            .unwrap();
        assert_eq!(alpha_result.message, "Wait completed.");

        let beta_result = AgentThreadDispatch::wait_agent(
            &beta_dispatch,
            WaitAgentV2Request {
                timeout_ms: Some(10_000),
            },
        )
        .await
        .unwrap();
        assert_eq!(beta_result.message, "Wait completed.");
    }

    #[tokio::test]
    async fn wait_final_status_is_visible_only_to_the_direct_parent() {
        let dir = tempfile::tempdir().unwrap();
        let dispatch = dispatch(&dir);
        let parent = committed_child(&dispatch, "parent");
        let sibling = committed_child(&dispatch, "sibling");
        let leaf_reservation = dispatch
            .control
            .reserve_spawn(&parent.canonical_path, "leaf")
            .unwrap();
        let leaf = leaf_reservation.thread().clone();
        leaf_reservation.commit().unwrap();
        let parent_dispatch = dispatch_for_thread(&dispatch, &parent);
        let sibling_dispatch = dispatch_for_thread(&dispatch, &sibling);

        dispatch
            .control
            .record_runner_event(
                &leaf.thread_id,
                RunnerEvent::TurnCompleted {
                    turn_id: "leaf-turn".into(),
                    last_message: "leaf done".into(),
                },
            )
            .unwrap();

        let parent_result = AgentThreadDispatch::wait_agent(
            &parent_dispatch,
            WaitAgentV2Request {
                timeout_ms: Some(10_000),
            },
        )
        .await
        .unwrap();
        assert_eq!(parent_result.message, "Wait completed.");

        for unrelated in [&dispatch, &sibling_dispatch] {
            let mut wait = Box::pin(AgentThreadDispatch::wait_agent(
                unrelated,
                WaitAgentV2Request {
                    timeout_ms: Some(10_000),
                },
            ));
            assert!(
                tokio::time::timeout(Duration::from_millis(20), wait.as_mut())
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn wait_main_steer_is_visible_only_to_root() {
        let dir = tempfile::tempdir().unwrap();
        let dispatch = dispatch(&dir);
        let child = committed_child(&dispatch, "worker");
        let child_dispatch = dispatch_for_thread(&dispatch, &child);
        dispatch
            .control
            .persist_main_steer(&AgentPath::root(), "new root input".into())
            .unwrap();
        dispatch.control.notify_main_steer();

        let root_result = AgentThreadDispatch::wait_agent(
            &dispatch,
            WaitAgentV2Request {
                timeout_ms: Some(10_000),
            },
        )
        .await
        .unwrap();
        assert_eq!(root_result.message, "Wait interrupted by new input.");

        let mut child_wait = Box::pin(AgentThreadDispatch::wait_agent(
            &child_dispatch,
            WaitAgentV2Request {
                timeout_ms: Some(10_000),
            },
        ));
        assert!(
            tokio::time::timeout(Duration::from_millis(20), child_wait.as_mut())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn concurrent_waits_share_the_same_relevant_wakeup() {
        let dir = tempfile::tempdir().unwrap();
        let dispatch = dispatch(&dir);
        let child = committed_child(&dispatch, "worker");
        let child_dispatch = dispatch_for_thread(&dispatch, &child);

        let first = AgentThreadDispatch::wait_agent(
            &child_dispatch,
            WaitAgentV2Request {
                timeout_ms: Some(10_000),
            },
        );
        let second = AgentThreadDispatch::wait_agent(
            &child_dispatch,
            WaitAgentV2Request {
                timeout_ms: Some(10_000),
            },
        );
        let publish = async {
            tokio::task::yield_now().await;
            AgentThreadDispatch::send_message(
                &dispatch,
                MessageAgentV2Request {
                    target: child.thread_id.clone(),
                    message: "wake both waits".into(),
                },
            )
            .await
            .unwrap();
        };

        let (first, second, ()) = tokio::time::timeout(Duration::from_millis(250), async {
            tokio::join!(first, second, publish)
        })
        .await
        .expect("both waits should observe the same caller input");
        assert_eq!(first.unwrap().message, "Wait completed.");
        assert_eq!(second.unwrap().message, "Wait completed.");
    }

    #[tokio::test]
    async fn runtime_terminated_wakes_only_the_direct_parent() {
        let dir = tempfile::tempdir().unwrap();
        let dispatch = dispatch(&dir);
        let parent = committed_child(&dispatch, "parent");
        let sibling = committed_child(&dispatch, "sibling");
        let leaf_reservation = dispatch
            .control
            .reserve_spawn(&parent.canonical_path, "leaf")
            .unwrap();
        let leaf = leaf_reservation.thread().clone();
        leaf_reservation.commit().unwrap();
        let parent_dispatch = dispatch_for_thread(&dispatch, &parent);
        let sibling_dispatch = dispatch_for_thread(&dispatch, &sibling);

        dispatch
            .control
            .record_runner_event(&leaf.thread_id, RunnerEvent::RuntimeTerminated)
            .unwrap();

        let parent_result = AgentThreadDispatch::wait_agent(
            &parent_dispatch,
            WaitAgentV2Request {
                timeout_ms: Some(10_000),
            },
        )
        .await
        .unwrap();
        assert_eq!(parent_result.message, "Wait completed.");

        for unrelated in [&dispatch, &sibling_dispatch] {
            let mut wait = Box::pin(AgentThreadDispatch::wait_agent(
                unrelated,
                WaitAgentV2Request {
                    timeout_ms: Some(10_000),
                },
            ));
            assert!(
                tokio::time::timeout(Duration::from_millis(20), wait.as_mut())
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn concurrent_waits_observe_an_already_pending_final_result() {
        let dir = tempfile::tempdir().unwrap();
        let dispatch = dispatch(&dir);
        let parent = committed_child(&dispatch, "parent");
        let leaf_reservation = dispatch
            .control
            .reserve_spawn(&parent.canonical_path, "leaf")
            .unwrap();
        let leaf = leaf_reservation.thread().clone();
        leaf_reservation.commit().unwrap();
        let parent_dispatch = dispatch_for_thread(&dispatch, &parent);
        dispatch
            .control
            .record_runner_event(
                &leaf.thread_id,
                RunnerEvent::TurnCompleted {
                    turn_id: "leaf-turn".into(),
                    last_message: "done".into(),
                },
            )
            .unwrap();
        let pending = dispatch
            .control
            .drain_mailbox(&parent.canonical_path)
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind, subagents::MailboxKind::Result);

        let waits = async {
            tokio::join!(
                AgentThreadDispatch::wait_agent(
                    &parent_dispatch,
                    WaitAgentV2Request {
                        timeout_ms: Some(10_000),
                    },
                ),
                AgentThreadDispatch::wait_agent(
                    &parent_dispatch,
                    WaitAgentV2Request {
                        timeout_ms: Some(10_000),
                    },
                )
            )
        };
        let (first, second) = tokio::time::timeout(Duration::from_millis(250), waits)
            .await
            .expect("both waits should observe the already-pending final result");
        assert_eq!(first.unwrap().message, "Wait completed.");
        assert_eq!(second.unwrap().message, "Wait completed.");
    }

    #[tokio::test]
    async fn each_followup_turn_completion_notifies_parent_after_prior_ack() {
        let dir = tempfile::tempdir().unwrap();
        let dispatch = dispatch(&dir);
        let parent = committed_child(&dispatch, "parent");
        let leaf_reservation = dispatch
            .control
            .reserve_spawn(&parent.canonical_path, "leaf")
            .unwrap();
        let leaf = leaf_reservation.thread().clone();
        leaf_reservation.commit().unwrap();
        let parent_dispatch = dispatch_for_thread(&dispatch, &parent);

        dispatch
            .control
            .record_runner_event(
                &leaf.thread_id,
                RunnerEvent::TurnCompleted {
                    turn_id: "turn-1".into(),
                    last_message: "first".into(),
                },
            )
            .unwrap();
        let first_wait = AgentThreadDispatch::wait_agent(
            &parent_dispatch,
            WaitAgentV2Request {
                timeout_ms: Some(10_000),
            },
        )
        .await
        .unwrap();
        assert_eq!(first_wait.message, "Wait completed.");
        let first = dispatch
            .control
            .drain_mailbox(&parent.canonical_path)
            .unwrap();
        assert_eq!(first.len(), 1);
        dispatch
            .control
            .ack_mailbox(&parent.canonical_path, first[0].sequence)
            .unwrap();

        dispatch
            .control
            .record_runner_event(
                &leaf.thread_id,
                RunnerEvent::TurnStarted {
                    turn_id: "turn-2".into(),
                },
            )
            .unwrap();
        dispatch
            .control
            .record_runner_event(
                &leaf.thread_id,
                RunnerEvent::TurnCompleted {
                    turn_id: "turn-2".into(),
                    last_message: "second".into(),
                },
            )
            .unwrap();
        let second_wait = AgentThreadDispatch::wait_agent(
            &parent_dispatch,
            WaitAgentV2Request {
                timeout_ms: Some(10_000),
            },
        )
        .await
        .unwrap();
        assert_eq!(second_wait.message, "Wait completed.");
        let second = dispatch
            .control
            .drain_mailbox(&parent.canonical_path)
            .unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(
            second[0].message_id,
            format!("agent-final:{}:turn-2", leaf.thread_id)
        );
    }

    #[tokio::test]
    async fn acknowledged_steer_event_does_not_stale_wake_root() {
        let dir = tempfile::tempdir().unwrap();
        let dispatch = dispatch(&dir);
        let root = AgentPath::root();
        let steer = dispatch
            .control
            .persist_main_steer(&root, "already consumed".into())
            .unwrap();
        dispatch.control.notify_main_steer();
        dispatch.control.ack_mailbox(&root, steer.sequence).unwrap();

        let mut wait = Box::pin(AgentThreadDispatch::wait_agent(
            &dispatch,
            WaitAgentV2Request {
                timeout_ms: Some(10_000),
            },
        ));
        assert!(
            tokio::time::timeout(Duration::from_millis(20), wait.as_mut())
                .await
                .is_err()
        );
        AgentThreadDispatch::send_message(
            &dispatch,
            MessageAgentV2Request {
                target: "/root".into(),
                message: "fresh input".into(),
            },
        )
        .await
        .unwrap();
        let result = tokio::time::timeout(Duration::from_millis(100), wait.as_mut())
            .await
            .expect("fresh root input should wake the existing wait")
            .unwrap();
        assert_eq!(result.message, "Wait completed.");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn active_followup_returns_after_queue_admission_before_turn_completion() {
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

        let result = tokio::time::timeout(
            Duration::from_millis(250),
            AgentThreadDispatch::followup_task(
                &*dispatch,
                MessageAgentV2Request {
                    target: spawned.thread.thread_id.clone(),
                    message: "consume at the next sampling boundary".into(),
                }
                .into(),
            ),
        )
        .await
        .expect("active followup should return after durable queue admission")
        .unwrap();
        assert!(result.queued);
        assert!(result.turn_triggered);
        assert!(dispatch
            .runtime_manager
            .is_running(&spawned.thread.thread_id));

        release_sampling.notify_one();
        while dispatch
            .runtime_manager
            .is_running(&spawned.thread.thread_id)
        {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn active_followup_consumed_at_sampling_boundary_does_not_start_empty_generation() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut dispatch = dispatch(&dir);
        let entered_sampling = Arc::new(tokio::sync::Notify::new());
        let release_sampling = Arc::new(tokio::sync::Notify::new());
        let saw_followup = Arc::new(AtomicBool::new(false));
        dispatch.chat_override = Some(gated_tool_then_final_chat(
            Arc::clone(&entered_sampling),
            Arc::clone(&release_sampling),
            Arc::clone(&saw_followup),
        ));
        let dispatch = Arc::new(dispatch);
        let spawned = AgentThreadDispatch::spawn_agent(&*dispatch, spawn_request(&memory_dir))
            .await
            .unwrap();
        entered_sampling.notified().await;

        AgentThreadDispatch::followup_task(
            &*dispatch,
            MessageAgentV2Request {
                target: spawned.thread.thread_id.clone(),
                message: "consume in current turn".into(),
            }
            .into(),
        )
        .await
        .unwrap();
        release_sampling.notify_one();
        while dispatch
            .runtime_manager
            .is_running(&spawned.thread.thread_id)
        {
            tokio::task::yield_now().await;
        }

        assert!(saw_followup.load(Ordering::SeqCst));
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
                .filter(|event| matches!(event.event, RunnerEvent::TurnStarted { .. }))
                .count(),
            1
        );
        assert!(!events
            .iter()
            .any(|event| matches!(event.event, RunnerEvent::TurnErrored { .. })));
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
            }
            .into(),
        )
        .await
        .unwrap_err();
        assert!(followup.to_string().contains("runtime configuration"));
        let mailbox = dispatch
            .control
            .drain_mailbox(&child.canonical_path)
            .unwrap();
        assert_eq!(mailbox.len(), 1);
        assert!(!mailbox[0].trigger_turn);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cold_followup_recovers_exact_model_effort_and_current_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let project = dir.path().join("project");
        std::fs::create_dir_all(memory_dir.join(".astro")).unwrap();
        std::fs::create_dir_all(project.join(".astro/agents")).unwrap();
        std::fs::write(
            memory_dir.join(".astro/config.toml"),
            format!(
                "[projects.{:?}]\ntrust_level = 'trusted'\n",
                project.to_string_lossy()
            ),
        )
        .unwrap();
        let definition = project.join(".astro/agents/reviewer.toml");
        std::fs::write(
            &definition,
            "name = \"reviewer\"\ndescription = \"review\"\ndeveloper_instructions = \"review\"\nmodel = \"openai:original-model\"\n",
        )
        .unwrap();
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut initial = dispatch(&dir);
        initial.chat_override = Some(scripted_chat("initial"));
        let mut spawn = spawn_request(&memory_dir);
        spawn.request.agent_type = Some("reviewer".into());
        spawn.runtime.project_root = Some(project);
        spawn.request.reasoning_effort = Some("max".into());
        let mut runtime_material = spawn.runtime.clone();
        runtime_material.chat_targets[0] = types::ChatTarget {
            provider_id: "current-anthropic".into(),
            backend_id: "anthropic".into(),
            model: "claude-current".into(),
            api_key: "anthropic-key-must-not-leak".into(),
            base_url: "https://anthropic.invalid".into(),
            api_mode: String::new(),
        };
        runtime_material.chat_targets.push(types::ChatTarget {
            provider_id: "current-openai".into(),
            backend_id: "openai".into(),
            model: "openai-current".into(),
            api_key: "restarted-openai-key".into(),
            base_url: "https://openai-current.invalid".into(),
            api_mode: String::new(),
        });
        let child = AgentThreadDispatch::spawn_agent(&initial, spawn)
            .await
            .unwrap()
            .thread;
        while initial.runtime_manager.is_running(&child.thread_id) {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            initial
                .control
                .runtime_descriptor(&child.thread_id)
                .unwrap()
                .unwrap()
                .model
                .as_deref(),
            Some("openai:original-model")
        );
        std::fs::write(
            &definition,
            "name = \"reviewer\"\ndescription = \"review\"\ndeveloper_instructions = \"review changed\"\nmodel = \"openai:changed-model\"\n",
        )
        .unwrap();

        let control = AgentControl::open(
            "root-session".into(),
            AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap(),
            Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .unwrap();
        let mut recovered = DefaultAgentThreadDispatch::for_test(
            control,
            AgentPath::root(),
            "root-session".into(),
            Arc::new(AgentRuntimeManager::default()),
        );
        let captured = Arc::new(Mutex::new(Vec::new()));
        recovered.chat_override = Some(capturing_config_chat(Arc::clone(&captured)));

        AgentThreadDispatch::followup_task(
            &recovered,
            FollowupAgentDispatchRequest {
                request: MessageAgentV2Request {
                    target: child.canonical_path.to_string(),
                    message: "resume exactly once".into(),
                },
                runtime: Some(runtime_material),
            },
        )
        .await
        .unwrap();
        while recovered.runtime_manager.is_running(&child.thread_id) {
            tokio::task::yield_now().await;
        }

        let configs = captured.lock().unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].additional_params["reasoning_effort"], "max");
        drop(configs);
        let stored = recovered
            .runtime_requests
            .get(&child.thread_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            stored.runtime.model_request.model.as_deref(),
            Some("openai:original-model")
        );
        assert_eq!(
            stored.runtime.model_request.reasoning_effort.as_deref(),
            Some("max")
        );
        assert_eq!(stored.runtime.chat_targets[0].backend_id, "openai");
        assert_eq!(
            stored.runtime.chat_targets[0].api_key,
            "restarted-openai-key"
        );
        assert_eq!(
            stored.runtime.chat_targets[0].base_url,
            "https://openai-current.invalid"
        );
        let messages = sessions.get_messages(&child.session_id).unwrap();
        assert_eq!(
            messages
                .iter()
                .filter(|message| message.content.as_deref() == Some("resume exactly once"))
                .count(),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cold_followup_rejects_missing_descriptor_provider_without_ghosts() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut initial = dispatch(&dir);
        initial.chat_override = Some(scripted_chat("initial"));
        let mut spawn = spawn_request(&memory_dir);
        spawn.request.model = Some("openai:pinned-model".into());
        let mut runtime_material = spawn.runtime.clone();
        let child = AgentThreadDispatch::spawn_agent(&initial, spawn)
            .await
            .unwrap()
            .thread;
        while initial.runtime_manager.is_running(&child.thread_id) {
            tokio::task::yield_now().await;
        }

        runtime_material.chat_targets = vec![types::ChatTarget {
            provider_id: "current-anthropic".into(),
            backend_id: "anthropic".into(),
            model: "claude-current".into(),
            api_key: "anthropic-key-must-not-leak".into(),
            base_url: "https://anthropic.invalid".into(),
            api_mode: String::new(),
        }];
        let control = AgentControl::open(
            "root-session".into(),
            AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap(),
            Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .unwrap();
        let recovered = DefaultAgentThreadDispatch::for_test(
            control,
            AgentPath::root(),
            "root-session".into(),
            Arc::new(AgentRuntimeManager::default()),
        );

        let error = AgentThreadDispatch::followup_task(
            &recovered,
            FollowupAgentDispatchRequest {
                request: MessageAgentV2Request {
                    target: child.canonical_path.to_string(),
                    message: "must not use anthropic credentials".into(),
                },
                runtime: Some(runtime_material),
            },
        )
        .await
        .unwrap_err();

        assert!(format!("{error:#}").contains("model provider \"openai\""));
        assert!(recovered
            .control
            .drain_mailbox(&child.canonical_path)
            .unwrap()
            .is_empty());
        assert!(recovered
            .runtime_requests
            .requests
            .lock()
            .unwrap()
            .is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn migrated_v2_child_reports_legacy_recovery_boundary_without_ghosts() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut initial = dispatch(&dir);
        initial.chat_override = Some(scripted_chat("initial"));
        let spawn = spawn_request(&memory_dir);
        let runtime_material = spawn.runtime.clone();
        let child = AgentThreadDispatch::spawn_agent(&initial, spawn)
            .await
            .unwrap()
            .thread;
        while initial.runtime_manager.is_running(&child.thread_id) {
            tokio::task::yield_now().await;
        }
        drop(initial);

        let graph_path = dir.path().join("subagents-v2.db");
        let pool = agent_db::sqlx::SqlitePool::connect(
            &format!("sqlite:{}", graph_path.display())
        ).await.unwrap();
        sqlx::raw_sql(
            "DROP TABLE agent_runtime_descriptors;
             UPDATE schema_meta SET value = '2' WHERE key = 'schema_version';",
        )
        .execute(&pool).await.unwrap();
        drop(pool);

        let control = AgentControl::open(
            "root-session".into(),
            AgentGraphStore::open(graph_path).await.unwrap(),
            Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .await
        .unwrap();
        let recovered = DefaultAgentThreadDispatch::for_test(
            control,
            AgentPath::root(),
            "root-session".into(),
            Arc::new(AgentRuntimeManager::default()),
        );

        let error = AgentThreadDispatch::followup_task(
            &recovered,
            FollowupAgentDispatchRequest {
                request: MessageAgentV2Request {
                    target: child.canonical_path.to_string(),
                    message: "must not guess a legacy model".into(),
                },
                runtime: Some(runtime_material),
            },
        )
        .await
        .unwrap_err();

        assert!(format!("{error:#}").contains("predates resumable runtime descriptors"));
        assert!(recovered
            .control
            .drain_mailbox(&child.canonical_path)
            .unwrap()
            .is_empty());
        assert!(recovered
            .runtime_requests
            .requests
            .lock()
            .unwrap()
            .is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cold_followup_recovers_trusted_descriptor_from_early_v3_schema() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut initial = dispatch(&dir);
        initial.chat_override = Some(scripted_chat("initial"));
        let mut spawn = spawn_request(&memory_dir);
        spawn.request.model = Some("openai:pinned-v3-model".into());
        let mut runtime_material = spawn.runtime.clone();
        runtime_material.chat_targets[0].api_key = "current-key-after-v3-upgrade".into();
        let child = AgentThreadDispatch::spawn_agent(&initial, spawn)
            .await
            .unwrap()
            .thread;
        while initial.runtime_manager.is_running(&child.thread_id) {
            tokio::task::yield_now().await;
        }
        drop(initial);

        let graph_path = dir.path().join("subagents-v2.db");
        let pool = agent_db::sqlx::SqlitePool::connect(
            &format!("sqlite:{}", graph_path.display())
        ).await.unwrap();
        sqlx::raw_sql(
            "PRAGMA foreign_keys=OFF;
             ALTER TABLE agent_runtime_descriptors RENAME TO descriptors_v4;
             CREATE TABLE agent_runtime_descriptors (
                 thread_id TEXT PRIMARY KEY,
                 model TEXT,
                 reasoning_effort TEXT,
                 FOREIGN KEY(thread_id) REFERENCES agent_threads(thread_id) ON DELETE CASCADE
             );
             INSERT INTO agent_runtime_descriptors (thread_id, model, reasoning_effort)
                 SELECT thread_id, model, reasoning_effort FROM descriptors_v4;
             DROP TABLE descriptors_v4;
             UPDATE schema_meta SET value = '3' WHERE key = 'schema_version';
             PRAGMA foreign_keys=ON;",
        )
        .execute(&pool).await.unwrap();
        drop(pool);

        let control = AgentControl::open(
            "root-session".into(),
            AgentGraphStore::open(graph_path).await.unwrap(),
            Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .await
        .unwrap();
        let mut recovered = DefaultAgentThreadDispatch::for_test(
            control,
            AgentPath::root(),
            "root-session".into(),
            Arc::new(AgentRuntimeManager::default()),
        );
        recovered.chat_override = Some(scripted_chat("recovered"));

        AgentThreadDispatch::followup_task(
            &recovered,
            FollowupAgentDispatchRequest {
                request: MessageAgentV2Request {
                    target: child.canonical_path.to_string(),
                    message: "resume old v3 safely".into(),
                },
                runtime: Some(runtime_material),
            },
        )
        .await
        .unwrap();
        while recovered.runtime_manager.is_running(&child.thread_id) {
            tokio::task::yield_now().await;
        }

        let stored = recovered
            .runtime_requests
            .get(&child.thread_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            stored.runtime.model_request.model.as_deref(),
            Some("openai:pinned-v3-model")
        );
        assert_eq!(
            stored.runtime.chat_targets[0].api_key,
            "current-key-after-v3-upgrade"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn spawn_rejects_cross_provider_model_without_matching_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let dispatch = dispatch(&dir);
        let mut request = spawn_request(&memory_dir);
        request.request.model = Some("openai:pinned-model".into());
        request.runtime.chat_targets = vec![types::ChatTarget {
            provider_id: "current-anthropic".into(),
            backend_id: "anthropic".into(),
            model: "claude-current".into(),
            api_key: "anthropic-key-must-not-leak".into(),
            base_url: "https://anthropic.invalid".into(),
            api_mode: String::new(),
        }];

        let error = AgentThreadDispatch::spawn_agent(&dispatch, request)
            .await
            .unwrap_err();

        assert!(format!("{error:#}").contains("model provider \"openai\""));
        assert_eq!(dispatch.control.identity_count().unwrap(), 0);
        assert_eq!(
            dispatch
                .control
                .list_agents(&AgentPath::root(), None)
                .unwrap()
                .len(),
            1
        );
        assert!(dispatch
            .runtime_requests
            .requests
            .lock()
            .unwrap()
            .is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cold_nested_followup_rebuilds_ancestor_skill_inheritance() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let project = dir.path().join("project");
        std::fs::create_dir_all(memory_dir.join(".astro")).unwrap();
        std::fs::create_dir_all(project.join(".astro/agents")).unwrap();
        std::fs::write(
            memory_dir.join(".astro/config.toml"),
            format!(
                "[projects.{:?}]\ntrust_level = 'trusted'\n",
                project.to_string_lossy()
            ),
        )
        .unwrap();
        std::fs::write(
            project.join(".astro/agents/parent.toml"),
            "name = \"parent\"\ndescription = \"parent\"\ndeveloper_instructions = \"parent\"\n[[skills.config]]\npath = \"skills/parent/SKILL.md\"\nenabled = true\n",
        )
        .unwrap();
        std::fs::write(
            project.join(".astro/agents/leaf.toml"),
            "name = \"leaf\"\ndescription = \"leaf\"\ndeveloper_instructions = \"leaf\"\n[[skills.config]]\npath = \"skills/leaf/SKILL.md\"\nenabled = true\n",
        )
        .unwrap();
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut initial = dispatch(&dir);
        initial.chat_override = Some(scripted_chat("done"));
        let mut parent_request = spawn_request(&memory_dir);
        parent_request.request.task_name = "parent".into();
        parent_request.request.agent_type = Some("parent".into());
        parent_request.runtime.project_root = Some(project.clone());
        let root_material = parent_request.runtime.clone();
        let parent = AgentThreadDispatch::spawn_agent(&initial, parent_request)
            .await
            .unwrap()
            .thread;
        while initial.runtime_manager.is_running(&parent.thread_id) {
            tokio::task::yield_now().await;
        }
        let inherited = initial
            .runtime_requests
            .get(&parent.thread_id)
            .unwrap()
            .unwrap()
            .runtime
            .skills_config
            .iter()
            .map(|entry| (entry.path.clone(), entry.enabled))
            .collect();
        let child_dispatch = DefaultAgentThreadDispatch {
            control: Arc::clone(&initial.control),
            current_path: parent.canonical_path.clone(),
            current_thread_id: parent.thread_id.clone(),
            runtime_manager: Arc::clone(&initial.runtime_manager),
            runtime_requests: Arc::clone(&initial.runtime_requests),
            wait_cursor: Arc::clone(&initial.wait_cursor),
            chat_override: initial.chat_override.clone(),
            before_followup_atomic_hook: None,
        };
        let mut leaf_request = spawn_request(&memory_dir);
        leaf_request.request.task_name = "leaf".into();
        leaf_request.request.agent_type = Some("leaf".into());
        leaf_request.runtime.project_root = Some(project);
        leaf_request.runtime.inherited_skill_config = inherited;
        let leaf = AgentThreadDispatch::spawn_agent(&child_dispatch, leaf_request)
            .await
            .unwrap()
            .thread;
        while initial.runtime_manager.is_running(&leaf.thread_id) {
            tokio::task::yield_now().await;
        }

        let control = AgentControl::open(
            "root-session".into(),
            AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap(),
            Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .unwrap();
        let mut recovered = DefaultAgentThreadDispatch::for_test(
            control,
            AgentPath::root(),
            "root-session".into(),
            Arc::new(AgentRuntimeManager::default()),
        );
        recovered.chat_override = Some(scripted_chat("recovered"));
        AgentThreadDispatch::followup_task(
            &recovered,
            FollowupAgentDispatchRequest {
                request: MessageAgentV2Request {
                    target: leaf.canonical_path.to_string(),
                    message: "continue".into(),
                },
                runtime: Some(root_material),
            },
        )
        .await
        .unwrap();
        while recovered.runtime_manager.is_running(&leaf.thread_id) {
            tokio::task::yield_now().await;
        }
        let stored = recovered
            .runtime_requests
            .get(&leaf.thread_id)
            .unwrap()
            .unwrap();
        let paths = stored
            .runtime
            .skills_config
            .iter()
            .map(|entry| entry.path.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert!(paths.iter().any(|path| path.contains("skills/parent")));
        assert!(paths.iter().any(|path| path.contains("skills/leaf")));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cold_followup_rejects_sibling_runtime_material_without_ghosts() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut initial = dispatch(&dir);
        initial.chat_override = Some(scripted_chat("done"));

        let mut alpha_request = spawn_request(&memory_dir);
        alpha_request.request.task_name = "alpha".into();
        let mut sibling_material = alpha_request.runtime.clone();
        sibling_material.inherited_skill_config =
            vec![(PathBuf::from("skills/alpha-only/SKILL.md"), true)];
        sibling_material.parent_sandbox_mode = "danger-full-access".into();
        let alpha = AgentThreadDispatch::spawn_agent(&initial, alpha_request)
            .await
            .unwrap()
            .thread;
        while initial.runtime_manager.is_running(&alpha.thread_id) {
            tokio::task::yield_now().await;
        }

        let mut beta_request = spawn_request(&memory_dir);
        beta_request.request.task_name = "beta".into();
        let beta = AgentThreadDispatch::spawn_agent(&initial, beta_request)
            .await
            .unwrap()
            .thread;
        while initial.runtime_manager.is_running(&beta.thread_id) {
            tokio::task::yield_now().await;
        }
        let beta_dispatch = DefaultAgentThreadDispatch {
            control: Arc::clone(&initial.control),
            current_path: beta.canonical_path.clone(),
            current_thread_id: beta.thread_id.clone(),
            runtime_manager: Arc::clone(&initial.runtime_manager),
            runtime_requests: Arc::clone(&initial.runtime_requests),
            wait_cursor: Arc::clone(&initial.wait_cursor),
            chat_override: initial.chat_override.clone(),
            before_followup_atomic_hook: None,
        };
        let mut leaf_request = spawn_request(&memory_dir);
        leaf_request.request.task_name = "leaf".into();
        let leaf = AgentThreadDispatch::spawn_agent(&beta_dispatch, leaf_request)
            .await
            .unwrap()
            .thread;
        while initial.runtime_manager.is_running(&leaf.thread_id) {
            tokio::task::yield_now().await;
        }

        let control = AgentControl::open(
            "root-session".into(),
            AgentGraphStore::open(dir.path().join("subagents-v2.db")).unwrap(),
            Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .unwrap();
        let mut recovered = DefaultAgentThreadDispatch::for_test(
            control,
            alpha.canonical_path.clone(),
            alpha.thread_id.clone(),
            Arc::new(AgentRuntimeManager::default()),
        );
        recovered.chat_override = Some(scripted_chat("must not run"));

        let error = AgentThreadDispatch::followup_task(
            &recovered,
            FollowupAgentDispatchRequest {
                request: MessageAgentV2Request {
                    target: leaf.canonical_path.to_string(),
                    message: "must not inherit alpha context".into(),
                },
                runtime: Some(sibling_material),
            },
        )
        .await
        .unwrap_err();

        assert!(format!("{error:#}").contains("root or an ancestor"));
        assert!(recovered
            .control
            .drain_mailbox(&leaf.canonical_path)
            .unwrap()
            .is_empty());
        assert!(recovered
            .runtime_requests
            .requests
            .lock()
            .unwrap()
            .is_empty());
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

        let pool = agent_db::sqlx::SqlitePool::connect(
            &format!("sqlite:{}", dir.path().join("subagents-v2.db").display())
        ).await.unwrap();
        sqlx::raw_sql(
            "CREATE TRIGGER fail_first_followup_ack
             BEFORE UPDATE OF delivery_state ON agent_mailbox
             WHEN NEW.delivery_state = 'delivered'
             BEGIN
               SELECT RAISE(ABORT, 'injected first followup ack failure');
             END;",
        )
        .execute(&pool).await.unwrap();
        let first = AgentThreadDispatch::followup_task(
            &dispatch,
            MessageAgentV2Request {
                target: spawned.thread.canonical_path.to_string(),
                message: "S1".into(),
            }
            .into(),
        )
        .await
        .unwrap_err();
        assert!(format!("{first:#}").contains("injected first followup ack failure"));
        sqlx::raw_sql("DROP TRIGGER fail_first_followup_ack;")
            .execute(&pool).await.unwrap();

        AgentThreadDispatch::followup_task(
            &dispatch,
            MessageAgentV2Request {
                target: spawned.thread.canonical_path.to_string(),
                message: "S2".into(),
            }
            .into(),
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
            }
            .into(),
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
                }
                .into(),
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
                }
                .into(),
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
        tokio::time::timeout(Duration::from_secs(1), async {
            while !dispatch
                .control
                .drain_mailbox(&spawned.thread.canonical_path)
                .unwrap()
                .is_empty()
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("second generation must consume the admitted mailbox batch");
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
                }
                .into(),
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
                }
                .into(),
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
    async fn concurrent_cold_followups_recover_once_and_share_one_starting_generation() {
        let dir = tempfile::tempdir().unwrap();
        let memory_dir = dir.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions.ensure_session("root-session", "test").unwrap();
        let mut dispatch = dispatch(&dir);
        dispatch.chat_override = Some(scripted_chat("done"));
        let dispatch = Arc::new(dispatch);
        let spawn = spawn_request(&memory_dir);
        let runtime_material = spawn.runtime.clone();
        let spawned = AgentThreadDispatch::spawn_agent(&*dispatch, spawn)
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
        dispatch.runtime_requests.remove(&spawned.thread.thread_id);
        assert!(dispatch
            .runtime_requests
            .get(&spawned.thread.thread_id)
            .unwrap()
            .is_none());

        let admission = Arc::new(tokio::sync::Barrier::new(3));
        dispatch
            .runtime_manager
            .set_followup_admission_barrier(Some(Arc::clone(&admission)));
        let first_dispatch = Arc::clone(&dispatch);
        let first_target = spawned.thread.canonical_path.to_string();
        let first_runtime = runtime_material.clone();
        let first = tokio::spawn(async move {
            AgentThreadDispatch::followup_task(
                &*first_dispatch,
                FollowupAgentDispatchRequest {
                    request: MessageAgentV2Request {
                        target: first_target,
                        message: "idle one".into(),
                    },
                    runtime: Some(first_runtime),
                },
            )
            .await
        });
        let second_dispatch = Arc::clone(&dispatch);
        let second_target = spawned.thread.canonical_path.to_string();
        let second_runtime = runtime_material;
        let second = tokio::spawn(async move {
            AgentThreadDispatch::followup_task(
                &*second_dispatch,
                FollowupAgentDispatchRequest {
                    request: MessageAgentV2Request {
                        target: second_target,
                        message: "idle two".into(),
                    },
                    runtime: Some(second_runtime),
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
        assert_eq!(dispatch.runtime_requests.requests.lock().unwrap().len(), 1);
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
                    }
                    .into(),
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
            }
            .into(),
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
                }
                .into(),
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
                }
                .into(),
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
                }
                .into(),
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
                }
                .into(),
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
        let accepted = followup.await.unwrap().unwrap();
        assert!(accepted.queued);
        assert!(accepted.turn_triggered);
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
                }
                .into(),
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
        assert!(root_error.to_string().contains("root agent"));

        let child = committed_child(&dispatch, "worker");
        let child_dispatch = DefaultAgentThreadDispatch {
            control: Arc::clone(&dispatch.control),
            current_path: child.canonical_path.clone(),
            current_thread_id: child.thread_id.clone(),
            runtime_manager: Arc::clone(&dispatch.runtime_manager),
            runtime_requests: Arc::clone(&dispatch.runtime_requests),
            wait_cursor: Arc::clone(&dispatch.wait_cursor),
            chat_override: None,
            before_followup_atomic_hook: None,
        };
        let self_error = AgentThreadDispatch::interrupt_agent(
            &child_dispatch,
            InterruptAgentV2Request {
                target: child.thread_id,
            },
        )
        .await
        .unwrap_err();
        assert!(self_error.to_string().contains("interrupt itself"));
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
                dispatch
                    .control
                    .persist_main_steer(&AgentPath::root(), "new root input".into())
                    .unwrap();
                dispatch.control.notify_main_steer();
            }
        );
        let value = serde_json::to_value(result.unwrap()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "message": "Wait interrupted by new input.",
                "timed_out": false
            })
        );
    }

    #[test]
    fn custom_layers_preserve_unknown_parent_sandbox_and_merge_skills() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(dir.path().join(".astro")).unwrap();
        std::fs::create_dir_all(project.join(".astro/agents")).unwrap();
        std::fs::write(
            dir.path().join(".astro/config.toml"),
            format!(
                "[projects.{:?}]\ntrust_level = 'trusted'\n",
                project.to_string_lossy()
            ),
        )
        .unwrap();
        std::fs::write(
            project.join(".astro/agents/reviewer.toml"),
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
        let catalog = subagents::load_agent_configuration(dir.path(), Some(&project))
            .unwrap()
            .catalog;
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
                runtime: ParentRuntimeMaterial {
                    memory_dir: dir.path().to_path_buf(),
                    parent_agent_id: "parent-agent".into(),
                    parent_model: Some("openai:gpt-5.6".into()),
                    parent_sandbox_mode: "locked".into(),
                    inherited_skill_config: vec![(PathBuf::from("parent/SKILL.md"), true)],
                    chat_targets: Vec::new(),
                    project_root: Some(project),
                    workspace_roots: Vec::new(),
                    hook_runtime: None,
                    hook_bus: None,
                },
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
