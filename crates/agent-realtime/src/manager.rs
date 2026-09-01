use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use agent_protocol::{
    CodexResponseHandoffMode, ConversationStartParams, ConversationStartTransport,
    ConversationTextParams, RealtimeAudioFormat, RealtimeAudioFrame, RealtimeConversationVersion,
    RealtimeEvent,
};
use anyhow::{anyhow, bail, Context, Result};
use futures::{SinkExt, StreamExt};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, LOCATION, USER_AGENT};
use reqwest::multipart::{Form, Part};
use serde_json::Value;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::{
    audio_append, function_call_output, handoff_append, handoff_complete, parse_realtime_event,
    session_config, session_update, speech_event, text_events,
};

type RealtimeSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

const INPUT_QUEUE_CAPACITY: usize = 256;
const OUTPUT_QUEUE_CAPACITY: usize = 256;
const MAX_AUDIO_FRAME_BYTES: usize = 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);
const RECONNECT_BASE_DELAY: Duration = Duration::from_millis(200);
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(5);
const MAX_SIDEBAND_RETRIES: u32 = 5;

#[derive(Debug)]
pub enum RealtimeTransportEvent {
    Event(RealtimeEvent),
    Closed(Option<String>),
}

#[derive(Debug)]
enum RealtimeCommand {
    Audio(RealtimeAudioFrame),
    Text(ConversationTextParams),
    Speech(String),
    HandoffDelta {
        handoff_id: String,
        text: String,
        final_answer: bool,
    },
    HandoffComplete {
        handoff_id: String,
    },
    Close,
}

struct ConversationState {
    command_tx: mpsc::Sender<RealtimeCommand>,
    cancel: CancellationToken,
    active: Arc<AtomicBool>,
    task: JoinHandle<()>,
    version: RealtimeConversationVersion,
    handoff_mode: CodexResponseHandoffMode,
}

#[derive(Default)]
pub struct RealtimeConversationManager {
    state: Mutex<Option<ConversationState>>,
}

pub struct RealtimeConnectionConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub api_flavor: RealtimeApiFlavor,
    pub params: ConversationStartParams,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RealtimeApiFlavor {
    OpenAi,
    AzureOpenAi,
}

impl RealtimeApiFlavor {
    pub fn from_backend_id(backend_id: &str) -> Self {
        if backend_id.eq_ignore_ascii_case("azure") {
            Self::AzureOpenAi
        } else {
            Self::OpenAi
        }
    }
}

pub struct RealtimeConnection {
    pub provider_session_id: Option<String>,
    pub call_id: Option<String>,
    pub sdp: Option<String>,
    pub version: RealtimeConversationVersion,
    pub events: mpsc::Receiver<RealtimeTransportEvent>,
}

struct WebrtcCall {
    sdp: String,
    call_id: String,
}

impl RealtimeConversationManager {
    pub async fn start(&self, config: RealtimeConnectionConfig) -> Result<RealtimeConnection> {
        let mut config = config;
        if config.api_flavor == RealtimeApiFlavor::AzureOpenAi {
            config.base_url = normalize_azure_realtime_base_url(&config.base_url)?;
        }
        let mut state = self.state.lock().await;
        if state
            .as_ref()
            .is_some_and(|current| current.active.load(Ordering::Acquire))
        {
            bail!("a realtime conversation is already active");
        }
        if let Some(stale) = state.take() {
            stale.task.abort();
        }
        validate_start(&config)?;

        let version = config.params.version;
        let handoff_mode = config.params.codex_response_handoff_mode;
        let is_direct_websocket = matches!(
            config.params.transport,
            ConversationStartTransport::Websocket
        );
        let (sdp, call_id, endpoint, initialize_session, initialize_items) =
            match &config.params.transport {
                ConversationStartTransport::Websocket => (
                    None,
                    None,
                    websocket_endpoint(&config.base_url, &config.model, version, None)?,
                    true,
                    version == RealtimeConversationVersion::V2,
                ),
                ConversationStartTransport::Webrtc { sdp } => {
                    let call = create_webrtc_call(&config, sdp).await?;
                    let endpoint = websocket_endpoint(
                        &config.base_url,
                        &config.model,
                        version,
                        Some(&call.call_id),
                    )?;
                    (
                        Some(call.sdp),
                        Some(call.call_id),
                        endpoint,
                        false,
                        version == RealtimeConversationVersion::V2,
                    )
                }
                ConversationStartTransport::ExistingCall { call_id } => (
                    None,
                    Some(call_id.clone()),
                    websocket_endpoint(&config.base_url, &config.model, version, Some(call_id))?,
                    false,
                    false,
                ),
            };

        let (command_tx, command_rx) = mpsc::channel(INPUT_QUEUE_CAPACITY);
        let (event_tx, event_rx) = mpsc::channel(OUTPUT_QUEUE_CAPACITY);
        let cancel = CancellationToken::new();
        let active = Arc::new(AtomicBool::new(true));
        let (initial_socket, initial_event) = if is_direct_websocket {
            let mut socket = connect(&endpoint, &config.api_key, config.api_flavor).await?;
            let event = await_session_started(&mut socket, version).await?;
            (Some(socket), event)
        } else {
            // Returning the SDP answer must not wait for the sideband socket. Some providers
            // activate that endpoint only after the browser applies the answer.
            (None, None)
        };
        let provider_session_id = initial_event
            .as_ref()
            .and_then(|event| match event {
                RealtimeEvent::SessionUpdated {
                    realtime_session_id,
                    ..
                } => realtime_session_id.clone(),
                _ => None,
            })
            .or_else(|| call_id.clone());
        if let Some(event) = initial_event {
            event_tx
                .try_send(RealtimeTransportEvent::Event(event))
                .expect("new realtime event queue has capacity");
        }
        let task = tokio::spawn(run_transport(TransportTask {
            initial_socket,
            endpoint,
            api_key: config.api_key,
            api_flavor: config.api_flavor,
            model: config.model.clone(),
            params: config.params.clone(),
            initialize_session,
            initialize_items,
            retry_initial_connect: initial_sideband_retry_enabled(
                config.api_flavor,
                version,
                &config.params.transport,
            ),
            reconnect_on_loss: sideband_reconnect_enabled(version, &config.params.transport),
            command_rx,
            event_tx,
            cancel: cancel.clone(),
            active: Arc::clone(&active),
        }));
        *state = Some(ConversationState {
            command_tx,
            cancel,
            active,
            task,
            version,
            handoff_mode,
        });
        Ok(RealtimeConnection {
            provider_session_id,
            call_id,
            sdp,
            version,
            events: event_rx,
        })
    }

