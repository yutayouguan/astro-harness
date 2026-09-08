//! Astro gRPC [`AstroService`] 实现：聊天流、会话、记忆、MCP、技能与文件列表。

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, RwLock as StdRwLock, Weak};

use agent::builder::AgentBuilder;
use agent::runtime::Session;
use agent::{HitlGate, HitlRegistry, TurnAbortReason};
#[cfg(test)]
use futures::FutureExt;
use home::AgentRuntimeConfig;
use memory::MemoryManager;
use proto::astro_service_server::AstroService;
use proto::{
    ApproveGuardianDeniedActionRequest, ChatControlAction, ChatControlRequest, Empty,
    FileListRequest, FileListResponse, ImageEvent, ImageRequest, McpReconnectRequest,
    McpServerList, McpServerListRequest, MemoryQuery, MemoryResult,
    RealtimeConversationAudioRequest, RealtimeConversationRequest,
    RealtimeConversationSpeechRequest, RealtimeConversationStartRequest,
    RealtimeConversationTextRequest, RealtimeOperationResponse, RealtimeVoicesResponse,
    ResolveElicitationRequest, RunUserShellCommandRequest, RunUserShellCommandResponse,
    SessionSnippet as ProtoSessionSnippet, SkillEvent, SkillInfo, SkillList, SkillRequest,
    SteerChatRequest, SteerChatResponse, TerminalIdRequest, TerminalOpenRequest,
    TerminalReadRequest, TerminalReadResponse, TerminalResizeRequest, TerminalSessionResponse,
    TerminalWriteRequest, UpdateTurnSettingsRequest, UpdateTurnSettingsResponse,
};
use providers::PauseControl;
use providers::ProviderConfig;
#[cfg(test)]
use tokio::sync::OwnedMutexGuard;
use tokio::sync::{Mutex, OnceCell, RwLock};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use super::interrupt_store::{clear_interrupt_file, resume_items_from_proto, save_interrupt_file};
use crate::thread_manager::RemoveCurrentThread;
use crate::{
    ConnectionRegistry, ManagedThread, ThreadActivity, ThreadHistoryBuilder, ThreadManager,
    ThreadState, ThreadStateManager, WORKSPACE_EVENT_THREAD_ID,
};

async fn open_sessions(memory_dir: &std::path::Path) -> Result<session::SessionStore, String> {
    memory::ensure_workspace(memory_dir).map_err(|e| e.to_string())?;
    session::SessionStore::open_sessions_dir(&home::sessions_dir(memory_dir))
        .await
        .map_err(|e| e.to_string())
}

fn event_turn_id(msg: &agent_protocol::EventMsg) -> Option<String> {
    use agent_protocol::EventMsg;
    match msg {
        EventMsg::RealtimeConversationStarted(_)
        | EventMsg::RealtimeConversationSdp(_)
        | EventMsg::RealtimeConversationRealtime(_)
        | EventMsg::RealtimeConversationClosed(_)
        | EventMsg::RealtimeConversationListVoicesResponse(_) => None,
        EventMsg::TurnStarted(event) => Some(event.turn_id.clone()),
        EventMsg::UserInputCommitted(event) => Some(event.turn_id.clone()),
        EventMsg::ItemStarted(event)
        | EventMsg::ItemCompleted(event)
        | EventMsg::McpToolCallBegin(event)
        | EventMsg::McpToolCallEnd(event)
        | EventMsg::SubAgentActivity(event)
        | EventMsg::ContextCompacted(event) => Some(event.turn_id.clone()),
        EventMsg::HookStarted(event) => event.turn_id.clone(),
        EventMsg::HookCompleted(event) => event.turn_id.clone(),
        EventMsg::AgentMessageContentDelta(event)
        | EventMsg::PlanDelta(event)
        | EventMsg::ReasoningContentDelta(event)
        | EventMsg::ExecCommandOutputDelta(event)
        | EventMsg::PatchApplyUpdated(event) => Some(event.turn_id.clone()),
        EventMsg::ExecApprovalRequest(event)
        | EventMsg::ApplyPatchApprovalRequest(event)
        | EventMsg::RequestPermissions(event)
        | EventMsg::RequestUserInput(event)
        | EventMsg::ElicitationRequest(event)
        | EventMsg::DynamicToolCallRequest(event)
        | EventMsg::DynamicToolCallResponse(event) => Some(event.turn_id.clone()),
        EventMsg::GuardianAssessment(event) => Some(event.turn_id.clone()),
        EventMsg::ContextUsage(event) => Some(event.turn_id.clone()),
        EventMsg::TokenCount(event) => event.turn_id.clone(),
        EventMsg::TurnComplete(event) => Some(event.turn_id.clone()),
        EventMsg::TurnAborted(event) => event.turn_id.clone(),
        EventMsg::ThreadSettingsApplied(_)
        | EventMsg::ThreadRolledBack(_)
        | EventMsg::Error(_)
        | EventMsg::Warning(_)
        | EventMsg::StreamError(_)
        | EventMsg::ShutdownComplete => None,
    }
}

fn mcp_server_info(status: mcp::ServerStatus) -> proto::McpServerInfo {
    proto::McpServerInfo {
        id: status.id,
        name: status.name,
        status: status.status,
        tools: status.tools,
        required: status.required,
        error: status.error.unwrap_or_default(),
        retryable: status.retryable,
        retry_attempt: status.retry_attempt,
        next_retry_at_unix_ms: status.next_retry_at_unix_ms.unwrap_or_default(),
        oauth_available: status.oauth_available,
        authenticated: status.authenticated,
    }
}

fn terminal_session_response(info: tools::TerminalSessionInfo) -> TerminalSessionResponse {
    TerminalSessionResponse {
        id: info.id,
        scope: info.scope,
        cwd: info.cwd,
        running: info.running,
        exit_code: info.exit_code,
        base_cursor: info.base_cursor,
        end_cursor: info.end_cursor,
    }
}

fn agent_thread_projection(
    activity: subagents::AgentActivity,
    stream_id: &str,
) -> Option<serde_json::Value> {
    let activity_kind = match activity.kind {
        subagents::AgentActivityKind::Spawned { .. } => "spawned",
        subagents::AgentActivityKind::Mailbox { .. } => "mailbox",
        subagents::AgentActivityKind::StatusChanged { .. } => "status_changed",
        subagents::AgentActivityKind::EdgeClosed { .. } => "edge_closed",
        subagents::AgentActivityKind::MainSteer => return None,
    };
    let thread = activity.thread?;
    let status_kind = match thread.status.kind() {
        subagents::AgentStatusKind::PendingInit => "pending_init",
        subagents::AgentStatusKind::Running => "running",
        subagents::AgentStatusKind::Interrupted => "interrupted",
        subagents::AgentStatusKind::Completed => "completed",
        subagents::AgentStatusKind::Errored => "errored",
        subagents::AgentStatusKind::Shutdown => "shutdown",
    };
    let status_payload_json = serde_json::to_string(&thread.status).ok()?;
    Some(serde_json::json!({
        "activity_sequence": activity.sequence,
        "stream_id": stream_id,
        "root_thread_id": thread.root_thread_id,
        "thread_id": thread.thread_id,
        "parent_thread_id": thread.parent_thread_id.unwrap_or_default(),
        "canonical_path": thread.canonical_path.to_string(),
        "task_name": thread.task_name,
        "agent_type": thread.agent_type,
        "session_id": thread.session_id,
        "status_kind": status_kind,
        "status_payload_json": status_payload_json,
        "activity_kind": activity_kind,
    }))
}

/// 会话 Agent 循环的共享句柄。
type SessionHandle = Arc<Session>;

#[derive(Clone)]
struct PauseRegistration {
    control: Arc<PauseControl>,
    session: Weak<Session>,
    hitl_gate: Arc<HitlGate>,
    ui_generation: ::hooks::UiTimelineGeneration,
    #[cfg(test)]
    operation: Arc<Mutex<()>>,
}

#[cfg(test)]
struct GenerationLaunchReply<T> {
    /// The worker retains the generation operation until this handoff is accepted or dropped.
    value: Option<T>,
    accepted: Option<tokio::sync::oneshot::Sender<bool>>,
}

#[cfg(test)]
struct PreparedPauseGeneration {
    session_id: String,
    session: Weak<Session>,
    hitl_gate: Arc<HitlGate>,
    hook_tx: Option<tokio::sync::mpsc::UnboundedSender<::hooks::UiHookEvent>>,
    operation: Arc<Mutex<()>>,
    admission: Option<OwnedMutexGuard<()>>,
    pause_controls: Arc<StdRwLock<HashMap<String, PauseRegistration>>>,
    hitl_registry: HitlRegistry,
    generation_operations: GenerationOperations,
    ui_slot: ::hooks::UiTimelineSlot,
    memory_dir: PathBuf,
    committed: bool,
}

#[cfg(test)]
impl PreparedPauseGeneration {
    fn commit(mut self) -> PauseRegistration {
        let ui_generation = self.ui_slot.install_tx(
            &self.session_id,
            self.hook_tx.take().expect("prepared UI sender missing"),
        );
        let registration = PauseRegistration {
            control: PauseControl::new(),
            session: self.session.clone(),
            hitl_gate: Arc::clone(&self.hitl_gate),
            ui_generation,
            operation: Arc::clone(&self.operation),
        };
        let old_registration = self
            .pause_controls
            .write()
            .expect("pause registry lock poisoned")
            .insert(self.session_id.clone(), registration.clone());
        let old_registry_gate = self
            .hitl_registry
            .replace_for_admission(Arc::clone(&self.hitl_gate));
        clear_interrupt_file(&self.memory_dir, &self.session_id);
        if let Some(old) = old_registration.as_ref() {
            old.control.cancel();
        }

        self.admission.take();
        self.committed = true;

        tokio::spawn(async move {
            if let Some(old) = old_registration.as_ref() {
                old.hitl_gate.cancel_all().await;
            }
            if let Some(old_gate) = old_registry_gate {
                let already_cancelled = old_registration
                    .as_ref()
                    .is_some_and(|old| Arc::ptr_eq(&old.hitl_gate, &old_gate));
                if !already_cancelled {
                    old_gate.cancel_all().await;
                }
            }
        });
        registration
    }
}

#[cfg(test)]
impl Drop for PreparedPauseGeneration {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        self.admission.take();
        prune_generation_operation(
            &self.generation_operations,
            &self.session_id,
            &self.operation,
            1,
        );
    }
}

#[cfg(test)]
impl<T> GenerationLaunchReply<T> {
    fn into_value(mut self) -> T {
        if let Some(accepted) = self.accepted.take() {
            let _ = accepted.send(true);
        }
        self.value.take().expect("generation launch value missing")
    }
}

#[cfg(test)]
impl<T> Drop for GenerationLaunchReply<T> {
    fn drop(&mut self) {
        if let Some(accepted) = self.accepted.take() {
            let _ = accepted.send(false);
        }
    }
}

type GenerationOperations = Arc<StdMutex<HashMap<String, Weak<Mutex<()>>>>>;
type ReleaseOwnerships = Arc<StdMutex<HashMap<String, Weak<ReleaseGenerationOwnership>>>>;

#[derive(Clone, Copy, Default)]
struct ReleaseSessionRuntimeResult {
    should_finalize_without_runtime: bool,
}

#[derive(Default)]
struct ReleaseGenerationOwnership {
    completion: OnceCell<ReleaseSessionRuntimeResult>,
    fallback_claimed: AtomicBool,
}

struct ReleaseGenerationOwnershipLease {
    session_id: String,
    entry: Arc<ReleaseGenerationOwnership>,
    registry: ReleaseOwnerships,
}

impl ReleaseGenerationOwnershipLease {
    #[cfg(test)]
    fn entry_ptr(&self) -> usize {
        Arc::as_ptr(&self.entry) as usize
    }
}

impl Drop for ReleaseGenerationOwnershipLease {
    fn drop(&mut self) {
        let Ok(mut registry) = self.registry.lock() else {
            return;
        };
        let is_current = registry
            .get(&self.session_id)
            .is_some_and(|current| Weak::ptr_eq(current, &Arc::downgrade(&self.entry)));
        if is_current && Arc::strong_count(&self.entry) == 1 {
            registry.remove(&self.session_id);
        }
    }
}

struct ReleaseSessionRuntimeReply {
    shared: ReleaseSessionRuntimeResult,
    ownership: ReleaseGenerationOwnershipLease,
}

impl ReleaseSessionRuntimeReply {
    fn into_shared_result(self) -> ReleaseSessionRuntimeResult {
        self.shared
    }

    fn claim_fallback(self) -> ReleaseSessionRuntimeResult {
        ReleaseSessionRuntimeResult {
            should_finalize_without_runtime: self.shared.should_finalize_without_runtime
                && self
                    .ownership
                    .entry
                    .fallback_claimed
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok(),
        }
    }
}

fn prune_generation_operation(
    operations: &GenerationOperations,
    session_id: &str,
    operation: &Arc<Mutex<()>>,
    owned_refs: usize,
) {
    let Ok(mut operations) = operations.lock() else {
        return;
    };
    let is_current = operations
        .get(session_id)
        .is_some_and(|current| Weak::ptr_eq(current, &Arc::downgrade(operation)));
    if is_current && Arc::strong_count(operation) <= owned_refs {
        operations.remove(session_id);
    }
}

#[cfg(test)]
async fn cleanup_pause_generation_parts(
    generation_operations: &GenerationOperations,
    pause_controls: &Arc<StdRwLock<HashMap<String, PauseRegistration>>>,
    hitl_registry: &HitlRegistry,
    ui_slot: &::hooks::UiTimelineSlot,
    memory_dir: &std::path::Path,
    session_id: &str,
    registration: &PauseRegistration,
) -> bool {
    let (removed_current, detached_gate) = {
        let _admission = registration.operation.lock().await;
        detach_pause_generation_parts(
            pause_controls,
            hitl_registry,
            ui_slot,
            memory_dir,
            session_id,
            registration,
        )
        .await
    };

    cancel_pause_generation(registration, detached_gate).await;
    prune_generation_operation(
        generation_operations,
        session_id,
        &registration.operation,
        1,
    );
    removed_current
}

async fn detach_pause_generation_parts(
    pause_controls: &Arc<StdRwLock<HashMap<String, PauseRegistration>>>,
    hitl_registry: &HitlRegistry,
    ui_slot: &::hooks::UiTimelineSlot,
    memory_dir: &std::path::Path,
    session_id: &str,
    registration: &PauseRegistration,
) -> (bool, Option<Arc<HitlGate>>) {
    let removed_current = {
        let mut registrations = pause_controls
            .write()
            .expect("pause registry lock poisoned");
        if registrations
            .get(session_id)
            .is_some_and(|current| Arc::ptr_eq(&current.control, &registration.control))
        {
            registrations.remove(session_id);
            true
        } else {
            false
        }
    };

    let detached_gate = if removed_current
        && hitl_registry
            .get(session_id)
            .await
            .is_some_and(|current| Arc::ptr_eq(&current, &registration.hitl_gate))
    {
        hitl_registry.remove(session_id).await
    } else {
        None
    };
    if removed_current {
        ui_slot.clear_if(session_id, registration.ui_generation);
        clear_interrupt_file(memory_dir, session_id);
    }
    (removed_current, detached_gate)
}

async fn cancel_pause_generation(
    registration: &PauseRegistration,
    detached_gate: Option<Arc<HitlGate>>,
) {
    registration.control.cancel();
    registration.hitl_gate.cancel_all().await;
    if detached_gate
        .as_ref()
        .is_some_and(|gate| !Arc::ptr_eq(gate, &registration.hitl_gate))
    {
        detached_gate
            .expect("checked detached gate")
            .cancel_all()
            .await;
    }
}
/// `ChatControlAction` 之外的后端保留动作码：仅释放会话运行时，不触发 new_chat hooks。
const CHAT_CONTROL_RELEASE_SESSION: i32 = 7;

/// 工具 / review 返回文本是否表示写入只入了 pending（未改 live）。
fn indicates_pending_enqueue(content: &str) -> bool {
    content.contains("待审批") || content.contains("pending") || content.contains("入队")
}

/// 仅正常成功的 Run 才允许触发记忆 review、标题生成等回合后副作用。
fn allows_post_turn_side_effects(outcome_type: &str) -> bool {
    outcome_type == "success"
}

