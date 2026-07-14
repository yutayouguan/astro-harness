//! Astro gRPC [`AstroService`] 实现：聊天流、会话、记忆、MCP、技能与文件列表。

use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use agent::builder::AgentBuilder;
use agent::hooks::ChannelHooks;
use agent::loop_::{AgentLoop, TurnResult};
use agent::streaming::{
    stream_multi_turn_with_hitl, MultiTurnStreamItem, StreamedAssistantContent,
};
use agent::{HitlGate, HitlRegistry};
use futures::StreamExt;
use memory::{AgentRuntimeConfig, MemoryManager};
use proto::astro_service_server::AstroService;
use proto::{
    ChatControlAction, ChatControlRequest, ChatEvent, ChatRequest, Empty, FileListRequest,
    FileListResponse, ImageEvent, ImageRequest, MemoryQuery, MemoryResult, McpServerList,
    SessionSnippet as ProtoSessionSnippet, SkillEvent, SkillList, SkillRequest, SkillInfo,
    UsageEvent,
};
use providers::registry::ProviderRegistry;
use providers::trait_::ProviderConfig;
use providers::PauseControl;
use tokio::sync::{Mutex, RwLock};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use super::interrupt_store::{
    clear_interrupt_file, resume_items_from_proto, save_interrupt_file,
};

/// 会话 Agent 循环的共享句柄。
type SessionHandle = Arc<Mutex<AgentLoop>>;
/// Chat RPC 返回的事件流类型别名。
type ChatStream = Pin<Box<dyn futures::Stream<Item = Result<ChatEvent, Status>> + Send>>;

/// Astro gRPC 服务实现：会话 Agent、流式聊天、记忆与技能等 RPC。
pub struct AstroServiceImpl {
    /// session_id → Agent 循环句柄。
    sessions: Arc<RwLock<HashMap<String, SessionHandle>>>,
    /// session_id → 暂停控制器。
    pause_controls: Arc<RwLock<HashMap<String, Arc<PauseControl>>>>,
    /// session_id → 活 HITL 闸门。
    hitl_registry: HitlRegistry,
    /// 模型供应商注册表。
    providers: Arc<ProviderRegistry>,
    /// 记忆根目录。
    memory_dir: PathBuf,
    /// Plugin / Gateway / Shell 钩子运行时。
    hook_runtime: Arc<::hooks::HookRuntime>,
}