    pub async fn send_audio(&self, frame: RealtimeAudioFrame) -> Result<()> {
        if frame.data.is_empty() {
            bail!("realtime audio frame is empty");
        }
        if frame.data.len() > MAX_AUDIO_FRAME_BYTES {
            bail!("realtime audio frame exceeds 1 MiB");
        }
        if frame.format != RealtimeAudioFormat::Pcm16
            || frame.sample_rate != agent_protocol::DEFAULT_REALTIME_SAMPLE_RATE
            || frame.num_channels != 1
        {
            bail!("realtime input must be mono PCM16 at 24000 Hz");
        }
        match self.sender().await?.try_send(RealtimeCommand::Audio(frame)) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                tracing::warn!("dropping realtime input audio frame because the queue is full");
                Ok(())
            }
            Err(TrySendError::Closed(_)) => Err(anyhow!("realtime conversation is closed")),
        }
    }

    pub async fn send_text(&self, params: ConversationTextParams) -> Result<()> {
        self.sender()
            .await?
            .send(RealtimeCommand::Text(params))
            .await
            .map_err(|_| anyhow!("realtime conversation is closed"))
    }

    pub async fn send_speech(&self, text: String) -> Result<()> {
        self.sender()
            .await?
            .send(RealtimeCommand::Speech(text))
            .await
            .map_err(|_| anyhow!("realtime conversation is closed"))
    }

    pub async fn send_handoff_delta(
        &self,
        handoff_id: String,
        text: String,
        final_answer: bool,
    ) -> Result<()> {
        self.sender()
            .await?
            .send(RealtimeCommand::HandoffDelta {
                handoff_id,
                text,
                final_answer,
            })
            .await
            .map_err(|_| anyhow!("realtime conversation is closed"))
    }

    pub async fn complete_handoff(&self, handoff_id: String) -> Result<()> {
        self.sender()
            .await?
            .send(RealtimeCommand::HandoffComplete { handoff_id })
            .await
            .map_err(|_| anyhow!("realtime conversation is closed"))
    }

    pub async fn version_and_handoff_mode(
        &self,
    ) -> Option<(RealtimeConversationVersion, CodexResponseHandoffMode)> {
        self.state
            .lock()
            .await
            .as_ref()
            .map(|state| (state.version, state.handoff_mode))
    }

    async fn sender(&self) -> Result<mpsc::Sender<RealtimeCommand>> {
        self.state
            .lock()
            .await
            .as_ref()
            .filter(|state| state.active.load(Ordering::Acquire))
            .map(|state| state.command_tx.clone())
            .ok_or_else(|| anyhow!("no realtime conversation is active"))
    }

    pub async fn close(&self) {
        let Some(state) = self.state.lock().await.take() else {
            return;
        };
        let _ = tokio::time::timeout(CLOSE_TIMEOUT, state.command_tx.send(RealtimeCommand::Close))
            .await;
        let mut task = state.task;
        if tokio::time::timeout(CLOSE_TIMEOUT, &mut task)
            .await
            .is_err()
        {
            state.cancel.cancel();
            task.abort();
        }
    }
}

async fn await_session_started(
    socket: &mut RealtimeSocket,
    version: RealtimeConversationVersion,
) -> Result<Option<RealtimeEvent>> {
    tokio::time::timeout(CONNECT_TIMEOUT, async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    if let Some(event) = parse_realtime_event(version, &text)? {
                        match event {
                            RealtimeEvent::SessionUpdated { .. } => return Ok(Some(event)),
                            RealtimeEvent::Error(message) => bail!(message),
                            _ => {}
                        }
                    }
                }
                Some(Ok(Message::Ping(payload))) => socket
                    .send(Message::Pong(payload))
                    .await
                    .context("reply to realtime handshake ping")?,
                Some(Ok(Message::Close(frame))) => bail!(
                    "{}",
                    frame
                        .map(|frame| frame.reason.to_string())
                        .unwrap_or_else(|| "realtime transport closed during handshake".into())
                ),
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(error).context("realtime handshake failed"),
                None => bail!("realtime transport closed during handshake"),
            }
        }
    })
    .await
    .context("timed out waiting for realtime session start")?
}

