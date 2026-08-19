use std::sync::Arc;

use agent_protocol::{Event, EventMsg, TextItem, TurnItem};
use tokio::sync::{mpsc, Mutex};

use crate::thread_state::{ListenerCommand, ThreadActivity, ThreadSnapshot, ThreadState};
use crate::transport::ConnectionRegistry;

fn item_type(item: &TurnItem) -> &'static str {
    match item {
        TurnItem::UserMessage(_) => "user_message",
        TurnItem::HookPrompt(_) => "hook_prompt",
        TurnItem::AgentMessage(_) => "agent_message",
        TurnItem::Plan(_) => "plan",
        TurnItem::Reasoning(_) => "reasoning",
        TurnItem::CommandExecution(_) => "command_execution",
        TurnItem::DynamicToolCall(_) => "dynamic_tool_call",
        TurnItem::McpToolCall(_) => "mcp_tool_call",
        TurnItem::CollabAgentToolCall(_) => "collab_agent_tool_call",
        TurnItem::SubAgentActivity(_) => "subagent_activity",
        TurnItem::WebSearch(_) => "web_search",
        TurnItem::ImageView(_) => "image_view",
        TurnItem::ImageGeneration(_) => "image_generation",
        TurnItem::FileChange(_) => "file_change",
        TurnItem::ContextCompaction(_) => "context_compaction",
        TurnItem::EnteredReviewMode(_) => "entered_review_mode",
        TurnItem::ExitedReviewMode(_) => "exited_review_mode",
        TurnItem::Extension(_) => "extension",
    }
}

fn item_to_proto(item: &TurnItem, status: &str) -> proto::ThreadItem {
    proto::ThreadItem {
        id: item.id().into(),
        item_type: item_type(item).into(),
        status: status.into(),
        payload_json: serde_json::to_string(item).unwrap_or_else(|error| {
            serde_json::json!({"serialization_error": error.to_string()}).to_string()
        }),
    }
}

fn item_payload(item: &TurnItem, status: &str) -> proto::ThreadItemEvent {
    proto::ThreadItemEvent {
        item: Some(item_to_proto(item, status)),
    }
}

fn delta_payload(delta: &agent_protocol::DeltaEvent) -> proto::ThreadDelta {
    proto::ThreadDelta {
        item_id: delta.item_id.clone(),
        delta: delta.delta.clone(),
    }
}

fn control_payload(
    kind: &str,
    request: &agent_protocol::ControlRequestEvent,
) -> proto::ThreadControlRequest {
    proto::ThreadControlRequest {
        kind: kind.into(),
        item_id: request.item_id.clone(),
        request_id: request.request_id.clone(),
        payload_json: serde_json::to_string(&request.payload).unwrap_or_else(|_| "null".into()),
    }
}

fn legacy_item(
    id: &str,
    text: &TextItem,
    kind: fn(TextItem) -> TurnItem,
) -> proto::ThreadItemEvent {
    let mut item = text.clone();
    if item.id.is_empty() {
        item.id = id.into();
    }
    item_payload(&kind(item), "completed")
}

fn extension_payload(
    item_id: impl Into<String>,
    namespace: impl Into<String>,
    value: &impl serde::Serialize,
) -> proto::ThreadExtension {
    proto::ThreadExtension {
        item_id: item_id.into(),
        namespace: namespace.into(),
        payload_json: serde_json::to_string(value).unwrap_or_else(|error| {
            serde_json::json!({"serialization_error": error.to_string()}).to_string()
        }),
    }
}

