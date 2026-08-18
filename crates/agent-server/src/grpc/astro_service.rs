//! Astro gRPC [`AstroService`] 实现：聊天流、会话、记忆、MCP、技能与文件列表。

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, RwLock as StdRwLock, Weak};

use agent::builder::AgentBuilder;
use agent::runtime::Session;
use agent::streaming::{
    stream_multi_turn_with_hitl, MultiTurnStreamItem, StreamedAssistantContent,
};
use agent::{HitlGate, HitlRegistry, TurnAbortReason, TurnInput};
use futures::{FutureExt, StreamExt};
use home::AgentRuntimeConfig;
use memory::MemoryManager;
use proto::astro_service_server::AstroService;
use proto::{
    ChatControlAction, ChatControlRequest, ChatEvent, ChatRequest, ContextUsageEvent,
    ContextUsageSegment, Empty, FileListRequest, FileListResponse, ImageEvent, ImageRequest,
    McpReconnectRequest, McpServerList, McpServerListRequest, MemoryQuery, MemoryResult,
    SessionEvent, SessionSnippet as ProtoSessionSnippet, SkillEvent, SkillInfo, SkillList,
    SkillRequest, SubscribeSessionEventsRequest, UsageEvent,
};
use providers::PauseControl;
use providers::ProviderConfig;
use tokio::sync::{Mutex, OnceCell, OwnedMutexGuard, RwLock};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use super::interrupt_store::{clear_interrupt_file, resume_items_from_proto, save_interrupt_file};
use crate::{
    to_proto, MemoryUpdatedPayload, PendingChangedPayload, SessionEventHub, SessionEventMsg,
    SubscribeFilter,
};