fn validate_start(config: &RealtimeConnectionConfig) -> Result<()> {
    if config.api_key.trim().is_empty() {
        bail!("the active provider has no API key for realtime authentication");
    }
    if config.api_flavor == RealtimeApiFlavor::AzureOpenAi
        && config.params.version != RealtimeConversationVersion::V2
    {
        bail!("Azure OpenAI Realtime supports the V2 GA protocol only");
    }
    match &config.params.transport {
        ConversationStartTransport::Webrtc { sdp } if sdp.trim().is_empty() => {
            bail!("WebRTC requires an SDP offer")
        }
        ConversationStartTransport::ExistingCall { call_id } if call_id.trim().is_empty() => {
            bail!("existing_call requires a call id")
        }
        ConversationStartTransport::ExistingCall { .. }
            if config.params.include_startup_context
                || config.params.instructions.is_some()
                || !config.params.initial_items.is_empty() =>
        {
            bail!("existing realtime calls do not accept session configuration")
        }
        _ => Ok(()),
    }
}

async fn create_webrtc_call(config: &RealtimeConnectionConfig, sdp: &str) -> Result<WebrtcCall> {
    if config.api_flavor == RealtimeApiFlavor::AzureOpenAi {
        return create_azure_webrtc_call(config, sdp).await;
    }
    let endpoint = calls_endpoint(&config.base_url)?;
    let session = session_config(&config.model, &config.params);
    let form = Form::new()
        .part(
            "sdp",
            Part::text(sdp.to_string())
                .mime_str("application/sdp")
                .context("build SDP multipart field")?,
        )
        .part(
            "session",
            Part::text(session.to_string())
                .mime_str("application/json")
                .context("build session multipart field")?,
        );
    let response = reqwest::Client::new()
        .post(endpoint.clone())
        .bearer_auth(&config.api_key)
        .header(USER_AGENT, "astro-agent/realtime")
        .multipart(form)
        .send()
        .await
        .with_context(|| format!("create realtime WebRTC call {endpoint}"))?;
    finish_webrtc_call(response, &endpoint).await
}

async fn create_azure_webrtc_call(
    config: &RealtimeConnectionConfig,
    sdp: &str,
) -> Result<WebrtcCall> {
    let client = reqwest::Client::new();
    let secret_endpoint = client_secrets_endpoint(&config.base_url)?;
    let secret_response = azure_client_secret_request(&client, secret_endpoint.clone(), config)
        .send()
        .await
        .with_context(|| format!("create Azure Realtime client secret {secret_endpoint}"))?;
    let secret_status = secret_response.status();
    let secret_body = secret_response
        .text()
        .await
        .context("read Azure Realtime client secret response")?;
    if !secret_status.is_success() {
        bail!("Azure Realtime client secret failed ({secret_status}): {secret_body}");
    }
    let ephemeral_key = serde_json::from_str::<Value>(&secret_body)
        .context("decode Azure Realtime client secret response")?
        .get("value")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("Azure Realtime client secret response omitted value"))?;

    let mut endpoint = calls_endpoint(&config.base_url)?;
    endpoint.query_pairs_mut().append_pair("webrtcfilter", "on");
    let response = azure_sdp_request(&client, endpoint.clone(), &ephemeral_key, sdp)
        .send()
        .await
        .with_context(|| format!("create Azure Realtime WebRTC call {endpoint}"))?;
    finish_webrtc_call(response, &endpoint).await
}

fn azure_client_secret_request(
    client: &reqwest::Client,
    endpoint: Url,
    config: &RealtimeConnectionConfig,
) -> reqwest::RequestBuilder {
    client
        .post(endpoint)
        .header("api-key", config.api_key.trim())
        .header(USER_AGENT, "astro-agent/realtime")
        .json(&serde_json::json!({
            "session": session_config(&config.model, &config.params)
        }))
}

fn azure_sdp_request(
    client: &reqwest::Client,
    endpoint: Url,
    ephemeral_key: &str,
    sdp: &str,
) -> reqwest::RequestBuilder {
    client
        .post(endpoint)
        .bearer_auth(ephemeral_key)
        .header(USER_AGENT, "astro-agent/realtime")
        .header(CONTENT_TYPE, "application/sdp")
        .body(sdp.to_string())
}

async fn finish_webrtc_call(response: reqwest::Response, endpoint: &Url) -> Result<WebrtcCall> {
    let status = response.status();
    let location = response
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let answer = response.text().await.context("read realtime SDP answer")?;
    if !status.is_success() {
        bail!("realtime WebRTC call failed ({status}) at {endpoint}: {answer}");
    }
    let call_id = location
        .as_deref()
        .and_then(call_id_from_location)
        .ok_or_else(|| anyhow!("realtime WebRTC response omitted the call id Location header"))?;
    Ok(WebrtcCall {
        sdp: answer,
        call_id,
    })
}