fn can_idle_unload_thread(activity: &ThreadActivity) -> bool {
    !activity.has_subscribers
        && matches!(
            activity.status.as_str(),
            "idle" | "completed" | "failed" | "aborted" | "errored"
        )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackgroundPhaseOutcome {
    Completed,
    Failed,
    TimedOut,
    Panicked,
    Cancelled,
}

#[cfg(test)]
async fn run_bounded_post_turn_phase<F, E, Finalize>(
    work: F,
    finalize: Finalize,
) -> BackgroundPhaseOutcome
where
    F: Future<Output = Result<(), E>> + Send + 'static,
    E: Send + 'static,
    Finalize: Future<Output = ()>,
{
    finish_bounded_post_turn_task_with_cancel(tokio::spawn(work), finalize, None).await
}

async fn run_bounded_post_turn_phase_until_cancelled<F, E, Finalize>(
    work: F,
    finalize: Finalize,
    lifecycle: tokio_util::sync::CancellationToken,
) -> BackgroundPhaseOutcome
where
    F: Future<Output = Result<(), E>> + Send + 'static,
    E: Send + 'static,
    Finalize: Future<Output = ()>,
{
    finish_bounded_post_turn_task_with_cancel(tokio::spawn(work), finalize, Some(lifecycle)).await
}

#[cfg(test)]
async fn finish_bounded_post_turn_task<E, Finalize>(
    task: tokio::task::JoinHandle<Result<(), E>>,
    finalize: Finalize,
) -> BackgroundPhaseOutcome
where
    E: Send + 'static,
    Finalize: Future<Output = ()>,
{
    finish_bounded_post_turn_task_with_cancel(task, finalize, None).await
}

async fn finish_bounded_post_turn_task_with_cancel<E, Finalize>(
    mut task: tokio::task::JoinHandle<Result<(), E>>,
    finalize: Finalize,
    lifecycle: Option<tokio_util::sync::CancellationToken>,
) -> BackgroundPhaseOutcome
where
    E: Send + 'static,
    Finalize: Future<Output = ()>,
{
    let cancelled = async {
        match lifecycle {
            Some(lifecycle) => lifecycle.cancelled().await,
            None => std::future::pending::<()>().await,
        }
    };
    let result = tokio::select! {
        result = tokio::time::timeout(crate::POST_TURN_SIDE_EFFECT_TIMEOUT, &mut task) => Some(result),
        () = cancelled => None,
    };
    let outcome = match result {
        Some(Ok(Ok(Ok(())))) => BackgroundPhaseOutcome::Completed,
        Some(Ok(Ok(Err(_)))) => BackgroundPhaseOutcome::Failed,
        Some(Ok(Err(error))) if error.is_panic() => BackgroundPhaseOutcome::Panicked,
        Some(Ok(Err(_))) => BackgroundPhaseOutcome::Cancelled,
        Some(Err(_)) => {
            task.abort();
            let _ = task.await;
            BackgroundPhaseOutcome::TimedOut
        }
        None => {
            task.abort();
            let _ = task.await;
            BackgroundPhaseOutcome::Cancelled
        }
    };
    finalize.await;
    outcome
}

struct ExtensionWaiterRegistration {
    waiter_id: uuid::Uuid,
    commands: tokio::sync::mpsc::UnboundedSender<crate::ListenerCommand>,
}

impl Drop for ExtensionWaiterRegistration {
    fn drop(&mut self) {
        let _ = self
            .commands
            .send(crate::ListenerCommand::CancelExtensionWaiter {
                waiter_id: self.waiter_id,
            });
    }
}

fn register_extension_waiter(
    commands: &tokio::sync::mpsc::UnboundedSender<crate::ListenerCommand>,
    item_id: String,
    payload_json: String,
) -> Option<(
    ExtensionWaiterRegistration,
    tokio::sync::oneshot::Receiver<()>,
)> {
    let waiter_id = uuid::Uuid::new_v4();
    let (reply, materialized) = tokio::sync::oneshot::channel();
    commands
        .send(crate::ListenerCommand::WaitForExtension {
            waiter_id,
            item_id,
            payload_json,
            reply,
        })
        .ok()?;
    Some((
        ExtensionWaiterRegistration {
            waiter_id,
            commands: commands.clone(),
        },
        materialized,
    ))
}

/// 等待 background review 完成，并将结果提交到原回合的 durable Thread Extension。
async fn spawn_review_to_thread(
    service: AstroServiceImpl,
    managed: &Arc<ManagedThread>,
    session: &SessionHandle,
    turn_id: &str,
) -> Result<(), Status> {
    let job = agent::exec::memory_review::job_from_agent(session.as_ref()).await;
    let applied = agent::exec::memory_review::maybe_run_background_review(job)
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    if let Some(n) = agent::exec::memory_review::review_notify_from_applied(&applied) {
        let live_written = !indicates_pending_enqueue(&n.content);
        service
            .emit_background_review_extension_for_turn(managed, turn_id, n.content, live_written)
            .await?;
    }
    Ok(())
}

/// 启动首轮标题生成，成功后提交 `astro.session_metadata` Extension。
async fn spawn_title_to_thread(
    service: AstroServiceImpl,
    managed: &Arc<ManagedThread>,
    session: &SessionHandle,
    turn_id: &str,
) -> Result<(), Status> {
    let job = agent::exec::title_generation::job_from_agent(session.as_ref());
    if let Some(n) = agent::exec::title_generation::maybe_generate_session_title(job)
        .await
        .map_err(|error| Status::internal(error.to_string()))?
    {
        service
            .emit_session_metadata_extension_for_turn(managed, turn_id, n.title)
            .await?;
    }
    Ok(())
}

/// Astro gRPC 服务实现：会话 Agent、流式聊天、记忆与技能等 RPC。
#[derive(Clone)]
pub struct AstroServiceImpl {
    /// session_id → Agent 循环句柄。
    sessions: Arc<RwLock<HashMap<String, SessionHandle>>>,
    pub(crate) threads: ThreadManager,
    pub(crate) thread_states: ThreadStateManager,
    pub(crate) connections: ConnectionRegistry,
    /// session_id → 暂停控制器。
    pause_controls: Arc<StdRwLock<HashMap<String, PauseRegistration>>>,
    /// session_id → generation 操作锁（Weak 以免 release 后无限增长）。
    generation_operations: GenerationOperations,
    /// 进行中的 release 所有权，弱引用持有以便已完成的唯一 id 被回收。
    release_ownerships: ReleaseOwnerships,
    /// session_id → 活 HITL 闸门。
    pub(crate) hitl_registry: HitlRegistry,
    /// 记忆根目录。
    memory_dir: PathBuf,
    /// Plugin / Gateway / Shell 钩子运行时。
    pub(crate) hook_runtime: Arc<::hooks::HookRuntime>,
    /// root thread → 当前 V2 AgentControl generation。
    agent_thread_watchers: Arc<Mutex<HashMap<String, Weak<subagents::AgentControl>>>>,
}

impl AstroServiceImpl {
    /// 使用给定记忆目录创建服务（空会话表）。
    pub fn new(memory_dir: PathBuf) -> Self {
        let hook_runtime = match ::hooks::HookRuntime::bootstrap_from_root(&memory_dir) {
            Ok(rt) => {
                rt.fire_gateway(
                    ::hooks::GATEWAY_STARTUP,
                    &::hooks::HookPayload {
                        turn_id: None,
                        detail: "hook runtime bootstrapped".into(),
                        ..Default::default()
                    },
                );
                Arc::new(rt)
            }
            Err(err) => {
                tracing::warn!(%err, "hook runtime bootstrap failed; using empty");
                Arc::new(::hooks::HookRuntime::new())
            }
        };
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            threads: ThreadManager::default(),
            thread_states: ThreadStateManager::default(),
            connections: ConnectionRegistry::default(),
            pause_controls: Arc::new(StdRwLock::new(HashMap::new())),
            generation_operations: Arc::new(StdMutex::new(HashMap::new())),
            release_ownerships: Arc::new(StdMutex::new(HashMap::new())),
            hitl_registry: HitlRegistry::new(),
            memory_dir,
            hook_runtime,
            agent_thread_watchers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn hook_runtime_for_project(&self, project_root: &str) -> Arc<::hooks::HookRuntime> {
        let requested_root = project_root.trim();
        if requested_root.is_empty() {
            return Arc::clone(&self.hook_runtime);
        }
        let project_root = match Path::new(requested_root).canonicalize() {
            Ok(project_root) => project_root,
            Err(error) => {
                tracing::warn!(%error, project_root = requested_root, "cannot resolve project hook root");
                return Arc::clone(&self.hook_runtime);
            }
        };
        match self
            .hook_runtime
            .with_project_commands(&self.memory_dir, &project_root)
        {
            Ok(runtime) => Arc::new(runtime),
            Err(error) => {
                tracing::warn!(%error, project_root = %project_root.display(), "failed to discover project command hooks");
                Arc::clone(&self.hook_runtime)
            }
        }
    }

    fn emit_extension<'a>(
        &'a self,
        thread_id: &'a str,
        target_turn_id: Option<&'a str>,
        item_key: &'a str,
        namespace: &'a str,
        payload: serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<(), Status>> + Send + 'a>> {
        Box::pin(async move {
            let managed = self.get_or_create_thread(thread_id).await?;
            let target_turn_id = match target_turn_id {
                Some(turn_id) => turn_id.to_string(),
                None if thread_id == WORKSPACE_EVENT_THREAD_ID => {
                    format!("{WORKSPACE_EVENT_THREAD_ID}:state")
                }
                None => match self.thread_states.get(thread_id).await {
                    Some(state) => state
                        .lock()
                        .await
                        .history
                        .completed_turns()
                        .last()
                        .map(|turn| turn.id.clone())
                        .unwrap_or_else(|| format!("{thread_id}:background")),
                    None => format!("{thread_id}:background"),
                },
            };
            Self::emit_extension_on_managed(&managed, target_turn_id, item_key, namespace, payload)
                .await
        })
    }

    async fn emit_extension_on_managed(
        managed: &Arc<ManagedThread>,
        target_turn_id: String,
        item_key: &str,
        namespace: &str,
        payload: serde_json::Value,
    ) -> Result<(), Status> {
        let item_id = format!("{target_turn_id}:{item_key}");
        let item = agent_protocol::ExtensionItem {
            id: item_id.clone(),
            namespace: namespace.into(),
            payload,
        };
        let payload_json =
            serde_json::to_string(&agent_protocol::TurnItem::Extension(item.clone()))
                .map_err(|error| Status::internal(error.to_string()))?;
        let (_registration, materialized) =
            register_extension_waiter(&managed.commands, item_id, payload_json)
                .ok_or_else(|| Status::unavailable("thread listener stopped"))?;
        managed
            .runtime
            .submit(agent_protocol::Op::EmitExtension {
                item,
                turn_id: Some(target_turn_id),
            })
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
        materialized
            .await
            .map_err(|_| Status::unavailable("thread extension was not materialized"))
    }

    /// 生产环境 background-review 发射器；公开以供持久化集成测试覆盖。
    #[doc(hidden)]
    pub async fn emit_background_review_extension(
        &self,
        thread_id: &str,
        summary: impl Into<String>,
        live_written: bool,
    ) -> Result<(), Status> {
        self.emit_extension(
            thread_id,
            None,
            "memory:review",
            "astro.memory",
            serde_json::json!({
                "source": "review",
                "target": "memory",
                "summary": summary.into(),
                "live_written": live_written,
            }),
        )
        .await
    }

    async fn emit_background_review_extension_for_turn(
        &self,
        managed: &Arc<ManagedThread>,
        turn_id: &str,
        summary: impl Into<String>,
        live_written: bool,
    ) -> Result<(), Status> {
        Self::emit_extension_on_managed(
            managed,
            turn_id.to_string(),
            "memory:review",
            "astro.memory",
            serde_json::json!({
                "source": "review",
                "target": "memory",
                "summary": summary.into(),
                "live_written": live_written,
            }),
        )
        .await
    }

    /// 生产环境会话标题发射器；公开以供持久化集成测试覆盖。
    #[doc(hidden)]
    pub async fn emit_session_metadata_extension(
        &self,
        thread_id: &str,
        title: impl Into<String>,
    ) -> Result<(), Status> {
        self.emit_extension(
            thread_id,
            None,
            "session_metadata",
            "astro.session_metadata",
            serde_json::json!({"title": title.into()}),
        )
        .await
    }

    async fn emit_session_metadata_extension_for_turn(
        &self,
        managed: &Arc<ManagedThread>,
        turn_id: &str,
        title: impl Into<String>,
    ) -> Result<(), Status> {
        Self::emit_extension_on_managed(
            managed,
            turn_id.to_string(),
            "session_metadata",
            "astro.session_metadata",
            serde_json::json!({"title": title.into()}),
        )
        .await
    }

    async fn emit_background_complete(
        &self,
        managed: &Arc<ManagedThread>,
        turn_id: &str,
    ) -> Result<(), Status> {
        Self::emit_extension_on_managed(
            managed,
            turn_id.to_string(),
            "background_complete",
            "astro.background_complete",
            serde_json::json!({}),
        )
        .await
    }

    async fn finalize_background_phase(&self, managed: &Arc<ManagedThread>, turn_id: &str) {
        let marker = tokio::time::timeout(
            crate::POST_TURN_COMPLETION_MARKER_TIMEOUT,
            self.emit_background_complete(managed, turn_id),
        )
        .await;
        if !matches!(marker, Ok(Ok(()))) {
            let _ = managed
                .commands
                .send(crate::ListenerCommand::ExpireBackgroundSink {
                    turn_id: turn_id.to_string(),
                });
        }
    }

    /// 生产环境 workspace pending 发射器；公开以供持久化集成测试覆盖。
    #[doc(hidden)]
    pub async fn emit_pending_extension(
        &self,
        pending_count: u32,
        reason: impl Into<String>,
    ) -> Result<(), Status> {
        self.emit_extension(
            WORKSPACE_EVENT_THREAD_ID,
            None,
            "pending",
            "astro.pending",
            serde_json::json!({
                "pending_count": pending_count,
                "reason": reason.into(),
            }),
        )
        .await
    }

    async fn observe_thread_side_effects(
        self,
        thread_id: String,
        session: SessionHandle,
        managed: std::sync::Weak<ManagedThread>,
        lifecycle: tokio_util::sync::CancellationToken,
        mut events: tokio::sync::mpsc::UnboundedReceiver<agent_protocol::Event>,
    ) {
        let mut phases = tokio::task::JoinSet::new();
        loop {
            let event = tokio::select! {
                biased;
                () = lifecycle.cancelled() => break,
                completed = phases.join_next(), if !phases.is_empty() => {
                    if let Some(Err(error)) = completed {
                        tracing::warn!(%error, %thread_id, "post-turn supervisor task failed");
                    }
                    continue;
                }
                event = events.recv() => match event {
                    Some(event) => event,
                    None => break,
                },
            };
            match event.msg {
                agent_protocol::EventMsg::TurnComplete(completed)
                    if completed.error.is_none() && allows_post_turn_side_effects("success") =>
                {
                    let Some(managed) = managed.upgrade() else {
                        break;
                    };
                    let service = self.clone();
                    let session = Arc::clone(&session);
                    let thread_id = thread_id.clone();
                    let turn_id = completed.turn_id;
                    let phase_lifecycle = lifecycle.clone();
                    phases.spawn(async move {
                        let work_service = service.clone();
                        let work_managed = Arc::clone(&managed);
                        let work_session = Arc::clone(&session);
                        let work_thread_id = thread_id.clone();
                        let work_turn_id = turn_id.clone();
                        let finalize_service = service;
                        let finalize_managed = Arc::clone(&managed);
                        let finalize_turn_id = turn_id.clone();
                        let outcome = run_bounded_post_turn_phase_until_cancelled(
                            async move {
                                let (review, title) = tokio::join!(
                                    spawn_review_to_thread(
                                        work_service.clone(),
                                        &work_managed,
                                        &work_session,
                                        &work_turn_id,
                                    ),
                                    spawn_title_to_thread(
                                        work_service,
                                        &work_managed,
                                        &work_session,
                                        &work_turn_id,
                                    ),
                                );
                                if let Err(error) = &review {
                                    tracing::warn!(%error, thread_id = %work_thread_id, turn_id = %work_turn_id, "background review extension failed");
                                }
                                if let Err(error) = &title {
                                    tracing::warn!(%error, thread_id = %work_thread_id, turn_id = %work_turn_id, "session metadata extension failed");
                                }
                                review?;
                                title?;
                                Ok::<(), Status>(())
                            },
                            async move {
                                finalize_service
                                    .finalize_background_phase(
                                        &finalize_managed,
                                        &finalize_turn_id,
                                    )
                                    .await;
                            },
                            phase_lifecycle,
                        )
                        .await;
                        if outcome != BackgroundPhaseOutcome::Completed {
                            tracing::warn!(?outcome, %thread_id, %turn_id, "post-turn background phase did not complete normally");
                        }
                    });
                }
                agent_protocol::EventMsg::ItemCompleted(agent_protocol::ItemEvent {
                    item: agent_protocol::TurnItem::Extension(extension),
                    ..
                }) if extension.namespace == "astro.memory"
                    && !extension
                        .payload
                        .get("live_written")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true) =>
                {
                    let pending_count = memory::list_pending(&self.memory_dir)
                        .map(|items| items.len() as u32)
                        .unwrap_or(0);
                    tokio::select! {
                        biased;
                        () = lifecycle.cancelled() => break,
                        result = self.emit_pending_extension(pending_count, "enqueued") => {
                            if let Err(error) = result {
                                tracing::warn!(%error, "workspace pending extension failed");
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        while let Some(result) = phases.join_next().await {
            if let Err(error) = result {
                tracing::warn!(%error, %thread_id, "post-turn supervisor task failed while stopping");
            }
        }
    }

    pub(crate) async fn get_or_create_thread(
        &self,
        thread_id: &str,
    ) -> Result<Arc<ManagedThread>, Status> {
        let creation_lock = self.threads.creation_lock(thread_id).await;
        let _creation = creation_lock.lock().await;
        if let Some(managed) = self.threads.get_locked(thread_id).await {
            return Ok(managed);
        }

        let rollout_root = self.memory_dir.join("sessions").join("rollouts");
        let rollout_path = agent_rollout::find_rollout(&rollout_root, thread_id)
            .map_err(|error| Status::internal(error.to_string()))?
            .unwrap_or_else(|| {
                agent_rollout::new_rollout_path(&rollout_root, thread_id, chrono::Utc::now())
            });
        let has_existing_rollout = rollout_path.exists();
        let existing_items = if has_existing_rollout {
            agent_rollout::read_rollout(&rollout_path)
                .await
                .map_err(|error| Status::internal(error.to_string()))?
        } else {
            Vec::new()
        };
        let has_native_history = existing_items
            .iter()
            .any(|item| matches!(item, agent_rollout::RolloutItem::ResponseItem(_)));
        if has_native_history {
            let projection = open_sessions(&self.memory_dir)
                .await
                .map_err(Status::internal)?;
            session::store::rebuild_response_items_from_rollout(
                &projection,
                thread_id,
                &existing_items,
            )
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
        }
        let session = self.get_session(thread_id).await?;
        session.restore_prompt_context_from_rollout(&existing_items);
        session.restore_token_usage_from_rollout(&existing_items);
        let rollout = agent_rollout::RolloutRecorder::open(rollout_path)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
        if !has_native_history {
            let legacy_history = session
                .clone_response_history()
                .await
                .into_iter()
                .map(agent_rollout::RolloutItem::ResponseItem)
                .collect();
            rollout
                .record(legacy_history)
                .await
                .map_err(|error| Status::internal(error.to_string()))?;
        }
        let runtime = agent::AstroThread::spawn(Arc::clone(&session), rollout)
            .map_err(|error| Status::failed_precondition(error.to_string()))?;

        let mut history = ThreadHistoryBuilder::default();
        for item in existing_items {
            if let agent_rollout::RolloutItem::EventMsg(msg) = item {
                let id = event_turn_id(&msg).unwrap_or_else(|| thread_id.to_string());
                history.track(&agent_protocol::Event { id, msg });
            }
        }
        let (commands, command_rx) = tokio::sync::mpsc::unbounded_channel();
        let (activity_tx, activity_rx) = tokio::sync::watch::channel(ThreadActivity {
            status: match runtime.status() {
                agent::AgentStatus::Idle => "idle",
                agent::AgentStatus::Running { .. } => "running",
                agent::AgentStatus::Errored(_) => "errored",
                agent::AgentStatus::Shutdown => "shutdown",
            }
            .into(),
            has_subscribers: false,
        });
        let state = Arc::new(Mutex::new(ThreadState {
            status: activity_rx.borrow().status.clone(),
            history,
            subscribers: Default::default(),
            background_extension_sinks: Default::default(),
            listener_command_tx: commands.clone(),
            activity_tx,
        }));
        self.thread_states
            .insert(thread_id.to_string(), Arc::clone(&state))
            .await;
        let (observed_tx, observed_rx) = tokio::sync::mpsc::unbounded_channel();
        let listener = tokio::spawn(crate::thread_listener::run_thread_listener_observed(
            thread_id.to_string(),
            Arc::clone(&runtime),
            state,
            commands.clone(),
            command_rx,
            self.connections.clone(),
            Some(observed_tx),
        ));
        let candidate = Arc::new(ManagedThread::new(runtime, commands, activity_rx, listener));
        if let Err(existing) = self
            .threads
            .insert_if_absent(thread_id.to_string(), Arc::clone(&candidate))
            .await
        {
            candidate.stop_listener().await;
            let _ = candidate.runtime.submit(agent_protocol::Op::Shutdown).await;
            candidate.runtime.wait_terminated().await;
            return Ok(existing);
        }
        let lifecycle = candidate.lifecycle_token();
        let supervisor = tokio::spawn(self.clone().observe_thread_side_effects(
            thread_id.to_string(),
            Arc::clone(&session),
            Arc::downgrade(&candidate),
            lifecycle,
            observed_rx,
        ));
        candidate.set_side_effect_supervisor(supervisor).await;
        let agent_id = home::active_agent_id(&self.memory_dir);
        match agent::exec::agent_control_directory::AgentControlDirectory::global()
            .get_at(thread_id, &home::subagents_db_path(&self.memory_dir))
        {
            Some(control) => {
                self.attach_agent_thread_watcher(
                    thread_id,
                    &agent_id,
                    control,
                    Arc::clone(&candidate),
                )
                .await;
            }
            None => tracing::warn!(thread_id, "root AgentControl is unavailable"),
        }
        self.spawn_idle_unload(thread_id.to_string(), Arc::clone(&candidate));
        Ok(candidate)
    }

    #[cfg(test)]
    pub(crate) async fn configure_thread_from_chat(
        &self,
        thread: &agent::AstroThread,
        req: &proto::ChatRequest,
    ) -> Result<(), Status> {
        let hook_runtime = self.hook_runtime_for_project(&req.project_root);
        let settings = self
            .prepare_thread_settings_from_chat_with_hooks(thread, req, hook_runtime)
            .await?;
        thread
            .session()
            .apply_thread_settings(settings)
            .map_err(Status::invalid_argument)?;
        Ok(())
    }

    pub(crate) async fn prepare_thread_settings_from_chat_with_hooks(
        &self,
        thread: &agent::AstroThread,
        req: &proto::ChatRequest,
        hook_runtime: Arc<::hooks::HookRuntime>,
    ) -> Result<agent_protocol::ThreadSettingsOverrides, Status> {
        let settings =
            super::thread_settings::from_chat_request(req).map_err(Status::invalid_argument)?;
        let session = thread.session();
        session.set_hook_runtime(hook_runtime);
        let (_, hitl_gate, _) = session.ensure_thread_controls();
        if !self
            .hitl_registry
            .get(session.session_id())
            .await
            .is_some_and(|current| Arc::ptr_eq(&current, &hitl_gate))
        {
            if let Some(replaced) = self.hitl_registry.replace_for_admission(hitl_gate) {
                replaced.cancel_all().await;
            }
        }
        Ok(settings)
    }

    fn spawn_idle_unload(&self, thread_id: String, managed: Arc<ManagedThread>) {
        let service = self.clone();
        let mut activity_rx = managed.activity_rx.clone();
        tokio::spawn(async move {
            loop {
                let idle = {
                    let activity = activity_rx.borrow().clone();
                    can_idle_unload_thread(&activity)
                };
                if !idle {
                    if activity_rx.changed().await.is_err() {
                        break;
                    }
                    continue;
                }
                tokio::select! {
                    changed = activity_rx.changed() => {
                        if changed.is_err() { break; }
                    }
                    () = tokio::time::sleep(std::time::Duration::from_secs(30 * 60)) => {
                        let activity = activity_rx.borrow().clone();
                        if can_idle_unload_thread(&activity) {
                            let creation_lock = service.threads.creation_lock(&thread_id).await;
                            let _creation = creation_lock.lock().await;
                            let activity = activity_rx.borrow().clone();
                            if !can_idle_unload_thread(&activity) {
                                continue;
                            }
                            match service
                                .threads
                                .remove_if_current_and_unleased(&thread_id, &managed)
                                .await
                            {
                                RemoveCurrentThread::Removed(_) => {}
                                RemoveCurrentThread::Leased => continue,
                                RemoveCurrentThread::NotCurrent => break,
                            }
                            service.thread_states.remove(&thread_id).await;
                            let removed_session = {
                                let mut sessions = service.sessions.write().await;
                                if sessions.get(&thread_id).is_some_and(|session| {
                                    Arc::ptr_eq(session, managed.runtime.session())
                                }) {
                                    sessions.remove(&thread_id)
                                } else {
                                    None
                                }
                            };
                            service.agent_thread_watchers.lock().await.remove(&thread_id);
                            if let Some(session) = removed_session.as_ref() {
                                agent::exec::dispatch::unregister_active_root_session(
                                    &service.memory_dir,
                                    &thread_id,
                                    session,
                                );
                            }
                            let _ = managed.runtime.submit(agent_protocol::Op::Shutdown).await;
                            managed.runtime.wait_terminated().await;
                            let _ = managed.runtime.flush_rollout().await;
                            managed.stop_listener().await;
                            break;
                        }
                    }
                }
            }
        });
    }

    fn generation_operation(&self, session_id: &str) -> Arc<Mutex<()>> {
        let mut operations = self
            .generation_operations
            .lock()
            .expect("generation operation map mutex poisoned");
        operations.retain(|_, operation| operation.strong_count() > 0);
        if let Some(operation) = operations.get(session_id).and_then(Weak::upgrade) {
            return operation;
        }
        let operation = Arc::new(Mutex::new(()));
        operations.insert(session_id.to_string(), Arc::downgrade(&operation));
        operation
    }

    fn release_generation_ownership(&self, session_id: &str) -> ReleaseGenerationOwnershipLease {
        let mut registry = self
            .release_ownerships
            .lock()
            .expect("release ownership registry mutex poisoned");
        registry.retain(|_, ownership| ownership.strong_count() > 0);
        let entry = registry
            .get(session_id)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| {
                let entry = Arc::new(ReleaseGenerationOwnership::default());
                registry.insert(session_id.to_string(), Arc::downgrade(&entry));
                entry
            });
        ReleaseGenerationOwnershipLease {
            session_id: session_id.to_string(),
            entry,
            registry: Arc::clone(&self.release_ownerships),
        }
    }

    async fn attach_agent_thread_watcher(
        &self,
        root_thread_id: &str,
        root_agent_id: &str,
        control: Arc<subagents::AgentControl>,
        managed: Arc<ManagedThread>,
    ) {
        let weak_control = Arc::downgrade(&control);
        let weak_managed = Arc::downgrade(&managed);
        let stream_id = uuid::Uuid::new_v4().to_string();
        let mut watchers = self.agent_thread_watchers.lock().await;
        if let Some(attached) = watchers.get(root_thread_id).and_then(Weak::upgrade) {
            if Arc::ptr_eq(&attached, &control) {
                return;
            }
        }
        watchers.insert(root_thread_id.to_string(), weak_control.clone());
        let reset_key = format!("agent_thread:resync:{}", uuid::Uuid::new_v4());
        if let Err(error) = Self::emit_extension_on_managed(
            &managed,
            format!("{root_thread_id}:background"),
            &reset_key,
            "astro.agent_thread_resync",
            serde_json::json!({
                "root_thread_id": root_thread_id,
                "agent_id": root_agent_id,
                "stream_id": stream_id.clone(),
                "reason": "agent_control_generation_changed",
            }),
        )
        .await
        {
            tracing::warn!(%error, root_thread_id, "failed to emit AgentControl generation reset");
        }
        drop(watchers);

        let root_thread_id = root_thread_id.to_string();
        let root_agent_id = root_agent_id.to_string();
        let watcher_registry = Arc::clone(&self.agent_thread_watchers);
        tokio::spawn(async move {
            // 从零开始，以便 control 创建与 watcher 调度之间竞争的 activity
            // 从 ActivityBus 缓冲区重放。
            let mut cursor = subagents::ActivityCursor(0);
            loop {
                let Some(control) = weak_control.upgrade() else {
                    break;
                };
                let observation =
                    control.next_activity_after(cursor, std::time::Duration::from_millis(250));
                drop(control);
                let observation = observation.await;

                // 在发布期间保持 generation 注册表锁定。
                // 替代 watcher 无法安装其 reset 标记，直到每个旧 generation
                // 的观察要么已先行发布，要么已观察到自己不再持有此根条目。
                let watchers = watcher_registry.lock().await;
                let owns_entry = watchers
                    .get(&root_thread_id)
                    .is_some_and(|registered| Weak::ptr_eq(registered, &weak_control));
                if !owns_entry {
                    break;
                }
                let activity = match observation {
                    subagents::ActivityObservation::Activity(activity) => *activity,
                    subagents::ActivityObservation::Gap { latest, .. } => {
                        cursor = latest;
                        let reset_key = format!("agent_thread:resync:{}", uuid::Uuid::new_v4());
                        let Some(managed) = weak_managed.upgrade() else {
                            break;
                        };
                        if let Err(error) = AstroServiceImpl::emit_extension_on_managed(
                            &managed,
                            format!("{root_thread_id}:background"),
                            &reset_key,
                            "astro.agent_thread_resync",
                            serde_json::json!({
                                "root_thread_id": root_thread_id,
                                "agent_id": root_agent_id,
                                "stream_id": stream_id.clone(),
                                "reason": "activity_gap",
                            }),
                        )
                        .await
                        {
                            tracing::warn!(%error, root_thread_id, "failed to emit agent activity resync");
                        }
                        continue;
                    }
                    subagents::ActivityObservation::TimedOut => continue,
                };
                if activity.sequence <= cursor.0 {
                    continue;
                }
                cursor = subagents::ActivityCursor(activity.sequence);
                let Some(projection) = agent_thread_projection(activity, &stream_id) else {
                    continue;
                };
                let item_key = format!("agent_thread:{}", cursor.0);
                let Some(managed) = weak_managed.upgrade() else {
                    break;
                };
                if let Err(error) = AstroServiceImpl::emit_extension_on_managed(
                    &managed,
                    format!("{root_thread_id}:background"),
                    &item_key,
                    "astro.agent_thread",
                    projection,
                )
                .await
                {
                    tracing::warn!(%error, root_thread_id, "failed to emit agent activity extension");
                }
            }

            let mut watchers = watcher_registry.lock().await;
            let owns_entry = watchers
                .get(&root_thread_id)
                .is_some_and(|registered| Weak::ptr_eq(registered, &weak_control));
            if owns_entry {
                watchers.remove(&root_thread_id);
            }
        });
    }

    /// 获取或惰性创建会话对应的 [`Session`]。
    ///
    /// 新建时读取当前 active agent 的 [`AgentRuntimeConfig`]，并用请求的 `session_id` 构建。
    ///
    /// # 错误
    /// Agent 构建失败时返回 `Status::internal`。
    async fn get_session(&self, session_id: &str) -> Result<SessionHandle, Status> {
        if let Some(handle) = self.sessions.read().await.get(session_id).cloned() {
            agent::exec::dispatch::register_active_root_session(
                &self.memory_dir,
                session_id,
                &handle,
            )
            .map_err(|error| Status::internal(error.to_string()))?;
            return Ok(handle);
        }

        let agent_id = home::active_agent_id(&self.memory_dir);
        let mut builder = AgentBuilder::new(self.memory_dir.clone()).agent_id(agent_id.clone());
        if let Ok(rt) = AgentRuntimeConfig::load(&self.memory_dir, &agent_id) {
            builder = builder.from_runtime_config(&rt);
        }
        let (agent, _) = builder
            .build_with_session_id(session_id.to_string())
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let handle = Arc::new(agent);
        handle.set_hook_runtime(Arc::clone(&self.hook_runtime));
        let mut sessions = self.sessions.write().await;
        if let Some(existing) = sessions.get(session_id) {
            return Ok(existing.clone());
        }
        self.release_ownerships
            .lock()
            .expect("release ownership registry mutex poisoned")
            .remove(session_id);
        sessions.insert(session_id.to_string(), handle.clone());
        agent::exec::dispatch::register_active_root_session(&self.memory_dir, session_id, &handle)
            .map_err(|error| Status::internal(error.to_string()))?;
        Ok(handle)
    }

    /// 原子切换同一 chat generation 的 pause / HITL / UI 状态。
    #[cfg(test)]
    async fn admit_pause_generation(
        &self,
        session_id: &str,
        session: &SessionHandle,
        hitl_gate: Arc<HitlGate>,
        hook_tx: tokio::sync::mpsc::UnboundedSender<::hooks::UiHookEvent>,
    ) -> Option<PauseRegistration> {
        let service = self.clone();
        let session_id = session_id.to_string();
        let session = Arc::clone(session);
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            service
                .run_admit_pause_generation_worker(
                    session_id, session, hitl_gate, hook_tx, reply_tx,
                )
                .await;
        });
        reply_rx
            .await
            .unwrap_or(None)
            .map(PreparedPauseGeneration::commit)
    }

    #[cfg(test)]
    async fn run_admit_pause_generation_worker(
        &self,
        session_id: String,
        session: SessionHandle,
        hitl_gate: Arc<HitlGate>,
        hook_tx: tokio::sync::mpsc::UnboundedSender<::hooks::UiHookEvent>,
        reply: tokio::sync::oneshot::Sender<Option<PreparedPauseGeneration>>,
    ) {
        let operation = self.generation_operation(&session_id);
        let admission = Arc::clone(&operation).lock_owned().await;
        let is_current_session = self
            .sessions
            .read()
            .await
            .get(&session_id)
            .is_some_and(|current| Arc::ptr_eq(current, &session));
        if !is_current_session {
            let _ = reply.send(None);
            return;
        }
        if let Some(current_gate) = self.hitl_registry.get(&session_id).await {
            if current_gate.is_waiting().await {
                let _ = reply.send(None);
                return;
            }
        }
        let prepared = PreparedPauseGeneration {
            session_id,
            session: Arc::downgrade(&session),
            hitl_gate,
            hook_tx: Some(hook_tx),
            operation,
            admission: Some(admission),
            pause_controls: Arc::clone(&self.pause_controls),
            hitl_registry: self.hitl_registry.clone(),
            generation_operations: Arc::clone(&self.generation_operations),
            ui_slot: self.hook_runtime.ui_slot.clone(),
            memory_dir: self.memory_dir.clone(),
            committed: false,
        };
        let _ = reply.send(Some(prepared));
    }

    #[cfg(test)]
    async fn launch_current_pause_generation_with<F, Fut, T>(
        &self,
        session_id: &str,
        registration: &PauseRegistration,
        launch: F,
    ) -> Option<T>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let session = registration.session.upgrade()?;
        self.launch_current_pause_generation_with_setup(
            session_id,
            registration,
            &session,
            |_| async {},
            launch,
        )
        .await
    }

    #[cfg(test)]
    async fn launch_current_pause_generation_with_setup<S, SetupFut, F, Fut, T>(
        &self,
        session_id: &str,
        registration: &PauseRegistration,
        session: &SessionHandle,
        setup: S,
        launch: F,
    ) -> Option<T>
    where
        S: FnOnce(SessionHandle) -> SetupFut + Send + 'static,
        SetupFut: Future<Output = ()> + Send + 'static,
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let service = self.clone();
        let session_id = session_id.to_string();
        let registration = registration.clone();
        let session = Arc::clone(session);
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            service
                .run_generation_launch_worker(
                    session_id,
                    registration,
                    session,
                    setup,
                    launch,
                    reply_tx,
                )
                .await;
        });
        reply_rx
            .await
            .unwrap_or(None)
            .map(GenerationLaunchReply::into_value)
    }

    #[cfg(test)]
    async fn run_generation_launch_worker<S, SetupFut, F, Fut, T>(
        &self,
        session_id: String,
        registration: PauseRegistration,
        session: SessionHandle,
        setup: S,
        launch: F,
        reply: tokio::sync::oneshot::Sender<Option<GenerationLaunchReply<T>>>,
    ) where
        S: FnOnce(SessionHandle) -> SetupFut + Send + 'static,
        SetupFut: Future<Output = ()> + Send + 'static,
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let admission = Arc::clone(&registration.operation).lock_owned().await;
        let is_current = self
            .pause_controls
            .read()
            .expect("pause registry lock poisoned")
            .get(&session_id)
            .is_some_and(|current| Arc::ptr_eq(&current.control, &registration.control));
        let is_current_session = self
            .sessions
            .read()
            .await
            .get(&session_id)
            .is_some_and(|current| Arc::ptr_eq(current, &session));
        let registration_matches_session = registration
            .session
            .upgrade()
            .is_some_and(|registered| Arc::ptr_eq(&registered, &session));
        if !is_current
            || !is_current_session
            || !registration_matches_session
            || registration.control.is_cancelled()
        {
            let _ = reply.send(None);
            return;
        }
        let settings_snapshot = session.snapshot_request_settings();
        let launch_result = std::panic::AssertUnwindSafe(async {
            setup(Arc::clone(&session)).await;
            launch().await
        })
        .catch_unwind()
        .await;
        if let Ok(value) = launch_result {
            let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();
            let guarded_reply = GenerationLaunchReply {
                value: Some(value),
                accepted: Some(accepted_tx),
            };
            match reply.send(Some(guarded_reply)) {
                Ok(()) if matches!(accepted_rx.await, Ok(true)) => return,
                Ok(()) => {}
                Err(returned) => drop(returned),
            }
        }

        self.cleanup_abandoned_generation_with_admission(
            session_id,
            registration,
            session,
            settings_snapshot,
            admission,
        )
        .await;
    }

    #[cfg(test)]
    async fn cleanup_abandoned_generation_with_admission(
        &self,
        session_id: String,
        registration: PauseRegistration,
        session: SessionHandle,
        settings_snapshot: agent::runtime::SessionRequestSettingsSnapshot,
        admission: tokio::sync::OwnedMutexGuard<()>,
    ) {
        registration.control.cancel();
        let _ = session.abort_all_tasks(TurnAbortReason::Interrupted).await;
        session.restore_request_settings(settings_snapshot);
        let detached_gate = detach_pause_generation_parts(
            &self.pause_controls,
            &self.hitl_registry,
            &self.hook_runtime.ui_slot,
            &self.memory_dir,
            &session_id,
            &registration,
        )
        .await
        .1;
        drop(admission);
        cancel_pause_generation(&registration, detached_gate).await;
        prune_generation_operation(
            &self.generation_operations,
            &session_id,
            &registration.operation,
            2,
        );
    }

    #[cfg(test)]
    async fn cleanup_pause_generation(
        &self,
        session_id: &str,
        registration: &PauseRegistration,
    ) -> bool {
        let service = self.clone();
        let session_id = session_id.to_string();
        let registration = registration.clone();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let removed = cleanup_pause_generation_parts(
                &service.generation_operations,
                &service.pause_controls,
                &service.hitl_registry,
                &service.hook_runtime.ui_slot,
                &service.memory_dir,
                &session_id,
                &registration,
            )
            .await;
            let _ = reply_tx.send(removed);
        });
        reply_rx.await.unwrap_or(false)
    }

    async fn cancel_current_pause_generation_with<F, Fut>(
        &self,
        session_id: &str,
        abort: F,
    ) -> anyhow::Result<Option<PauseRegistration>>
    where
        F: FnOnce(Option<SessionHandle>) -> Fut + Send + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let service = self.clone();
        let session_id = session_id.to_string();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let result = service
                .cancel_current_pause_generation_inner(&session_id, abort)
                .await;
            let _ = reply_tx.send(result);
        });
        reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("cancel generation worker stopped unexpectedly"))?
    }

    async fn cancel_current_pause_generation_inner<F, Fut>(
        &self,
        session_id: &str,
        abort: F,
    ) -> anyhow::Result<Option<PauseRegistration>>
    where
        F: FnOnce(Option<SessionHandle>) -> Fut,
        Fut: Future<Output = anyhow::Result<()>>,
    {
        let operation = self.generation_operation(session_id);
        let admission = operation.lock().await;
        let Some(registration) = self
            .pause_controls
            .read()
            .expect("pause registry lock poisoned")
            .get(session_id)
            .cloned()
        else {
            drop(admission);
            prune_generation_operation(&self.generation_operations, session_id, &operation, 1);
            return Ok(None);
        };
        let detached_gate = detach_pause_generation_parts(
            &self.pause_controls,
            &self.hitl_registry,
            &self.hook_runtime.ui_slot,
            &self.memory_dir,
            session_id,
            &registration,
        )
        .await
        .1;
        registration.control.cancel();
        let abort_result = abort(registration.session.upgrade()).await;
        drop(admission);
        cancel_pause_generation(&registration, detached_gate).await;
        prune_generation_operation(&self.generation_operations, session_id, &operation, 2);
        abort_result?;
        Ok(Some(registration))
    }

    async fn cancel_current_pause_generation(
        &self,
        session_id: &str,
    ) -> anyhow::Result<Option<PauseRegistration>> {
        self.cancel_current_pause_generation_with(session_id, |session| async move {
            if let Some(session) = session {
                session
                    .abort_all_tasks(TurnAbortReason::Interrupted)
                    .await?;
            }
            Ok(())
        })
        .await
    }

    /// 释放会话运行时：取消暂停/HITL/中断文件，并从内存移除 Session。
    ///
    /// 返回是否仍需无 runtime 的 finalize fallback；真实 Session 清理由幂等
    /// `shutdown_runtime` 统一承接并登记 finalization ownership。
    async fn release_session_runtime(&self, session_id: &str) -> ReleaseSessionRuntimeResult {
        let ownership = self.release_generation_ownership(session_id);
        self.release_session_runtime_with_ownership(session_id, ownership)
            .await
    }

    async fn release_session_runtime_with_ownership(
        &self,
        session_id: &str,
        ownership: ReleaseGenerationOwnershipLease,
    ) -> ReleaseSessionRuntimeResult {
        self.release_session_runtime_reply_with_ownership(session_id, ownership)
            .await
            .map(ReleaseSessionRuntimeReply::into_shared_result)
            .unwrap_or_default()
    }

    async fn release_session_runtime_for_new_chat(
        &self,
        session_id: &str,
    ) -> ReleaseSessionRuntimeResult {
        let ownership = self.release_generation_ownership(session_id);
        self.release_session_runtime_reply_with_ownership(session_id, ownership)
            .await
            .map(ReleaseSessionRuntimeReply::claim_fallback)
            .unwrap_or_default()
    }

    async fn release_session_runtime_reply_with_ownership(
        &self,
        session_id: &str,
        ownership: ReleaseGenerationOwnershipLease,
    ) -> Option<ReleaseSessionRuntimeReply> {
        let service = self.clone();
        let session_id = session_id.to_string();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            service
                .run_release_session_runtime_worker(session_id, ownership, reply_tx)
                .await;
        });
        reply_rx.await.ok()
    }

    async fn run_release_session_runtime_worker(
        &self,
        session_id: String,
        ownership: ReleaseGenerationOwnershipLease,
        reply: tokio::sync::oneshot::Sender<ReleaseSessionRuntimeReply>,
    ) {
        let shared = *ownership
            .entry
            .completion
            .get_or_init(|| self.release_session_runtime_inner(&session_id))
            .await;
        let _ = reply.send(ReleaseSessionRuntimeReply { shared, ownership });
    }

    async fn release_session_runtime_inner(&self, session_id: &str) -> ReleaseSessionRuntimeResult {
        let creation_lock = self.threads.creation_lock(session_id).await;
        let _thread_creation = creation_lock.lock().await;
        let operation = self.generation_operation(session_id);
        let (
            registration,
            detached_gate,
            orphan_gate,
            removed,
            removed_thread,
            should_finalize_without_runtime,
        ) = {
            let _admission = operation.lock().await;
            let registration = self
                .pause_controls
                .read()
                .expect("pause registry lock poisoned")
                .get(session_id)
                .cloned();
            let expected_session = match registration.as_ref() {
                Some(registration) => registration.session.upgrade(),
                None => self.sessions.read().await.get(session_id).cloned(),
            };
            let detached_gate = if let Some(registration) = registration.as_ref() {
                detach_pause_generation_parts(
                    &self.pause_controls,
                    &self.hitl_registry,
                    &self.hook_runtime.ui_slot,
                    &self.memory_dir,
                    session_id,
                    registration,
                )
                .await
                .1
            } else {
                None
            };
            let orphan_gate = self.hitl_registry.remove(session_id).await;
            clear_interrupt_file(&self.memory_dir, session_id);
            let removed_thread = self.threads.remove(session_id).await;
            if removed_thread.is_some() {
                self.thread_states.remove(session_id).await;
            }
            let mut sessions = self.sessions.write().await;
            let removed = match expected_session {
                Some(expected)
                    if sessions
                        .get(session_id)
                        .is_some_and(|current| Arc::ptr_eq(current, &expected)) =>
                {
                    sessions.remove(session_id)
                }
                Some(_) | None => None,
            };
            let has_live_session = sessions.contains_key(session_id);
            let should_finalize_without_runtime = removed.is_none() && !has_live_session;
            (
                registration,
                detached_gate,
                orphan_gate,
                removed,
                removed_thread,
                should_finalize_without_runtime,
            )
        };
        if let Some(registration) = registration.as_ref() {
            cancel_pause_generation(registration, detached_gate).await;
        } else if let Some(gate) = detached_gate {
            gate.cancel_all().await;
        }
        if let Some(gate) = orphan_gate {
            let already_cancelled = registration
                .as_ref()
                .is_some_and(|registration| Arc::ptr_eq(&registration.hitl_gate, &gate));
            if !already_cancelled {
                gate.cancel_all().await;
            }
        }
        self.agent_thread_watchers.lock().await.remove(session_id);
        if let Some(handle) = removed.as_ref() {
            agent::exec::dispatch::unregister_active_root_session(
                &self.memory_dir,
                session_id,
                handle,
            );
        }
        if let Some(managed) = removed_thread.as_ref() {
            managed.stop_side_effects().await;
            let _ = managed.runtime.submit(agent_protocol::Op::Shutdown).await;
            managed.runtime.wait_terminated().await;
            let _ = managed.runtime.flush_rollout().await;
            managed.stop_listener().await;
        } else if let Some(handle) = removed.as_ref() {
            handle.shutdown_runtime().await;
        }
        prune_generation_operation(
            &self.generation_operations,
            session_id,
            &operation,
            if registration.is_some() { 2 } else { 1 },
        );
        ReleaseSessionRuntimeResult {
            should_finalize_without_runtime,
        }
    }

    /// UI「新建对话」：统一投递 command/reset/finalize，并卸内存会话。
    async fn release_session_for_new_chat(&self, session_id: &str) {
        let payload = ::hooks::HookPayload {
            session_id: session_id.to_string(),
            turn_id: None,
            detail: "new_chat".into(),
            ..Default::default()
        };
        let _ = self
            .hook_runtime
            .dispatch(::hooks::COMMAND_NEW_CHAT, &payload);
        let _ = self.hook_runtime.dispatch(::hooks::SESSION_RESET, &payload);
        if self
            .release_session_runtime_for_new_chat(session_id)
            .await
            .should_finalize_without_runtime
        {
            let mut end_payload = payload.clone();
            end_payload.reason = Some("other".into());
            let _ = self
                .hook_runtime
                .dispatch(::hooks::SESSION_END, &end_payload);
        }
    }
}

