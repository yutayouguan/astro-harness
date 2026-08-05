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

use common::ChatTarget;
use futures::stream::{AbortHandle, Abortable};
use futures::StreamExt;
use providers::{PauseControl, Usage};
use providers::ProviderConfig;
use tokio::sync::{mpsc, Mutex};

use super::run_state::{RunPhase, RunState};
use crate::control::hitl::HitlGate;
use crate::runtime::usage::{apply_llm_usage_dual_write, LlmUsageWrite};
use crate::runtime::AgentLoop;

use super::hitl_bridge::{
    parse_astro_hitl, register_live_parent_hitl, unregister_live_parent_hitl, ParentHitlCtx,
};
use super::provider::ProviderStreamer;
use super::summary::{run_max_iterations_summary, SummaryOutcome};
use super::tools_exec::{execute_tools_concurrent, execute_tools_serial, terminal_needs_approval};
use super::traits::StreamingChat;
use super::types::{MultiTurnStream, MultiTurnStreamItem, StreamedAssistantContent};

/// `pre_verify` 单次 turn 内允许的最多验证轮次（含首次结束尝试）。
///
/// 对齐设计文档：仅本轮写盘且无工具终态时才计入；超过后不再 fire，直接收尾。
const MAX_VERIFY_ATTEMPTS: usize = 2;

