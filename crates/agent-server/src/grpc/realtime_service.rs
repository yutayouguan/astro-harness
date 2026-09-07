//! Application boundary for realtime conversation commands.

use proto::{
    RealtimeConversationAudioRequest, RealtimeConversationRequest,
    RealtimeConversationSpeechRequest, RealtimeConversationStartRequest,
    RealtimeConversationTextRequest, RealtimeOperationResponse, RealtimeVoicesResponse,
};
use tonic::{Response, Status};

use super::AstroServiceImpl;

#[allow(clippy::result_large_err)]
fn require_session_id(session_id: &str) -> Result<&str, Status> {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        Err(Status::invalid_argument("session_id is required"))
    } else {
        Ok(session_id)
    }
}

fn nonempty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

async fn submit(
    service: &AstroServiceImpl,
    session_id: &str,
    op: agent_protocol::Op,
) -> Result<Response<RealtimeOperationResponse>, Status> {
    let managed = service
        .threads
        .get(session_id)
        .await
        .ok_or_else(|| Status::failed_precondition("realtime conversation is not loaded"))?;
    let submission_id = managed
        .runtime
        .submit(op)
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    Ok(Response::new(RealtimeOperationResponse { submission_id }))
}

pub(crate) async fn start(
    service: &AstroServiceImpl,
    req: RealtimeConversationStartRequest,
) -> Result<Response<RealtimeOperationResponse>, Status> {
    let session_id = require_session_id(&req.session_id)?;
    if req.api_key.trim().is_empty() {
        return Err(Status::invalid_argument("realtime api_key is required"));
    }
    let model = if req.model.trim().is_empty() {
        agent_protocol::DEFAULT_REALTIME_MODEL.to_string()
    } else {
        req.model.trim().to_string()
    };
    let output_modality = match req.output_modality.as_str() {
        "" | "audio" => agent_protocol::RealtimeOutputModality::Audio,
        "text" => agent_protocol::RealtimeOutputModality::Text,
        value => {
            return Err(Status::invalid_argument(format!(
                "unsupported realtime output_modality {value}"
            )))
        }
    };
    let turn_detection = match req.turn_detection.as_str() {
        "" | "server_vad" => agent_protocol::RealtimeTurnDetection::ServerVad,
        "semantic_vad" => agent_protocol::RealtimeTurnDetection::SemanticVad,
        "disabled" => agent_protocol::RealtimeTurnDetection::Disabled,
        value => {
            return Err(Status::invalid_argument(format!(
                "unsupported realtime turn_detection {value}"
            )))
        }
    };
    let noise_reduction = match req.noise_reduction.as_str() {
        "" => None,
        "near_field" => Some(agent_protocol::RealtimeNoiseReduction::NearField),
        "far_field" => Some(agent_protocol::RealtimeNoiseReduction::FarField),
        value => {
            return Err(Status::invalid_argument(format!(
                "unsupported realtime noise_reduction {value}"
            )))
        }
    };
    let version = match req.version.as_str() {
        "" | "v2" => agent_protocol::RealtimeConversationVersion::V2,
        "v3" => agent_protocol::RealtimeConversationVersion::V3,
        value => {
            return Err(Status::invalid_argument(format!(
                "unsupported realtime version {value}"
            )))
        }
    };
    let transport = match req.transport.as_str() {
        "" | "websocket" => agent_protocol::ConversationStartTransport::Websocket,
        "webrtc" if !req.sdp.trim().is_empty() => {
            agent_protocol::ConversationStartTransport::Webrtc {
                sdp: req.sdp.clone(),
            }
        }
        "webrtc" => return Err(Status::invalid_argument("WebRTC requires an SDP offer")),
        "existing_call" if !req.call_id.trim().is_empty() => {
            agent_protocol::ConversationStartTransport::ExistingCall {
                call_id: req.call_id.clone(),
            }
        }
        "existing_call" => {
            return Err(Status::invalid_argument("existing_call requires a call id"))
        }
        value => {
            return Err(Status::invalid_argument(format!(
                "unsupported realtime transport {value}"
            )))
        }
    };
    let handoff_mode = match req.handoff_mode.as_str() {
        "" | "thinking" => agent_protocol::CodexResponseHandoffMode::Thinking,
        "commentary" => agent_protocol::CodexResponseHandoffMode::Commentary,
        "bem_tags" => agent_protocol::CodexResponseHandoffMode::BemTags,
        value => {
            return Err(Status::invalid_argument(format!(
                "unsupported realtime handoff mode {value}"
            )))
        }
    };
    let handoff_channel_prefixes = if req.handoff_channel_prefixes_json.trim().is_empty() {
        None
    } else {
        Some(
            serde_json::from_str(&req.handoff_channel_prefixes_json).map_err(|error| {
                Status::invalid_argument(format!(
                    "invalid realtime handoff channel prefixes: {error}"
                ))
            })?,
        )
    };
    let include_startup_context = req.include_startup_context.unwrap_or(!matches!(
        &transport,
        agent_protocol::ConversationStartTransport::ExistingCall { .. }
    ));
    let managed = service.get_or_create_thread(session_id).await?;
    let subscription = service
        .connections
        .current_generation_key(req.connection_id.trim())
        .await
        .ok_or_else(|| Status::failed_precondition("connection is not subscribed"))?;
    super::thread_service::resume(&managed, subscription, false).await?;
    let target = types::ModelTarget {
        provider_id: req.provider.clone(),
        backend_id: req.provider,
        model: model.clone(),
        api_key: req.api_key,
        base_url: req.base_url,
    };
    let (reply, result) = tokio::sync::oneshot::channel();
    let submission_id = managed
        .runtime
        .submit(agent_protocol::Op::RealtimeConversationStart {
            params: agent_protocol::ConversationStartParams {
                model: Some(model),
                output_modality,
                voice: nonempty(req.voice),
                instructions: nonempty(req.instructions),
                include_startup_context,
                initial_items: Vec::new(),
                turn_detection,
                noise_reduction,
                transport,
                version,
                client_managed_handoffs: req.client_managed_handoffs,
                codex_response_handoff_mode: handoff_mode,
                codex_response_handoff_channel_prefixes: handoff_channel_prefixes,
                flush_transcript_tail_on_session_end: req.flush_transcript_tail_on_session_end,
            },
            target,
            reply,
        })
        .await
        .map_err(|error| Status::internal(error.to_string()))?;
    result
        .await
        .map_err(|_| Status::internal("realtime start reply channel closed"))?
        .map_err(Status::failed_precondition)?;
    Ok(Response::new(RealtimeOperationResponse { submission_id }))
}

