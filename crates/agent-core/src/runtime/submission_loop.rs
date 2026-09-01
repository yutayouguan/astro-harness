use std::sync::Arc;

use agent_protocol::{
    ErrorEvent, EventMsg, InterAgentCommunication, ItemEvent, Op, ReviewDecision, Submission,
    TurnInput, TurnInputMode, TurnInputRequest, TurnItem,
};
use async_channel::Receiver;
use futures::FutureExt;
use serde_json::Value;

use super::Session;
use crate::streaming::ResponsesOverride;
use realtime::{
    bem_presentations, BemChannelParser, BemPhase, RealtimeConnectionConfig, RealtimeHistory,
    RealtimeTransportEvent,
};

pub(crate) async fn submission_loop(
    session: Arc<Session>,
    rx_sub: Receiver<Submission>,
    responses_override: Option<ResponsesOverride>,
) {
    let mut shutdown_received = false;
    while let Ok(submission) = rx_sub.recv().await {
        let should_exit = match submission.op {
            Op::TurnInput {
                request,
                mode,
                reply,
            } => {
                let result = session
                    .submit_turn_input(submission.id, request, mode, responses_override.clone())
                    .await;
                let _ = reply.send(result);
                false
            }
            Op::RecoverTurn { turn_id, reply } => {
                let result = session
                    .recover_turn(turn_id, responses_override.clone())
                    .await;
                let _ = reply.send(result);
                false
            }
            Op::SuspendTurnAndShutdown { reply } => {
                let result = session
                    .suspend_active_regular_turn()
                    .await
                    .map_err(|error| agent_protocol::TurnInputError::Invalid(error.to_string()));
                let should_exit = matches!(
                    result,
                    Ok(agent_protocol::SuspendTurnOutcome::Suspended { .. })
                );
                if should_exit {
                    session.shutdown(submission.id).await;
                }
                let _ = reply.send(result);
                should_exit
            }
            Op::Interrupt => {
                if let Err(error) = session
                    .abort_all_tasks(agent_protocol::TurnAbortReason::Interrupted)
                    .await
                {
                    session
                        .send_event(
                            &submission.id,
                            EventMsg::Error(ErrorEvent {
                                message: error.to_string(),
                                error_type: "task_abort".into(),
                            }),
                        )
                        .await;
                }
                false
            }
            Op::CleanBackgroundTerminals => {
                let stopped_jobs =
                    tools::shutdown_background_jobs_for_session(session.session_id());
                tracing::debug!(
                    stopped_jobs,
                    session_id = %session.session_id(),
                    "background terminals cleaned on request"
                );
                false
            }
            Op::EmitExtension { item, turn_id } => {
                session.record_extension(submission.id, item, turn_id).await;
                false
            }
            Op::Shutdown => {
                session.shutdown(submission.id).await;
                true
            }
            op => {
                session
                    .dispatch_control_op(submission.id, op, responses_override.clone())
                    .await;
                false
            }
        };
        if should_exit {
            shutdown_received = true;
            break;
        }
    }
    if !shutdown_received {
        session
            .shutdown(format!("{}:shutdown", session.session_id()))
            .await;
    }
}

impl Session {
    pub(crate) async fn shutdown(self: &Arc<Self>, submission_id: String) {
        if let Err(error) = self
            .abort_all_tasks(agent_protocol::TurnAbortReason::Interrupted)
            .await
        {
            tracing::warn!(%error, session_id = %self.session_id(), "failed to abort session task before shutdown");
        }
        self.shutdown_runtime().await;
        if let Some(bindings) = self.runtime_io.get() {
            if let Err(error) = bindings.rollout.shutdown().await {
                self.send_event_raw_with_persistence(
                    agent_protocol::Event {
                        id: submission_id.clone(),
                        msg: EventMsg::Error(ErrorEvent {
                            message: error.to_string(),
                            error_type: "rollout_shutdown".into(),
                        }),
                    },
                    false,
                )
                .await;
            }
        }
        self.deliver_event_raw(agent_protocol::Event {
            id: submission_id,
            msg: EventMsg::ShutdownComplete,
        })
        .await;
    }

