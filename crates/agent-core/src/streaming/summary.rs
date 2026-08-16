//! 迭代预算耗尽后的强制总结轮（对齐 Hermes `handle_max_iterations`）。

use std::sync::Arc;

use futures::stream::{AbortHandle, Abortable};
use futures::StreamExt;
use providers::{PauseControl, Usage};
use tokio::sync::{mpsc, Mutex};
use types::message::Message;

use crate::runtime::AgentLoop;

use super::lifecycle::{emit, finish_usage_and_done};
use super::provider::ProviderStreamer;
use super::traits::StreamingChat;
use super::types::{MultiTurnStreamItem, StreamedAssistantContent};

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
    pub session: &'a Arc<Mutex<AgentLoop>>,
    pub streamer: &'a ProviderStreamer,
    pub system_prompt: &'a str,
    pub pause: &'a Arc<PauseControl>,
    pub tx: &'a mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    pub timeline: &'a mut crate::timeline::TimelineBuilder,
    pub total_usage: &'a mut Usage,
    pub saw_usage: &'a mut bool,
    pub run_id: &'a str,
    pub used: usize,
    pub max_total: usize,
}

/// 预算耗尽后：注入总结提示，再发一轮 **无 tools** 的 completion（对齐 Hermes）。
pub(crate) async fn run_max_iterations_summary(a: MaxIterationsSummaryArgs<'_>) -> SummaryOutcome {
    let MaxIterationsSummaryArgs {
        session,
        streamer,
        system_prompt,
        pause,
        tx,
        timeline,
        total_usage,
        saw_usage,
        run_id,
        used,
        max_total,
    } = a;
    let notice =
        format!("⚠️ 迭代预算已用尽（{used}/{max_total}），正在请求模型总结（不再调用工具）…\n\n");
    if !emit(
        tx,
        MultiTurnStreamItem::Assistant(StreamedAssistantContent::Text(notice)),
    )
    .await
    {
        return SummaryOutcome::Aborted;
    }

    let history = {
        let mut agent = session.lock().await;
        agent
            .session_messages
            .push(Message::user(MAX_ITERATIONS_SUMMARY_PROMPT));
        agent.session_messages.clone()
    };

    let raw_stream = match streamer
        .stream_chat(system_prompt, &history, Vec::new())
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
            finish_usage_and_done(
                session,
                streamer,
                tx,
                saw_usage.then_some(*total_usage),
                run_id,
            )
            .await;
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
                finish_usage_and_done(
                    session,
                    streamer,
                    tx,
                    saw_usage.then_some(*total_usage),
                    run_id,
                )
                .await;
                return SummaryOutcome::Aborted;
            }
            item = stream.next() => item,
        };

        match next {
            None => break,
            Some(Ok(StreamedAssistantContent::Text(text))) => {
                full_response.push_str(&text);
                if !emit(
                    tx,
                    MultiTurnStreamItem::Assistant(StreamedAssistantContent::Text(text)),
                )
                .await
                {
                    pause.clear_abort();
                    return SummaryOutcome::Aborted;
                }
            }
            Some(Ok(StreamedAssistantContent::Reasoning(r))) => {
                full_reasoning.push_str(&r);
                timeline.push_reasoning_delta(&r, now_ms());
                if !emit(
                    tx,
                    MultiTurnStreamItem::Assistant(StreamedAssistantContent::Reasoning(r)),
                )
                .await
                {
                    pause.clear_abort();
                    return SummaryOutcome::Aborted;
                }
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
                    let mut agent = session.lock().await;
                    let details = Some(timeline.reasoning_details_snapshot());
                    let _ = agent.record_assistant_message_with_tools(
                        &full_response,
                        None,
                        (!full_reasoning.is_empty()).then_some(full_reasoning.as_str()),
                        details,
                    );
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
        if !emit(
            tx,
            MultiTurnStreamItem::Assistant(StreamedAssistantContent::Text(fallback)),
        )
        .await
        {
            return SummaryOutcome::Aborted;
        }
    }

    {
        let mut agent = session.lock().await;
        let details = Some(timeline.reasoning_details_snapshot());
        if let Err(err) = agent.record_assistant_message_with_tools(
            &full_response,
            None,
            (!full_reasoning.is_empty()).then_some(full_reasoning.as_str()),
            details,
        ) {
            return SummaryOutcome::Failed(err.to_string());
        }
    }

    SummaryOutcome::Finished
}
