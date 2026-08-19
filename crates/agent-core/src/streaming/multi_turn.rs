//! 多轮工具循环：从 gRPC handler 收拢到 Agent 层的核心编排。
//!
//! **关键不变量**
//! - Pause/Cancel 对齐 Rig：`wait_if_paused` 先于上游 poll；取消时通过 `Abortable` 中止 Provider 流
//! - 每轮 assistant 回复必须写入 `SessionState.history`（含 tool_calls）后再执行工具
//! - 迭代预算对齐 Hermes：默认 90 轮；`code_exec` 独占轮可 refund；耗尽后无工具强制总结再 Done
//! - usage 采用覆盖式累加，兼容 Google 等 Provider 的累计式 `usageMetadata`
//!
//! HITL park/resume 桥见 [`super::hitl_bridge`]；预算耗尽后的总结轮见 [`super::summary`]。

use std::sync::Arc;

use futures::stream::{AbortHandle, Abortable};
use futures::StreamExt;
use providers::ProviderConfig;
use providers::{PauseControl, Usage};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use types::ChatTarget;

use super::lifecycle::{emit, finish_error, finish_interrupted, finish_success};
use super::maintenance::{
    emit_context_usage, post_tool_maintenance, pre_llm_maintenance, record_tool_outcomes,
    run_sampling_request,
};
use super::provider::ProviderStreamer;
use super::run_state::{RunPhase, RunState};
use super::summary::{run_max_iterations_summary, SummaryOutcome};
use super::tools_exec::{
    execute_tools_concurrent, execute_tools_serial, tool_may_require_permission,
};
use super::types::{MultiTurnStream, MultiTurnStreamItem, StreamedAssistantContent};
use crate::control::hitl::HitlGate;
use crate::runtime::{Session, TurnContext};
use crate::tasks::{RegularTask, TurnInput};

/// `pre_verify` 单次 turn 内允许的最多验证轮次（含首次结束尝试）。
const MAX_VERIFY_ATTEMPTS: usize = 2;

/// 模型只返回思考/推理内容而没有文本回复时，允许的最大重试次数。
const MAX_THINKING_ONLY_RETRIES: usize = 1;

/// [`run_multi_turn_stream`] 入参打包。
pub struct MultiTurnStreamArgs {
    pub session: Arc<Session>,
    pub targets: Vec<ChatTarget>,
    pub base_config: ProviderConfig,
    pub input: Vec<TurnInput>,
    /// Compatibility path for tests and callers that already prepared a turn.
    pub system_prompt: Option<String>,
    pub pause: Arc<PauseControl>,
    pub hitl_gate: Option<Arc<HitlGate>>,
    pub tx: mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    /// 测试覆盖：非空时跳过 dispatch，直接使用此函数获取 CompletionStream。
    pub chat_override: Option<super::provider::ChatOverride>,
}

/// 多轮工具调用流式循环：从 gRPC handler 收拢到 Agent 层的核心编排。
///
/// 每轮：锁定 session → 流式 LLM → 累积 tool_calls → 执行工具 → 写入历史 → 下一轮。
/// 取消/暂停时清理 abort handle 并以 usage + Done 收尾。
/// `hitl_gate` 非空时，confirm/clarify/危险命令在同回合 park，不结束 run。
pub async fn run_multi_turn_stream(args: MultiTurnStreamArgs) {
    let MultiTurnStreamArgs {
        session,
        targets,
        base_config,
        input,
        system_prompt,
        pause,
        hitl_gate,
        tx,
        chat_override,
    } = args;
    let session_id = {
        let agent = session.as_ref();
        agent.session_id().to_string()
    };
    let sub_id = uuid::Uuid::new_v4().to_string();
    let turn_context = {
        let sess = session.as_ref();
        sess.create_turn_context(sub_id.clone()).await
    };
    let task = RegularTask::new(RunTurnArgs {
        session: session.clone(),
        turn_context: Arc::clone(&turn_context),
        targets,
        base_config,
        system_prompt,
        pause,
        hitl_gate,
        tx: tx.clone(),
        thread_id: session_id.clone(),
        run_id: sub_id.clone(),
        chat_override,
    });
    tracing::info!(session_id = %session_id, turn_id = %sub_id, "turn started");
    if let Err(error) = session.spawn_task(turn_context, input, task).await {
        let _ = tx
            .send(Ok(MultiTurnStreamItem::Error(error.to_string())))
            .await;
        let _ = tx.send(Ok(MultiTurnStreamItem::Done)).await;
    }
    tracing::info!(session_id = %session_id, turn_id = %sub_id, "turn finished");
}