/// 向 mpsc 发送单个成功事件；接收方关闭时返回 `false`。
pub(crate) async fn emit(
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
    let turn_id = agent.current_turn_id().map(str::to_string);
    let fallback_provider = agent.chat_provider().to_string();
    let fallback_base_url = agent.chat_base_url().to_string();
    let fallback_api_key = agent.chat_api_key().to_string();
    let fallback_model = agent.chat_model().to_string();

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

    // 持锁写入，确保账单落在与消息相同的 SessionStore（非 default_memory_dir 另开库）。
    apply_llm_usage_dual_write(
        &LlmUsageWrite {
            agent_id: &agent_id,
            session_id: Some(&session_id),
            turn_id: turn_id.as_deref(),
            model: &model,
            usage,
            provider: &provider,
            base_url: &base_url,
            api_key: &api_key,
        },
        None,
        Some(agent.sessions()),
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
async fn finish_success(tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>, run_id: &str) {
    let mut state = RunState::new();
    state.set_phase(RunPhase::Finished);
    let _ = emit(
        tx,
        MultiTurnStreamItem::RunFinished {
            run_id: run_id.to_string(),
            outcome_type: state.outcome_type().into(),
            interrupts_json: "[]".into(),
        },
    )
    .await;
    finish_done(tx).await;
}

/// 可选发送累计 usage 后发送 Done；若有 usage 则旁路写入 `usage.db`（kind=llm）。
pub(crate) async fn finish_usage_and_done(
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
///
/// 用于集成测试注入脚本化 Provider 回复，替代已移除的 `run_multi_turn_stream_from_provider`。
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
    // 工具循环结束后是否需要无工具强制总结（预算耗尽且尚无自然语言终答）
    let need_summary;
    // 原始迭代计数（不受 refund 影响），防止 code_exec-only 反复 refund 导致净预算永不耗尽。
    // 硬上限 = max_rounds × 2，超过即视为预算耗尽。
    let mut raw_rounds: usize = 0;
    // `pre_verify` 已消耗的验证尝试次数（每个 run 独立，跨 KeepGoing 轮次累加）。
    let mut verify_attempt: usize = 0;

    // 整次 run 累积时间线，供每轮 assistant 落盘写入 reasoning_details
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

        // Gateway 预压安全网（Hermes Session Hygiene）：进 LLM 前再跑一轮廉价维护。
        {
            let mut agent = session.lock().await;
            let recommend_ratio = agent.compression_config().recommend_compact_ratio;
            if agent.occupancy_ratio() >= recommend_ratio {
                match agent.maintain_tool_context().await {
                    Ok(report) if report.pruned + report.compressed > 0 => {
                        tracing::info!(
                            pruned = report.pruned,
                            compressed = report.compressed,
                            llm_summarized = report.llm_summarized,
                            occupancy_before = report.occupancy_before,
                            occupancy_after = report.occupancy_after,
                            recommend_ratio,
                            "gateway pre-maintain applied"
                        );
                    }
                    Ok(report) if report.recommend_session_compact => {
                        tracing::warn!(
                            occupancy = report.occupancy_after,
                            "gateway pre-maintain: still critical; recommend /compact"
                        );
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %e, "gateway pre-maintain failed"),
                }
            }
        }

        // Hard 阶段 mid-run 辅模型中间摘要（不拆 session）；每用户轮最多一次。
        {
            let mut agent = session.lock().await;
            match crate::exec::mid_run_summary::maybe_apply_mid_run_summary(&mut agent).await {
                Ok(true) => tracing::info!("mid-run summary applied before LLM round"),
                Ok(false) => {}
                Err(e) => tracing::warn!(error = %e, "mid-run summary failed"),
            }
        }

        let (history, tools) = {
            let mut agent = session.lock().await;
            agent.reload_tools_and_mcp().await;
            let mut messages = agent.provider_history();
            if let Some(ctx) = agent.take_inject_context() {
                messages.push(common::message::Message::user(&format!(
                    "[astro:hook-context]\n{ctx}"
                )));
            }
            let tools = agent.schemas_for_api();
            (messages, tools)
        };

        {
            let agent = session.lock().await;
            let sid = agent.session_id().to_string();
            let turn_id = agent.current_turn_id().map(str::to_string);
            let _ = agent.fire_hook(
                ::hooks::PRE_API_REQUEST,
                ::hooks::HookPayload {
                    session_id: sid,
                    turn_id,
                    ..Default::default()
                },
            );
        }

        {
            let agent = session.lock().await;
            let layers = agent.system_prompt_layer_breakdown();
            let recommend_compact_ratio = agent.compression_config().recommend_compact_ratio;
            let snap = crate::prompt::context_usage::build_snapshot(
                crate::prompt::context_usage::ContextUsageInput {
                    system_chars: layers.system_chars,
                    memory_chars: layers.memory_chars,
                    skills_chars: layers.skills_chars,
                    recall_chars: layers.recall_chars,
                    system_items: &layers.system_items,
                    memory_items: &layers.memory_items,
                    skill_items: &layers.skill_items,
                    tools: &tools,
                    messages: &history,
                    context_window: agent.context_window(),
                    updated_at_ms: chrono::Utc::now().timestamp_millis(),
                    recommend_compact: agent.should_recommend_compact(),
                    recommend_compact_ratio,
                },
            );
            drop(agent);
            let _ = emit(&tx, MultiTurnStreamItem::ContextUsage(snap)).await;
        }

        let raw_stream = match streamer.stream_chat(&system_prompt, &history, tools).await {
            Ok(s) => {
                let agent = session.lock().await;
                let sid = agent.session_id().to_string();
                let turn_id = agent.current_turn_id().map(str::to_string);
                let _ = agent.fire_hook(
                    ::hooks::POST_API_REQUEST,
                    ::hooks::HookPayload {
                        session_id: sid,
                        turn_id,
                        ..Default::default()
                    },
                );
                drop(agent);
                s
            }
            Err(err) => {
                let agent = session.lock().await;
                let sid = agent.session_id().to_string();
                let turn_id = agent.current_turn_id().map(str::to_string);
                let _ = agent.fire_hook(
                    ::hooks::POST_API_REQUEST,
                    ::hooks::HookPayload {
                        session_id: sid,
                        turn_id,
                        error: Some(err.to_string()),
                        detail: format!("error={err}"),
                        ..Default::default()
                    },
                );
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
        };

        let (abort_handle, abort_reg) = AbortHandle::new_pair();
        pause.attach_abort(abort_handle);
        let mut stream = Abortable::new(raw_stream, abort_reg);

        let mut full_response = String::new();
        let mut full_reasoning = String::new();
        let mut thought_signature: Option<String> = None;
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
                    round_usage = Some(u); // 覆盖：兼容累计式 usageMetadata
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

        // `pre_verify`：仅无工具终态（`calls.is_empty()`）且本轮写过盘时才 fire，
        // 在 `transform_llm_output` / `post_llm_call` 之前判断，`KeepGoing` 则注入
        // 提示并继续外层 API 循环（不进入本轮收尾）。
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
                let details = common::message::merge_google_thought_signature(
                    Some(timeline.reasoning_details_snapshot()),
                    thought_signature.as_deref(),
                );
                if let Err(err) = agent.record_assistant_message_with_tools(
                    &full_response,
                    None,
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
                // 直接持久化桥接 user 消息到 session_messages / SessionStore（而非仅
                // `queue_inject_context` 排队临时注入）：否则第二次 KeepGoing 时
                // `session_messages` 会出现连续 assistant，导致下一轮历史触发
                // Anthropic/Gemini 400。与 inject 保持同一文本形态，且不再排队注入，
                // 避免下一轮 history 重复出现该 user 消息（连续 user）。
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
                            signature: c.signature.clone(),
                        })
                        .collect(),
                )
            };
            for c in &calls {
                timeline.upsert_activity(&c.id, now_ms());
            }
            let details = common::message::merge_google_thought_signature(
                Some(timeline.reasoning_details_snapshot()),
                thought_signature.as_deref(),
            );
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

        for (call, result) in calls.iter().zip(outcomes) {
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

            let tool_media = result.media().to_vec();
            let result_text = result.text().to_string();
            if !emit(
                &tx,
                MultiTurnStreamItem::ToolResult {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    arguments_json: call.arguments.to_string(),
                    result: result_text.clone(),
                    media: tool_media.clone(),
                },
            )
            .await
            {
                return;
            }

            if matches!(call.name.as_str(), "memory")
                && !result_text.starts_with("工具错误")
                && !result_text.starts_with("工具已禁用")
                && !result_text.starts_with("工具参数 JSON 解析失败")
            {
                let preview = {
                    let s = result_text.trim();
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

            let info_ui = parse_astro_ui(&result_text);
            let result_for_history = if let Some(ref ui) = info_ui {
                format!("Presented info card: {}", ui.summary)
            } else if parse_astro_hitl(&result_text).is_some() {
                // 串行路径已把 HITL park 结果写成非 astro_hitl；若仍是标记则兜底
                result_text.clone()
            } else {
                result_text.clone()
            };

            if let Some(ref ui) = info_ui {
                let message_id = format!("a2ui-surface-{}", call.id);
                let content_json = serde_json::json!({ "operations": ui.operations }).to_string();
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
                // media 已从 ToolOutput 结构化取出，直接注入 session message，
                // 跳过 record_tool_result_with_id 内部的 extract_tool_media 冗余解析。
                if !tool_media.is_empty() {
                    if let Some(last) = agent.session_messages.last_mut() {
                        if last.role == common::message::Role::Tool && last.media.is_empty() {
                            last.media = tool_media;
                        }
                    }
                }
            }
        }

        // 工具循环后回写 timeline/surfaces，避免历史恢复丢 A2UI 卡片。
        {
            let agent = session.lock().await;
            if let Err(e) =
                agent.patch_last_assistant_timeline(timeline.reasoning_details_snapshot())
            {
                tracing::warn!(error = %e, "patch assistant timeline after tools failed");
            }
        }

        {
            let mut agent = session.lock().await;
            match agent.maintain_tool_context().await {
                Ok(report) if report.pruned + report.compressed > 0 => {
                    tracing::info!(
                        pruned = report.pruned,
                        compressed = report.compressed,
                        llm_summarized = report.llm_summarized,
                        occupancy_before = report.occupancy_before,
                        occupancy_after = report.occupancy_after,
                        stage_ratio = ?report.stage_ratio,
                        "tool context maintenance applied"
                    );
                }
                Ok(report) if report.recommend_session_compact => {
                    tracing::warn!(
                        occupancy = report.occupancy_after,
                        "context still critical after tool maintenance; recommend /compact"
                    );
                }
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "tool context maintenance failed"),
            }
            match crate::exec::mid_run_summary::maybe_apply_mid_run_summary(&mut agent).await {
                Ok(true) => tracing::info!("mid-run summary applied after tool maintenance"),
                Ok(false) => {}
                Err(e) => tracing::warn!(error = %e, "mid-run summary failed"),
            }
        }

        // 对齐 Hermes：本轮工具仅 code_exec 时退还本次迭代
        let names: Vec<&str> = calls.iter().map(|c| c.name.as_str()).collect();
        if crate::runtime::budget::should_refund_tool_round(&names) {
            budget.refund();
        }

        // Agno `stop_after_tool_call`：本轮含标记工具则不再请求下一轮 LLM
        let stop_after = {
            let agent = session.lock().await;
            agent.tool_registry().any_stop_after(&names)
        };
        if stop_after {
            tracing::info!(
                ?names,
                "stop_after_tool_call: ending run without next LLM round"
            );
            need_summary = false;
            break;
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
    stream_multi_turn_with_hitl(
        session,
        targets,
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

struct AstroUiPayload {
    summary: String,
    operations: serde_json::Value,
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