    async fn record_extension(
        &self,
        submission_id: String,
        item: agent_protocol::ExtensionItem,
        target_turn_id: Option<String>,
    ) {
        let turn_id = match target_turn_id {
            Some(turn_id) => turn_id,
            None => self
                .active_turn
                .lock()
                .await
                .as_ref()
                .and_then(|turn| turn.task.as_ref())
                .map(|running| running.turn_context.sub_id().to_string())
                .unwrap_or_else(|| submission_id.clone()),
        };
        self.send_event(
            &turn_id,
            EventMsg::ItemCompleted(ItemEvent {
                turn_id: turn_id.clone(),
                item: TurnItem::Extension(item),
            }),
        )
        .await;
    }

    async fn dispatch_control_op(
        self: &Arc<Self>,
        submission_id: String,
        op: Op,
        responses_override: Option<ResponsesOverride>,
    ) {
        match op {
            Op::RealtimeConversationStart {
                mut params,
                target,
                reply,
            } => {
                let model = params
                    .model
                    .clone()
                    .filter(|model| !model.trim().is_empty())
                    .unwrap_or_else(|| {
                        if target.model.trim().is_empty() {
                            agent_protocol::DEFAULT_REALTIME_MODEL.to_string()
                        } else {
                            target.model.clone()
                        }
                    });
                if params.include_startup_context {
                    let context = self.build_system_prompt().await;
                    params.instructions = Some(match params.instructions.take() {
                        Some(instructions) if !instructions.trim().is_empty() => {
                            truncate_realtime_text(&format!("{context}\n\n{instructions}"), 24_000)
                        }
                        _ => truncate_realtime_text(&context, 24_000),
                    });
                    let mut history = self
                        .clone_history()
                        .await
                        .into_iter()
                        .rev()
                        .filter_map(|item| {
                            let role = match item.role()? {
                                "user" => agent_protocol::ConversationTextRole::User,
                                "assistant" => agent_protocol::ConversationTextRole::Assistant,
                                _ => return None,
                            };
                            let text = item.text();
                            (!text.trim().is_empty()).then_some(
                                agent_protocol::ConversationTextParams {
                                    text: truncate_realtime_text(&text, 8_000),
                                    role,
                                },
                            )
                        })
                        .take(32)
                        .collect::<Vec<_>>();
                    history.reverse();
                    history.append(&mut params.initial_items);
                    params.initial_items = history;
                }
                let handoff_settings = (
                    params.client_managed_handoffs,
                    params.codex_response_handoff_mode,
                    params
                        .codex_response_handoff_channel_prefixes
                        .clone()
                        .unwrap_or_default(),
                    params.flush_transcript_tail_on_session_end,
                );
                match self
                    .realtime
                    .start(RealtimeConnectionConfig {
                        api_key: target.api_key,
                        base_url: target.base_url,
                        model: model.clone(),
                        params: params.clone(),
                    })
                    .await
                {
                    Ok(connection) => {
                        let _ = reply.send(Ok(()));
                        let version = connection.version;
                        let call_id = connection.call_id.clone();
                        self.send_event(
                            &submission_id,
                            EventMsg::RealtimeConversationStarted(
                                agent_protocol::RealtimeConversationStartedEvent {
                                    realtime_session_id: connection.provider_session_id.clone(),
                                    call_id,
                                    model,
                                    version,
                                },
                            ),
                        )
                        .await;
                        if let Some(sdp) = connection.sdp {
                            self.send_event(
                                &submission_id,
                                EventMsg::RealtimeConversationSdp(
                                    agent_protocol::RealtimeConversationSdpEvent { sdp },
                                ),
                            )
                            .await;
                        }
                        self.spawn_realtime_event_fanout(
                            submission_id,
                            connection.provider_session_id,
                            handoff_settings,
                            connection.events,
                        );
                    }
                    Err(error) => {
                        let reason = error.to_string();
                        let _ = reply.send(Err(reason.clone()));
                        self.emit_control_error(submission_id.clone(), "realtime_start", &reason)
                            .await;
                        self.send_event(
                            &submission_id,
                            EventMsg::RealtimeConversationClosed(
                                agent_protocol::RealtimeConversationClosedEvent {
                                    reason: Some(reason),
                                },
                            ),
                        )
                        .await;
                    }
                }
            }
            Op::RealtimeConversationAudio(params) => {
                if let Err(error) = self.realtime.send_audio(params.frame).await {
                    self.emit_control_error(submission_id, "realtime_audio", error)
                        .await;
                }
            }
            Op::RealtimeConversationText(params) => {
                if let Err(error) = self.realtime.send_text(params).await {
                    self.emit_control_error(submission_id, "realtime_text", error)
                        .await;
                }
            }
            Op::RealtimeConversationSpeech(params) => {
                if let Err(error) = self.realtime.send_speech(params.text).await {
                    self.emit_control_error(submission_id, "realtime_speech", error)
                        .await;
                }
            }
            Op::RealtimeConversationClose => {
                self.realtime.close().await;
            }
            Op::RealtimeConversationListVoices => {
                let version = self
                    .realtime
                    .version_and_handoff_mode()
                    .await
                    .map(|(version, _)| version)
                    .unwrap_or(agent_protocol::RealtimeConversationVersion::V2);
                self.send_event(
                    &submission_id,
                    EventMsg::RealtimeConversationListVoicesResponse(
                        agent_protocol::RealtimeConversationListVoicesResponseEvent {
                            voices: agent_protocol::RealtimeVoicesList::builtin(version),
                        },
                    ),
                )
                .await;
            }
            Op::ThreadSettings { thread_settings } => {
                match self.apply_thread_settings(thread_settings) {
                    Ok(thread_settings) => {
                        self.send_event(
                            &submission_id,
                            EventMsg::ThreadSettingsApplied(
                                agent_protocol::ThreadSettingsAppliedEvent { thread_settings },
                            ),
                        )
                        .await;
                    }
                    Err(error) => {
                        self.emit_control_error(submission_id, "thread_settings", error)
                            .await;
                    }
                }
            }
            Op::RefreshMcpServers => {
                if let Err(error) = self.reload_mcp().await {
                    self.send_event(
                        &submission_id,
                        EventMsg::Error(ErrorEvent {
                            message: error.to_string(),
                            error_type: "mcp_refresh".into(),
                        }),
                    )
                    .await;
                }
            }
            Op::ReloadUserConfig => {
                if let Err(error) = self.reload_tools_and_mcp().await {
                    self.send_event(
                        &submission_id,
                        EventMsg::Error(ErrorEvent {
                            message: error.to_string(),
                            error_type: "user_config_reload".into(),
                        }),
                    )
                    .await;
                }
            }
            Op::ExecApproval { id, decision } => {
                self.resolve_approval(submission_id, id, decision).await;
            }
            Op::PatchApproval { id, decision } => {
                self.resolve_approval(submission_id, id, decision).await;
            }
            Op::UserInputAnswer { id, response } => {
                self.resolve_control_response(
                    submission_id,
                    id,
                    "resolved",
                    serde_json::to_value(response).expect("user-input response is serializable"),
                )
                .await;
            }
            Op::RequestPermissionsResponse { id, response } => {
                self.resolve_control_response(
                    submission_id,
                    id,
                    "resolved",
                    serde_json::to_value(response).expect("permissions response is serializable"),
                )
                .await;
            }
            Op::DynamicToolResponse { id, response } => {
                self.resolve_control_response(
                    submission_id,
                    id,
                    "resolved",
                    serde_json::to_value(response).expect("dynamic-tool response is serializable"),
                )
                .await;
            }
            Op::ResolveElicitation {
                server_name,
                request_id,
                response,
                reply,
            } => {
                let broker = Arc::clone(&self.mcp_elicitation);
                let action = match response.action {
                    agent_protocol::ElicitationAction::Accept => mcp::McpElicitationAction::Accept,
                    agent_protocol::ElicitationAction::Decline => {
                        mcp::McpElicitationAction::Decline
                    }
                    agent_protocol::ElicitationAction::Cancel => mcp::McpElicitationAction::Cancel,
                };
                let resolved = broker
                    .resolve(
                        &server_name,
                        &request_id,
                        mcp::McpElicitationResponse {
                            action,
                            content: response.content,
                            meta: response.meta,
                        },
                    )
                    .await;
                let _ = reply.send(resolved);
                if !resolved {
                    self.emit_control_error(
                        submission_id,
                        "resolve_elicitation",
                        anyhow::anyhow!("MCP elicitation is not pending or already resolved"),
                    )
                    .await;
                }
            }
            Op::TurnSettings {
                turn_id,
                update,
                reply,
            } => {
                let outcome = {
                    let active = self.active_turn.lock().await;
                    match active.as_ref().and_then(|active| active.task.as_ref()) {
                        Some(running) if running.turn_context.sub_id() == turn_id => {
                            running.turn_context.apply_settings_update(update)
                        }
                        _ => agent_protocol::TurnSettingsOutcome::Rejected {
                            message: "target turn is not active".into(),
                        },
                    }
                };
                let _ = reply.send(outcome);
            }
            Op::RunUserShellCommand {
                command,
                cwd,
                reply,
            } => {
                let _ = reply.send(self.launch_user_shell(command, cwd).await);
            }
            Op::ApproveGuardianDeniedAction {
                assessment_id,
                reply,
            } => {
                let approved = self.guardian_retry.approve_denied(&assessment_id);
                let _ = reply.send(approved);
                if !approved {
                    self.emit_control_error(
                        submission_id,
                        "approve_guardian_denied_action",
                        anyhow::anyhow!("Guardian assessment is not denied or already consumed"),
                    )
                    .await;
                }
            }
            Op::Compact => {
                let context = self.create_turn_context(submission_id.clone()).await;
                if let Err(error) = self
                    .spawn_task(context, Vec::new(), crate::tasks::CompactTask)
                    .await
                {
                    self.send_event(
                        &submission_id,
                        EventMsg::Error(ErrorEvent {
                            message: error.to_string(),
                            error_type: "compact_failed".into(),
                        }),
                    )
                    .await;
                }
            }
            Op::ThreadRollback { num_turns } => {
                self.rollback_thread(submission_id, num_turns).await;
            }
            Op::Review { review_request } => {
                let context = self.create_turn_context(submission_id.clone()).await;
                let args = crate::streaming::multi_turn::RunTurnArgs::submitted(
                    Arc::clone(self),
                    Arc::clone(&context),
                    responses_override,
                );
                if let Err(error) = self
                    .spawn_task(
                        context,
                        Vec::new(),
                        crate::tasks::ReviewTask::new(args, review_request),
                    )
                    .await
                {
                    self.send_event(
                        &submission_id,
                        EventMsg::Error(ErrorEvent {
                            message: error.to_string(),
                            error_type: "review_failed".into(),
                        }),
                    )
                    .await;
                }
            }
            Op::InterAgentCommunication { communication } => {
                self.handle_inter_agent_communication(
                    submission_id,
                    communication,
                    responses_override,
                )
                .await;
            }
            Op::TurnInput { .. }
            | Op::RecoverTurn { .. }
            | Op::SuspendTurnAndShutdown { .. }
            | Op::Interrupt
            | Op::CleanBackgroundTerminals
            | Op::EmitExtension { .. }
            | Op::Shutdown => {
                unreachable!("submission loop routes primary control operations directly")
            }
            op => {
                self.emit_control_error(
                    submission_id,
                    "unsupported_operation",
                    format!("operation {op:?} is not supported by this runtime"),
                )
                .await;
            }
        }
    }