#[tonic::async_trait]
impl AstroService for AstroServiceImpl {
    type SubscribeThreadEventsStream =
        Pin<Box<dyn futures::Stream<Item = Result<proto::ThreadEvent, Status>> + Send>>;
    /// [`generate_image`](Self::generate_image) 流类型。
    type GenerateImageStream =
        Pin<Box<dyn futures::Stream<Item = Result<ImageEvent, Status>> + Send>>;
    /// [`execute_skill`](Self::execute_skill) 流类型。
    type ExecuteSkillStream =
        Pin<Box<dyn futures::Stream<Item = Result<SkillEvent, Status>> + Send>>;
    async fn subscribe_thread_events(
        &self,
        request: Request<proto::SubscribeThreadEventsRequest>,
    ) -> Result<Response<Self::SubscribeThreadEventsStream>, Status> {
        super::thread_service::subscribe_thread_events(self, request).await
    }

    async fn submit_turn(
        &self,
        request: Request<proto::SubmitTurnRequest>,
    ) -> Result<Response<proto::SubmitTurnResponse>, Status> {
        super::thread_service::submit_turn(self, request).await
    }

    async fn realtime_conversation_start(
        &self,
        request: Request<RealtimeConversationStartRequest>,
    ) -> Result<Response<RealtimeOperationResponse>, Status> {
        super::realtime_service::start(self, request.into_inner()).await
    }

