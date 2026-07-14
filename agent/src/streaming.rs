//! Rig 风格流式分层与多轮工具循环。
//!
//! 本模块将 Provider 原始 chunk 流映射为 Agent 语义事件，并实现
//! `StreamingCompletion` / `StreamingChat` / `StreamingPrompt` 三层 trait，
//! 最终在 [`run_multi_turn_stream`] 中驱动「LLM 流式 → 工具执行 → 再请求」闭环。
//!
//! **关键不变量**
//! - Pause/Cancel 对齐 Rig：`wait_if_paused` 先于上游 poll；取消时通过 `Abortable` 中止 Provider 流
//! - 每轮 assistant 回复必须写入 `session_messages`（含 tool_calls）后再执行工具
//! - 末轮仍含工具调用时以 Error 结束，避免静默 `Done`
//! - usage 采用覆盖式累加，兼容 Google 等 Provider 的累计式 `usageMetadata`

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use common::message::Message;
use common::ChatTarget;
use futures::stream::{AbortHandle, Abortable};
use futures::{Stream, StreamExt};
use providers::registry::ProviderRegistry;
use providers::streaming::{PauseControl, Usage};
use providers::trait_::{
    AiProvider, ChatChunk, ChatMessage as ProviderMessage, ChatStream, ProviderConfig,
    ToolCallDeltaChunk,
};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinSet;

use crate::chat_fallback::{try_stream_completion_with_fallback, ActiveTargetMeta};
use crate::hitl::{is_exclusive_tool, is_interactive_tool, HitlGate, HITL_DEFAULT_TIMEOUT_SECS};
use crate::interrupt::Interrupt;
use crate::loop_::AgentLoop;
use crate::usage_record::apply_llm_usage_dual_write;

tokio::task_local! {
    /// 同步 `delegate` 子路径上浮 HITL 时读取；由串行工具执行注入。
    static PARENT_HITL_CTX: Option<ParentHitlCtx>;
}

/// 父会话 HITL 桥：子 Agent park 时复用同一 gate 与流。
#[derive(Clone)]
pub(crate) struct ParentHitlCtx {
    pub gate: Arc<HitlGate>,
    pub tx: mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    pub run_id: String,
}

/// 进行中聊天流的父 HITL 表（供 `delegate_async` 子任务上浮）。
static LIVE_PARENT_HITL: OnceLock<tokio::sync::RwLock<HashMap<String, ParentHitlCtx>>> =
    OnceLock::new();

fn live_parent_hitl_map() -> &'static tokio::sync::RwLock<HashMap<String, ParentHitlCtx>> {
    LIVE_PARENT_HITL.get_or_init(|| tokio::sync::RwLock::new(HashMap::new()))
}

pub(crate) async fn register_live_parent_hitl(session_id: &str, ctx: ParentHitlCtx) {
    live_parent_hitl_map()
        .write()
        .await
        .insert(session_id.to_string(), ctx);
}

pub(crate) async fn unregister_live_parent_hitl(session_id: &str) {
    live_parent_hitl_map().write().await.remove(session_id);
}

fn decorate_delegate_hitl(mut hitl: AstroHitlPayload, async_child: bool) -> AstroHitlPayload {
    let tag = if async_child {
        "[delegate_async]"
    } else {
        "[delegate]"
    };
    if !hitl.reason.starts_with("[delegate") {
        hitl.reason = format!("{tag} {}", hitl.reason);
    }
    if hitl.message.is_empty() {
        hitl.message = format!("{tag} Sub-agent needs your input");
    } else if !hitl.message.starts_with("[delegate") {
        hitl.message = format!("{tag} {}", hitl.message);
    }
    hitl
}

/// 子 Agent 若有父 HITL 上下文则 park 并返回 tool result；否则 `None`。
///
/// 查找顺序：task_local（同步 delegate）→ live 会话表（async，父流仍在）。
pub(crate) async fn try_park_parent_hitl(
    tool_call_id: &str,
    hitl: AstroHitlPayload,
    parent_session_id: Option<&str>,
) -> Option<String> {
    if let Some(ctx) = PARENT_HITL_CTX.try_with(|c| c.clone()).ok().flatten() {
        let hitl = decorate_delegate_hitl(hitl, false);
        return park_astro_hitl(&ctx.gate, &ctx.tx, &ctx.run_id, tool_call_id, hitl).await;
    }
    if let Some(sid) = parent_session_id {
        if let Some(ctx) = live_parent_hitl_map().read().await.get(sid).cloned() {
            let hitl = decorate_delegate_hitl(hitl, true);
            return park_astro_hitl(&ctx.gate, &ctx.tx, &ctx.run_id, tool_call_id, hitl).await;
        }
    }
    None
}

/// 单次模型流式片段，对齐 Rig `StreamedAssistantContent` 并扩展 Reasoning 通道。
#[derive(Debug, Clone)]
pub enum StreamedAssistantContent {
    /// 可见 assistant 文本 token。
    Text(String),
    /// 推理/思考过程 token（部分 Provider 专用）。
    Reasoning(String),
    /// 工具调用的增量片段，需经 [`tools::ToolCallAccumulator`] 合并。
    ToolCallDelta(ToolCallDeltaChunk),
    /// 本轮或累计 token 用量，通常在流末尾出现。
    FinalUsage(Usage),
}

/// 多轮 Agent 流式事件，在 assistant 片段之上扩展工具结果与产品语义。
#[derive(Debug, Clone)]
pub enum MultiTurnStreamItem {
    /// 模型输出片段（文本、推理、工具 delta、usage）。
    Assistant(StreamedAssistantContent),
    /// 单次工具调用完成后的结果，供 UI 展示。
    ToolResult {
        /// 与 assistant tool_call 对应的 id。
        id: String,
        /// 工具 qualified name。
        name: String,
        /// 原始 arguments JSON 字符串。
        arguments_json: String,
        /// 工具返回文本（含错误前缀时仍原样传递）。
        result: String,
    },
    /// 记忆工具成功变更，供右侧时间线展示。
    MemoryUpdate {
        /// 操作类型：`memory_add` / `memory_replace` / `memory_remove`。
        op: String,
        /// 结果预览（最长 240 字符）。
        content: String,
    },
    /// AG-UI `RUN_STARTED`：一次用户发送对应一个 run。
    RunStarted {
        thread_id: String,
        run_id: String,
    },
    /// AG-UI `ACTIVITY_SNAPSHOT`（如 A2UI surface）。
    Activity {
        message_id: String,
        activity_type: String,
        content_json: String,
        replace: bool,
    },
    /// AG-UI `RUN_FINISHED`：`outcome_type` 为 `success`、`interrupt` 或 `hitl_waiting`。
    /// `hitl_waiting`：同回合阻塞 HITL，流不随后发 Done。
    RunFinished {
        run_id: String,
        outcome_type: String,
        /// JSON array of Interrupt；success 时为空数组 `[]`。
        interrupts_json: String,
    },
    /// 不可恢复错误，之后必跟 `Done`。
    Error(String),
    /// 流正常或异常结束标记。
    Done,
}