    async fn resolve_approval(&self, submission_id: String, id: String, decision: ReviewDecision) {
        let (status, payload) = approval_resolution(decision);
        self.resolve_control_response(submission_id, id, status, payload)
            .await;
    }

    async fn handle_inter_agent_communication(
        self: &Arc<Self>,
        submission_id: String,
        mut communication: InterAgentCommunication,
        responses_override: Option<ResponsesOverride>,
    ) {
        let Some(bindings) = self.runtime_io.get() else {
            self.emit_control_error(
                submission_id,
                "inter_agent_communication",
                "inter-agent communication requires bound rollout persistence",
            )
            .await;
            return;
        };
        if communication.id.is_none() {
            communication.id = Some(agent_protocol::ResponseItemId::new("agent_message"));
        }
        let payload = serde_json::to_value(&communication)
            .expect("inter-agent communication is serializable");
        if let Err(error) = bindings
            .rollout
            .record(vec![agent_rollout::RolloutItem::InterAgentCommunication(
                payload,
            )])
            .await
        {
            self.emit_control_error(submission_id, "inter_agent_communication", error)
                .await;
            return;
        }

        let visible_text = communication.model_input_text();
        if communication.trigger_turn {
            if let Some(visible_text) = visible_text.as_ref() {
                let result = self
                    .submit_turn_input(
                        submission_id.clone(),
                        TurnInputRequest {
                            input: vec![TurnInput {
                                content: visible_text.clone(),
                                image_data_urls: Vec::new(),
                                client_message_id: communication
                                    .id
                                    .as_ref()
                                    .map(ToString::to_string),
                            }],
                            thread_settings: Default::default(),
                        },
                        TurnInputMode::StartOrSteer,
                        responses_override,
                    )
                    .await;
                if matches!(
                    result,
                    Ok(agent_protocol::TurnInputSubmission::Started { .. }
                        | agent_protocol::TurnInputSubmission::Steered { .. })
                ) {
                    return;
                }
                if let Err(error) = result {
                    self.emit_control_error(
                        submission_id.clone(),
                        "inter_agent_communication",
                        error,
                    )
                    .await;
                }
            }
        }

        if visible_text.is_none() && communication.encrypted_content.is_none() {
            return;
        }
        let item = communication.to_model_input_item();
        if let Err(error) = self.record_response_items(vec![item]).await {
            self.emit_control_error(submission_id, "inter_agent_communication", error)
                .await;
        }
    }

