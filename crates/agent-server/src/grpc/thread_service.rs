use std::pin::Pin;

use agent_protocol::{Op, TurnInput, TurnInputMode, TurnInputRequest, TurnInputSubmission};
use futures::Stream;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use super::AstroServiceImpl;
use crate::transport::ConnectionGenerationKey;
use crate::{ListenerCommand, ThreadSnapshot, TurnSnapshot};

#[derive(Debug)]
struct ValidatedChatRequest {
    turn_request: Option<TurnInputRequest>,
    interaction_mode: types::InteractionMode,
    resume_items: Vec<agent::ResumeItem>,
}

struct PreparedResume {
    managed: std::sync::Arc<crate::ManagedThread>,
    gate: std::sync::Arc<agent::HitlGate>,
    turn_id: String,
}

#[cfg(test)]
#[derive(Default)]
struct ResumeResolveBarrier {
    reached: tokio::sync::Notify,
    release: tokio::sync::Notify,
    subscription: std::sync::Mutex<Option<ConnectionGenerationKey>>,
}

#[cfg(test)]
fn resume_resolve_barriers() -> &'static std::sync::Mutex<
    std::collections::HashMap<String, std::sync::Arc<ResumeResolveBarrier>>,
> {
    static BARRIERS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<ResumeResolveBarrier>>>,
    > = std::sync::OnceLock::new();
    BARRIERS.get_or_init(Default::default)
}

#[cfg(test)]
fn install_resume_resolve_barrier(thread_id: &str) -> std::sync::Arc<ResumeResolveBarrier> {
    let barrier = std::sync::Arc::new(ResumeResolveBarrier::default());
    resume_resolve_barriers()
        .lock()
        .expect("resume barrier registry")
        .insert(thread_id.into(), barrier.clone());
    barrier
}

#[cfg(test)]
async fn wait_at_resume_resolve_barrier(thread_id: &str, subscription: &ConnectionGenerationKey) {
    let barrier = resume_resolve_barriers()
        .lock()
        .expect("resume barrier registry")
        .remove(thread_id);
    if let Some(barrier) = barrier {
        *barrier
            .subscription
            .lock()
            .expect("resume barrier subscription") = Some(subscription.clone());
        barrier.reached.notify_one();
        barrier.release.notified().await;
    }
}

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
        pending_background_turn_ids: snapshot.pending_background_turn_ids,
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
    subscription: crate::transport::ConnectionGenerationKey,
    include_turns: bool,
) -> Result<ThreadSnapshot, Status> {
    let (reply, receive) = tokio::sync::oneshot::channel();
    managed
        .commands
        .send(ListenerCommand::Resume {
            subscription,
            include_turns,
            reply,
        })
        .map_err(|_| Status::unavailable("thread listener stopped"))?;
    receive
        .await
        .map_err(|_| Status::unavailable("thread listener stopped"))
}

fn submit_turn_response(
    submission_id: String,
    submission: TurnInputSubmission,
) -> Response<proto::SubmitTurnResponse> {
    let (turn_id, disposition, reason) = match submission {
        TurnInputSubmission::Started { turn_id } => (turn_id, "started", String::new()),
        TurnInputSubmission::Steered { turn_id } => (turn_id, "steered", String::new()),
        TurnInputSubmission::NotSubmitted { reason } => (String::new(), "not_submitted", reason),
    };
    Response::new(proto::SubmitTurnResponse {
        submission_id,
        turn_id,
        disposition: disposition.into(),
        reason,
    })
}

pub(crate) async fn subscribe_thread_events(
    service: &AstroServiceImpl,
    request: Request<proto::SubscribeThreadEventsRequest>,
) -> Result<Response<ThreadEventsStream>, Status> {
    let connection_id = require_connection_id(&request.get_ref().connection_id)?.to_string();
    let (receiver, cancel, generation) = service.connections.register(connection_id).await;
    let registry = service.connections.clone();
    let thread_states = service.thread_states.clone();
    let cleanup_subscription = generation.key().clone();
    // 预留一个槽位用于单个终端慢消费者状态。
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
        registry.remove_generation(&generation).await;
        thread_states
            .unsubscribe_all_detached(&cleanup_subscription)
            .await;
    });
    Ok(Response::new(Box::pin(ReceiverStream::new(stream_rx))))
}

