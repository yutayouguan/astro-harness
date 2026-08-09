//! 无头（非流式）多轮工具循环，供定时任务等不需要 UI 推流的场景使用。
//!
//! 与 `run_multi_turn_stream_inner` 共享核心基础设施：
//! - [`ProviderStreamer`] 统一 fallback 链与流式消费
//! - [`collect_response`] 一次性累积流式响应
//! - [`AgentLoop::prepare_llm_context`] 统一上下文准备
//! - [`AgentLoop::record_assistant_with_calls`] 统一 assistant 消息记录
//! - [`IterationBudget`] 迭代预算（对齐 Hermes）
//!
//! 省略 HITL、pause/cancel、streaming channel、A2UI 渲染、timeline 等 UI 专属逻辑。

use common::ChatTarget;
use providers::Usage;
use providers::ProviderConfig;

use crate::runtime::budget::{should_refund_tool_round, IterationBudget, DEFAULT_MAX_ITERATIONS};
use crate::runtime::AgentLoop;
use crate::streaming::accumulate::collect_response;
use crate::streaming::{ProviderStreamer, StreamingChat};

const MAX_THINKING_ONLY_RETRIES: usize = 1;

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
    let streamer = ProviderStreamer::new(targets, base_config);

    let mut total_usage = Usage::default();
    let mut last_text = String::new();
    let mut thinking_only_retries: usize = 0;

    let max_rounds = {
        let n = agent.multi_turn();
        if n == 0 { DEFAULT_MAX_ITERATIONS } else { n }
    };
    let budget = IterationBudget::new(max_rounds);

    loop {
        if !budget.consume() || agent.is_tool_depth_exhausted() {
            break;
        }

        let _ = agent.maintain_tool_context().await;

        let (history, tools) = agent.prepare_llm_context().await;

        let stream = streamer
            .stream_chat(&system_prompt, &history, tools)
            .await?;

        let response = collect_response(stream).await?;

        if let Some(u) = response.usage {
            total_usage.add_assign(u);
        }

        // 空响应处理：thinking-only 重试
        if response.text.is_empty() && response.calls.is_empty() {
            if !response.reasoning.is_empty()
                && thinking_only_retries < MAX_THINKING_ONLY_RETRIES
            {
                thinking_only_retries += 1;
                tracing::warn!(
                    reasoning_len = response.reasoning.len(),
                    attempt = thinking_only_retries,
                    "headless: model returned reasoning only; injecting retry prompt"
                );
                agent.record_assistant_with_calls(
                    &response.text,
                    &[],
                    Some(response.reasoning.as_str()),
                    None,
                )?;
                agent.record_user_message(
                    "[astro:system]\n你的思考过程已记录，但没有生成回复内容。请直接给出你的回答。",
                )?;
                continue;
            }
            break;
        }

        last_text = response.text.clone();
        agent.record_assistant_with_calls(&response.text, &response.calls, None, None)?;

        if response.calls.is_empty() {
            return Ok((last_text, total_usage));
        }

        // 工具执行
        let names: Vec<&str> = response.calls.iter().map(|c| c.name.as_str()).collect();
        let stop_after = agent.tool_registry().any_stop_after(&names);

        for call in &response.calls {
            let result = match agent
                .handle_tool_call_async(&call.name, &call.arguments)
                .await
            {
                Ok(output) => output,
                Err(crate::runtime::ToolCallError::Cancelled) => break,
                Err(e) => format!("工具错误: {e}").into(),
            };
            agent.record_tool_result_with_id(Some(&call.id), Some(&call.name), result.text())?;
        }

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