    async fn resolve_control_response(
        &self,
        submission_id: String,
        request_id: String,
        status: &str,
        payload: Value,
    ) {
        let (_, gate, _) = self.ensure_thread_controls();
        let result = gate
            .resolve(&[crate::ResumeItem {
                interrupt_id: request_id,
                status: status.to_string(),
                payload_json: payload.to_string(),
            }])
            .await;
        if let Err(error) = result {
            self.send_event(
                &submission_id,
                EventMsg::Error(ErrorEvent {
                    message: error,
                    error_type: "control_response".into(),
                }),
            )
            .await;
        }
    }

    async fn emit_control_error(
        &self,
        submission_id: String,
        operation: &str,
        error: impl std::fmt::Display,
    ) {
        self.send_event(
            &submission_id,
            EventMsg::Error(ErrorEvent {
                message: error.to_string(),
                error_type: format!("{operation}_failed"),
            }),
        )
        .await;
    }

    fn spawn_realtime_event_fanout(
        self: &Arc<Self>,
        route_id: String,
        realtime_session_id: Option<String>,
        handoff_settings: (
            bool,
            agent_protocol::CodexResponseHandoffMode,
            std::collections::BTreeMap<String, Vec<String>>,
            bool,
        ),
        mut events: tokio::sync::mpsc::Receiver<RealtimeTransportEvent>,
    ) {
        let session = Arc::clone(self);
        tokio::spawn(async move {
            let history = Arc::new(tokio::sync::Mutex::new(RealtimeHistory::default()));
            let history_session_id = realtime_session_id.unwrap_or_else(|| route_id.clone());
            let started = history.lock().await.start(history_session_id);
            session.record_realtime_items(started).await;
            while let Some(event) = events.recv().await {
                match event {
                    RealtimeTransportEvent::Event(payload) => {
                        let items = history.lock().await.observe(&payload);
                        session.record_realtime_items(items).await;
                        if let agent_protocol::RealtimeEvent::HandoffRequested(request) = &payload {
                            if !handoff_settings.0 {
                                session.spawn_realtime_handoff(
                                    request.clone(),
                                    handoff_settings.1,
                                    handoff_settings.2.clone(),
                                    Arc::clone(&history),
                                );
                            }
                        }
                        session
                            .send_event(
                                &route_id,
                                EventMsg::RealtimeConversationRealtime(
                                    agent_protocol::RealtimeConversationRealtimeEvent { payload },
                                ),
                            )
                            .await;
                    }
                    RealtimeTransportEvent::Closed(reason) => {
                        let items = if handoff_settings.3 {
                            history.lock().await.close()
                        } else {
                            history.lock().await.close_discarding_tail()
                        };
                        session.record_realtime_items(items).await;
                        session
                            .send_event(
                                &route_id,
                                EventMsg::RealtimeConversationClosed(
                                    agent_protocol::RealtimeConversationClosedEvent { reason },
                                ),
                            )
                            .await;
                        break;
                    }
                }
            }
        });
    }