struct TransportTask {
    initial_socket: Option<RealtimeSocket>,
    endpoint: Url,
    api_key: String,
    api_flavor: RealtimeApiFlavor,
    model: String,
    params: ConversationStartParams,
    initialize_session: bool,
    initialize_items: bool,
    retry_initial_connect: bool,
    reconnect_on_loss: bool,
    command_rx: mpsc::Receiver<RealtimeCommand>,
    event_tx: mpsc::Sender<RealtimeTransportEvent>,
    cancel: CancellationToken,
    active: Arc<AtomicBool>,
}

async fn run_transport(mut task: TransportTask) {
    let mut attempt = 0_u32;
    let reason = loop {
        let connection = match task.initial_socket.take() {
            Some(socket) => Ok(socket),
            None => connect(&task.endpoint, &task.api_key, task.api_flavor).await,
        };
        match connection {
            Ok(mut websocket) => {
                attempt = 0;
                if task.initialize_session {
                    let update = session_update(&task.model, &task.params);
                    if let Err(error) = websocket
                        .send(Message::Text(update.to_string().into()))
                        .await
                    {
                        break Some(format!("realtime session initialization failed: {error}"));
                    }
                    if task.initialize_items {
                        let mut startup_error = None;
                        for item in &task.params.initial_items {
                            let payload = text_events(task.params.version, item).remove(0);
                            if let Err(error) = websocket
                                .send(Message::Text(payload.to_string().into()))
                                .await
                            {
                                startup_error =
                                    Some(format!("realtime startup context failed: {error}"));
                                break;
                            }
                        }
                        if let Some(error) = startup_error {
                            break Some(error);
                        }
                    }
                } else if task.initialize_items {
                    let mut startup_error = None;
                    for item in &task.params.initial_items {
                        let payload = text_events(task.params.version, item).remove(0);
                        if let Err(error) = websocket
                            .send(Message::Text(payload.to_string().into()))
                            .await
                        {
                            startup_error =
                                Some(format!("realtime startup context failed: {error}"));
                            break;
                        }
                    }
                    if let Some(error) = startup_error {
                        break Some(error);
                    }
                }
                match run_socket(&mut websocket, &mut task).await {
                    SocketExit::Requested(reason) => break reason,
                    SocketExit::Lost(reason)
                        if task.reconnect_on_loss
                            && attempt < MAX_SIDEBAND_RETRIES
                            && !task.cancel.is_cancelled() =>
                    {
                        attempt += 1;
                        let delay = RECONNECT_BASE_DELAY
                            .saturating_mul(2_u32.saturating_pow(attempt - 1))
                            .min(RECONNECT_MAX_DELAY);
                        tracing::warn!(attempt, delay_ms = delay.as_millis(), %reason, "reconnecting realtime V3 sideband");
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    SocketExit::Lost(reason) => break Some(reason),
                }
            }
            Err(error)
                if task.retry_initial_connect
                    && attempt < MAX_SIDEBAND_RETRIES
                    && !task.cancel.is_cancelled() =>
            {
                attempt += 1;
                let delay = RECONNECT_BASE_DELAY
                    .saturating_mul(2_u32.saturating_pow(attempt - 1))
                    .min(RECONNECT_MAX_DELAY);
                tracing::warn!(attempt, delay_ms = delay.as_millis(), %error, "retrying initial realtime sideband connection");
                tokio::time::sleep(delay).await;
            }
            Err(error) => break Some(error.to_string()),
        }
    };
    task.active.store(false, Ordering::Release);
    let _ = task
        .event_tx
        .send(RealtimeTransportEvent::Closed(reason))
        .await;
}

enum SocketExit {
    Requested(Option<String>),
    Lost(String),
}

async fn run_socket<S>(
    websocket: &mut tokio_tungstenite::WebSocketStream<S>,
    task: &mut TransportTask,
) -> SocketExit
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    loop {
        tokio::select! {
            _ = task.cancel.cancelled() => {
                let _ = websocket.close(None).await;
                return SocketExit::Requested(Some("cancelled".into()));
            }
            command = task.command_rx.recv() => {
                let Some(command) = command else {
                    let _ = websocket.close(None).await;
                    return SocketExit::Requested(Some("input channel closed".into()));
                };
                if matches!(command, RealtimeCommand::Close) {
                    if task.params.version == RealtimeConversationVersion::V3 {
                        let _ = websocket.send(Message::Text(
                            serde_json::json!({"type": "session.close"}).to_string().into()
                        )).await;
                    }
                    let _ = websocket.close(None).await;
                    return SocketExit::Requested(Some("requested".into()));
                }
                for payload in command_payloads(command, task.params.version, task.params.codex_response_handoff_mode) {
                    if let Err(error) = websocket.send(Message::Text(payload.to_string().into())).await {
                        return SocketExit::Lost(format!("realtime write failed: {error}"));
                    }
                }
            }
            message = websocket.next() => match message {
                Some(Ok(Message::Text(text))) => match parse_realtime_event(task.params.version, &text) {
                    Ok(Some(event)) => {
                        if task.params.version == RealtimeConversationVersion::V2 {
                            if let RealtimeEvent::NoopRequested { call_id, .. } = &event {
                                let payload = function_call_output(call_id, "");
                                if let Err(error) = websocket
                                    .send(Message::Text(payload.to_string().into()))
                                    .await
                                {
                                    return SocketExit::Lost(format!(
                                        "failed to acknowledge remain_silent: {error}"
                                    ));
                                }
                            }
                        }
                        if task.event_tx.send(RealtimeTransportEvent::Event(event)).await.is_err() {
                            return SocketExit::Requested(Some("event receiver closed".into()));
                        }
                    }
                    Ok(None) => {}
                    Err(error) => return SocketExit::Lost(error.to_string()),
                },
                Some(Ok(Message::Binary(bytes))) => {
                    let event = RealtimeEvent::AudioOut(RealtimeAudioFrame {
                        data: bytes.to_vec(), sample_rate: 24_000, num_channels: 1,
                        format: RealtimeAudioFormat::Pcm16,
                    });
                    if task.event_tx.send(RealtimeTransportEvent::Event(event)).await.is_err() {
                        return SocketExit::Requested(Some("event receiver closed".into()));
                    }
                }
                Some(Ok(Message::Close(frame))) => {
                    if frame.as_ref().is_some_and(|frame| frame.code == CloseCode::Normal) {
                        return SocketExit::Requested(None);
                    }
                    return SocketExit::Lost(frame.map(|frame| frame.reason.to_string()).unwrap_or_else(|| "realtime transport closed".into()));
                }
                Some(Ok(Message::Ping(payload))) => {
                    if let Err(error) = websocket.send(Message::Pong(payload)).await {
                        return SocketExit::Lost(error.to_string());
                    }
                }
                Some(Ok(Message::Pong(_))) | Some(Ok(Message::Frame(_))) => {}
                Some(Err(error)) => return SocketExit::Lost(format!("realtime transport failed: {error}")),
                None => return SocketExit::Lost("realtime transport closed".into()),
            }
        }
    }
}

