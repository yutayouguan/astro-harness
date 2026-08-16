//! 多轮工具循环：从 gRPC handler 收拢到 Agent 层的核心编排。
//!
//! **关键不变量**
//! - Pause/Cancel 对齐 Rig：`wait_if_paused` 先于上游 poll；取消时通过 `Abortable` 中止 Provider 流
//! - 每轮 assistant 回复必须写入 `session_messages`（含 tool_calls）后再执行工具
//! - 迭代预算对齐 Hermes：默认 90 轮；`code_exec` 独占轮可 refund；耗尽后无工具强制总结再 Done
//! - usage 采用覆盖式累加，兼容 Google 等 Provider 的累计式 `usageMetadata`
//!
//! HITL park/resume 桥见 [`super::hitl_bridge`]；预算耗尽后的总结轮见 [`super::summary`]。

use std::sync::Arc;

use futures::stream::{AbortHandle, Abortable};
use futures::StreamExt;
use providers::ProviderConfig;
use providers::{PauseControl, Usage};
use tokio::sync::{mpsc, Mutex};
use types::ChatTarget;

use super::hitl_bridge::{register_live_parent_hitl, unregister_live_parent_hitl, ParentHitlCtx};
use super::lifecycle::{emit, finish_error, finish_usage_and_done};
use super::maintenance::{
    emit_context_usage, post_tool_maintenance, pre_llm_maintenance, record_tool_outcomes,
    stream_chat_with_hooks,
};
use super::provider::ProviderStreamer;
use super::run_state::{RunPhase, RunState};
use super::summary::{run_max_iterations_summary, SummaryOutcome};
use super::tools_exec::{execute_tools_concurrent, execute_tools_serial, terminal_needs_approval};
use super::types::{MultiTurnStream, MultiTurnStreamItem, StreamedAssistantContent};
use crate::control::hitl::HitlGate;
use crate::runtime::AgentLoop;

/// `pre_verify` 单次 turn 内允许的最多验证轮次（含首次结束尝试）。
const MAX_VERIFY_ATTEMPTS: usize = 2;

/// 模型只返回思考/推理内容而没有文本回复时，允许的最大重试次数。
const MAX_THINKING_ONLY_RETRIES: usize = 1;

/// [`run_multi_turn_stream`] 入参打包。
pub struct MultiTurnStreamArgs {
    pub session: Arc<Mutex<AgentLoop>>,
    pub targets: Vec<ChatTarget>,
    pub base_config: ProviderConfig,
    pub system_prompt: String,
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
        system_prompt,
        pause,
        hitl_gate,
        tx,
        chat_override,
    } = args;
    let session_id = {
        let agent = session.lock().await;
        agent.session_id().to_string()
    };
    let run_id = uuid::Uuid::new_v4().to_string();
    let turn_id = run_id.clone();
    {
        let mut agent = session.lock().await;
        agent.set_current_turn_id(turn_id.clone());
    }
    tracing::info!(session_id = %session_id, turn_id = %turn_id, "turn started");
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
    run_multi_turn_stream_inner(MultiTurnStreamInnerArgs {
        session: session.clone(),
        targets,
        base_config,
        system_prompt,
        pause,
        hitl_gate,
        tx,
        thread_id: session_id.clone(),
        run_id: run_id.clone(),
        chat_override,
    })
    .await;
    {
        let mut agent = session.lock().await;
        agent.clear_current_turn_id();
    }
    tracing::info!(session_id = %session_id, turn_id = %run_id, "turn finished");
    unregister_live_parent_hitl(&session_id).await;
}

/// 测试入口：以自定义 chat 函数替代 dispatch，驱动多轮工具循环。
pub async fn run_multi_turn_stream_with_chat_fn(
    session: Arc<Mutex<AgentLoop>>,
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
        system_prompt,
        pause,
        hitl_gate,
        tx,
        chat_override: Some(chat_fn),
    })
    .await;
}

struct MultiTurnStreamInnerArgs {
    session: Arc<Mutex<AgentLoop>>,
    targets: Vec<ChatTarget>,
    base_config: ProviderConfig,
    system_prompt: String,
    pause: Arc<PauseControl>,
    hitl_gate: Option<Arc<HitlGate>>,
    tx: mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    thread_id: String,
    run_id: String,
    chat_override: Option<super::provider::ChatOverride>,
}