    fn spawn_realtime_handoff(
        self: &Arc<Self>,
        request: agent_protocol::RealtimeHandoffRequested,
        mode: agent_protocol::CodexResponseHandoffMode,
        prefixes: std::collections::BTreeMap<String, Vec<String>>,
        history: Arc<tokio::sync::Mutex<RealtimeHistory>>,
    ) {
        let session = Arc::clone(self);
        tokio::spawn(async move {
            let input = if request.input_transcript.trim().is_empty() {
                request
                    .active_transcript
                    .iter()
                    .map(|entry| format!("{}: {}", entry.role, entry.text))
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                request.input_transcript.clone()
            };
            if input.trim().is_empty() {
                let _ = session.realtime.complete_handoff(request.handoff_id).await;
                return;
            }
            let prospective_turn_id = session
                .active_turn_id()
                .await
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let mut receiver = session.subscribe_turn_events(&prospective_turn_id).await;
            let result = session
                .submit_turn_input(
                    prospective_turn_id.clone(),
                    agent_protocol::TurnInputRequest {
                        input: vec![agent_protocol::TurnInput {
                            content: format!(
                                "<realtime_delegation>\n{}\n</realtime_delegation>",
                                input
                            ),
                            image_data_urls: Vec::new(),
                            client_message_id: None,
                        }],
                        thread_settings: Default::default(),
                    },
                    agent_protocol::TurnInputMode::StartOrSteer,
                    None,
                )
                .await;
            let Some(turn_id) = result
                .ok()
                .and_then(|result| result.turn_id().map(str::to_string))
            else {
                let _ = session.realtime.complete_handoff(request.handoff_id).await;
                return;
            };
            if turn_id != prospective_turn_id {
                receiver = session.subscribe_turn_events(&turn_id).await;
            }
            let mut bem = BemChannelParser::new(Arc::new(prefixes));
            let mut promoted_item = None;
            let mut pending = String::new();
            let mut flush = tokio::time::interval(std::time::Duration::from_millis(200));
            flush.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            flush.tick().await;
            loop {
                tokio::select! {
                    event = receiver.recv() => {
                        let Ok(event) = event else { break };
                        match event.msg {
                            EventMsg::AgentMessageContentDelta(delta) => match mode {
                                agent_protocol::CodexResponseHandoffMode::BemTags => {
                                    if let Some(chunk) = bem.push(&delta.delta) {
                                        pending.push_str(&chunk);
                                    }
                                }
                                agent_protocol::CodexResponseHandoffMode::Thinking
                                | agent_protocol::CodexResponseHandoffMode::Commentary => {
                                    pending.push_str(&delta.delta);
                                }
                            },
                            EventMsg::ItemCompleted(item) => {
                                if let TurnItem::AgentMessage(message) = item.item {
                                    promoted_item = Some((
                                        message.id,
                                        bem_presentations(&message.content),
                                    ));
                                }
                            }
                            EventMsg::TurnComplete(_) | EventMsg::TurnAborted(_) => break,
                            _ => {}
                        }
                    }
                    _ = flush.tick() => {
                        if !pending.is_empty() {
                            let chunk = std::mem::take(&mut pending);
                            let final_answer = mode == agent_protocol::CodexResponseHandoffMode::BemTags
                                && bem.phase() == Some(BemPhase::Final);
                            let _ = session.realtime.send_handoff_delta(
                                request.handoff_id.clone(), chunk, final_answer,
                            ).await;
                        }
                    }
                }
            }
            if mode == agent_protocol::CodexResponseHandoffMode::BemTags {
                pending.push_str(&bem.finish());
            }
            if !pending.is_empty() {
                let final_answer = mode == agent_protocol::CodexResponseHandoffMode::BemTags
                    && bem.phase().is_none_or(|phase| phase == BemPhase::Final);
                let _ = session
                    .realtime
                    .send_handoff_delta(request.handoff_id.clone(), pending, final_answer)
                    .await;
            }
            if let Some((item_id, presentations)) = promoted_item {
                for presentation in presentations {
                    let items = history.lock().await.promote(
                        turn_id.clone(),
                        item_id.clone(),
                        presentation,
                    );
                    session.record_realtime_items(items).await;
                }
            }
            let _ = session.realtime.complete_handoff(request.handoff_id).await;
        });
    }