pub(crate) async fn submit_turn(
    service: &AstroServiceImpl,
    request: Request<proto::SubmitTurnRequest>,
) -> Result<Response<proto::SubmitTurnResponse>, Status> {
    let req = request.into_inner();
    let connection_id = require_connection_id(&req.connection_id)?.to_string();
    let chat = req
        .chat
        .ok_or_else(|| Status::invalid_argument("chat request is required"))?;
    let thread_id = chat.session_id.trim();
    if thread_id.is_empty() {
        return Err(Status::invalid_argument("chat.session_id is required"));
    }
    let mode = validate_turn_mode(&req.mode, &req.expected_turn_id)?;
    let explicit_steer = matches!(&mode, TurnInputMode::Steer { .. });
    let validated = validate_chat_request(&chat)?;
    let subscription = service
        .connections
        .current_generation_key(&connection_id)
        .await
        .ok_or_else(|| Status::failed_precondition("connection is not subscribed"))?;
    if let Some(prepared) =
        prepare_resume_before_side_effects(service, thread_id, &validated.resume_items).await?
    {
        resume(&prepared.managed, subscription.clone(), false).await?;
        #[cfg(test)]
        wait_at_resume_resolve_barrier(thread_id, &subscription).await;
        if let Err(error) = prepared.gate.resolve(&validated.resume_items).await {
            return Err(Status::invalid_argument(format!(
                "invalid resume_json: {error}"
            )));
        }
        return Ok(Response::new(proto::SubmitTurnResponse {
            submission_id: String::new(),
            turn_id: prepared.turn_id,
            disposition: "resumed".into(),
            reason: String::new(),
        }));
    }
    let managed = service.get_or_create_thread(thread_id).await?;
    resume(&managed, subscription.clone(), false).await?;
    let submit = async {
        service
            .configure_thread_from_chat(&managed.runtime, &chat)
            .await?;
        debug_assert_eq!(
            managed.runtime.session().interaction_mode().await,
            validated.interaction_mode
        );
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
        let submitted = managed
            .runtime
            .submit_turn(
                validated
                    .turn_request
                    .expect("non-resume validation must produce turn input"),
                mode,
            )
            .await;
        match submitted {
            Ok(submitted) => Ok(submitted),
            Err(error) if explicit_steer => {
                if let Some(agent_protocol::TurnInputError::Invalid(reason)) =
                    error.downcast_ref::<agent_protocol::TurnInputError>()
                {
                    let reason = if reason == "no active turn available for steering" {
                        "no_active_turn".to_string()
                    } else {
                        reason.clone()
                    };
                    Ok((String::new(), TurnInputSubmission::NotSubmitted { reason }))
                } else {
                    Err(Status::failed_precondition(error.to_string()))
                }
            }
            Err(error) => Err(Status::failed_precondition(error.to_string())),
        }
    }
    .await;
    match submit {
        Ok((submission_id, submission)) => Ok(submit_turn_response(submission_id, submission)),
        Err(error) => Err(error),
    }
}

#[allow(clippy::result_large_err)]
fn validate_turn_mode(mode: &str, expected_turn_id: &str) -> Result<TurnInputMode, Status> {
    match mode {
        "" | "start_or_steer" => Ok(TurnInputMode::StartOrSteer),
        "start_if_idle" => Ok(TurnInputMode::StartIfIdle),
        "steer" if expected_turn_id.trim().is_empty() => Err(Status::invalid_argument(
            "expected_turn_id is required for steer",
        )),
        "steer" => Ok(TurnInputMode::Steer {
            expected_turn_id: expected_turn_id.trim().into(),
        }),
        other => Err(Status::invalid_argument(format!(
            "unsupported mode: {other}"
        ))),
    }
}

pub(crate) async fn resume_thread(
    service: &AstroServiceImpl,
    request: Request<proto::ResumeThreadRequest>,
) -> Result<Response<proto::ResumeThreadResponse>, Status> {
    let req = request.into_inner();
    let connection_id = require_connection_id(&req.connection_id)?.to_string();
    let thread_id = req.thread_id.trim();
    if thread_id.is_empty() {
        return Err(Status::invalid_argument("thread_id is required"));
    }
    let subscription = service
        .connections
        .current_generation_key(&connection_id)
        .await
        .ok_or_else(|| Status::failed_precondition("connection is not subscribed"))?;
    let managed = service.get_or_create_thread(thread_id).await?;
    let snapshot = resume(&managed, subscription, req.include_turns).await?;
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
    let subscription = service
        .connections
        .current_generation_key(&connection_id)
        .await
        .ok_or_else(|| Status::failed_precondition("connection is not subscribed"))?;
    let managed = service
        .threads
        .get(thread_id)
        .await
        .ok_or_else(|| Status::not_found("thread is not loaded"))?;
    unsubscribe_and_wait(&managed.commands, subscription).await?;
    Ok(Response::new(proto::Empty {}))
}

async fn unsubscribe_and_wait(
    commands: &tokio::sync::mpsc::UnboundedSender<ListenerCommand>,
    subscription: ConnectionGenerationKey,
) -> Result<(), Status> {
    let (reply, receive) = tokio::sync::oneshot::channel();
    commands
        .send(ListenerCommand::Unsubscribe {
            subscription,
            reply: Some(reply),
        })
        .map_err(|_| Status::unavailable("thread listener stopped"))?;
    receive
        .await
        .map_err(|_| Status::unavailable("thread listener stopped before unsubscribe completed"))
}