fn event_to_proto(thread_id: &str, event: &Event) -> proto::ThreadEvent {
    use proto::thread_event::Payload;

    let (turn_id, payload) = match &event.msg {
        EventMsg::TurnStarted(started) => (
            started.turn_id.clone(),
            Payload::TurnStarted(proto::ThreadTurnStarted {
                turn_id: started.turn_id.clone(),
            }),
        ),
        EventMsg::ItemStarted(item) => (
            item.turn_id.clone(),
            Payload::ItemStarted(item_payload(&item.item, "in_progress")),
        ),
        EventMsg::ItemCompleted(item) => {
            if let TurnItem::Extension(extension) = &item.item {
                (
                    item.turn_id.clone(),
                    Payload::Extension(proto::ThreadExtension {
                        item_id: extension.id.clone(),
                        namespace: extension.namespace.clone(),
                        payload_json: serde_json::to_string(&extension.payload)
                            .unwrap_or_else(|_| "null".into()),
                    }),
                )
            } else {
                (
                    item.turn_id.clone(),
                    Payload::ItemCompleted(item_payload(&item.item, "completed")),
                )
            }
        }
        EventMsg::AgentMessageContentDelta(delta) => (
            delta.turn_id.clone(),
            Payload::AgentMessageDelta(delta_payload(delta)),
        ),
        EventMsg::PlanDelta(delta) => (
            delta.turn_id.clone(),
            Payload::PlanDelta(delta_payload(delta)),
        ),
        EventMsg::ReasoningContentDelta(delta) => (
            delta.turn_id.clone(),
            Payload::ReasoningDelta(delta_payload(delta)),
        ),
        EventMsg::ExecCommandOutputDelta(delta) => (
            delta.turn_id.clone(),
            Payload::ExecOutputDelta(delta_payload(delta)),
        ),
        EventMsg::PatchApplyUpdated(delta) => (
            delta.turn_id.clone(),
            Payload::PatchDelta(delta_payload(delta)),
        ),
        EventMsg::ExecApprovalRequest(request) => (
            request.turn_id.clone(),
            Payload::ControlRequest(control_payload("exec_approval", request)),
        ),
        EventMsg::ApplyPatchApprovalRequest(request) => (
            request.turn_id.clone(),
            Payload::ControlRequest(control_payload("apply_patch_approval", request)),
        ),
        EventMsg::RequestPermissions(request) => (
            request.turn_id.clone(),
            Payload::ControlRequest(control_payload("request_permissions", request)),
        ),
        EventMsg::RequestUserInput(request) => (
            request.turn_id.clone(),
            Payload::ControlRequest(control_payload("request_user_input", request)),
        ),
        EventMsg::ElicitationRequest(request) => (
            request.turn_id.clone(),
            Payload::ControlRequest(control_payload("elicitation", request)),
        ),
        EventMsg::DynamicToolCallRequest(request) => (
            request.turn_id.clone(),
            Payload::ControlRequest(control_payload("dynamic_tool_call", request)),
        ),
        EventMsg::DynamicToolCallResponse(request) => (
            request.turn_id.clone(),
            Payload::ControlRequest(control_payload("dynamic_tool_response", request)),
        ),
        EventMsg::McpToolCallBegin(item) | EventMsg::HookStarted(item) => (
            item.turn_id.clone(),
            Payload::ItemStarted(item_payload(&item.item, "in_progress")),
        ),
        EventMsg::McpToolCallEnd(item)
        | EventMsg::HookCompleted(item)
        | EventMsg::SubAgentActivity(item)
        | EventMsg::ContextCompacted(item)
        | EventMsg::LegacyMcpToolCallEnd(item)
        | EventMsg::LegacyPatchApplyEnd(item)
        | EventMsg::LegacyContextCompacted(item)
        | EventMsg::LegacySubAgentActivity(item) => (
            item.turn_id.clone(),
            Payload::ItemCompleted(item_payload(&item.item, "completed")),
        ),
        EventMsg::ContextUsage(usage) => (
            usage.turn_id.clone(),
            Payload::Extension(extension_payload(
                format!("{}:context_usage", usage.turn_id),
                "astro.context_usage",
                usage,
            )),
        ),
        EventMsg::LegacyUserMessage(text) => (
            event.id.clone(),
            Payload::ItemCompleted(legacy_item(&event.id, text, TurnItem::UserMessage)),
        ),
        EventMsg::LegacyAgentMessage(text) => (
            event.id.clone(),
            Payload::ItemCompleted(legacy_item(&event.id, text, TurnItem::AgentMessage)),
        ),
        EventMsg::LegacyReasoning(text) => (
            event.id.clone(),
            Payload::ItemCompleted(legacy_item(&event.id, text, TurnItem::Reasoning)),
        ),
        EventMsg::TokenCount(tokens) => (
            tokens.turn_id.clone().unwrap_or_else(|| event.id.clone()),
            Payload::TokenCount(proto::ThreadTokenCount {
                input_tokens: tokens.input_tokens,
                output_tokens: tokens.output_tokens,
                total_tokens: tokens.total_tokens,
            }),
        ),
        EventMsg::ThreadSettingsApplied(value) => (
            event.id.clone(),
            Payload::Extension(extension_payload(
                format!("{}:settings", event.id),
                "astro.thread_settings",
                value,
            )),
        ),
        EventMsg::ThreadRolledBack(value) => (
            event.id.clone(),
            Payload::Extension(extension_payload(
                format!("{}:rollback", event.id),
                "astro.thread_rollback",
                value,
            )),
        ),
        EventMsg::Error(error) | EventMsg::StreamError(error) => (
            event.id.clone(),
            Payload::Error(proto::ThreadError {
                message: error.message.clone(),
                error_type: error.error_type.clone(),
            }),
        ),
        EventMsg::Warning(error) => (
            event.id.clone(),
            Payload::Warning(proto::ThreadError {
                message: error.message.clone(),
                error_type: error.error_type.clone(),
            }),
        ),
        EventMsg::TurnComplete(completed) => (
            completed.turn_id.clone(),
            Payload::TurnComplete(proto::ThreadTurnComplete {
                last_agent_message: completed.last_agent_message.clone().unwrap_or_default(),
                error: completed.error.as_ref().map(|error| proto::ThreadError {
                    message: error.message.clone(),
                    error_type: error.error_type.clone(),
                }),
                has_error: completed.error.is_some(),
            }),
        ),
        EventMsg::TurnAborted(aborted) => (
            aborted.turn_id.clone().unwrap_or_else(|| event.id.clone()),
            Payload::TurnAborted(proto::ThreadTurnAborted {
                reason: match aborted.reason {
                    agent_protocol::TurnAbortReason::Interrupted => "interrupted",
                    agent_protocol::TurnAbortReason::Replaced => "replaced",
                    agent_protocol::TurnAbortReason::ReviewEnded => "review_ended",
                    agent_protocol::TurnAbortReason::BudgetLimited => "budget_limited",
                }
                .into(),
            }),
        ),
        EventMsg::ShutdownComplete => (event.id.clone(), Payload::ShutdownComplete(true)),
    };

    proto::ThreadEvent {
        thread_id: thread_id.into(),
        turn_id,
        payload: Some(payload),
    }
}

