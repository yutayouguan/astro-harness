//! 多轮工具循环：从 gRPC handler 收拢到 Agent 层的核心编排。
//!
//! **关键不变量**
//! - Pause/Cancel 对齐 Rig：`wait_if_paused` 先于上游 poll；取消时通过 `Abortable` 中止 Provider 流
//! - 每轮 assistant 回复必须写入 `SessionState.history`（含 tool_calls）后再执行工具
//! - 迭代预算对齐 Hermes：默认 90 轮；`code_exec` 独占轮可 refund；耗尽后无工具强制总结再终止
//! - usage 采用覆盖式累加，兼容 Google 等 Provider 的累计式 `usageMetadata`
//!
//! HITL park/resume 桥见 [`super::hitl_bridge`]；预算耗尽后的总结轮见 [`super::summary`]。

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use agent_protocol::{
    ContentItem, ControlRequestEvent, Event, EventMsg, ItemEvent, ResponseItem, ToolExecutionMode,
    ToolStatus, TurnInput, TurnItem, UserInputCommittedEvent,
};
use futures::stream::{AbortHandle, Abortable};
use futures::StreamExt;
use providers::ProviderConfig;
use providers::{PauseControl, Usage};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use types::ModelTarget;

use super::lifecycle::{
    emit, emit_delta, emit_response_items_completed, emit_text_item_started, emit_usage,
    tool_turn_item_with_execution, ToolExecutionMetadata,
};
use super::maintenance::{
    emit_context_usage, emit_provider_context_usage, post_tool_maintenance, pre_llm_maintenance,
    record_tool_outcomes, run_sampling_request, SamplingRequest,
};
use super::provider::ProviderStreamer;
use super::run_state::{RunPhase, RunState};
use super::summary::{run_max_iterations_summary, SummaryOutcome};
use super::tools_exec::{
    execute_tools_concurrent, execute_tools_serial, tool_may_require_permission,
};
use super::types::StreamedAssistantContent;
use crate::control::hitl::HitlGate;
use crate::runtime::turn_context::{QueuedTurnInput, TerminalInputDecision};
use crate::runtime::{Session, TurnContext};
use crate::streaming::maintenance::emit_post_llm_telemetry;
use crate::tasks::{RegularTask, SessionTaskResult, TurnCancelled};

/// `Stop` 单次 turn 内允许的最多验证轮次（含首次结束尝试）。
const MAX_VERIFY_ATTEMPTS: usize = 2;

/// 模型只返回思考/推理内容而没有文本回复时，允许的最大重试次数。
const MAX_THINKING_ONLY_RETRIES: usize = 1;

/// 按 index 的缓冲区，延迟参数事件直到 provider call id 已知。
#[derive(Default)]
struct PendingToolArgumentEvents {
    item_id: Option<String>,
    deltas: Vec<types::ToolCallDelta>,
}

fn response_item_calls(items: &[ResponseItem]) -> Vec<types::ParsedToolCall> {
    items
        .iter()
        .filter_map(|item| match item {
            ResponseItem::FunctionCall {
                id,
                name,
                namespace,
                arguments,
                encrypted_function_args,
                call_id,
                ..
            } => Some(types::ParsedToolCall {
                item_id: id.as_ref().map(ToString::to_string),
                id: call_id.clone(),
                name: name.clone(),
                namespace: namespace.clone(),
                arguments: serde_json::from_str(arguments)
                    .unwrap_or_else(|_| serde_json::Value::String(arguments.clone())),
                encrypted_arguments: encrypted_function_args.clone(),
                args_parse_error: false,
                signature: None,
            }),
            ResponseItem::CustomToolCall {
                id,
                call_id,
                name,
                namespace,
                input,
                ..
            } => Some(types::ParsedToolCall {
                item_id: id.as_ref().map(ToString::to_string),
                id: call_id.clone(),
                name: name.clone(),
                namespace: namespace.clone(),
                arguments: serde_json::Value::String(input.clone()),
                encrypted_arguments: None,
                args_parse_error: false,
                signature: None,
            }),
            ResponseItem::ToolSearchCall {
                id,
                call_id: Some(call_id),
                arguments,
                ..
            } => Some(types::ParsedToolCall {
                item_id: id.as_ref().map(ToString::to_string),
                id: call_id.clone(),
                name: "tool_search".into(),
                namespace: None,
                arguments: arguments.clone(),
                encrypted_arguments: None,
                args_parse_error: false,
                signature: None,
            }),
            _ => None,
        })
        .collect()
}