/// 测试入口：以自定义 chat 函数替代 dispatch，驱动多轮工具循环。
pub async fn run_multi_turn_stream_with_chat_fn(
    session: Arc<Session>,
    chat_fn: super::provider::ChatOverride,
    config: ProviderConfig,
    system_prompt: String,
    pause: Arc<PauseControl>,
    hitl_gate: Option<Arc<HitlGate>>,
    tx: mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
) {
    let target = ChatTarget {
        provider_id: "scripted".into(),
        backend_id: "scripted".into(),
        model: config.model.clone(),
        api_key: config.api_key.clone(),
        base_url: config.base_url.clone().unwrap_or_default(),
    };
    let targets = vec![target];
    run_multi_turn_stream(MultiTurnStreamArgs {
        session,
        targets,
        base_config: config,
        input: Vec::new(),
        system_prompt: Some(system_prompt),
        pause,
        hitl_gate,
        tx,
        chat_override: Some(chat_fn),
    })
    .await;
}

#[derive(Clone)]
pub(crate) struct RunTurnArgs {
    session: Arc<Session>,
    turn_context: Arc<TurnContext>,
    targets: Vec<ChatTarget>,
    base_config: ProviderConfig,
    system_prompt: Option<String>,
    pause: Arc<PauseControl>,
    hitl_gate: Option<Arc<HitlGate>>,
    tx: mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    thread_id: String,
    run_id: String,
    chat_override: Option<super::provider::ChatOverride>,
}

impl RunTurnArgs {
    pub(crate) fn with_session_and_turn(
        &self,
        session: Arc<Session>,
        turn_context: Arc<TurnContext>,
    ) -> Self {
        Self {
            session,
            turn_context,
            thread_id: self.thread_id.clone(),
            ..self.clone()
        }
    }

    pub(crate) fn with_system_prompt(&self, system_prompt: String) -> Self {
        Self {
            system_prompt: Some(system_prompt),
            ..self.clone()
        }
    }

    pub(crate) fn prepared_system_prompt(&self) -> Option<&str> {
        self.system_prompt.as_deref()
    }
}

async fn record_pending_input(
    session: &Arc<Session>,
    pending_input: Vec<TurnInput>,
) -> anyhow::Result<()> {
    if pending_input.is_empty() {
        return Ok(());
    }
    let sess = session.as_ref();
    sess.record_turn_inputs(pending_input).await
}