pub async fn run_thread_listener(
    thread_id: String,
    thread: Arc<agent::AstroThread>,
    state: Arc<Mutex<ThreadState>>,
    commands: mpsc::UnboundedSender<ListenerCommand>,
    command_rx: mpsc::UnboundedReceiver<ListenerCommand>,
    connections: ConnectionRegistry,
) {
    run_thread_listener_observed(
        thread_id,
        thread,
        state,
        commands,
        command_rx,
        connections,
        None,
    )
    .await;
}

pub(crate) async fn run_thread_listener_observed(
    thread_id: String,
    thread: Arc<agent::AstroThread>,
    state: Arc<Mutex<ThreadState>>,
    commands: mpsc::UnboundedSender<ListenerCommand>,
    command_rx: mpsc::UnboundedReceiver<ListenerCommand>,
    connections: ConnectionRegistry,
    observed_events: Option<mpsc::UnboundedSender<Event>>,
) {
    let pump_commands = commands.clone();
    let pump = tokio::spawn(async move {
        while let Ok(event) = thread.next_event().await {
            let observed = event.clone();
            if pump_commands
                .send(ListenerCommand::CoreEvent(event))
                .is_err()
            {
                break;
            }
            if let Some(observed_events) = observed_events.as_ref() {
                let _ = observed_events.send(observed);
            }
        }
        let _ = pump_commands.send(ListenerCommand::Stop);
    });
    run_listener_commands(thread_id, state, command_rx, connections).await;
    pump.abort();
    let _ = pump.await;
}