/// Provider 层 assistant 内容流：每项为 `StreamedAssistantContent` 或错误。
pub type AssistantContentStream =
    Pin<Box<dyn Stream<Item = anyhow::Result<StreamedAssistantContent>> + Send>>;

/// 多轮 Agent 事件流：由 [`stream_multi_turn`] 暴露给 gRPC / UI 消费。
pub type MultiTurnStream =
    Pin<Box<dyn Stream<Item = anyhow::Result<MultiTurnStreamItem>> + Send>>;

/// 将单个 Provider [`ChatChunk`] 拆分为零或多个 [`StreamedAssistantContent`]。
///
/// 空字段跳过；同一 chunk 可同时产出文本、delta 与 usage。
fn chunk_to_contents(chunk: ChatChunk) -> Vec<StreamedAssistantContent> {
    let mut out = Vec::new();
    if let Some(reasoning) = chunk.reasoning {
        if !reasoning.is_empty() {
            out.push(StreamedAssistantContent::Reasoning(reasoning));
        }
    }
    if let Some(token) = chunk.token {
        if !token.is_empty() {
            out.push(StreamedAssistantContent::Text(token));
        }
    }
    for d in chunk.tool_call_deltas {
        out.push(StreamedAssistantContent::ToolCallDelta(d));
    }
    if let Some(usage) = chunk.usage {
        out.push(StreamedAssistantContent::FinalUsage(usage));
    }
    out
}

/// 将会话消息转为 Provider 消息序列。
///
/// 实现见 [`crate::messages::to_provider_messages`]（缺 `tool_call_id` 的 tool 消息会跳过）。
pub use crate::messages::to_provider_messages;

/// 将 Provider 原始 [`ChatStream`] 映射为 [`AssistantContentStream`]。
///
/// `finish_reason` 以 `error:` 前缀开头时转为 `Err` 并终止该 chunk 的展开。
fn map_provider_stream(stream: ChatStream) -> AssistantContentStream {
    Box::pin(stream.flat_map(|item| {
        let contents: Vec<anyhow::Result<StreamedAssistantContent>> = match item {
            Ok(chunk) => {
                if let Some(fr) = chunk.finish_reason.as_deref() {
                    if fr.starts_with("error:") {
                        return futures::stream::iter(vec![Err(anyhow::anyhow!(
                            "{}",
                            fr.trim_start_matches("error:")
                        ))]);
                    }
                }
                chunk_to_contents(chunk).into_iter().map(Ok).collect()
            }
            Err(err) => vec![Err(err)],
        };
        futures::stream::iter(contents)
    }))
}

/// 底层流式 completion：直接接收 Provider 格式消息列表。
#[async_trait]
pub trait StreamingCompletion: Send + Sync {
    /// 对给定 messages 与 tools schema 发起流式 completion。
    async fn stream_completion(
        &self,
        messages: Vec<ProviderMessage>,
        tools: Vec<serde_json::Value>,
    ) -> anyhow::Result<AssistantContentStream>;
}

/// 带 Astro 会话历史的流式 chat 抽象。
#[async_trait]
pub trait StreamingChat: Send + Sync {
    /// 将 system prompt 与会话历史转换为 Provider 消息后流式请求。
    async fn stream_chat(
        &self,
        system_prompt: &str,
        history: &[Message],
        tools: Vec<serde_json::Value>,
    ) -> anyhow::Result<AssistantContentStream>;
}

/// 无历史的一次性流式 prompt 抽象。
#[async_trait]
pub trait StreamingPrompt: Send + Sync {
    /// 将单条 user prompt 包装为历史后流式请求。
    async fn stream_prompt(
        &self,
        system_prompt: &str,
        prompt: &str,
        tools: Vec<serde_json::Value>,
    ) -> anyhow::Result<AssistantContentStream>;
}

/// 包装 [`ProviderRegistry`] + fallback 链，实现三层 Streaming trait。
pub struct ProviderStreamer {
    /// 按 `backend_id` 解析具体 [`AiProvider`]。
    pub registry: Arc<ProviderRegistry>,
    /// 含 primary 的聊天目标链（失败切模仅用此列表，不改会话默认凭据）。
    pub targets: Vec<ChatTarget>,
    /// temperature / thinking 等；model/key/url 由每跳 target 覆盖。
    pub base_config: ProviderConfig,
    /// 最近一次成功补全命中的目标元数据（供 usage 记录）。
    last_hit: StdMutex<Option<ActiveTargetMeta>>,
}

impl ProviderStreamer {
    pub fn new(
        registry: Arc<ProviderRegistry>,
        targets: Vec<ChatTarget>,
        base_config: ProviderConfig,
    ) -> Self {
        Self {
            registry,
            targets,
            base_config,
            last_hit: StdMutex::new(None),
        }
    }

    /// 最近一次成功 stream 的命中元数据。
    pub fn last_hit_meta(&self) -> Option<ActiveTargetMeta> {
        self.last_hit.lock().ok().and_then(|g| g.clone())
    }

    fn api_key_for(&self, meta: &ActiveTargetMeta) -> String {
        self.targets
            .iter()
            .find(|t| {
                t.backend_id == meta.backend_id
                    && t.model == meta.model
                    && (meta.provider_id.is_empty() || t.provider_id == meta.provider_id)
            })
            .map(|t| t.api_key.clone())
            .unwrap_or_else(|| self.base_config.api_key.clone())
    }

    fn primary_model(&self) -> String {
        self.targets
            .first()
            .map(|t| t.model.clone())
            .unwrap_or_else(|| self.base_config.model.clone())
    }
}

/// 从单 Provider + config 构造单元素 fallback 链（旧调用方兼容）。
pub fn chat_target_from_provider_config(
    provider: &dyn AiProvider,
    config: &ProviderConfig,
) -> ChatTarget {
    let backend_id = provider.name().to_string();
    ChatTarget {
        provider_id: backend_id.clone(),
        backend_id,
        model: config.model.clone(),
        api_key: config.api_key.clone(),
        base_url: config.base_url.clone().unwrap_or_default(),
    }
}