async fn record_assistant_output(
    agent: &Session,
    content: &str,
    calls: &[types::ParsedToolCall],
    reasoning: Option<&str>,
    reasoning_details: Option<serde_json::Value>,
    native_items: &[ResponseItem],
) -> anyhow::Result<()> {
    if native_items.is_empty() {
        return agent
            .record_assistant_with_calls(content, calls, reasoning, reasoning_details)
            .await;
    }
    let tool_calls = (!calls.is_empty()).then(|| {
        calls
            .iter()
            .map(|call| types::model_tool::ToolCall {
                id: call.id.clone(),
                name: call.name.clone(),
                namespace: call.namespace.clone(),
                arguments: call.arguments.clone(),
                signature: call.signature.clone(),
            })
            .collect()
    });
    let mut items = native_items.to_vec();
    let native_text = items
        .iter()
        .filter_map(|item| match item {
            ResponseItem::Message { role, content, .. } if role == "assistant" => Some(content),
            _ => None,
        })
        .flatten()
        .filter_map(|item| match item {
            ContentItem::OutputText { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<String>();
    if native_text != content {
        if let Some(ResponseItem::Message {
            content: item_content,
            ..
        }) = items
            .iter_mut()
            .find(|item| matches!(item, ResponseItem::Message { role, .. } if role == "assistant"))
        {
            *item_content = vec![ContentItem::OutputText {
                text: content.to_string(),
            }];
        }
    }
    agent
        .record_assistant_response_items(content, tool_calls, reasoning, reasoning_details, items)
        .await
}

async fn emit_tool_argument_events(
    session: &Session,
    turn_context: &TurnContext,
    item_id: &str,
    deltas: Vec<types::ToolCallDelta>,
) {
    for delta in deltas {
        emit(
            session,
            turn_context,
            EventMsg::DynamicToolCallRequest(ControlRequestEvent {
                turn_id: turn_context.sub_id().to_string(),
                request_id: format!("{}:{item_id}:arguments", turn_context.sub_id()),
                item_id: item_id.to_string(),
                payload: serde_json::json!({
                    "index": delta.index,
                    "name": delta.name,
                    "delta": delta.arguments,
                }),
            }),
        )
        .await;
    }
}

pub(crate) struct ThreadTurnTaskArgs {
    pub(crate) session: Arc<Session>,
    pub(crate) targets: Vec<ModelTarget>,
    pub(crate) base_config: ProviderConfig,
    pub(crate) input: Vec<TurnInput>,
    pub(crate) system_prompt: Option<String>,
    pub(crate) prompt: Option<crate::prompt::PromptContract>,
    pub(crate) pause: Arc<PauseControl>,
    pub(crate) hitl_gate: Option<Arc<HitlGate>>,
    pub(crate) responses_override: Option<super::provider::ResponsesOverride>,
}

pub(crate) struct InstalledMultiTurn {
    pub(crate) session: Arc<Session>,
    pub(crate) turn_id: String,
    pub(crate) events: async_channel::Receiver<Event>,
}

pub(crate) struct MultiTurnInstallError {
    pub(crate) turn_id: String,
    pub(crate) message: String,
}

/// 集成测试和适配器使用的规范 Thread 事件执行接缝。
#[doc(hidden)]
pub struct ThreadTurnEventArgs {
    pub session: Arc<Session>,
    pub targets: Vec<ModelTarget>,
    pub base_config: ProviderConfig,
    pub input: Vec<TurnInput>,
    pub system_prompt: Option<String>,
    pub pause: Arc<PauseControl>,
    pub hitl_gate: Option<Arc<HitlGate>>,
    pub tx: mpsc::Sender<anyhow::Result<Event>>,
    pub responses_override: Option<super::provider::ResponsesOverride>,
}

pub(crate) async fn install_multi_turn_task(
    args: ThreadTurnTaskArgs,
) -> Result<InstalledMultiTurn, MultiTurnInstallError> {
    let ThreadTurnTaskArgs {
        session,
        targets,
        base_config,
        input,
        system_prompt,
        prompt,
        pause,
        hitl_gate,
        responses_override,
    } = args;
    let session_id = session.session_id().to_string();
    let sub_id = uuid::Uuid::new_v4().to_string();
    let events = session.subscribe_turn_events(&sub_id).await;
    let turn_context = session.create_turn_context(sub_id.clone()).await;
    turn_context.initialize_provider_settings(targets.clone(), base_config.clone());
    let task = RegularTask::new(RunTurnArgs {
        session: session.clone(),
        turn_context: Arc::clone(&turn_context),
        targets,
        base_config,
        system_prompt,
        prompt,
        pause,
        hitl_gate,
        responses_override,
        drain_mailbox: true,
    });
    tracing::info!(session_id = %session_id, turn_id = %sub_id, "turn started");
    if let Err(error) = session.spawn_task(turn_context, input, task).await {
        session.remove_turn_event_taps(&sub_id).await;
        return Err(MultiTurnInstallError {
            turn_id: sub_id.clone(),
            message: error.to_string(),
        });
    }
    Ok(InstalledMultiTurn {
        session,
        turn_id: sub_id,
        events,
    })
}

/// 集成测试和适配器使用的规范 Thread 事件执行接缝。
#[doc(hidden)]
pub async fn run_thread_turn_events(args: ThreadTurnEventArgs) {
    let ThreadTurnEventArgs {
        session,
        targets,
        base_config,
        input,
        system_prompt,
        pause,
        hitl_gate,
        tx,
        responses_override,
    } = args;
    match install_multi_turn_task(ThreadTurnTaskArgs {
        session: Arc::clone(&session),
        targets,
        base_config,
        input,
        system_prompt,
        prompt: None,
        pause,
        hitl_gate,
        responses_override,
    })
    .await
    {
        Ok(installed) => {
            let turn_id = installed.turn_id;
            let event_turn_id = crate::runtime::event_identity::event_turn_id(&turn_id);
            while let Ok(event) = installed.events.recv().await {
                if event.id != event_turn_id {
                    continue;
                }
                let terminal = event.msg.is_terminal();
                if tx.send(Ok(event)).await.is_err() || terminal {
                    break;
                }
            }
            session.wait_for_task(&turn_id).await;
        }
        Err(error) => {
            let _ = tx
                .send(Ok(Event {
                    id: crate::runtime::event_identity::event_turn_id(&error.turn_id),
                    msg: EventMsg::Error(agent_protocol::ErrorEvent {
                        message: error.message,
                        error_type: "turn_prepare".into(),
                    }),
                }))
                .await;
        }
    }
}

/// 为现有生命周期测试提供的预构建 turn 便捷接缝。
#[doc(hidden)]
pub async fn run_multi_turn_events_with_responses_fn(
    session: Arc<Session>,
    responses_fn: super::provider::ResponsesOverride,
    config: ProviderConfig,
    system_prompt: String,
    pause: Arc<PauseControl>,
    hitl_gate: Option<Arc<HitlGate>>,
    tx: mpsc::Sender<anyhow::Result<Event>>,
) {
    let target = ModelTarget {
        provider_id: "scripted".into(),
        backend_id: "scripted".into(),
        model: config.model.clone(),
        api_key: config.api_key.clone(),
        base_url: config.base_url.clone().unwrap_or_default(),
    };
    run_thread_turn_events(ThreadTurnEventArgs {
        session,
        targets: vec![target],
        base_config: config,
        input: Vec::new(),
        system_prompt: Some(system_prompt),
        pause,
        hitl_gate,
        tx,
        responses_override: Some(responses_fn),
    })
    .await;
}
pub async fn run_multi_turn_stream_with_responses_fn(
    session: Arc<Session>,
    turn_context: Arc<TurnContext>,
    input: Vec<TurnInput>,
    responses_fn: super::provider::ResponsesOverride,
) -> anyhow::Result<()> {
    let args = RunTurnArgs::submitted(
        Arc::clone(&session),
        Arc::clone(&turn_context),
        Some(responses_fn),
    );
    let turn_id = turn_context.sub_id().to_string();
    session
        .spawn_task(turn_context, input, RegularTask::new(args))
        .await?;
    session.wait_for_task(&turn_id).await;
    Ok(())
}

#[derive(Clone)]
pub(crate) struct RunTurnArgs {
    session: Arc<Session>,
    turn_context: Arc<TurnContext>,
    targets: Vec<ModelTarget>,
    base_config: ProviderConfig,
    system_prompt: Option<String>,
    prompt: Option<crate::prompt::PromptContract>,
    pause: Arc<PauseControl>,
    hitl_gate: Option<Arc<HitlGate>>,
    responses_override: Option<super::provider::ResponsesOverride>,
    drain_mailbox: bool,
}

impl RunTurnArgs {
    pub(crate) fn submitted(
        session: Arc<Session>,
        turn_context: Arc<TurnContext>,
        responses_override: Option<super::provider::ResponsesOverride>,
    ) -> Self {
        let mut targets = session.model_targets();
        let provider = session.chat_provider();
        let model = session.chat_model();
        let api_key = session.chat_api_key();
        let base_url = session.chat_base_url();
        if targets.is_empty() {
            targets.push(ModelTarget {
                provider_id: provider.clone(),
                backend_id: provider,
                model: model.clone(),
                api_key: api_key.clone(),
                base_url: base_url.clone(),
            });
        }
        let provider_options = session.thread_provider_options();
        let base_config = ProviderConfig {
            model,
            api_key,
            base_url: (!base_url.is_empty()).then_some(base_url),
            temperature: session.temperature(),
            thinking_enabled: provider_options.thinking_enabled,
            reasoning_effort: provider_options.reasoning_effort,
            additional_params: session.additional_params(),
            max_tokens: provider_options.max_tokens,
            ..ProviderConfig::default()
        };
        let (pause, hitl_gate, _approval_cache) = session.ensure_thread_controls();
        turn_context.initialize_provider_settings(targets.clone(), base_config.clone());
        Self {
            session,
            turn_context,
            targets,
            base_config,
            system_prompt: None,
            prompt: None,
            pause,
            hitl_gate: Some(hitl_gate),
            responses_override,
            drain_mailbox: true,
        }
    }

    pub(crate) fn with_turn_context(&self, turn_context: Arc<TurnContext>) -> Self {
        Self {
            turn_context,
            ..self.clone()
        }
    }

    pub(crate) fn session(&self) -> &Arc<Session> {
        &self.session
    }

    pub(crate) fn turn_context(&self) -> &Arc<TurnContext> {
        &self.turn_context
    }

    pub(crate) fn with_prompt(&self, prompt: crate::prompt::PromptContract) -> Self {
        Self {
            prompt: Some(prompt),
            system_prompt: None,
            ..self.clone()
        }
    }

    pub(crate) fn for_isolated_review(
        &self,
        session: Arc<Session>,
        turn_context: Arc<TurnContext>,
    ) -> Self {
        let mut args = Self::submitted(session, turn_context, self.responses_override.clone());
        args.drain_mailbox = false;
        args
    }

    pub(crate) fn prepared_system_prompt(&self) -> Option<&str> {
        self.system_prompt.as_deref()
    }

    pub(crate) fn prepared_prompt(&self) -> Option<crate::prompt::PromptContract> {
        self.prompt.clone().or_else(|| {
            self.prepared_system_prompt()
                .map(crate::prompt::PromptContract::from_base_instructions)
        })
    }
}

async fn record_pending_input(
    session: &Arc<Session>,
    pending_input: Vec<QueuedTurnInput>,
) -> anyhow::Result<()> {
    if pending_input.is_empty() {
        return Ok(());
    }
    let client_message_ids = pending_input
        .iter()
        .filter_map(|queued| queued.input.client_message_id.clone())
        .collect::<Vec<_>>();
    session.record_queued_turn_inputs(pending_input).await?;
    let turn_id = session
        .current_turn_id()
        .await
        .unwrap_or_else(|| session.session_id().to_string());
    for client_message_id in client_message_ids {
        session
            .send_event(
                &turn_id,
                EventMsg::UserInputCommitted(UserInputCommittedEvent {
                    turn_id: turn_id.clone(),
                    client_message_id,
                }),
            )
            .await;
    }
    Ok(())
}

async fn drain_available_mailbox(
    session: &Arc<Session>,
    turn_context: &TurnContext,
) -> anyhow::Result<crate::exec::subagents::MailboxDrainOutcome> {
    let outcome = crate::exec::subagents::drain_mailbox_at_safe_boundary(session).await?;
    if outcome.deferred {
        return Ok(outcome);
    }
    turn_context.acknowledge_mailbox_inputs(&outcome.delivered_steer_ids);
    let turn_id = turn_context.sub_id().to_string();
    for client_message_id in &outcome.delivered_client_message_ids {
        session
            .send_event(
                &turn_id,
                EventMsg::UserInputCommitted(UserInputCommittedEvent {
                    turn_id: turn_id.clone(),
                    client_message_id: client_message_id.clone(),
                }),
            )
            .await;
    }
    Ok(outcome)
}

async fn finish_task_error(
    session: &Arc<Session>,
    turn_context: &TurnContext,
    streamer: &ProviderStreamer,
    message: impl Into<String>,
    usage: Option<Usage>,
) -> SessionTaskResult {
    let message = message.into();
    emit_usage(session, turn_context, streamer, usage).await;
    Err(anyhow::anyhow!(message))
}

async fn finish_task_cancelled(
    session: &Arc<Session>,
    turn_context: &TurnContext,
    streamer: &ProviderStreamer,
    usage: Option<Usage>,
) -> SessionTaskResult {
    emit_usage(session, turn_context, streamer, usage).await;
    Err(TurnCancelled.into())
}

/// 前台和后台适配器共享的常规 turn 循环。
pub(crate) async fn run_turn(
    args: RunTurnArgs,
    cancellation_token: CancellationToken,
) -> SessionTaskResult {
    let RunTurnArgs {
        session,
        turn_context,
        targets,
        base_config,
        system_prompt,
        prompt,
        pause,
        hitl_gate,
        responses_override,
        drain_mailbox,
    } = args;
    debug_assert!(
        system_prompt.is_none(),
        "prebuilt system prompt must be consumed by RegularTask"
    );
    let prompt = prompt.expect("RegularTask prepares the prompt contract");
    turn_context.initialize_provider_settings(targets, base_config);
    let initial_settings = turn_context
        .provider_settings()
        .expect("turn provider settings initialized above");
    let mut settings_generation = initial_settings.generation;
    let mut streamer = match responses_override.clone() {
        Some(f) => ProviderStreamer::with_responses_override(
            initial_settings.targets,
            initial_settings.base_config,
            f,
        ),
        None => ProviderStreamer::new(initial_settings.targets, initial_settings.base_config),
    };
    let mut total_usage = Usage::default();
    let mut saw_usage = false;

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

    let mut timeline = crate::timeline::TimelineBuilder::new();
    let now_ms = || chrono::Utc::now().timestamp_millis();

    loop {
        raw_rounds += 1;
        if raw_rounds > max_rounds.saturating_mul(2) || !budget.consume() {
            need_summary = true;
            break;
        }
        if cancellation_token.is_cancelled() || pause.is_cancelled() {
            return finish_task_cancelled(
                &session,
                &turn_context,
                &streamer,
                saw_usage.then_some(total_usage),
            )
            .await;
        }
        if !pause.wait_if_paused().await {
            return finish_task_cancelled(
                &session,
                &turn_context,
                &streamer,
                saw_usage.then_some(total_usage),
            )
            .await;
        }

        if let Err(error) = record_pending_input(&session, turn_context.take_pending_input()).await
        {
            return finish_task_error(
                &session,
                &turn_context,
                &streamer,
                error.to_string(),
                saw_usage.then_some(total_usage),
            )
            .await;
        }

        if drain_mailbox {
            if let Err(error) = drain_available_mailbox(&session, &turn_context).await {
                return finish_task_error(
                    &session,
                    &turn_context,
                    &streamer,
                    error.to_string(),
                    saw_usage.then_some(total_usage),
                )
                .await;
            }
        }
        if pre_llm_maintenance(&session, &turn_context).await {
            return finish_task_cancelled(
                &session,
                &turn_context,
                &streamer,
                saw_usage.then_some(total_usage),
            )
            .await;
        }

        let step_context = { session.capture_step_context().await };
        let step_context = match step_context {
            Ok(step_context) => step_context,
            Err(error) => {
                return finish_task_error(
                    &session,
                    &turn_context,
                    &streamer,
                    error.to_string(),
                    saw_usage.then_some(total_usage),
                )
                .await;
            }
        };
        tracing::debug!(
            sub_id = %step_context.turn.sub_id(),
            turn = step_context.turn.turn(),
            "step context captured"
        );
        let history = step_context.history.clone();
        let prompt_context = step_context.prompt_context.clone();
        let tool_specs = step_context.tool_router.model_visible_specs().to_vec();

        let context_usage_snapshot = emit_context_usage(
            &session,
            &turn_context,
            &prompt,
            &prompt_context,
            &history,
            &tool_specs,
        )
        .await;

        // An in-flight request keeps its original snapshot. A complete settings update is
        // published atomically immediately before the next provider request.
        if let Some(settings) = turn_context.provider_settings() {
            if settings.generation != settings_generation {
                settings_generation = settings.generation;
                streamer = match responses_override.clone() {
                    Some(f) => ProviderStreamer::with_responses_override(
                        settings.targets,
                        settings.base_config,
                        f,
                    ),
                    None => ProviderStreamer::new(settings.targets, settings.base_config),
                };
            }
        }

        let sampling = match run_sampling_request(
            &session,
            &streamer,
            &prompt,
            &prompt_context,
            &history,
            tool_specs,
            raw_rounds,
        )
        .await
        {
            Ok(s) => s,
            Err(err) => {
                return finish_task_error(
                    &session,
                    &turn_context,
                    &streamer,
                    err,
                    saw_usage.then_some(total_usage),
                )
                .await;
            }
        };
        let SamplingRequest {
            stream: raw_stream,
            provider: sampling_provider,
            model: sampling_model,
            attempt: sampling_attempt,
            started_at: sampling_started_at,
        } = sampling;

        let (abort_handle, abort_reg) = AbortHandle::new_pair();
        pause.attach_abort(abort_handle);
        let mut stream = Abortable::new(raw_stream, abort_reg);

        let mut full_response = String::new();
        let mut full_reasoning = String::new();
        let mut completed_response_items = Vec::new();
        let mut thought_signature: Option<String> = None;
        let mut tool_acc = types::ToolCallAccumulator::new();
        let mut tool_argument_events: HashMap<u32, PendingToolArgumentEvents> = HashMap::new();
        let mut tool_call_indices = BTreeSet::new();
        let mut round_usage: Option<Usage> = None;
        let assistant_item_id = uuid::Uuid::new_v4().to_string();
        let reasoning_item_id = uuid::Uuid::new_v4().to_string();
        let mut assistant_started = false;
        let mut reasoning_started = false;

        loop {
            if !pause.wait_if_paused().await {
                pause.clear_abort();
                emit_post_llm_telemetry(
                    &session,
                    sampling_provider.clone(),
                    sampling_model.clone(),
                    sampling_attempt,
                    sampling_started_at,
                    "cancelled",
                    full_response.len(),
                    None,
                )
                .await;
                return finish_task_cancelled(&session, &turn_context, &streamer, {
                    if let Some(u) = round_usage {
                        total_usage.add_assign(u);
                        saw_usage = true;
                    }
                    saw_usage.then_some(total_usage)
                })
                .await;
            }

            let next = tokio::select! {
                biased;
                _ = cancellation_token.cancelled() => {
                    pause.clear_abort();
                    emit_post_llm_telemetry(
                        &session,
                        sampling_provider.clone(),
                        sampling_model.clone(),
                        sampling_attempt,
                        sampling_started_at,
                        "cancelled",
                        full_response.len(),
                        None,
                    ).await;
                    return finish_task_cancelled(
                        &session,
                        &turn_context,
                        &streamer,
                        saw_usage.then_some(total_usage),
                    )
                    .await;
                }
                _ = pause.wait_cancelled() => {
                    pause.clear_abort();
                    emit_post_llm_telemetry(
                        &session,
                        sampling_provider.clone(),
                        sampling_model.clone(),
                        sampling_attempt,
                        sampling_started_at,
                        "cancelled",
                        full_response.len(),
                        None,
                    ).await;
                    return finish_task_cancelled(
                    &session,
                    &turn_context,
                    &streamer,
                        {
                            if let Some(u) = round_usage {
                                total_usage.add_assign(u);
                                saw_usage = true;
                            }
                            saw_usage.then_some(total_usage)
                        },
                    )
                    .await;
                }
                item = stream.next() => item,
            };

            match next {
                None => break,
                Some(Ok(StreamedAssistantContent::ResponseItemDone(item))) => {
                    completed_response_items.push(item);
                }
                Some(Ok(StreamedAssistantContent::Text(text))) => {
                    if !text.is_empty() {
                        timeline.push_text_delta(&text, now_ms());
                        if !assistant_started {
                            emit_text_item_started(
                                &session,
                                &turn_context,
                                assistant_item_id.clone(),
                                false,
                            )
                            .await;
                            assistant_started = true;
                        }
                        full_response.push_str(&text);
                        emit_delta(&session, &turn_context, &assistant_item_id, text, false).await;
                    }
                }
                Some(Ok(StreamedAssistantContent::Reasoning(r))) => {
                    full_reasoning.push_str(&r);
                    timeline.push_reasoning_delta(&r, now_ms());
                    if !reasoning_started {
                        emit_text_item_started(
                            &session,
                            &turn_context,
                            reasoning_item_id.clone(),
                            true,
                        )
                        .await;
                        reasoning_started = true;
                    }
                    emit_delta(&session, &turn_context, &reasoning_item_id, r, true).await;
                }
                Some(Ok(StreamedAssistantContent::ThoughtSignature(sig))) => {
                    thought_signature = Some(sig);
                }
                Some(Ok(StreamedAssistantContent::ToolCallDelta(d))) => {
                    if d.name.as_deref().is_some_and(|name| !name.is_empty()) {
                        tool_call_indices.insert(d.index);
                    }
                    let has_named_call = tool_call_indices.contains(&d.index);
                    let (item_id, buffered) = {
                        let pending = tool_argument_events.entry(d.index).or_default();
                        pending.deltas.push(d.clone());
                        if pending.item_id.is_none() {
                            pending.item_id = d.id.as_ref().filter(|id| !id.is_empty()).cloned();
                        }
                        if has_named_call {
                            pending
                                .item_id
                                .clone()
                                .map(|item_id| (item_id, std::mem::take(&mut pending.deltas)))
                                .unzip()
                        } else {
                            (None, None)
                        }
                    };
                    let mut accumulated_delta = d;
                    if let Some(item_id) = item_id.as_ref() {
                        accumulated_delta.id = Some(item_id.clone());
                    }
                    tool_acc.push(&accumulated_delta);
                    if let (Some(item_id), Some(buffered)) = (item_id, buffered) {
                        timeline.upsert_activity(&item_id, now_ms());
                        emit_tool_argument_events(&session, &turn_context, &item_id, buffered)
                            .await;
                    }
                }
                Some(Ok(StreamedAssistantContent::FinalUsage(u))) => {
                    round_usage = Some(u);
                    emit_provider_context_usage(
                        &session,
                        &turn_context,
                        &context_usage_snapshot,
                        u,
                    )
                    .await;
                }
                Some(Ok(StreamedAssistantContent::Citations(cites))) => {
                    let _ = cites;
                }
                Some(Ok(StreamedAssistantContent::InteractionId(_))) => {}
                Some(Err(err)) => {
                    pause.clear_abort();
                    emit_post_llm_telemetry(
                        &session,
                        sampling_provider.clone(),
                        sampling_model.clone(),
                        sampling_attempt,
                        sampling_started_at,
                        "failed",
                        full_response.len(),
                        Some(err.to_string()),
                    )
                    .await;
                    if let Some(u) = round_usage {
                        total_usage.add_assign(u);
                        saw_usage = true;
                    }
                    return finish_task_error(
                        &session,
                        &turn_context,
                        &streamer,
                        err.to_string(),
                        saw_usage.then_some(total_usage),
                    )
                    .await;
                }
            }
        }

        pause.clear_abort();

        if pause.is_cancelled() {
            emit_post_llm_telemetry(
                &session,
                sampling_provider.clone(),
                sampling_model.clone(),
                sampling_attempt,
                sampling_started_at,
                "cancelled",
                full_response.len(),
                None,
            )
            .await;
            if let Some(u) = round_usage {
                total_usage.add_assign(u);
                saw_usage = true;
            }
            return finish_task_cancelled(
                &session,
                &turn_context,
                &streamer,
                saw_usage.then_some(total_usage),
            )
            .await;
        }

        if let Some(u) = round_usage {
            total_usage.add_assign(u);
            saw_usage = true;
        }

        emit_post_llm_telemetry(
            &session,
            sampling_provider.clone(),
            sampling_model.clone(),
            sampling_attempt,
            sampling_started_at,
            "succeeded",
            full_response.len(),
            None,
        )
        .await;

        for index in &tool_call_indices {
            let pending = tool_argument_events
                .get_mut(index)
                .expect("tool argument index collected above");
            let item_id = pending
                .item_id
                .get_or_insert_with(|| uuid::Uuid::new_v4().to_string())
                .clone();
            let buffered = std::mem::take(&mut pending.deltas);
            timeline.upsert_activity(&item_id, now_ms());
            emit_tool_argument_events(&session, &turn_context, &item_id, buffered).await;
        }

        let response_calls = response_item_calls(&completed_response_items);
        let mut accumulated_calls = tool_acc.finish();
        for (call, index) in accumulated_calls.iter_mut().zip(tool_call_indices) {
            if let Some(item_id) = tool_argument_events
                .get(&index)
                .and_then(|pending| pending.item_id.as_ref())
            {
                call.item_id = Some(item_id.clone());
            }
        }
        let calls = if response_calls.is_empty() {
            accumulated_calls
        } else {
            response_calls
        };

        if full_response.is_empty() && calls.is_empty() {
            if !full_reasoning.is_empty() && thinking_only_retries < MAX_THINKING_ONLY_RETRIES {
                thinking_only_retries += 1;
                tracing::warn!(
                    reasoning_len = full_reasoning.len(),
                    attempt = thinking_only_retries,
                    "model returned reasoning only with no text; injecting retry prompt"
                );
                let agent = session.as_ref();
                let details = types::model_tool::merge_google_thought_signature(
                    Some(timeline.reasoning_details_snapshot()),
                    thought_signature.as_deref(),
                );
                if let Err(err) = record_assistant_output(
                    agent,
                    &full_response,
                    &[],
                    Some(full_reasoning.as_str()),
                    details,
                    &completed_response_items,
                )
                .await
                {
                    return finish_task_error(
                        &session,
                        &turn_context,
                        &streamer,
                        err.to_string(),
                        saw_usage.then_some(total_usage),
                    )
                    .await;
                }
                emit_response_items_completed(
                    &session,
                    &turn_context,
                    assistant_started,
                    assistant_item_id,
                    full_response.clone(),
                    reasoning_item_id,
                    full_reasoning.clone(),
                )
                .await;
                if let Err(err) = agent.record_user_message(
                    "[astro:system]\n你的思考过程已记录，但没有生成回复内容。请直接给出你的回答。",
                )
                .await
                {
                    return finish_task_error(
                        &session,
                        &turn_context,
                        &streamer,
                        err.to_string(),
                        saw_usage.then_some(total_usage),
                    )
                    .await;
                }
                continue;
            }
            return finish_task_error(
                &session,
                &turn_context,
                &streamer,
                "模型返回了空回复。请重试，或换一个模型。",
                saw_usage.then_some(total_usage),
            )
            .await;
        }

        // `Stop` 钩子。
        if calls.is_empty() {
            let agent = session.as_ref();
            let verify_outcome = agent.run_stop_hook(
                agent.current_turn_id().await,
                verify_attempt > 0,
                Some(full_response.clone()),
            );
            if verify_outcome.should_block
                && verify_attempt < MAX_VERIFY_ATTEMPTS
                && !verify_outcome.continuation_fragments.is_empty()
            {
                verify_attempt += 1;
                let details = types::model_tool::merge_google_thought_signature(
                    Some(timeline.reasoning_details_snapshot()),
                    thought_signature.as_deref(),
                );
                if let Err(err) = record_assistant_output(
                    agent,
                    &full_response,
                    &[],
                    (!full_reasoning.is_empty()).then_some(full_reasoning.as_str()),
                    details,
                    &completed_response_items,
                )
                .await
                {
                    return finish_task_error(
                        &session,
                        &turn_context,
                        &streamer,
                        err.to_string(),
                        saw_usage.then_some(total_usage),
                    )
                    .await;
                }
                emit_response_items_completed(
                    &session,
                    &turn_context,
                    assistant_started,
                    assistant_item_id,
                    full_response.clone(),
                    reasoning_item_id,
                    full_reasoning.clone(),
                )
                .await;
                let hook_prompt = match agent
                    .record_hook_prompt(verify_outcome.continuation_fragments)
                    .await
                {
                    Ok(Some(item)) => item,
                    Ok(None) => {
                        return finish_task_error(
                            &session,
                            &turn_context,
                            &streamer,
                            "Stop hook blocked completion without attributed feedback.",
                            saw_usage.then_some(total_usage),
                        )
                        .await;
                    }
                    Err(err) => {
                        return finish_task_error(
                            &session,
                            &turn_context,
                            &streamer,
                            err.to_string(),
                            saw_usage.then_some(total_usage),
                        )
                        .await;
                    }
                };
                emit(
                    &session,
                    &turn_context,
                    EventMsg::ItemCompleted(ItemEvent {
                        turn_id: turn_context.sub_id().to_string(),
                        item: TurnItem::HookPrompt(hook_prompt),
                    }),
                )
                .await;
                continue;
            }
        }

        {
            let agent = session.as_ref();
            let sid = agent.session_id().to_string();
            let turn_id = agent.current_turn_id().await;
            let transformed = agent.fire_hook(
                ::hooks::TRANSFORM_FINAL_LLM_OUTPUT,
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
            let cancelled = agent.cancel_signal().is_cancelled();
            if cancelled {
                return finish_task_cancelled(
                    &session,
                    &turn_context,
                    &streamer,
                    saw_usage.then_some(total_usage),
                )
                .await;
            }
        }

        if !assistant_started && !full_response.is_empty() {
            emit_text_item_started(&session, &turn_context, assistant_item_id.clone(), false).await;
            assistant_started = true;
        }

        let force_serial = {
            calls.iter().any(|call| {
                step_context
                    .tool_router
                    .needs_confirmation(call.namespace.as_deref(), &call.name)
                    || step_context
                        .tool_router
                        .exclusive_access(call.namespace.as_deref(), &call.name)
                    || step_context
                        .tool_router
                        .may_require_approval(call.namespace.as_deref(), &call.name)
            }) || calls
                .iter()
                .any(|c| tool_may_require_permission(&c.name, &c.arguments))
        };
        let tool_execution = calls.first().map(|first| ToolExecutionMetadata {
            batch_id: format!("tool-batch-{}", first.id),
            mode: if force_serial || hitl_gate.is_none() {
                ToolExecutionMode::Serial
            } else {
                ToolExecutionMode::Parallel
            },
        });

        {
            let agent = session.as_ref();
            for c in &calls {
                timeline.upsert_activity_with_execution(
                    &c.id,
                    now_ms(),
                    tool_execution.as_ref().map(|value| value.batch_id.as_str()),
                    tool_execution.as_ref().map(|value| match value.mode {
                        ToolExecutionMode::Serial => "serial",
                        ToolExecutionMode::Parallel => "parallel",
                    }),
                );
            }
            let details = types::model_tool::merge_google_thought_signature(
                Some(timeline.reasoning_details_snapshot()),
                thought_signature.as_deref(),
            );
            if let Err(err) = record_assistant_output(
                agent,
                &full_response,
                &calls,
                (!full_reasoning.is_empty()).then_some(full_reasoning.as_str()),
                details,
                &completed_response_items,
            )
            .await
            {
                return finish_task_error(
                    &session,
                    &turn_context,
                    &streamer,
                    err.to_string(),
                    saw_usage.then_some(total_usage),
                )
                .await;
            }
        }
        emit_response_items_completed(
            &session,
            &turn_context,
            assistant_started,
            assistant_item_id,
            full_response.clone(),
            reasoning_item_id,
            full_reasoning.clone(),
        )
        .await;

        if calls.is_empty() {
            match turn_context.wait_for_terminal_input().await {
                TerminalInputDecision::Queued(pending_input) => {
                    if let Err(error) = record_pending_input(&session, pending_input).await {
                        return finish_task_error(
                            &session,
                            &turn_context,
                            &streamer,
                            error.to_string(),
                            saw_usage.then_some(total_usage),
                        )
                        .await;
                    }
                    run_state.set_phase(RunPhase::StreamingLlm);
                    continue;
                }
                TerminalInputDecision::MailboxPending => {
                    let outcome = match drain_available_mailbox(&session, &turn_context).await {
                        Ok(outcome) => outcome,
                        Err(error) => {
                            return finish_task_error(
                                &session,
                                &turn_context,
                                &streamer,
                                error.to_string(),
                                saw_usage.then_some(total_usage),
                            )
                            .await;
                        }
                    };
                    if outcome.deferred || outcome.delivered == 0 {
                        return finish_task_error(
                            &session,
                            &turn_context,
                            &streamer,
                            "active-turn mailbox signal had no deliverable durable input",
                            saw_usage.then_some(total_usage),
                        )
                        .await;
                    }
                    run_state.set_phase(RunPhase::StreamingLlm);
                    continue;
                }
                TerminalInputDecision::Closed => {}
            }
            need_summary = false;
            break;
        }

        for call in &calls {
            emit(
                &session,
                &turn_context,
                EventMsg::ItemStarted(ItemEvent {
                    turn_id: turn_context.sub_id().to_string(),
                    item: tool_turn_item_with_execution(
                        call.id.clone(),
                        call.display_name(),
                        call.arguments.clone(),
                        None,
                        Vec::new(),
                        ToolStatus::InProgress,
                        tool_execution.as_ref(),
                    ),
                }),
            )
            .await;
        }

        run_state.set_phase(RunPhase::ExecutingTools);
        let outcomes = if force_serial || hitl_gate.is_none() {
            execute_tools_serial(
                &session,
                Arc::clone(&step_context),
                &calls,
                &pause,
                &turn_context,
                hitl_gate.as_ref(),
            )
            .await
        } else {
            execute_tools_concurrent(&session, Arc::clone(&step_context), &calls, &pause).await
        };

        let Some(outcomes) = outcomes else {
            return finish_task_cancelled(
                &session,
                &turn_context,
                &streamer,
                saw_usage.then_some(total_usage),
            )
            .await;
        };

        if !record_tool_outcomes(
            &session,
            &calls,
            outcomes,
            &pause,
            &turn_context,
            &mut timeline,
            now_ms,
            tool_execution.as_ref(),
        )
        .await
        {
            return finish_task_cancelled(
                &session,
                &turn_context,
                &streamer,
                saw_usage.then_some(total_usage),
            )
            .await;
        }

        if post_tool_maintenance(&session, &step_context, &turn_context, &calls).await {
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
            prompt: &prompt,
            pause: &pause,
            turn_context: &turn_context,
            timeline: &mut timeline,
            total_usage: &mut total_usage,
            saw_usage: &mut saw_usage,
            used: budget.used(),
            max_total: budget.max_total(),
        })
        .await
        {
            SummaryOutcome::Finished => {}
            SummaryOutcome::Aborted => return Err(TurnCancelled.into()),
            SummaryOutcome::Failed(err) => {
                return finish_task_error(
                    &session,
                    &turn_context,
                    &streamer,
                    err,
                    saw_usage.then_some(total_usage),
                )
                .await;
            }
        }
    }

    if cancellation_token.is_cancelled() || pause.is_cancelled() {
        return finish_task_cancelled(
            &session,
            &turn_context,
            &streamer,
            saw_usage.then_some(total_usage),
        )
        .await;
    }

    emit_usage(
        &session,
        &turn_context,
        &streamer,
        saw_usage.then_some(total_usage),
    )
    .await;
    Ok(None)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use tempfile::TempDir;

    use super::*;

    fn queued_input(client_message_id: &str) -> QueuedTurnInput {
        QueuedTurnInput {
            input: TurnInput {
                content: "steered input".into(),
                image_data_urls: Vec::new(),
                client_message_id: Some(client_message_id.into()),
            },
            inject_context: None,
        }
    }

    #[test]
    fn native_response_call_keeps_item_and_call_identity() {
        let calls = response_item_calls(&[ResponseItem::FunctionCall {
            id: Some("item_7".into()),
            name: "lookup".into(),
            namespace: Some("mcp".into()),
            arguments: "{\"q\":1}".into(),
            encrypted_function_args: Some(vec!["ciphertext".into()]),
            call_id: "call_7".into(),
            internal_chat_message_metadata_passthrough: None,
        }]);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].item_id.as_deref(), Some("item_7"));
        assert_eq!(calls[0].id, "call_7");
        assert_eq!(calls[0].namespace.as_deref(), Some("mcp"));
        assert_eq!(calls[0].display_name(), "mcp.lookup");
        assert_eq!(
            calls[0].encrypted_arguments.as_deref(),
            Some(&["ciphertext".into()][..])
        );
    }

    #[tokio::test]
    async fn steer_ack_is_emitted_only_after_db_and_memory_recording() {
        let dir = TempDir::new().unwrap();
        let config = crate::runtime::Config::with_defaults(dir.path().to_path_buf());
        let session = Arc::new(
            Session::with_session_id(config, "steer-ack".into())
                .await
                .unwrap(),
        );
        session.set_current_turn_id("turn-steer-ack").await;
        let events = session.subscribe_turn_events("turn-steer-ack").await;
        let memory_recorded = Arc::new(AtomicBool::new(false));
        session.set_turn_input_after_memory_write_hook(Some({
            let memory_recorded = Arc::clone(&memory_recorded);
            Arc::new(move || memory_recorded.store(true, Ordering::SeqCst))
        }));

        record_pending_input(&session, vec![queued_input("client-steer")])
            .await
            .unwrap();

        let event = events.recv().await.unwrap();
        assert!(memory_recorded.load(Ordering::SeqCst));
        assert!(matches!(
            event.msg,
            EventMsg::UserInputCommitted(UserInputCommittedEvent {
                turn_id,
                client_message_id,
            }) if turn_id == "turn-steer-ack" && client_message_id == "client-steer"
        ));
    }

    #[tokio::test]
    async fn steer_write_failure_does_not_emit_ack() {
        let dir = TempDir::new().unwrap();
        let config = crate::runtime::Config::with_defaults(dir.path().to_path_buf());
        let session = Arc::new(
            Session::with_session_id(config, "steer-no-ack".into())
                .await
                .unwrap(),
        );
        session.set_current_turn_id("turn-steer-no-ack").await;
        let events = session.subscribe_turn_events("turn-steer-no-ack").await;
        session.set_turn_input_after_db_write_hook(Some(Arc::new(|| {
            anyhow::bail!("injected post-DB failure")
        })));

        let error = record_pending_input(&session, vec![queued_input("client-failed")])
            .await
            .unwrap_err();

        assert!(error.to_string().contains("injected post-DB failure"));
        assert!(events.try_recv().is_err());
    }

    #[tokio::test]
    async fn durable_active_steer_write_failure_does_not_emit_ack() {
        let dir = TempDir::new().unwrap();
        let config = crate::runtime::Config::with_defaults(dir.path().to_path_buf());
        let session = Arc::new(
            Session::with_session_id(config, "durable-steer-no-ack".into())
                .await
                .unwrap(),
        );
        let turn_context = Arc::new(TurnContext::new(
            "turn-durable-steer-no-ack".into(),
            1,
            types::InteractionMode::Agent,
            None,
            None,
        ));
        let message_id = turn_context.reserve_mailbox_input().unwrap();
        let input = TurnInput {
            content: "durable steered input".into(),
            image_data_urls: Vec::new(),
            client_message_id: Some("client-durable-failed".into()),
        };
        let payload =
            crate::exec::subagents::encode_main_steer_input_with_context(&input, None).unwrap();
        session
            .services
            .agent_control
            .persist_main_steer_with_id(&session.services.agent_path, message_id, payload)
            .await
            .unwrap();
        let events = session
            .subscribe_turn_events("turn-durable-steer-no-ack")
            .await;
        session.set_turn_input_after_db_write_hook(Some(Arc::new(|| {
            anyhow::bail!("injected durable steer post-DB failure")
        })));

        let error = drain_available_mailbox(&session, &turn_context)
            .await
            .unwrap_err();

        assert!(error
            .to_string()
            .contains("injected durable steer post-DB failure"));
        assert!(events.try_recv().is_err());
    }
}