    pub async fn shutdown_runtime(self: &Arc<Self>) {
        if self.begin_runtime_shutdown() {
            let session = Arc::clone(self);
            tokio::spawn(async move {
                let result = std::panic::AssertUnwindSafe(session.run_shutdown_worker())
                    .catch_unwind()
                    .await;
                if result.is_err() {
                    tracing::error!(session_id = %session.session_id(), "session shutdown worker panicked");
                }
                session.complete_runtime_shutdown();
            });
        }
        self.wait_runtime_shutdown_complete().await;
    }

    async fn run_shutdown_worker(self: &Arc<Self>) {
        self.realtime.close().await;
        let (task_lifecycle, abort_result) = self
            .abort_all_tasks_for_shutdown(agent_protocol::TurnAbortReason::Interrupted)
            .await;
        if let Err(error) = abort_result {
            tracing::warn!(%error, session_id = %self.session_id(), "failed to abort session task during shutdown");
        }
        let turn_id = task_lifecycle.as_ref().map(|(turn_id, _)| turn_id.clone());
        if let Some((_, task_completion)) = task_lifecycle {
            task_completion.cancelled().await;
        }
        let _ = self.run_session_end_hook(turn_id);
        self.hook_runtime().shutdown().await;
        if let Err(error) = self
            .mcp_hub
            .lock()
            .await
            .reload_with_configs(Vec::new())
            .await
        {
            tracing::warn!(%error, session_id = %self.session_id(), "failed to shut down MCP connections");
        }
        let stopped_jobs = tools::shutdown_background_jobs_for_session(self.session_id());
        tracing::debug!(stopped_jobs, session_id = %self.session_id(), "session runtime shutdown complete");
    }
}