/// 将自定义/测试 Provider 注入注册表，并返回单元素 `targets`。
pub fn targets_and_registry_from_primary(
    provider: Arc<dyn AiProvider>,
    config: &ProviderConfig,
) -> (Vec<ChatTarget>, Arc<ProviderRegistry>) {
    let target = chat_target_from_provider_config(provider.as_ref(), config);
    let mut registry = ProviderRegistry::new();
    registry.insert(target.backend_id.clone(), provider);
    (vec![target], Arc::new(registry))
}

#[async_trait]
impl StreamingCompletion for ProviderStreamer {
    /// 经 [`try_stream_completion_with_fallback`] 再 [`map_provider_stream`] 归一化。
    async fn stream_completion(
        &self,
        messages: Vec<ProviderMessage>,
        tools: Vec<serde_json::Value>,
    ) -> anyhow::Result<AssistantContentStream> {
        let (stream, meta) = try_stream_completion_with_fallback(
            &self.targets,
            self.registry.as_ref(),
            messages,
            tools,
            &self.base_config,
            |from, to, err| {
                tracing::warn!(
                    from_backend = %from.backend_id,
                    from_model = %from.model,
                    to_backend = %to.backend_id,
                    to_model = %to.model,
                    error = %err,
                    "chat failover: switching target before first content"
                );
            },
        )
        .await?;
        if let Ok(mut guard) = self.last_hit.lock() {
            *guard = Some(meta);
        }
        Ok(map_provider_stream(stream))
    }
}

#[async_trait]
impl StreamingChat for ProviderStreamer {
    /// 通过 [`to_provider_messages`] 转换历史后调用 `stream_completion`。
    async fn stream_chat(
        &self,
        system_prompt: &str,
        history: &[Message],
        tools: Vec<serde_json::Value>,
    ) -> anyhow::Result<AssistantContentStream> {
        let messages = to_provider_messages(system_prompt, history);
        self.stream_completion(messages, tools).await
    }
}

#[async_trait]
impl StreamingPrompt for ProviderStreamer {
    /// 构造单条 user 历史后委托 `stream_chat`。
    async fn stream_prompt(
        &self,
        system_prompt: &str,
        prompt: &str,
        tools: Vec<serde_json::Value>,
    ) -> anyhow::Result<AssistantContentStream> {
        let history = vec![Message::user(prompt)];
        self.stream_chat(system_prompt, &history, tools).await
    }
}

/// 当 Agent 配置 `multi_turn == 0` 时使用的默认工具轮次上限。
const DEFAULT_MAX_ROUNDS: usize = 8;

/// 向 mpsc 发送单个成功事件；接收方关闭时返回 `false`。
async fn emit(
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    item: MultiTurnStreamItem,
) -> bool {
    tx.send(Ok(item)).await.is_ok()
}

/// 尽力双写 `kind=llm` 事件与会话账单；失败忽略。
/// `meta` 优先使用本轮实际命中目标；缺省时回退到 AgentLoop 上的会话凭据（不应在 failover 时写入）。
async fn record_llm_usage(
    session: &Arc<Mutex<AgentLoop>>,
    streamer: &ProviderStreamer,
    usage: &Usage,
) {
    if usage.is_empty() {
        return;
    }
    let agent = session.lock().await;
    let agent_id = agent.agent_id().to_string();
    let session_id = agent.session_id().to_string();
    let fallback_provider = agent.chat_provider().to_string();
    let fallback_base_url = agent.chat_base_url().to_string();
    let fallback_api_key = agent.chat_api_key().to_string();
    let fallback_model = agent.chat_model().to_string();
    drop(agent);

    let (model, provider, base_url, api_key) = if let Some(meta) = streamer.last_hit_meta() {
        let api_key = streamer.api_key_for(&meta);
        (meta.model, meta.backend_id, meta.base_url, api_key)
    } else {
        (
            if fallback_model.is_empty() {
                streamer.primary_model()
            } else {
                fallback_model
            },
            fallback_provider,
            fallback_base_url,
            fallback_api_key,
        )
    };

    apply_llm_usage_dual_write(
        &agent_id,
        Some(&session_id),
        &model,
        usage,
        &provider,
        &base_url,
        &api_key,
        None,
    );
}

/// 发送 Error 后立即发送 Done；若有已累计 usage 则先写入 `usage.db`。
async fn finish_error(
    session: &Arc<Mutex<AgentLoop>>,
    streamer: &ProviderStreamer,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    msg: impl Into<String>,
    usage: Option<Usage>,
) {
    if let Some(u) = usage.as_ref() {
        record_llm_usage(session, streamer, u).await;
    }
    let _ = emit(tx, MultiTurnStreamItem::Error(msg.into())).await;
    let _ = emit(tx, MultiTurnStreamItem::Done).await;
}

/// 仅发送 Done，表示正常结束。
async fn finish_done(tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>) {
    let _ = emit(tx, MultiTurnStreamItem::Done).await;
}

/// 发送 RunFinished(success) 后 Done。
async fn finish_success(
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    run_id: &str,
) {
    let _ = emit(
        tx,
        MultiTurnStreamItem::RunFinished {
            run_id: run_id.to_string(),
            outcome_type: "success".into(),
            interrupts_json: "[]".into(),
        },
    )
    .await;
    finish_done(tx).await;
}

/// 可选发送累计 usage 后发送 Done；若有 usage 则旁路写入 `usage.db`（kind=llm）。
async fn finish_usage_and_done(
    session: &Arc<Mutex<AgentLoop>>,
    streamer: &ProviderStreamer,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    usage: Option<Usage>,
    run_id: &str,
) {
    if let Some(u) = usage {
        record_llm_usage(session, streamer, &u).await;
        let _ = emit(
            tx,
            MultiTurnStreamItem::Assistant(StreamedAssistantContent::FinalUsage(u)),
        )
        .await;
    }
    finish_success(tx, run_id).await;
}

/// 多轮工具调用流式循环：从 gRPC handler 收拢到 Agent 层的核心编排。
///
/// 每轮：锁定 session → 流式 LLM → 累积 tool_calls → 执行工具 → 写入历史 → 下一轮。
/// 取消/暂停时清理 abort handle 并以 usage + Done 收尾。
/// `hitl_gate` 非空时，confirm/clarify/危险命令在同回合 park，不结束 run。
pub async fn run_multi_turn_stream(
    session: Arc<Mutex<AgentLoop>>,
    targets: Vec<ChatTarget>,
    registry: Arc<ProviderRegistry>,
    base_config: ProviderConfig,
    system_prompt: String,
    pause: Arc<PauseControl>,
    hitl_gate: Option<Arc<HitlGate>>,
    tx: mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
) {
    let session_id = {
        let agent = session.lock().await;
        agent.session_id().to_string()
    };
    let run_id = uuid::Uuid::new_v4().to_string();
    if let Some(ref gate) = hitl_gate {
        register_live_parent_hitl(
            &session_id,
            ParentHitlCtx {
                gate: gate.clone(),
                tx: tx.clone(),
                run_id: run_id.clone(),
            },
        )
        .await;
    }
    run_multi_turn_stream_inner(
        session,
        targets,
        registry,
        base_config,
        system_prompt,
        pause,
        hitl_gate,
        tx,
        session_id.clone(),
        run_id,
    )
    .await;
    unregister_live_parent_hitl(&session_id).await;
}