impl AstroServiceImpl {
    /// 使用给定记忆目录创建服务（空会话表）。
    pub fn new(memory_dir: PathBuf) -> Self {
        let hook_runtime = match ::hooks::HookRuntime::bootstrap_from_root(&memory_dir) {
            Ok(rt) => {
                rt.fire_gateway(
                    ::hooks::GATEWAY_STARTUP,
                    &::hooks::HookPayload {
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
            providers: Arc::new(ProviderRegistry::new()),
            memory_dir,
            hook_runtime,
        }
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

        let agent_id = memory::active_agent_id(&self.memory_dir);
        let mut builder = AgentBuilder::new(self.memory_dir.clone()).agent_id(agent_id.clone());
        if let Ok(rt) = AgentRuntimeConfig::load(&self.memory_dir, &agent_id) {
            builder = builder.from_runtime_config(&rt);
        }
        let (agent, _, _) = builder
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

    /// UI「新建对话」：Gateway `command:new_chat` + Plugin reset/finalize，并卸内存会话。
    async fn release_session_for_new_chat(&self, session_id: &str) {
        let payload = ::hooks::HookPayload {
            session_id: session_id.to_string(),
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
        if let Some(handle) = removed {
            let agent = handle.lock().await;
            agent.cancel_signal().cancel();
            let hooks = agent.prompt_hooks();
            let cancel = agent.cancel_signal();
            hooks.on_session_reset(session_id, &cancel).await;
            hooks.on_session_finalize(session_id, &cancel).await;
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
fn multi_turn_to_chat_event(item: MultiTurnStreamItem) -> Option<ChatEvent> {
    match item {
        MultiTurnStreamItem::Assistant(StreamedAssistantContent::Text(token)) => Some(ChatEvent {
            payload: Some(proto::chat_event::Payload::Token(token)),
        }),
        MultiTurnStreamItem::Assistant(StreamedAssistantContent::Reasoning(r)) => Some(ChatEvent {
            payload: Some(proto::chat_event::Payload::Reasoning(r)),
        }),
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
        MultiTurnStreamItem::Assistant(StreamedAssistantContent::FinalUsage(u)) => Some(ChatEvent {
            payload: Some(proto::chat_event::Payload::Usage(UsageEvent {
                prompt_tokens: u.prompt_tokens(),
                completion_tokens: u.completion_tokens(),
                total_tokens: u.total_tokens(),
            })),
        }),
        MultiTurnStreamItem::ToolResult {
            id,
            name,
            arguments_json,
            result,
        } => Some(ChatEvent {
            payload: Some(proto::chat_event::Payload::ToolCall(proto::ToolCallEvent {
                id,
                name,
                arguments_json,
                result,
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
    type GenerateImageStream = Pin<
        Box<dyn futures::Stream<Item = Result<ImageEvent, Status>> + Send>,
    >;
    /// [`execute_skill`](Self::execute_skill) 流类型。
    type ExecuteSkillStream =
        Pin<Box<dyn futures::Stream<Item = Result<SkillEvent, Status>> + Send>>;

    /// Chat 流控制：暂停 / 继续 / 取消指定 `session_id` 的进行中对话。
    ///
    /// Cancel 会同时触发 [`PauseControl::cancel`] 与 Agent 的 [`CancelSignal`]。
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
        let action = ChatControlAction::try_from(req.action).unwrap_or_default();

        // 新建对话不依赖进行中的流；无内存会话时仍触发 Gateway 事件。
        if matches!(action, ChatControlAction::ChatControlNewChat) {
            self.release_session_for_new_chat(&req.session_id).await;
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
            ChatControlAction::ChatControlResume
            | ChatControlAction::ChatControlStreamResume => pause.resume(),
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
        clear_interrupt_file(&self.memory_dir, &req.session_id);
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
        let thinking_enabled = req.thinking_enabled;
        let reasoning_effort = if req.reasoning_effort.trim().is_empty() {
            "high".to_string()
        } else {
            req.reasoning_effort
        };
        let image_targets = agent::loop_::ImageGenTargets::from_parts(
            &req.image_gen_provider,
            &req.image_gen_model,
            &req.image_gen_api_key,
            &req.image_gen_base_url,
            &req.image_gen_fallback_provider,
            &req.image_gen_fallback_model,
            &req.image_gen_fallback_api_key,
            &req.image_gen_fallback_base_url,
        );

        let session = self.get_session(&session_id).await?;
        if is_new_session {
            self.hook_runtime.fire_gateway(
                ::hooks::SESSION_START,
                &::hooks::HookPayload {
                    session_id: session_id.clone(),
                    ..Default::default()
                },
            );
        }
        let (hook_tx, mut hook_rx) = tokio::sync::mpsc::unbounded_channel();
        {
            let mut agent = session.lock().await;
            agent.set_image_gen_targets(image_targets);
            agent.set_chat_credentials(&provider_name, &model, &api_key, &base_url);
            agent.set_hooks(Arc::new(ChannelHooks::new(hook_tx)));
            agent.set_hook_bus(Arc::clone(&self.hook_runtime.plugin));
        }
        let providers = self.providers.clone();

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

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<ChatEvent, Status>>(8);
        let hook_out = tx.clone();
        tokio::spawn(async move {
            while let Some(ev) = hook_rx.recv().await {
                let _ = hook_out
                    .send(Ok(ChatEvent {
                        payload: Some(proto::chat_event::Payload::Hook(proto::HookEvent {
                            name: ev.kind,
                            detail: ev.detail,
                            outcome: ev.outcome,
                        })),
                    }))
                    .await;
            }
        });

        tokio::spawn(async move {
            let cleanup = || async {
                let mut map = pause_controls.write().await;
                map.remove(&sid_cleanup);
                hitl_registry.cancel_and_remove(&sid_cleanup).await;
            };

            let turn_content = content;

            let run_result = {
                let mut agent = session.lock().await;
                agent.run_turn(&turn_content, "grpc-chat").await
            };

            let turn_result = match run_result {
                Ok(result) => result,
                Err(err) => {
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
                TurnResult::ToolCalls(_) | TurnResult::Interrupted => {
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
            };

            let provider = match providers.get(&provider_name) {
                Some(provider) => provider,
                None => {
                    let _ = tx
                        .send(Ok(ChatEvent {
                            payload: Some(proto::chat_event::Payload::Error(format!(
                                "未知 Provider: {provider_name}"
                            ))),
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
                    provider.default_model().to_string()
                } else {
                    model.clone()
                },
                api_key: {
                    let from_req = api_key.trim().to_string();
                    if !from_req.is_empty() {
                        from_req
                    } else {
                        providers::client::read_env_api_key(&provider_name).unwrap_or_default()
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
                ..ProviderConfig::default()
            };

            let hitl_gate = HitlGate::new(sid_cleanup.clone());
            hitl_registry.insert(hitl_gate.clone()).await;

            // Task 3：单元素链（Task 4 再接入 chat_fallbacks proto）
            let chat_targets = vec![common::ChatTarget {
                provider_id: String::new(),
                backend_id: provider_name.clone(),
                model: config.model.clone(),
                api_key: config.api_key.clone(),
                base_url: config.base_url.clone().unwrap_or_default(),
            }];

            let mut stream = stream_multi_turn_with_hitl(
                session,
                chat_targets,
                providers,
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
                                    let _ = save_interrupt_file(
                                        &memory_dir,
                                        &sid_cleanup,
                                        &interrupts,
                                    );
                                }
                            }
                        }
                        if let Some(event) = multi_turn_to_chat_event(mt) {
                            if tx.send(Ok(event)).await.is_err() {
                                break;
                            }
                        }
                        if is_done {
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
        let providers = self.providers.clone();

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<ImageEvent, Status>>(8);

        tokio::spawn(async move {
            let _ = tx
                .send(Ok(ImageEvent {
                    payload: Some(proto::image_event::Payload::Progress(
                        format!("正在使用 {provider_name} 生成图片…"),
                    )),
                }))
                .await;

            let provider = match providers.get(&provider_name) {
                Some(p) => p,
                None => {
                    let _ = tx
                        .send(Ok(ImageEvent {
                            payload: Some(proto::image_event::Payload::Error(format!(
                                "未知 Provider: {provider_name}"
                            ))),
                        }))
                        .await;
                    return;
                }
            };

            if !provider.supports_image_gen() {
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
                        providers::client::read_env_api_key(&provider_name).unwrap_or_default()
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

            match provider.generate_image(&prompt, &config).await {
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
    async fn list_skills(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<SkillList>, Status> {
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
            if !memory::is_toolset_enabled("skills") {
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
                            payload: Some(proto::skill_event::Payload::Error(
                                err.to_string(),
                            )),
                        }))
                        .await;
                }
                Err(err) => {
                    let _ = tx
                        .send(Ok(SkillEvent {
                            payload: Some(proto::skill_event::Payload::Error(
                                err.to_string(),
                            )),
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
        _request: Request<Empty>,
    ) -> Result<Response<McpServerList>, Status> {
        // 优先：active agent 的 hub（实时连接状态）；勿用任意会话以免串 agent
        let active = memory::active_agent_id(&self.memory_dir);
        let sessions = self.sessions.read().await;
        for handle in sessions.values() {
            let agent = handle.lock().await;
            if agent.mcp_hub().agent_id() != Some(active.as_str()) {
                continue;
            }
            let servers = agent
                .mcp_hub()
                .server_status()
                .into_iter()
                .map(|s| {
                    let status = match s.error {
                        Some(err) if !err.is_empty() => format!("{}: {}", s.status, err),
                        _ => s.status,
                    };
                    proto::McpServerInfo {
                        name: s.name,
                        status,
                        tools: s.tools,
                    }
                })
                .collect();
            return Ok(Response::new(McpServerList { servers }));
        }
        drop(sessions);

        let configs = mcp::load_mcp_servers(Some(&active)).unwrap_or_default();
        let servers = configs
            .into_iter()
            .map(|c| proto::McpServerInfo {
                name: c.name,
                status: if c.enabled {
                    "configured".into()
                } else {
                    "disabled".into()
                },
                tools: c.discovered.iter().map(|d| d.name.clone()).collect(),
            })
            .collect();
        Ok(Response::new(McpServerList { servers }))
    }

    /// 召回 MEMORY.md / USER.md 文本，并按 `query` 搜索历史消息（`limit` 至少 1）。
    ///
    /// # 错误
    /// MemoryManager 或 session_store 失败 → `internal`。
    async fn query_memory(
        &self,
        request: Request<MemoryQuery>,
    ) -> Result<Response<MemoryResult>, Status> {
        let query = request.into_inner();
        let memory = MemoryManager::new(self.memory_dir.clone())
            .map_err(|e| Status::internal(e.to_string()))?;
        let (memory_content, user_content) = memory.prompt_content();

        let sessions = memory
            .session_store
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
        let entries = crate::grpc::files::list_directory(
            &self.memory_dir,
            &req.path,
            req.depth,
        )
        .map_err(|e| Status::invalid_argument(e.to_string()))?;

        Ok(Response::new(FileListResponse { entries }))
    }
}