pub async fn run_listener_commands(
    thread_id: String,
    state: Arc<Mutex<ThreadState>>,
    mut commands: mpsc::UnboundedReceiver<ListenerCommand>,
    connections: ConnectionRegistry,
) {
    while let Some(command) = commands.recv().await {
        match command {
            ListenerCommand::CoreEvent(event) => {
                let (subscribers, outbound) = {
                    let mut state = state.lock().await;
                    state.history.track(&event);
                    state.status = match &event.msg {
                        EventMsg::TurnStarted(_) => "running".into(),
                        EventMsg::TurnComplete(completed) if completed.error.is_some() => {
                            "errored".into()
                        }
                        EventMsg::TurnComplete(_) | EventMsg::TurnAborted(_) => "idle".into(),
                        EventMsg::ShutdownComplete => "shutdown".into(),
                        _ => state.status.clone(),
                    };
                    let _ = state.activity_tx.send(ThreadActivity {
                        status: state.status.clone(),
                        has_subscribers: !state.subscribers.is_empty(),
                    });
                    (
                        state.subscribers.values().cloned().collect::<Vec<_>>(),
                        event_to_proto(&thread_id, &event),
                    )
                };
                let mut disconnected = Vec::new();
                for subscription in subscribers {
                    if !connections
                        .send_to_generation(&subscription, outbound.clone())
                        .await
                    {
                        disconnected.push(subscription);
                    }
                }
                if !disconnected.is_empty() {
                    let mut state = state.lock().await;
                    for subscription in disconnected {
                        if state
                            .subscribers
                            .get(subscription.connection_id())
                            .is_some_and(|current| current == &subscription)
                        {
                            state.subscribers.remove(subscription.connection_id());
                        }
                    }
                    let _ = state.activity_tx.send(ThreadActivity {
                        status: state.status.clone(),
                        has_subscribers: !state.subscribers.is_empty(),
                    });
                }
            }
            ListenerCommand::Resume {
                subscription,
                include_turns,
                reply,
            } => {
                let snapshot = {
                    let mut state = state.lock().await;
                    state
                        .subscribers
                        .insert(subscription.connection_id().into(), subscription);
                    let _ = state.activity_tx.send(ThreadActivity {
                        status: state.status.clone(),
                        has_subscribers: true,
                    });
                    ThreadSnapshot {
                        thread_id: thread_id.clone(),
                        status: state.status.clone(),
                        turns: if include_turns {
                            state.history.completed_turns().to_vec()
                        } else {
                            Vec::new()
                        },
                        active_turn: state.history.active_turn_snapshot(),
                    }
                };
                let _ = reply.send(snapshot);
            }
            ListenerCommand::Unsubscribe {
                subscription,
                reply,
            } => {
                let mut state = state.lock().await;
                if state
                    .subscribers
                    .get(subscription.connection_id())
                    .is_some_and(|current| current == &subscription)
                {
                    state.subscribers.remove(subscription.connection_id());
                }
                let _ = state.activity_tx.send(ThreadActivity {
                    status: state.status.clone(),
                    has_subscribers: !state.subscribers.is_empty(),
                });
                if let Some(reply) = reply {
                    let _ = reply.send(());
                }
            }
            ListenerCommand::Stop => break,
        }
    }
}