#[allow(clippy::result_large_err)]
pub(crate) fn turn_request_from_chat(
    chat: &proto::ChatRequest,
) -> Result<TurnInputRequest, Status> {
    turn_request_from_chat_with_requirement(chat, true)
}

#[allow(clippy::result_large_err)]
fn turn_request_from_chat_with_requirement(
    chat: &proto::ChatRequest,
    require_input: bool,
) -> Result<TurnInputRequest, Status> {
    if require_input && chat.content.trim().is_empty() && chat.images.is_empty() {
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
            client_message_id: (!chat.client_message_id.trim().is_empty())
                .then(|| chat.client_message_id.trim().to_string()),
        }],
    })
}

#[allow(clippy::result_large_err)]
fn validate_chat_request(chat: &proto::ChatRequest) -> Result<ValidatedChatRequest, Status> {
    if !chat.tool_names.is_empty() {
        return Err(Status::invalid_argument(
            "tool_names overrides are not supported by the Thread runtime",
        ));
    }
    if !chat.use_memory {
        return Err(Status::invalid_argument(
            "use_memory=false is not supported by the Thread runtime",
        ));
    }
    let interaction_mode =
        types::InteractionMode::parse(&chat.interaction_mode).ok_or_else(|| {
            Status::invalid_argument(format!(
                "unsupported interaction_mode: {}",
                chat.interaction_mode.trim().to_ascii_lowercase()
            ))
        })?;
    if let Some(temperature) = chat.temperature {
        if !temperature.is_finite() || !(0.0..=2.0).contains(&temperature) {
            return Err(Status::invalid_argument(
                "temperature must be between 0 and 2",
            ));
        }
    }
    if !chat.additional_params_json.trim().is_empty() {
        let params: serde_json::Value = serde_json::from_str(&chat.additional_params_json)
            .map_err(|error| {
                Status::invalid_argument(format!("invalid additional_params_json: {error}"))
            })?;
        if !params.is_object() {
            return Err(Status::invalid_argument(
                "additional_params_json must be an object",
            ));
        }
    }
    let resume_items = if chat.resume_json.trim().is_empty() {
        Vec::new()
    } else {
        super::interrupt_store::parse_resume_items_json(&chat.resume_json)
            .map_err(Status::invalid_argument)?
    };
    let turn_request = if resume_items.is_empty() {
        Some(turn_request_from_chat(chat)?)
    } else {
        turn_request_from_chat_with_requirement(chat, false)?;
        None
    };
    Ok(ValidatedChatRequest {
        turn_request,
        interaction_mode,
        resume_items,
    })
}