fn open_sessions(memory_dir: &std::path::Path) -> Result<session::SessionStore, String> {
    memory::ensure_workspace(memory_dir).map_err(|e| e.to_string())?;
    session::SessionStore::open_sessions_dir(&memory_dir.join("sessions"))
        .map_err(|e| e.to_string())
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

/// 将 ChatRequest 下传的辅助目标按 `task` 分组、按 `order` 排序后写入 Session。
///
/// 未知 `task` 字符串静默跳过（旧客户端/脏数据不阻塞主聊）；API key 仅存内存。
fn parse_auxiliary_targets(
    items: Vec<proto::AuxiliaryModelTarget>,
) -> HashMap<types::AuxiliaryTask, Vec<types::ChatTarget>> {
    let mut grouped: HashMap<types::AuxiliaryTask, Vec<(u32, types::ChatTarget)>> = HashMap::new();
    for item in items {
        let Some(task) = types::AuxiliaryTask::parse(item.task.trim()) else {
            continue;
        };
        grouped.entry(task).or_default().push((
            item.order,
            types::ChatTarget {
                provider_id: item.provider_id,
                backend_id: item.backend_id,
                model: item.model,
                api_key: item.api_key,
                base_url: item.base_url,
            },
        ));
    }
    grouped
        .into_iter()
        .map(|(task, mut ordered)| {
            ordered.sort_by_key(|(order, _)| *order);
            (task, ordered.into_iter().map(|(_, t)| t).collect())
        })
        .collect()
}

/// 会话 Agent 循环的共享句柄。
type SessionHandle = Arc<Session>;

#[derive(Clone)]
struct PauseRegistration {
    control: Arc<PauseControl>,
    session: Weak<Session>,
    hitl_gate: Arc<HitlGate>,
    ui_generation: ::hooks::UiTimelineGeneration,
    operation: Arc<Mutex<()>>,
}

struct GenerationLaunchReply<T> {
    /// The worker retains the generation operation until this handoff is accepted or dropped.
    value: Option<T>,
    accepted: Option<tokio::sync::oneshot::Sender<bool>>,
}

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

impl<T> GenerationLaunchReply<T> {
    fn into_value(mut self) -> T {
        if let Some(accepted) = self.accepted.take() {
            let _ = accepted.send(true);
        }
        self.value.take().expect("generation launch value missing")
    }
}

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
/// Chat RPC 返回的事件流类型别名。
type ChatStream = Pin<Box<dyn futures::Stream<Item = Result<ChatEvent, Status>> + Send>>;
/// SubscribeSessionEvents RPC 返回的事件流类型别名。
type SessionEventsStream =
    Pin<Box<dyn futures::Stream<Item = Result<SessionEvent, Status>> + Send>>;

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

/// 工具入 pending 时 publish 全局 `pending_changed`（+ live_written=false 的 memory_updated）。
///
/// 回合内 **live** 工具写仍只走 Chat `MemoryUpdate`，不调用本函数。
fn publish_tool_pending_to_hub(
    hub: &SessionEventHub,
    memory_dir: &std::path::Path,
    agent_id: &str,
    summary: &str,
) {
    let pending_count = memory::list_pending(memory_dir)
        .map(|v| v.len() as u32)
        .unwrap_or(0);
    hub.publish(SessionEventMsg {
        session_id: None,
        agent_id: agent_id.to_string(),
        memory_updated: Some(MemoryUpdatedPayload {
            source: "tool".into(),
            target: "mixed".into(),
            summary: summary.to_string(),
            live_written: false,
        }),
        pending_changed: None,
        session_metadata_changed: None,
    });
    hub.publish(SessionEventMsg {
        session_id: None,
        agent_id: agent_id.to_string(),
        memory_updated: None,
        pending_changed: Some(PendingChangedPayload {
            pending_count,
            reason: "enqueued".into(),
        }),
        session_metadata_changed: None,
    });
}

/// 启动 background review，完成后将结果 fire-and-forget 发布到 [`SessionEventHub`]。
///
/// 本函数只在拿锁并 `spawn` 等待任务后立即返回；**不**阻塞 Chat 流。
async fn spawn_review_to_hub(session: &SessionHandle, session_id: &str, hub: &SessionEventHub) {
    let hub = hub.clone();
    let sid = session_id.to_string();
    let (notify_tx, mut notify_rx) = tokio::sync::mpsc::unbounded_channel();
    let (agent_id, memory_dir) = {
        let agent = session.as_ref();
        let id = agent.agent_id().to_string();
        let dir = agent.memory_dir().to_path_buf();
        agent::exec::memory_review::spawn_background_review_after_turn(agent, Some(notify_tx))
            .await;
        (id, dir)
    };
    tokio::spawn(async move {
        if let Some(n) = notify_rx.recv().await {
            let live_written = !indicates_pending_enqueue(&n.content);
            hub.publish(SessionEventMsg {
                session_id: Some(sid),
                agent_id: agent_id.clone(),
                memory_updated: Some(MemoryUpdatedPayload {
                    source: "review".into(),
                    target: "mixed".into(),
                    summary: n.content.clone(),
                    live_written,
                }),
                pending_changed: None,
                session_metadata_changed: None,
            });
            if !live_written {
                let pending_count = memory::list_pending(&memory_dir)
                    .map(|v| v.len() as u32)
                    .unwrap_or(0);
                hub.publish(SessionEventMsg {
                    session_id: None,
                    agent_id,
                    memory_updated: None,
                    pending_changed: Some(PendingChangedPayload {
                        pending_count,
                        reason: "enqueued".into(),
                    }),
                    session_metadata_changed: None,
                });
            }
        }
    });
}

/// 启动首轮标题生成，成功后发布 `session_metadata_changed`。
async fn spawn_title_to_hub(session: &SessionHandle, hub: &SessionEventHub) {
    let hub = hub.clone();
    let (notify_tx, mut notify_rx) = tokio::sync::mpsc::unbounded_channel();
    let agent_id = {
        let agent = session.as_ref();
        let id = agent.agent_id().to_string();
        agent::exec::title_generation::spawn_title_generation_after_turn(agent, Some(notify_tx));
        id
    };
    tokio::spawn(async move {
        if let Some(n) = notify_rx.recv().await {
            hub.publish(SessionEventMsg {
                session_id: Some(n.session_id),
                agent_id,
                memory_updated: None,
                pending_changed: None,
                session_metadata_changed: Some(crate::SessionMetadataChangedPayload {
                    title: n.title,
                }),
            });
        }
    });
}

/// Astro gRPC 服务实现：会话 Agent、流式聊天、记忆与技能等 RPC。
#[derive(Clone)]
pub struct AstroServiceImpl {
    /// session_id → Agent 循环句柄。
    sessions: Arc<RwLock<HashMap<String, SessionHandle>>>,
    /// session_id → 暂停控制器。
    pause_controls: Arc<StdRwLock<HashMap<String, PauseRegistration>>>,
    /// session_id → generation 操作锁（Weak 以免 release 后无限增长）。
    generation_operations: GenerationOperations,
    /// In-flight release ownership, weakly retained so completed unique ids are reclaimed.
    release_ownerships: ReleaseOwnerships,
    /// session_id → 活 HITL 闸门。
    hitl_registry: HitlRegistry,
    /// 记忆根目录。
    memory_dir: PathBuf,
    /// Plugin / Gateway / Shell 钩子运行时。
    hook_runtime: Arc<::hooks::HookRuntime>,
    /// 会话记忆副作用事件 fan-out（SubscribeSessionEvents）。
    session_events: SessionEventHub,
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
                        detail: "backend ready".into(),
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
            pause_controls: Arc::new(StdRwLock::new(HashMap::new())),
            generation_operations: Arc::new(StdMutex::new(HashMap::new())),
            release_ownerships: Arc::new(StdMutex::new(HashMap::new())),
            hitl_registry: HitlRegistry::new(),
            memory_dir,
            hook_runtime,
            session_events: SessionEventHub::new(64),
        }
    }

    /// 会话记忆事件 hub（供 Chat / 其它 RPC 发布副作用）。
    pub(crate) fn session_event_hub(&self) -> &SessionEventHub {
        &self.session_events
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

    /// 获取或惰性创建会话对应的 [`Session`]。
    ///
    /// 新建时读取当前 active agent 的 [`AgentRuntimeConfig`]，并用请求的 `session_id` 构建。
    ///
    /// # 错误
    /// Agent 构建失败时返回 `Status::internal`。
    async fn get_session(&self, session_id: &str) -> Result<SessionHandle, Status> {
        let mut sessions = self.sessions.write().await;
        if let Some(handle) = sessions.get(session_id) {
            return Ok(handle.clone());
        }

        let agent_id = home::active_agent_id(&self.memory_dir);
        let mut builder = AgentBuilder::new(self.memory_dir.clone()).agent_id(agent_id.clone());
        if let Ok(rt) = AgentRuntimeConfig::load(&self.memory_dir, &agent_id) {
            builder = builder.from_runtime_config(&rt);
        }
        let (mut agent, _) = builder
            .build_with_session_id(session_id.to_string())
            .map_err(|e| Status::internal(e.to_string()))?;
        agent.set_hook_bus(Arc::clone(&self.hook_runtime.plugin));
        let handle = Arc::new(agent);
        self.release_ownerships
            .lock()
            .expect("release ownership registry mutex poisoned")
            .remove(session_id);
        sessions.insert(session_id.to_string(), handle.clone());
        Ok(handle)
    }

    /// 原子切换同一 chat generation 的 pause / HITL / UI 状态。
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
        let operation = self.generation_operation(session_id);
        let (registration, detached_gate, orphan_gate, removed, should_finalize_without_runtime) = {
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
        if let Some(handle) = removed.as_ref() {
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

    /// UI「新建对话」：Gateway `command:new_chat` + Plugin reset/finalize，并卸内存会话。
    async fn release_session_for_new_chat(&self, session_id: &str) {
        let payload = ::hooks::HookPayload {
            session_id: session_id.to_string(),
            turn_id: None,
            detail: "new_chat".into(),
            ..Default::default()
        };
        self.hook_runtime
            .fire_gateway(::hooks::COMMAND_NEW_CHAT, &payload);
        let _ = self
            .hook_runtime
            .fire_plugin(::hooks::ON_SESSION_RESET, &payload);
        if self
            .release_session_runtime_for_new_chat(session_id)
            .await
            .should_finalize_without_runtime
        {
            let _ = self
                .hook_runtime
                .fire_plugin(::hooks::ON_SESSION_FINALIZE, &payload);
        }
    }
}

/// 解析 RunFinished.interrupts_json 为 proto Interrupt 列表。
fn parse_interrupts_json(raw: &str) -> Vec<proto::Interrupt> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    let Some(arr) = value.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|item| {
            Some(proto::Interrupt {
                id: item.get("id")?.as_str()?.to_string(),
                reason: item
                    .get("reason")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                message: item
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                tool_call_id: item
                    .get("tool_call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                response_schema_json: item
                    .get("response_schema_json")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                expires_at: item
                    .get("expires_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                metadata_json: item
                    .get("metadata_json")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect()
}

/// 将 agent 多轮流事件映射为 proto [`ChatEvent`]；无对应项时返回 `None`（当前均有映射）。
fn media_asset_to_proto(asset: types::MediaAsset) -> proto::MediaAsset {
    let (ref_kind, ref_value) = match asset.reference {
        types::MediaRef::WorkspacePath(p) => ("workspace_path", p),
        types::MediaRef::DataUrl(u) => ("data_url", u),
        types::MediaRef::RemoteUri(u) => ("remote_uri", u),
    };
    let kind = match asset.kind {
        types::MediaKind::Image => "image",
        types::MediaKind::Audio => "audio",
        types::MediaKind::Video => "video",
        types::MediaKind::File => "file",
    };
    proto::MediaAsset {
        kind: kind.into(),
        mime_type: asset.mime_type,
        ref_kind: ref_kind.into(),
        ref_value,
        label: asset.label.unwrap_or_default(),
        id: asset.id.unwrap_or_default(),
    }
}

fn multi_turn_to_chat_event(item: MultiTurnStreamItem) -> Option<ChatEvent> {
    match item {
        MultiTurnStreamItem::Assistant(StreamedAssistantContent::Text(token)) => Some(ChatEvent {
            payload: Some(proto::chat_event::Payload::Token(token)),
        }),
        MultiTurnStreamItem::Assistant(StreamedAssistantContent::Reasoning(r)) => Some(ChatEvent {
            payload: Some(proto::chat_event::Payload::Reasoning(r)),
        }),
        MultiTurnStreamItem::Assistant(StreamedAssistantContent::ThoughtSignature(_)) => None,
        MultiTurnStreamItem::Assistant(StreamedAssistantContent::ToolCallDelta(d)) => {
            Some(ChatEvent {
                payload: Some(proto::chat_event::Payload::ToolCallDelta(
                    proto::ToolCallDeltaEvent {
                        index: d.index,
                        id: d.id.unwrap_or_default(),
                        name: d.name.unwrap_or_default(),
                        arguments: d.arguments.unwrap_or_default(),
                    },
                )),
            })
        }
        MultiTurnStreamItem::Assistant(StreamedAssistantContent::FinalUsage(u)) => {
            Some(ChatEvent {
                payload: Some(proto::chat_event::Payload::Usage(UsageEvent {
                    prompt_tokens: u.prompt_tokens(),
                    completion_tokens: u.completion_tokens(),
                    total_tokens: u.total_tokens(),
                })),
            })
        }
        MultiTurnStreamItem::Assistant(StreamedAssistantContent::Citations(cites)) => {
            Some(ChatEvent {
                payload: Some(proto::chat_event::Payload::CitationsJson(
                    serde_json::to_string(&cites).unwrap_or_default(),
                )),
            })
        }
        MultiTurnStreamItem::Assistant(StreamedAssistantContent::InteractionId(_)) => None,
        MultiTurnStreamItem::ToolStarted {
            id,
            name,
            arguments_json,
        } => Some(ChatEvent {
            payload: Some(proto::chat_event::Payload::ToolCall(proto::ToolCallEvent {
                id,
                name,
                arguments_json,
                result: String::new(),
                media: Vec::new(),
                phase: "started".into(),
            })),
        }),
        MultiTurnStreamItem::ToolResult {
            id,
            name,
            arguments_json,
            result,
            media,
        } => Some(ChatEvent {
            payload: Some(proto::chat_event::Payload::ToolCall(proto::ToolCallEvent {
                id,
                name,
                arguments_json,
                result,
                media: media.into_iter().map(media_asset_to_proto).collect(),
                phase: "completed".into(),
            })),
        }),
        MultiTurnStreamItem::MemoryUpdate { op, content } => Some(ChatEvent {
            payload: Some(proto::chat_event::Payload::MemoryUpdate(
                proto::MemoryUpdateEvent {
                    operation: op,
                    content,
                },
            )),
        }),
        MultiTurnStreamItem::ContextUsage(snap) => Some(ChatEvent {
            payload: Some(proto::chat_event::Payload::ContextUsage(
                ContextUsageEvent {
                    context_window: snap.context_window,
                    total_tokens: snap.total_tokens,
                    segments: snap
                        .segments
                        .into_iter()
                        .map(|s| ContextUsageSegment {
                            id: s.id,
                            tokens: s.tokens,
                            count: s.meta.and_then(|m| m.count).unwrap_or(0),
                            items: s
                                .items
                                .into_iter()
                                .map(|it| proto::ContextUsageItem {
                                    id: it.id,
                                    label: it.label,
                                    tokens: it.tokens,
                                })
                                .collect(),
                        })
                        .collect(),
                    updated_at: snap.updated_at,
                    recommend_compact: snap.recommend_compact,
                },
            )),
        }),
        MultiTurnStreamItem::RunStarted { thread_id, run_id } => Some(ChatEvent {
            payload: Some(proto::chat_event::Payload::RunStarted(
                proto::RunStartedEvent { thread_id, run_id },
            )),
        }),
        MultiTurnStreamItem::Activity {
            message_id,
            activity_type,
            content_json,
            replace,
        } => Some(ChatEvent {
            payload: Some(proto::chat_event::Payload::Activity(proto::ActivityEvent {
                message_id,
                activity_type,
                content_json,
                replace,
            })),
        }),
        MultiTurnStreamItem::RunFinished {
            run_id,
            outcome_type,
            interrupts_json,
        } => {
            let interrupts = parse_interrupts_json(&interrupts_json);
            Some(ChatEvent {
                payload: Some(proto::chat_event::Payload::RunFinished(
                    proto::RunFinishedEvent {
                        run_id,
                        outcome_type,
                        interrupts,
                    },
                )),
            })
        }
        MultiTurnStreamItem::Error(err) => Some(ChatEvent {
            payload: Some(proto::chat_event::Payload::Error(err)),
        }),
        MultiTurnStreamItem::Done => Some(ChatEvent {
            payload: Some(proto::chat_event::Payload::Done(true)),
        }),
    }
}

#[tonic::async_trait]
impl AstroService for AstroServiceImpl {
    /// [`chat`](Self::chat) 流类型。
    type ChatStream = ChatStream;
    /// [`generate_image`](Self::generate_image) 流类型。
    type GenerateImageStream =
        Pin<Box<dyn futures::Stream<Item = Result<ImageEvent, Status>> + Send>>;
    /// [`execute_skill`](Self::execute_skill) 流类型。
    type ExecuteSkillStream =
        Pin<Box<dyn futures::Stream<Item = Result<SkillEvent, Status>> + Send>>;
    /// [`subscribe_session_events`](Self::subscribe_session_events) 流类型。
    type SubscribeSessionEventsStream = SessionEventsStream;

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

    /// 多轮流式聊天：返回 `ChatEvent` 流（token / reasoning / tool / usage / done）。
    ///
    /// - `session_id` 为空时服务端生成 UUID
    /// - `provider` 为空时默认 `ollama`
    /// - 注入图片生成凭证、聊天密钥与 hooks，并注册 [`PauseControl`]
    ///
    /// # 返回
    /// 异步流；客户端断开或取消后会清理 pause 注册。
    ///
    /// # 错误
    /// 会话创建失败时返回 tonic `Status`。
    async fn chat(
        &self,
        request: Request<ChatRequest>,
    ) -> Result<Response<Self::ChatStream>, Status> {
        let req = request.into_inner();
        let session_id = if req.session_id.is_empty() {
            Uuid::new_v4().to_string()
        } else {
            req.session_id
        };

        // pre_gateway_dispatch：可 Skip / Rewrite
        let mut content = req.content;
        let dispatch = self.hook_runtime.fire_plugin(
            ::hooks::PRE_GATEWAY_DISPATCH,
            &::hooks::HookPayload {
                session_id: session_id.clone(),
                turn_id: None,
                message: Some(content.clone()),
                detail: content.chars().take(200).collect(),
                ..Default::default()
            },
        );
        match dispatch {
            ::hooks::HookOutcome::Skip(reason) => {
                let (tx, rx) = tokio::sync::mpsc::channel::<Result<ChatEvent, Status>>(4);
                let _ = tx
                    .send(Ok(ChatEvent {
                        payload: Some(proto::chat_event::Payload::Error(format!(
                            "请求被钩子跳过: {reason}"
                        ))),
                    }))
                    .await;
                let _ = tx
                    .send(Ok(ChatEvent {
                        payload: Some(proto::chat_event::Payload::Done(true)),
                    }))
                    .await;
                return Ok(Response::new(Box::pin(ReceiverStream::new(rx))));
            }
            ::hooks::HookOutcome::Rewrite(msg) => {
                content = msg;
            }
            _ => {}
        }

        let is_new_session = {
            let sessions = self.sessions.read().await;
            !sessions.contains_key(&session_id)
        };

        let provider_name = if req.provider.is_empty() {
            "ollama".to_string()
        } else {
            req.provider
        };
        let model = req.model;
        let _resume_json = req.resume_json;
        let _use_memory = req.use_memory;
        let api_key = req.api_key;
        let base_url = req.base_url;
        let chat_fallbacks = req.chat_fallbacks;
        let auxiliary_targets = parse_auxiliary_targets(req.auxiliary_targets);
        let thinking_enabled = req.thinking_enabled;
        // 当前模型最大输出 token（前端由模型元数据下传）；0 = 用默认。
        let chat_max_output_tokens = req.max_output_tokens;
        let image_data_urls: Vec<String> = req
            .images
            .iter()
            .filter_map(|image| {
                let mime = image.mime.trim();
                let data = image.data_base64.trim();
                if mime.is_empty() || data.is_empty() {
                    return None;
                }
                Some(format!("data:{mime};base64,{data}"))
            })
            .collect();
        let reasoning_effort = if req.reasoning_effort.trim().is_empty() {
            "high".to_string()
        } else {
            req.reasoning_effort
        };
        let image_targets = tools::image_gen_targets_from_parts(tools::ImageGenParts {
            provider: &req.image_gen_provider,
            model: &req.image_gen_model,
            api_key: &req.image_gen_api_key,
            base_url: &req.image_gen_base_url,
            fb_provider: &req.image_gen_fallback_provider,
            fb_model: &req.image_gen_fallback_model,
            fb_api_key: &req.image_gen_fallback_api_key,
            fb_base_url: &req.image_gen_fallback_base_url,
            video_model: &req.image_gen_video_model,
            music_model: &req.image_gen_music_model,
            tts_model: &req.image_gen_tts_model,
            fb_video_model: &req.image_gen_fallback_video_model,
            fb_music_model: &req.image_gen_fallback_music_model,
            fb_tts_model: &req.image_gen_fallback_tts_model,
            vision_model: &req.image_gen_vision_model,
            fb_vision_model: &req.image_gen_fallback_vision_model,
        });

        let session = self.get_session(&session_id).await?;
        let steered_turn_id = { session.steer_input(&content, &image_data_urls).await };
        if steered_turn_id.is_some() {
            let (tx, rx) = tokio::sync::mpsc::channel::<Result<ChatEvent, Status>>(1);
            let _ = tx
                .send(Ok(ChatEvent {
                    payload: Some(proto::chat_event::Payload::Done(true)),
                }))
                .await;
            return Ok(Response::new(Box::pin(ReceiverStream::new(rx))));
        }
        if is_new_session {
            self.hook_runtime.fire_gateway(
                ::hooks::SESSION_START,
                &::hooks::HookPayload {
                    session_id: session_id.clone(),
                    turn_id: None,
                    ..Default::default()
                },
            );
        }
        let (hook_tx, mut hook_rx) = tokio::sync::mpsc::unbounded_channel::<::hooks::UiHookEvent>();
        let interaction_mode = tools::InteractionMode::parse(&req.interaction_mode);
        let project_root = (!req.project_root.trim().is_empty())
            .then(|| std::path::PathBuf::from(req.project_root.trim()));
        let temperature_override = req
            .temperature
            .filter(|temperature| temperature.is_finite() && (0.0..=2.0).contains(temperature));
        let additional_params_override =
            serde_json::from_str::<serde_json::Value>(req.additional_params_json.trim())
                .ok()
                .filter(serde_json::Value::is_object);
        let context_window = (req.context_window > 0).then_some(req.context_window);
        let hitl_gate = HitlGate::new(session_id.clone());
        let Some(registration) = self
            .admit_pause_generation(&session_id, &session, Arc::clone(&hitl_gate), hook_tx)
            .await
        else {
            let (tx, rx) = tokio::sync::mpsc::channel::<Result<ChatEvent, Status>>(4);
            let _ = tx
                .send(Ok(ChatEvent {
                    payload: Some(proto::chat_event::Payload::Error(
                        "请先完成上方确认或澄清卡片（HITL waiting）".into(),
                    )),
                }))
                .await;
            let _ = tx
                .send(Ok(ChatEvent {
                    payload: Some(proto::chat_event::Payload::Done(true)),
                }))
                .await;
            return Ok(Response::new(Box::pin(ReceiverStream::new(rx))));
        };
        let pause = Arc::clone(&registration.control);
        let resolved_model = if model.is_empty() {
            providers::dispatch::default_model(&provider_name)
        } else {
            model.clone()
        };
        let resolved_api_key = if api_key.trim().is_empty() {
            providers::read_env_api_key(&provider_name).unwrap_or_default()
        } else {
            api_key.trim().to_string()
        };
        let resolved_base_url = (!base_url.trim().is_empty()).then(|| base_url.trim().to_string());
        let mut chat_targets = vec![types::ChatTarget {
            provider_id: String::new(),
            backend_id: provider_name.clone(),
            model: resolved_model.clone(),
            api_key: resolved_api_key.clone(),
            base_url: resolved_base_url.clone().unwrap_or_default(),
        }];
        for fb in chat_fallbacks {
            chat_targets.push(types::ChatTarget {
                provider_id: fb.provider_id,
                backend_id: fb.provider,
                model: fb.model,
                api_key: fb.api_key,
                base_url: fb.base_url,
            });
        }
        let setup_targets = chat_targets.clone();
        let setup_provider_name = provider_name.clone();
        let setup_model = model.clone();
        let setup_api_key = api_key.clone();
        let setup_base_url = base_url.clone();
        let launch_session = Arc::clone(&session);
        let setup_session = Arc::clone(&session);
        let launch_gate = Arc::clone(&hitl_gate);
        let Some(mut stream) = self
            .launch_current_pause_generation_with_setup(
                &session_id,
                &registration,
                &setup_session,
                move |session| async move {
                    session.set_image_gen_targets(image_targets);
                    session.set_chat_credentials(
                        &setup_provider_name,
                        &setup_model,
                        &setup_api_key,
                        &setup_base_url,
                    );
                    session.set_auxiliary_targets(auxiliary_targets);
                    if let Some(context_window) = context_window {
                        session.set_context_window(context_window);
                    }
                    session.set_interaction_mode(interaction_mode).await;
                    session.set_project_root(project_root);
                    if let Some(temperature) = temperature_override {
                        session.set_temperature(temperature);
                    }
                    if let Some(additional_params) = additional_params_override {
                        session.set_additional_params(additional_params);
                    }
                    session.set_chat_targets(setup_targets);
                },
                move || async move {
                    let config = ProviderConfig {
                        model: resolved_model,
                        api_key: resolved_api_key,
                        base_url: resolved_base_url,
                        temperature: launch_session.temperature(),
                        thinking_enabled,
                        reasoning_effort,
                        additional_params: launch_session.additional_params(),
                        max_tokens: if chat_max_output_tokens > 0 {
                            chat_max_output_tokens
                        } else {
                            8192
                        },
                        ..ProviderConfig::default()
                    };
                    stream_multi_turn_with_hitl(
                        launch_session,
                        chat_targets,
                        config,
                        vec![TurnInput {
                            content,
                            image_data_urls,
                        }],
                        pause,
                        Some(launch_gate),
                    )
                    .await
                },
            )
            .await
        else {
            self.cleanup_pause_generation(&session_id, &registration)
                .await;
            let (tx, rx) = tokio::sync::mpsc::channel::<Result<ChatEvent, Status>>(1);
            let _ = tx
                .send(Ok(ChatEvent {
                    payload: Some(proto::chat_event::Payload::Done(true)),
                }))
                .await;
            return Ok(Response::new(Box::pin(ReceiverStream::new(rx))));
        };
        let registration_for_cleanup = registration.clone();
        let pause_controls = self.pause_controls.clone();
        let generation_operations = self.generation_operations.clone();
        let hitl_registry = self.hitl_registry.clone();
        let memory_dir = self.memory_dir.clone();
        let sid_cleanup = session_id.clone();
        let hook_runtime = Arc::clone(&self.hook_runtime);
        let ui_slot = self.hook_runtime.ui_slot.clone();
        let session_events_hub = self.session_event_hub().clone();

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<ChatEvent, Status>>(8);
        let hook_out = tx.clone();
        tokio::spawn(async move {
            while let Some(ev) = hook_rx.recv().await {
                let _ = hook_out
                    .send(Ok(ChatEvent {
                        payload: Some(proto::chat_event::Payload::Hook(proto::HookEvent {
                            name: ev.name,
                            detail: ev.detail,
                            outcome: ev.outcome,
                        })),
                    }))
                    .await;
            }
        });

        tokio::spawn(async move {
            let session_for_review = session.clone();
            let agent_id_for_events = { session.agent_id().to_string() };
            let mut run_succeeded = false;
            while let Some(item) = stream.next().await {
                match item {
                    Ok(mt) => {
                        let is_done = matches!(mt, MultiTurnStreamItem::Done);
                        if let MultiTurnStreamItem::RunFinished {
                            ref outcome_type,
                            ref interrupts_json,
                            ..
                        } = mt
                        {
                            run_succeeded = allows_post_turn_side_effects(outcome_type);
                            if outcome_type == "hitl_waiting" || outcome_type == "interrupt" {
                                let interrupts: Vec<agent::Interrupt> =
                                    serde_json::from_str(interrupts_json).unwrap_or_default();
                                if !interrupts.is_empty() {
                                    let _admission =
                                        registration_for_cleanup.operation.lock().await;
                                    let is_current = pause_controls
                                        .read()
                                        .expect("pause registry lock poisoned")
                                        .get(&sid_cleanup)
                                        .is_some_and(|current| {
                                            Arc::ptr_eq(
                                                &current.control,
                                                &registration_for_cleanup.control,
                                            )
                                        });
                                    if is_current {
                                        let _ = save_interrupt_file(
                                            &memory_dir,
                                            &sid_cleanup,
                                            &interrupts,
                                        );
                                    }
                                }
                            }
                        }
                        // 工具入 pending → 只走 Hub（不刷 Chat 时间线）；live 仍走 Chat MemoryUpdate
                        let skip_chat_memory_update = matches!(
                            &mt,
                            MultiTurnStreamItem::MemoryUpdate { content, .. }
                                if indicates_pending_enqueue(content)
                        );
                        if let MultiTurnStreamItem::MemoryUpdate { ref content, .. } = mt {
                            if indicates_pending_enqueue(content) {
                                publish_tool_pending_to_hub(
                                    &session_events_hub,
                                    &memory_dir,
                                    &agent_id_for_events,
                                    content,
                                );
                            }
                        }
                        if !skip_chat_memory_update {
                            if let Some(event) = multi_turn_to_chat_event(mt) {
                                if tx.send(Ok(event)).await.is_err() {
                                    break;
                                }
                            }
                        }
                        if is_done {
                            if run_succeeded {
                                // 仅成功回合 fire-and-forget review / 标题 → SessionEventHub。
                                spawn_review_to_hub(
                                    &session_for_review,
                                    &sid_cleanup,
                                    &session_events_hub,
                                )
                                .await;
                                spawn_title_to_hub(&session_for_review, &session_events_hub).await;
                            }
                            break;
                        }
                    }
                    Err(err) => {
                        let _ = tx
                            .send(Ok(ChatEvent {
                                payload: Some(proto::chat_event::Payload::Error(err.to_string())),
                            }))
                            .await;
                        break;
                    }
                }
            }
            hook_runtime.fire_gateway(
                ::hooks::AGENT_END,
                &::hooks::HookPayload {
                    session_id: sid_cleanup.clone(),
                    turn_id: None,
                    ..Default::default()
                },
            );
            cleanup_pause_generation_parts(
                &generation_operations,
                &pause_controls,
                &hitl_registry,
                &ui_slot,
                &memory_dir,
                &sid_cleanup,
                &registration_for_cleanup,
            )
            .await;
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
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

            match providers::dispatch::generate_image(&provider_name, &prompt, &config).await {
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
    /// 若 `tools-enabled.json` 中 `skills=false`，返回 error 事件。
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
                            "技能工具已禁用（tools-enabled.json → skills=false）".into(),
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

        let configs = mcp::load_mcp_servers_layered(Some(&agent_id), project_root.as_deref())
            .unwrap_or_default();
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
        let sessions =
            open_sessions(&self.memory_dir).map_err(|e| Status::internal(e.to_string()))?;

        let sessions = sessions
            .search_messages(&query.query, None, None, query.limit.max(1) as i64)
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

    /// 订阅会话级记忆副作用事件（记忆更新 / pending 变化），与 Chat 流生命周期解耦。
    ///
    /// - `session_id` 为空：仅全局 pending
    /// - `session_id` 非空：该 session 事件 + 全局 pending
    /// - `agent_id` 为空：不过滤 agent
    async fn subscribe_session_events(
        &self,
        request: Request<SubscribeSessionEventsRequest>,
    ) -> Result<Response<Self::SubscribeSessionEventsStream>, Status> {
        let req = request.into_inner();
        let filter = SubscribeFilter {
            session_id: if req.session_id.trim().is_empty() {
                None
            } else {
                Some(req.session_id)
            },
            agent_id: if req.agent_id.trim().is_empty() {
                None
            } else {
                Some(req.agent_id)
            },
        };
        let hub = self.session_events.clone();
        let resume_stream_id = req.stream_id;
        let after_event_id = req.after_event_id;
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        tokio::spawn(async move {
            let mut filtered = hub.subscribe(filter, &resume_stream_id, after_event_id);
            let stream_id = hub.stream_id().to_string();
            while let Some(ev) = filtered.recv().await {
                if tx.send(Ok(to_proto(&ev, &stream_id))).await.is_err() {
                    break;
                }
            }
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    async fn count_tokens(
        &self,
        request: Request<proto::CountTokensRequest>,
    ) -> Result<Response<proto::CountTokensResponse>, Status> {
        let req = request.into_inner();
        let api_key = providers::read_env_api_key("claude").unwrap_or_default();
        if api_key.is_empty() {
            return Err(Status::failed_precondition("缺少 ANTHROPIC_API_KEY"));
        }
        let config = providers::ProviderConfig {
            api_key,
            model: if req.model.is_empty() {
                "claude-opus-4-8".into()
            } else {
                req.model
            },
            ..providers::ProviderConfig::default()
        };
        let client = reqwest::Client::new();
        let count =
            providers::anthropic::token_count::anthropic_count_tokens(&client, &[], &[], &config)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(proto::CountTokensResponse {
            input_tokens: count,
        }))
    }

    async fn create_batch(
        &self,
        request: Request<proto::BatchCreateRequest>,
    ) -> Result<Response<proto::BatchResponse>, Status> {
        let req = request.into_inner();
        let api_key = providers::read_env_api_key("claude").unwrap_or_default();
        if api_key.is_empty() {
            return Err(Status::failed_precondition("缺少 ANTHROPIC_API_KEY"));
        }
        let requests: Vec<serde_json::Value> = serde_json::from_str(&req.requests_json)
            .map_err(|e| Status::invalid_argument(format!("requests_json: {e}")))?;
        let config = providers::ProviderConfig {
            api_key,
            ..providers::ProviderConfig::default()
        };
        let client = reqwest::Client::new();
        let result =
            providers::anthropic::batch::anthropic_create_batch(&client, &requests, &config)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(proto::BatchResponse {
            response_json: result.to_string(),
        }))
    }

    async fn get_batch(
        &self,
        request: Request<proto::BatchStatusRequest>,
    ) -> Result<Response<proto::BatchResponse>, Status> {
        let req = request.into_inner();
        let api_key = providers::read_env_api_key("claude").unwrap_or_default();
        if api_key.is_empty() {
            return Err(Status::failed_precondition("缺少 ANTHROPIC_API_KEY"));
        }
        let config = providers::ProviderConfig {
            api_key,
            ..providers::ProviderConfig::default()
        };
        let client = reqwest::Client::new();
        let result =
            providers::anthropic::batch::anthropic_get_batch(&client, &req.batch_id, &config)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(proto::BatchResponse {
            response_json: result.to_string(),
        }))
    }

    async fn list_batches(
        &self,
        request: Request<proto::BatchListRequest>,
    ) -> Result<Response<proto::BatchResponse>, Status> {
        let _req = request.into_inner();
        let api_key = providers::read_env_api_key("claude").unwrap_or_default();
        if api_key.is_empty() {
            return Err(Status::failed_precondition("缺少 ANTHROPIC_API_KEY"));
        }
        let config = providers::ProviderConfig {
            api_key,
            ..providers::ProviderConfig::default()
        };
        let client = reqwest::Client::new();
        let result = providers::anthropic::batch::anthropic_list_batches(&client, &config)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(proto::BatchResponse {
            response_json: result.to_string(),
        }))
    }

    async fn get_batch_results(
        &self,
        request: Request<proto::BatchStatusRequest>,
    ) -> Result<Response<proto::BatchResultsResponse>, Status> {
        let req = request.into_inner();
        let api_key = providers::read_env_api_key("claude").unwrap_or_default();
        if api_key.is_empty() {
            return Err(Status::failed_precondition("缺少 ANTHROPIC_API_KEY"));
        }
        let config = providers::ProviderConfig {
            api_key,
            ..providers::ProviderConfig::default()
        };
        let client = reqwest::Client::new();
        let result =
            providers::anthropic::batch::anthropic_batch_results(&client, &req.batch_id, &config)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(proto::BatchResultsResponse {
            results_jsonl: result,
        }))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use agent::streaming::{run_multi_turn_stream_with_chat_fn, ChatOverride};
    use providers::CompletionStream;
    use tempfile::TempDir;

    #[test]
    fn only_successful_run_allows_post_turn_side_effects() {
        assert!(allows_post_turn_side_effects("success"));
        for outcome in ["error", "interrupt", "hitl_waiting", ""] {
            assert!(!allows_post_turn_side_effects(outcome), "outcome={outcome}");
        }
    }

    #[test]
    fn tool_lifecycle_maps_to_started_and_completed_phases() {
        let started = multi_turn_to_chat_event(MultiTurnStreamItem::ToolStarted {
            id: "call-1".into(),
            name: "echo".into(),
            arguments_json: r#"{"text":"hello"}"#.into(),
        })
        .expect("started event");
        let Some(proto::chat_event::Payload::ToolCall(started)) = started.payload else {
            panic!("tool call payload");
        };
        assert_eq!(started.phase, "started");
        assert!(started.result.is_empty());

        let completed = multi_turn_to_chat_event(MultiTurnStreamItem::ToolResult {
            id: "call-1".into(),
            name: "echo".into(),
            arguments_json: r#"{"text":"hello"}"#.into(),
            result: "hello".into(),
            media: Vec::new(),
        })
        .expect("completed event");
        let Some(proto::chat_event::Payload::ToolCall(completed)) = completed.payload else {
            panic!("tool call payload");
        };
        assert_eq!(completed.phase, "completed");
        assert_eq!(completed.result, "hello");
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
        tokio::sync::mpsc::Receiver<anyhow::Result<MultiTurnStreamItem>>,
    ) {
        let chat: ChatOverride = Arc::new(|_, _, _| {
            Box::pin(async move { Ok(Box::pin(futures::stream::pending()) as CompletionStream) })
        });
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let handle = tokio::spawn(run_multi_turn_stream_with_chat_fn(
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
        assert!(matches!(started, MultiTurnStreamItem::RunStarted { .. }));
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
                                .set_interaction_mode(tools::InteractionMode::Ask)
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
        assert_eq!(second, (tools::InteractionMode::Ask, 1.4));
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
                                .set_interaction_mode(tools::InteractionMode::Ask)
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
                                .set_interaction_mode(tools::InteractionMode::Ask)
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
            tools::InteractionMode::Ask
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
            .register(::hooks::ON_SESSION_FINALIZE, move |_| {
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
            .register(::hooks::ON_SESSION_FINALIZE, move |_| {
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
            .register(::hooks::ON_SESSION_FINALIZE, move |_| {
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
            .register(::hooks::ON_SESSION_FINALIZE, move |_| {
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
                ::hooks::ON_SESSION_FINALIZE,
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
            .register(::hooks::ON_SESSION_FINALIZE, move |_| {
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
            ::hooks::ON_SESSION_FINALIZE,
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
            .register(::hooks::ON_SESSION_FINALIZE, move |_| {
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
            .register(::hooks::ON_SESSION_FINALIZE, move |_| {
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
            "name: audit\nevents:\n  - command:new_chat\n",
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
            .register(::hooks::ON_SESSION_RESET, move |_| {
                reset_counter.fetch_add(1, Ordering::SeqCst);
                ::hooks::HookOutcome::Continue
            });
        let finalize_hits = Arc::new(AtomicUsize::new(0));
        let finalize_counter = Arc::clone(&finalize_hits);
        service
            .hook_runtime
            .plugin
            .register(::hooks::ON_SESSION_FINALIZE, move |_| {
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
        let map = parse_auxiliary_targets(vec![
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
}