async fn run_multi_turn_stream_inner(args: MultiTurnStreamInnerArgs) {
    let MultiTurnStreamInnerArgs {
        session,
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
    let streamer = match chat_override {
        Some(f) => ProviderStreamer::with_chat_override(targets, base_config, f),
        None => ProviderStreamer::new(targets, base_config),
    };
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

    let mut timeline = crate::timeline::TimelineBuilder::new();
    let now_ms = || chrono::Utc::now().timestamp_millis();

    loop {
        raw_rounds += 1;
        if raw_rounds > max_rounds.saturating_mul(2) || !budget.consume() {
            need_summary = true;
            break;
        }
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
        if !pause.wait_if_paused().await {
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

        pre_llm_maintenance(&session).await;

        let (history, tools) = {
            let mut agent = session.lock().await;
            agent.prepare_llm_context().await
        };

        emit_context_usage(&session, &tx, &history, &tools).await;

        let raw_stream = match stream_chat_with_hooks(
            &session,
            &streamer,
            &system_prompt,
            &history,
            tools,
        )
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
        let mut thought_signature: Option<String> = None;
        let mut tool_acc = types::ToolCallAccumulator::new();
        let mut round_usage: Option<Usage> = None;

        loop {
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
                let mut agent = session.lock().await;
                let details = types::message::merge_google_thought_signature(
                    Some(timeline.reasoning_details_snapshot()),
                    thought_signature.as_deref(),
                );
                if let Err(err) = agent.record_assistant_with_calls(
                    &full_response,
                    &[],
                    Some(full_reasoning.as_str()),
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
                if let Err(err) = agent.record_user_message(
                    "[astro:system]\n你的思考过程已记录，但没有生成回复内容。请直接给出你的回答。",
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
                drop(agent);
                continue;
            }
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

        // `pre_verify` hook
        if calls.is_empty() {
            let verify_outcome = {
                let agent = session.lock().await;
                if agent.turn_wrote_disk() && verify_attempt < MAX_VERIFY_ATTEMPTS {
                    verify_attempt += 1;
                    let sid = agent.session_id().to_string();
                    let turn_id = agent.current_turn_id().map(str::to_string);
                    Some(agent.fire_hook(
                        ::hooks::PRE_VERIFY,
                        ::hooks::HookPayload {
                            session_id: sid,
                            turn_id,
                            message: Some(full_response.clone()),
                            detail: format!("attempt={verify_attempt}"),
                            ..Default::default()
                        },
                    ))
                } else {
                    None
                }
            };
            if let Some(::hooks::HookOutcome::KeepGoing(prompt)) = verify_outcome {
                let mut agent = session.lock().await;
                let details = types::message::merge_google_thought_signature(
                    Some(timeline.reasoning_details_snapshot()),
                    thought_signature.as_deref(),
                );
                if let Err(err) = agent.record_assistant_with_calls(
                    &full_response,
                    &[],
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
                if let Err(err) =
                    agent.record_user_message(&format!("[astro:hook-context]\n{prompt}"))
                {
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
                drop(agent);
                continue;
            }
        }

        {
            let agent = session.lock().await;
            let sid = agent.session_id().to_string();
            let turn_id = agent.current_turn_id().map(str::to_string);
            let transformed = agent.fire_hook(
                ::hooks::TRANSFORM_LLM_OUTPUT,
                ::hooks::HookPayload {
                    session_id: sid.clone(),
                    turn_id: turn_id.clone(),
                    message: Some(full_response.clone()),
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
            drop(agent);
            if cancelled {
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
            for c in &calls {
                timeline.upsert_activity(&c.id, now_ms());
            }
            let details = types::message::merge_google_thought_signature(
                Some(timeline.reasoning_details_snapshot()),
                thought_signature.as_deref(),
            );
            if let Err(err) = agent.record_assistant_with_calls(
                &full_response,
                &calls,
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
            need_summary = false;
            break;
        }

        run_state.set_phase(RunPhase::ExecutingTools);
        let force_serial = {
            let agent = session.lock().await;
            let names: Vec<&str> = calls.iter().map(|c| c.name.as_str()).collect();
            agent.tool_registry().any_needs_confirmation(&names)
                || agent.tool_registry().any_exclusive_access(&names)
                || calls
                    .iter()
                    .any(|c| terminal_needs_approval(&c.name, &c.arguments))
        };

        let outcomes = if force_serial || hitl_gate.is_none() {
            execute_tools_serial(&session, &calls, &pause, &tx, &run_id, hitl_gate.as_ref()).await
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

        if post_tool_maintenance(&session, &calls).await {
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
                )
                .await;
                return;
            }
        }
    }

    {
        let agent = session.lock().await;
        let sid = agent.session_id().to_string();
        let turn = agent.session_turn();
        let turn_id = agent.current_turn_id().map(str::to_string);
        let _ = agent.fire_hook(
            ::hooks::ON_SESSION_END,
            ::hooks::HookPayload {
                session_id: sid,
                turn_id,
                turn: Some(turn),
                detail: format!("turn={turn}"),
                ..Default::default()
            },
        );
    }

    finish_usage_and_done(
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
    session: Arc<Mutex<AgentLoop>>,
    targets: Vec<ChatTarget>,
    base_config: ProviderConfig,
    system_prompt: String,
    pause: Arc<PauseControl>,
) -> MultiTurnStream {
    stream_multi_turn_with_hitl(session, targets, base_config, system_prompt, pause, None)
}

/// 带 HITL 闸门的多轮流。
pub fn stream_multi_turn_with_hitl(
    session: Arc<Mutex<AgentLoop>>,
    targets: Vec<ChatTarget>,
    base_config: ProviderConfig,
    system_prompt: String,
    pause: Arc<PauseControl>,
    hitl_gate: Option<Arc<HitlGate>>,
) -> MultiTurnStream {
    let (tx, rx) = mpsc::channel(32);
    tokio::spawn(async move {
        run_multi_turn_stream(MultiTurnStreamArgs {
            session,
            targets,
            base_config,
            system_prompt,
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
