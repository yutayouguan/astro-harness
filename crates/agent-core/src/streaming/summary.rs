//! 迭代预算耗尽后的强制总结轮（对齐 Hermes `handle_max_iterations`）。

use std::sync::Arc;

use futures::stream::{AbortHandle, Abortable};
use futures::StreamExt;
use providers::{PauseControl, Usage};
use types::message::Message;

use crate::runtime::{AgentLoop, TurnContext};

use super::lifecycle::{emit_delta, emit_response_items_completed, emit_text_item_started};
use super::provider::ProviderStreamer;
use super::types::StreamedAssistantContent;

/// 预算耗尽后注入的总结提示（对齐 Hermes `handle_max_iterations`）。
const MAX_ITERATIONS_SUMMARY_PROMPT: &str = "\
你已达到本回合允许的最大工具调用迭代次数。\
请直接给出最终回复，总结目前已完成与尚未完成的内容，不要再调用任何工具。";

pub(crate) enum SummaryOutcome {
    Finished,
    Aborted,
    Failed(String),
}

/// [`run_max_iterations_summary`] 入参打包。
pub(crate) struct MaxIterationsSummaryArgs<'a> {
    pub session: &'a Arc<AgentLoop>,
    pub streamer: &'a ProviderStreamer,
    pub prompt: &'a crate::prompt::PromptContract,
    pub pause: &'a Arc<PauseControl>,
    pub turn_context: &'a TurnContext,
    pub timeline: &'a mut crate::timeline::TimelineBuilder,
    pub total_usage: &'a mut Usage,
    pub saw_usage: &'a mut bool,
    pub used: usize,
    pub max_total: usize,
}

/// 预算耗尽后：注入总结提示，再发一轮 **无 tools** 的 completion（对齐 Hermes）。
pub(crate) async fn run_max_iterations_summary(a: MaxIterationsSummaryArgs<'_>) -> SummaryOutcome {
    let MaxIterationsSummaryArgs {
        session,
        streamer,
        prompt,
        pause,
        turn_context,
        timeline,
        total_usage,
        saw_usage,
        used,
        max_total,
    } = a;
    let notice =
        format!("⚠️ 迭代预算已用尽（{used}/{max_total}），正在请求模型总结（不再调用工具）…\n\n");
    let assistant_item_id = uuid::Uuid::new_v4().to_string();
    let reasoning_item_id = uuid::Uuid::new_v4().to_string();
    let mut reasoning_started = false;
    emit_text_item_started(session, turn_context, assistant_item_id.clone(), false).await;
    emit_delta(session, turn_context, &assistant_item_id, notice, false).await;

    let (prompt_context, history) = {
        let agent = session.as_ref();
        agent
            .record_items(vec![Message::user(MAX_ITERATIONS_SUMMARY_PROMPT)])
            .await;
        (
            agent.prompt_context_history(),
            agent.provider_history().await,
        )
    };

    let raw_stream = match streamer
        .stream_chat_with_contract(prompt, &prompt_context, &history, Vec::new())
        .await
    {
        Ok(s) => s,
        Err(err) => {
            return SummaryOutcome::Failed(format!(
                "迭代预算已用尽（{used}/{max_total}），且总结请求失败: {err}"
            ));
        }
    };

    let (abort_handle, abort_reg) = AbortHandle::new_pair();
    pause.attach_abort(abort_handle);
    let mut stream = Abortable::new(raw_stream, abort_reg);

    let mut full_response = String::new();
    let mut full_reasoning = String::new();
    let mut round_usage: Option<Usage> = None;
    let now_ms = || chrono::Utc::now().timestamp_millis();

    loop {
        if !pause.wait_if_paused().await {
            pause.clear_abort();
            if let Some(u) = round_usage {
                total_usage.add_assign(u);
                *saw_usage = true;
            }
            return SummaryOutcome::Aborted;
        }

        let next = tokio::select! {
            biased;
            _ = pause.wait_cancelled() => {
                pause.clear_abort();
                if let Some(u) = round_usage {
                    total_usage.add_assign(u);
                    *saw_usage = true;
                }
                return SummaryOutcome::Aborted;
            }
            item = stream.next() => item,
        };

        match next {
            None => break,
            Some(Ok(StreamedAssistantContent::Text(text))) => {
                full_response.push_str(&text);
                timeline.push_text_delta(&text, now_ms());
                emit_delta(session, turn_context, &assistant_item_id, text, false).await;
            }
            Some(Ok(StreamedAssistantContent::Reasoning(r))) => {
                full_reasoning.push_str(&r);
                timeline.push_reasoning_delta(&r, now_ms());
                if !reasoning_started {
                    emit_text_item_started(session, turn_context, reasoning_item_id.clone(), true)
                        .await;
                    reasoning_started = true;
                }
                emit_delta(session, turn_context, &reasoning_item_id, r, true).await;
            }
            // 总结轮禁止再调工具：忽略 tool delta
            Some(Ok(StreamedAssistantContent::ToolCallDelta(_))) => {}
            Some(Ok(StreamedAssistantContent::ThoughtSignature(_))) => {}
            Some(Ok(StreamedAssistantContent::Citations(_))) => {}
            Some(Ok(StreamedAssistantContent::InteractionId(_))) => {}
            Some(Ok(StreamedAssistantContent::FinalUsage(u))) => {
                round_usage = Some(u);
            }
            Some(Err(err)) => {
                pause.clear_abort();
                if let Some(u) = round_usage {
                    total_usage.add_assign(u);
                    *saw_usage = true;
                }
                // 持久化已积累的部分回复，保证历史连贯
                if !full_response.is_empty() {
                    let agent = session.as_ref();
                    let details = Some(timeline.reasoning_details_snapshot());
                    if agent
                        .record_assistant_message_with_tools(
                            &full_response,
                            None,
                            (!full_reasoning.is_empty()).then_some(full_reasoning.as_str()),
                            details,
                        )
                        .await
                        .is_ok()
                    {
                        emit_response_items_completed(
                            session,
                            turn_context,
                            true,
                            assistant_item_id,
                            full_response.clone(),
                            reasoning_item_id,
                            full_reasoning.clone(),
                        )
                        .await;
                    }
                }
                return SummaryOutcome::Failed(format!(
                    "迭代预算已用尽（{used}/{max_total}），总结流式失败: {err}"
                ));
            }
        }
    }
    pause.clear_abort();

    if let Some(u) = round_usage {
        total_usage.add_assign(u);
        *saw_usage = true;
    }

    if full_response.trim().is_empty() {
        let fallback = format!(
            "迭代预算已用尽（{used}/{max_total}）。模型未能生成总结，请基于已有工具结果继续或简化任务。"
        );
        full_response = fallback.clone();
        timeline.push_text_delta(&fallback, now_ms());
        emit_delta(session, turn_context, &assistant_item_id, fallback, false).await;
    }

    {
        let agent = session.as_ref();
        let details = Some(timeline.reasoning_details_snapshot());
        if let Err(err) = agent
            .record_assistant_message_with_tools(
                &full_response,
                None,
                (!full_reasoning.is_empty()).then_some(full_reasoning.as_str()),
                details,
            )
            .await
        {
            return SummaryOutcome::Failed(err.to_string());
        }
    }

    emit_response_items_completed(
        session,
        turn_context,
        true,
        assistant_item_id,
        full_response,
        reasoning_item_id,
        full_reasoning,
    )
    .await;

    SummaryOutcome::Finished
}
