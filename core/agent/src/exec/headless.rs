//! 无头（非流式）多轮工具循环，供定时任务等不需要 UI 推流的场景使用。
//!
//! [`run_headless_multi_turn`] 与 `run_multi_turn_stream_inner` 共享核心逻辑：
//! - 原生 `tool_call_deltas` + XML `<tool_call>` 统一解析（[`tools::resolve_tool_calls`]）
//! - [`tools::ToolCallAccumulator`] 累积 native function calling 增量
//! - [`AgentLoop::maintain_tool_context`] 上下文压缩/修剪
//! - [`AgentLoop::provider_history`]（含 mid-run handoff 折叠）
//! - [`AgentLoop::take_inject_context`] hook 注入
//! - [`crate::runtime::budget::IterationBudget`] 迭代预算（对齐 Hermes）
//! - `stop_after_tool_call` 与 `code_exec` 预算退还
//!
//! 省略 HITL、pause/cancel、streaming channel、A2UI 渲染、timeline 等 UI 专属逻辑。

use common::ChatTarget;
use futures::StreamExt;
use providers::Usage;
use providers::ProviderConfig;

use crate::prompt::messages::to_provider_messages;
use crate::runtime::budget::{should_refund_tool_round, IterationBudget, DEFAULT_MAX_ITERATIONS};
use crate::runtime::AgentLoop;
use crate::streaming::fallback::try_stream_completion_with_fallback;
use tools::{ToolCallAccumulator, ToolCallDelta};

/// 无头多轮工具循环：驱动 LLM → 工具 → LLM 直至无工具调用或预算耗尽。
///
/// # 参数
///
/// - `agent`：已经过 [`AgentLoop::run_turn`] 初始化的实例（`session_messages` 含用户消息）。
/// - `targets`：聊天 fallback 链（`CronExecCredentials::effective_targets` 返回值）。
/// - `system_prompt`：来自 `TurnResult::Continue { system_prompt }` 的系统提示；整轮不变。
///
/// # 返回
///
/// `(最终 assistant 文本, 累计 token 用量)`。
///
/// # 错误
///
/// - Provider 全部失败。
/// - 预算耗尽后仍无有效回复。
/// - 模型始终返回空响应。
pub async fn run_headless_multi_turn(
    agent: &mut AgentLoop,
    targets: Vec<ChatTarget>,
    system_prompt: String,
) -> anyhow::Result<(String, Usage)> {
    let base_config = ProviderConfig {
        temperature: agent.temperature(),
        additional_params: agent.additional_params().clone(),
        ..ProviderConfig::default()
    };

    const MAX_THINKING_ONLY_RETRIES: usize = 1;

    let mut total_usage = Usage::default();
    let mut last_text = String::new();
    let mut thinking_only_retries: usize = 0;

    let max_rounds = {
        let n = agent.multi_turn();
        if n == 0 {
            DEFAULT_MAX_ITERATIONS
        } else {
            n
        }
    };
    let budget = IterationBudget::new(max_rounds);

    loop {
        if !budget.consume() {
            break;
        }
        if agent.is_tool_depth_exhausted() {
            break;
        }

        // 进入 LLM 前维护上下文（压缩/修剪），与 streaming 路径 gateway 一致
        let _ = agent.maintain_tool_context().await;

        agent.reload_tools_and_mcp().await;

        let mut history = agent.provider_history();
        if let Some(ctx) = agent.take_inject_context() {
            history.push(common::message::Message::user(&format!(
                "[astro:hook-context]\n{ctx}"
            )));
        }
        let messages = to_provider_messages(&system_prompt, &history);
        let tools_schema = agent.schemas_for_api();

        let (mut stream, _meta) = try_stream_completion_with_fallback(
            &targets,
            messages,
            tools_schema,
            &base_config,
            |from, to, err| {
                tracing::warn!(
                    from_backend = %from.backend_id,
                    to_backend = %to.backend_id,
                    error = %err,
                    "headless multi-turn: failover"
                );
            },
        )
        .await?;

        let mut full_text = String::new();
        let mut full_reasoning = String::new();
        let mut tool_acc = ToolCallAccumulator::new();
        // 同轮内覆盖式取最后一次 usage（兼容 Google 累计式 usageMetadata）
        let mut round_usage: Option<Usage> = None;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| anyhow::anyhow!("{e}"))?;
            match chunk {
                providers::types::stream::StreamChunk::Text(token) => {
                    full_text.push_str(&token);
                }
                providers::types::stream::StreamChunk::Thinking(r) => {
                    full_reasoning.push_str(&r);
                }
                providers::types::stream::StreamChunk::ToolCallStart { index, id, name } => {
                    tool_acc.push(&ToolCallDelta {
                        index,
                        id: Some(id),
                        name: Some(name),
                        arguments: None,
                        signature: None,
                    });
                }
                providers::types::stream::StreamChunk::ToolCallDelta { index, arguments } => {
                    tool_acc.push(&ToolCallDelta {
                        index,
                        id: None,
                        name: None,
                        arguments: Some(arguments),
                        signature: None,
                    });
                }
                providers::types::stream::StreamChunk::Usage(u) => {
                    round_usage = Some(u);
                }
                _ => {}
            }
        }

        if let Some(u) = round_usage {
            total_usage.add_assign(u);
        }

        let native_calls = tool_acc.finish();
        let calls = tools::resolve_tool_calls(native_calls, &full_text);

        if full_text.is_empty() && calls.is_empty() {
            if !full_reasoning.is_empty()
                && thinking_only_retries < MAX_THINKING_ONLY_RETRIES
            {
                thinking_only_retries += 1;
                tracing::warn!(
                    reasoning_len = full_reasoning.len(),
                    attempt = thinking_only_retries,
                    "headless: model returned reasoning only with no text; injecting retry prompt"
                );
                agent.record_assistant_message_with_tools(
                    &full_text,
                    None,
                    Some(full_reasoning.as_str()),
                    None,
                )?;
                agent.record_user_message(
                    "[astro:system]\n你的思考过程已记录，但没有生成回复内容。请直接给出你的回答。",
                )?;
                continue;
            }
            break;
        }

        last_text = full_text.clone();

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
                        signature: c.signature.clone(),
                    })
                    .collect(),
            )
        };
        agent.record_assistant_message_with_tools(&full_text, tc, None, None)?;

        if calls.is_empty() {
            return Ok((last_text, total_usage));
        }

        let names: Vec<&str> = calls.iter().map(|c| c.name.as_str()).collect();
        let stop_after = agent.tool_registry().any_stop_after(&names);

        for call in &calls {
            let result = agent
                .handle_tool_call_async(&call.name, &call.arguments)
                .await
                .unwrap_or_else(|e| format!("工具错误: {e}").into());
            agent.record_tool_result_with_id(Some(&call.id), Some(&call.name), result.text())?;
        }

        // 工具执行后维护上下文，与 streaming 路径对齐
        let _ = agent.maintain_tool_context().await;

        if should_refund_tool_round(&names) {
            budget.refund();
        }

        if stop_after {
            break;
        }
    }

    if last_text.is_empty() {
        anyhow::bail!("模型未返回有效回复");
    }
    Ok((last_text, total_usage))
}