/// Codex-aligned regular turn loop shared by foreground and background adapters.
pub(crate) async fn run_turn(args: RunTurnArgs, cancellation_token: CancellationToken) {
    let RunTurnArgs {
        session,
        turn_context,
        targets,
        base_config,
        system_prompt,
        pause,
        hitl_gate,
        tx,
        thread_id,
        run_id,
        chat_override,
    } = args;
    let system_prompt = system_prompt.expect("RegularTask prepares the system prompt");
    let streamer = match chat_override {
        Some(f) => ProviderStreamer::with_chat_override(targets, base_config, f),
        None => ProviderStreamer::new(targets, base_config),
    };
    let mut total_usage = Usage::default();
    let mut saw_usage = false;

    {
        let agent = session.as_ref();
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
        let agent = session.as_ref();
        let n = agent.multi_turn();
        if n == 0 {
            crate::runtime::budget::DEFAULT_MAX_ITERATIONS
        } else {
            n
        }
    };
    let budget = crate::runtime::budget::IterationBudget::new(max_rounds);
    let mut run_state = RunState::new();
    let need_summary;
    let mut raw_rounds: usize = 0;
    let mut verify_attempt: usize = 0;
    let mut thinking_only_retries: usize = 0;
    let mut has_sampled = false;
    let mut defer_pending_input_after_stop = false;

    let mut timeline = crate::timeline::TimelineBuilder::new();
    let now_ms = || chrono::Utc::now().timestamp_millis();

    loop {
        raw_rounds += 1;
        if raw_rounds > max_rounds.saturating_mul(2) || !budget.consume() {
            need_summary = true;
            break;
        }
        if cancellation_token.is_cancelled() || pause.is_cancelled() {
            finish_interrupted(
                &session,
                &streamer,
                &tx,
                saw_usage.then_some(total_usage),
                &run_id,
            )
            .await;
            return;
        }
        if !pause.wait_if_paused().await {
            finish_interrupted(
                &session,
                &streamer,
                &tx,
                saw_usage.then_some(total_usage),
                &run_id,
            )
            .await;
            return;
        }

        if has_sampled && !defer_pending_input_after_stop {
            if let Err(error) =
                record_pending_input(&session, turn_context.take_pending_input()).await
            {
                finish_error(
                    &session,
                    &streamer,
                    &tx,
                    error.to_string(),
                    saw_usage.then_some(total_usage),
                    &run_id,
                )
                .await;
                return;
            }
        }

        pre_llm_maintenance(&session).await;

        let step_context = {
            let agent = session.as_ref();
            agent.capture_step_context().await
        };
        let step_context = match step_context {
            Ok(step_context) => step_context,
            Err(error) => {
                finish_error(
                    &session,
                    &streamer,
                    &tx,
                    error.to_string(),
                    saw_usage.then_some(total_usage),
                    &run_id,
                )
                .await;
                return;
            }
        };
        tracing::debug!(
            sub_id = %step_context.turn.sub_id(),
            turn = step_context.turn.turn(),
            "step context captured"
        );
        let history = step_context.history.clone();
        let tool_specs = step_context.tool_router.model_visible_specs().to_vec();

        emit_context_usage(&session, &tx, &history, &tool_specs).await;

        let raw_stream =
            match run_sampling_request(&session, &streamer, &system_prompt, &history, tool_specs)
                .await
            {
                Ok(s) => s,
                Err(err) => {
                    finish_error(
                        &session,
                        &streamer,
                        &tx,
                        err,
                        saw_usage.then_some(total_usage),
                        &run_id,
                    )
                    .await;
                    return;
                }
            };
        has_sampled = true;

        let (abort_handle, abort_reg) = AbortHandle::new_pair();
        pause.attach_abort(abort_handle);
        let mut stream = Abortable::new(raw_stream, abort_reg);

        let mut full_response = String::new();
        let mut full_reasoning = String::new();
        let mut thought_signature: Option<String> = None;
        let mut tool_acc = types::ToolCallAccumulator::new();
        let mut round_usage: Option<Usage> = None;

        loop {
            if !pause.wait_if_paused().await {
                pause.clear_abort();
                finish_interrupted(
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
                _ = cancellation_token.cancelled() => {
                    pause.clear_abort();
                    finish_interrupted(
                        &session,
                        &streamer,
                        &tx,
                        saw_usage.then_some(total_usage),
                        &run_id,
                    )
                    .await;
                    return;
                }
                _ = pause.wait_cancelled() => {
                    pause.clear_abort();
                    finish_interrupted(
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
                None => break,
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
                Some(Ok(StreamedAssistantContent::ThoughtSignature(sig))) => {
                    thought_signature = Some(sig);
                }
                Some(Ok(StreamedAssistantContent::ToolCallDelta(d))) => {
                    tool_acc.push(&d);
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
                    round_usage = Some(u);
                }
                Some(Ok(StreamedAssistantContent::Citations(cites))) => {
                    let _ = emit(
                        &tx,
                        MultiTurnStreamItem::Assistant(StreamedAssistantContent::Citations(cites)),
                    )
                    .await;
                }
                Some(Ok(StreamedAssistantContent::InteractionId(_))) => {}
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
                        &run_id,
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
            finish_interrupted(
                &session,
                &streamer,
                &tx,
                saw_usage.then_some(total_usage),
                &run_id,
            )
            .await;
            return;
        }

        if let Some(u) = round_usage {
            total_usage.add_assign(u);
            saw_usage = true;
        }

        let native_calls = tool_acc.finish();
        let calls = types::resolve_tool_calls(native_calls, &full_response);

        if full_response.is_empty() && calls.is_empty() {
            if !full_reasoning.is_empty() && thinking_only_retries < MAX_THINKING_ONLY_RETRIES {
                thinking_only_retries += 1;
                tracing::warn!(
                    reasoning_len = full_reasoning.len(),
                    attempt = thinking_only_retries,
                    "model returned reasoning only with no text; injecting retry prompt"
                );
                let agent = session.as_ref();
                let details = types::message::merge_google_thought_signature(
                    Some(timeline.reasoning_details_snapshot()),
                    thought_signature.as_deref(),
                );
                if let Err(err) = agent
                    .record_assistant_with_calls(
                        &full_response,
                        &[],
                        Some(full_reasoning.as_str()),
                        details,
                    )
                    .await
                {
                    finish_error(
                        &session,
                        &streamer,
                        &tx,
                        err.to_string(),
                        saw_usage.then_some(total_usage),
                        &run_id,
                    )
                    .await;
                    return;
                }
                if let Err(err) = agent.record_user_message(
                    "[astro:system]\n你的思考过程已记录，但没有生成回复内容。请直接给出你的回答。",
                )
                .await
                {
                    finish_error(
                        &session,
                        &streamer,
                        &tx,
                        err.to_string(),
                        saw_usage.then_some(total_usage),
                        &run_id,
                    )
                    .await;
                    return;
                }
                continue;
            }
            finish_error(
                &session,
                &streamer,
                &tx,
                "模型返回了空回复。请重试，或换一个模型。",
                saw_usage.then_some(total_usage),
                &run_id,
            )
            .await;
            return;
        }

        // `Stop` hook
        if calls.is_empty() {
            let verify_outcome = if verify_attempt < MAX_VERIFY_ATTEMPTS {
                let agent = session.as_ref();
                let sid = agent.session_id().to_string();
                let turn_id = agent.current_turn_id().await;
                Some(agent.fire_hook(
                    ::hooks::STOP,
                    ::hooks::HookPayload {
                        session_id: sid,
                        turn_id,
                        stop_hook_active: Some(verify_attempt > 0),
                        last_assistant_message: Some(full_response.clone()),
                        detail: format!("attempt={}", verify_attempt + 1),
                        ..Default::default()
                    },
                ))
            } else {
                None
            };
            if let Some(::hooks::HookOutcome::KeepGoing(prompt)) = verify_outcome {
                verify_attempt += 1;
                let agent = session.as_ref();
                let details = types::message::merge_google_thought_signature(
                    Some(timeline.reasoning_details_snapshot()),
                    thought_signature.as_deref(),
                );
                if let Err(err) = agent
                    .record_assistant_with_calls(
                        &full_response,
                        &[],
                        (!full_reasoning.is_empty()).then_some(full_reasoning.as_str()),
                        details,
                    )
                    .await
                {
                    finish_error(
                        &session,
                        &streamer,
                        &tx,
                        err.to_string(),
                        saw_usage.then_some(total_usage),
                        &run_id,
                    )
                    .await;
                    return;
                }
                if let Err(err) = agent
                    .record_user_message(&format!("[astro:hook-context]\n{prompt}"))
                    .await
                {
                    finish_error(
                        &session,
                        &streamer,
                        &tx,
                        err.to_string(),
                        saw_usage.then_some(total_usage),
                        &run_id,
                    )
                    .await;
                    return;
                }
                defer_pending_input_after_stop = true;
                continue;
            }
        }

        {
            let agent = session.as_ref();
            let sid = agent.session_id().to_string();
            let turn_id = agent.current_turn_id().await;
            let transformed = agent.fire_hook(
                ::hooks::TRANSFORM_LLM_OUTPUT,
                ::hooks::HookPayload {
                    session_id: sid.clone(),
                    turn_id: turn_id.clone(),
                    last_assistant_message: Some(full_response.clone()),
                    assistant_chars: Some(full_response.len()),
                    detail: format!("assistant_chars={}", full_response.len()),
                    ..Default::default()
                },
            );
            if let ::hooks::HookOutcome::ReplaceText(s) = transformed {
                full_response = s;
            }
            let _ = agent.fire_hook(
                ::hooks::POST_LLM_CALL,
                ::hooks::HookPayload {
                    session_id: sid,
                    turn_id,
                    assistant_chars: Some(full_response.len()),
                    detail: format!("assistant_chars={}", full_response.len()),
                    ..Default::default()
                },
            );
            let cancelled = agent.cancel_signal().is_cancelled();
            if cancelled {
                finish_interrupted(
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
            let agent = session.as_ref();
            for c in &calls {
                timeline.upsert_activity(&c.id, now_ms());
            }
            let details = types::message::merge_google_thought_signature(
                Some(timeline.reasoning_details_snapshot()),
                thought_signature.as_deref(),
            );
            if let Err(err) = agent
                .record_assistant_with_calls(
                    &full_response,
                    &calls,
                    (!full_reasoning.is_empty()).then_some(full_reasoning.as_str()),
                    details,
                )
                .await
            {
                finish_error(
                    &session,
                    &streamer,
                    &tx,
                    err.to_string(),
                    saw_usage.then_some(total_usage),
                    &run_id,
                )
                .await;
                return;
            }
            // A Stop bridge can survive reasoning-only retries. Clear its one-shot
            // deferral only after a normal assistant message (including tool calls)
            // is durably represented in history.
            defer_pending_input_after_stop = false;
        }

        if calls.is_empty() {
            let pending_input = turn_context.take_pending_input_or_close().await;
            if !pending_input.is_empty() {
                if let Err(error) = record_pending_input(&session, pending_input).await {
                    finish_error(
                        &session,
                        &streamer,
                        &tx,
                        error.to_string(),
                        saw_usage.then_some(total_usage),
                        &run_id,
                    )
                    .await;
                    return;
                }
                run_state.set_phase(RunPhase::StreamingLlm);
                continue;
            }
            need_summary = false;
            break;
        }

        for call in &calls {
            if !emit(
                &tx,
                MultiTurnStreamItem::ToolStarted {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    arguments_json: call.arguments.to_string(),
                },
            )
            .await
            {
                return;
            }
        }

        run_state.set_phase(RunPhase::ExecutingTools);
        let names: Vec<&str> = calls.iter().map(|c| c.name.as_str()).collect();
        let force_serial = step_context.tool_router.any_needs_confirmation(&names)
            || step_context.tool_router.any_exclusive_access(&names)
            || calls
                .iter()
                .any(|c| tool_may_require_permission(&c.name, &c.arguments));

        let outcomes = if force_serial || hitl_gate.is_none() {
            execute_tools_serial(
                &session,
                Arc::clone(&step_context),
                &calls,
                &pause,
                &tx,
                &run_id,
                hitl_gate.as_ref(),
            )
            .await
        } else {
            execute_tools_concurrent(
                &session,
                Arc::clone(&step_context),
                &calls,
                &pause,
                cancellation_token.child_token(),
            )
            .await
        };

        let Some(outcomes) = outcomes else {
            finish_interrupted(
                &session,
                &streamer,
                &tx,
                saw_usage.then_some(total_usage),
                &run_id,
            )
            .await;
            return;
        };

        if !record_tool_outcomes(
            &session,
            &calls,
            outcomes,
            &pause,
            &tx,
            &mut timeline,
            now_ms,
        )
        .await
        {
            finish_interrupted(
                &session,
                &streamer,
                &tx,
                saw_usage.then_some(total_usage),
                &run_id,
            )
            .await;
            return;
        }

        if post_tool_maintenance(&session, &step_context, &calls).await {
            need_summary = false;
            break;
        }

        let names: Vec<&str> = calls.iter().map(|c| c.name.as_str()).collect();
        if crate::runtime::budget::should_refund_tool_round(&names) {
            budget.refund();
        }
        if budget.remaining() == 0 {
            need_summary = true;
            break;
        }
        run_state.set_phase(RunPhase::StreamingLlm);
    }

    if need_summary {
        match run_max_iterations_summary(crate::streaming::summary::MaxIterationsSummaryArgs {
            session: &session,
            streamer: &streamer,
            system_prompt: &system_prompt,
            pause: &pause,
            tx: &tx,
            timeline: &mut timeline,
            total_usage: &mut total_usage,
            saw_usage: &mut saw_usage,
            run_id: &run_id,
            used: budget.used(),
            max_total: budget.max_total(),
        })
        .await
        {
            SummaryOutcome::Finished => {}
            SummaryOutcome::Aborted => return,
            SummaryOutcome::Failed(err) => {
                finish_error(
                    &session,
                    &streamer,
                    &tx,
                    err,
                    saw_usage.then_some(total_usage),
                    &run_id,
                )
                .await;
                return;
            }
        }
    }

    {
        let agent = session.as_ref();
        let sid = agent.session_id().to_string();
        let turn = agent.session_turn().await;
        let turn_id = agent.current_turn_id().await;
        let _ = agent.fire_hook(
            ::hooks::AGENT_END,
            ::hooks::HookPayload {
                session_id: sid,
                turn_id,
                turn: Some(turn),
                detail: format!("turn={turn}"),
                ..Default::default()
            },
        );
    }

    finish_success(
        &session,
        &streamer,
        &tx,
        saw_usage.then_some(total_usage),
        &run_id,
    )
    .await;
}

/// 在后台 task 启动 [`run_multi_turn_stream`]，并返回可消费的 [`MultiTurnStream`]。
///
/// channel 容量为 32；消费者 drop 后发送方通过 [`emit`] 返回 `false` 自然退出。
pub fn stream_multi_turn(
    session: Arc<Session>,
    targets: Vec<ChatTarget>,
    base_config: ProviderConfig,
    input: Vec<TurnInput>,
    pause: Arc<PauseControl>,
) -> MultiTurnStream {
    stream_multi_turn_with_hitl(session, targets, base_config, input, pause, None)
}

/// 带 HITL 闸门的多轮流。
pub fn stream_multi_turn_with_hitl(
    session: Arc<Session>,
    targets: Vec<ChatTarget>,
    base_config: ProviderConfig,
    input: Vec<TurnInput>,
    pause: Arc<PauseControl>,
    hitl_gate: Option<Arc<HitlGate>>,
) -> MultiTurnStream {
    let (tx, rx) = mpsc::channel(32);
    tokio::spawn(async move {
        run_multi_turn_stream(MultiTurnStreamArgs {
            session,
            targets,
            base_config,
            input,
            system_prompt: None,
            pause,
            hitl_gate,
            tx,
            chat_override: None,
        })
        .await;
    });
    Box::pin(futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    }))
}
