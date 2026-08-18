//! 多轮工具循环：从 gRPC handler 收拢到 Agent 层的核心编排。
//!
//! **关键不变量**
//! - Pause/Cancel 对齐 Rig：`wait_if_paused` 先于上游 poll；取消时通过 `Abortable` 中止 Provider 流
//! - 每轮 assistant 回复必须写入 `SessionState.history`（含 tool_calls）后再执行工具
//! - 迭代预算对齐 Hermes：默认 90 轮；`code_exec` 独占轮可 refund；耗尽后无工具强制总结再 Done
//! - usage 采用覆盖式累加，兼容 Google 等 Provider 的累计式 `usageMetadata`
//!
//! HITL park/resume 桥见 [`super::hitl_bridge`]；预算耗尽后的总结轮见 [`super::summary`]。

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use agent_protocol::{ControlRequestEvent, Event, EventMsg, ItemEvent, ToolStatus, TurnInput};
use futures::stream::{AbortHandle, Abortable};
use futures::StreamExt;
use providers::ProviderConfig;
use providers::{PauseControl, Usage};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use types::ChatTarget;

use super::lifecycle::{
    emit, emit_delta, emit_hook_completed, emit_hook_started, emit_response_items_completed,
    emit_text_item_started, emit_usage, tool_turn_item,
};
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
use crate::tasks::{RegularTask, SessionTaskResult, TurnCancelled};

/// `pre_verify` 单次 turn 内允许的最多验证轮次（含首次结束尝试）。
const MAX_VERIFY_ATTEMPTS: usize = 2;

/// 模型只返回思考/推理内容而没有文本回复时，允许的最大重试次数。
const MAX_THINKING_ONLY_RETRIES: usize = 1;

