//! Astro gRPC [`AstroService`] 实现：聊天流、会话、记忆、MCP、技能与文件列表。

use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use agent::builder::AgentBuilder;
use agent::runtime::{AgentLoop, TurnResult};
use agent::streaming::{
    stream_multi_turn_with_hitl, MultiTurnStreamItem, StreamedAssistantContent,
};
use agent::{HitlGate, HitlRegistry};
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
    }
}

/// 将 ChatRequest 下传的辅助目标按 `task` 分组、按 `order` 排序后写入 AgentLoop。
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
type SessionHandle = Arc<Mutex<AgentLoop>>;
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
        let agent = session.lock().await;
        let id = agent.agent_id().to_string();
        let dir = agent.memory_dir().to_path_buf();
        agent::exec::memory_review::spawn_background_review_after_turn(&agent, Some(notify_tx));
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
        let agent = session.lock().await;
        let id = agent.agent_id().to_string();
        agent::exec::title_generation::spawn_title_generation_after_turn(&agent, Some(notify_tx));
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
    pause_controls: Arc<RwLock<HashMap<String, Arc<PauseControl>>>>,
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

    /// 获取或惰性创建会话对应的 [`AgentLoop`]。
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
        let (agent, _) = builder
            .build_with_session_id(session_id.to_string())
            .map_err(|e| Status::internal(e.to_string()))?;
        let handle = Arc::new(Mutex::new(agent));
        sessions.insert(session_id.to_string(), handle.clone());
        Ok(handle)
    }

    /// 同一 session 重入时取消旧 PauseControl，避免幽灵 pause
    async fn register_pause(&self, session_id: &str) -> Arc<PauseControl> {
        let pause = PauseControl::new();
        let mut map = self.pause_controls.write().await;
        if let Some(old) = map.insert(session_id.to_string(), pause.clone()) {
            old.cancel();
        }
        pause
    }

    /// 释放会话运行时：取消暂停/HITL/中断文件，并从内存移除 AgentLoop。
    ///
    /// 返回被移除的会话句柄，供 `new_chat` 在卸载后继续派发 session hooks。
    async fn release_session_runtime(&self, session_id: &str) -> Option<SessionHandle> {
        {
            let mut map = self.pause_controls.write().await;
            if let Some(pause) = map.remove(session_id) {
                pause.cancel();
            }
        }
        self.hitl_registry.cancel_and_remove(session_id).await;
        clear_interrupt_file(&self.memory_dir, session_id);

        let removed = {
            let mut sessions = self.sessions.write().await;
            sessions.remove(session_id)
        };
        if let Some(handle) = removed.as_ref() {
            handle.lock().await.cancel_signal().cancel();
        }
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
        let _ = self
            .hook_runtime
            .fire_plugin(::hooks::ON_SESSION_FINALIZE, &payload);

        if let Some(handle) = self.release_session_runtime(session_id).await {
            let agent = handle.lock().await;
            let bus = agent.hook_bus();
            let turn_id = agent.current_turn_id().map(str::to_string);
            let payload = ::hooks::HookPayload {
                session_id: session_id.to_string(),
                turn_id,
                detail: format!("session={session_id}"),
                ..Default::default()
            };
            let _ = bus.fire(::hooks::ON_SESSION_RESET, &payload);
            let _ = bus.fire(::hooks::ON_SESSION_FINALIZE, &payload);
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
                    "会话 {} 不在内存中（无活 AgentLoop 可刷新）",
                    req.session_id
                )));
            };
            drop(sessions);
            let mut agent = session.lock().await;
            agent
                .refresh_memory()
                .map_err(|e| Status::internal(e.to_string()))?;
            return Ok(Response::new(Empty {}));
        }

        let map = self.pause_controls.read().await;
        let Some(pause) = map.get(&req.session_id) else {
            return Err(Status::not_found(format!(
                "会话 {} 当前没有进行中的流式对话",
                req.session_id
            )));
        };
        match action {
            ChatControlAction::ChatControlPause => pause.pause(),
            ChatControlAction::ChatControlResume | ChatControlAction::ChatControlStreamResume => {
                pause.resume()
            }
            ChatControlAction::ChatControlCancel => {
                pause.cancel();
                drop(map);
                self.hitl_registry.cancel_and_remove(&req.session_id).await;
                clear_interrupt_file(&self.memory_dir, &req.session_id);
                if let Ok(session) = self.get_session(&req.session_id).await {
                    session.lock().await.cancel_signal().cancel();
                }
            }
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
        let Some(gate) = self.hitl_registry.get(&req.session_id).await else {
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
        if remaining.is_empty() {
            clear_interrupt_file(&self.memory_dir, &req.session_id);
        } else {
            let _ = save_interrupt_file(&self.memory_dir, &req.session_id, &remaining);
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
        let images = req.images;
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
            let mut agent = session.lock().await;
            agent.set_image_gen_targets(image_targets);
            agent.set_chat_credentials(&provider_name, &model, &api_key, &base_url);
            // 五类辅助目标随本轮 ChatRequest 刷新；未下传的任务在 AgentLoop 内回退主模型。
            agent.set_auxiliary_targets(auxiliary_targets);
            if req.context_window > 0 {
                agent.set_context_window(req.context_window);
            }
            agent.set_interaction_mode(tools::InteractionMode::parse(&req.interaction_mode));
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
            agent.set_hook_bus(Arc::clone(&self.hook_runtime.plugin));
            self.hook_runtime.ui_slot.set_tx(Some(hook_tx));
        }
        // 有活 HITL 时拒绝新 chat（须在 register_pause 之前，避免取消进行中的流）
        if let Some(gate) = self.hitl_registry.get(&session_id).await {
            if gate.is_waiting().await {
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
            }
        }

        let pause = self.register_pause(&session_id).await;
        let pause_controls = self.pause_controls.clone();
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
            let cleanup = || async {
                ui_slot.set_tx(None);
                let mut map = pause_controls.write().await;
                map.remove(&sid_cleanup);
                hitl_registry.cancel_and_remove(&sid_cleanup).await;
            };

            let turn_content = content;
            let image_data_urls: Vec<String> = images
                .iter()
                .filter_map(|img| {
                    let mime = img.mime.trim();
                    let data = img.data_base64.trim();
                    if mime.is_empty() || data.is_empty() {
                        return None;
                    }
                    Some(format!("data:{mime};base64,{data}"))
                })
                .collect();

            let run_result = {
                let mut agent = session.lock().await;
                agent
                    .run_turn_with_images(&turn_content, &image_data_urls, "grpc-chat")
                    .await
            };

            let turn_result = match run_result {
                Ok(result) => result,
                Err(err) => {
                    if err.downcast_ref::<mcp::RequiredMcpServersError>().is_some() {
                        let _ = tx
                            .send(Ok(ChatEvent {
                                payload: Some(proto::chat_event::Payload::Error(err.to_string())),
                            }))
                            .await;
                        let _ = tx
                            .send(Ok(ChatEvent {
                                payload: Some(proto::chat_event::Payload::Done(true)),
                            }))
                            .await;
                        cleanup().await;
                        return;
                    }
                    let _ = tx.send(Err(Status::internal(err.to_string()))).await;
                    cleanup().await;
                    return;
                }
            };

            let system_prompt = match turn_result {
                TurnResult::Continue { system_prompt, .. } => system_prompt,
                TurnResult::BudgetExhausted => {
                    let _ = tx
                        .send(Ok(ChatEvent {
                            payload: Some(proto::chat_event::Payload::Error(
                                "对话轮次预算已用尽".to_string(),
                            )),
                        }))
                        .await;
                    cleanup().await;
                    return;
                }
                TurnResult::Finished(message) => {
                    let _ = tx
                        .send(Ok(ChatEvent {
                            payload: Some(proto::chat_event::Payload::Token(message)),
                        }))
                        .await;
                    let _ = tx
                        .send(Ok(ChatEvent {
                            payload: Some(proto::chat_event::Payload::Done(true)),
                        }))
                        .await;
                    spawn_review_to_hub(&session, &sid_cleanup, &session_events_hub).await;
                    spawn_title_to_hub(&session, &session_events_hub).await;
                    cleanup().await;
                    return;
                }
                TurnResult::MaxDepth => {
                    let _ = tx
                        .send(Ok(ChatEvent {
                            payload: Some(proto::chat_event::Payload::Error(
                                "工具调用轮次已达上限".to_string(),
                            )),
                        }))
                        .await;
                    cleanup().await;
                    return;
                }
                TurnResult::ToolCalls(_) => {
                    let _ = tx
                        .send(Ok(ChatEvent {
                            payload: Some(proto::chat_event::Payload::Error(
                                "当前 gRPC Chat 尚未支持该轮次结果".to_string(),
                            )),
                        }))
                        .await;
                    cleanup().await;
                    return;
                }
                TurnResult::Interrupted => {
                    let _ = tx
                        .send(Ok(ChatEvent {
                            payload: Some(proto::chat_event::Payload::Error(
                                "上一轮对话已中断，请重新发送消息".to_string(),
                            )),
                        }))
                        .await;
                    cleanup().await;
                    return;
                }
            };

            let (temperature, additional_params) = {
                let agent = session.lock().await;
                (agent.temperature(), agent.additional_params().clone())
            };

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
                // 优先用当前模型元数据的 max_output_tokens；未知(0)时回退 8192。
                // 默认 4096 对会写文件的 agent 偏低，单次写大文件时 tool-call 参数
                // JSON 易被截断（EOF 解析失败）。
                max_tokens: if chat_max_output_tokens > 0 {
                    chat_max_output_tokens
                } else {
                    8192
                },
                ..ProviderConfig::default()
            };

            let hitl_gate = HitlGate::new(sid_cleanup.clone());
            hitl_registry.insert(hitl_gate.clone()).await;

            // primary（ChatRequest 字段）+ chat_fallbacks → Vec<ChatTarget>
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
            {
                let mut agent = session.lock().await;
                agent.set_chat_targets(chat_targets.clone());
            }

            let session_for_review = session.clone();
            let agent_id_for_events = {
                let agent = session.lock().await;
                agent.agent_id().to_string()
            };
            let mut stream = stream_multi_turn_with_hitl(
                session,
                chat_targets,
                config,
                system_prompt,
                pause,
                Some(hitl_gate),
            );
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
                            if outcome_type == "hitl_waiting" || outcome_type == "interrupt" {
                                let interrupts: Vec<agent::Interrupt> =
                                    serde_json::from_str(interrupts_json).unwrap_or_default();
                                if !interrupts.is_empty() {
                                    let _ =
                                        save_interrupt_file(&memory_dir, &sid_cleanup, &interrupts);
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
                            // 回合成功后 fire-and-forget review / 标题 → SessionEventHub（不阻塞 Chat 流）
                            spawn_review_to_hub(
                                &session_for_review,
                                &sid_cleanup,
                                &session_events_hub,
                            )
                            .await;
                            spawn_title_to_hub(&session_for_review, &session_events_hub).await;
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
            clear_interrupt_file(&memory_dir, &sid_cleanup);
            hook_runtime.fire_gateway(
                ::hooks::AGENT_END,
                &::hooks::HookPayload {
                    session_id: sid_cleanup.clone(),
                    turn_id: None,
                    ..Default::default()
                },
            );
            cleanup().await;
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
            let agent = handle.lock().await;
            let hub = agent.mcp_hub();
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
            .map(|c| proto::McpServerInfo {
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
            let mut agent = handle.lock().await;
            if agent.agent_id() != agent_id {
                continue;
            }
            if let Err(error) = agent.reconnect_mcp_server(server_id).await {
                if error
                    .downcast_ref::<mcp::RequiredMcpServersError>()
                    .is_none()
                {
                    return Err(Status::failed_precondition(error.to_string()));
                }
            }
            let hub = agent.mcp_hub();
            drop(agent);
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
        let (tx, rx) = tokio::sync::mpsc::channel(16);
        tokio::spawn(async move {
            let mut filtered = hub.subscribe(filter);
            while let Some(ev) = filtered.recv().await {
                if tx.send(Ok(to_proto(&ev))).await.is_err() {
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

    use std::sync::atomic::{AtomicUsize, Ordering};

    use tempfile::TempDir;

    #[tokio::test]
    async fn release_session_runtime_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let session_id = "release-session-runtime-idempotent";

        service.get_session(session_id).await.unwrap();
        let pause = service.register_pause(session_id).await;
        let gate = HitlGate::new(session_id.to_string());
        service.hitl_registry.insert(gate.clone()).await;

        let request = Request::new(ChatControlRequest {
            session_id: session_id.to_string(),
            action: ChatControlAction::ReleaseSession as i32,
        });
        service.chat_control(request).await.expect("first release");

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
        drop(pause);
        drop(gate);
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