fn command_payloads(
    command: RealtimeCommand,
    version: RealtimeConversationVersion,
    handoff_mode: CodexResponseHandoffMode,
) -> Vec<Value> {
    match command {
        RealtimeCommand::Audio(frame) => vec![audio_append(version, &frame)],
        RealtimeCommand::Text(params) => text_events(version, &params),
        RealtimeCommand::Speech(text) => match version {
            RealtimeConversationVersion::V2 => vec![speech_event(version, &text)],
            RealtimeConversationVersion::V3 => crate::context_append_chunks(&text)
                .into_iter()
                .map(|chunk| speech_event(version, &chunk))
                .collect(),
        },
        RealtimeCommand::HandoffDelta {
            handoff_id,
            text,
            final_answer,
        } => match version {
            RealtimeConversationVersion::V2 => vec![handoff_append(
                version,
                &handoff_id,
                &text,
                handoff_mode,
                final_answer,
            )],
            RealtimeConversationVersion::V3 => crate::context_append_chunks(&text)
                .into_iter()
                .map(|chunk| {
                    handoff_append(version, &handoff_id, &chunk, handoff_mode, final_answer)
                })
                .collect(),
        },
        RealtimeCommand::HandoffComplete { handoff_id } => {
            let mut payloads = vec![handoff_complete(version, &handoff_id)];
            if version == RealtimeConversationVersion::V2 {
                payloads.push(serde_json::json!({"type": "response.create"}));
            }
            payloads
        }
        RealtimeCommand::Close => Vec::new(),
    }
}

fn websocket_request(
    endpoint: &Url,
    api_key: &str,
    api_flavor: RealtimeApiFlavor,
) -> Result<tokio_tungstenite::tungstenite::http::Request<()>> {
    let mut request = endpoint
        .as_str()
        .into_client_request()
        .context("build realtime websocket request")?;
    match api_flavor {
        RealtimeApiFlavor::OpenAi => {
            request.headers_mut().insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {api_key}"))
                    .context("invalid realtime API key")?,
            );
        }
        RealtimeApiFlavor::AzureOpenAi => {
            request.headers_mut().insert(
                "api-key",
                HeaderValue::from_str(api_key).context("invalid Azure Realtime API key")?,
            );
        }
    }
    request
        .headers_mut()
        .insert(USER_AGENT, HeaderValue::from_static("astro-agent/realtime"));
    Ok(request)
}

async fn connect(
    endpoint: &Url,
    api_key: &str,
    api_flavor: RealtimeApiFlavor,
) -> Result<RealtimeSocket> {
    let request = websocket_request(endpoint, api_key, api_flavor)?;
    let (socket, _) =
        tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(request))
            .await
            .context("realtime websocket connection timed out")?
            .with_context(|| format!("connect realtime websocket {endpoint}"))?;
    Ok(socket)
}

pub fn calls_endpoint(base_url: &str) -> Result<Url> {
    let mut url = api_url(base_url)?;
    let path = url.path().trim_end_matches('/');
    if path.ends_with("/realtime") {
        url.set_path(&format!("{path}/calls"));
    } else {
        url.set_path(&format!("{path}/realtime/calls"));
    }
    Ok(url)
}

pub fn client_secrets_endpoint(base_url: &str) -> Result<Url> {
    let mut url = api_url(base_url)?;
    let path = url.path().trim_end_matches('/');
    if path.ends_with("/realtime") {
        url.set_path(&format!("{path}/client_secrets"));
    } else {
        url.set_path(&format!("{path}/realtime/client_secrets"));
    }
    url.set_query(None);
    Ok(url)
}