/// Per-index buffer that delays argument events until the provider call id is known.
#[derive(Default)]
struct PendingToolArgumentEvents {
    item_id: Option<String>,
    deltas: Vec<types::ToolCallDelta>,
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

pub(crate) struct MultiTurnTaskArgs {
    pub(crate) session: Arc<Session>,
    pub(crate) targets: Vec<ChatTarget>,
    pub(crate) base_config: ProviderConfig,
    pub(crate) input: Vec<TurnInput>,
    pub(crate) system_prompt: Option<String>,
    pub(crate) pause: Arc<PauseControl>,
    pub(crate) hitl_gate: Option<Arc<HitlGate>>,
    pub(crate) chat_override: Option<super::provider::ChatOverride>,
}

impl MultiTurnStreamArgs {
    fn into_task_args(
        self,
    ) -> (
        MultiTurnTaskArgs,
        mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    ) {
        let Self {
            session,
            targets,
            base_config,
            input,
            system_prompt,
            pause,
            hitl_gate,
            tx,
            chat_override,
        } = self;
        (
            MultiTurnTaskArgs {
                session,
                targets,
                base_config,
                input,
                system_prompt,
                pause,
                hitl_gate,
                chat_override,
            },
            tx,
        )
    }
}

/// 多轮工具调用流式循环：从 gRPC handler 收拢到 Agent 层的核心编排。
///
/// 每轮：锁定 session → 流式 LLM → 累积 tool_calls → 执行工具 → 写入历史 → 下一轮。
/// 取消/暂停时清理 abort handle 并以 usage + Done 收尾。
/// `hitl_gate` 非空时，confirm/clarify/危险命令在同回合 park，不结束 run。
pub async fn run_multi_turn_stream(args: MultiTurnStreamArgs) {
    let (task_args, legacy_tx) = args.into_task_args();
    match install_multi_turn_task(task_args).await {
        Ok(installed) => {
            let InstalledMultiTurn {
                session,
                session_id,
                turn_id,
                events,
            } = installed;
            let forward = tokio::spawn(forward_unified_to_legacy(
                events,
                turn_id.clone(),
                legacy_tx,
            ));
            session.wait_for_task(&turn_id).await;
            let _ = forward.await;
            tracing::info!(session_id = %session_id, turn_id = %turn_id, "turn finished");
        }
        Err(error) => {
            let _ = legacy_tx
                .send(Ok(MultiTurnStreamItem::Error(error.message)))
                .await;
            let _ = legacy_tx.send(Ok(MultiTurnStreamItem::Done)).await;
        }
    }
}

pub(crate) struct InstalledMultiTurn {
    pub(crate) session: Arc<Session>,
    pub(crate) session_id: String,
    pub(crate) turn_id: String,
    pub(crate) events: async_channel::Receiver<Event>,
}

pub(crate) struct MultiTurnInstallError {
    pub(crate) turn_id: String,
    pub(crate) message: String,
}

pub(crate) async fn install_multi_turn_task(
    args: MultiTurnTaskArgs,
) -> Result<InstalledMultiTurn, MultiTurnInstallError> {
    let MultiTurnTaskArgs {
        session,
        targets,
        base_config,
        input,
        system_prompt,
        pause,
        hitl_gate,
        chat_override,
    } = args;
    let session_id = session.session_id().to_string();
    let sub_id = uuid::Uuid::new_v4().to_string();
    let events = session.subscribe_turn_events(&sub_id).await;
    let turn_context = session.create_turn_context(sub_id.clone()).await;
    let task = RegularTask::new(RunTurnArgs {
        session: session.clone(),
        turn_context: Arc::clone(&turn_context),
        targets,
        base_config,
        system_prompt,
        pause,
        hitl_gate,
        chat_override,
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
        session_id,
        turn_id: sub_id,
        events,
    })
}

fn legacy_items_from_event(event: Event) -> Vec<MultiTurnStreamItem> {
    let run_id = event.id;
    match event.msg {
        EventMsg::TurnStarted(_) => vec![MultiTurnStreamItem::RunStarted {
            thread_id: String::new(),
            run_id,
        }],
        EventMsg::AgentMessageContentDelta(delta) => vec![MultiTurnStreamItem::Assistant(
            StreamedAssistantContent::Text(delta.delta),
        )],
        EventMsg::ReasoningContentDelta(delta) => vec![MultiTurnStreamItem::Assistant(
            StreamedAssistantContent::Reasoning(delta.delta),
        )],
        EventMsg::DynamicToolCallRequest(request) => {
            vec![MultiTurnStreamItem::Assistant(
                StreamedAssistantContent::ToolCallDelta(types::ToolCallDelta {
                    index: request
                        .payload
                        .get("index")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or_default() as u32,
                    id: Some(request.item_id),
                    name: request
                        .payload
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string),
                    arguments: request
                        .payload
                        .get("delta")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string),
                    signature: None,
                }),
            )]
        }
        EventMsg::RequestUserInput(request)
        | EventMsg::RequestPermissions(request)
        | EventMsg::ExecApprovalRequest(request)
        | EventMsg::ApplyPatchApprovalRequest(request) => vec![
            MultiTurnStreamItem::Activity {
                message_id: format!("a2ui-surface-{}", request.item_id),
                activity_type: "a2ui-surface".into(),
                content_json: serde_json::json!({
                    "operations": request.payload.get("operations").cloned().unwrap_or_default(),
                })
                .to_string(),
                replace: true,
            },
            MultiTurnStreamItem::RunFinished {
                run_id,
                outcome_type: "hitl_waiting".into(),
                interrupts_json: serde_json::json!([{
                    "id": request.request_id,
                    "reason": request.payload.get("reason").cloned().unwrap_or_default(),
                    "message": request.payload.get("message").cloned().unwrap_or_default(),
                    "tool_call_id": request.item_id,
                    "response_schema_json": request
                        .payload
                        .get("response_schema")
                        .cloned()
                        .unwrap_or_default()
                        .to_string(),
                }])
                .to_string(),
            },
        ],
        EventMsg::ItemStarted(item) => match item.item {
            agent_protocol::TurnItem::CommandExecution(tool)
            | agent_protocol::TurnItem::DynamicToolCall(tool)
            | agent_protocol::TurnItem::McpToolCall(tool)
            | agent_protocol::TurnItem::CollabAgentToolCall(tool) => {
                vec![MultiTurnStreamItem::ToolStarted {
                    id: tool.id,
                    name: tool.name,
                    arguments_json: tool.arguments.to_string(),
                }]
            }
            _ => Vec::new(),
        },
        EventMsg::ItemCompleted(item) => match item.item {
            agent_protocol::TurnItem::CommandExecution(tool)
            | agent_protocol::TurnItem::DynamicToolCall(tool)
            | agent_protocol::TurnItem::McpToolCall(tool)
            | agent_protocol::TurnItem::CollabAgentToolCall(tool) => {
                vec![MultiTurnStreamItem::ToolResult {
                    id: tool.id,
                    name: tool.name,
                    arguments_json: tool.arguments.to_string(),
                    result: tool
                        .output
                        .map(|value| match value {
                            serde_json::Value::String(text) => text,
                            other => other.to_string(),
                        })
                        .unwrap_or_default(),
                    media: tool.media,
                }]
            }
            agent_protocol::TurnItem::Extension(extension)
                if extension.namespace == "astro.memory" =>
            {
                vec![MultiTurnStreamItem::MemoryUpdate {
                    op: extension
                        .payload
                        .get("op")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("memory")
                        .to_string(),
                    content: extension
                        .payload
                        .get("content")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                }]
            }
            agent_protocol::TurnItem::Extension(extension)
                if extension.namespace == "astro.a2ui" =>
            {
                vec![MultiTurnStreamItem::Activity {
                    message_id: extension.id,
                    activity_type: "a2ui-surface".into(),
                    content_json: serde_json::json!({
                        "operations": extension
                            .payload
                            .get("operations")
                            .cloned()
                            .unwrap_or_default(),
                    })
                    .to_string(),
                    replace: extension
                        .payload
                        .get("replace")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true),
                }]
            }
            _ => Vec::new(),
        },
        EventMsg::ContextUsage(context) => {
            vec![MultiTurnStreamItem::ContextUsage(
                crate::prompt::context_usage::ContextUsageSnapshot {
                    context_window: context.context_window,
                    total_tokens: context.total_tokens,
                    segments: context
                        .segments
                        .into_iter()
                        .map(
                            |segment| crate::prompt::context_usage::ContextUsageSegment {
                                id: segment.id,
                                tokens: segment.tokens,
                                meta: segment.count.map(|count| {
                                    crate::prompt::context_usage::ContextUsageSegmentMeta {
                                        count: Some(count),
                                    }
                                }),
                                items: segment
                                    .items
                                    .into_iter()
                                    .map(|item| crate::prompt::context_usage::ContextUsageItem {
                                        id: item.id,
                                        label: item.label,
                                        tokens: item.tokens,
                                    })
                                    .collect(),
                            },
                        )
                        .collect(),
                    updated_at: context.updated_at,
                    recommend_compact: context.recommend_compact,
                },
            )]
        }
        EventMsg::TokenCount(tokens) => vec![MultiTurnStreamItem::Assistant(
            StreamedAssistantContent::FinalUsage(Usage {
                input_tokens: u32::try_from(tokens.input_tokens).unwrap_or(u32::MAX),
                output_tokens: u32::try_from(tokens.output_tokens).unwrap_or(u32::MAX),
                cache_read_tokens: u32::try_from(tokens.cache_read_tokens).unwrap_or(u32::MAX),
                cache_write_tokens: u32::try_from(tokens.cache_write_tokens).unwrap_or(u32::MAX),
                reasoning_tokens: u32::try_from(tokens.reasoning_tokens).unwrap_or(u32::MAX),
                request_count: u32::try_from(tokens.request_count).unwrap_or(u32::MAX),
            }),
        )],
        EventMsg::Error(error) => vec![MultiTurnStreamItem::Error(error.message)],
        EventMsg::TurnComplete(complete) => vec![
            MultiTurnStreamItem::RunFinished {
                run_id,
                outcome_type: if complete.error.is_some() {
                    "error".into()
                } else {
                    "success".into()
                },
                interrupts_json: "[]".into(),
            },
            MultiTurnStreamItem::Done,
        ],
        EventMsg::TurnAborted(_) => vec![
            MultiTurnStreamItem::RunFinished {
                run_id,
                outcome_type: "interrupt".into(),
                interrupts_json: "[]".into(),
            },
            MultiTurnStreamItem::Done,
        ],
        _ => Vec::new(),
    }
}

