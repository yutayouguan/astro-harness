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

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use common::message::Message;
use futures::stream::{AbortHandle, Abortable};
use futures::{Stream, StreamExt};
use providers::streaming::{PauseControl, Usage};
use providers::trait_::{
    AiProvider, ChatChunk, ChatMessage as ProviderMessage, ChatStream, ChatToolCall,
    ProviderConfig, ToolCallDeltaChunk,
};
use tokio::sync::{mpsc, Mutex};

use crate::loop_::AgentLoop;

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
    /// AG-UI `RUN_FINISHED`：`outcome_type` 为 `success` 或 `interrupt`。
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

/// 将会话消息转为 Provider 消息序列，首条固定为 system prompt。
///
/// tool 角色消息会回溯 assistant 中的 `tool_calls` 以填充 `name` 字段。
pub fn to_provider_messages(system_prompt: &str, session: &[Message]) -> Vec<ProviderMessage> {
    use common::message::Role;

    let mut messages = vec![ProviderMessage::text("system", system_prompt)];

    for message in session {
        let role = match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
            Role::Tool => "tool",
        };
        let tool_calls = message.tool_calls.as_ref().map(|calls| {
            calls
                .iter()
                .map(|c| ChatToolCall {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    arguments: c.arguments.clone(),
                })
                .collect()
        });
        let tool_name = if message.role == Role::Tool {
            message.tool_call_id.as_ref().and_then(|id| {
                session.iter().rev().find_map(|m| {
                    m.tool_calls
                        .as_ref()?
                        .iter()
                        .find(|c| &c.id == id)
                        .map(|c| c.name.clone())
                })
            })
        } else {
            None
        };
        messages.push(ProviderMessage {
            role: role.to_string(),
            content: message.content_str().to_string(),
            tool_calls,
            tool_call_id: message.tool_call_id.clone(),
            name: tool_name,
        });
    }

    messages
}

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

/// 包装 [`AiProvider`]，实现三层 Streaming trait 的默认适配器。
pub struct ProviderStreamer {
    /// 底层 LLM Provider 实例。
    pub provider: Arc<dyn AiProvider>,
    /// 模型、温度、API 等运行时配置。
    pub config: ProviderConfig,
}

