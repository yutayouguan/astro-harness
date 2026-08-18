use std::pin::Pin;

use agent_protocol::{Op, TurnInput, TurnInputMode, TurnInputRequest, TurnInputSubmission};
use futures::Stream;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use super::AstroServiceImpl;
use crate::{ListenerCommand, ThreadSnapshot, TurnSnapshot};

pub(crate) type ThreadEventsStream =
    Pin<Box<dyn Stream<Item = Result<proto::ThreadEvent, Status>> + Send>>;

#[allow(clippy::result_large_err)]
fn require_connection_id(connection_id: &str) -> Result<&str, Status> {
    let connection_id = connection_id.trim();
    if connection_id.is_empty() {
        Err(Status::invalid_argument("connection_id is required"))
    } else {
        Ok(connection_id)
    }
}

fn snapshot_to_proto(snapshot: ThreadSnapshot) -> proto::ThreadSnapshot {
    let active_turn = snapshot.active_turn.map(turn_to_proto);
    proto::ThreadSnapshot {
        thread_id: snapshot.thread_id,
        status: snapshot.status,
        turns: snapshot.turns.into_iter().map(turn_to_proto).collect(),
        has_active_turn: active_turn.is_some(),
        active_turn,
    }
}

fn turn_to_proto(turn: TurnSnapshot) -> proto::ThreadTurn {
    let error = turn.error.map(|error| proto::ThreadError {
        message: error.message,
        error_type: error.error_type,
    });
    proto::ThreadTurn {
        id: turn.id,
        status: turn.status,
        items: turn
            .items
            .into_iter()
            .map(|item| proto::ThreadItem {
                id: item.id,
                item_type: item_type(&item.item).into(),
                status: item.status,
                payload_json: serde_json::to_string(&item.item).unwrap_or_else(|_| "null".into()),
            })
            .collect(),
        last_agent_message: turn.last_agent_message.unwrap_or_default(),
        has_error: error.is_some(),
        error,
    }
}

fn item_type(item: &agent_protocol::TurnItem) -> &'static str {
    use agent_protocol::TurnItem;
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

async fn resume(
    managed: &crate::ManagedThread,
    connection_id: String,
    include_turns: bool,
) -> Result<ThreadSnapshot, Status> {
    let (reply, receive) = tokio::sync::oneshot::channel();
    managed
        .commands
        .send(ListenerCommand::Resume {
            connection_id,
            include_turns,
            reply,
        })
        .map_err(|_| Status::unavailable("thread listener stopped"))?;
    receive
        .await
        .map_err(|_| Status::unavailable("thread listener stopped"))
}