/// 旧签名兼容：单 Provider + config → 单元素链后走 fallback 路径。
pub async fn run_multi_turn_stream_from_provider(
    session: Arc<Mutex<AgentLoop>>,
    provider: Arc<dyn AiProvider>,
    config: ProviderConfig,
    system_prompt: String,
    pause: Arc<PauseControl>,
    hitl_gate: Option<Arc<HitlGate>>,
    tx: mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
) {
    let (targets, registry) = targets_and_registry_from_primary(provider, &config);
    run_multi_turn_stream(
        session,
        targets,
        registry,
        config,
        system_prompt,
        pause,
        hitl_gate,
        tx,
    )
    .await;
}

async fn run_multi_turn_stream_inner(
    session: Arc<Mutex<AgentLoop>>,
    targets: Vec<ChatTarget>,
    registry: Arc<ProviderRegistry>,
    base_config: ProviderConfig,
    system_prompt: String,
    pause: Arc<PauseControl>,
    hitl_gate: Option<Arc<HitlGate>>,
    tx: mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    thread_id: String,
    run_id: String,
) {
    let streamer = ProviderStreamer::new(registry, targets, base_config);
    let mut total_usage = Usage::default();
    let mut saw_usage = false;

    {
        let agent = session.lock().await;
        let _ = agent.ensure_session("tauri");
    }
    let _ = emit(
        &tx,
        MultiTurnStreamItem::RunStarted {
            thread_id,
            run_id: run_id.clone(),
        },
    )
    .await;

    let max_rounds = {
        let agent = session.lock().await;
        let n = agent.multi_turn();
        if n == 0 {
            DEFAULT_MAX_ROUNDS
        } else {
            n
        }
    };

    // 整次 run 累积时间线，供每轮 assistant 落盘写入 reasoning_details
    let mut timeline = crate::timeline::TimelineBuilder::new();
    let now_ms = || chrono::Utc::now().timestamp_millis();

    for round in 0..max_rounds {
        if pause.is_cancelled() {
            finish_usage_and_done(&session, &streamer, &tx, saw_usage.then_some(total_usage), &run_id).await;
            return;
        }
        if !pause.wait_if_paused().await {
            finish_usage_and_done(&session, &streamer, &tx, saw_usage.then_some(total_usage), &run_id).await;
            return;
        }

        let (history, tools) = {
            let mut agent = session.lock().await;
            agent.reload_tools_and_mcp().await;
            let mut messages = agent.session_messages.clone();
            if let Some(ctx) = agent.take_inject_context() {
                messages.push(common::message::Message::user(&format!(
                    "[astro:hook-context]\n{ctx}"
                )));
            }
            let tools = agent.tool_registry().schemas_for_api();
            (messages, tools)
        };

        {
            let (hooks, cancel) = {
                let agent = session.lock().await;
                (agent.prompt_hooks(), agent.cancel_signal())
            };
            hooks.pre_api_request(&cancel).await;
        }

        let raw_stream = match streamer
            .stream_chat(&system_prompt, &history, tools)
            .await
        {
            Ok(s) => {
                let (hooks, cancel) = {
                    let agent = session.lock().await;
                    (agent.prompt_hooks(), agent.cancel_signal())
                };
                hooks.post_api_request(None, &cancel).await;
                s
            }
            Err(err) => {
                let (hooks, cancel) = {
                    let agent = session.lock().await;
                    (agent.prompt_hooks(), agent.cancel_signal())
                };
                hooks
                    .post_api_request(Some(err.to_string().as_str()), &cancel)
                    .await;
                finish_error(
                    &session,
                    &streamer,
                    &tx,
                    err.to_string(),
                    saw_usage.then_some(total_usage),
                )
                .await;
                return;
            }
        };

        let (abort_handle, abort_reg) = AbortHandle::new_pair();
        pause.attach_abort(abort_handle);
        let mut stream = Abortable::new(raw_stream, abort_reg);

        let mut full_response = String::new();
        let mut full_reasoning = String::new();
        let mut tool_acc = tools::ToolCallAccumulator::new();
        // Google 等会在每个 chunk 带累计 usage：本轮覆盖式取最后一次
        let mut round_usage: Option<Usage> = None;

        loop {
            // Rig 语义：先确认未 pause，再 poll 上游
            if !pause.wait_if_paused().await {
                pause.clear_abort();
                finish_usage_and_done(
                    &session,
                    &streamer,
                    &tx,
                    {
                        if let Some(u) = round_usage {
                            total_usage.add_assign(u);
                            saw_usage = true;
                        }
                        saw_usage.then_some(total_usage)
                    },
                    &run_id,
                )
                .await;
                return;
            }

            let next = tokio::select! {
                biased;
                _ = pause.wait_cancelled() => {
                    pause.clear_abort();
                    finish_usage_and_done(
                    &session,
                    &streamer,
                        &tx,
                        {
                            if let Some(u) = round_usage {
                                total_usage.add_assign(u);
                                saw_usage = true;
                            }
                            saw_usage.then_some(total_usage)
                        },
                        &run_id,
                    )
                    .await;
                    return;
                }
                item = stream.next() => item,
            };

            match next {
                None => break, // 正常结束或 abort
                Some(Ok(StreamedAssistantContent::Text(text))) => {
                    full_response.push_str(&text);
                    if !emit(
                        &tx,
                        MultiTurnStreamItem::Assistant(StreamedAssistantContent::Text(text)),
                    )
                    .await
                    {
                        pause.clear_abort();
                        return;
                    }
                }
                Some(Ok(StreamedAssistantContent::Reasoning(r))) => {
                    full_reasoning.push_str(&r);
                    timeline.push_reasoning_delta(&r, now_ms());
                    if !emit(
                        &tx,
                        MultiTurnStreamItem::Assistant(StreamedAssistantContent::Reasoning(r)),
                    )
                    .await
                    {
                        pause.clear_abort();
                        return;
                    }
                }
                Some(Ok(StreamedAssistantContent::ToolCallDelta(d))) => {
                    tool_acc.push(&tools::ToolCallDelta {
                        index: d.index,
                        id: d.id.clone(),
                        name: d.name.clone(),
                        arguments: d.arguments.clone(),
                    });
                    if !emit(
                        &tx,
                        MultiTurnStreamItem::Assistant(StreamedAssistantContent::ToolCallDelta(d)),
                    )
                    .await
                    {
                        pause.clear_abort();
                        return;
                    }
                }
                Some(Ok(StreamedAssistantContent::FinalUsage(u))) => {
                    round_usage = Some(u); // 覆盖：兼容累计式 usageMetadata
                }
                Some(Err(err)) => {
                    pause.clear_abort();
                    if let Some(u) = round_usage {
                        total_usage.add_assign(u);
                        saw_usage = true;
                    }
                    finish_error(
                    &session,
                    &streamer,
                        &tx,
                        err.to_string(),
                        saw_usage.then_some(total_usage),
                    )
                    .await;
                    return;
                }
            }
        }

        pause.clear_abort();

        if pause.is_cancelled() {
            if let Some(u) = round_usage {
                total_usage.add_assign(u);
                saw_usage = true;
            }
            finish_usage_and_done(&session, &streamer, &tx, saw_usage.then_some(total_usage), &run_id).await;
            return;
        }

        if let Some(u) = round_usage {
            total_usage.add_assign(u);
            saw_usage = true;
        }

        let native_calls = tool_acc.finish();
        let calls = tools::resolve_tool_calls(native_calls, &full_response);

        if full_response.is_empty() && calls.is_empty() {
            finish_error(
                    &session,
                    &streamer,
                &tx,
                "模型返回了空回复。请重试，或换一个模型。",
                saw_usage.then_some(total_usage),
            )
            .await;
            return;
        }

        {
            let (hooks, cancel) = {
                let agent = session.lock().await;
                (agent.prompt_hooks(), agent.cancel_signal())
            };
            hooks.post_llm_call(&full_response, &cancel).await;
            if cancel.is_cancelled() {
                finish_usage_and_done(
                    &session,
                    &streamer,
                    &tx,
                    saw_usage.then_some(total_usage),
                    &run_id,
                )
                .await;
                return;
            }
        }

        {
            let mut agent = session.lock().await;
            // 原生与 XML 回退统一：assistant 带 tool_calls，tool 带 tool_call_id
            let tc = if calls.is_empty() {
                None
            } else {
                Some(
                    calls
                        .iter()
                        .map(|c| common::message::ToolCall {
                            id: c.id.clone(),
                            name: c.name.clone(),
                            arguments: c.arguments.clone(),
                        })
                        .collect(),
                )
            };
            for c in &calls {
                timeline.upsert_activity(&c.id, now_ms());
            }
            let details = Some(timeline.reasoning_details_snapshot());
            if let Err(err) = agent.record_assistant_message_with_tools(
                &full_response,
                tc,
                (!full_reasoning.is_empty()).then_some(full_reasoning.as_str()),
                details,
            ) {
                drop(agent);
                finish_error(
                    &session,
                    &streamer,
                    &tx,
                    err.to_string(),
                    saw_usage.then_some(total_usage),
                )
                .await;
                return;
            }
        }

        if calls.is_empty() {
            break;
        }

        // 最后一轮仍要调工具：执行后结束并报错，避免静默 Done
        let last_round = round + 1 >= max_rounds;

        let force_serial = calls.iter().any(|c| {
            is_interactive_tool(&c.name)
                || is_exclusive_tool(&c.name)
                || terminal_needs_approval(&c.name, &c.arguments)
        });

        let outcomes = if force_serial || hitl_gate.is_none() {
            execute_tools_serial(
                &session,
                &calls,
                &pause,
                &tx,
                &run_id,
                hitl_gate.as_ref(),
            )
            .await
        } else {
            execute_tools_concurrent(&session, &calls, &pause).await
        };

        let Some(outcomes) = outcomes else {
            finish_usage_and_done(
                    &session,
                    &streamer,
                &tx,
                saw_usage.then_some(total_usage),
                &run_id,
            )
            .await;
            return;
        };

        for (call, result) in calls.iter().zip(outcomes.into_iter()) {
            if pause.is_cancelled() {
                finish_usage_and_done(
                    &session,
                    &streamer,
                    &tx,
                    saw_usage.then_some(total_usage),
                    &run_id,
                )
                .await;
                return;
            }

            if !emit(
                &tx,
                MultiTurnStreamItem::ToolResult {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    arguments_json: call.arguments.to_string(),
                    result: result.clone(),
                },
            )
            .await
            {
                return;
            }

            if matches!(
                call.name.as_str(),
                "memory_add" | "memory_replace" | "memory_remove"
            ) && !result.starts_with("工具错误")
                && !result.starts_with("工具已禁用")
                && !result.starts_with("工具参数 JSON 解析失败")
            {
                let preview = {
                    let s = result.trim();
                    if s.chars().count() > 240 {
                        format!("{}…", s.chars().take(240).collect::<String>())
                    } else {
                        s.to_string()
                    }
                };
                if !emit(
                    &tx,
                    MultiTurnStreamItem::MemoryUpdate {
                        op: call.name.clone(),
                        content: preview,
                    },
                )
                .await
                {
                    return;
                }
            }

            let info_ui = parse_astro_ui(&result);
            let result_for_history = if let Some(ref ui) = info_ui {
                format!("Presented info card: {}", ui.summary)
            } else if parse_astro_hitl(&result).is_some() {
                // 串行路径已把 HITL park 结果写成非 astro_hitl；若仍是标记则兜底
                result.clone()
            } else {
                result.clone()
            };

            if let Some(ref ui) = info_ui {
                let message_id = format!("a2ui-surface-{}", call.id);
                let content_json =
                    serde_json::json!({ "operations": ui.operations }).to_string();
                timeline.upsert_surface(
                    serde_json::json!({
                        "messageId": message_id,
                        "activityType": "a2ui-surface",
                        "operations": ui.operations,
                        "status": "active",
                    }),
                    now_ms(),
                );
                if !emit(
                    &tx,
                    MultiTurnStreamItem::Activity {
                        message_id,
                        activity_type: "a2ui-surface".into(),
                        content_json,
                        replace: true,
                    },
                )
                .await
                {
                    return;
                }
            }

            {
                let mut agent = session.lock().await;
                let _ = agent.record_tool_result_with_id(
                    Some(&call.id),
                    Some(&call.name),
                    &result_for_history,
                );
            }
        }

        if last_round {
            finish_error(
                    &session,
                    &streamer,
                &tx,
                "工具调用轮次已用尽，请简化任务后重试。".to_string(),
                saw_usage.then_some(total_usage),
            )
            .await;
            return;
        }
    }

    {
        let (hooks, cancel, turn) = {
            let agent = session.lock().await;
            (agent.prompt_hooks(), agent.cancel_signal(), agent.session_turn())
        };
        hooks.on_session_end(turn, &cancel).await;
    }

    finish_usage_and_done(&session, &streamer, &tx, saw_usage.then_some(total_usage), &run_id).await;
}