    async fn realtime_conversation_audio(
        &self,
        request: Request<RealtimeConversationAudioRequest>,
    ) -> Result<Response<RealtimeOperationResponse>, Status> {
        super::realtime_service::audio(self, request.into_inner()).await
    }

    async fn realtime_conversation_text(
        &self,
        request: Request<RealtimeConversationTextRequest>,
    ) -> Result<Response<RealtimeOperationResponse>, Status> {
        super::realtime_service::text(self, request.into_inner()).await
    }

    async fn realtime_conversation_speech(
        &self,
        request: Request<RealtimeConversationSpeechRequest>,
    ) -> Result<Response<RealtimeOperationResponse>, Status> {
        super::realtime_service::speech(self, request.into_inner()).await
    }

    async fn realtime_conversation_close(
        &self,
        request: Request<RealtimeConversationRequest>,
    ) -> Result<Response<RealtimeOperationResponse>, Status> {
        super::realtime_service::close(self, request.into_inner()).await
    }

    async fn realtime_conversation_list_voices(
        &self,
        request: Request<RealtimeConversationRequest>,
    ) -> Result<Response<RealtimeVoicesResponse>, Status> {
        super::realtime_service::list_voices(self, request.into_inner()).await
    }

    async fn resume_thread(
        &self,
        request: Request<proto::ResumeThreadRequest>,
    ) -> Result<Response<proto::ResumeThreadResponse>, Status> {
        super::thread_service::resume_thread(self, request).await
    }

    async fn unsubscribe_thread(
        &self,
        request: Request<proto::UnsubscribeThreadRequest>,
    ) -> Result<Response<proto::Empty>, Status> {
        super::thread_service::unsubscribe_thread(self, request).await
    }

    /// Chat 流控制：暂停 / 继续 / 取消指定 `session_id` 的进行中对话。
    ///
    /// Cancel 会同时触发 [`PauseControl::cancel`] 与 Agent 的 `agent::prompt::hooks::CancelSignal`。
    ///
    /// # 错误
    /// - `invalid_argument`：`session_id` 为空
    /// - `not_found`：该会话当前没有注册的流式 PauseControl
    async fn chat_control(
        &self,
        request: Request<ChatControlRequest>,
    ) -> Result<Response<Empty>, Status> {
        let req = request.into_inner();
        if req.session_id.is_empty() {
            return Err(Status::invalid_argument("session_id 不能为空"));
        }
        if req.action == CHAT_CONTROL_RELEASE_SESSION {
            self.release_session_runtime(&req.session_id).await;
            return Ok(Response::new(Empty {}));
        }

        let action = ChatControlAction::try_from(req.action).unwrap_or_default();

        if matches!(action, ChatControlAction::ReleaseSession) {
            self.release_session_runtime(&req.session_id).await;
            return Ok(Response::new(Empty {}));
        }

        // 新建对话不依赖进行中的流；无内存会话时仍触发 Gateway 事件。
        if matches!(action, ChatControlAction::ChatControlNewChat) {
            self.release_session_for_new_chat(&req.session_id).await;
            return Ok(Response::new(Empty {}));
        }

        // 刷新活会话记忆快照：不要求有进行中的流；也不惰性创建新会话。
        if matches!(action, ChatControlAction::ChatControlRefreshMemory) {
            let sessions = self.sessions.read().await;
            let Some(session) = sessions.get(&req.session_id).cloned() else {
                return Err(Status::not_found(format!(
                    "会话 {} 不在内存中（无活 Session 可刷新）",
                    req.session_id
                )));
            };
            drop(sessions);
            session
                .refresh_memory()
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
            return Ok(Response::new(Empty {}));
        }

        if matches!(action, ChatControlAction::ChatControlCancel) {
            if let Some(managed) = self.threads.get(&req.session_id).await {
                let (_, gate, _) = managed.runtime.session().ensure_thread_controls();
                gate.cancel_all().await;
                managed
                    .runtime
                    .submit(agent_protocol::Op::Interrupt)
                    .await
                    .map_err(|error| Status::internal(error.to_string()))?;
                return Ok(Response::new(Empty {}));
            }
            let Some(_registration) = self
                .cancel_current_pause_generation(&req.session_id)
                .await
                .map_err(|error| Status::internal(error.to_string()))?
            else {
                return Err(Status::not_found(format!(
                    "会话 {} 当前没有进行中的流式对话",
                    req.session_id
                )));
            };
            return Ok(Response::new(Empty {}));
        }

        if let Some(managed) = self.threads.get(&req.session_id).await {
            let (pause, _, _) = managed.runtime.session().ensure_thread_controls();
            match action {
                ChatControlAction::ChatControlPause => pause.pause(),
                ChatControlAction::ChatControlResume
                | ChatControlAction::ChatControlStreamResume => pause.resume(),
                _ => {}
            }
            return Ok(Response::new(Empty {}));
        }

        let registration = {
            let operation = self.generation_operation(&req.session_id);
            let _admission = operation.lock().await;
            self.pause_controls
                .read()
                .expect("pause registry lock poisoned")
                .get(&req.session_id)
                .cloned()
        };
        let Some(registration) = registration else {
            return Err(Status::not_found(format!(
                "会话 {} 当前没有进行中的流式对话",
                req.session_id
            )));
        };
        let pause = &registration.control;
        match action {
            ChatControlAction::ChatControlPause => pause.pause(),
            ChatControlAction::ChatControlResume | ChatControlAction::ChatControlStreamResume => {
                pause.resume()
            }
            ChatControlAction::ChatControlCancel => unreachable!("cancel handled above"),
            ChatControlAction::ChatControlNewChat
            | ChatControlAction::ChatControlRefreshMemory
            | ChatControlAction::ReleaseSession
            | ChatControlAction::ChatControlUnspecified => {}
        }
        Ok(Response::new(Empty {}))
    }

    /// 将用户补充输入排入当前活动的普通 turn。
    ///
    /// 与 `SubmitTurn(start_or_steer)` 不同，这个 RPC 只接受已有活动 turn：
    /// 不创建 Session，也不启动新的 Thread 事件流。
    async fn steer_chat(
        &self,
        request: Request<SteerChatRequest>,
    ) -> Result<Response<SteerChatResponse>, Status> {
        let req = request.into_inner();
        let session_id = req.session_id.trim();
        if session_id.is_empty() {
            return Err(Status::invalid_argument("session_id 不能为空"));
        }
        if req.content.trim().is_empty() && req.images.is_empty() {
            return Err(Status::invalid_argument("steering 内容不能为空"));
        }

        let Some(managed) = self.threads.get(session_id).await else {
            return Ok(Response::new(SteerChatResponse {
                accepted: false,
                turn_id: String::new(),
            }));
        };
        let expected_turn_id = req.expected_turn_id.trim().to_string();
        if expected_turn_id.is_empty() {
            return Err(Status::invalid_argument("expected_turn_id 不能为空"));
        }
        let mut image_data_urls = Vec::with_capacity(req.images.len());
        for image in &req.images {
            let mime = image.mime.trim();
            let data = image.data_base64.trim();
            if mime.is_empty() || data.is_empty() {
                return Err(Status::invalid_argument(
                    "每个 steering 图片都需要 mime 和 data_base64",
                ));
            }
            image_data_urls.push(format!("data:{mime};base64,{data}"));
        }
        let submitted = managed
            .runtime
            .submit_turn(
                agent_protocol::TurnInputRequest {
                    input: vec![agent_protocol::TurnInput {
                        content: req.content,
                        image_data_urls,
                        client_message_id: (!req.client_message_id.trim().is_empty())
                            .then(|| req.client_message_id.trim().to_string()),
                    }],
                    rollback_keep_chat_bubbles: None,
                    thread_settings: Default::default(),
                },
                agent_protocol::TurnInputMode::Steer { expected_turn_id },
            )
            .await;
        let (_, submission) = match submitted {
            Ok(submitted) => submitted,
            Err(error)
                if matches!(
                    error.downcast_ref::<agent_protocol::TurnInputError>(),
                    Some(agent_protocol::TurnInputError::Invalid(_))
                ) =>
            {
                return Ok(Response::new(SteerChatResponse {
                    accepted: false,
                    turn_id: String::new(),
                }));
            }
            Err(error) => return Err(Status::failed_precondition(error.to_string())),
        };
        let turn_id = submission.turn_id().unwrap_or_default().to_string();
        Ok(Response::new(SteerChatResponse {
            accepted: matches!(
                submission,
                agent_protocol::TurnInputSubmission::Steered { .. }
            ),
            turn_id,
        }))
    }

    /// 完成同回合 HITL 等待（解析活闸门 oneshot）；不注入 user 消息、不新开 run。
    async fn interrupt_resume(
        &self,
        request: Request<proto::InterruptResumeRequest>,
    ) -> Result<Response<Empty>, Status> {
        let req = request.into_inner();
        if req.session_id.is_empty() {
            return Err(Status::invalid_argument("session_id 不能为空"));
        }
        let operation = self.generation_operation(&req.session_id);
        let gate = {
            let _admission = operation.lock().await;
            self.hitl_registry.get(&req.session_id).await
        };
        let Some(gate) = gate else {
            return Err(Status::failed_precondition(
                "当前会话没有等待中的 HITL（可能已超时或过期）",
            ));
        };
        let items = resume_items_from_proto(&req.resume);
        gate.resolve(&items)
            .await
            .map_err(|e| Status::invalid_argument(format!("interrupt resume 无效: {e}")))?;
        // 同批多 HITL：只清已解决项，保留 sibling waiting 的旁路文件
        let remaining = gate.pending_interrupts().await;
        let _admission = operation.lock().await;
        let is_current = self
            .hitl_registry
            .get(&req.session_id)
            .await
            .is_some_and(|current| Arc::ptr_eq(&current, &gate));
        if is_current {
            if remaining.is_empty() {
                clear_interrupt_file(&self.memory_dir, &req.session_id);
            } else {
                let _ = save_interrupt_file(&self.memory_dir, &req.session_id, &remaining);
            }
        }
        Ok(Response::new(Empty {}))
    }

    async fn resolve_elicitation(
        &self,
        request: Request<ResolveElicitationRequest>,
    ) -> Result<Response<Empty>, Status> {
        let req = request.into_inner();
        let managed = self
            .threads
            .get(&req.session_id)
            .await
            .ok_or_else(|| Status::not_found("session runtime is not loaded"))?;
        let action = match req.action.as_str() {
            "accept" => agent_protocol::ElicitationAction::Accept,
            "decline" => agent_protocol::ElicitationAction::Decline,
            "cancel" => agent_protocol::ElicitationAction::Cancel,
            _ => {
                return Err(Status::invalid_argument(
                    "action must be accept, decline, or cancel",
                ))
            }
        };
        let parse_optional_json = |raw: String| -> Result<Option<serde_json::Value>, Status> {
            if raw.trim().is_empty() {
                Ok(None)
            } else {
                serde_json::from_str(&raw)
                    .map(Some)
                    .map_err(|error| Status::invalid_argument(error.to_string()))
            }
        };
        let (reply, result) = tokio::sync::oneshot::channel();
        managed
            .runtime
            .submit(agent_protocol::Op::ResolveElicitation {
                server_name: req.server_name,
                request_id: req.request_id,
                response: agent_protocol::ElicitationResponse {
                    action,
                    content: parse_optional_json(req.content_json)?,
                    meta: parse_optional_json(req.meta_json)?,
                },
                reply,
            })
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
        if !result
            .await
            .map_err(|_| Status::internal("elicitation reply channel closed"))?
        {
            return Err(Status::failed_precondition(
                "MCP elicitation is not pending or already resolved",
            ));
        }
        Ok(Response::new(Empty {}))
    }