async fn forward_unified_to_legacy(
    rx: async_channel::Receiver<Event>,
    turn_id: String,
    tx: mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
) {
    loop {
        let Ok(event) = rx.recv().await else {
            return;
        };
        if event.id != turn_id {
            continue;
        }
        let terminal = event.msg.is_terminal();
        for item in legacy_items_from_event(event) {
            if tx.send(Ok(item)).await.is_err() {
                return;
            }
        }
        if terminal {
            return;
        }
    }
}

/// Legacy stream adapter retained while app-server migrates to unified thread events.
#[doc(hidden)]
pub async fn run_multi_turn_stream_with_chat_fn_legacy(
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
    run_multi_turn_stream(MultiTurnStreamArgs {
        session,
        targets: vec![target],
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

/// Integration-test seam that installs the same [`RegularTask`] used in production.
#[doc(hidden)]
pub async fn run_multi_turn_stream_with_chat_fn(
    session: Arc<Session>,
    turn_context: Arc<TurnContext>,
    input: Vec<TurnInput>,
    chat_fn: super::provider::ChatOverride,
) -> anyhow::Result<()> {
    let args = RunTurnArgs::submitted(
        Arc::clone(&session),
        Arc::clone(&turn_context),
        Some(chat_fn),
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
    targets: Vec<ChatTarget>,
    base_config: ProviderConfig,
    system_prompt: Option<String>,
    pause: Arc<PauseControl>,
    hitl_gate: Option<Arc<HitlGate>>,
    chat_override: Option<super::provider::ChatOverride>,
}

impl RunTurnArgs {
    pub(crate) fn submitted(
        session: Arc<Session>,
        turn_context: Arc<TurnContext>,
        chat_override: Option<super::provider::ChatOverride>,
    ) -> Self {
        let mut targets = session.chat_targets();
        let provider = session.chat_provider();
        let model = session.chat_model();
        let api_key = session.chat_api_key();
        let base_url = session.chat_base_url();
        if targets.is_empty() {
            targets.push(ChatTarget {
                provider_id: provider.clone(),
                backend_id: provider,
                model: model.clone(),
                api_key: api_key.clone(),
                base_url: base_url.clone(),
            });
        }
        let base_config = ProviderConfig {
            model,
            api_key,
            base_url: (!base_url.is_empty()).then_some(base_url),
            temperature: session.temperature(),
            additional_params: session.additional_params(),
            ..ProviderConfig::default()
        };
        Self {
            session,
            turn_context,
            targets,
            base_config,
            system_prompt: None,
            pause: PauseControl::new(),
            hitl_gate: None,
            chat_override,
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
    for input in pending_input {
        session.record_turn_input(input).await?;
    }
    Ok(())
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

/// Codex-aligned regular turn loop shared by foreground and background adapters.
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
        pause,
        hitl_gate,
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

        pre_llm_maintenance(&session, &turn_context).await;

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
        let tool_specs = step_context.tool_specs.clone();

        emit_context_usage(&session, &turn_context, &history, &tool_specs).await;

        let raw_stream = match run_sampling_request(
            &session,
            &turn_context,
            &streamer,
            &system_prompt,
            &history,
            tool_specs,
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

        let (abort_handle, abort_reg) = AbortHandle::new_pair();
        pause.attach_abort(abort_handle);
        let mut stream = Abortable::new(raw_stream, abort_reg);

        let mut full_response = String::new();
        let mut full_reasoning = String::new();
        let mut thought_signature: Option<String> = None;
        let mut tool_acc = types::ToolCallAccumulator::new();
        let mut tool_argument_events: HashMap<u32, PendingToolArgumentEvents> = HashMap::new();
        let mut tool_call_indices = BTreeSet::new();
        let mut round_usage: Option<Usage> = None;
        let assistant_item_id = uuid::Uuid::new_v4().to_string();
        let reasoning_item_id = uuid::Uuid::new_v4().to_string();
        let mut reasoning_started = false;
        emit_text_item_started(&session, &turn_context, assistant_item_id.clone(), false).await;

        loop {
            if !pause.wait_if_paused().await {
                pause.clear_abort();
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
                Some(Ok(StreamedAssistantContent::Text(text))) => {
                    full_response.push_str(&text);
                    emit_delta(&session, &turn_context, &assistant_item_id, text, false).await;
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
                        emit_tool_argument_events(&session, &turn_context, &item_id, buffered)
                            .await;
                    }
                }
                Some(Ok(StreamedAssistantContent::FinalUsage(u))) => {
                    round_usage = Some(u);
                }
                Some(Ok(StreamedAssistantContent::Citations(cites))) => {
                    let _ = cites;
                }
                Some(Ok(StreamedAssistantContent::InteractionId(_))) => {}
                Some(Err(err)) => {
                    pause.clear_abort();
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

        for index in &tool_call_indices {
            let pending = tool_argument_events
                .get_mut(index)
                .expect("tool argument index collected above");
            let item_id = pending
                .item_id
                .get_or_insert_with(|| uuid::Uuid::new_v4().to_string())
                .clone();
            let buffered = std::mem::take(&mut pending.deltas);
            emit_tool_argument_events(&session, &turn_context, &item_id, buffered).await;
        }

        let mut native_calls = tool_acc.finish();
        for (call, index) in native_calls.iter_mut().zip(tool_call_indices) {
            if let Some(item_id) = tool_argument_events
                .get(&index)
                .and_then(|pending| pending.item_id.as_ref())
            {
                call.id.clone_from(item_id);
            }
        }
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

        // `pre_verify` hook
        if calls.is_empty() {
            let verify_outcome = {
                let agent = session.as_ref();
                if agent.turn_wrote_disk().await && verify_attempt < MAX_VERIFY_ATTEMPTS {
                    verify_attempt += 1;
                    let sid = agent.session_id().to_string();
                    let turn_id = agent.current_turn_id().await;
                    let hook_item =
                        emit_hook_started(&session, &turn_context, ::hooks::PRE_VERIFY).await;
                    let outcome = agent.fire_hook(
                        ::hooks::PRE_VERIFY,
                        ::hooks::HookPayload {
                            session_id: sid,
                            turn_id,
                            message: Some(full_response.clone()),
                            detail: format!("attempt={verify_attempt}"),
                            ..Default::default()
                        },
                    );
                    emit_hook_completed(&session, &turn_context, hook_item, ::hooks::PRE_VERIFY)
                        .await;
                    Some(outcome)
                } else {
                    None
                }
            };
            if let Some(::hooks::HookOutcome::KeepGoing(prompt)) = verify_outcome {
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
                    assistant_item_id,
                    full_response.clone(),
                    reasoning_item_id,
                    full_reasoning.clone(),
                )
                .await;
                if let Err(err) = agent
                    .record_user_message(&format!("[astro:hook-context]\n{prompt}"))
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
        }

        {
            let agent = session.as_ref();
            let sid = agent.session_id().to_string();
            let turn_id = agent.current_turn_id().await;
            let transform_hook =
                emit_hook_started(&session, &turn_context, ::hooks::TRANSFORM_LLM_OUTPUT).await;
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
            emit_hook_completed(
                &session,
                &turn_context,
                transform_hook,
                ::hooks::TRANSFORM_LLM_OUTPUT,
            )
            .await;
            if let ::hooks::HookOutcome::ReplaceText(s) = transformed {
                full_response = s;
            }
            let post_hook =
                emit_hook_started(&session, &turn_context, ::hooks::POST_LLM_CALL).await;
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
            emit_hook_completed(&session, &turn_context, post_hook, ::hooks::POST_LLM_CALL).await;
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
            assistant_item_id,
            full_response.clone(),
            reasoning_item_id,
            full_reasoning.clone(),
        )
        .await;

        if calls.is_empty() {
            let pending_input = turn_context.take_pending_input_or_close();
            if !pending_input.is_empty() {
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
            need_summary = false;
            break;
        }

        for call in &calls {
            emit(
                &session,
                &turn_context,
                EventMsg::ItemStarted(ItemEvent {
                    turn_id: turn_context.sub_id().to_string(),
                    item: tool_turn_item(
                        call.id.clone(),
                        call.name.clone(),
                        call.arguments.clone(),
                        None,
                        Vec::new(),
                        ToolStatus::InProgress,
                    ),
                }),
            )
            .await;
        }

        run_state.set_phase(RunPhase::ExecutingTools);
        let force_serial = {
            let agent = session.as_ref();
            let names: Vec<&str> = calls.iter().map(|c| c.name.as_str()).collect();
            let registry = agent.tool_registry().await;
            registry.any_needs_confirmation(&names)
                || registry.any_exclusive_access(&names)
                || calls
                    .iter()
                    .any(|c| tool_may_require_permission(&c.name, &c.arguments))
        };

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

        if post_tool_maintenance(&session, &turn_context, &calls).await {
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

    {
        let agent = session.as_ref();
        let sid = agent.session_id().to_string();
        let turn = agent.session_turn().await;
        let turn_id = agent.current_turn_id().await;
        let hook_item = emit_hook_started(&session, &turn_context, ::hooks::ON_SESSION_END).await;
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
        emit_hook_completed(&session, &turn_context, hook_item, ::hooks::ON_SESSION_END).await;
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

/// 先完成 Session task 安装，再返回可消费的 [`MultiTurnStream`]。
///
/// channel 容量为 32；消费者 drop 后发送方通过 [`emit`] 返回 `false` 自然退出。
pub async fn stream_multi_turn(
    session: Arc<Session>,
    targets: Vec<ChatTarget>,
    base_config: ProviderConfig,
    input: Vec<TurnInput>,
    pause: Arc<PauseControl>,
) -> MultiTurnStream {
    stream_multi_turn_with_hitl(session, targets, base_config, input, pause, None).await
}

/// 带 HITL 闸门的多轮流。
pub async fn stream_multi_turn_with_hitl(
    session: Arc<Session>,
    targets: Vec<ChatTarget>,
    base_config: ProviderConfig,
    input: Vec<TurnInput>,
    pause: Arc<PauseControl>,
    hitl_gate: Option<Arc<HitlGate>>,
) -> MultiTurnStream {
    let (tx, rx) = mpsc::channel(32);
    let legacy_tx = tx.clone();
    let installed = install_multi_turn_task(MultiTurnTaskArgs {
        session,
        targets,
        base_config,
        input,
        system_prompt: None,
        pause,
        hitl_gate,
        chat_override: None,
    })
    .await;
    match installed {
        Ok(installed) => {
            let InstalledMultiTurn {
                session,
                session_id,
                turn_id,
                events,
            } = installed;
            tokio::spawn(async move {
                let forward = tokio::spawn(forward_unified_to_legacy(
                    events,
                    turn_id.clone(),
                    legacy_tx,
                ));
                session.wait_for_task(&turn_id).await;
                let _ = forward.await;
                tracing::info!(session_id = %session_id, turn_id = %turn_id, "turn finished");
            });
        }
        Err(error) => {
            let _ = legacy_tx
                .send(Ok(MultiTurnStreamItem::Error(error.message)))
                .await;
            let _ = legacy_tx.send(Ok(MultiTurnStreamItem::Done)).await;
        }
    }
    Box::pin(futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    }))
}