fn terminal_needs_approval(name: &str, args: &serde_json::Value) -> bool {
    if name != "terminal" {
        return false;
    }
    let cmd = args
        .get("command")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    matches!(
        tools::classify_dangerous_command(cmd).map(|d| d.action),
        Some(tools::ApprovalAction::Ask)
    )
}

/// 串行执行；`None` 表示已处理 cancel/断开，调用方应直接 return。
async fn execute_tools_serial(
    session: &Arc<Mutex<AgentLoop>>,
    calls: &[tools::ParsedToolCall],
    pause: &Arc<PauseControl>,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    run_id: &str,
    hitl_gate: Option<&Arc<HitlGate>>,
) -> Option<Vec<String>> {
    let ctx = hitl_gate.map(|gate| ParentHitlCtx {
        gate: gate.clone(),
        tx: tx.clone(),
        run_id: run_id.to_string(),
    });
    PARENT_HITL_CTX
        .scope(ctx, execute_tools_serial_inner(session, calls, pause, tx, run_id, hitl_gate))
        .await
}

async fn execute_tools_serial_inner(
    session: &Arc<Mutex<AgentLoop>>,
    calls: &[tools::ParsedToolCall],
    pause: &Arc<PauseControl>,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    run_id: &str,
    hitl_gate: Option<&Arc<HitlGate>>,
) -> Option<Vec<String>> {
    let mut out = Vec::with_capacity(calls.len());
    for call in calls {
        if pause.is_cancelled() {
            return None;
        }
        if !pause.wait_if_paused().await {
            return None;
        }

        // 危险 terminal：deny / auto / ask
        if call.name == "terminal" && !call.args_parse_error {
            if let Some(decision) = call
                .arguments
                .get("command")
                .and_then(|v| v.as_str())
                .and_then(tools::classify_dangerous_command)
            {
                match decision.action {
                    tools::ApprovalAction::Deny => {
                        out.push(format!(
                            "Command denied by policy (dangerous: {}). Do not retry without changing the command.",
                            decision.description
                        ));
                        continue;
                    }
                    tools::ApprovalAction::Auto => {
                        // 放行，继续执行
                    }
                    tools::ApprovalAction::Ask => {
                        let cmd = call
                            .arguments
                            .get("command")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        // 可选辅模型降级 Ask → Auto
                        let smart_action = {
                            let agent = session.lock().await;
                            let pname = agent.chat_provider().to_string();
                            let model = agent.chat_model().to_string();
                            let api_key = agent.chat_api_key().to_string();
                            let base_url = agent.chat_base_url().to_string();
                            let providers = agent.providers_arc();
                            drop(agent);
                            if let Some(provider) = providers.get(&pname) {
                                let config = ProviderConfig {
                                    model: if model.trim().is_empty() {
                                        provider.default_model().to_string()
                                    } else {
                                        model
                                    },
                                    api_key,
                                    base_url: if base_url.trim().is_empty() {
                                        None
                                    } else {
                                        Some(base_url)
                                    },
                                    ..ProviderConfig::default()
                                };
                                crate::smart_approval::maybe_smart_downgrade_ask(
                                    cmd,
                                    decision.description,
                                    provider,
                                    config,
                                )
                                .await
                            } else {
                                tools::ApprovalAction::Ask
                            }
                        };
                        if smart_action == tools::ApprovalAction::Auto {
                            tracing::info!(
                                command = %cmd,
                                reason = decision.description,
                                "smart approval auto-approved dangerous command"
                            );
                            // 放行，继续执行
                        } else if let Some(gate) = hitl_gate {
                            let title = "批准危险命令";
                            let body = format!(
                                "检测到潜在危险操作（{}）：\n\n```\n{cmd}\n```",
                                decision.description
                            );
                            let approved =
                                park_confirm(gate, tx, run_id, &call.id, title, &body).await?;
                            if !approved {
                                out.push(
                                    "Command denied by user (dangerous-command approval). Do not retry the same command without explicit user request.".to_string(),
                                );
                                continue;
                            }
                        } else {
                            out.push(format!(
                                "Command blocked: dangerous ({}) and no HITL gate available.",
                                decision.description
                            ));
                            continue;
                        }
                    }
                }
            }
        }

        let mut result = if call.args_parse_error {
            format!(
                "工具参数 JSON 解析失败: {}",
                call.arguments
                    .get("_parse_error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("invalid json")
            )
        } else {
            let mut agent = session.lock().await;
            tokio::task::block_in_place(|| {
                agent.handle_tool_call(&call.name, &call.arguments)
            })
            .unwrap_or_else(|e| format!("工具错误: {e}"))
        };

        // confirm/clarify：astro_hitl → 同回合 park
        if let Some(hitl) = parse_astro_hitl(&result) {
            if let Some(gate) = hitl_gate {
                result = park_astro_hitl(gate, tx, run_id, &call.id, hitl).await?;
            } else {
                // 无闸门（测试/legacy）：退回旧行为不可用，改为说明
                result = "HITL gate unavailable; confirmation/clarification could not be shown to the user.".to_string();
            }
        }

        if pause.is_cancelled() {
            return None;
        }
        out.push(result);
    }
    Some(out)
}

/// 并发执行非 interactive/exclusive 工具；按调用顺序返回结果。
async fn execute_tools_concurrent(
    session: &Arc<Mutex<AgentLoop>>,
    calls: &[tools::ParsedToolCall],
    pause: &Arc<PauseControl>,
) -> Option<Vec<String>> {
    if pause.is_cancelled() || !pause.wait_if_paused().await {
        return None;
    }

    let snap = {
        let agent = session.lock().await;
        ToolExecSnapshot {
            memory_dir: agent.memory_dir().to_path_buf(),
            agent_id: agent.agent_id().to_string(),
            workspace_dir: agent.workspace_dir(),
            session_id: agent.session_id().to_string(),
            chat_api_key: agent.chat_api_key().to_string(),
            chat_base_url: agent.chat_base_url().to_string(),
            chat_provider: agent.chat_provider().to_string(),
            chat_model: agent.chat_model().to_string(),
            image_gen_targets: agent.image_gen_targets().clone(),
            providers: agent.providers_arc(),
        }
    };

    let mut join_set = JoinSet::new();
    for (idx, call) in calls.iter().cloned().enumerate() {
        let snap = snap.clone();
        join_set.spawn_blocking(move || {
            let result = if call.args_parse_error {
                format!(
                    "工具参数 JSON 解析失败: {}",
                    call.arguments
                        .get("_parse_error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("invalid json")
                )
            } else {
                run_tool_on_snapshot(&snap, &call.name, &call.arguments)
            };
            (idx, result)
        });
    }

    let mut slots: Vec<Option<String>> = (0..calls.len()).map(|_| None).collect();
    while let Some(joined) = join_set.join_next().await {
        match joined {
            Ok((idx, result)) => {
                if let Some(slot) = slots.get_mut(idx) {
                    *slot = Some(result);
                }
            }
            Err(e) => {
                // 标记失败占位
                let msg = format!("工具错误: join failed: {e}");
                if let Some(empty_idx) = slots.iter().position(|s| s.is_none()) {
                    slots[empty_idx] = Some(msg);
                }
            }
        }
    }
    Some(
        slots
            .into_iter()
            .map(|s| s.unwrap_or_else(|| "工具错误: missing result".into()))
            .collect(),
    )
}

#[derive(Clone)]
struct ToolExecSnapshot {
    memory_dir: std::path::PathBuf,
    agent_id: String,
    workspace_dir: std::path::PathBuf,
    session_id: String,
    chat_api_key: String,
    chat_base_url: String,
    chat_provider: String,
    chat_model: String,
    image_gen_targets: tools::ImageGenTargets,
    providers: Arc<providers::registry::ProviderRegistry>,
}

fn run_tool_on_snapshot(
    snap: &ToolExecSnapshot,
    name: &str,
    args: &serde_json::Value,
) -> String {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => return format!("工具错误: runtime: {e}"),
    };
    rt.block_on(async {
        let mut memory = match memory::MemoryManager::for_agent(
            snap.memory_dir.clone(),
            &snap.agent_id,
        ) {
            Ok(m) => m,
            Err(e) => return format!("工具错误: memory: {e}"),
        };
        let mut ctx = tools::ToolContext {
            memory: &mut memory,
            memory_dir: snap.memory_dir.clone(),
            workspace_dir: snap.workspace_dir.clone(),
            image_gen_targets: &snap.image_gen_targets,
            providers: snap.providers.as_ref(),
            session_id: snap.session_id.clone(),
            chat_api_key: snap.chat_api_key.clone(),
            chat_base_url: snap.chat_base_url.clone(),
            chat_provider: snap.chat_provider.clone(),
            chat_model: snap.chat_model.clone(),
        };
        tools::dispatch_tool(|_| true, &mut ctx, name, args)
            .await
            .unwrap_or_else(|e| format!("工具错误: {e}"))
    })
}

async fn park_confirm(
    gate: &Arc<HitlGate>,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    run_id: &str,
    tool_call_id: &str,
    title: &str,
    body: &str,
) -> Option<bool> {
    let surface_id = format!("confirm-{}", uuid::Uuid::new_v4());
    let operations = a2ui::templates::build_confirm_surface(&surface_id, title, body);
    let ops_value = serde_json::Value::Array(operations);
    let result = park_astro_hitl(
        gate,
        tx,
        run_id,
        tool_call_id,
        AstroHitlPayload {
            reason: "confirmation".into(),
            message: title.into(),
            operations: ops_value,
            response_schema: serde_json::json!({
                "type": "object",
                "properties": { "approved": { "type": "boolean" } },
                "required": ["approved"]
            }),
        },
    )
    .await?;
    let v: serde_json::Value = serde_json::from_str(&result).unwrap_or_default();
    Some(v.get("approved").and_then(|x| x.as_bool()).unwrap_or(false))
}

async fn park_astro_hitl(
    gate: &Arc<HitlGate>,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    run_id: &str,
    tool_call_id: &str,
    hitl: AstroHitlPayload,
) -> Option<String> {
    let message_id = format!("a2ui-surface-{tool_call_id}");
    let content_json = serde_json::json!({ "operations": hitl.operations }).to_string();
    if !emit(
        tx,
        MultiTurnStreamItem::Activity {
            message_id,
            activity_type: "a2ui-surface".into(),
            content_json,
            replace: true,
        },
    )
    .await
    {
        return None;
    }

    let interrupt = Interrupt {
        id: uuid::Uuid::new_v4().to_string(),
        reason: hitl.reason.clone(),
        message: hitl.message.clone(),
        tool_call_id: tool_call_id.to_string(),
        response_schema_json: hitl.response_schema.to_string(),
        expires_at: String::new(),
        metadata_json: String::new(),
    };
    let interrupts_json =
        serde_json::to_string(&vec![&interrupt]).unwrap_or_else(|_| "[]".into());
    if !emit(
        tx,
        MultiTurnStreamItem::RunFinished {
            run_id: run_id.to_string(),
            outcome_type: "hitl_waiting".into(),
            interrupts_json,
        },
    )
    .await
    {
        return None;
    }

    let rx = gate.begin_wait(interrupt.clone()).await;
    let resolution = gate
        .finish_wait(
            &interrupt.id,
            rx,
            Duration::from_secs(HITL_DEFAULT_TIMEOUT_SECS),
        )
        .await;
    Some(resolution.to_tool_result())
}

/// 在后台 task 启动 [`run_multi_turn_stream`]，并返回可消费的 [`MultiTurnStream`]。
///
/// channel 容量为 32；消费者 drop 后发送方通过 [`emit`] 返回 `false` 自然退出。
pub fn stream_multi_turn(
    session: Arc<Mutex<AgentLoop>>,
    targets: Vec<ChatTarget>,
    registry: Arc<ProviderRegistry>,
    base_config: ProviderConfig,
    system_prompt: String,
    pause: Arc<PauseControl>,
) -> MultiTurnStream {
    stream_multi_turn_with_hitl(
        session,
        targets,
        registry,
        base_config,
        system_prompt,
        pause,
        None,
    )
}

/// 带 HITL 闸门的多轮流。
pub fn stream_multi_turn_with_hitl(
    session: Arc<Mutex<AgentLoop>>,
    targets: Vec<ChatTarget>,
    registry: Arc<ProviderRegistry>,
    base_config: ProviderConfig,
    system_prompt: String,
    pause: Arc<PauseControl>,
    hitl_gate: Option<Arc<HitlGate>>,
) -> MultiTurnStream {
    let (tx, rx) = mpsc::channel(32);
    tokio::spawn(async move {
        run_multi_turn_stream(
            session,
            targets,
            registry,
            base_config,
            system_prompt,
            pause,
            hitl_gate,
            tx,
        )
        .await;
    });
    Box::pin(futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    }))
}