pub fn websocket_endpoint(
    base_url: &str,
    model: &str,
    version: RealtimeConversationVersion,
    call_id: Option<&str>,
) -> Result<Url> {
    let mut url = api_url(base_url)?;
    url.set_scheme(match url.scheme() {
        "https" => "wss",
        "http" => "ws",
        "wss" => "wss",
        "ws" => "ws",
        scheme => bail!("unsupported realtime URL scheme {scheme}"),
    })
    .map_err(|_| anyhow!("invalid realtime websocket scheme"))?;
    let path = url.path().trim_end_matches('/').to_string();
    match version {
        RealtimeConversationVersion::V2 => {
            if !path.ends_with("/realtime") {
                url.set_path(&format!("{path}/realtime"));
            }
        }
        RealtimeConversationVersion::V3 => {
            let prefix = path.strip_suffix("/realtime").unwrap_or(&path);
            url.set_path(&format!("{prefix}/live"));
        }
    }
    url.set_query(None);
    if let Some(call_id) = call_id {
        if call_id == "." || call_id == ".." || call_id.contains('/') {
            bail!("invalid realtime call id");
        }
        match version {
            RealtimeConversationVersion::V2 => {
                url.query_pairs_mut().append_pair("call_id", call_id);
            }
            RealtimeConversationVersion::V3 => {
                url.path_segments_mut()
                    .map_err(|_| anyhow!("realtime URL cannot contain path segments"))?
                    .push(call_id);
            }
        }
    } else {
        url.query_pairs_mut().append_pair("model", model);
    }
    Ok(url)
}

fn api_url(base_url: &str) -> Result<Url> {
    Url::parse(if base_url.trim().is_empty() {
        "https://api.openai.com/v1"
    } else {
        base_url.trim().trim_end_matches('/')
    })
    .context("invalid realtime base URL")
}

pub fn normalize_azure_realtime_base_url(base_url: &str) -> Result<String> {
    let mut url = Url::parse(base_url.trim()).context("invalid Azure Realtime base URL")?;
    if url.query().is_some() || url.fragment().is_some() {
        bail!("Azure Realtime requires the GA /openai/v1 endpoint without api-version");
    }
    let path = url.path().trim_end_matches('/');
    let normalized = match path {
        "" | "/" | "/openai" | "/v1" => "/openai/v1",
        "/openai/v1" => "/openai/v1",
        "/openai/v1/realtime" => "/openai/v1",
        _ => bail!("Azure Realtime base URL must be the resource root or end with /openai/v1"),
    };
    url.set_path(normalized);
    Ok(url.to_string().trim_end_matches('/').to_string())
}

fn sideband_reconnect_enabled(
    version: RealtimeConversationVersion,
    transport: &ConversationStartTransport,
) -> bool {
    version == RealtimeConversationVersion::V3
        && matches!(
            transport,
            ConversationStartTransport::Webrtc { .. }
                | ConversationStartTransport::ExistingCall { .. }
        )
}

fn initial_sideband_retry_enabled(
    api_flavor: RealtimeApiFlavor,
    version: RealtimeConversationVersion,
    transport: &ConversationStartTransport,
) -> bool {
    sideband_reconnect_enabled(version, transport)
        || (api_flavor == RealtimeApiFlavor::AzureOpenAi
            && matches!(
                transport,
                ConversationStartTransport::Webrtc { .. }
                    | ConversationStartTransport::ExistingCall { .. }
            ))
}