pub(crate) async fn audio(
    service: &AstroServiceImpl,
    req: RealtimeConversationAudioRequest,
) -> Result<Response<RealtimeOperationResponse>, Status> {
    let format = match req.format.as_str() {
        "" | "pcm16" => agent_protocol::RealtimeAudioFormat::Pcm16,
        value => {
            return Err(Status::invalid_argument(format!(
                "unsupported realtime audio format {value}"
            )))
        }
    };
    let frame = agent_protocol::RealtimeAudioFrame {
        data: req.data,
        sample_rate: if req.sample_rate == 0 {
            agent_protocol::DEFAULT_REALTIME_SAMPLE_RATE
        } else {
            req.sample_rate
        },
        num_channels: if req.num_channels == 0 {
            1
        } else {
            u16::try_from(req.num_channels)
                .map_err(|_| Status::invalid_argument("num_channels exceeds u16"))?
        },
        format,
    };
    submit(
        service,
        require_session_id(&req.session_id)?,
        agent_protocol::Op::RealtimeConversationAudio(agent_protocol::ConversationAudioParams {
            frame,
        }),
    )
    .await
}

pub(crate) async fn text(
    service: &AstroServiceImpl,
    req: RealtimeConversationTextRequest,
) -> Result<Response<RealtimeOperationResponse>, Status> {
    if req.text.trim().is_empty() {
        return Err(Status::invalid_argument("realtime text is required"));
    }
    let role = match req.role.as_str() {
        "" | "user" => agent_protocol::ConversationTextRole::User,
        "developer" => agent_protocol::ConversationTextRole::Developer,
        "assistant" => agent_protocol::ConversationTextRole::Assistant,
        value => {
            return Err(Status::invalid_argument(format!(
                "unsupported realtime text role {value}"
            )))
        }
    };
    submit(
        service,
        require_session_id(&req.session_id)?,
        agent_protocol::Op::RealtimeConversationText(agent_protocol::ConversationTextParams {
            text: req.text,
            role,
        }),
    )
    .await
}

pub(crate) async fn speech(
    service: &AstroServiceImpl,
    req: RealtimeConversationSpeechRequest,
) -> Result<Response<RealtimeOperationResponse>, Status> {
    if req.text.trim().is_empty() {
        return Err(Status::invalid_argument("realtime speech text is required"));
    }
    submit(
        service,
        require_session_id(&req.session_id)?,
        agent_protocol::Op::RealtimeConversationSpeech(agent_protocol::ConversationSpeechParams {
            text: req.text,
        }),
    )
    .await
}

pub(crate) async fn close(
    service: &AstroServiceImpl,
    req: RealtimeConversationRequest,
) -> Result<Response<RealtimeOperationResponse>, Status> {
    submit(
        service,
        require_session_id(&req.session_id)?,
        agent_protocol::Op::RealtimeConversationClose,
    )
    .await
}

pub(crate) async fn list_voices(
    service: &AstroServiceImpl,
    req: RealtimeConversationRequest,
) -> Result<Response<RealtimeVoicesResponse>, Status> {
    let session_id = require_session_id(&req.session_id)?;
    let voices = agent_protocol::RealtimeVoicesList::builtin(
        agent_protocol::RealtimeConversationVersion::V2,
    );
    if let Some(managed) = service.threads.get(session_id).await {
        managed
            .runtime
            .submit(agent_protocol::Op::RealtimeConversationListVoices)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
    }
    Ok(Response::new(RealtimeVoicesResponse {
        voices: voices.voices,
        default_voice: voices.default_voice,
    }))
}