/// 旧签名兼容：单 Provider + config → 单元素链。
pub fn stream_multi_turn_from_provider(
    session: Arc<Mutex<AgentLoop>>,
    provider: Arc<dyn AiProvider>,
    config: ProviderConfig,
    system_prompt: String,
    pause: Arc<PauseControl>,
) -> MultiTurnStream {
    let (targets, registry) = targets_and_registry_from_primary(provider, &config);
    stream_multi_turn(session, targets, registry, config, system_prompt, pause)
}

pub(crate) struct AstroHitlPayload {
    pub reason: String,
    pub message: String,
    pub operations: serde_json::Value,
    pub response_schema: serde_json::Value,
}

struct AstroUiPayload {
    summary: String,
    operations: serde_json::Value,
}

pub(crate) fn parse_astro_hitl(result: &str) -> Option<AstroHitlPayload> {
    let value: serde_json::Value = serde_json::from_str(result).ok()?;
    if value.get("astro_hitl")?.as_bool() != Some(true) {
        return None;
    }
    let operations = value.get("operations")?.clone();
    if !operations.is_array() {
        return None;
    }
    Some(AstroHitlPayload {
        reason: value
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("confirmation")
            .to_string(),
        message: value
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        operations,
        response_schema: value
            .get("response_schema")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    })
}