fn call_id_from_location(location: &str) -> Option<String> {
    if let Ok(url) = Url::parse(location) {
        if let Some(call_id) = url
            .query_pairs()
            .find_map(|(key, value)| (key == "call_id").then(|| value.into_owned()))
        {
            return (!call_id.is_empty()).then_some(call_id);
        }
    }
    location
        .split('?')
        .next()?
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    #[test]
    fn endpoints_are_version_and_call_aware() {
        assert_eq!(
            websocket_endpoint(
                "https://api.openai.com/v1",
                "gpt-realtime",
                RealtimeConversationVersion::V2,
                Some("rtc_123")
            )
            .unwrap()
            .as_str(),
            "wss://api.openai.com/v1/realtime?call_id=rtc_123"
        );
        assert_eq!(
            websocket_endpoint(
                "https://api.openai.com/v1/realtime",
                "gpt-live",
                RealtimeConversationVersion::V3,
                Some("rtc_123")
            )
            .unwrap()
            .as_str(),
            "wss://api.openai.com/v1/live/rtc_123"
        );
        assert_eq!(
            calls_endpoint("https://example.openai.azure.com/openai/v1")
                .unwrap()
                .as_str(),
            "https://example.openai.azure.com/openai/v1/realtime/calls"
        );
        assert_eq!(
            client_secrets_endpoint("https://example.openai.azure.com/openai/v1")
                .unwrap()
                .as_str(),
            "https://example.openai.azure.com/openai/v1/realtime/client_secrets"
        );
    }

    #[test]
    fn normalizes_azure_ga_base_and_rejects_preview_query() {
        assert_eq!(
            normalize_azure_realtime_base_url("https://example.openai.azure.com").unwrap(),
            "https://example.openai.azure.com/openai/v1"
        );
        assert_eq!(
            normalize_azure_realtime_base_url(
                "https://example.services.ai.azure.com/openai/v1/realtime/"
            )
            .unwrap(),
            "https://example.services.ai.azure.com/openai/v1"
        );
        assert!(normalize_azure_realtime_base_url(
            "https://example.openai.azure.com/openai/realtimeapi/sessions?api-version=2025-04-01-preview"
        )
        .unwrap_err()
        .to_string()
        .contains("without api-version"));
    }

    #[test]
    fn extracts_call_id_from_location() {
        assert_eq!(
            call_id_from_location("https://api.openai.com/v1/realtime/calls/rtc_123?x=1"),
            Some("rtc_123".into())
        );
        assert_eq!(
            call_id_from_location(
                "https://example.openai.azure.com/openai/v1/realtime?call_id=rtc_azure"
            ),
            Some("rtc_azure".into())
        );
    }

    #[test]
    fn websocket_auth_is_provider_specific() {
        let endpoint = Url::parse("wss://example.invalid/openai/v1/realtime").unwrap();
        let openai = websocket_request(&endpoint, "openai-key", RealtimeApiFlavor::OpenAi).unwrap();
        assert_eq!(
            openai.headers().get(AUTHORIZATION).unwrap(),
            "Bearer openai-key"
        );
        assert!(openai.headers().get("api-key").is_none());

        let azure =
            websocket_request(&endpoint, "azure-key", RealtimeApiFlavor::AzureOpenAi).unwrap();
        assert_eq!(azure.headers().get("api-key").unwrap(), "azure-key");
        assert!(azure.headers().get(AUTHORIZATION).is_none());
    }

    #[test]
    fn only_v3_sideband_transports_reconnect() {
        let websocket = ConversationStartTransport::Websocket;
        let webrtc = ConversationStartTransport::Webrtc {
            sdp: "offer".into(),
        };
        let existing = ConversationStartTransport::ExistingCall {
            call_id: "rtc_123".into(),
        };

        assert!(!sideband_reconnect_enabled(
            RealtimeConversationVersion::V2,
            &webrtc
        ));
        assert!(!sideband_reconnect_enabled(
            RealtimeConversationVersion::V2,
            &existing
        ));
        assert!(!sideband_reconnect_enabled(
            RealtimeConversationVersion::V3,
            &websocket
        ));
        assert!(sideband_reconnect_enabled(
            RealtimeConversationVersion::V3,
            &webrtc
        ));
        assert!(sideband_reconnect_enabled(
            RealtimeConversationVersion::V3,
            &existing
        ));
        assert!(initial_sideband_retry_enabled(
            RealtimeApiFlavor::AzureOpenAi,
            RealtimeConversationVersion::V2,
            &webrtc
        ));
        assert!(initial_sideband_retry_enabled(
            RealtimeApiFlavor::AzureOpenAi,
            RealtimeConversationVersion::V2,
            &existing
        ));
        assert!(!initial_sideband_retry_enabled(
            RealtimeApiFlavor::OpenAi,
            RealtimeConversationVersion::V2,
            &webrtc
        ));
    }

    #[tokio::test]
    async fn websocket_start_waits_for_handshake_and_emits_typed_event() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            socket
                .send(Message::Text(
                    serde_json::json!({
                        "type": "session.created",
                        "session": {"id": "rt-test"}
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
            let update = socket.next().await.unwrap().unwrap();
            assert!(update.into_text().unwrap().contains("session.update"));
            while let Some(Ok(message)) = socket.next().await {
                if matches!(message, Message::Close(_)) {
                    break;
                }
            }
        });
        let manager = RealtimeConversationManager::default();
        let mut connection = manager
            .start(RealtimeConnectionConfig {
                api_key: "test-key".into(),
                base_url: format!("http://{address}/v1"),
                model: "gpt-realtime".into(),
                api_flavor: RealtimeApiFlavor::OpenAi,
                params: ConversationStartParams::default(),
            })
            .await
            .unwrap();
        assert_eq!(connection.provider_session_id.as_deref(), Some("rt-test"));
        assert!(matches!(
            connection.events.recv().await,
            Some(RealtimeTransportEvent::Event(
                RealtimeEvent::SessionUpdated { .. }
            ))
        ));
        manager.close().await;
        server.await.unwrap();
    }

    #[tokio::test]
    async fn remain_silent_is_acknowledged_without_creating_a_response() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            socket
                .send(Message::Text(
                    serde_json::json!({
                        "type": "session.created",
                        "session": {"id": "rt-silent"}
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();

            let update = socket.next().await.unwrap().unwrap();
            assert!(update.into_text().unwrap().contains("session.update"));
            socket
                .send(Message::Text(
                    serde_json::json!({
                        "type": "conversation.item.done",
                        "item": {
                            "id": "item-silent",
                            "call_id": "call-silent",
                            "type": "function_call",
                            "name": "remain_silent",
                            "arguments": "{}"
                        }
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();

            let acknowledgement = socket.next().await.unwrap().unwrap();
            let acknowledgement: Value =
                serde_json::from_str(&acknowledgement.into_text().unwrap()).unwrap();
            assert_eq!(acknowledgement["type"], "conversation.item.create");
            assert_eq!(acknowledgement["item"]["type"], "function_call_output");
            assert_eq!(acknowledgement["item"]["call_id"], "call-silent");
            assert_eq!(acknowledgement["item"]["output"], "");

            assert!(
                tokio::time::timeout(Duration::from_millis(100), socket.next())
                    .await
                    .is_err()
            );
        });
        let manager = RealtimeConversationManager::default();
        let mut connection = manager
            .start(RealtimeConnectionConfig {
                api_key: "test-key".into(),
                base_url: format!("http://{address}/v1"),
                model: "gpt-realtime".into(),
                api_flavor: RealtimeApiFlavor::OpenAi,
                params: ConversationStartParams::default(),
            })
            .await
            .unwrap();
        assert!(matches!(
            connection.events.recv().await,
            Some(RealtimeTransportEvent::Event(
                RealtimeEvent::SessionUpdated { .. }
            ))
        ));
        assert!(matches!(
            connection.events.recv().await,
            Some(RealtimeTransportEvent::Event(
                RealtimeEvent::NoopRequested { ref call_id, .. }
            )) if call_id == "call-silent"
        ));
        server.await.unwrap();
        manager.close().await;
    }

    #[tokio::test]
    async fn webrtc_requests_follow_openai_and_azure_contracts() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            let header_end = loop {
                let read = stream.read(&mut buffer).await.unwrap();
                assert!(read > 0);
                request.extend_from_slice(&buffer[..read]);
                if let Some(index) = request.windows(4).position(|value| value == b"\r\n\r\n") {
                    break index + 4;
                }
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|value| value.trim().parse::<usize>().ok())
                })
                .unwrap();
            while request.len() < header_end + content_length {
                let read = stream.read(&mut buffer).await.unwrap();
                assert!(read > 0);
                request.extend_from_slice(&buffer[..read]);
            }
            let request = String::from_utf8_lossy(&request);
            assert!(request.starts_with("POST /v1/realtime/calls "));
            assert!(request.contains("offer-sdp"));
            assert!(request.contains("application/json"));
            let answer = "answer-sdp";
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/sdp\r\nContent-Length: {}\r\nLocation: /v1/realtime/calls/rtc_test\r\nConnection: close\r\n\r\n{}",
                        answer.len(), answer
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let config = RealtimeConnectionConfig {
            api_key: "test-key".into(),
            base_url: format!("http://{address}/v1"),
            model: "gpt-realtime".into(),
            api_flavor: RealtimeApiFlavor::OpenAi,
            params: ConversationStartParams {
                transport: ConversationStartTransport::Webrtc {
                    sdp: "offer-sdp".into(),
                },
                ..ConversationStartParams::default()
            },
        };
        let manager = RealtimeConversationManager::default();
        let connection = tokio::time::timeout(CONNECT_TIMEOUT, manager.start(config))
            .await
            .expect("WebRTC start must return before sideband connection")
            .unwrap();
        assert_eq!(connection.sdp.as_deref(), Some("answer-sdp"));
        assert_eq!(connection.call_id.as_deref(), Some("rtc_test"));
        manager.close().await;
        server.await.unwrap();

        // Build the Azure requests after the live HTTP fixture is done so TLS/client
        // initialization cannot contend with the timing-sensitive local handshake test.
        let config = RealtimeConnectionConfig {
            api_key: "azure-key".into(),
            base_url: "https://example.openai.azure.com/openai/v1".into(),
            model: "deployment-realtime".into(),
            api_flavor: RealtimeApiFlavor::AzureOpenAi,
            params: ConversationStartParams {
                transport: ConversationStartTransport::Webrtc {
                    sdp: "azure-offer-sdp".into(),
                },
                ..ConversationStartParams::default()
            },
        };
        let client = reqwest::Client::new();
        let secret = azure_client_secret_request(
            &client,
            client_secrets_endpoint(&config.base_url).unwrap(),
            &config,
        )
        .build()
        .unwrap();
        assert_eq!(secret.url().path(), "/openai/v1/realtime/client_secrets");
        assert_eq!(secret.headers().get("api-key").unwrap(), "azure-key");
        assert!(secret.headers().get(AUTHORIZATION).is_none());
        let secret_body = secret.body().unwrap().as_bytes().unwrap();
        assert!(String::from_utf8_lossy(secret_body).contains("\"model\":\"deployment-realtime\""));

        let mut calls = calls_endpoint(&config.base_url).unwrap();
        calls.query_pairs_mut().append_pair("webrtcfilter", "on");
        let offer = azure_sdp_request(&client, calls, "ephemeral-token", "azure-offer-sdp")
            .build()
            .unwrap();
        assert_eq!(offer.url().query(), Some("webrtcfilter=on"));
        assert_eq!(
            offer.headers().get(AUTHORIZATION).unwrap(),
            "Bearer ephemeral-token"
        );
        assert_eq!(
            offer.headers().get(CONTENT_TYPE).unwrap(),
            "application/sdp"
        );
        assert_eq!(
            offer.body().unwrap().as_bytes().unwrap(),
            b"azure-offer-sdp"
        );
    }
}