pub(crate) async fn subscribe_thread_events(
    service: &AstroServiceImpl,
    request: Request<proto::SubscribeThreadEventsRequest>,
) -> Result<Response<ThreadEventsStream>, Status> {
    let connection_id = require_connection_id(&request.get_ref().connection_id)?.to_string();
    let (receiver, cancel, generation) = service.connections.register(connection_id).await;
    let registry = service.connections.clone();
    let thread_states = service.thread_states.clone();
    let cleanup_connection_id = request.get_ref().connection_id.clone();
    // Keep one slot reserved for the single terminal slow-consumer status.
    let (outbound, stream_rx) = tokio::sync::mpsc::channel(crate::transport::CHANNEL_CAPACITY + 1);
    tokio::spawn(async move {
        let mut receiver = receiver;
        loop {
            tokio::select! {
                biased;
                () = outbound.closed() => break,
                () = cancel.cancelled() => {
                    let _ = outbound.try_send(Err(Status::resource_exhausted("slow thread-event consumer")));
                    break;
                }
                event = receiver.recv() => match event {
                    Some(event) => {
                        if outbound.capacity() <= 1 {
                            let _ = outbound.try_send(Err(Status::resource_exhausted("slow thread-event consumer")));
                            break;
                        }
                        if outbound.try_send(Ok(event)).is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
        if registry.remove_generation(&generation).await {
            thread_states.unsubscribe_all(&cleanup_connection_id).await;
        }
    });
    Ok(Response::new(Box::pin(ReceiverStream::new(stream_rx))))
}

pub(crate) async fn submit_turn(
    service: &AstroServiceImpl,
    request: Request<proto::SubmitTurnRequest>,
) -> Result<Response<proto::SubmitTurnResponse>, Status> {
    let req = request.into_inner();
    let connection_id = require_connection_id(&req.connection_id)?.to_string();
    if !service.connections.contains(&connection_id).await {
        return Err(Status::failed_precondition("connection is not subscribed"));
    }
    let chat = req
        .chat
        .ok_or_else(|| Status::invalid_argument("chat request is required"))?;
    let thread_id = chat.session_id.trim();
    if thread_id.is_empty() {
        return Err(Status::invalid_argument("chat.session_id is required"));
    }
    let managed = service.get_or_create_thread(thread_id).await?;
    resume(&managed, connection_id, false).await?;
    service
        .configure_thread_from_chat(&managed.runtime, &chat)
        .await?;
    managed
        .runtime
        .submit(Op::ThreadSettings {
            settings: serde_json::json!({
                "provider": chat.provider,
                "model": chat.model,
                "interaction_mode": chat.interaction_mode,
                "project_root": chat.project_root,
            }),
        })
        .await
        .map_err(|error| Status::unavailable(error.to_string()))?;
    let mode = match req.mode.as_str() {
        "" | "start_or_steer" => TurnInputMode::StartOrSteer,
        "start_if_idle" => TurnInputMode::StartIfIdle,
        "steer" if req.expected_turn_id.trim().is_empty() => {
            return Err(Status::invalid_argument(
                "expected_turn_id is required for steer",
            ));
        }
        "steer" => TurnInputMode::Steer {
            expected_turn_id: req.expected_turn_id,
        },
        other => {
            return Err(Status::invalid_argument(format!(
                "unsupported mode: {other}"
            )))
        }
    };
    let turn_request = turn_request_from_chat(&chat)?;
    let (submission_id, submission) = managed
        .runtime
        .submit_turn(turn_request, mode)
        .await
        .map_err(|error| Status::failed_precondition(error.to_string()))?;
    let (turn_id, disposition, reason) = match submission {
        TurnInputSubmission::Started { turn_id } => (turn_id, "started", String::new()),
        TurnInputSubmission::Steered { turn_id } => (turn_id, "steered", String::new()),
        TurnInputSubmission::NotSubmitted { reason } => (String::new(), "not_submitted", reason),
    };
    Ok(Response::new(proto::SubmitTurnResponse {
        submission_id,
        turn_id,
        disposition: disposition.into(),
        reason,
    }))
}

pub(crate) async fn resume_thread(
    service: &AstroServiceImpl,
    request: Request<proto::ResumeThreadRequest>,
) -> Result<Response<proto::ResumeThreadResponse>, Status> {
    let req = request.into_inner();
    let connection_id = require_connection_id(&req.connection_id)?.to_string();
    if !service.connections.contains(&connection_id).await {
        return Err(Status::failed_precondition("connection is not subscribed"));
    }
    let thread_id = req.thread_id.trim();
    if thread_id.is_empty() {
        return Err(Status::invalid_argument("thread_id is required"));
    }
    let managed = service.get_or_create_thread(thread_id).await?;
    let snapshot = resume(&managed, connection_id, req.include_turns).await?;
    Ok(Response::new(proto::ResumeThreadResponse {
        thread: Some(snapshot_to_proto(snapshot)),
    }))
}

pub(crate) async fn unsubscribe_thread(
    service: &AstroServiceImpl,
    request: Request<proto::UnsubscribeThreadRequest>,
) -> Result<Response<proto::Empty>, Status> {
    let req = request.into_inner();
    let connection_id = require_connection_id(&req.connection_id)?.to_string();
    let thread_id = req.thread_id.trim();
    if thread_id.is_empty() {
        return Err(Status::invalid_argument("thread_id is required"));
    }
    let managed = service
        .threads
        .get(thread_id)
        .await
        .ok_or_else(|| Status::not_found("thread is not loaded"))?;
    managed
        .commands
        .send(ListenerCommand::Unsubscribe { connection_id })
        .map_err(|_| Status::unavailable("thread listener stopped"))?;
    Ok(Response::new(proto::Empty {}))
}

#[allow(clippy::result_large_err)]
pub(crate) fn turn_request_from_chat(
    chat: &proto::ChatRequest,
) -> Result<TurnInputRequest, Status> {
    if chat.content.trim().is_empty() && chat.images.is_empty() {
        return Err(Status::invalid_argument("content or images are required"));
    }
    let mut image_data_urls = Vec::with_capacity(chat.images.len());
    for image in &chat.images {
        let mime = image.mime.trim();
        let data = image.data_base64.trim();
        if mime.is_empty() || data.is_empty() {
            return Err(Status::invalid_argument(
                "each image requires mime and data_base64",
            ));
        }
        image_data_urls.push(format!("data:{mime};base64,{data}"));
    }
    Ok(TurnInputRequest {
        input: vec![TurnInput {
            content: chat.content.clone(),
            image_data_urls,
        }],
    })
}

pub(crate) fn thread_event_to_chat_events(event: proto::ThreadEvent) -> Vec<proto::ChatEvent> {
    use proto::chat_event::Payload as ChatPayload;
    use proto::thread_event::Payload;
    let turn_id = event.turn_id;
    let Some(payload) = event.payload else {
        return vec![chat_error("thread event has no payload")];
    };
    match payload {
        Payload::TurnStarted(started) => vec![proto::ChatEvent {
            payload: Some(ChatPayload::RunStarted(proto::RunStartedEvent {
                thread_id: event.thread_id,
                run_id: started.turn_id,
            })),
        }],
        Payload::ItemStarted(item) => vec![map_item_event(item, true)],
        Payload::ItemCompleted(item) => vec![map_item_event(item, false)],
        Payload::AgentMessageDelta(delta) => vec![proto::ChatEvent {
            payload: Some(ChatPayload::Token(delta.delta)),
        }],
        Payload::ReasoningDelta(delta) => vec![proto::ChatEvent {
            payload: Some(ChatPayload::Reasoning(delta.delta)),
        }],
        Payload::PlanDelta(delta) => vec![activity(delta.item_id, "plan_delta", delta.delta)],
        Payload::ExecOutputDelta(delta) => {
            vec![activity(delta.item_id, "exec_output_delta", delta.delta)]
        }
        Payload::PatchDelta(delta) => vec![activity(delta.item_id, "patch_delta", delta.delta)],
        Payload::ControlRequest(control) => control_chat_events(turn_id, control),
        Payload::TokenCount(tokens) => vec![proto::ChatEvent {
            payload: Some(ChatPayload::Usage(proto::UsageEvent {
                prompt_tokens: tokens.input_tokens.min(u32::MAX.into()) as u32,
                completion_tokens: tokens.output_tokens.min(u32::MAX.into()) as u32,
                total_tokens: tokens.total_tokens.min(u32::MAX.into()) as u32,
            })),
        }],
        Payload::Error(error) => vec![chat_error(error.message)],
        Payload::Warning(warning) => vec![activity(
            turn_id,
            "warning",
            serde_json::json!({"message":warning.message,"error_type":warning.error_type})
                .to_string(),
        )],
        Payload::TurnComplete(complete) => terminal_chat_events(
            turn_id,
            if complete.has_error {
                "error"
            } else {
                "success"
            },
            Vec::new(),
        ),
        Payload::TurnAborted(aborted) => terminal_chat_events(
            turn_id,
            "interrupt",
            vec![proto::Interrupt {
                id: String::new(),
                reason: aborted.reason,
                message: String::new(),
                tool_call_id: String::new(),
                response_schema_json: String::new(),
                expires_at: String::new(),
                metadata_json: String::new(),
            }],
        ),
        Payload::Extension(extension)
            if matches!(
                extension.namespace.as_str(),
                "astro.thread_settings" | "astro.thread_rollback"
            ) =>
        {
            Vec::new()
        }
        Payload::Extension(extension) if extension.namespace == "astro.context_usage" => {
            vec![context_usage_chat_event(&extension.payload_json)]
        }
        Payload::Extension(extension) => vec![activity(
            extension.item_id,
            &extension.namespace,
            extension.payload_json,
        )],
        Payload::ShutdownComplete(_) => vec![proto::ChatEvent {
            payload: Some(ChatPayload::Done(true)),
        }],
    }
}

fn terminal_chat_events(
    run_id: String,
    outcome_type: &str,
    interrupts: Vec<proto::Interrupt>,
) -> Vec<proto::ChatEvent> {
    vec![
        proto::ChatEvent {
            payload: Some(proto::chat_event::Payload::RunFinished(
                proto::RunFinishedEvent {
                    run_id,
                    outcome_type: outcome_type.into(),
                    interrupts,
                },
            )),
        },
        proto::ChatEvent {
            payload: Some(proto::chat_event::Payload::Done(true)),
        },
    ]
}

fn control_chat_events(
    run_id: String,
    control: proto::ThreadControlRequest,
) -> Vec<proto::ChatEvent> {
    let payload = serde_json::from_str::<serde_json::Value>(&control.payload_json)
        .unwrap_or(serde_json::Value::Null);
    let surface = activity(
        format!("a2ui-surface-{}", control.item_id),
        "a2ui-surface",
        serde_json::json!({
            "operations": payload.get("operations").cloned().unwrap_or_default(),
        })
        .to_string(),
    );
    let waiting = proto::ChatEvent {
        payload: Some(proto::chat_event::Payload::RunFinished(
            proto::RunFinishedEvent {
                run_id,
                outcome_type: "hitl_waiting".into(),
                interrupts: vec![proto::Interrupt {
                    id: control.request_id,
                    reason: payload
                        .get("reason")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .into(),
                    message: payload
                        .get("message")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .into(),
                    tool_call_id: control.item_id,
                    response_schema_json: payload
                        .get("response_schema")
                        .cloned()
                        .unwrap_or_default()
                        .to_string(),
                    expires_at: String::new(),
                    metadata_json: String::new(),
                }],
            },
        )),
    };
    vec![surface, waiting]
}

fn chat_error(message: impl Into<String>) -> proto::ChatEvent {
    proto::ChatEvent {
        payload: Some(proto::chat_event::Payload::Error(message.into())),
    }
}

fn activity(
    message_id: impl Into<String>,
    activity_type: &str,
    content_json: impl Into<String>,
) -> proto::ChatEvent {
    proto::ChatEvent {
        payload: Some(proto::chat_event::Payload::Activity(proto::ActivityEvent {
            message_id: message_id.into(),
            activity_type: activity_type.into(),
            content_json: content_json.into(),
            replace: false,
        })),
    }
}

fn map_item_event(item_event: proto::ThreadItemEvent, started: bool) -> proto::ChatEvent {
    let Some(item) = item_event.item else {
        return chat_error("thread item event has no item");
    };
    match serde_json::from_str::<agent_protocol::TurnItem>(&item.payload_json) {
        Ok(agent_protocol::TurnItem::CommandExecution(tool))
        | Ok(agent_protocol::TurnItem::DynamicToolCall(tool))
        | Ok(agent_protocol::TurnItem::McpToolCall(tool))
        | Ok(agent_protocol::TurnItem::CollabAgentToolCall(tool))
        | Ok(agent_protocol::TurnItem::WebSearch(tool))
        | Ok(agent_protocol::TurnItem::ImageView(tool))
        | Ok(agent_protocol::TurnItem::ImageGeneration(tool))
        | Ok(agent_protocol::TurnItem::FileChange(tool)) => proto::ChatEvent {
            payload: Some(proto::chat_event::Payload::ToolCall(proto::ToolCallEvent {
                id: tool.id,
                name: tool.name,
                arguments_json: tool.arguments.to_string(),
                result: tool
                    .output
                    .map(|value| value.to_string())
                    .unwrap_or_default(),
                media: tool.media.into_iter().map(media_to_proto).collect(),
                phase: if started { "started" } else { "completed" }.into(),
            })),
        },
        Ok(agent_protocol::TurnItem::AgentMessage(text)) => proto::ChatEvent {
            payload: Some(proto::chat_event::Payload::Token(text.content)),
        },
        Ok(agent_protocol::TurnItem::Reasoning(text)) => proto::ChatEvent {
            payload: Some(proto::chat_event::Payload::Reasoning(text.content)),
        },
        Ok(agent_protocol::TurnItem::HookPrompt(text)) => proto::ChatEvent {
            payload: Some(proto::chat_event::Payload::Hook(proto::HookEvent {
                name: "hook_prompt".into(),
                detail: text.content,
                outcome: if started { "started" } else { "completed" }.into(),
            })),
        },
        Ok(_) => activity(item.id, &item.item_type, item.payload_json),
        Err(error) => chat_error(format!("invalid thread item: {error}")),
    }
}

fn media_to_proto(asset: types::MediaAsset) -> proto::MediaAsset {
    let (ref_kind, ref_value) = match asset.reference {
        types::MediaRef::WorkspacePath(path) => ("workspace_path", path),
        types::MediaRef::DataUrl(url) => ("data_url", url),
        types::MediaRef::RemoteUri(uri) => ("remote_uri", uri),
    };
    let kind = match asset.kind {
        types::MediaKind::Image => "image",
        types::MediaKind::Audio => "audio",
        types::MediaKind::Video => "video",
        types::MediaKind::File => "file",
    };
    proto::MediaAsset {
        kind: kind.into(),
        mime_type: asset.mime_type,
        ref_kind: ref_kind.into(),
        ref_value,
        label: asset.label.unwrap_or_default(),
        id: asset.id.unwrap_or_default(),
    }
}

fn context_usage_chat_event(payload: &str) -> proto::ChatEvent {
    let Ok(usage) = serde_json::from_str::<agent_protocol::ContextUsageEvent>(payload) else {
        return chat_error("invalid context usage payload");
    };
    proto::ChatEvent {
        payload: Some(proto::chat_event::Payload::ContextUsage(
            proto::ContextUsageEvent {
                context_window: usage.context_window,
                total_tokens: usage.total_tokens,
                segments: usage
                    .segments
                    .into_iter()
                    .map(|segment| proto::ContextUsageSegment {
                        id: segment.id,
                        tokens: segment.tokens,
                        count: segment.count.unwrap_or_default(),
                        items: segment
                            .items
                            .into_iter()
                            .map(|item| proto::ContextUsageItem {
                                id: item.id,
                                label: item.label,
                                tokens: item.tokens,
                            })
                            .collect(),
                    })
                    .collect(),
                updated_at: usage.updated_at,
                recommend_compact: usage.recommend_compact,
            },
        )),
    }
}

pub(crate) async fn chat(
    service: &AstroServiceImpl,
    request: Request<proto::ChatRequest>,
) -> Result<Response<super::astro_service::ChatStream>, Status> {
    let mut chat = request.into_inner();
    if chat.session_id.trim().is_empty() {
        chat.session_id = uuid::Uuid::new_v4().to_string();
    }
    let connection_id = format!("chat-{}", uuid::Uuid::new_v4());
    let (mut event_rx, cancel, generation) =
        service.connections.register(connection_id.clone()).await;
    let mut subscribed_commands = None;
    let setup = async {
        let managed = service.get_or_create_thread(&chat.session_id).await?;
        service
            .configure_thread_from_chat(&managed.runtime, &chat)
            .await?;
        let turn_request = turn_request_from_chat(&chat)?;
        resume(&managed, connection_id.clone(), false).await?;
        subscribed_commands = Some(managed.commands.clone());
        managed
            .runtime
            .submit(Op::ThreadSettings {
                settings: serde_json::json!({"provider":chat.provider,"model":chat.model}),
            })
            .await
            .map_err(|error| Status::unavailable(error.to_string()))?;
        let (_, submission) = managed
            .runtime
            .submit_turn(turn_request, TurnInputMode::StartOrSteer)
            .await
            .map_err(|error| Status::failed_precondition(error.to_string()))?;
        let turn_id = submission
            .turn_id()
            .map(str::to_owned)
            .ok_or_else(|| Status::failed_precondition("turn input was not submitted"))?;
        Ok::<_, Status>((managed, turn_id))
    }
    .await;
    let (managed, turn_id) = match setup {
        Ok(setup) => setup,
        Err(error) => {
            if let Some(commands) = subscribed_commands {
                let _ = commands.send(ListenerCommand::Unsubscribe {
                    connection_id: connection_id.clone(),
                });
            }
            service.connections.remove_generation(&generation).await;
            return Err(error);
        }
    };
    let registry = service.connections.clone();
    let unsubscribe = managed.commands.clone();
    let cleanup_connection = connection_id.clone();
    let (tx, rx) = tokio::sync::mpsc::channel(128);
    tokio::spawn(async move {
        loop {
            let event = tokio::select! {
                biased;
                () = tx.closed() => break,
                () = cancel.cancelled() => {
                    let _ = tx.send(Err(Status::resource_exhausted("slow thread-event consumer"))).await;
                    break;
                }
                event = event_rx.recv() => match event { Some(event) => event, None => break },
            };
            if event.turn_id != turn_id {
                continue;
            }
            let terminal = matches!(
                event.payload,
                Some(proto::thread_event::Payload::TurnComplete(_))
                    | Some(proto::thread_event::Payload::TurnAborted(_))
            );
            for mapped in thread_event_to_chat_events(event) {
                if tx.send(Ok(mapped)).await.is_err() {
                    break;
                }
            }
            if terminal {
                break;
            }
        }
        let _ = unsubscribe.send(ListenerCommand::Unsubscribe {
            connection_id: cleanup_connection,
        });
        registry.remove_generation(&generation).await;
    });
    Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn terminal_thread_event_maps_to_run_finished_then_done() {
        let mapped = thread_event_to_chat_events(proto::ThreadEvent {
            thread_id: "session-1".into(),
            turn_id: "turn-1".into(),
            payload: Some(proto::thread_event::Payload::TurnComplete(
                proto::ThreadTurnComplete {
                    last_agent_message: "done".into(),
                    error: None,
                    has_error: false,
                },
            )),
        });
        assert!(matches!(
            mapped.as_slice(),
            [proto::ChatEvent { payload: Some(proto::chat_event::Payload::RunFinished(finished)) },
             proto::ChatEvent { payload: Some(proto::chat_event::Payload::Done(true)) }]
                if finished.outcome_type == "success"
        ));
    }

    #[test]
    fn settings_extension_is_not_exposed_to_legacy_chat() {
        let mapped = thread_event_to_chat_events(proto::ThreadEvent {
            thread_id: "session-1".into(),
            turn_id: "settings-1".into(),
            payload: Some(proto::thread_event::Payload::Extension(
                proto::ThreadExtension {
                    item_id: "settings-1".into(),
                    namespace: "astro.thread_settings".into(),
                    payload_json: "{}".into(),
                },
            )),
        });
        assert!(mapped.is_empty());
    }

    #[test]
    fn control_request_preserves_surface_and_interrupt_identity() {
        let mapped = thread_event_to_chat_events(proto::ThreadEvent {
            thread_id: "session-1".into(),
            turn_id: "turn-1".into(),
            payload: Some(proto::thread_event::Payload::ControlRequest(
                proto::ThreadControlRequest {
                    kind: "request_user_input".into(),
                    item_id: "tool-1".into(),
                    request_id: "request-1".into(),
                    payload_json: serde_json::json!({
                        "reason":"confirmation",
                        "message":"continue?",
                        "operations":[],
                        "response_schema":{}
                    })
                    .to_string(),
                },
            )),
        });
        assert!(matches!(
            mapped.as_slice(),
            [proto::ChatEvent { payload: Some(proto::chat_event::Payload::Activity(_)) },
             proto::ChatEvent { payload: Some(proto::chat_event::Payload::RunFinished(finished)) }]
                if finished.outcome_type == "hitl_waiting"
                    && finished.interrupts.first().is_some_and(|interrupt| interrupt.id == "request-1")
        ));
    }

    #[tokio::test]
    async fn cancelled_connection_stream_reports_resource_exhausted_once() {
        let dir = TempDir::new().expect("tempdir");
        memory::ensure_workspace(dir.path()).expect("workspace");
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let mut stream = subscribe_thread_events(
            &service,
            Request::new(proto::SubscribeThreadEventsRequest {
                connection_id: "same-id".into(),
            }),
        )
        .await
        .expect("subscribe")
        .into_inner();
        let (_replacement, _cancel, replacement_generation) =
            service.connections.register("same-id".into()).await;

        let error = stream
            .next()
            .await
            .expect("one cancellation status")
            .expect_err("cancellation must be an error");
        assert_eq!(error.code(), tonic::Code::ResourceExhausted);
        assert!(stream.next().await.is_none());
        service
            .connections
            .remove_generation(&replacement_generation)
            .await;
    }

    #[tokio::test]
    async fn slow_connection_reports_one_resource_exhausted_then_closes() {
        let dir = TempDir::new().expect("tempdir");
        memory::ensure_workspace(dir.path()).expect("workspace");
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let mut stream = subscribe_thread_events(
            &service,
            Request::new(proto::SubscribeThreadEventsRequest {
                connection_id: "slow".into(),
            }),
        )
        .await
        .expect("subscribe")
        .into_inner();
        for index in 0..300 {
            let _ = service
                .connections
                .send_to(
                    "slow",
                    proto::ThreadEvent {
                        thread_id: "thread".into(),
                        turn_id: format!("turn-{index}"),
                        payload: Some(proto::thread_event::Payload::TurnStarted(
                            proto::ThreadTurnStarted {
                                turn_id: format!("turn-{index}"),
                            },
                        )),
                    },
                )
                .await;
            tokio::task::yield_now().await;
        }
        let mut errors = 0;
        while let Some(result) = stream.next().await {
            if let Err(status) = result {
                assert_eq!(status.code(), tonic::Code::ResourceExhausted);
                errors += 1;
            }
        }
        assert_eq!(errors, 1);
    }

    #[tokio::test]
    async fn dropping_stream_removes_exact_connection_generation() {
        let dir = TempDir::new().expect("tempdir");
        memory::ensure_workspace(dir.path()).expect("workspace");
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let stream = subscribe_thread_events(
            &service,
            Request::new(proto::SubscribeThreadEventsRequest {
                connection_id: "drop-me".into(),
            }),
        )
        .await
        .expect("subscribe")
        .into_inner();
        assert!(service.connections.contains("drop-me").await);
        drop(stream);
        for _ in 0..8 {
            if !service.connections.contains("drop-me").await {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("dropped stream generation was not removed");
    }

    #[tokio::test]
    async fn stale_same_id_stream_cleanup_preserves_replacement_subscription() {
        let dir = TempDir::new().expect("tempdir");
        memory::ensure_workspace(dir.path()).expect("workspace");
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let managed = service
            .get_or_create_thread("same-id-thread")
            .await
            .expect("thread");
        let old_stream = subscribe_thread_events(
            &service,
            Request::new(proto::SubscribeThreadEventsRequest {
                connection_id: "same-id-stream".into(),
            }),
        )
        .await
        .expect("old stream")
        .into_inner();
        resume(&managed, "same-id-stream".into(), false)
            .await
            .expect("old resume");
        let replacement_stream = subscribe_thread_events(
            &service,
            Request::new(proto::SubscribeThreadEventsRequest {
                connection_id: "same-id-stream".into(),
            }),
        )
        .await
        .expect("replacement stream")
        .into_inner();
        resume(&managed, "same-id-stream".into(), false)
            .await
            .expect("replacement resume");

        drop(old_stream);
        for _ in 0..8 {
            tokio::task::yield_now().await;
        }
        assert!(
            service
                .thread_states
                .has_subscribers("same-id-thread")
                .await
        );

        drop(replacement_stream);
        for _ in 0..8 {
            if !service
                .thread_states
                .has_subscribers("same-id-thread")
                .await
            {
                managed
                    .runtime
                    .submit(agent_protocol::Op::Shutdown)
                    .await
                    .expect("shutdown");
                managed.runtime.wait_terminated().await;
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("replacement stream cleanup did not unsubscribe the connection");
    }

    #[tokio::test]
    async fn thread_hitl_gate_is_resolvable_through_existing_interrupt_rpc() {
        use proto::astro_service_server::AstroService;

        let dir = TempDir::new().expect("tempdir");
        memory::ensure_workspace(dir.path()).expect("workspace");
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let managed = service
            .get_or_create_thread("hitl-thread")
            .await
            .expect("thread");
        service
            .configure_thread_from_chat(
                &managed.runtime,
                &proto::ChatRequest {
                    session_id: "hitl-thread".into(),
                    ..Default::default()
                },
            )
            .await
            .expect("configure controls");
        let gate = service
            .hitl_registry
            .get("hitl-thread")
            .await
            .expect("registered thread gate");
        let resolution = gate
            .begin_wait(agent::Interrupt {
                id: "request-1".into(),
                reason: "confirmation".into(),
                message: "continue?".into(),
                tool_call_id: "tool-1".into(),
                response_schema_json: "{}".into(),
                expires_at: String::new(),
                metadata_json: String::new(),
            })
            .await;

        AstroService::interrupt_resume(
            &service,
            Request::new(proto::InterruptResumeRequest {
                session_id: "hitl-thread".into(),
                resume: vec![proto::InterruptResumeItem {
                    interrupt_id: "request-1".into(),
                    status: "resolved".into(),
                    payload_json: r#"{"approved":true}"#.into(),
                }],
            }),
        )
        .await
        .expect("interrupt response");
        let resolved = resolution.await.expect("gate response");
        assert_eq!(resolved.status, "resolved");
        assert_eq!(resolved.payload_json, r#"{"approved":true}"#);

        managed
            .runtime
            .submit(agent_protocol::Op::Shutdown)
            .await
            .expect("shutdown");
        managed.runtime.wait_terminated().await;
    }
}