fn parse_astro_ui(result: &str) -> Option<AstroUiPayload> {
    let value: serde_json::Value = serde_json::from_str(result).ok()?;
    if value.get("astro_ui")?.as_bool() != Some(true) {
        return None;
    }
    // HITL 优先：同结果不应既 hitl 又 ui
    if value.get("astro_hitl").and_then(|v| v.as_bool()) == Some(true) {
        return None;
    }
    let operations = value.get("operations")?.clone();
    if !operations.is_array() {
        return None;
    }
    Some(AstroUiPayload {
        summary: value
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("info")
            .to_string(),
        operations,
    })
}

#[cfg(test)]
mod child_hitl_tests {
    use super::*;
    use crate::interrupt::ResumeItem;
    use serde_json::json;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn try_park_parent_hitl_resolves_via_gate() {
        let gate = HitlGate::new("parent-sess");
        let (tx, mut rx) = mpsc::channel::<anyhow::Result<MultiTurnStreamItem>>(8);
        let ctx = ParentHitlCtx {
            gate: gate.clone(),
            tx,
            run_id: "run-1".into(),
        };

        let gate_resolver = gate.clone();
        let resolve_task = tokio::spawn(async move {
            // 等到有 waiting
            for _ in 0..50 {
                if gate_resolver.is_waiting().await {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let pending = gate_resolver.pending_interrupts().await;
            assert!(!pending.is_empty());
            assert!(pending[0].reason.contains("[delegate]"));
            gate_resolver
                .resolve(&[ResumeItem {
                    interrupt_id: pending[0].id.clone(),
                    status: "resolved".into(),
                    payload_json: r#"{"approved":true}"#.into(),
                }])
                .await
                .unwrap();
        });

        let hitl = AstroHitlPayload {
            reason: "confirmation".into(),
            message: "ok?".into(),
            operations: json!([]),
            response_schema: json!({
                "type": "object",
                "properties": { "approved": { "type": "boolean" } },
                "required": ["approved"]
            }),
        };

        let result = PARENT_HITL_CTX
            .scope(Some(ctx), async {
                try_park_parent_hitl("tc-child-1", hitl, None).await
            })
            .await
            .expect("park should return");

        assert!(result.contains("approved") || result.contains("true"), "got {result}");
        resolve_task.await.unwrap();

        // 至少收到 Activity 或 hitl_waiting
        let mut saw_waiting = false;
        while let Ok(item) = rx.try_recv() {
            if let Ok(MultiTurnStreamItem::RunFinished { outcome_type, .. }) = item {
                if outcome_type == "hitl_waiting" {
                    saw_waiting = true;
                }
            }
        }
        assert!(saw_waiting, "expected hitl_waiting on parent stream");
    }

    #[tokio::test]
    async fn try_park_without_ctx_returns_none() {
        let hitl = AstroHitlPayload {
            reason: "confirmation".into(),
            message: "x".into(),
            operations: json!([]),
            response_schema: json!({}),
        };
        assert!(try_park_parent_hitl("tc", hitl, None).await.is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn try_park_via_live_session_map() {
        let gate = HitlGate::new("async-parent");
        let (tx, mut rx) = mpsc::channel::<anyhow::Result<MultiTurnStreamItem>>(8);
        register_live_parent_hitl(
            "async-parent",
            ParentHitlCtx {
                gate: gate.clone(),
                tx,
                run_id: "run-async".into(),
            },
        )
        .await;

        let gate_resolver = gate.clone();
        let resolve_task = tokio::spawn(async move {
            for _ in 0..50 {
                if gate_resolver.is_waiting().await {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let pending = gate_resolver.pending_interrupts().await;
            assert!(!pending.is_empty());
            assert!(pending[0].reason.contains("delegate_async"));
            gate_resolver
                .resolve(&[ResumeItem {
                    interrupt_id: pending[0].id.clone(),
                    status: "resolved".into(),
                    payload_json: r#"{"approved":true}"#.into(),
                }])
                .await
                .unwrap();
        });

        let hitl = AstroHitlPayload {
            reason: "confirmation".into(),
            message: "async?".into(),
            operations: json!([]),
            response_schema: json!({
                "type": "object",
                "properties": { "approved": { "type": "boolean" } },
                "required": ["approved"]
            }),
        };
        let result = try_park_parent_hitl("tc-async", hitl, Some("async-parent"))
            .await
            .expect("live park");
        assert!(result.contains("approved") || result.contains("true"), "got {result}");
        resolve_task.await.unwrap();
        unregister_live_parent_hitl("async-parent").await;

        let mut saw_waiting = false;
        while let Ok(item) = rx.try_recv() {
            if let Ok(MultiTurnStreamItem::RunFinished { outcome_type, .. }) = item {
                if outcome_type == "hitl_waiting" {
                    saw_waiting = true;
                }
            }
        }
        assert!(saw_waiting);
    }
}

