//! 迭代预算耗尽后的强制总结轮（对齐 Hermes `handle_max_iterations`）。

use std::sync::Arc;

use futures::stream::{AbortHandle, Abortable};
use futures::StreamExt;
use providers::{PauseControl, Usage};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::runtime::{AgentLoop, TurnContext};

use super::lifecycle::{emit, finish_interrupted};
use super::provider::ProviderStreamer;
use super::traits::StreamingChat;
use super::types::{MultiTurnStreamItem, StreamedAssistantContent};

/// 预算耗尽后注入的总结提示（对齐 Hermes `handle_max_iterations`）。
const MAX_ITERATIONS_SUMMARY_PROMPT: &str = "\
你已达到本回合允许的最大工具调用迭代次数。\
请直接给出最终回复，总结目前已完成与尚未完成的内容，不要再调用任何工具。";

/// `Stop` hook 单个 response chain 内允许的最多 KeepGoing continuation 次数。
///
/// 每个 terminal candidate 仍会 dispatch Stop；此上限只决定是否接受其
/// KeepGoing outcome。
pub(crate) const MAX_VERIFY_ATTEMPTS: usize = 2;

pub(crate) enum SummaryOutcome {
    Finished,
    Aborted,
    Failed(String),
}

/// [`run_max_iterations_summary`] 入参打包。
pub(crate) struct MaxIterationsSummaryArgs<'a> {
    pub session: &'a Arc<AgentLoop>,
    pub turn_context: &'a Arc<TurnContext>,
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
    /// Shares the response-chain quota accumulated by the normal turn loop.
    pub verify_attempt: &'a mut usize,
    /// The owning regular-task cancellation must also gate summary setup.
    pub cancellation_token: &'a CancellationToken,
}

/// 预算耗尽后：注入总结提示，再发一轮 **无 tools** 的 completion（对齐 Hermes）。
pub(crate) async fn run_max_iterations_summary(a: MaxIterationsSummaryArgs<'_>) -> SummaryOutcome {
    let MaxIterationsSummaryArgs {
        session,
        turn_context,
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
        verify_attempt,
        cancellation_token,
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

    // The summary instruction is request-local: persisting it as a user message
    // could produce user/user after a main-loop Stop bridge, and would leave a
    // synthetic dangling user on setup or provider failure.
    let summary_system_prompt = format!("{system_prompt}\n\n{MAX_ITERATIONS_SUMMARY_PROMPT}");

    loop {
        if cancellation_token.is_cancelled() || pause.is_cancelled() {
            finish_interrupted(
                session,
                streamer,
                tx,
                saw_usage.then_some(*total_usage),
                run_id,
            )
            .await;
            return SummaryOutcome::Aborted;
        }

        let step_context = {
            let agent = session.as_ref();
            tokio::select! {
                biased;
                _ = cancellation_token.cancelled() => {
                    finish_interrupted(
                        session,
                        streamer,
                        tx,
                        saw_usage.then_some(*total_usage),
                        run_id,
                    ).await;
                    return SummaryOutcome::Aborted;
                }
                _ = pause.wait_cancelled() => {
                    finish_interrupted(
                        session,
                        streamer,
                        tx,
                        saw_usage.then_some(*total_usage),
                        run_id,
                    ).await;
                    return SummaryOutcome::Aborted;
                }
                result = agent.capture_step_context() => result,
            }
        };
        let history = match step_context {
            Ok(step_context) => step_context.history.clone(),
            Err(err) => {
                return SummaryOutcome::Failed(format!(
                    "迭代预算已用尽（{used}/{max_total}），且总结请求准备失败: {err}"
                ));
            }
        };
        let stream_result = tokio::select! {
            biased;
            _ = cancellation_token.cancelled() => {
                finish_interrupted(
                    session,
                    streamer,
                    tx,
                    saw_usage.then_some(*total_usage),
                    run_id,
                ).await;
                return SummaryOutcome::Aborted;
            }
            _ = pause.wait_cancelled() => {
                finish_interrupted(
                    session,
                    streamer,
                    tx,
                    saw_usage.then_some(*total_usage),
                    run_id,
                ).await;
                return SummaryOutcome::Aborted;
            }
            result = streamer.stream_chat(&summary_system_prompt, &history, Vec::new()) => result,
        };
        let raw_stream = match stream_result {
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
                finish_interrupted(
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
                _ = cancellation_token.cancelled() => {
                    pause.clear_abort();
                    if let Some(u) = round_usage {
                        total_usage.add_assign(u);
                        *saw_usage = true;
                    }
                    finish_interrupted(
                        session,
                        streamer,
                        tx,
                        saw_usage.then_some(*total_usage),
                        run_id,
                    )
                    .await;
                    return SummaryOutcome::Aborted;
                }
                _ = pause.wait_cancelled() => {
                    pause.clear_abort();
                    if let Some(u) = round_usage {
                        total_usage.add_assign(u);
                        *saw_usage = true;
                    }
                    finish_interrupted(
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
                        let agent = session.as_ref();
                        let details = Some(timeline.reasoning_details_snapshot());
                        let _ = agent
                            .record_assistant_message_with_tools(
                                &full_response,
                                None,
                                (!full_reasoning.is_empty()).then_some(full_reasoning.as_str()),
                                details,
                            )
                            .await;
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

        let verify_outcome = {
            let agent = session.as_ref();
            let sid = agent.session_id().to_string();
            let turn_id = agent.current_turn_id().await;
            agent.fire_hook(
                ::hooks::STOP,
                ::hooks::HookPayload {
                    session_id: sid,
                    turn_id,
                    stop_hook_active: Some(*verify_attempt > 0),
                    last_assistant_message: Some(full_response.clone()),
                    detail: format!("attempt={}", *verify_attempt + 1),
                    ..Default::default()
                },
            )
        };

        if *verify_attempt < MAX_VERIFY_ATTEMPTS {
            if let ::hooks::HookOutcome::KeepGoing(prompt) = verify_outcome {
                *verify_attempt += 1;
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
                if let Err(err) = agent
                    .record_user_message(&format!("[astro:hook-context]\n{prompt}"))
                    .await
                {
                    return SummaryOutcome::Failed(err.to_string());
                }
                continue;
            }
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

        // A steer can be admitted while a Stop hook is deciding whether this
        // summary candidate is terminal. Close admission only after persisting
        // the assistant reply, then either sample the queued response chain or
        // finish without dropping an acknowledged input.
        let pending_input = turn_context.take_pending_input_or_close().await;
        if !pending_input.is_empty() {
            let agent = session.as_ref();
            if let Err(err) = agent.record_queued_turn_inputs(pending_input).await {
                return SummaryOutcome::Failed(err.to_string());
            }
            *verify_attempt = 0;
            continue;
        }

        return SummaryOutcome::Finished;
    }
}