fn approval_resolution(decision: ReviewDecision) -> (&'static str, Value) {
    match decision {
        ReviewDecision::Approved => ("resolved", serde_json::json!({ "approved": true })),
        ReviewDecision::ApprovedForSession => (
            "resolved",
            serde_json::json!({ "approved": true, "always": true }),
        ),
        ReviewDecision::ApprovedExecpolicyAmendment {
            proposed_execpolicy_amendment,
        } => (
            "resolved",
            serde_json::json!({
                "approved": true,
                "proposed_execpolicy_amendment": proposed_execpolicy_amendment,
            }),
        ),
        ReviewDecision::ApprovedMcpPolicyAmendment => (
            "resolved",
            serde_json::json!({ "approved": true, "mcp_policy_amendment": true }),
        ),
        ReviewDecision::NetworkPolicyAmendment {
            network_policy_amendment,
        } => (
            "resolved",
            serde_json::json!({
                "approved": true,
                "network_policy_amendment": network_policy_amendment,
            }),
        ),
        ReviewDecision::Denied { rejection } => (
            "resolved",
            serde_json::json!({ "approved": false, "rejection": rejection }),
        ),
        ReviewDecision::TimedOut => (
            "timeout",
            serde_json::json!({ "approved": false, "timed_out": true }),
        ),
        ReviewDecision::Abort => (
            "cancelled",
            serde_json::json!({ "approved": false, "abort": true }),
        ),
    }
}