    async fn update_turn_settings(
        &self,
        request: Request<UpdateTurnSettingsRequest>,
    ) -> Result<Response<UpdateTurnSettingsResponse>, Status> {
        let req = request.into_inner();
        let managed = self
            .threads
            .get(&req.session_id)
            .await
            .ok_or_else(|| Status::not_found("session runtime is not loaded"))?;
        if req.reasoning_effort.as_deref() == Some("persistent") {
            let session = managed.runtime.session();
            let is_openai = session
                .model_targets()
                .first()
                .is_some_and(|target| target.backend_id == "openai");
            let has_instructions = session
                .additional_params()
                .get("astro_persistent_instructions")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| !value.trim().is_empty());
            if !is_openai || !has_instructions {
                return Err(Status::invalid_argument(
                    "persistent reasoning is unavailable for the active model",
                ));
            }
        }
        let optional = |value: Option<String>| {
            value.map(|value| (!value.trim().is_empty()).then(|| value.trim().to_string()))
        };
        let (reply, result) = tokio::sync::oneshot::channel();
        managed
            .runtime
            .submit(agent_protocol::Op::TurnSettings {
                turn_id: req.turn_id,
                update: agent_protocol::TurnSettingsUpdate {
                    model: req.model.map(|value| value.trim().to_string()),
                    reasoning_effort: optional(req.reasoning_effort),
                    reasoning_summary: optional(req.reasoning_summary),
                    service_tier: optional(req.service_tier),
                },
                reply,
            })
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
        let outcome = result
            .await
            .map_err(|_| Status::internal("turn settings reply channel closed"))?;
        let (status, message) = match outcome {
            agent_protocol::TurnSettingsOutcome::Applied => ("applied", String::new()),
            agent_protocol::TurnSettingsOutcome::TargetUnavailable { message } => {
                ("target_unavailable", message)
            }
            agent_protocol::TurnSettingsOutcome::Rejected { message } => ("rejected", message),
        };
        Ok(Response::new(UpdateTurnSettingsResponse {
            status: status.into(),
            message,
        }))
    }

    async fn reconcile_extensions(
        &self,
        request: Request<proto::ReconcileExtensionsRequest>,
    ) -> Result<Response<proto::ReconcileExtensionsResponse>, Status> {
        let req = request.into_inner();
        if req.session_id.trim().is_empty() {
            return Err(Status::invalid_argument("session_id is required"));
        }
        let managed = self.get_or_create_thread(req.session_id.trim()).await?;
        let report = managed
            .runtime
            .session()
            .reconcile_extensions()
            .await
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        let changed_extensions = report
            .changed_extensions
            .into_iter()
            .map(|change| proto::ExtensionReconcileChange {
                extension_id: change.extension_id,
                kind: match change.kind {
                    agent::extensions::ExtensionChangeKind::Added => "added",
                    agent::extensions::ExtensionChangeKind::Updated => "updated",
                    agent::extensions::ExtensionChangeKind::Removed => "removed",
                }
                .into(),
            })
            .collect();
        Ok(Response::new(proto::ReconcileExtensionsResponse {
            previous_version: report.previous_version,
            next_version: report.next_version,
            changed_extensions,
            refresh_mcp: report.refresh_mcp,
            refresh_skills: report.refresh_skills,
            refresh_hooks: report.refresh_hooks,
            refresh_toolsets: report.refresh_toolsets,
        }))
    }

    async fn approve_guardian_denied_action(
        &self,
        request: Request<ApproveGuardianDeniedActionRequest>,
    ) -> Result<Response<Empty>, Status> {
        let req = request.into_inner();
        let managed = self
            .threads
            .get(&req.session_id)
            .await
            .ok_or_else(|| Status::not_found("session runtime is not loaded"))?;
        let (reply, result) = tokio::sync::oneshot::channel();
        managed
            .runtime
            .submit(agent_protocol::Op::ApproveGuardianDeniedAction {
                assessment_id: req.assessment_id,
                reply,
            })
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
        if !result
            .await
            .map_err(|_| Status::internal("guardian approval reply channel closed"))?
        {
            return Err(Status::failed_precondition(
                "Guardian assessment is not denied or already consumed",
            ));
        }
        Ok(Response::new(Empty {}))
    }

    async fn run_user_shell_command(
        &self,
        request: Request<RunUserShellCommandRequest>,
    ) -> Result<Response<RunUserShellCommandResponse>, Status> {
        let req = request.into_inner();
        let managed = self.get_or_create_thread(&req.session_id).await?;
        let (reply, result) = tokio::sync::oneshot::channel();
        let submission_id = managed
            .runtime
            .submit(agent_protocol::Op::RunUserShellCommand {
                command: req.command,
                cwd: (!req.cwd.trim().is_empty()).then(|| PathBuf::from(req.cwd)),
                reply,
            })
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
        let launch = result
            .await
            .map_err(|_| Status::internal("user shell reply channel closed"))?
            .map_err(Status::invalid_argument)?;
        Ok(Response::new(RunUserShellCommandResponse {
            submission_id,
            turn_id: launch.turn_id,
            item_id: launch.item_id,
            attached_to_active_turn: launch.attached_to_active_turn,
        }))
    }

    async fn open_terminal(
        &self,
        request: Request<TerminalOpenRequest>,
    ) -> Result<Response<TerminalSessionResponse>, Status> {
        let req = request.into_inner();
        let scope = PathBuf::from(req.scope.trim());
        if req.scope.trim().is_empty() {
            return Err(Status::invalid_argument("terminal scope is required"));
        }
        let cwd = if req.cwd.trim().is_empty() {
            scope.clone()
        } else {
            PathBuf::from(req.cwd.trim())
        };
        let policy = match req.execution_mode.trim() {
            "" | "project" => tools::context::build_command_sandbox_policy_with_roots(
                &self.memory_dir,
                &scope,
                std::slice::from_ref(&scope),
                None,
                false,
                None,
            )
            .map_err(|error| Status::failed_precondition(error.to_string()))?,
            "system" => sandbox::SandboxPolicy::new(
                types::SandboxMode::DangerFullAccess,
                &scope,
                std::iter::empty(),
                true,
            )
            .map_err(|error| Status::failed_precondition(error.to_string()))?,
            mode => {
                return Err(Status::invalid_argument(format!(
                    "unsupported terminal execution mode: {mode}"
                )))
            }
        };
        // PTY allocation and process startup are synchronous OS operations. Running them on a
        // Tokio worker can stall the whole embedded gRPC service, which makes a second tab (and
        // even its subsequent restart request) fail with `Cancelled: Timeout expired`.
        let info = tokio::task::spawn_blocking(move || {
            let manager = tools::shared_terminal_sessions();
            if req.client_token.trim().is_empty() {
                manager.ensure_shell_with_options(
                    &scope,
                    &cwd,
                    &policy,
                    req.cols.min(u16::MAX.into()) as u16,
                    req.rows.min(u16::MAX.into()) as u16,
                    req.replace_mode_mismatch,
                )
            } else {
                manager.open_desktop_shell(
                    &scope,
                    &cwd,
                    &policy,
                    &req.client_token,
                    req.agent_default,
                    tools::TerminalDimensions {
                        cols: req.cols.min(u16::MAX.into()) as u16,
                        rows: req.rows.min(u16::MAX.into()) as u16,
                    },
                )
            }
        })
        .await
        .map_err(|error| Status::internal(format!("terminal startup task failed: {error}")))?
        .map_err(|error| Status::invalid_argument(error.to_string()))?;
        Ok(Response::new(terminal_session_response(info)))
    }

    async fn read_terminal(
        &self,
        request: Request<TerminalReadRequest>,
    ) -> Result<Response<TerminalReadResponse>, Status> {
        let req = request.into_inner();
        let result = tools::shared_terminal_sessions()
            .read(
                req.id,
                req.cursor,
                req.max_bytes.max(1) as usize,
                req.wait_ms.into(),
            )
            .await
            .map_err(|error| Status::not_found(error.to_string()))?;
        Ok(Response::new(TerminalReadResponse {
            id: result.id,
            data: result.data,
            next_cursor: result.next_cursor,
            dropped: result.dropped,
            running: result.running,
            exit_code: result.exit_code,
        }))
    }

    async fn write_terminal(
        &self,
        request: Request<TerminalWriteRequest>,
    ) -> Result<Response<Empty>, Status> {
        let req = request.into_inner();
        if req.data.len() > 64 * 1024 {
            return Err(Status::invalid_argument("terminal write exceeds 64 KiB"));
        }
        tokio::task::spawn_blocking(move || {
            tools::shared_terminal_sessions().write(req.id, &req.data)
        })
        .await
        .map_err(|error| Status::internal(format!("terminal write task failed: {error}")))?
        .map_err(|error| Status::failed_precondition(error.to_string()))?;
        Ok(Response::new(Empty {}))
    }

    async fn resize_terminal(
        &self,
        request: Request<TerminalResizeRequest>,
    ) -> Result<Response<Empty>, Status> {
        let req = request.into_inner();
        tokio::task::spawn_blocking(move || {
            tools::shared_terminal_sessions().resize(
                req.id,
                req.cols.min(u16::MAX.into()) as u16,
                req.rows.min(u16::MAX.into()) as u16,
            )
        })
        .await
        .map_err(|error| Status::internal(format!("terminal resize task failed: {error}")))?
        .map_err(|error| Status::failed_precondition(error.to_string()))?;
        Ok(Response::new(Empty {}))
    }

    async fn kill_terminal(
        &self,
        request: Request<TerminalIdRequest>,
    ) -> Result<Response<Empty>, Status> {
        let id = request.into_inner().id;
        tokio::task::spawn_blocking(move || tools::shared_terminal_sessions().kill(id))
            .await
            .map_err(|error| Status::internal(format!("terminal kill task failed: {error}")))?
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        Ok(Response::new(Empty {}))
    }

    async fn close_terminal(
        &self,
        request: Request<TerminalIdRequest>,
    ) -> Result<Response<Empty>, Status> {
        let id = request.into_inner().id;
        tokio::task::spawn_blocking(move || tools::shared_terminal_sessions().close(id))
            .await
            .map_err(|error| Status::internal(format!("terminal close task failed: {error}")))?
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        Ok(Response::new(Empty {}))
    }

    /// 文生图流：先推送 progress，再推送 `image_data` 或 error。
    ///
    /// 默认 provider 为 `google`；模型空则用供应商默认图片模型；api_key 空则读环境变量。
    async fn generate_image(
        &self,
        request: Request<ImageRequest>,
    ) -> Result<Response<Self::GenerateImageStream>, Status> {
        let req = request.into_inner();
        let provider_name = if req.provider.is_empty() {
            "google".to_string()
        } else {
            req.provider
        };
        let prompt = req.prompt;
        let model = req.model;
        let api_key = req.api_key;
        let base_url = req.base_url;
        let input_images = if req.input_image.is_empty() {
            Vec::new()
        } else {
            vec![providers::types::ImageInput {
                data: req.input_image,
                mime_type: if req.input_image_mime.trim().is_empty() {
                    "image/png".to_string()
                } else {
                    req.input_image_mime
                },
                filename: if req.input_image_name.trim().is_empty() {
                    "reference.png".to_string()
                } else {
                    req.input_image_name
                },
            }]
        };
        let image_options = providers::types::ImageGenConfig {
            width: (req.width > 0).then_some(req.width as u32),
            height: (req.height > 0).then_some(req.height as u32),
            n: if req.count > 0 { req.count as u32 } else { 1 },
            input_images,
            ..providers::types::ImageGenConfig::default()
        };

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<ImageEvent, Status>>(8);

        tokio::spawn(async move {
            let _ = tx
                .send(Ok(ImageEvent {
                    payload: Some(proto::image_event::Payload::Progress(format!(
                        "正在使用 {provider_name} 生成图片…"
                    ))),
                }))
                .await;

            if !providers::dispatch::supports_image_gen(&provider_name) {
                let _ = tx
                    .send(Ok(ImageEvent {
                        payload: Some(proto::image_event::Payload::Error(format!(
                            "{provider_name} 不支持图片生成"
                        ))),
                    }))
                    .await;
                return;
            }

            let default_model = providers::image_gen::default_image_model(&provider_name);
            let config = ProviderConfig {
                model: if model.is_empty() {
                    default_model.to_string()
                } else {
                    model
                },
                api_key: {
                    let from_req = api_key.trim().to_string();
                    if !from_req.is_empty() {
                        from_req
                    } else {
                        providers::read_env_api_key(&provider_name).unwrap_or_default()
                    }
                },
                base_url: {
                    let from_req = base_url.trim().to_string();
                    if from_req.is_empty() {
                        None
                    } else {
                        Some(from_req)
                    }
                },
                ..ProviderConfig::default()
            };

            match providers::dispatch::generate_image_with_options(
                &provider_name,
                &prompt,
                &config,
                &image_options,
            )
            .await
            {
                Ok(images) => {
                    for img in images {
                        let _ = tx
                            .send(Ok(ImageEvent {
                                payload: Some(proto::image_event::Payload::ImageData(img.data)),
                            }))
                            .await;
                    }
                }
                Err(err) => {
                    let _ = tx
                        .send(Ok(ImageEvent {
                            payload: Some(proto::image_event::Payload::Error(err.to_string())),
                        }))
                        .await;
                }
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    /// 列出本机已安装技能；`category` 字段编码启用状态与 `source_dir`。
    async fn list_skills(&self, _request: Request<Empty>) -> Result<Response<SkillList>, Status> {
        let skills = skills::list_installed()
            .into_iter()
            .map(|s| SkillInfo {
                name: s.name,
                description: s.description,
                category: format!(
                    "{} · {}",
                    if s.enabled { "enabled" } else { "disabled" },
                    s.source_dir
                ),
            })
            .collect();
        Ok(Response::new(SkillList { skills }))
    }

    /// 按名称加载 Skill 正文（可选附带 `params_json`），以流形式返回 output → done。
    ///
    /// 若 `tools/enabled.json` 中 `skills=false`，返回 error 事件。
    async fn execute_skill(
        &self,
        request: Request<SkillRequest>,
    ) -> Result<Response<Self::ExecuteSkillStream>, Status> {
        let req = request.into_inner();
        let name = req.name;
        let params_json = req.params_json;
        let (tx, rx) = tokio::sync::mpsc::channel(8);

        tokio::spawn(async move {
            if !home::is_toolset_enabled("skills") {
                let _ = tx
                    .send(Ok(SkillEvent {
                        payload: Some(proto::skill_event::Payload::Error(
                            "技能工具已禁用（tools/enabled.json → skills=false）".into(),
                        )),
                    }))
                    .await;
                return;
            }
            let load_result =
                tokio::task::spawn_blocking(move || skills::load_skill_by_name(&name)).await;

            match load_result {
                Ok(Ok(skill)) => {
                    let mut output = skill.content;
                    if !params_json.is_empty() && params_json != "{}" {
                        output.push_str("\n\n---\n\n# 调用参数\n");
                        output.push_str(&params_json);
                    }
                    if tx
                        .send(Ok(SkillEvent {
                            payload: Some(proto::skill_event::Payload::Output(output)),
                        }))
                        .await
                        .is_err()
                    {
                        return;
                    }
                    let _ = tx
                        .send(Ok(SkillEvent {
                            payload: Some(proto::skill_event::Payload::Done(true)),
                        }))
                        .await;
                }
                Ok(Err(err)) => {
                    let _ = tx
                        .send(Ok(SkillEvent {
                            payload: Some(proto::skill_event::Payload::Error(err.to_string())),
                        }))
                        .await;
                }
                Err(err) => {
                    let _ = tx
                        .send(Ok(SkillEvent {
                            payload: Some(proto::skill_event::Payload::Error(err.to_string())),
                        }))
                        .await;
                }
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    /// 列出 MCP 服务器状态与工具名。
    ///
    /// 优先从当前 active agent 的会话 Hub 取实时连接；否则回退磁盘配置。
    async fn list_mcp_servers(
        &self,
        request: Request<McpServerListRequest>,
    ) -> Result<Response<McpServerList>, Status> {
        let request = request.into_inner();
        let requested_agent_id = request.agent_id.trim();
        let agent_id = if requested_agent_id.is_empty() {
            home::active_agent_id(&self.memory_dir)
        } else {
            requested_agent_id.to_string()
        };
        let project_root = (!request.project_root.trim().is_empty())
            .then(|| PathBuf::from(request.project_root.trim()));

        // 优先：指定 agent 的 hub（实时连接状态）；勿用任意会话以免串 agent。
        let sessions = self.sessions.read().await;
        for handle in sessions.values() {
            let hub = handle.mcp_hub();
            let hub_guard = hub.lock().await;
            if hub_guard.agent_id() != Some(agent_id.as_str()) {
                continue;
            }
            let servers = hub_guard
                .server_status()
                .into_iter()
                .map(mcp_server_info)
                .collect();
            return Ok(Response::new(McpServerList { servers }));
        }
        drop(sessions);

        let configs = mcp::load_mcp_servers_layered(project_root.as_deref()).unwrap_or_default();
        let servers = configs
            .into_iter()
            .map(|c| {
                let oauth_available = mcp::auth::is_oauth_available(&c);
                proto::McpServerInfo {
                    id: c.id,
                    name: c.name,
                    status: if c.enabled {
                        "configured".into()
                    } else {
                        "disabled".into()
                    },
                    tools: c.discovered.iter().map(|d| d.name.clone()).collect(),
                    required: c.required,
                    error: String::new(),
                    retryable: false,
                    retry_attempt: 0,
                    next_retry_at_unix_ms: 0,
                    oauth_available,
                    authenticated: false,
                }
            })
            .collect();
        Ok(Response::new(McpServerList { servers }))
    }

    /// 清除退避并立即重连指定 Agent Hub 中的 MCP Server。
    async fn reconnect_mcp_server(
        &self,
        request: Request<McpReconnectRequest>,
    ) -> Result<Response<McpServerList>, Status> {
        let request = request.into_inner();
        let agent_id = request.agent_id.trim();
        let server_id = request.server_id.trim();
        if agent_id.is_empty() || server_id.is_empty() {
            return Err(Status::invalid_argument(
                "agent_id and server_id are required",
            ));
        }

        let handles: Vec<_> = self.sessions.read().await.values().cloned().collect();
        for handle in handles {
            if handle.agent_id() != agent_id {
                continue;
            }
            if let Err(error) = handle.reconnect_mcp_server(server_id).await {
                if error
                    .downcast_ref::<mcp::RequiredMcpServersError>()
                    .is_none()
                {
                    return Err(Status::failed_precondition(error.to_string()));
                }
            }
            let hub = handle.mcp_hub();
            let servers = hub
                .lock()
                .await
                .server_status()
                .into_iter()
                .map(mcp_server_info)
                .collect();
            return Ok(Response::new(McpServerList { servers }));
        }

        Err(Status::failed_precondition(
            "no active Agent runtime is available for MCP reconnect",
        ))
    }

    /// 召回 MEMORY.md / USER.md 文本，并按 `query` 搜索历史消息（`limit` 至少 1）。
    ///
    /// # 错误
    /// SessionStore 失败 → `internal`。
    async fn query_memory(
        &self,
        request: Request<MemoryQuery>,
    ) -> Result<Response<MemoryResult>, Status> {
        let query = request.into_inner();
        let memory = MemoryManager::new(self.memory_dir.clone())
            .map_err(|e| Status::internal(e.to_string()))?;
        let (memory_content, user_content) = memory.prompt_content();
        let sessions = open_sessions(&self.memory_dir)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let sessions = sessions
            .search_messages(&query.query, None, None, query.limit.max(1) as i64)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .into_iter()
            .map(|hit| {
                let summary = if hit.snippet.trim().is_empty() {
                    hit.context
                } else {
                    hit.snippet
                };
                ProtoSessionSnippet {
                    session_id: hit.session_id,
                    summary,
                }
            })
            .collect();

        Ok(Response::new(MemoryResult {
            memory_content,
            user_content,
            sessions,
        }))
    }

    /// 在 `memory_dir` 沙箱下列举文件；越界路径返回 `invalid_argument`。
    async fn list_files(
        &self,
        request: Request<FileListRequest>,
    ) -> Result<Response<FileListResponse>, Status> {
        let req = request.into_inner();
        let entries = crate::grpc::files::list_directory(&self.memory_dir, &req.path, req.depth)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        Ok(Response::new(FileListResponse { entries }))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::ListenerCommand;

    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    use agent::streaming::{run_multi_turn_events_with_responses_fn, ResponsesOverride};
    use providers::CompletionStream;
    use tempfile::TempDir;

    async fn wait_for_agent_thread_extensions(
        managed: &Arc<ManagedThread>,
        memory_dir: &std::path::Path,
        root_thread_id: &str,
        ready: impl Fn(&[agent_protocol::ExtensionItem]) -> bool,
    ) -> Vec<agent_protocol::ExtensionItem> {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                managed.runtime.flush_rollout().await.unwrap();
                let rollout_root = memory_dir.join("sessions").join("rollouts");
                let extensions =
                    match agent_rollout::find_rollout(&rollout_root, root_thread_id).unwrap() {
                        Some(path) => agent_rollout::read_rollout(&path)
                            .await
                            .unwrap()
                            .into_iter()
                            .filter_map(|item| match item {
                                agent_rollout::RolloutItem::EventMsg(
                                    agent_protocol::EventMsg::ItemCompleted(
                                        agent_protocol::ItemEvent {
                                            item: agent_protocol::TurnItem::Extension(extension),
                                            ..
                                        },
                                    ),
                                ) if extension.namespace == "astro.agent_thread"
                                    || extension.namespace == "astro.agent_thread_resync" =>
                                {
                                    Some(extension)
                                }
                                _ => None,
                            })
                            .collect::<Vec<_>>(),
                        None => Vec::new(),
                    };
                if ready(&extensions) {
                    return extensions;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("agent thread extensions were not materialized")
    }

    #[test]
    fn only_successful_run_allows_post_turn_side_effects() {
        assert!(allows_post_turn_side_effects("success"));
        for outcome in ["error", "interrupt", "hitl_waiting", ""] {
            assert!(!allows_post_turn_side_effects(outcome), "outcome={outcome}");
        }
    }

    #[tokio::test]
    async fn agent_thread_projection_carries_unified_stream_generation() {
        let dir = TempDir::new().unwrap();
        let control = subagents::AgentControl::open(
            "projection-root".into(),
            subagents::AgentGraphStore::open(dir.path().join("graph.db"))
                .await
                .unwrap(),
            subagents::Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 4,
            },
        )
        .await
        .unwrap();
        control
            .reserve_spawn(&subagents::AgentPath::root(), "child")
            .await
            .unwrap()
            .commit()
            .await
            .unwrap();
        let observation = control
            .next_activity_after(
                subagents::ActivityCursor(0),
                std::time::Duration::from_secs(1),
            )
            .await;
        let subagents::ActivityObservation::Activity(activity) = observation else {
            panic!("spawn activity missing");
        };
        let projection = agent_thread_projection(*activity, "generation-2").unwrap();
        assert_eq!(projection["stream_id"], "generation-2");
        assert_eq!(projection["activity_sequence"], 1);
        assert_eq!(projection["canonical_path"], "/root/child");
        assert_eq!(projection["activity_kind"], "spawned");
    }

    #[tokio::test]
    async fn agent_thread_watcher_replays_then_skips_projectionless_activity() {
        let dir = TempDir::new().unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let root = "agent-thread-replay-root";
        let managed = service.get_or_create_thread(root).await.unwrap();
        let control = Arc::new(
            subagents::AgentControl::open(
                root.into(),
                subagents::AgentGraphStore::open(dir.path().join("agent-graph.db"))
                    .await
                    .unwrap(),
                subagents::Limits {
                    max_threads: 8,
                    max_depth: 4,
                    max_running: 4,
                },
            )
            .await
            .unwrap(),
        );
        let spawn = control
            .reserve_spawn(&subagents::AgentPath::root(), "child")
            .await
            .unwrap();
        let child_thread_id = spawn.thread_id().to_string();
        spawn.commit().await.unwrap();

        service
            .attach_agent_thread_watcher(
                root,
                "default",
                Arc::clone(&control),
                Arc::clone(&managed),
            )
            .await;
        control.notify_main_steer();
        control
            .record_runner_event(&child_thread_id, subagents::RunnerEvent::RuntimeTerminated)
            .await
            .unwrap();

        let extensions = wait_for_agent_thread_extensions(&managed, dir.path(), root, |items| {
            let Some(stream_id) = items
                .iter()
                .find(|item| item.namespace == "astro.agent_thread")
                .map(|item| &item.payload["stream_id"])
            else {
                return false;
            };
            items
                .iter()
                .filter(|item| {
                    item.namespace == "astro.agent_thread"
                        && item.payload["stream_id"] == *stream_id
                })
                .count()
                >= 2
                && items.iter().any(|item| {
                    item.namespace == "astro.agent_thread_resync"
                        && item.payload["stream_id"] == *stream_id
                })
        })
        .await;
        let projected = extensions
            .iter()
            .filter(|extension| extension.namespace == "astro.agent_thread")
            .collect::<Vec<_>>();
        assert_eq!(projected.len(), 2, "MainSteer must not create a projection");
        let stream_id = &projected[0].payload["stream_id"];
        let resync = extensions
            .iter()
            .find(|extension| {
                extension.namespace == "astro.agent_thread_resync"
                    && extension.payload["stream_id"] == *stream_id
            })
            .unwrap();
        assert_eq!(projected[0].payload["activity_sequence"], 1);
        assert_eq!(projected[0].payload["activity_kind"], "spawned");
        assert_eq!(projected[1].payload["activity_sequence"], 3);
        assert_eq!(projected[1].payload["activity_kind"], "edge_closed");
        assert_eq!(
            projected[0].payload["stream_id"], resync.payload["stream_id"],
            "replayed and live activity must share the watcher generation"
        );
        assert_eq!(
            projected[1].payload["stream_id"],
            resync.payload["stream_id"]
        );
    }

    #[tokio::test]
    async fn agent_thread_watcher_resyncs_retention_gap_before_next_projection() {
        let dir = TempDir::new().unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let root = "agent-thread-gap-root";
        let managed = service.get_or_create_thread(root).await.unwrap();
        let control = Arc::new(
            subagents::AgentControl::open(
                root.into(),
                subagents::AgentGraphStore::open(dir.path().join("agent-gap-graph.db"))
                    .await
                    .unwrap(),
                subagents::Limits {
                    max_threads: 8,
                    max_depth: 4,
                    max_running: 4,
                },
            )
            .await
            .unwrap(),
        );
        for _ in 0..1_025 {
            control.notify_main_steer();
        }

        service
            .attach_agent_thread_watcher(
                root,
                "default",
                Arc::clone(&control),
                Arc::clone(&managed),
            )
            .await;
        let gap_extensions =
            wait_for_agent_thread_extensions(&managed, dir.path(), root, |items| {
                items.iter().any(|item| {
                    item.namespace == "astro.agent_thread_resync"
                        && item.payload["reason"] == "activity_gap"
                })
            })
            .await;
        let stream_id = gap_extensions
            .iter()
            .find(|item| {
                item.namespace == "astro.agent_thread_resync"
                    && item.payload["reason"] == "activity_gap"
            })
            .unwrap()
            .payload["stream_id"]
            .clone();
        control
            .reserve_spawn(&subagents::AgentPath::root(), "after_gap")
            .await
            .unwrap()
            .commit()
            .await
            .unwrap();

        let extensions = wait_for_agent_thread_extensions(&managed, dir.path(), root, |items| {
            ["agent_control_generation_changed", "activity_gap"]
                .into_iter()
                .all(|reason| {
                    items.iter().any(|item| {
                        item.namespace == "astro.agent_thread_resync"
                            && item.payload["stream_id"] == stream_id
                            && item.payload["reason"] == reason
                    })
                })
                && items.iter().any(|item| {
                    item.namespace == "astro.agent_thread" && item.payload["stream_id"] == stream_id
                })
        })
        .await;
        let projection = extensions
            .iter()
            .find(|extension| extension.namespace == "astro.agent_thread")
            .unwrap();
        let stream_id = &projection.payload["stream_id"];
        let resyncs = extensions
            .iter()
            .filter(|extension| {
                extension.namespace == "astro.agent_thread_resync"
                    && extension.payload["stream_id"] == *stream_id
            })
            .collect::<Vec<_>>();
        assert_eq!(resyncs.len(), 2);
        assert_eq!(
            resyncs[0].payload["reason"],
            "agent_control_generation_changed"
        );
        assert_eq!(resyncs[1].payload["reason"], "activity_gap");
        assert_eq!(
            resyncs[0].payload["stream_id"],
            resyncs[1].payload["stream_id"]
        );

        assert_eq!(projection.payload["canonical_path"], "/root/after_gap");
        assert_eq!(projection.payload["activity_sequence"], 1_026);
        assert_eq!(
            projection.payload["stream_id"],
            resyncs[0].payload["stream_id"]
        );
    }

    #[tokio::test]
    async fn release_session_runtime_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let session_id = "release-session-runtime-idempotent";

        let session = service.get_session(session_id).await.unwrap();
        let gate = HitlGate::new(session_id.to_string());
        let (ui_tx, _ui_rx) = tokio::sync::mpsc::unbounded_channel();
        let pause = service
            .admit_pause_generation(session_id, &session, Arc::clone(&gate), ui_tx)
            .await
            .unwrap();

        let request = Request::new(ChatControlRequest {
            session_id: session_id.to_string(),
            action: ChatControlAction::ReleaseSession as i32,
        });
        service.chat_control(request).await.expect("first release");
        drop(pause);
        drop(gate);

        let request = Request::new(ChatControlRequest {
            session_id: session_id.to_string(),
            action: ChatControlAction::ReleaseSession as i32,
        });
        service.chat_control(request).await.expect("second release");

        assert!(service.sessions.read().await.get(session_id).is_none());
        assert!(
            agent::exec::dispatch::active_root_runtime_material(dir.path(), session_id)
                .await
                .unwrap()
                .is_none()
        );
        assert!(service
            .pause_controls
            .read()
            .expect("pause registry lock poisoned")
            .get(session_id)
            .is_none());
        assert!(service.hitl_registry.get(session_id).await.is_none());
        assert!(!service
            .generation_operations
            .lock()
            .unwrap()
            .contains_key(session_id));
    }

    #[tokio::test]
    async fn cancel_with_stale_pause_does_not_recreate_released_session() {
        let dir = TempDir::new().unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let session_id = "released-session-with-stale-pause";
        service
            .pause_controls
            .write()
            .expect("pause registry lock poisoned")
            .insert(
                session_id.into(),
                PauseRegistration {
                    control: PauseControl::new(),
                    session: Weak::new(),
                    hitl_gate: HitlGate::new(session_id),
                    ui_generation: {
                        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
                        service.hook_runtime.ui_slot.install_tx(session_id, tx)
                    },
                    operation: service.generation_operation(session_id),
                },
            );
        assert!(service.sessions.read().await.get(session_id).is_none());

        service
            .chat_control(Request::new(ChatControlRequest {
                session_id: session_id.into(),
                action: ChatControlAction::ChatControlCancel as i32,
            }))
            .await
            .expect("stale cancel should remain idempotent");

        assert!(
            service.sessions.read().await.get(session_id).is_none(),
            "cancel must not lazily recreate a released session"
        );
    }

    async fn spawn_pending_turn(
        session: SessionHandle,
    ) -> (
        tokio::task::JoinHandle<()>,
        tokio::sync::mpsc::Receiver<anyhow::Result<agent_protocol::Event>>,
    ) {
        let chat: ResponsesOverride = Arc::new(|_, _, _| {
            Box::pin(async move { Ok(Box::pin(futures::stream::pending()) as CompletionStream) })
        });
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let handle = tokio::spawn(run_multi_turn_events_with_responses_fn(
            session,
            chat,
            ProviderConfig {
                model: "scripted".into(),
                ..ProviderConfig::default()
            },
            "system".into(),
            PauseControl::new(),
            None,
            tx,
        ));
        let started = rx.recv().await.expect("pending turn should start").unwrap();
        assert!(matches!(
            started.msg,
            agent_protocol::EventMsg::TurnStarted(_)
        ));
        (handle, rx)
    }

    #[tokio::test]
    async fn stale_cancel_targets_the_session_generation_that_registered_pause() {
        let dir = TempDir::new().unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let session_id = "cancel-generation-race";
        let old_session = service.get_session(session_id).await.unwrap();
        let (old_turn, _old_rx) = spawn_pending_turn(Arc::clone(&old_session)).await;
        let old_gate = HitlGate::new(session_id);
        let (old_ui_tx, _old_ui_rx) = tokio::sync::mpsc::unbounded_channel();
        let _old_pause = service
            .admit_pause_generation(session_id, &old_session, old_gate, old_ui_tx)
            .await
            .unwrap();

        service.sessions.write().await.remove(session_id);
        let replacement = service.get_session(session_id).await.unwrap();
        let (replacement_turn, _replacement_rx) =
            spawn_pending_turn(Arc::clone(&replacement)).await;

        service
            .chat_control(Request::new(ChatControlRequest {
                session_id: session_id.into(),
                action: ChatControlAction::ChatControlCancel as i32,
            }))
            .await
            .expect("stale cancel should remain valid for its exact generation");

        assert!(
            old_session.cancel_signal().is_cancelled(),
            "the pause registration must retain the old session generation"
        );
        assert!(
            !replacement.cancel_signal().is_cancelled(),
            "a same-id replacement session must not be cancelled"
        );

        replacement
            .abort_all_tasks(TurnAbortReason::Replaced)
            .await
            .unwrap();
        old_turn.await.unwrap();
        replacement_turn.await.unwrap();
    }

    #[tokio::test]
    async fn stale_session_arc_cannot_admit_over_its_same_id_replacement() {
        let dir = TempDir::new().unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let session_id = "stale-admission-session";
        let stale = service.get_session(session_id).await.unwrap();
        service.sessions.write().await.remove(session_id);
        let replacement = service.get_session(session_id).await.unwrap();
        assert!(!Arc::ptr_eq(&stale, &replacement));

        let (stale_ui_tx, _stale_ui_rx) = tokio::sync::mpsc::unbounded_channel();
        assert!(
            service
                .admit_pause_generation(session_id, &stale, HitlGate::new(session_id), stale_ui_tx,)
                .await
                .is_none(),
            "an Arc removed from sessions must not overwrite the replacement generation"
        );
        assert!(service
            .pause_controls
            .read()
            .expect("pause registry lock poisoned")
            .get(session_id)
            .is_none());

        let (replacement_ui_tx, _replacement_ui_rx) = tokio::sync::mpsc::unbounded_channel();
        assert!(service
            .admit_pause_generation(
                session_id,
                &replacement,
                HitlGate::new(session_id),
                replacement_ui_tx,
            )
            .await
            .is_some());
    }

    #[tokio::test]
    async fn concurrent_generation_setup_and_install_keep_request_settings_together() {
        let dir = TempDir::new().unwrap();
        let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
        let session_id = "generation-config-isolation";
        let session = service.get_session(session_id).await.unwrap();
        let (ui_tx, _ui_rx) = tokio::sync::mpsc::unbounded_channel();
        let registration = service
            .admit_pause_generation(session_id, &session, HitlGate::new(session_id), ui_tx)
            .await
            .unwrap();
        let first_setup_started = Arc::new(tokio::sync::Notify::new());
        let release_first_setup = Arc::new(tokio::sync::Notify::new());
        let second_setup_started = Arc::new(AtomicBool::new(false));
        let first = {
            let service = Arc::clone(&service);
            let session = Arc::clone(&session);
            let launch_session = Arc::clone(&session);
            let registration = registration.clone();
            let first_setup_started = Arc::clone(&first_setup_started);
            let release_first_setup = Arc::clone(&release_first_setup);
            tokio::spawn(async move {
                service
                    .launch_current_pause_generation_with_setup(
                        session_id,
                        &registration,
                        &session,
                        move |session| async move {
                            session
                                .set_interaction_mode(tools::InteractionMode::Plan)
                                .await;
                            session.set_temperature(0.2);
                            first_setup_started.notify_one();
                            release_first_setup.notified().await;
                        },
                        move || {
                            let session = Arc::clone(&launch_session);
                            async move { (session.interaction_mode().await, session.temperature()) }
                        },
                    )
                    .await
                    .unwrap()
            })
        };
        first_setup_started.notified().await;

        let second = {
            let service = Arc::clone(&service);
            let session = Arc::clone(&session);
            let launch_session = Arc::clone(&session);
            let registration = registration.clone();
            let second_setup_started = Arc::clone(&second_setup_started);
            tokio::spawn(async move {
                service
                    .launch_current_pause_generation_with_setup(
                        session_id,
                        &registration,
                        &session,
                        move |session| async move {
                            second_setup_started.store(true, Ordering::SeqCst);
                            session
                                .set_interaction_mode(tools::InteractionMode::Agent)
                                .await;
                            session.set_temperature(1.4);
                        },
                        move || {
                            let session = Arc::clone(&launch_session);
                            async move { (session.interaction_mode().await, session.temperature()) }
                        },
                    )
                    .await
                    .unwrap()
            })
        };
        tokio::task::yield_now().await;
        assert!(
            !second_setup_started.load(Ordering::SeqCst),
            "the second request must not apply settings before the first installs"
        );
        release_first_setup.notify_one();

        let first = first.await.unwrap();
        let second = second.await.unwrap();
        assert_eq!(first, (tools::InteractionMode::Plan, 0.2));
        assert_eq!(second, (tools::InteractionMode::Agent, 1.4));
    }

    #[tokio::test]
    async fn cancelled_generation_launch_cleans_registration_and_restores_settings() {
        let dir = TempDir::new().unwrap();
        let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
        let session_id = "cancelled-generation-launch";
        let session = service.get_session(session_id).await.unwrap();
        let initial_temperature = session.temperature();
        let (ui_tx, mut ui_rx) = tokio::sync::mpsc::unbounded_channel();
        let gate = HitlGate::new(session_id);
        let registration = service
            .admit_pause_generation(session_id, &session, Arc::clone(&gate), ui_tx)
            .await
            .unwrap();
        let setup_applied = Arc::new(tokio::sync::Notify::new());
        let release_launch = Arc::new(tokio::sync::Notify::new());
        let launch_owner = tokio::spawn({
            let service = Arc::clone(&service);
            let session = Arc::clone(&session);
            let setup_applied = Arc::clone(&setup_applied);
            let release_launch = Arc::clone(&release_launch);
            async move {
                service
                    .launch_current_pause_generation_with_setup(
                        session_id,
                        &registration,
                        &session,
                        move |session| async move {
                            session.set_temperature(0.2);
                            session
                                .set_interaction_mode(tools::InteractionMode::Plan)
                                .await;
                            setup_applied.notify_one();
                        },
                        move || async move {
                            release_launch.notified().await;
                        },
                    )
                    .await
            }
        });
        setup_applied.notified().await;
        launch_owner.abort();
        assert!(launch_owner.await.unwrap_err().is_cancelled());
        release_launch.notify_one();

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if service
                    .pause_controls
                    .read()
                    .expect("pause registry lock poisoned")
                    .get(session_id)
                    .is_none()
                    && service.hitl_registry.get(session_id).await.is_none()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("abandoned generation must be cleaned by service-owned work");
        assert!(matches!(
            ui_rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected)
        ));

        let (next_ui_tx, _next_ui_rx) = tokio::sync::mpsc::unbounded_channel();
        let next = service
            .admit_pause_generation(session_id, &session, HitlGate::new(session_id), next_ui_tx)
            .await
            .unwrap();
        let inherited = service
            .launch_current_pause_generation_with_setup(
                session_id,
                &next,
                &session,
                |_| async {},
                {
                    let session = Arc::clone(&session);
                    move || async move { session.temperature() }
                },
            )
            .await
            .unwrap();
        assert_eq!(inherited, initial_temperature);
    }

    #[tokio::test]
    async fn abandoned_launch_reply_after_send_rolls_back_exact_generation() {
        let dir = TempDir::new().unwrap();
        let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
        let session_id = "abandoned-launch-reply-after-send";
        let session = service.get_session(session_id).await.unwrap();
        let initial_temperature = session.temperature();
        let initial_mode = session.interaction_mode().await;
        let gate = HitlGate::new(session_id);
        let mut gate_wait = gate
            .begin_wait(agent::Interrupt {
                id: "abandoned-launch-reply-gate".into(),
                ..Default::default()
            })
            .await;
        let (ui_tx, mut ui_rx) = tokio::sync::mpsc::unbounded_channel();
        let registration = service
            .admit_pause_generation(session_id, &session, Arc::clone(&gate), ui_tx)
            .await
            .unwrap();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let worker = tokio::spawn({
            let service = Arc::clone(&service);
            let worker_session = Arc::clone(&session);
            let launch_session = Arc::clone(&session);
            async move {
                service
                    .run_generation_launch_worker(
                        session_id.to_string(),
                        registration,
                        worker_session,
                        move |session| async move {
                            session.set_temperature(0.2);
                            session
                                .set_interaction_mode(tools::InteractionMode::Plan)
                                .await;
                        },
                        move || async move { spawn_pending_turn(launch_session).await },
                        reply_tx,
                    )
                    .await;
            }
        });

        let guarded_reply = reply_rx
            .await
            .expect("worker must send its launch reply")
            .expect("current generation must launch");
        tokio::task::yield_now().await;
        assert!(
            !worker.is_finished(),
            "a successful reply send must leave the worker waiting for caller acceptance"
        );
        assert_eq!(session.temperature(), 0.2);
        assert_eq!(
            session.interaction_mode().await,
            tools::InteractionMode::Plan
        );
        assert!(!session.cancel_signal().is_cancelled());

        drop(guarded_reply);
        tokio::time::timeout(std::time::Duration::from_secs(1), worker)
            .await
            .expect("false acknowledgement must release the launch worker")
            .unwrap();

        assert!(session.cancel_signal().is_cancelled());
        assert_eq!(session.temperature(), initial_temperature);
        assert_eq!(session.interaction_mode().await, initial_mode);
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), &mut gate_wait)
                .await
                .expect("false acknowledgement must resolve the exact gate")
                .expect("gate resolution")
                .status,
            "cancelled"
        );
        assert!(service
            .pause_controls
            .read()
            .expect("pause registry lock poisoned")
            .get(session_id)
            .is_none());
        assert!(service.hitl_registry.get(session_id).await.is_none());
        loop {
            match ui_rx.try_recv() {
                Ok(_) => {}
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                    panic!("false acknowledgement must detach the exact UI generation")
                }
            }
        }
        assert!(!service
            .generation_operations
            .lock()
            .unwrap()
            .contains_key(session_id));
    }

    #[tokio::test]
    async fn abandoned_prepared_admission_after_send_does_not_commit() {
        let dir = TempDir::new().unwrap();
        let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
        let session_id = "abandoned-admission-reply-after-send";
        let session = service.get_session(session_id).await.unwrap();
        let live_gate = HitlGate::new(session_id);
        let (live_ui_tx, mut live_ui_rx) = tokio::sync::mpsc::unbounded_channel();
        let live = service
            .admit_pause_generation(session_id, &session, Arc::clone(&live_gate), live_ui_tx)
            .await
            .unwrap();
        let abandoned_gate = HitlGate::new(session_id);
        let (abandoned_ui_tx, mut abandoned_ui_rx) = tokio::sync::mpsc::unbounded_channel();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let worker = tokio::spawn({
            let service = Arc::clone(&service);
            let session = Arc::clone(&session);
            let abandoned_gate = Arc::clone(&abandoned_gate);
            async move {
                service
                    .run_admit_pause_generation_worker(
                        session_id.to_string(),
                        session,
                        abandoned_gate,
                        abandoned_ui_tx,
                        reply_tx,
                    )
                    .await;
            }
        });

        let prepared = reply_rx
            .await
            .expect("worker must send its admission reply")
            .expect("current session must be admitted");
        tokio::time::timeout(std::time::Duration::from_secs(1), worker)
            .await
            .expect("prepare worker must finish after transferring the commit capability")
            .unwrap();
        assert!(service
            .pause_controls
            .read()
            .expect("pause registry lock poisoned")
            .get(session_id)
            .is_some_and(|current| Arc::ptr_eq(&current.control, &live.control)));
        assert!(!live.control.is_cancelled());

        drop(prepared);
        assert!(service
            .pause_controls
            .read()
            .expect("pause registry lock poisoned")
            .get(session_id)
            .is_some_and(|current| Arc::ptr_eq(&current.control, &live.control)));
        assert!(!live.control.is_cancelled());
        assert!(service
            .hitl_registry
            .get(session_id)
            .await
            .is_some_and(|gate| Arc::ptr_eq(&gate, &live_gate)));
        loop {
            match abandoned_ui_rx.try_recv() {
                Ok(_) => {}
                Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
                Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                    panic!("false acknowledgement must detach the abandoned UI generation")
                }
            }
        }
        let _ = service.hook_runtime.plugin.fire(
            ::hooks::PRE_LLM_CALL,
            &::hooks::HookPayload {
                session_id: session_id.into(),
                ..Default::default()
            },
        );
        assert_eq!(live_ui_rx.try_recv().unwrap().name, ::hooks::PRE_LLM_CALL);
        assert!(service
            .generation_operations
            .lock()
            .unwrap()
            .contains_key(session_id));

        let replacement_gate = HitlGate::new(session_id);
        let (replacement_ui_tx, mut replacement_ui_rx) = tokio::sync::mpsc::unbounded_channel();
        let replacement = service
            .admit_pause_generation(
                session_id,
                &session,
                Arc::clone(&replacement_gate),
                replacement_ui_tx,
            )
            .await
            .expect("a replacement must admit after abandoned handoff cleanup");
        assert!(!replacement.control.is_cancelled());
        assert!(service
            .hitl_registry
            .get(session_id)
            .await
            .is_some_and(|gate| Arc::ptr_eq(&gate, &replacement_gate)));
        let _ = service.hook_runtime.plugin.fire(
            ::hooks::PRE_LLM_CALL,
            &::hooks::HookPayload {
                session_id: session_id.into(),
                ..Default::default()
            },
        );
        assert_eq!(
            replacement_ui_rx.try_recv().unwrap().name,
            ::hooks::PRE_LLM_CALL
        );
    }

    #[tokio::test]
    async fn abandoned_prepared_admission_without_live_generation_prunes_operation() {
        let dir = TempDir::new().unwrap();
        let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
        let session_id = "abandoned-prepared-admission-prunes-operation";
        let session = service.get_session(session_id).await.unwrap();
        let (ui_tx, _ui_rx) = tokio::sync::mpsc::unbounded_channel();
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let worker = tokio::spawn({
            let service = Arc::clone(&service);
            let session = Arc::clone(&session);
            async move {
                service
                    .run_admit_pause_generation_worker(
                        session_id.to_string(),
                        session,
                        HitlGate::new(session_id),
                        ui_tx,
                        reply_tx,
                    )
                    .await;
            }
        });

        let prepared = reply_rx
            .await
            .expect("worker must send its prepared admission")
            .expect("current session must prepare admission");
        worker.await.unwrap();
        assert!(service
            .generation_operations
            .lock()
            .unwrap()
            .contains_key(session_id));

        drop(prepared);
        assert!(!service
            .generation_operations
            .lock()
            .unwrap()
            .contains_key(session_id));
    }

    #[tokio::test]
    async fn closed_admission_reply_does_not_replace_live_generation() {
        let dir = TempDir::new().unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let session_id = "closed-admission-reply-live-generation";
        let session = service.get_session(session_id).await.unwrap();
        let live_gate = HitlGate::new(session_id);
        let (live_ui_tx, mut live_ui_rx) = tokio::sync::mpsc::unbounded_channel();
        let live = service
            .admit_pause_generation(session_id, &session, Arc::clone(&live_gate), live_ui_tx)
            .await
            .unwrap();

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        drop(reply_rx);
        let (abandoned_ui_tx, _abandoned_ui_rx) = tokio::sync::mpsc::unbounded_channel();
        service
            .run_admit_pause_generation_worker(
                session_id.to_string(),
                Arc::clone(&session),
                HitlGate::new(session_id),
                abandoned_ui_tx,
                reply_tx,
            )
            .await;

        assert!(
            !live.control.is_cancelled(),
            "a closed caller must not irreversibly cancel the live generation"
        );
        assert!(service
            .pause_controls
            .read()
            .expect("pause registry lock poisoned")
            .get(session_id)
            .is_some_and(|current| Arc::ptr_eq(&current.control, &live.control)));
        assert!(service
            .hitl_registry
            .get(session_id)
            .await
            .is_some_and(|gate| Arc::ptr_eq(&gate, &live_gate)));
        let _ = service.hook_runtime.plugin.fire(
            ::hooks::PRE_LLM_CALL,
            &::hooks::HookPayload {
                session_id: session_id.into(),
                ..Default::default()
            },
        );
        assert_eq!(live_ui_rx.try_recv().unwrap().name, ::hooks::PRE_LLM_CALL);
    }

    #[tokio::test]
    async fn stale_chat_cleanup_preserves_replacement_pause_gate_and_ui() {
        let dir = TempDir::new().unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let session_id = "cleanup-generation-race";
        let old_session = service.get_session(session_id).await.unwrap();
        let old_gate = HitlGate::new(session_id);
        let old_gate_rx = old_gate
            .begin_wait(agent::Interrupt {
                id: "old-gate".into(),
                ..Default::default()
            })
            .await;
        service.hitl_registry.insert(Arc::clone(&old_gate)).await;
        let (old_ui_tx, _old_ui_rx) = tokio::sync::mpsc::unbounded_channel();
        let old_ui_generation = service
            .hook_runtime
            .ui_slot
            .install_tx(session_id, old_ui_tx);
        let old_registration = PauseRegistration {
            control: PauseControl::new(),
            session: Arc::downgrade(&old_session),
            hitl_gate: Arc::clone(&old_gate),
            ui_generation: old_ui_generation,
            operation: service.generation_operation(session_id),
        };
        service
            .pause_controls
            .write()
            .expect("pause registry lock poisoned")
            .insert(session_id.into(), old_registration.clone());

        service.sessions.write().await.remove(session_id);
        let replacement = service.get_session(session_id).await.unwrap();
        let replacement_gate = HitlGate::new(session_id);
        let mut replacement_gate_rx = replacement_gate
            .begin_wait(agent::Interrupt {
                id: "replacement-gate".into(),
                ..Default::default()
            })
            .await;
        service
            .hitl_registry
            .insert(Arc::clone(&replacement_gate))
            .await;
        let (replacement_ui_tx, mut replacement_ui_rx) = tokio::sync::mpsc::unbounded_channel();
        let replacement_ui_generation = service
            .hook_runtime
            .ui_slot
            .install_tx(session_id, replacement_ui_tx);
        let replacement_registration = PauseRegistration {
            control: PauseControl::new(),
            session: Arc::downgrade(&replacement),
            hitl_gate: Arc::clone(&replacement_gate),
            ui_generation: replacement_ui_generation,
            operation: service.generation_operation(session_id),
        };
        service
            .pause_controls
            .write()
            .expect("pause registry lock poisoned")
            .insert(session_id.into(), replacement_registration.clone());

        service
            .cleanup_pause_generation(session_id, &old_registration)
            .await;

        assert!(old_registration.control.is_cancelled());
        assert_eq!(old_gate_rx.await.unwrap().status, "cancelled");
        assert!(replacement_gate_rx.try_recv().is_err());
        assert!(service
            .pause_controls
            .read()
            .expect("pause registry lock poisoned")
            .get(session_id)
            .is_some_and(|registration| Arc::ptr_eq(
                &registration.control,
                &replacement_registration.control
            )));
        assert!(service
            .hitl_registry
            .get(session_id)
            .await
            .is_some_and(|gate| Arc::ptr_eq(&gate, &replacement_gate)));

        let _ = service.hook_runtime.plugin.fire(
            ::hooks::PRE_LLM_CALL,
            &::hooks::HookPayload {
                session_id: session_id.into(),
                system_prompt_chars: Some(17),
                ..Default::default()
            },
        );
        assert_eq!(
            replacement_ui_rx.try_recv().unwrap().name,
            ::hooks::PRE_LLM_CALL
        );
    }

    #[tokio::test]
    async fn concurrent_chat_admission_and_stale_cleanup_preserve_one_generation() {
        let dir = TempDir::new().unwrap();
        let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
        let session_id = "concurrent-chat-generation";
        let session = service.get_session(session_id).await.unwrap();
        let gate_a = HitlGate::new(session_id);
        let gate_b = HitlGate::new(session_id);
        let (ui_tx_a, mut ui_rx_a) = tokio::sync::mpsc::unbounded_channel();
        let (ui_tx_b, mut ui_rx_b) = tokio::sync::mpsc::unbounded_channel();
        let barrier = Arc::new(tokio::sync::Barrier::new(3));

        let admission_a = {
            let service = Arc::clone(&service);
            let session = Arc::clone(&session);
            let gate = Arc::clone(&gate_a);
            let barrier = Arc::clone(&barrier);
            tokio::spawn(async move {
                barrier.wait().await;
                service
                    .admit_pause_generation(session_id, &session, gate, ui_tx_a)
                    .await
                    .unwrap()
            })
        };
        let admission_b = {
            let service = Arc::clone(&service);
            let session = Arc::clone(&session);
            let gate = Arc::clone(&gate_b);
            let barrier = Arc::clone(&barrier);
            tokio::spawn(async move {
                barrier.wait().await;
                service
                    .admit_pause_generation(session_id, &session, gate, ui_tx_b)
                    .await
                    .unwrap()
            })
        };
        barrier.wait().await;
        let registration_a = admission_a.await.unwrap();
        let registration_b = admission_b.await.unwrap();

        let current = service
            .pause_controls
            .read()
            .expect("pause registry lock poisoned")
            .get(session_id)
            .cloned()
            .expect("one generation remains registered");
        let (winner, loser, winner_gate, winner_ui_rx, loser_ui_rx) =
            if Arc::ptr_eq(&current.control, &registration_a.control) {
                (
                    registration_a,
                    registration_b,
                    gate_a,
                    &mut ui_rx_a,
                    &mut ui_rx_b,
                )
            } else {
                (
                    registration_b,
                    registration_a,
                    gate_b,
                    &mut ui_rx_b,
                    &mut ui_rx_a,
                )
            };
        assert!(service
            .hitl_registry
            .get(session_id)
            .await
            .is_some_and(|gate| Arc::ptr_eq(&gate, &winner_gate)));

        save_interrupt_file(
            &service.memory_dir,
            session_id,
            &[agent::Interrupt {
                id: "winner-interrupt".into(),
                ..Default::default()
            }],
        )
        .unwrap();
        service.cleanup_pause_generation(session_id, &loser).await;

        assert!(service
            .pause_controls
            .read()
            .expect("pause registry lock poisoned")
            .get(session_id)
            .is_some_and(|registration| Arc::ptr_eq(&registration.control, &winner.control)));
        assert!(service
            .hitl_registry
            .get(session_id)
            .await
            .is_some_and(|gate| Arc::ptr_eq(&gate, &winner_gate)));
        let _ = service.hook_runtime.plugin.fire(
            ::hooks::PRE_LLM_CALL,
            &::hooks::HookPayload {
                session_id: session_id.into(),
                ..Default::default()
            },
        );
        assert_eq!(winner_ui_rx.try_recv().unwrap().name, ::hooks::PRE_LLM_CALL);
        assert!(loser_ui_rx.try_recv().is_err());
        let interrupt_path =
            crate::grpc::interrupt_store::interrupt_file_path(&service.memory_dir, session_id);
        let interrupts: Vec<agent::Interrupt> =
            serde_json::from_slice(&std::fs::read(interrupt_path).unwrap()).unwrap();
        assert_eq!(interrupts[0].id, "winner-interrupt");
    }

    #[tokio::test]
    async fn same_session_admission_waits_for_cancel_abort_to_finish() {
        let dir = TempDir::new().unwrap();
        let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
        let session_id = "cancel-admission-generation";
        let session = service.get_session(session_id).await.unwrap();
        let other_session_id = "independent-generation";
        let other_session = service.get_session(other_session_id).await.unwrap();
        let (old_ui_tx, _old_ui_rx) = tokio::sync::mpsc::unbounded_channel();
        service
            .admit_pause_generation(session_id, &session, HitlGate::new(session_id), old_ui_tx)
            .await
            .unwrap();

        let cancel_entered = Arc::new(tokio::sync::Barrier::new(2));
        let allow_abort = Arc::new(tokio::sync::Barrier::new(2));
        let cancel = {
            let service = Arc::clone(&service);
            let cancel_entered = Arc::clone(&cancel_entered);
            let allow_abort = Arc::clone(&allow_abort);
            tokio::spawn(async move {
                service
                    .cancel_current_pause_generation_with(session_id, move |session| async move {
                        cancel_entered.wait().await;
                        allow_abort.wait().await;
                        if let Some(session) = session {
                            session
                                .abort_all_tasks(TurnAbortReason::Interrupted)
                                .await?;
                        }
                        Ok(())
                    })
                    .await
            })
        };
        cancel_entered.wait().await;

        let (new_ui_tx, _new_ui_rx) = tokio::sync::mpsc::unbounded_channel();
        let admission = {
            let service = Arc::clone(&service);
            let session = Arc::clone(&session);
            tokio::spawn(async move {
                service
                    .admit_pause_generation(
                        session_id,
                        &session,
                        HitlGate::new(session_id),
                        new_ui_tx,
                    )
                    .await
            })
        };
        tokio::task::yield_now().await;
        assert!(
            !admission.is_finished(),
            "new generation must wait until old cancel finishes aborting its Session"
        );
        let (other_ui_tx, _other_ui_rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            service.admit_pause_generation(
                other_session_id,
                &other_session,
                HitlGate::new(other_session_id),
                other_ui_tx,
            ),
        )
        .await
        .expect("a different session must not wait behind the blocked cancel")
        .unwrap();

        allow_abort.wait().await;
        cancel.await.unwrap().unwrap().unwrap();
        let new_registration = admission.await.unwrap().unwrap();
        assert!(!new_registration.control.is_cancelled());

        let (new_turn, _new_rx) = spawn_pending_turn(Arc::clone(&session)).await;
        tokio::task::yield_now().await;
        assert!(
            !new_turn.is_finished(),
            "the completed old cancel must not abort the newly admitted turn"
        );
        session
            .abort_all_tasks(TurnAbortReason::Replaced)
            .await
            .unwrap();
        new_turn.await.unwrap();
    }

    #[tokio::test]
    async fn cancelled_cancel_owner_still_finishes_gate_cleanup() {
        let dir = TempDir::new().unwrap();
        let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
        let session_id = "cancelled-cancel-owner";
        let session = service.get_session(session_id).await.unwrap();
        let gate = HitlGate::new(session_id);
        let (ui_tx, _ui_rx) = tokio::sync::mpsc::unbounded_channel();
        service
            .admit_pause_generation(session_id, &session, Arc::clone(&gate), ui_tx)
            .await
            .unwrap();
        let mut gate_wait = gate
            .begin_wait(agent::Interrupt {
                id: "cancel-owner-gate".into(),
                ..Default::default()
            })
            .await;
        let abort_entered = Arc::new(tokio::sync::Notify::new());
        let release_abort = Arc::new(tokio::sync::Notify::new());
        let owner = tokio::spawn({
            let service = Arc::clone(&service);
            let abort_entered = Arc::clone(&abort_entered);
            let release_abort = Arc::clone(&release_abort);
            async move {
                service
                    .cancel_current_pause_generation_with(session_id, move |_| async move {
                        abort_entered.notify_one();
                        release_abort.notified().await;
                        Ok(())
                    })
                    .await
            }
        });
        abort_entered.notified().await;
        owner.abort();
        assert!(matches!(owner.await, Err(error) if error.is_cancelled()));
        release_abort.notify_one();

        let resolution = tokio::time::timeout(std::time::Duration::from_secs(1), &mut gate_wait)
            .await
            .expect("detached cancel worker must resolve the exact gate")
            .expect("gate resolution");
        assert_eq!(resolution.status, "cancelled");
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if !service
                    .generation_operations
                    .lock()
                    .unwrap()
                    .contains_key(session_id)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("cancel worker must prune its generation operation");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cancelled_release_owner_still_finishes_session_cleanup() {
        let dir = TempDir::new().unwrap();
        let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
        let session_id = "cancelled-release-owner";
        let finalize_hits = Arc::new(AtomicUsize::new(0));
        let finalize_counter = Arc::clone(&finalize_hits);
        let (finalize_entered_tx, finalize_entered_rx) = std::sync::mpsc::channel();
        let (release_finalize_tx, release_finalize_rx) = std::sync::mpsc::channel();
        let release_finalize_rx = Arc::new(std::sync::Mutex::new(release_finalize_rx));
        service
            .hook_runtime
            .plugin
            .register(::hooks::SESSION_END, move |_| {
                finalize_counter.fetch_add(1, Ordering::SeqCst);
                let _ = finalize_entered_tx.send(());
                let _ = release_finalize_rx
                    .lock()
                    .expect("finalize release mutex poisoned")
                    .recv();
                ::hooks::HookOutcome::Continue
            });
        let session = service.get_session(session_id).await.unwrap();
        let gate = HitlGate::new(session_id);
        let (ui_tx, _ui_rx) = tokio::sync::mpsc::unbounded_channel();
        service
            .admit_pause_generation(session_id, &session, gate, ui_tx)
            .await
            .unwrap();

        let owner = tokio::spawn({
            let service = Arc::clone(&service);
            async move { service.release_session_runtime(session_id).await }
        });
        tokio::task::spawn_blocking(move || finalize_entered_rx.recv())
            .await
            .unwrap()
            .unwrap();
        owner.abort();
        assert!(matches!(owner.await, Err(error) if error.is_cancelled()));
        release_finalize_tx.send(()).unwrap();
        session.shutdown_runtime().await;

        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if !service
                    .generation_operations
                    .lock()
                    .unwrap()
                    .contains_key(session_id)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("release worker must prune its generation operation");
        assert_eq!(finalize_hits.load(Ordering::SeqCst), 1);
        assert!(service.sessions.read().await.get(session_id).is_none());
        assert!(service
            .pause_controls
            .read()
            .expect("pause registry lock poisoned")
            .get(session_id)
            .is_none());
        assert!(service.hitl_registry.get(session_id).await.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_new_chat_release_finalizes_session_once() {
        let dir = TempDir::new().unwrap();
        let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
        let session_id = "concurrent-new-chat-finalize-once";
        let finalize_hits = Arc::new(AtomicUsize::new(0));
        let finalize_counter = Arc::clone(&finalize_hits);
        let (finalize_entered_tx, finalize_entered_rx) = std::sync::mpsc::channel();
        let (release_finalize_tx, release_finalize_rx) = std::sync::mpsc::channel();
        let release_finalize_rx = Arc::new(std::sync::Mutex::new(release_finalize_rx));
        service
            .hook_runtime
            .plugin
            .register(::hooks::SESSION_END, move |_| {
                if finalize_counter.fetch_add(1, Ordering::SeqCst) == 0 {
                    let _ = finalize_entered_tx.send(());
                    let _ = release_finalize_rx
                        .lock()
                        .expect("finalize release mutex poisoned")
                        .recv();
                }
                ::hooks::HookOutcome::Continue
            });
        service.get_session(session_id).await.unwrap();

        let first = tokio::spawn({
            let service = Arc::clone(&service);
            async move { service.release_session_for_new_chat(session_id).await }
        });
        tokio::task::spawn_blocking(move || finalize_entered_rx.recv())
            .await
            .unwrap()
            .unwrap();
        let second = tokio::spawn({
            let service = Arc::clone(&service);
            async move { service.release_session_for_new_chat(session_id).await }
        });
        tokio::task::yield_now().await;
        assert!(
            !second.is_finished(),
            "the concurrent release must share the in-flight completion"
        );
        assert_eq!(finalize_hits.load(Ordering::SeqCst), 1);

        release_finalize_tx.send(()).unwrap();
        first.await.unwrap();
        second.await.unwrap();
        assert_eq!(finalize_hits.load(Ordering::SeqCst), 1);
        assert!(service.release_ownerships.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn concurrent_release_callers_share_ownership_then_reclaim_entry() {
        let dir = TempDir::new().unwrap();
        let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
        let session_id = "concurrent-release-shared-ownership";
        let finalize_hits = Arc::new(AtomicUsize::new(0));
        let finalize_counter = Arc::clone(&finalize_hits);
        service
            .hook_runtime
            .plugin
            .register(::hooks::SESSION_END, move |_| {
                finalize_counter.fetch_add(1, Ordering::SeqCst);
                ::hooks::HookOutcome::Continue
            });
        service.get_session(session_id).await.unwrap();

        let acquired = Arc::new(tokio::sync::Barrier::new(3));
        let release = Arc::new(tokio::sync::Barrier::new(3));
        let ownership_ptrs = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut callers = Vec::new();
        for _ in 0..2 {
            callers.push(tokio::spawn({
                let service = Arc::clone(&service);
                let acquired = Arc::clone(&acquired);
                let release = Arc::clone(&release);
                let ownership_ptrs = Arc::clone(&ownership_ptrs);
                async move {
                    let ownership = service.release_generation_ownership(session_id);
                    ownership_ptrs.lock().unwrap().push(ownership.entry_ptr());
                    acquired.wait().await;
                    release.wait().await;
                    service
                        .release_session_runtime_with_ownership(session_id, ownership)
                        .await
                }
            }));
        }

        acquired.wait().await;
        let ownership_ptrs = ownership_ptrs.lock().unwrap().clone();
        assert_eq!(ownership_ptrs.len(), 2);
        assert_eq!(ownership_ptrs[0], ownership_ptrs[1]);
        assert_eq!(service.release_ownerships.lock().unwrap().len(), 1);
        release.wait().await;
        for caller in callers {
            let result = caller.await.unwrap();
            assert!(!result.should_finalize_without_runtime);
        }
        assert_eq!(finalize_hits.load(Ordering::SeqCst), 1);
        assert!(service.release_ownerships.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancelled_cold_release_reply_does_not_consume_fallback_claim() {
        let dir = TempDir::new().unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let session_id = "cancelled-cold-release-fallback-claim";
        let cancelled_ownership = service.release_generation_ownership(session_id);
        let surviving_ownership = service.release_generation_ownership(session_id);
        assert_eq!(
            cancelled_ownership.entry_ptr(),
            surviving_ownership.entry_ptr()
        );
        let finalize_hits = Arc::new(AtomicUsize::new(0));
        let finalize_counter = Arc::clone(&finalize_hits);
        service
            .hook_runtime
            .plugin
            .register(::hooks::SESSION_END, move |_| {
                finalize_counter.fetch_add(1, Ordering::SeqCst);
                ::hooks::HookOutcome::Continue
            });

        let (cancelled_tx, cancelled_rx) = tokio::sync::oneshot::channel();
        drop(cancelled_rx);
        service
            .run_release_session_runtime_worker(
                session_id.to_string(),
                cancelled_ownership,
                cancelled_tx,
            )
            .await;

        let (surviving_tx, surviving_rx) = tokio::sync::oneshot::channel();
        service
            .run_release_session_runtime_worker(
                session_id.to_string(),
                surviving_ownership,
                surviving_tx,
            )
            .await;
        let result = surviving_rx
            .await
            .expect("surviving caller must receive the shared completion")
            .claim_fallback();
        if result.should_finalize_without_runtime {
            let _ = service.hook_runtime.fire_plugin(
                ::hooks::SESSION_END,
                &::hooks::HookPayload {
                    session_id: session_id.into(),
                    ..Default::default()
                },
            );
        }

        assert!(result.should_finalize_without_runtime);
        assert_eq!(finalize_hits.load(Ordering::SeqCst), 1);
        assert!(service.release_ownerships.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn release_only_completion_does_not_consume_new_chat_fallback() {
        let dir = TempDir::new().unwrap();
        let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
        let session_id = "release-only-before-new-chat-fallback";
        let finalize_hits = Arc::new(AtomicUsize::new(0));
        let finalize_counter = Arc::clone(&finalize_hits);
        service
            .hook_runtime
            .plugin
            .register(::hooks::SESSION_END, move |_| {
                finalize_counter.fetch_add(1, Ordering::SeqCst);
                ::hooks::HookOutcome::Continue
            });
        let acquired = Arc::new(tokio::sync::Barrier::new(3));
        let (ownership_tx, mut ownership_rx) = tokio::sync::mpsc::unbounded_channel();
        for is_new_chat in [false, true] {
            tokio::spawn({
                let service = Arc::clone(&service);
                let acquired = Arc::clone(&acquired);
                let ownership_tx = ownership_tx.clone();
                async move {
                    let ownership = service.release_generation_ownership(session_id);
                    ownership_tx.send((is_new_chat, ownership)).unwrap();
                    acquired.wait().await;
                }
            });
        }
        drop(ownership_tx);
        acquired.wait().await;
        let mut release_only = None;
        let mut new_chat = None;
        while let Some((is_new_chat, ownership)) = ownership_rx.recv().await {
            if is_new_chat {
                new_chat = Some(ownership);
            } else {
                release_only = Some(ownership);
            }
        }
        let release_only = release_only.expect("release-only ownership");
        let new_chat = new_chat.expect("new-chat ownership");
        assert_eq!(release_only.entry_ptr(), new_chat.entry_ptr());

        let release_result = service
            .release_session_runtime_with_ownership(session_id, release_only)
            .await;
        assert!(release_result.should_finalize_without_runtime);

        let (new_chat_tx, new_chat_rx) = tokio::sync::oneshot::channel();
        service
            .run_release_session_runtime_worker(session_id.to_string(), new_chat, new_chat_tx)
            .await;
        let new_chat_result = new_chat_rx
            .await
            .expect("new-chat caller must receive the shared completion")
            .claim_fallback();
        assert!(
            new_chat_result.should_finalize_without_runtime,
            "release-only completion must not consume fallback ownership"
        );
        let _ = service.hook_runtime.fire_plugin(
            ::hooks::SESSION_END,
            &::hooks::HookPayload {
                session_id: session_id.into(),
                ..Default::default()
            },
        );
        assert_eq!(finalize_hits.load(Ordering::SeqCst), 1);
        assert!(service.release_ownerships.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn unique_release_ownership_entries_are_reclaimed() {
        let dir = TempDir::new().unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let finalize_hits = Arc::new(AtomicUsize::new(0));
        let finalize_counter = Arc::clone(&finalize_hits);
        service
            .hook_runtime
            .plugin
            .register(::hooks::SESSION_END, move |_| {
                finalize_counter.fetch_add(1, Ordering::SeqCst);
                ::hooks::HookOutcome::Continue
            });

        for index in 0..32 {
            let session_id = format!("unique-release-ownership-{index}");
            service.get_session(&session_id).await.unwrap();
            let result = service.release_session_runtime(&session_id).await;
            assert!(!result.should_finalize_without_runtime);
        }

        assert_eq!(finalize_hits.load(Ordering::SeqCst), 32);
        assert!(service.release_ownerships.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn recreated_session_resets_finalize_ownership_generation() {
        let dir = TempDir::new().unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let session_id = "recreated-session-finalize-ownership";
        let finalize_hits = Arc::new(AtomicUsize::new(0));
        let finalize_counter = Arc::clone(&finalize_hits);
        service
            .hook_runtime
            .plugin
            .register(::hooks::SESSION_END, move |_| {
                finalize_counter.fetch_add(1, Ordering::SeqCst);
                ::hooks::HookOutcome::Continue
            });

        let first = service.get_session(session_id).await.unwrap();
        let first_release = service.release_session_runtime(session_id).await;
        assert!(!first_release.should_finalize_without_runtime);
        assert_eq!(finalize_hits.load(Ordering::SeqCst), 1);
        assert!(service.release_ownerships.lock().unwrap().is_empty());

        let second = service.get_session(session_id).await.unwrap();
        assert!(!Arc::ptr_eq(&first, &second));
        assert!(service.release_ownerships.lock().unwrap().is_empty());

        let second_release = service.release_session_runtime(session_id).await;
        assert!(!second_release.should_finalize_without_runtime);
        assert_eq!(finalize_hits.load(Ordering::SeqCst), 2);
        assert!(service.release_ownerships.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn stale_launcher_cannot_replace_a_newly_installed_generation() {
        let dir = TempDir::new().unwrap();
        let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
        let session_id = "stale-launcher-generation";
        let session = service.get_session(session_id).await.unwrap();
        let (old_ui_tx, _old_ui_rx) = tokio::sync::mpsc::unbounded_channel();
        let old_registration = service
            .admit_pause_generation(session_id, &session, HitlGate::new(session_id), old_ui_tx)
            .await
            .unwrap();
        let old_ready = Arc::new(tokio::sync::Barrier::new(2));
        let release_old = Arc::new(tokio::sync::Barrier::new(2));
        let old_touched_session = Arc::new(AtomicBool::new(false));
        let old_launcher = {
            let service = Arc::clone(&service);
            let old_ready = Arc::clone(&old_ready);
            let release_old = Arc::clone(&release_old);
            let old_touched_session = Arc::clone(&old_touched_session);
            tokio::spawn(async move {
                old_ready.wait().await;
                release_old.wait().await;
                service
                    .launch_current_pause_generation_with(
                        session_id,
                        &old_registration,
                        move || async move {
                            old_touched_session.store(true, Ordering::SeqCst);
                        },
                    )
                    .await
            })
        };
        old_ready.wait().await;

        service
            .cancel_current_pause_generation(session_id)
            .await
            .unwrap()
            .unwrap();
        let (new_ui_tx, _new_ui_rx) = tokio::sync::mpsc::unbounded_channel();
        let new_registration = service
            .admit_pause_generation(session_id, &session, HitlGate::new(session_id), new_ui_tx)
            .await
            .unwrap();
        let (new_turn, _new_rx) = service
            .launch_current_pause_generation_with(session_id, &new_registration, {
                let session = Arc::clone(&session);
                move || async move { spawn_pending_turn(session).await }
            })
            .await
            .expect("the current generation must install its task");

        release_old.wait().await;
        assert!(old_launcher.await.unwrap().is_none());
        assert!(!old_touched_session.load(Ordering::SeqCst));
        assert!(
            !new_turn.is_finished(),
            "the stale launcher must not replace or abort the new task"
        );
        session
            .abort_all_tasks(TurnAbortReason::Replaced)
            .await
            .unwrap();
        new_turn.await.unwrap();
    }

    #[tokio::test]
    async fn new_chat_preserves_hooks_while_release_session_skips_them() {
        let dir = TempDir::new().unwrap();
        let hook_dir = dir.path().join("hooks").join("audit");
        std::fs::create_dir_all(&hook_dir).unwrap();
        std::fs::write(
            hook_dir.join("HOOK.yaml"),
            "name: audit\nevents:\n  - CommandNewChat\n",
        )
        .unwrap();

        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let gateway_hits = Arc::new(AtomicUsize::new(0));
        let gateway_counter = Arc::clone(&gateway_hits);
        service
            .hook_runtime
            .gateway
            .register_handler("audit", move |event, _| {
                if event == ::hooks::COMMAND_NEW_CHAT {
                    gateway_counter.fetch_add(1, Ordering::SeqCst);
                }
            });

        let reset_hits = Arc::new(AtomicUsize::new(0));
        let reset_counter = Arc::clone(&reset_hits);
        service
            .hook_runtime
            .plugin
            .register(::hooks::SESSION_RESET, move |_| {
                reset_counter.fetch_add(1, Ordering::SeqCst);
                ::hooks::HookOutcome::Continue
            });
        let finalize_hits = Arc::new(AtomicUsize::new(0));
        let finalize_counter = Arc::clone(&finalize_hits);
        service
            .hook_runtime
            .plugin
            .register(::hooks::SESSION_END, move |_| {
                finalize_counter.fetch_add(1, Ordering::SeqCst);
                ::hooks::HookOutcome::Continue
            });

        service
            .chat_control(Request::new(ChatControlRequest {
                session_id: "release-only".into(),
                action: ChatControlAction::ReleaseSession as i32,
            }))
            .await
            .expect("release session");
        assert_eq!(gateway_hits.load(Ordering::SeqCst), 0);
        assert_eq!(reset_hits.load(Ordering::SeqCst), 0);
        assert_eq!(finalize_hits.load(Ordering::SeqCst), 0);

        let session = service.get_session("new-chat").await.unwrap();
        session.set_hook_runtime(Arc::clone(&service.hook_runtime));
        service
            .chat_control(Request::new(ChatControlRequest {
                session_id: "new-chat".into(),
                action: ChatControlAction::ChatControlNewChat as i32,
            }))
            .await
            .expect("new chat");
        assert_eq!(gateway_hits.load(Ordering::SeqCst), 1);
        assert_eq!(reset_hits.load(Ordering::SeqCst), 1);
        assert_eq!(finalize_hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn parse_auxiliary_targets_groups_by_task_and_sorts_by_order() {
        let map = crate::grpc::thread_settings::parse_auxiliary_targets(vec![
            proto::AuxiliaryModelTarget {
                task: "compaction".into(),
                provider_id: "p-fb".into(),
                backend_id: "openai".into(),
                model: "gpt-fb".into(),
                api_key: "k-fb".into(),
                base_url: "https://fb".into(),
                order: 1,
            },
            proto::AuxiliaryModelTarget {
                task: "compaction".into(),
                provider_id: "p-pref".into(),
                backend_id: "deepseek".into(),
                model: "gpt-pref".into(),
                api_key: "k-pref".into(),
                base_url: "https://pref".into(),
                order: 0,
            },
            proto::AuxiliaryModelTarget {
                task: "unknown_task".into(),
                provider_id: "x".into(),
                backend_id: "x".into(),
                model: "x".into(),
                api_key: "x".into(),
                base_url: "x".into(),
                order: 0,
            },
        ]);

        let chain = map
            .get(&types::AuxiliaryTask::Compaction)
            .expect("compaction");
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].provider_id, "p-pref");
        assert_eq!(chain[1].provider_id, "p-fb");
        assert!(!map.contains_key(&types::AuxiliaryTask::Dreaming));
    }

    #[tokio::test]
    async fn concurrent_thread_creation_installs_exactly_one_listener() {
        let dir = TempDir::new().unwrap();
        memory::ensure_workspace(dir.path()).unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let first_service = service.clone();
        let second_service = service.clone();
        let (first, second) = tokio::join!(
            first_service.get_or_create_thread("race-thread"),
            second_service.get_or_create_thread("race-thread")
        );
        let first = first.expect("first create");
        let second = second.expect("second create");
        assert!(Arc::ptr_eq(&first, &second));
        assert!(service.threads.contains("race-thread").await);
        first
            .runtime
            .submit(agent_protocol::Op::Shutdown)
            .await
            .unwrap();
        first.runtime.wait_terminated().await;
    }

    #[tokio::test]
    async fn idle_thread_unloads_only_after_thirty_minutes() {
        let dir = TempDir::new().unwrap();
        memory::ensure_workspace(dir.path()).unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        service
            .get_session("idle-thread")
            .await
            .expect("prewarm idle session");
        tokio::time::pause();
        let managed = service
            .get_or_create_thread("idle-thread")
            .await
            .expect("create idle thread");
        let old_runtime = Arc::clone(&managed.runtime);
        drop(managed);
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;

        tokio::time::advance(std::time::Duration::from_secs(29 * 60)).await;
        tokio::task::yield_now().await;
        assert!(service.threads.contains("idle-thread").await);

        tokio::time::advance(std::time::Duration::from_secs(60)).await;
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert!(!service.threads.contains("idle-thread").await);
        assert!(service.thread_states.get("idle-thread").await.is_none());
        tokio::time::resume();
        let replacement = service
            .get_or_create_thread("idle-thread")
            .await
            .expect("idle thread should be reloadable");
        assert!(!Arc::ptr_eq(&old_runtime, &replacement.runtime));
        assert!(!replacement.listener_is_finished().await);
        replacement
            .runtime
            .submit(agent_protocol::Op::Shutdown)
            .await
            .unwrap();
        replacement.runtime.wait_terminated().await;
    }

    #[tokio::test]
    async fn acquired_managed_handle_prevents_idle_unload_until_released() {
        let dir = TempDir::new().unwrap();
        memory::ensure_workspace(dir.path()).unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let managed = service
            .get_or_create_thread("leased-idle-thread")
            .await
            .expect("create idle thread");
        tokio::time::pause();
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;

        tokio::time::advance(std::time::Duration::from_secs(30 * 60)).await;
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        let current = service
            .threads
            .get("leased-idle-thread")
            .await
            .expect("leased thread remains current");
        assert!(Arc::ptr_eq(&managed, &current));
        assert!(!managed.listener_is_finished().await);

        drop(current);
        drop(managed);
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        tokio::time::advance(std::time::Duration::from_secs(30 * 60)).await;
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert!(!service.threads.contains("leased-idle-thread").await);
    }

    #[tokio::test]
    async fn failed_terminal_unsubscribed_thread_unloads_after_thirty_minutes() {
        let dir = TempDir::new().unwrap();
        memory::ensure_workspace(dir.path()).unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let managed = service
            .get_or_create_thread("failed-thread")
            .await
            .expect("create thread");
        tokio::time::pause();
        let (_receiver, _cancel, generation) = service
            .connections
            .register("connection-failed".into())
            .await;
        let subscription = generation.key().clone();
        let (resume_reply, resume_rx) = tokio::sync::oneshot::channel();
        managed
            .commands
            .send(ListenerCommand::Resume {
                subscription: subscription.clone(),
                include_turns: true,
                reply: resume_reply,
            })
            .unwrap();
        resume_rx.await.unwrap();
        managed
            .commands
            .send(ListenerCommand::CoreEvent(agent_protocol::Event {
                id: "turn-failed".into(),
                msg: agent_protocol::EventMsg::TurnStarted(agent_protocol::TurnStartedEvent {
                    turn_id: "turn-failed".into(),
                }),
            }))
            .unwrap();
        managed
            .commands
            .send(ListenerCommand::CoreEvent(agent_protocol::Event {
                id: "turn-failed".into(),
                msg: agent_protocol::EventMsg::TurnComplete(agent_protocol::TurnCompleteEvent {
                    turn_id: "turn-failed".into(),
                    last_agent_message: None,
                    error: Some(agent_protocol::ErrorEvent {
                        message: "boom".into(),
                        error_type: "provider".into(),
                    }),
                }),
            }))
            .unwrap();
        let (unsubscribe_reply, unsubscribe_rx) = tokio::sync::oneshot::channel();
        managed
            .commands
            .send(ListenerCommand::Unsubscribe {
                subscription,
                reply: Some(unsubscribe_reply),
            })
            .unwrap();
        unsubscribe_rx.await.unwrap();
        assert_eq!(managed.activity_rx.borrow().status, "errored");
        assert!(!managed.activity_rx.borrow().has_subscribers);
        drop(managed);
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;

        tokio::time::advance(std::time::Duration::from_secs(30 * 60)).await;
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }

        assert!(!service.threads.contains("failed-thread").await);
        assert!(service.thread_states.get("failed-thread").await.is_none());
    }

    #[tokio::test]
    async fn never_returning_background_phase_releases_sink_then_allows_idle_unload() {
        let dir = TempDir::new().unwrap();
        memory::ensure_workspace(dir.path()).unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let managed = service
            .get_or_create_thread("stuck-background-thread")
            .await
            .expect("create thread");
        tokio::time::pause();
        let (_receiver, _cancel, generation) = service
            .connections
            .register("connection-stuck".into())
            .await;
        let subscription = generation.key().clone();
        let (resume_reply, resume_rx) = tokio::sync::oneshot::channel();
        managed
            .commands
            .send(ListenerCommand::Resume {
                subscription: subscription.clone(),
                include_turns: true,
                reply: resume_reply,
            })
            .unwrap();
        resume_rx.await.unwrap();
        managed
            .commands
            .send(ListenerCommand::ObservedCoreEvent(agent_protocol::Event {
                id: "turn-stuck".into(),
                msg: agent_protocol::EventMsg::TurnComplete(agent_protocol::TurnCompleteEvent {
                    turn_id: "turn-stuck".into(),
                    last_agent_message: Some("done".into()),
                    error: None,
                }),
            }))
            .unwrap();
        let (unsubscribe_reply, unsubscribe_rx) = tokio::sync::oneshot::channel();
        managed
            .commands
            .send(ListenerCommand::Unsubscribe {
                subscription,
                reply: Some(unsubscribe_reply),
            })
            .unwrap();
        unsubscribe_rx.await.unwrap();
        assert!(managed.activity_rx.borrow().has_subscribers);
        drop(managed);
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;

        // Simulate a provider future that never resolves. The background lease must retain
        // the sink through the work timeout plus marker window, then expire independently
        // before the normal thirty-minute idle-unload window.
        tokio::time::advance(crate::BACKGROUND_EXTENSION_SINK_TIMEOUT).await;
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        let current = service
            .threads
            .get("stuck-background-thread")
            .await
            .expect("thread remains during idle window");
        assert!(!current.activity_rx.borrow().has_subscribers);
        drop(current);
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;

        tokio::time::advance(std::time::Duration::from_secs(30 * 60)).await;
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert!(!service.threads.contains("stuck-background-thread").await);
        assert!(service
            .thread_states
            .get("stuck-background-thread")
            .await
            .is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn bounded_background_phase_runs_finalizer_after_provider_timeout() {
        let finalized = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let finalized_for_task = Arc::clone(&finalized);
        let phase = tokio::spawn(run_bounded_post_turn_phase(
            std::future::pending::<Result<(), &'static str>>(),
            async move {
                finalized_for_task.store(true, std::sync::atomic::Ordering::SeqCst);
            },
        ));
        tokio::task::yield_now().await;
        tokio::time::advance(std::time::Duration::from_secs(5 * 60)).await;
        let outcome = phase.await.expect("phase supervisor");
        assert_eq!(outcome, BackgroundPhaseOutcome::TimedOut);
        assert!(finalized.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn bounded_background_phase_finalizes_error_panic_and_cancellation() {
        let finalized = Arc::new(AtomicUsize::new(0));

        let error_finalized = Arc::clone(&finalized);
        let error = run_bounded_post_turn_phase(async { Err::<(), _>("provider") }, async move {
            error_finalized.fetch_add(1, Ordering::SeqCst);
        })
        .await;
        assert_eq!(error, BackgroundPhaseOutcome::Failed);

        let panic_finalized = Arc::clone(&finalized);
        let panicked = run_bounded_post_turn_phase(
            async {
                panic!("provider panic");
                #[allow(unreachable_code)]
                Ok::<(), &'static str>(())
            },
            async move {
                panic_finalized.fetch_add(1, Ordering::SeqCst);
            },
        )
        .await;
        assert_eq!(panicked, BackgroundPhaseOutcome::Panicked);

        let cancelled_task = tokio::spawn(std::future::pending::<Result<(), &'static str>>());
        cancelled_task.abort();
        let cancel_finalized = Arc::clone(&finalized);
        let cancelled = finish_bounded_post_turn_task(cancelled_task, async move {
            cancel_finalized.fetch_add(1, Ordering::SeqCst);
        })
        .await;
        assert_eq!(cancelled, BackgroundPhaseOutcome::Cancelled);
        assert_eq!(finalized.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn released_generation_background_finalizer_cannot_resurrect_thread_or_session() {
        let dir = TempDir::new().unwrap();
        memory::ensure_workspace(dir.path()).unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let old = service
            .get_or_create_thread("released-background")
            .await
            .expect("old generation");
        let lifecycle = old.lifecycle_token();
        let finalized = Arc::new(AtomicBool::new(false));
        let finalized_by_task = Arc::clone(&finalized);
        let commands = old.commands.clone();
        let supervisor = tokio::spawn(async move {
            run_bounded_post_turn_phase_until_cancelled(
                std::future::pending::<Result<(), Status>>(),
                async move {
                    let _ = commands.send(crate::ListenerCommand::ExpireBackgroundSink {
                        turn_id: "turn-old".into(),
                    });
                    finalized_by_task.store(true, Ordering::SeqCst);
                },
                lifecycle,
            )
            .await;
        });
        old.set_side_effect_supervisor(supervisor).await;

        let release_service = service.clone();
        let release = tokio::spawn(async move {
            release_service
                .release_session_runtime("released-background")
                .await
        });
        for _ in 0..16 {
            if release.is_finished() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(
            release.is_finished(),
            "explicit release must cancel work and expire the old sink without waiting for the 30s marker timeout"
        );
        release.await.expect("release task");
        assert!(!service.threads.contains("released-background").await);
        assert!(!service
            .sessions
            .read()
            .await
            .contains_key("released-background"));
        assert!(finalized.load(Ordering::SeqCst));
        assert!(old.side_effect_supervisor_is_finished().await);
        assert!(old.listener_is_finished().await);
    }

    #[tokio::test]
    async fn old_background_finalizer_cannot_write_into_new_same_id_generation() {
        let dir = TempDir::new().unwrap();
        memory::ensure_workspace(dir.path()).unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let old = service
            .get_or_create_thread("reused-background")
            .await
            .expect("old generation");
        service.release_session_runtime("reused-background").await;
        let current = service
            .get_or_create_thread("reused-background")
            .await
            .expect("new generation");
        assert!(!Arc::ptr_eq(&old, &current));

        let _ = service.emit_background_complete(&old, "turn-old").await;
        let state = service
            .thread_states
            .get("reused-background")
            .await
            .expect("new state");
        let state = state.lock().await;
        assert!(
            state
                .history
                .completed_turns()
                .iter()
                .all(|turn| turn.id != "turn-old"),
            "an old-generation marker must never mutate the replacement generation"
        );
    }

    #[tokio::test]
    async fn cancelled_background_phase_materializes_completion_before_release() {
        let dir = TempDir::new().unwrap();
        memory::ensure_workspace(dir.path()).unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let managed = service
            .get_or_create_thread("cancelled-background")
            .await
            .expect("thread generation");
        let lifecycle = managed.lifecycle_token();
        lifecycle.cancel();

        service
            .finalize_background_phase(&managed, "turn-cancelled")
            .await;

        let state = service
            .thread_states
            .get("cancelled-background")
            .await
            .expect("thread state");
        let state = state.lock().await;
        let marker = state
            .history
            .completed_turns()
            .iter()
            .find(|turn| turn.id == "turn-cancelled")
            .and_then(|turn| {
                turn.items.iter().find(|item| {
                    matches!(
                        &item.item,
                        agent_protocol::TurnItem::Extension(extension)
                            if extension.namespace == "astro.background_complete"
                    )
                })
            });
        assert!(
            marker.is_some(),
            "release cancellation must converge desktop background-pending state through the unified extension event"
        );
    }

    #[tokio::test]
    async fn cancelled_blocked_extension_submissions_remove_all_registered_waiters() {
        let connections = ConnectionRegistry::default();
        let (commands, command_rx) = tokio::sync::mpsc::unbounded_channel();
        let (activity_tx, _activity_rx) = tokio::sync::watch::channel(ThreadActivity {
            status: "idle".into(),
            has_subscribers: false,
        });
        let state = Arc::new(Mutex::new(ThreadState {
            status: "idle".into(),
            history: ThreadHistoryBuilder::default(),
            subscribers: Default::default(),
            background_extension_sinks: Default::default(),
            listener_command_tx: commands.clone(),
            activity_tx,
        }));
        tokio::spawn(crate::run_listener_commands(
            "waiter-cleanup".into(),
            state,
            command_rx,
            connections,
        ));

        let (blocked_submission_tx, _blocked_submission_rx) = tokio::sync::mpsc::channel(1);
        blocked_submission_tx
            .send(())
            .await
            .expect("fill bounded submission queue");
        for index in 0..16 {
            let (registration, _materialized) = register_extension_waiter(
                &commands,
                format!("item-{index}"),
                format!("payload-{index}"),
            )
            .expect("register waiter");
            let blocked_submission_tx = blocked_submission_tx.clone();
            let blocked = tokio::spawn(async move {
                let _registration = registration;
                blocked_submission_tx
                    .send(())
                    .await
                    .expect("queue remains open");
            });
            tokio::task::yield_now().await;
            blocked.abort();
            let _ = blocked.await;
        }

        let (count_reply, count_rx) = tokio::sync::oneshot::channel();
        commands
            .send(ListenerCommand::ExtensionWaiterCount { reply: count_reply })
            .expect("inspect waiter count");
        assert_eq!(count_rx.await.expect("waiter count"), 0);
    }

    #[tokio::test]
    async fn completed_snapshot_can_resubscribe_then_unload_after_explicit_unsubscribe() {
        let dir = TempDir::new().unwrap();
        memory::ensure_workspace(dir.path()).unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let managed = service
            .get_or_create_thread("completed-thread")
            .await
            .expect("create thread");
        tokio::time::pause();
        let (_receiver, _cancel, generation) = service
            .connections
            .register("connection-completed".into())
            .await;
        let subscription = generation.key().clone();
        let (resume_reply, resume_rx) = tokio::sync::oneshot::channel();
        managed
            .commands
            .send(ListenerCommand::Resume {
                subscription: subscription.clone(),
                include_turns: true,
                reply: resume_reply,
            })
            .unwrap();
        resume_rx.await.unwrap();
        for msg in [
            agent_protocol::EventMsg::TurnStarted(agent_protocol::TurnStartedEvent {
                turn_id: "turn-completed".into(),
            }),
            agent_protocol::EventMsg::TurnComplete(agent_protocol::TurnCompleteEvent {
                turn_id: "turn-completed".into(),
                last_agent_message: Some("done".into()),
                error: None,
            }),
        ] {
            managed
                .commands
                .send(ListenerCommand::CoreEvent(agent_protocol::Event {
                    id: "turn-completed".into(),
                    msg,
                }))
                .unwrap();
        }
        let (unsubscribe_reply, unsubscribe_rx) = tokio::sync::oneshot::channel();
        managed
            .commands
            .send(ListenerCommand::Unsubscribe {
                subscription: subscription.clone(),
                reply: Some(unsubscribe_reply),
            })
            .unwrap();
        unsubscribe_rx.await.unwrap();

        let (resume_reply, resume_rx) = tokio::sync::oneshot::channel();
        managed
            .commands
            .send(ListenerCommand::Resume {
                subscription: subscription.clone(),
                include_turns: true,
                reply: resume_reply,
            })
            .unwrap();
        let snapshot = resume_rx.await.unwrap();
        assert_eq!(snapshot.turns.len(), 1);
        assert_eq!(snapshot.turns[0].status, "completed");
        let (unsubscribe_reply, unsubscribe_rx) = tokio::sync::oneshot::channel();
        managed
            .commands
            .send(ListenerCommand::Unsubscribe {
                subscription,
                reply: Some(unsubscribe_reply),
            })
            .unwrap();
        unsubscribe_rx.await.unwrap();
        drop(managed);
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;

        tokio::time::advance(std::time::Duration::from_secs(30 * 60)).await;
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert!(!service.threads.contains("completed-thread").await);
    }

    #[tokio::test]
    async fn reconcile_extensions_rejects_an_empty_session_id_without_side_effects() {
        let dir = TempDir::new().unwrap();
        memory::ensure_workspace(dir.path()).unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());

        let error = AstroService::reconcile_extensions(
            &service,
            Request::new(proto::ReconcileExtensionsRequest {
                session_id: "  ".into(),
            }),
        )
        .await
        .unwrap_err();

        assert_eq!(error.code(), tonic::Code::InvalidArgument);
        assert!(!service.threads.contains("").await);
    }
}