async fn prepare_resume_before_side_effects(
    service: &AstroServiceImpl,
    thread_id: &str,
    resume_items: &[agent::ResumeItem],
) -> Result<Option<PreparedResume>, Status> {
    if resume_items.is_empty() {
        return Ok(None);
    }
    let managed = service
        .threads
        .get(thread_id)
        .await
        .ok_or_else(|| Status::invalid_argument("resume_json requires a loaded thread"))?;
    let turn_id = if let Some(state) = service.thread_states.get(thread_id).await {
        state
            .lock()
            .await
            .history
            .active_turn_snapshot()
            .map(|turn| turn.id)
    } else {
        None
    }
    .or_else(|| match managed.runtime.status() {
        agent::AgentStatus::Running { turn_id } => Some(turn_id),
        _ => None,
    })
    .ok_or_else(|| Status::invalid_argument("resume_json requires an active turn"))?;
    let gate = service
        .hitl_registry
        .get(thread_id)
        .await
        .ok_or_else(|| Status::invalid_argument("thread has no pending HITL"))?;
    gate.validate_resolve(resume_items)
        .await
        .map_err(|error| Status::invalid_argument(format!("invalid resume_json: {error}")))?;
    Ok(Some(PreparedResume {
        managed,
        gate,
        turn_id,
    }))
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn snapshot_proto_preserves_authoritative_pending_background_turns() {
        let mapped = snapshot_to_proto(ThreadSnapshot {
            thread_id: "session-1".into(),
            status: "idle".into(),
            turns: vec![],
            active_turn: None,
            pending_background_turn_ids: vec!["turn-1".into(), "turn-2".into()],
        });
        assert_eq!(mapped.pending_background_turn_ids, vec!["turn-1", "turn-2"]);
    }

    fn valid_chat_request() -> proto::ChatRequest {
        proto::ChatRequest {
            session_id: "validation-thread".into(),
            content: "hello".into(),
            use_memory: true,
            interaction_mode: "agent".into(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn configure_thread_preserves_primary_and_fallback_api_modes() {
        let dir = TempDir::new().expect("tempdir");
        memory::ensure_workspace(dir.path()).expect("workspace");
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let managed = service
            .get_or_create_thread("api-mode-thread")
            .await
            .expect("thread");

        service
            .configure_thread_from_chat(
                &managed.runtime,
                &proto::ChatRequest {
                    session_id: "api-mode-thread".into(),
                    provider: "deepseek".into(),
                    model: "deepseek-v4-flash".into(),
                    api_mode: "chat_completions".into(),
                    chat_fallbacks: vec![proto::ChatFallbackTarget {
                        provider: "openai".into(),
                        model: "gpt-5.6".into(),
                        api_mode: "responses".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            )
            .await
            .expect("configure thread");

        let targets = managed.runtime.session().chat_targets();
        assert_eq!(targets[0].api_mode, "chat_completions");
        assert_eq!(targets[1].api_mode, "responses");

        managed
            .runtime
            .submit(agent_protocol::Op::Shutdown)
            .await
            .expect("shutdown");
        managed.runtime.wait_terminated().await;
    }

    #[tokio::test]
    async fn configure_thread_rejects_removed_ask_mode_without_mutating_session() {
        let dir = TempDir::new().expect("tempdir");
        memory::ensure_workspace(dir.path()).expect("workspace");
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let managed = service
            .get_or_create_thread("removed-ask-mode")
            .await
            .expect("thread");
        let session = managed.runtime.session();
        let initial_temperature = session.temperature();
        let initial_targets = session.chat_targets();

        let error = service
            .configure_thread_from_chat(
                &managed.runtime,
                &proto::ChatRequest {
                    session_id: "removed-ask-mode".into(),
                    provider: "deepseek".into(),
                    model: "deepseek-v4-flash".into(),
                    interaction_mode: "ask".into(),
                    temperature: Some(0.2),
                    ..Default::default()
                },
            )
            .await
            .expect_err("removed ask mode must fail");

        assert_eq!(error.code(), tonic::Code::InvalidArgument);
        assert_eq!(session.temperature(), initial_temperature);
        assert_eq!(session.chat_targets(), initial_targets);

        managed
            .runtime
            .submit(agent_protocol::Op::Shutdown)
            .await
            .expect("shutdown");
        managed.runtime.wait_terminated().await;
    }

    #[test]
    fn turn_request_preserves_client_message_identity() {
        let mut chat = valid_chat_request();
        chat.client_message_id = "queued-message-7".into();
        let request = turn_request_from_chat(&chat).expect("valid turn input");
        assert_eq!(request.input.len(), 1);
        assert_eq!(
            request.input[0].client_message_id.as_deref(),
            Some("queued-message-7")
        );
    }

    #[test]
    fn chat_contract_rejects_tool_name_override() {
        let mut chat = valid_chat_request();
        chat.tool_names = vec!["exec_command".into()];
        assert_eq!(
            validate_chat_request(&chat)
                .expect_err("tool override must fail")
                .code(),
            tonic::Code::InvalidArgument
        );
    }

    #[test]
    fn chat_contract_rejects_disabled_memory_instead_of_ignoring_it() {
        let mut chat = valid_chat_request();
        chat.use_memory = false;
        assert_eq!(
            validate_chat_request(&chat)
                .expect_err("disabled memory must fail")
                .code(),
            tonic::Code::InvalidArgument
        );
    }

    #[test]
    fn chat_contract_validates_resume_json() {
        let mut chat = valid_chat_request();
        chat.resume_json = "not-json".into();
        assert_eq!(
            validate_chat_request(&chat)
                .expect_err("malformed resume must fail")
                .code(),
            tonic::Code::InvalidArgument
        );
        chat.resume_json = r#"[{"interrupt_id":"request-1","payload":{"approved":true}}]"#.into();
        let validated = validate_chat_request(&chat).expect("valid resume payload");
        assert_eq!(validated.resume_items.len(), 1);
        assert_eq!(validated.resume_items[0].interrupt_id, "request-1");
    }

    #[test]
    fn chat_contract_rejects_unknown_interaction_mode() {
        let mut chat = valid_chat_request();
        chat.interaction_mode = "unknown".into();
        assert_eq!(
            validate_chat_request(&chat)
                .expect_err("unknown interaction mode must fail")
                .code(),
            tonic::Code::InvalidArgument
        );
        chat.interaction_mode = "plan".into();
        assert_eq!(
            validate_chat_request(&chat)
                .expect("known mode")
                .interaction_mode,
            types::InteractionMode::Plan
        );
        chat.interaction_mode = "ask".into();
        assert_eq!(
            validate_chat_request(&chat)
                .expect_err("removed ask mode must fail")
                .code(),
            tonic::Code::InvalidArgument
        );
    }

    #[tokio::test]
    async fn invalid_submit_turn_has_no_thread_rollout_or_subscription_side_effects() {
        let dir = TempDir::new().expect("tempdir");
        memory::ensure_workspace(dir.path()).expect("workspace");
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let (_rx, _cancel, generation) = service.connections.register(" submit-id ".into()).await;
        let mut chat = valid_chat_request();
        chat.session_id = "invalid-submit-thread".into();
        chat.interaction_mode = "mystery".into();

        let error = submit_turn(
            &service,
            Request::new(proto::SubmitTurnRequest {
                connection_id: " submit-id ".into(),
                chat: Some(chat),
                mode: "start_or_steer".into(),
                expected_turn_id: String::new(),
            }),
        )
        .await
        .expect_err("invalid settings must fail before thread creation");
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
        assert!(!service.threads.contains("invalid-submit-thread").await);
        assert!(service
            .thread_states
            .get("invalid-submit-thread")
            .await
            .is_none());
        let rollout_root = dir.path().join("sessions").join("rollouts");
        assert!(
            agent_rollout::find_rollout(&rollout_root, "invalid-submit-thread")
                .expect("rollout lookup")
                .is_none()
        );
        service.connections.remove_generation(&generation).await;
    }

    #[tokio::test]
    async fn not_submitted_response_preserves_shared_subscription() {
        let dir = TempDir::new().expect("tempdir");
        memory::ensure_workspace(dir.path()).expect("workspace");
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let managed = service
            .get_or_create_thread("not-submitted-thread")
            .await
            .expect("thread");
        let (_rx, _cancel, generation) = service
            .connections
            .register("not-submitted-connection".into())
            .await;
        let subscription = generation.key().clone();
        resume(&managed, subscription.clone(), false)
            .await
            .expect("resume");
        assert!(
            service
                .thread_states
                .has_subscribers("not-submitted-thread")
                .await
        );

        let response = submit_turn_response(
            "submission-1".into(),
            TurnInputSubmission::NotSubmitted {
                reason: "terminating".into(),
            },
        )
        .into_inner();

        assert_eq!(response.disposition, "not_submitted");
        assert_eq!(response.reason, "terminating");
        assert!(
            service
                .thread_states
                .has_subscribers("not-submitted-thread")
                .await
        );
        service.connections.remove_generation(&generation).await;
        managed
            .runtime
            .submit(agent_protocol::Op::Shutdown)
            .await
            .expect("shutdown submission should succeed");
        managed.runtime.wait_terminated().await;
    }

    #[tokio::test]
    async fn submit_turn_rpc_preserves_shared_subscription_on_not_submitted() {
        let dir = TempDir::new().expect("tempdir");
        memory::ensure_workspace(dir.path()).expect("workspace");
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let (_rx, _cancel, generation) = service
            .connections
            .register("rpc-not-submitted-connection".into())
            .await;
        let response = submit_turn(
            &service,
            Request::new(proto::SubmitTurnRequest {
                connection_id: "rpc-not-submitted-connection".into(),
                chat: Some(proto::ChatRequest {
                    session_id: "rpc-not-submitted-thread".into(),
                    content: "late steer".into(),
                    use_memory: true,
                    interaction_mode: "agent".into(),
                    ..Default::default()
                }),
                mode: "steer".into(),
                expected_turn_id: "missing-turn".into(),
            }),
        )
        .await
        .expect("not submitted is a response")
        .into_inner();

        assert_eq!(response.disposition, "not_submitted");
        assert_eq!(response.reason, "no_active_turn");
        assert!(
            service
                .thread_states
                .has_subscribers("rpc-not-submitted-thread")
                .await
        );
        let managed = service
            .threads
            .get("rpc-not-submitted-thread")
            .await
            .expect("created thread");
        service.connections.remove_generation(&generation).await;
        managed
            .runtime
            .submit(agent_protocol::Op::Shutdown)
            .await
            .expect("shutdown submission should succeed");
        managed.runtime.wait_terminated().await;
    }

    #[tokio::test]
    async fn accepted_submit_response_keeps_started_and_steered_subscriptions() {
        let dir = TempDir::new().expect("tempdir");
        memory::ensure_workspace(dir.path()).expect("workspace");
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let managed = service
            .get_or_create_thread("accepted-submit-thread")
            .await
            .expect("thread");
        let (_rx, _cancel, generation) = service
            .connections
            .register("accepted-submit-connection".into())
            .await;
        let subscription = generation.key().clone();
        resume(&managed, subscription.clone(), false)
            .await
            .expect("resume");

        for (submission, expected) in [
            (
                TurnInputSubmission::Started {
                    turn_id: "turn-1".into(),
                },
                "started",
            ),
            (
                TurnInputSubmission::Steered {
                    turn_id: "turn-1".into(),
                },
                "steered",
            ),
        ] {
            assert_eq!(
                submit_turn_response("submission-1".into(), submission)
                    .into_inner()
                    .disposition,
                expected
            );
            assert!(
                service
                    .thread_states
                    .has_subscribers("accepted-submit-thread")
                    .await
            );
        }

        unsubscribe_and_wait(&managed.commands, subscription)
            .await
            .expect("cleanup subscription");
        service.connections.remove_generation(&generation).await;
        managed
            .runtime
            .submit(agent_protocol::Op::Shutdown)
            .await
            .expect("shutdown submission should succeed");
        managed.runtime.wait_terminated().await;
    }

    #[tokio::test]
    async fn semantic_resume_errors_have_no_subscription_or_config_side_effects() {
        let dir = TempDir::new().expect("tempdir");
        memory::ensure_workspace(dir.path()).expect("workspace");
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let managed = service
            .get_or_create_thread("semantic-resume")
            .await
            .expect("thread");
        service
            .configure_thread_from_chat(
                &managed.runtime,
                &proto::ChatRequest {
                    session_id: "semantic-resume".into(),
                    content: "initial".into(),
                    use_memory: true,
                    ..Default::default()
                },
            )
            .await
            .expect("configure gate");
        let gate = service
            .hitl_registry
            .get("semantic-resume")
            .await
            .expect("gate");
        service
            .thread_states
            .get("semantic-resume")
            .await
            .expect("thread state")
            .lock()
            .await
            .history
            .track(&agent_protocol::Event {
                id: "pending-turn".into(),
                msg: agent_protocol::EventMsg::TurnStarted(agent_protocol::TurnStartedEvent {
                    turn_id: "pending-turn".into(),
                }),
            });
        let _wait = gate
            .begin_wait(agent::Interrupt {
                id: "known".into(),
                reason: "confirmation".into(),
                response_schema_json: serde_json::json!({
                    "type":"object",
                    "required":["approved"],
                    "properties":{"approved":{"type":"boolean"}}
                })
                .to_string(),
                ..Default::default()
            })
            .await;
        let initial_temperature = managed.runtime.session().temperature();
        let initial_mode = managed.runtime.session().interaction_mode().await;
        let (_rx, _cancel, generation) =
            service.connections.register("semantic-client".into()).await;

        for resume_json in [
            r#"[{"interrupt_id":"unknown","status":"resolved","payload":{"approved":true}}]"#,
            r#"[{"interrupt_id":"known","status":"bogus","payload":{"approved":true}}]"#,
            r#"[{"interrupt_id":"known","status":"resolved","payload":{"approved":"yes"}}]"#,
        ] {
            let error = submit_turn(
                &service,
                Request::new(proto::SubmitTurnRequest {
                    connection_id: "semantic-client".into(),
                    chat: Some(proto::ChatRequest {
                        session_id: "semantic-resume".into(),
                        content: "must not start a new turn".into(),
                        use_memory: true,
                        interaction_mode: "plan".into(),
                        temperature: Some(1.7),
                        resume_json: resume_json.into(),
                        ..Default::default()
                    }),
                    mode: "start_or_steer".into(),
                    expected_turn_id: String::new(),
                }),
            )
            .await
            .expect_err("semantic resume must fail");
            assert_eq!(error.code(), tonic::Code::InvalidArgument);
            assert!(
                !service
                    .thread_states
                    .has_subscribers("semantic-resume")
                    .await
            );
            assert_eq!(managed.runtime.session().temperature(), initial_temperature);
            assert_eq!(
                managed.runtime.session().interaction_mode().await,
                initial_mode
            );
            assert_eq!(gate.pending_interrupts().await.len(), 1);
        }

        service.connections.remove_generation(&generation).await;
        managed
            .runtime
            .submit(Op::Shutdown)
            .await
            .expect("shutdown");
        managed.runtime.wait_terminated().await;
        managed.stop_listener().await;
    }

    #[tokio::test]
    async fn resume_for_unknown_thread_creates_no_thread_state_or_rollout() {
        let dir = TempDir::new().expect("tempdir");
        memory::ensure_workspace(dir.path()).expect("workspace");
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let (_rx, _cancel, generation) = service.connections.register("resume-client".into()).await;
        let error = submit_turn(
            &service,
            Request::new(proto::SubmitTurnRequest {
                connection_id: "resume-client".into(),
                chat: Some(proto::ChatRequest {
                    session_id: "unknown-resume-thread".into(),
                    use_memory: true,
                    resume_json: r#"[{"interrupt_id":"missing","status":"resolved"}]"#.into(),
                    ..Default::default()
                }),
                mode: "start_or_steer".into(),
                expected_turn_id: String::new(),
            }),
        )
        .await
        .expect_err("unknown resume thread");
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
        assert!(!service.threads.contains("unknown-resume-thread").await);
        assert!(service
            .thread_states
            .get("unknown-resume-thread")
            .await
            .is_none());
        assert!(agent_rollout::find_rollout(
            &dir.path().join("sessions").join("rollouts"),
            "unknown-resume-thread"
        )
        .expect("rollout lookup")
        .is_none());
        service.connections.remove_generation(&generation).await;
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
    async fn slow_connection_unsubscribes_exact_generation_from_every_thread() {
        let dir = TempDir::new().expect("tempdir");
        memory::ensure_workspace(dir.path()).expect("workspace");
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let first = service
            .get_or_create_thread("slow-first")
            .await
            .expect("first");
        let second = service
            .get_or_create_thread("slow-second")
            .await
            .expect("second");
        let mut stream = subscribe_thread_events(
            &service,
            Request::new(proto::SubscribeThreadEventsRequest {
                connection_id: " slow-both ".into(),
            }),
        )
        .await
        .expect("subscribe")
        .into_inner();
        let key = service
            .connections
            .current_generation_key("slow-both")
            .await
            .expect("canonical generation");
        resume(&first, key.clone(), false)
            .await
            .expect("first resume");
        resume(&second, key.clone(), false)
            .await
            .expect("second resume");

        for index in 0..600 {
            let _ = service
                .connections
                .send_to_generation(
                    &key,
                    proto::ThreadEvent {
                        thread_id: "slow-first".into(),
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
        loop {
            let next = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
                .await
                .expect("slow stream must terminate");
            let Some(result) = next else { break };
            if let Err(error) = result {
                assert_eq!(error.code(), tonic::Code::ResourceExhausted);
                errors += 1;
            }
        }
        assert_eq!(errors, 1);
        for _ in 0..16 {
            if !service.thread_states.has_subscribers("slow-first").await
                && !service.thread_states.has_subscribers("slow-second").await
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(!service.thread_states.has_subscribers("slow-first").await);
        assert!(!service.thread_states.has_subscribers("slow-second").await);
        for managed in [first, second] {
            managed.stop_listener().await;
            managed
                .runtime
                .submit(Op::Shutdown)
                .await
                .expect("shutdown");
            managed.runtime.wait_terminated().await;
        }
    }

    #[tokio::test]
    async fn connection_ids_are_trimmed_for_subscription_membership() {
        let dir = TempDir::new().expect("tempdir");
        memory::ensure_workspace(dir.path()).expect("workspace");
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let stream = subscribe_thread_events(
            &service,
            Request::new(proto::SubscribeThreadEventsRequest {
                connection_id: " spaced-id ".into(),
            }),
        )
        .await
        .expect("subscribe")
        .into_inner();
        resume_thread(
            &service,
            Request::new(proto::ResumeThreadRequest {
                connection_id: " spaced-id ".into(),
                thread_id: "trim-thread".into(),
                include_turns: false,
            }),
        )
        .await
        .expect("resume");
        assert_eq!(
            service
                .thread_states
                .subscribed_connection_ids("trim-thread")
                .await,
            vec!["spaced-id".to_string()]
        );
        drop(stream);
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
        let old_key = service
            .connections
            .current_generation_key("same-id-stream")
            .await
            .expect("old generation");
        resume(&managed, old_key, false).await.expect("old resume");
        let replacement_stream = subscribe_thread_events(
            &service,
            Request::new(proto::SubscribeThreadEventsRequest {
                connection_id: "same-id-stream".into(),
            }),
        )
        .await
        .expect("replacement stream")
        .into_inner();
        let replacement_key = service
            .connections
            .current_generation_key("same-id-stream")
            .await
            .expect("replacement generation");
        resume(&managed, replacement_key, false)
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

    #[tokio::test]
    async fn submit_turn_resume_json_resolves_existing_thread_hitl_gate() {
        let dir = TempDir::new().expect("tempdir");
        memory::ensure_workspace(dir.path()).expect("workspace");
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let managed = service
            .get_or_create_thread("submit-hitl-thread")
            .await
            .expect("thread");
        service
            .configure_thread_from_chat(
                &managed.runtime,
                &proto::ChatRequest {
                    session_id: "submit-hitl-thread".into(),
                    content: "continue".into(),
                    use_memory: true,
                    ..Default::default()
                },
            )
            .await
            .expect("configure controls");
        let gate = service
            .hitl_registry
            .get("submit-hitl-thread")
            .await
            .expect("gate");
        service
            .thread_states
            .get("submit-hitl-thread")
            .await
            .expect("thread state")
            .lock()
            .await
            .history
            .track(&agent_protocol::Event {
                id: "submit-pending-turn".into(),
                msg: agent_protocol::EventMsg::TurnStarted(agent_protocol::TurnStartedEvent {
                    turn_id: "submit-pending-turn".into(),
                }),
            });
        let resolution = gate
            .begin_wait(agent::Interrupt {
                id: "submit-request".into(),
                reason: "confirmation".into(),
                ..Default::default()
            })
            .await;
        let (_rx, _cancel, generation) = service.connections.register("hitl-submit".into()).await;

        let response = submit_turn(
            &service,
            Request::new(proto::SubmitTurnRequest {
                connection_id: "hitl-submit".into(),
                chat: Some(proto::ChatRequest {
                    session_id: "submit-hitl-thread".into(),
                    use_memory: true,
                    resume_json:
                        r#"[{"interrupt_id":"submit-request","payload":{"approved":true}}]"#.into(),
                    ..Default::default()
                }),
                mode: "start_or_steer".into(),
                expected_turn_id: String::new(),
            }),
        )
        .await
        .expect("submit")
        .into_inner();
        assert_eq!(response.disposition, "resumed");
        assert_eq!(response.turn_id, "submit-pending-turn");
        assert!(matches!(managed.runtime.status(), agent::AgentStatus::Idle));
        let resolved = resolution.await.expect("resolution");
        assert_eq!(resolved.status, "resolved");
        assert!(resolved.payload_json.contains("approved"));

        service.connections.remove_generation(&generation).await;
        managed
            .runtime
            .submit(Op::Shutdown)
            .await
            .expect("shutdown");
        managed.runtime.wait_terminated().await;
        managed.stop_listener().await;
    }

    #[tokio::test]
    async fn concurrent_resume_error_preserves_shared_subscription_for_owner_cleanup() {
        let dir = TempDir::new().expect("tempdir");
        memory::ensure_workspace(dir.path()).expect("workspace");
        let service = std::sync::Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
        let managed = service
            .get_or_create_thread("resume-cleanup-race")
            .await
            .expect("thread");
        service
            .configure_thread_from_chat(
                &managed.runtime,
                &proto::ChatRequest {
                    session_id: "resume-cleanup-race".into(),
                    use_memory: true,
                    ..Default::default()
                },
            )
            .await
            .expect("configure controls");
        let gate = service
            .hitl_registry
            .get("resume-cleanup-race")
            .await
            .expect("gate");
        let state = service
            .thread_states
            .get("resume-cleanup-race")
            .await
            .expect("thread state");
        state.lock().await.history.track(&agent_protocol::Event {
            id: "resume-cleanup-turn".into(),
            msg: agent_protocol::EventMsg::TurnStarted(agent_protocol::TurnStartedEvent {
                turn_id: "resume-cleanup-turn".into(),
            }),
        });
        let resolution = gate
            .begin_wait(agent::Interrupt {
                id: "resume-cleanup-request".into(),
                reason: "confirmation".into(),
                ..Default::default()
            })
            .await;
        let (_rx, _cancel, generation) = service
            .connections
            .register("resume-cleanup-client".into())
            .await;
        let barrier = install_resume_resolve_barrier("resume-cleanup-race");
        let service_for_submit = service.clone();
        let mut submit = tokio::spawn(async move {
            submit_turn(
                &service_for_submit,
                Request::new(proto::SubmitTurnRequest {
                    connection_id: "resume-cleanup-client".into(),
                    chat: Some(proto::ChatRequest {
                        session_id: "resume-cleanup-race".into(),
                        use_memory: true,
                        resume_json: r#"[{"interrupt_id":"resume-cleanup-request","payload":{"approved":true}}]"#.into(),
                        ..Default::default()
                    }),
                    mode: "start_or_steer".into(),
                    expected_turn_id: String::new(),
                }),
            )
            .await
        });

        barrier.reached.notified().await;
        let state_guard = state.lock().await;
        gate.resolve(&[agent::ResumeItem {
            interrupt_id: "resume-cleanup-request".into(),
            status: "resolved".into(),
            payload_json: r#"{"approved":true}"#.into(),
        }])
        .await
        .expect("concurrent consumer wins");
        barrier.release.notify_one();
        let error = tokio::time::timeout(std::time::Duration::from_secs(1), &mut submit)
            .await
            .expect("server must not perform activation-blind cleanup")
            .expect("submit task")
            .expect_err("the second resume consumer must be rejected");
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
        drop(state_guard);
        assert!(
            service
                .thread_states
                .has_subscribers("resume-cleanup-race")
                .await,
            "desktop activation owner must decide whether this shared subscriber is stale"
        );
        resolution.await.expect("first consumer resolution");

        service.connections.remove_generation(&generation).await;
        managed
            .runtime
            .submit(Op::Shutdown)
            .await
            .expect("shutdown");
        managed.runtime.wait_terminated().await;
        managed.stop_listener().await;
    }
}
