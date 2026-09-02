use std::collections::HashSet;
use std::sync::Arc;

use agent_protocol::{Event, EventMsg, TurnItem};
use tokio::sync::{mpsc, Mutex};

use crate::thread_state::{ListenerCommand, ThreadActivity, ThreadSnapshot, ThreadState};
use crate::transport::ConnectionRegistry;

fn background_sink_retention_timeout() -> std::time::Duration {
    crate::BACKGROUND_EXTENSION_SINK_TIMEOUT
}

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
        EventMsg::RealtimeConversationStarted(value) => (
            event.id.clone(),
            Payload::Realtime(proto::ThreadRealtimeEvent {
                kind: "started".into(),
                payload_json: serde_json::to_string(value).unwrap_or_else(|_| "null".into()),
            }),
        ),
        EventMsg::RealtimeConversationSdp(value) => (
            event.id.clone(),
            Payload::Realtime(proto::ThreadRealtimeEvent {
                kind: "sdp".into(),
                payload_json: serde_json::to_string(value).unwrap_or_else(|_| "null".into()),
            }),
        ),
        EventMsg::RealtimeConversationRealtime(value) => (
            event.id.clone(),
            Payload::Realtime(proto::ThreadRealtimeEvent {
                kind: "event".into(),
                payload_json: serde_json::to_string(&value.payload)
                    .unwrap_or_else(|_| "null".into()),
            }),
        ),
        EventMsg::RealtimeConversationClosed(value) => (
            event.id.clone(),
            Payload::Realtime(proto::ThreadRealtimeEvent {
                kind: "closed".into(),
                payload_json: serde_json::to_string(value).unwrap_or_else(|_| "null".into()),
            }),
        ),
        EventMsg::RealtimeConversationListVoicesResponse(value) => (
            event.id.clone(),
            Payload::Realtime(proto::ThreadRealtimeEvent {
                kind: "voices".into(),
                payload_json: serde_json::to_string(value).unwrap_or_else(|_| "null".into()),
            }),
        ),
        EventMsg::TurnStarted(started) => (
            started.turn_id.clone(),
            Payload::TurnStarted(proto::ThreadTurnStarted {
                turn_id: started.turn_id.clone(),
            }),
        ),
        EventMsg::UserInputCommitted(committed) => (
            committed.turn_id.clone(),
            Payload::Extension(extension_payload(
                format!(
                    "{}:user_input_committed:{}",
                    committed.turn_id, committed.client_message_id
                ),
                "astro.user_input_committed",
                committed,
            )),
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
        EventMsg::GuardianAssessment(assessment) => (
            assessment.turn_id.clone(),
            Payload::Extension(extension_payload(
                assessment.id.clone(),
                "astro.guardian_assessment",
                assessment,
            )),
        ),
        EventMsg::DynamicToolCallRequest(request) => (
            request.turn_id.clone(),
            Payload::ControlRequest(control_payload("dynamic_tool_call", request)),
        ),
        EventMsg::DynamicToolCallResponse(request) => (
            request.turn_id.clone(),
            Payload::ControlRequest(control_payload("dynamic_tool_response", request)),
        ),
        EventMsg::McpToolCallBegin(item) => (
            item.turn_id.clone(),
            Payload::ItemStarted(item_payload(&item.item, "in_progress")),
        ),
        EventMsg::McpToolCallEnd(item)
        | EventMsg::SubAgentActivity(item)
        | EventMsg::ContextCompacted(item) => (
            item.turn_id.clone(),
            Payload::ItemCompleted(item_payload(&item.item, "completed")),
        ),
        EventMsg::HookStarted(hook) => (
            hook.turn_id.clone().unwrap_or_else(|| event.id.clone()),
            Payload::Extension(extension_payload(
                hook.run.id.clone(),
                "astro.hook_started",
                hook,
            )),
        ),
        EventMsg::HookCompleted(hook) => (
            hook.turn_id.clone().unwrap_or_else(|| event.id.clone()),
            Payload::Extension(extension_payload(
                hook.run.id.clone(),
                "astro.hook_completed",
                hook,
            )),
        ),
        EventMsg::ContextUsage(usage) => (
            usage.turn_id.clone(),
            Payload::Extension(extension_payload(
                format!("{}:context_usage", usage.turn_id),
                "astro.context_usage",
                usage,
            )),
        ),
        EventMsg::TokenCount(tokens) => (
            tokens.turn_id.clone().unwrap_or_else(|| event.id.clone()),
            Payload::TokenCount(proto::ThreadTokenCount {
                input_tokens: tokens.input_tokens,
                input_tokens_include_cache: tokens.input_tokens_include_cache,
                output_tokens: tokens.output_tokens,
                total_tokens: tokens.total_tokens,
                uncached_input_tokens: tokens.uncached_input_tokens,
                cache_read_tokens: tokens.cache_read_tokens,
                cache_write_tokens: tokens.cache_write_tokens,
                reasoning_tokens: tokens.reasoning_tokens,
                request_count: tokens.request_count,
                provider_total_tokens: tokens.provider_total_tokens.unwrap_or_default(),
                provider_total_tokens_reported: tokens.provider_total_tokens.is_some(),
                cache_read_reported: tokens.cache_read_reported,
                cache_write_reported: tokens.cache_write_reported,
                reasoning_reported: tokens.reasoning_reported,
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
            let command = if observed_events.is_some() {
                ListenerCommand::ObservedCoreEvent(event)
            } else {
                ListenerCommand::CoreEvent(event)
            };
            if pump_commands.send(command).is_err() {
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
    struct ExtensionWaiter {
        item_id: String,
        payload_json: String,
        reply: tokio::sync::oneshot::Sender<()>,
    }
    let mut extension_waiters: std::collections::HashMap<uuid::Uuid, ExtensionWaiter> =
        std::collections::HashMap::new();
    while let Some(command) = commands.recv().await {
        match command {
            command @ (ListenerCommand::CoreEvent(_) | ListenerCommand::ObservedCoreEvent(_)) => {
                let (event, retain_background_sink) = match command {
                    ListenerCommand::CoreEvent(event) => (event, false),
                    ListenerCommand::ObservedCoreEvent(event) => (event, true),
                    _ => unreachable!("matched core event variants"),
                };
                let (subscribers, retained_connection_ids, outbound) = {
                    let mut state = state.lock().await;
                    state.history.track(&event);
                    if let EventMsg::TurnComplete(completed) = &event.msg {
                        if retain_background_sink && completed.error.is_none() {
                            let retained = state.subscribers.keys().cloned().collect();
                            state
                                .background_extension_sinks
                                .insert(completed.turn_id.clone(), retained);
                            let commands = state.listener_command_tx.clone();
                            let turn_id = completed.turn_id.clone();
                            tokio::spawn(async move {
                                tokio::time::sleep(background_sink_retention_timeout()).await;
                                let _ = commands
                                    .send(ListenerCommand::ExpireBackgroundSink { turn_id });
                            });
                        }
                    }
                    state.status = match &event.msg {
                        EventMsg::TurnStarted(_) => "running".into(),
                        EventMsg::TurnComplete(completed) if completed.error.is_some() => {
                            "errored".into()
                        }
                        EventMsg::TurnComplete(_) | EventMsg::TurnAborted(_) => "idle".into(),
                        EventMsg::ShutdownComplete => "shutdown".into(),
                        _ => state.status.clone(),
                    };
                    let subscribers = state.subscribers.values().cloned().collect::<HashSet<_>>();
                    let mut retained_connection_ids = HashSet::new();
                    let completed_background_turn = match &event.msg {
                        EventMsg::ItemCompleted(item) => match &item.item {
                            TurnItem::Extension(extension) => {
                                if let Some(sink) =
                                    state.background_extension_sinks.get(&item.turn_id)
                                {
                                    retained_connection_ids.extend(sink.iter().cloned());
                                }
                                (extension.namespace == "astro.background_complete")
                                    .then(|| item.turn_id.clone())
                            }
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(turn_id) = completed_background_turn {
                        state.background_extension_sinks.remove(&turn_id);
                    }
                    let _ = state.activity_tx.send(ThreadActivity {
                        status: state.status.clone(),
                        has_subscribers: !state.subscribers.is_empty()
                            || !state.background_extension_sinks.is_empty(),
                    });
                    (
                        subscribers.into_iter().collect::<Vec<_>>(),
                        retained_connection_ids.into_iter().collect::<Vec<_>>(),
                        event_to_proto(&thread_id, &event),
                    )
                };
                let mut disconnected_subscribers = Vec::new();
                let mut delivered_connection_ids = HashSet::new();
                for subscription in subscribers {
                    if connections
                        .send_to_generation(&subscription, outbound.clone())
                        .await
                    {
                        delivered_connection_ids.insert(subscription.connection_id().to_string());
                    } else {
                        disconnected_subscribers.push(subscription);
                    }
                }
                for connection_id in retained_connection_ids {
                    if delivered_connection_ids.contains(&connection_id) {
                        continue;
                    }
                    if connections.send_to(&connection_id, outbound.clone()).await {
                        delivered_connection_ids.insert(connection_id);
                    }
                }
                if !disconnected_subscribers.is_empty() {
                    let mut state = state.lock().await;
                    for subscription in disconnected_subscribers {
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
                        has_subscribers: !state.subscribers.is_empty()
                            || !state.background_extension_sinks.is_empty(),
                    });
                }
                if let EventMsg::ItemCompleted(item) = &event.msg {
                    if let TurnItem::Extension(extension) = &item.item {
                        if let Ok(payload_json) = serde_json::to_string(&item.item) {
                            let completed = extension_waiters
                                .iter()
                                .filter_map(|(waiter_id, waiter)| {
                                    (waiter.item_id == extension.id
                                        && waiter.payload_json == payload_json)
                                        .then_some(*waiter_id)
                                })
                                .collect::<Vec<_>>();
                            for waiter_id in completed {
                                if let Some(waiter) = extension_waiters.remove(&waiter_id) {
                                    let _ = waiter.reply.send(());
                                }
                            }
                        }
                    }
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
                    let mut pending_background_turn_ids = state
                        .background_extension_sinks
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>();
                    pending_background_turn_ids.sort();
                    ThreadSnapshot {
                        thread_id: thread_id.clone(),
                        status: state.status.clone(),
                        model: None,
                        reasoning_effort: None,
                        turns: if include_turns {
                            state.history.completed_turns().to_vec()
                        } else {
                            Vec::new()
                        },
                        active_turn: state.history.active_turn_snapshot(),
                        pending_background_turn_ids,
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
                    has_subscribers: !state.subscribers.is_empty()
                        || !state.background_extension_sinks.is_empty(),
                });
                if let Some(reply) = reply {
                    let _ = reply.send(());
                }
            }
            ListenerCommand::WaitForExtension {
                waiter_id,
                item_id,
                payload_json,
                reply,
            } => {
                if state
                    .lock()
                    .await
                    .history
                    .contains_item_payload(&item_id, &payload_json)
                {
                    let _ = reply.send(());
                } else {
                    extension_waiters.insert(
                        waiter_id,
                        ExtensionWaiter {
                            item_id,
                            payload_json,
                            reply,
                        },
                    );
                }
            }
            ListenerCommand::CancelExtensionWaiter { waiter_id } => {
                extension_waiters.remove(&waiter_id);
            }
            #[cfg(test)]
            ListenerCommand::ExtensionWaiterCount { reply } => {
                let _ = reply.send(extension_waiters.len());
            }
            ListenerCommand::ExpireBackgroundSink { turn_id } => {
                let (subscribers, retained_connection_ids, outbound) = {
                    let mut state = state.lock().await;
                    let Some(retained_connection_ids) =
                        state.background_extension_sinks.remove(&turn_id)
                    else {
                        continue;
                    };
                    let subscribers = state.subscribers.values().cloned().collect::<Vec<_>>();
                    let _ = state.activity_tx.send(ThreadActivity {
                        status: state.status.clone(),
                        has_subscribers: !state.subscribers.is_empty()
                            || !state.background_extension_sinks.is_empty(),
                    });
                    let item_id = format!("{turn_id}:background_expired");
                    (
                        subscribers,
                        retained_connection_ids,
                        proto::ThreadEvent {
                            thread_id: thread_id.clone(),
                            turn_id: turn_id.clone(),
                            payload: Some(proto::thread_event::Payload::Extension(
                                proto::ThreadExtension {
                                    item_id,
                                    namespace: "astro.background_expired".into(),
                                    payload_json: serde_json::json!({
                                        "turn_id": turn_id,
                                    })
                                    .to_string(),
                                },
                            )),
                        },
                    )
                };
                let mut disconnected_subscribers = Vec::new();
                let mut delivered_connection_ids = HashSet::new();
                for subscription in subscribers {
                    if connections
                        .send_to_generation(&subscription, outbound.clone())
                        .await
                    {
                        delivered_connection_ids.insert(subscription.connection_id().to_string());
                    } else {
                        disconnected_subscribers.push(subscription);
                    }
                }
                for connection_id in retained_connection_ids {
                    if !delivered_connection_ids.contains(&connection_id) {
                        let _ = connections.send_to(&connection_id, outbound.clone()).await;
                    }
                }
                if !disconnected_subscribers.is_empty() {
                    let mut state = state.lock().await;
                    for subscription in disconnected_subscribers {
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
                        has_subscribers: !state.subscribers.is_empty()
                            || !state.background_extension_sinks.is_empty(),
                    });
                }
            }
            ListenerCommand::Stop => break,
        }
    }
}

#[cfg(test)]
mod background_sink_tests {
    use super::*;

    #[test]
    fn committed_input_becomes_unified_thread_extension() {
        let mapped = event_to_proto(
            "thread-1",
            &Event {
                id: "turn-1".into(),
                msg: EventMsg::UserInputCommitted(agent_protocol::UserInputCommittedEvent {
                    turn_id: "turn-1".into(),
                    client_message_id: "queued-7".into(),
                }),
            },
        );
        let Some(proto::thread_event::Payload::Extension(extension)) = mapped.payload else {
            panic!("expected committed-input extension");
        };
        assert_eq!(mapped.turn_id, "turn-1");
        assert_eq!(extension.namespace, "astro.user_input_committed");
        assert!(extension.payload_json.contains("queued-7"));
    }

    #[test]
    fn retained_sink_outlives_work_and_completion_marker_window() {
        assert!(
            background_sink_retention_timeout()
                > crate::POST_TURN_SIDE_EFFECT_TIMEOUT
                    + crate::POST_TURN_COMPLETION_MARKER_TIMEOUT,
            "the sink must remain available while timed-out background work emits its completion marker"
        );
    }
}
