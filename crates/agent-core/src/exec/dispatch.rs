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
    resolve_model_targets_for_model, AgentRuntimeManager, CloseThreadStart, FollowupAdmission,
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
            .model_targets()
            .first()
            .map(|target| format!("{}:{}", target.backend_id.trim(), target.model.trim()));
        let material = ParentRuntimeMaterial {
            memory_dir: session.memory_dir().to_path_buf(),
            parent_agent_id: session.agent_id().to_string(),
            parent_model,
            root_service_tier: session.thread_provider_options().service_tier,
            parent_sandbox_mode: session
                .permission_profile()
                .unwrap_or_else(|| types::WORKSPACE_PROFILE.to_string()),
            inherited_skill_config: session.skill_config_overrides(),
            model_targets: session.model_targets(),
            model_spec: session.model_spec(),
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
        sessions
            .delete_session_permanently(&self.child_session_id)
            .await?;
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
    responses_override: Option<crate::streaming::ResponsesOverride>,
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
            responses_override: None,
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
            responses_override: None,
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
            responses_override: self.responses_override.clone(),
            #[cfg(not(any(test, feature = "test-support")))]
            responses_override: None,
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
            .runtime_descriptor(&target.thread_id)
            .await?
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
                .runtime_descriptor(&ancestor.thread_id)
                .await?
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
        runtime.model_targets = resolve_model_targets_for_model(
            &runtime.model_targets,
            runtime.model_request.model.as_deref(),
        )?;
        validate_recovered_runtime_setup(&material.memory_dir, &self.control, target, &runtime)
            .await?;
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
        request.runtime.model_targets = resolve_model_targets_for_model(
            &request.runtime.model_targets,
            resolved.model.as_deref(),
        )?;

        let reservation = self
            .control
            .reserve_spawn_typed(
                &self.current_path,
                &request.request.task_name,
                &resolved.definition.name,
            )
            .await
            .context("reserve agent thread")?;
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
            })
            .await
            .context("persist agent runtime descriptor")?;

        let mut forked_session = fork_parent_session(
            &memory_dir,
            &runtime,
            &thread.session_id,
            &runtime.model_request.fork_turns,
        )
        .await
        .context("fork parent session")?;
        if let Err(error) = validate_runtime_setup(
            &memory_dir,
            &self.control,
            &thread,
            &runtime,
            &thread.session_id,
        )
        .await
        {
            return Err(rollback_fork_error(
                &mut forked_session,
                "validate agent runtime setup",
                error,
            )
            .await);
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
            )
            .await);
        }
        if let Err(error) = reservation.commit().await {
            self.runtime_requests.remove(&thread.thread_id);
            return Err(rollback_fork_error(
                &mut forked_session,
                "commit agent thread reservation",
                error,
            )
            .await);
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
            std::thread::scope(|scope| {
                scope
                    .spawn(|| {
                        let runtime = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .map_err(anyhow::Error::from)?;
                        let graph_result = runtime
                            .block_on(
                                cleanup_control.finalize_unaccepted_spawn(&cleanup_thread, turn_id),
                            )
                            .map_err(|error| error.context("spawn graph and identity rollback"));
                        let mut owned_session = ForkedSessionGuard {
                            sessions_dir: sessions_dir.clone(),
                            child_session_id: child_session_id.clone(),
                            parent_session_id: parent_session_id.clone(),
                            armed: true,
                        };
                        let session_result = runtime
                            .block_on(owned_session.rollback())
                            .map_err(|error| error.context("child session rollback"));
                        match (graph_result, session_result) {
                            (Ok(()), Ok(())) => Ok(()),
                            (Err(graph), Ok(())) => Err(graph),
                            (Ok(()), Err(session)) => Err(session),
                            (Err(graph), Err(session)) => Err(anyhow::anyhow!(
                                "{graph:#}; child session rollback failed: {session:#}"
                            )),
                        }
                    })
                    .join()
                    .unwrap_or_else(|_| {
                        Err(anyhow::anyhow!("unaccepted spawn cleanup thread panicked"))
                    })
            })
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
            .list_agents(&self.current_path, request.path_prefix.as_deref())
            .await
    }

    async fn send_message(
        &self,
        request: MessageAgentV2Request,
    ) -> anyhow::Result<MessageAgentV2Result> {
        let message = self
            .control
            .enqueue_message(&self.current_path, request, false)
            .await?;
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
            .resolve_target(&self.current_path, &request.request.target)
            .await?;
        anyhow::ensure!(
            resolved.canonical_path != AgentPath::root(),
            "follow-up tasks cannot target the root agent"
        );
        let followup_text = request.request.message.trim().to_string();
        #[cfg(test)]
        if let Some(hook) = self.before_followup_atomic_hook.as_ref() {
            let _ = self
                .control
                .resolve_target(&self.current_path, &request.request.target)
                .await?;
            hook.entered.notify_one();
            hook.release.notified().await;
        }
        let (message, admission) = self
            .control
            .enqueue_followup_with_admission(
                &self.current_path,
                request.request,
                |target| async move {
                    let stored = match self.runtime_requests.get(&target.thread_id)? {
                        Some(stored) => stored,
                        None => {
                            let recovered = self
                                .recover_runtime_request(&target, request.runtime.as_ref())
                                .await?;
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
            )
            .await?;
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
            .resolve_target(&self.current_path, &request.target)
            .await?;
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
            .resolve_target(&self.current_path, target.canonical_path.as_str())
            .await?;
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
        model_targets: material.model_targets,
        model_spec: material.model_spec,
        root_service_tier: material.root_service_tier,
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
    let sessions_dir = home::sessions_dir(memory_dir);
    let sessions = session::SessionStore::open_sessions_dir(&sessions_dir).await?;
    let recent_turns = parse_fork_turns(fork_turns.as_deref())?;
    sessions
        .fork_session_recent_turns(&runtime.parent_session_id, child_session_id, recent_turns)
        .await?;
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
    let _ = resolve_model_targets_for_model(
        &runtime.model_targets,
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
    )
    .await?;
    Ok(())
}

async fn validate_recovered_runtime_setup(
    memory_dir: &Path,
    control: &Arc<AgentControl>,
    thread: &subagents::AgentThreadV2,
    runtime: &SpawnRuntimeV2Request,
) -> anyhow::Result<()> {
    let sessions =
        session::SessionStore::open_sessions_dir(&home::sessions_dir(memory_dir)).await?;
    let stored = sessions
        .get_session(&thread.session_id)
        .await?
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
                return Err(
                    close_subtree_error(control.as_ref(), &thread.canonical_path, error).await,
                );
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
    responses_override: Option<crate::streaming::ResponsesOverride>,
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
            responses_override: None,
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
        responses_override: Option<crate::streaming::ResponsesOverride>,
    ) -> Self {
        Self {
            memory_dir,
            runtime_manager,
            runtime_requests,
            control_override: Some(control),
            responses_override,
            close_barrier_hook: None,
        }
    }

    #[cfg(feature = "test-support")]
    fn for_acceptance(
        memory_dir: PathBuf,
        control: Arc<AgentControl>,
        runtime_manager: Arc<AgentRuntimeManager>,
        runtime_requests: Arc<RuntimeRequestRegistry>,
        responses_override: crate::streaming::ResponsesOverride,
    ) -> Self {
        Self {
            memory_dir,
            runtime_manager,
            runtime_requests,
            control_override: Some(control),
            responses_override: Some(responses_override),
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
            responses_override: self.responses_override.clone(),
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
        let messages =
            session::SessionStore::open_sessions_dir(&home::sessions_dir(&self.memory_dir))
                .await?
                .get_response_items(&thread.session_id)
                .await?
                .into_iter()
                .map(|message| AgentThreadMessageV2 {
                    id: message.id,
                    session_id: message.session_id,
                    item: message.item,
                    timestamp: message.timestamp,
                    token_count: message.token_count,
                    finish_reason: message.finish_reason,
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
        control
            .resolve_desktop_target(target_thread.canonical_path.as_str())
            .await
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
        let thread = control
            .resolve_desktop_target(target_thread.canonical_path.as_str())
            .await?;
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
                )
                .await);
            }
        }
        #[cfg(test)]
        if let Some(hook) = self.close_barrier_hook.as_ref() {
            hook.entered.notify_one();
            hook.release.notified().await;
        }
        let mut threads = control
            .snapshot()
            .await?
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
#[allow(clippy::disallowed_methods)] // 测试直接开原始 pool，生产路径必须走 agent-db
#[path = "dispatch_tests.rs"]
mod tests;