#[async_trait]
impl StreamingCompletion for ProviderStreamer {
    /// 委托 `provider.chat_stream` 并经由 [`map_provider_stream`] 归一化 chunk。
    async fn stream_completion(
        &self,
        messages: Vec<ProviderMessage>,
        tools: Vec<serde_json::Value>,
    ) -> anyhow::Result<AssistantContentStream> {
        let stream = self
            .provider
            .chat_stream(messages, tools, &self.config)
            .await?;
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

/// 尽力写入一条 `kind=llm` 事件；失败忽略。
async fn record_llm_usage(session: &Arc<Mutex<AgentLoop>>, model: &str, usage: &Usage) {
    if usage.prompt_tokens == 0 && usage.completion_tokens == 0 && usage.total_tokens == 0 {
        return;
    }
    let agent = session.lock().await;
    let agent_id = agent.agent_id().to_string();
    let session_id = Some(agent.session_id().to_string());
    drop(agent);
    let cost = memory::estimate_llm_cost(model, usage.prompt_tokens, usage.completion_tokens);
    memory::UsageDb::try_record(memory::NewUsageEvent {
        ts: chrono::Utc::now().to_rfc3339(),
        kind: "llm".into(),
        name: model.to_string(),
        agent_id,
        session_id,
        prompt_tokens: i64::from(usage.prompt_tokens),
        completion_tokens: i64::from(usage.completion_tokens),
        total_tokens: i64::from(usage.total_tokens),
        cost_usd: cost,
        meta_json: None,
    });
}

/// 发送 Error 后立即发送 Done；若有已累计 usage 则先写入 `usage.db`。
async fn finish_error(
    session: &Arc<Mutex<AgentLoop>>,
    model: &str,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    msg: impl Into<String>,
    usage: Option<Usage>,
) {
    if let Some(u) = usage.as_ref() {
        record_llm_usage(session, model, u).await;
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
    model: &str,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    usage: Option<Usage>,
    run_id: &str,
) {
    if let Some(u) = usage {
        record_llm_usage(session, model, &u).await;
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
pub async fn run_multi_turn_stream(
    session: Arc<Mutex<AgentLoop>>,
    provider: Arc<dyn AiProvider>,
    config: ProviderConfig,
    system_prompt: String,
    pause: Arc<PauseControl>,
    tx: mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
) {
    let model = config.model.clone();
    let streamer = ProviderStreamer {
        provider: provider.clone(),
        config,
    };
    let mut total_usage = Usage::default();
    let mut saw_usage = false;

    let (thread_id, run_id) = {
        let agent = session.lock().await;
        (agent.session_id().to_string(), uuid::Uuid::new_v4().to_string())
    };
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

    for round in 0..max_rounds {
        if pause.is_cancelled() {
            finish_usage_and_done(&session, &model, &tx, saw_usage.then_some(total_usage), &run_id).await;
            return;
        }
        if !pause.wait_if_paused().await {
            finish_usage_and_done(&session, &model, &tx, saw_usage.then_some(total_usage), &run_id).await;
            return;
        }

        let (history, tools) = {
            let mut agent = session.lock().await;
            agent.reload_tools_and_mcp().await;
            let messages = agent.session_messages.clone();
            let tools = agent.tool_registry().schemas_for_api();
            (messages, tools)
        };

        let raw_stream = match streamer
            .stream_chat(&system_prompt, &history, tools)
            .await
        {
            Ok(s) => s,
            Err(err) => {
                finish_error(
                    &session,
                    &model,
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
        let mut tool_acc = tools::ToolCallAccumulator::new();
        // Google 等会在每个 chunk 带累计 usage：本轮覆盖式取最后一次
        let mut round_usage: Option<Usage> = None;

        loop {
            // Rig 语义：先确认未 pause，再 poll 上游
            if !pause.wait_if_paused().await {
                pause.clear_abort();
                finish_usage_and_done(
                    &session,
                    &model,
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
                        &model,
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
                        &model,
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
            finish_usage_and_done(&session, &model, &tx, saw_usage.then_some(total_usage), &run_id).await;
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
                &model,
                &tx,
                "模型返回了空回复。请重试，或换一个模型。",
                saw_usage.then_some(total_usage),
            )
            .await;
            return;
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
            if let Err(err) = agent.record_assistant_message_with_tools(&full_response, tc) {
                finish_error(
                    &session,
                    &model,
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

        for call in calls {
            if pause.is_cancelled() {
                finish_usage_and_done(&session, &model, &tx, saw_usage.then_some(total_usage), &run_id).await;
                return;
            }
            if !pause.wait_if_paused().await {
                finish_usage_and_done(&session, &model, &tx, saw_usage.then_some(total_usage), &run_id).await;
                return;
            }

            let result = if call.args_parse_error {
                format!(
                    "工具参数 JSON 解析失败: {}",
                    call.arguments
                        .get("_parse_error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("invalid json")
                )
            } else {
                // MemoryManager !Send：持锁 + block_in_place（Rig 亦在工具边界同步执行）
                let mut agent = session.lock().await;
                tokio::task::block_in_place(|| {
                    agent.handle_tool_call(&call.name, &call.arguments)
                })
                .unwrap_or_else(|e| format!("工具错误: {e}"))
            };

            if pause.is_cancelled() {
                finish_usage_and_done(&session, &model, &tx, saw_usage.then_some(total_usage), &run_id).await;
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

            // 记忆工具成功后发出 MemoryUpdate，供右侧时间线展示
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

            // 先解析声明式 UI：信息卡只发 Activity；HITL 另走 interrupt
            let info_ui = parse_astro_ui(&result);
            let result_for_history = if let Some(ref ui) = info_ui {
                format!("Presented info card: {}", ui.summary)
            } else {
                result.clone()
            };

            if let Some(ref ui) = info_ui {
                let message_id = format!("a2ui-surface-{}", call.id);
                let content_json =
                    serde_json::json!({ "operations": ui.operations }).to_string();
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
                let _ = agent.record_tool_result_with_id(Some(&call.id), &result_for_history);
            }

            // HITL：confirm/clarify 等返回 astro_hitl → Activity + RunFinished(interrupt)
            if let Some(hitl) = parse_astro_hitl(&result) {
                let message_id = format!("a2ui-surface-{}", call.id);
                let content_json = serde_json::json!({ "operations": hitl.operations }).to_string();
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

                let interrupt = crate::interrupt::Interrupt {
                    id: uuid::Uuid::new_v4().to_string(),
                    reason: hitl.reason,
                    message: hitl.message,
                    tool_call_id: call.id.clone(),
                    response_schema_json: hitl.response_schema.to_string(),
                    expires_at: String::new(),
                    metadata_json: String::new(),
                };
                let interrupts_json = serde_json::to_string(&vec![&interrupt]).unwrap_or_else(|_| "[]".into());
                let _ = emit(
                    &tx,
                    MultiTurnStreamItem::RunFinished {
                        run_id: run_id.clone(),
                        outcome_type: "interrupt".into(),
                        interrupts_json,
                    },
                )
                .await;
                let _ = emit(&tx, MultiTurnStreamItem::Done).await;
                return;
            }
        }

        if last_round {
            finish_error(
                &session,
                &model,
                &tx,
                "工具调用轮次已用尽，请简化任务后重试。".to_string(),
                saw_usage.then_some(total_usage),
            )
            .await;
            return;
        }
    }

    finish_usage_and_done(&session, &model, &tx, saw_usage.then_some(total_usage), &run_id).await;
}

/// 在后台 task 启动 [`run_multi_turn_stream`]，并返回可消费的 [`MultiTurnStream`]。
///
/// channel 容量为 32；消费者 drop 后发送方通过 [`emit`] 返回 `false` 自然退出。
pub fn stream_multi_turn(
    session: Arc<Mutex<AgentLoop>>,
    provider: Arc<dyn AiProvider>,
    config: ProviderConfig,
    system_prompt: String,
    pause: Arc<PauseControl>,
) -> MultiTurnStream {
    let (tx, rx) = mpsc::channel(32);
    tokio::spawn(async move {
        run_multi_turn_stream(session, provider, config, system_prompt, pause, tx).await;
    });
    Box::pin(futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    }))
}

struct AstroHitlPayload {
    reason: String,
    message: String,
    operations: serde_json::Value,
    response_schema: serde_json::Value,
}

struct AstroUiPayload {
    summary: String,
    operations: serde_json::Value,
}

fn parse_astro_hitl(result: &str) -> Option<AstroHitlPayload> {
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