fn truncate_realtime_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use agent_protocol::{
        Op, RequestUserInputAnswer, RequestUserInputResponse, ReviewDecision, Submission, TurnInput,
    };
    use serde_json::json;
    use tokio::sync::Notify;
    use tokio_util::sync::CancellationToken;

    use crate::runtime::{Config, TurnContext};
    use crate::tasks::{SessionTask, SessionTaskResult, TaskKind};

    struct PendingTask {
        started: Arc<Notify>,
    }

    impl SessionTask for PendingTask {
        fn kind(&self) -> TaskKind {
            TaskKind::Regular
        }

        fn span_name(&self) -> &'static str {
            "session_task.submission_loop_test"
        }

        async fn run(
            self: Arc<Self>,
            _session: Arc<Session>,
            _ctx: Arc<TurnContext>,
            _input: Vec<TurnInput>,
            cancellation_token: CancellationToken,
        ) -> SessionTaskResult {
            self.started.notify_one();
            cancellation_token.cancelled().await;
            Ok(None)
        }
    }

    #[tokio::test]
    async fn interrupt_is_dispatched_while_a_turn_task_is_running() {
        let dir = tempfile::tempdir().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "submission-loop-test".into(),
            )
            .await
            .unwrap(),
        );
        let started = Arc::new(Notify::new());
        let context = session.create_turn_context("turn-1".into()).await;
        session
            .spawn_task(
                context,
                Vec::new(),
                PendingTask {
                    started: Arc::clone(&started),
                },
            )
            .await
            .unwrap();
        started.notified().await;

        let (tx, rx) = async_channel::bounded(4);
        let loop_task = tokio::spawn(submission_loop(Arc::clone(&session), rx, None));
        tx.send(Submission {
            id: "interrupt-1".into(),
            op: Op::Interrupt,
        })
        .await
        .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if session.active_turn.lock().await.is_none() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        drop(tx);
        loop_task.await.unwrap();
    }

    #[tokio::test]
    async fn ordered_control_ops_resolve_the_session_hitl_gate() {
        let dir = tempfile::tempdir().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "submission-control-test".into(),
            )
            .await
            .unwrap(),
        );
        let (_, gate, _) = session.ensure_thread_controls();
        let approval = gate
            .begin_wait(crate::Interrupt {
                id: "approval-1".into(),
                response_schema_json: json!({
                    "type": "object",
                    "required": ["approved"],
                    "properties": { "approved": { "type": "boolean" } }
                })
                .to_string(),
                ..Default::default()
            })
            .await;
        let answer = gate
            .begin_wait(crate::Interrupt {
                id: "question-1".into(),
                ..Default::default()
            })
            .await;

        let (tx, rx) = async_channel::bounded(4);
        let loop_task = tokio::spawn(submission_loop(Arc::clone(&session), rx, None));
        tx.send(Submission {
            id: "approval-submission".into(),
            op: Op::ExecApproval {
                id: "approval-1".into(),
                decision: ReviewDecision::ApprovedForSession,
            },
        })
        .await
        .unwrap();
        let approval = approval.await.unwrap();
        assert_eq!(approval.status, "resolved");
        let payload: Value = serde_json::from_str(&approval.payload_json).unwrap();
        assert_eq!(payload["approved"], true);
        assert_eq!(payload["always"], true);

        tx.send(Submission {
            id: "answer-submission".into(),
            op: Op::UserInputAnswer {
                id: "question-1".into(),
                response: RequestUserInputResponse {
                    answers: std::collections::HashMap::from([(
                        "choice".into(),
                        RequestUserInputAnswer {
                            answers: vec!["yes".into()],
                        },
                    )]),
                },
            },
        })
        .await
        .unwrap();
        let answer = answer.await.unwrap();
        assert_eq!(answer.status, "resolved");
        assert_eq!(
            serde_json::from_str::<Value>(&answer.payload_json).unwrap(),
            json!({ "answers": { "choice": { "answers": ["yes"] } } })
        );

        drop(tx);
        loop_task.await.unwrap();
    }

    #[test]
    fn approval_resolution_accepts_codex_decision_names() {
        let (status, approved) = approval_resolution(ReviewDecision::Approved);
        assert_eq!(status, "resolved");
        assert_eq!(approved, json!({ "approved": true }));

        let (status, denied) = approval_resolution(ReviewDecision::Denied {
            rejection: "unsafe".into(),
        });
        assert_eq!(status, "resolved");
        assert_eq!(denied["approved"], false);

        let (status, timeout) = approval_resolution(ReviewDecision::TimedOut);
        assert_eq!(status, "timeout");
        assert_eq!(timeout["approved"], false);
    }
}
