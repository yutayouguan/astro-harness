//! Astro gRPC [`AstroService`] 实现：聊天流、会话、记忆、MCP、技能与文件列表。

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex as StdMutex, Weak};

use agent::builder::AgentBuilder;
use agent::runtime::Session;
use agent::streaming::{
    stream_multi_turn_with_hitl, MultiTurnStreamItem, StreamedAssistantContent,
};
use agent::{HitlGate, HitlRegistry, TurnAbortReason, TurnInput};
use futures::StreamExt;
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
use tokio::sync::{Mutex, RwLock};
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

type GenerationOperations = Arc<StdMutex<HashMap<String, Weak<Mutex<()>>>>>;

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
    pause_controls: &Arc<RwLock<HashMap<String, PauseRegistration>>>,
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
    pause_controls: &Arc<RwLock<HashMap<String, PauseRegistration>>>,
    hitl_registry: &HitlRegistry,
    ui_slot: &::hooks::UiTimelineSlot,
    memory_dir: &std::path::Path,
    session_id: &str,
    registration: &PauseRegistration,
) -> (bool, Option<Arc<HitlGate>>) {
    let mut registrations = pause_controls.write().await;
    let removed_current = if registrations
        .get(session_id)
        .is_some_and(|current| Arc::ptr_eq(&current.control, &registration.control))
    {
        registrations.remove(session_id);
        true
    } else {
        false
    };
    drop(registrations);

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
pub struct AstroServiceImpl {
    /// session_id → Agent 循环句柄。
    sessions: Arc<RwLock<HashMap<String, SessionHandle>>>,
    /// session_id → 暂停控制器。
    pause_controls: Arc<RwLock<HashMap<String, PauseRegistration>>>,
    /// session_id → generation 操作锁（Weak 以免 release 后无限增长）。
    generation_operations: GenerationOperations,
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
            pause_controls: Arc::new(RwLock::new(HashMap::new())),
            generation_operations: Arc::new(StdMutex::new(HashMap::new())),
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
        let operation = self.generation_operation(session_id);
        let (registration, old_registration, old_registry_gate) = {
            let _admission = operation.lock().await;
            if let Some(current_gate) = self.hitl_registry.get(session_id).await {
                if current_gate.is_waiting().await {
                    return None;
                }
            }

            let old_registry_gate = self.hitl_registry.remove(session_id).await;
            let ui_generation = self.hook_runtime.ui_slot.install_tx(session_id, hook_tx);
            let registration = PauseRegistration {
                control: PauseControl::new(),
                session: Arc::downgrade(session),
                hitl_gate: Arc::clone(&hitl_gate),
                ui_generation,
                operation: Arc::clone(&operation),
            };
            let old_registration = self
                .pause_controls
                .write()
                .await
                .insert(session_id.to_string(), registration.clone());
            self.hitl_registry.insert(hitl_gate).await;
            clear_interrupt_file(&self.memory_dir, session_id);
            if let Some(old) = old_registration.as_ref() {
                old.control.cancel();
            }
            (registration, old_registration, old_registry_gate)
        };

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
        Some(registration)
    }

    async fn launch_current_pause_generation_with<F, Fut, T>(
        &self,
        session_id: &str,
        registration: &PauseRegistration,
        launch: F,
    ) -> Option<T>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = T>,
    {
        let _admission = registration.operation.lock().await;
        let is_current = self
            .pause_controls
            .read()
            .await
            .get(session_id)
            .is_some_and(|current| Arc::ptr_eq(&current.control, &registration.control));
        if !is_current || registration.control.is_cancelled() {
            return None;
        }
        Some(launch().await)
    }

    async fn cleanup_pause_generation(
        &self,
        session_id: &str,
        registration: &PauseRegistration,
    ) -> bool {
        cleanup_pause_generation_parts(
            &self.generation_operations,
            &self.pause_controls,
            &self.hitl_registry,
            &self.hook_runtime.ui_slot,
            &self.memory_dir,
            session_id,
            registration,
        )
        .await
    }

    async fn cancel_current_pause_generation_with<F, Fut>(
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
        let Some(registration) = self.pause_controls.read().await.get(session_id).cloned() else {
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
                let turn_id = session.current_turn_id().await;
                let abort_result = session.abort_all_tasks(TurnAbortReason::Interrupted).await;
                if abort_result.is_err() {
                    if let Some(turn_id) = turn_id {
                        session.wait_for_task(&turn_id).await;
                    }
                }
                abort_result?;
            }
            Ok(())
        })
        .await
    }

    /// 释放会话运行时：取消暂停/HITL/中断文件，并从内存移除 Session。
    ///
    /// 返回被移除的会话句柄；Session 自身清理由幂等 `shutdown_runtime` 统一承接。
    async fn release_session_runtime(&self, session_id: &str) -> Option<SessionHandle> {
        let operation = self.generation_operation(session_id);
        let (registration, detached_gate, orphan_gate, removed) = {
            let _admission = operation.lock().await;
            let registration = self.pause_controls.read().await.get(session_id).cloned();
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
            (registration, detached_gate, orphan_gate, removed)
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
        removed
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
        if self.release_session_runtime(session_id).await.is_none() {
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
                .await
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
        {
            let agent = session.as_ref();
            agent.set_image_gen_targets(image_targets);
            agent.set_chat_credentials(&provider_name, &model, &api_key, &base_url);
            // 五类辅助目标随本轮 ChatRequest 刷新；未下传的任务在 Session 内回退主模型。
            agent.set_auxiliary_targets(auxiliary_targets);
            if req.context_window > 0 {
                agent.set_context_window(req.context_window);
            }
            agent
                .set_interaction_mode(tools::InteractionMode::parse(&req.interaction_mode))
                .await;
            let project_root = req.project_root.trim();
            if project_root.is_empty() {
                agent.set_project_root(None);
            } else {
                agent.set_project_root(Some(std::path::PathBuf::from(project_root)));
            }
            if let Some(t) = req.temperature {
                if t.is_finite() && (0.0..=2.0).contains(&t) {
                    agent.set_temperature(t);
                }
            }
            let raw_params = req.additional_params_json.trim();
            if !raw_params.is_empty() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw_params) {
                    if v.is_object() {
                        agent.set_additional_params(v);
                    }
                }
            }
        }
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
        let (temperature, additional_params) = (session.temperature(), session.additional_params());
        let config = ProviderConfig {
            model: if model.is_empty() {
                providers::dispatch::default_model(&provider_name)
            } else {
                model.clone()
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
            temperature,
            thinking_enabled,
            reasoning_effort: reasoning_effort.clone(),
            additional_params,
            max_tokens: if chat_max_output_tokens > 0 {
                chat_max_output_tokens
            } else {
                8192
            },
            ..ProviderConfig::default()
        };
        let mut chat_targets = vec![types::ChatTarget {
            provider_id: String::new(),
            backend_id: provider_name.clone(),
            model: config.model.clone(),
            api_key: config.api_key.clone(),
            base_url: config.base_url.clone().unwrap_or_default(),
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
        session.set_chat_targets(chat_targets.clone());
        let launch_session = Arc::clone(&session);
        let launch_gate = Arc::clone(&hitl_gate);
        let Some(mut stream) = self
            .launch_current_pause_generation_with(&session_id, &registration, move || async move {
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
            })
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
                                    let is_current =
                                        pause_controls.read().await.get(&sid_cleanup).is_some_and(
                                            |current| {
                                                Arc::ptr_eq(
                                                    &current.control,
                                                    &registration_for_cleanup.control,
                                                )
                                            },
                                        );
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
            .await
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
        service.pause_controls.write().await.insert(
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
            .await
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
            .await
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
            .await
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
            .await
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
            .await
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
