//! Session-scoped OpenAI-compatible realtime WebSocket transport.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use agent_protocol::{
    ConversationStartParams, ConversationTextParams, RealtimeAudioFrame, RealtimeNoiseReduction,
    RealtimeOutputModality, RealtimeTurnDetection,
};
use anyhow::{anyhow, bail, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::{AUTHORIZATION, USER_AGENT};
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use url::Url;

const INPUT_QUEUE_CAPACITY: usize = 256;
const OUTPUT_QUEUE_CAPACITY: usize = 256;
const MAX_AUDIO_FRAME_BYTES: usize = 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub(crate) enum RealtimeTransportEvent {
    Payload(Value),
    Closed(Option<String>),
}

#[derive(Debug)]
enum RealtimeCommand {
    Audio(RealtimeAudioFrame),
    Text(ConversationTextParams),
    Speech(String),
    Close,
}

struct ConversationState {
    command_tx: mpsc::Sender<RealtimeCommand>,
    cancel: CancellationToken,
    active: Arc<AtomicBool>,
    task: JoinHandle<()>,
}

#[derive(Default)]
pub(crate) struct RealtimeConversationManager {
    state: Mutex<Option<ConversationState>>,
}

pub(crate) struct RealtimeConnectionConfig {
    pub(crate) api_key: String,
    pub(crate) base_url: String,
    pub(crate) model: String,
    pub(crate) params: ConversationStartParams,
}

pub(crate) struct RealtimeConnection {
    pub(crate) provider_session_id: Option<String>,
    pub(crate) events: mpsc::Receiver<RealtimeTransportEvent>,
}

impl RealtimeConversationManager {
    pub(crate) async fn start(
        &self,
        config: RealtimeConnectionConfig,
    ) -> Result<RealtimeConnection> {
        let mut state = self.state.lock().await;
        if let Some(current) = state.as_ref() {
            if current.active.load(Ordering::Acquire) {
                bail!("a realtime conversation is already active");
            }
        }
        if let Some(stale) = state.take() {
            stale.task.abort();
        }

        if config.api_key.trim().is_empty() {
            bail!("the active provider has no API key for realtime authentication");
        }
        let endpoint = realtime_endpoint(&config.base_url, &config.model)?;
        let mut request = endpoint
            .as_str()
            .into_client_request()
            .context("build realtime websocket request")?;
        let bearer = HeaderValue::from_str(&format!("Bearer {}", config.api_key))
            .context("invalid realtime API key")?;
        request.headers_mut().insert(AUTHORIZATION, bearer);
        request
            .headers_mut()
            .insert(USER_AGENT, HeaderValue::from_static("astro-agent/realtime"));
        let (mut websocket, _) =
            tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(request))
                .await
                .context("realtime websocket connection timed out")?
                .with_context(|| format!("connect realtime websocket {endpoint}"))?;
        let created = await_session_created(&mut websocket).await?;
        let provider_session_id = created
            .get("session")
            .and_then(|session| session.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string);
        websocket
            .send(Message::Text(
                session_update_message(&config.model, &config.params)
                    .to_string()
                    .into(),
            ))
            .await
            .context("send realtime session configuration")?;
        for item in &config.params.initial_items {
            websocket
                .send(Message::Text(text_item_message(item).to_string().into()))
                .await
                .context("send realtime initial conversation item")?;
        }

        let (command_tx, command_rx) = mpsc::channel(INPUT_QUEUE_CAPACITY);
        let (event_tx, event_rx) = mpsc::channel(OUTPUT_QUEUE_CAPACITY);
        event_tx
            .try_send(RealtimeTransportEvent::Payload(created))
            .expect("new realtime output queue has capacity");
        let cancel = CancellationToken::new();
        let active = Arc::new(AtomicBool::new(true));
        let task = tokio::spawn(run_connection(
            websocket,
            command_rx,
            event_tx,
            cancel.clone(),
            Arc::clone(&active),
        ));
        *state = Some(ConversationState {
            command_tx,
            cancel,
            active,
            task,
        });
        Ok(RealtimeConnection {
            provider_session_id,
            events: event_rx,
        })
    }

    pub(crate) async fn send_audio(&self, frame: RealtimeAudioFrame) -> Result<()> {
        if frame.data.is_empty() {
            bail!("realtime audio frame is empty");
        }
        if frame.data.len() > MAX_AUDIO_FRAME_BYTES {
            bail!("realtime audio frame exceeds 1 MiB");
        }
        if frame.format != agent_protocol::RealtimeAudioFormat::Pcm16
            || frame.sample_rate != agent_protocol::DEFAULT_REALTIME_SAMPLE_RATE
            || frame.num_channels != 1
        {
            bail!("realtime input must be mono PCM16 at 24000 Hz");
        }
        match self
            .active_sender()
            .await?
            .try_send(RealtimeCommand::Audio(frame))
        {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                tracing::warn!("dropping realtime input audio frame because the queue is full");
                Ok(())
            }
            Err(TrySendError::Closed(_)) => Err(anyhow!("realtime conversation is closed")),
        }
    }

    pub(crate) async fn send_text(&self, params: ConversationTextParams) -> Result<()> {
        self.active_sender()
            .await?
            .send(RealtimeCommand::Text(params))
            .await
            .map_err(|_| anyhow!("realtime conversation is closed"))
    }

    pub(crate) async fn send_speech(&self, text: String) -> Result<()> {
        self.active_sender()
            .await?
            .send(RealtimeCommand::Speech(text))
            .await
            .map_err(|_| anyhow!("realtime conversation is closed"))
    }

    async fn active_sender(&self) -> Result<mpsc::Sender<RealtimeCommand>> {
        let state = self.state.lock().await;
        let current = state
            .as_ref()
            .filter(|current| current.active.load(Ordering::Acquire))
            .ok_or_else(|| anyhow!("no realtime conversation is active"))?;
        Ok(current.command_tx.clone())
    }

    pub(crate) async fn close(&self) {
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

async fn await_session_created<S>(
    websocket: &mut tokio_tungstenite::WebSocketStream<S>,
) -> Result<Value>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    tokio::time::timeout(CONNECT_TIMEOUT, async {
        loop {
            match websocket.next().await {
                Some(Ok(Message::Text(text))) => {
                    let payload: Value =
                        serde_json::from_str(&text).context("invalid realtime handshake JSON")?;
                    match payload.get("type").and_then(Value::as_str) {
                        Some("session.created") => return Ok(payload),
                        Some("error") => {
                            let message = payload
                                .pointer("/error/message")
                                .and_then(Value::as_str)
                                .unwrap_or("realtime handshake rejected");
                            bail!(message.to_string());
                        }
                        _ => continue,
                    }
                }
                Some(Ok(Message::Ping(payload))) => {
                    websocket
                        .send(Message::Pong(payload))
                        .await
                        .context("reply to realtime handshake ping")?;
                }
                Some(Ok(Message::Close(frame))) => {
                    let reason = frame
                        .map(|frame| frame.reason.to_string())
                        .unwrap_or_else(|| "realtime transport closed during handshake".into());
                    bail!(reason);
                }
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(error).context("realtime handshake failed"),
                None => bail!("realtime transport closed during handshake"),
            }
        }
    })
    .await
    .context("timed out waiting for realtime session.created")?
}

async fn run_connection<S>(
    mut websocket: tokio_tungstenite::WebSocketStream<S>,
    mut command_rx: mpsc::Receiver<RealtimeCommand>,
    event_tx: mpsc::Sender<RealtimeTransportEvent>,
    cancel: CancellationToken,
    active: Arc<AtomicBool>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let reason = loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                let _ = websocket.close(None).await;
                break Some("cancelled".to_string());
            }
            command = command_rx.recv() => {
                let Some(command) = command else {
                    let _ = websocket.close(None).await;
                    break Some("input channel closed".to_string());
                };
                if matches!(command, RealtimeCommand::Close) {
                    let _ = websocket.close(None).await;
                    break Some("requested".to_string());
                }
                let payloads = match command {
                    RealtimeCommand::Audio(frame) => vec![audio_append_message(&frame)],
                    RealtimeCommand::Text(params) => vec![
                        text_item_message(&params),
                        json!({ "type": "response.create" }),
                    ],
                    RealtimeCommand::Speech(text) => vec![speech_message(&text)],
                    RealtimeCommand::Close => unreachable!(),
                };
                let mut write_failure = None;
                for payload in payloads {
                    if let Err(error) = websocket.send(Message::Text(payload.to_string().into())).await {
                        write_failure = Some(format!("realtime write failed: {error}"));
                        break;
                    }
                }
                if let Some(reason) = write_failure {
                    break Some(reason);
                }
            }
            message = websocket.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<Value>(&text) {
                            Ok(payload) => {
                                if event_tx.send(RealtimeTransportEvent::Payload(payload)).await.is_err() {
                                    break Some("event receiver closed".to_string());
                                }
                            }
                            Err(error) => {
                                break Some(format!("invalid realtime event JSON: {error}"));
                            }
                        }
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        let payload = json!({
                            "type": "astro.realtime.binary",
                            "audio": BASE64_STANDARD.encode(bytes),
                        });
                        if event_tx.send(RealtimeTransportEvent::Payload(payload)).await.is_err() {
                            break Some("event receiver closed".to_string());
                        }
                    }
                    Some(Ok(Message::Close(frame))) => {
                        break frame.map(|frame| frame.reason.to_string());
                    }
                    Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) | Some(Ok(Message::Frame(_))) => {}
                    Some(Err(error)) => break Some(format!("realtime transport failed: {error}")),
                    None => break Some("realtime transport closed".to_string()),
                }
            }
        }
    };
    active.store(false, Ordering::Release);
    let _ = event_tx.send(RealtimeTransportEvent::Closed(reason)).await;
}

fn realtime_endpoint(base_url: &str, model: &str) -> Result<Url> {
    let raw = if base_url.trim().is_empty() {
        "https://api.openai.com/v1"
    } else {
        base_url.trim().trim_end_matches('/')
    };
    let mut url = Url::parse(raw).context("invalid realtime base URL")?;
    match url.scheme() {
        "https" => url.set_scheme("wss").expect("wss is a valid scheme"),
        "http" => url.set_scheme("ws").expect("ws is a valid scheme"),
        "wss" | "ws" => {}
        scheme => bail!("unsupported realtime URL scheme {scheme}"),
    }
    let path = url.path().trim_end_matches('/');
    if !path.ends_with("/realtime") {
        url.set_path(&format!("{path}/realtime"));
    }
    let existing_query = url
        .query_pairs()
        .filter(|(key, _)| key != "model")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    url.set_query(None);
    {
        let mut query = url.query_pairs_mut();
        for (key, value) in existing_query {
            query.append_pair(&key, &value);
        }
        query.append_pair("model", model);
    }
    Ok(url)
}

fn session_update_message(model: &str, params: &ConversationStartParams) -> Value {
    let output_modalities = match params.output_modality {
        RealtimeOutputModality::Text => vec!["text"],
        RealtimeOutputModality::Audio => vec!["audio"],
    };
    let turn_detection = match params.turn_detection {
        RealtimeTurnDetection::Disabled => Value::Null,
        RealtimeTurnDetection::ServerVad => json!({
            "type": "server_vad",
            "create_response": true,
            "interrupt_response": true,
            "silence_duration_ms": 500,
        }),
        RealtimeTurnDetection::SemanticVad => json!({
            "type": "semantic_vad",
            "create_response": true,
            "interrupt_response": true,
        }),
    };
    let noise_reduction = params.noise_reduction.map(|mode| match mode {
        RealtimeNoiseReduction::NearField => json!({ "type": "near_field" }),
        RealtimeNoiseReduction::FarField => json!({ "type": "far_field" }),
    });
    let transcription = params
        .input_audio_transcription_model
        .as_ref()
        .map(|model| json!({ "model": model }));

    json!({
        "type": "session.update",
        "session": {
            "type": "realtime",
            "model": model,
            "instructions": params.instructions,
            "output_modalities": output_modalities,
            "audio": {
                "input": {
                    "format": { "type": "audio/pcm", "rate": 24_000 },
                    "transcription": transcription,
                    "noise_reduction": noise_reduction,
                    "turn_detection": turn_detection,
                },
                "output": {
                    "format": { "type": "audio/pcm", "rate": 24_000 },
                    "voice": params.voice.as_deref().unwrap_or("marin"),
                }
            }
        }
    })
}

fn audio_append_message(frame: &RealtimeAudioFrame) -> Value {
    json!({
        "type": "input_audio_buffer.append",
        "audio": BASE64_STANDARD.encode(&frame.data),
    })
}

fn text_item_message(params: &ConversationTextParams) -> Value {
    let role = match params.role {
        agent_protocol::ConversationTextRole::User => "user",
        agent_protocol::ConversationTextRole::Developer => "developer",
        agent_protocol::ConversationTextRole::Assistant => "assistant",
    };
    let content_type = if role == "assistant" {
        "output_text"
    } else {
        "input_text"
    };
    json!({
        "type": "conversation.item.create",
        "item": {
            "type": "message",
            "role": role,
            "content": [{ "type": content_type, "text": params.text }],
        }
    })
}

fn speech_message(text: &str) -> Value {
    json!({
        "type": "response.create",
        "response": {
            "instructions": text,
            "output_modalities": ["audio"],
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    #[test]
    fn endpoint_converts_https_and_preserves_provider_prefix() {
        let endpoint =
            realtime_endpoint("https://gateway.example/openai/v1/", "gpt-realtime").unwrap();
        assert_eq!(
            endpoint.as_str(),
            "wss://gateway.example/openai/v1/realtime?model=gpt-realtime"
        );
    }

    #[test]
    fn endpoint_does_not_duplicate_realtime_path() {
        let endpoint =
            realtime_endpoint("wss://api.openai.com/v1/realtime", "gpt-realtime-preview").unwrap();
        assert_eq!(
            endpoint.as_str(),
            "wss://api.openai.com/v1/realtime?model=gpt-realtime-preview"
        );
    }

    #[test]
    fn endpoint_replaces_an_existing_model_query() {
        let endpoint = realtime_endpoint(
            "wss://gateway.example/v1/realtime?tenant=astro&model=old",
            "gpt-realtime",
        )
        .unwrap();
        assert_eq!(
            endpoint.as_str(),
            "wss://gateway.example/v1/realtime?tenant=astro&model=gpt-realtime"
        );
    }

    #[test]
    fn session_update_carries_audio_controls() {
        let params = ConversationStartParams {
            voice: Some("coral".into()),
            noise_reduction: Some(RealtimeNoiseReduction::NearField),
            ..ConversationStartParams::default()
        };
        let update = session_update_message("gpt-realtime", &params);
        assert_eq!(update["session"]["audio"]["output"]["voice"], "coral");
        assert_eq!(
            update["session"]["audio"]["input"]["turn_detection"]["type"],
            "server_vad"
        );
    }

    #[test]
    fn audio_frames_are_base64_encoded() {
        let payload = audio_append_message(&RealtimeAudioFrame {
            data: vec![0, 1, 2],
            sample_rate: 24_000,
            num_channels: 1,
            format: agent_protocol::RealtimeAudioFormat::Pcm16,
        });
        assert_eq!(payload["audio"], "AAEC");
    }

    #[tokio::test]
    async fn full_audio_queue_drops_frames_without_blocking_control_loop() {
        let manager = RealtimeConversationManager::default();
        let (command_tx, _command_rx) = mpsc::channel(INPUT_QUEUE_CAPACITY);
        for _ in 0..INPUT_QUEUE_CAPACITY {
            command_tx
                .try_send(RealtimeCommand::Audio(RealtimeAudioFrame {
                    data: vec![0, 0],
                    sample_rate: agent_protocol::DEFAULT_REALTIME_SAMPLE_RATE,
                    num_channels: 1,
                    format: agent_protocol::RealtimeAudioFormat::Pcm16,
                }))
                .expect("queue has capacity");
        }
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let task = tokio::spawn(async move { task_cancel.cancelled().await });
        *manager.state.lock().await = Some(ConversationState {
            command_tx,
            cancel,
            active: Arc::new(AtomicBool::new(true)),
            task,
        });

        tokio::time::timeout(
            Duration::from_millis(50),
            manager.send_audio(RealtimeAudioFrame {
                data: vec![0, 0],
                sample_rate: agent_protocol::DEFAULT_REALTIME_SAMPLE_RATE,
                num_channels: 1,
                format: agent_protocol::RealtimeAudioFormat::Pcm16,
            }),
        )
        .await
        .expect("full audio queue must not block")
        .expect("dropping a frame is not a transport failure");

        manager.close().await;
    }

    #[tokio::test]
    async fn manager_streams_text_and_closes_cleanly() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut websocket = accept_async(stream).await.unwrap();
            websocket
                .send(Message::Text(
                    json!({
                        "type": "session.created",
                        "session": { "id": "sess-test" }
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
            let session_update = websocket.next().await.unwrap().unwrap();
            let session_update: Value =
                serde_json::from_str(session_update.to_text().unwrap()).unwrap();
            assert_eq!(session_update["type"], "session.update");

            let item = websocket.next().await.unwrap().unwrap();
            let item: Value = serde_json::from_str(item.to_text().unwrap()).unwrap();
            assert_eq!(item["type"], "conversation.item.create");
            let response = websocket.next().await.unwrap().unwrap();
            let response: Value = serde_json::from_str(response.to_text().unwrap()).unwrap();
            assert_eq!(response["type"], "response.create");
        });

        let manager = RealtimeConversationManager::default();
        let connection = manager
            .start(RealtimeConnectionConfig {
                api_key: "test-key".into(),
                base_url: format!("ws://{address}/v1"),
                model: "gpt-realtime".into(),
                params: ConversationStartParams::default(),
            })
            .await
            .unwrap();
        assert_eq!(connection.provider_session_id.as_deref(), Some("sess-test"));
        manager
            .send_text(ConversationTextParams {
                text: "hello".into(),
                role: agent_protocol::ConversationTextRole::User,
            })
            .await
            .unwrap();
        server.await.unwrap();
        manager.close().await;
        drop(connection.events);
    }

    #[tokio::test]
    async fn manager_surfaces_provider_handshake_errors() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut websocket = accept_async(stream).await.unwrap();
            websocket
                .send(Message::Text(
                    json!({
                        "type": "error",
                        "error": { "message": "invalid realtime credentials" }
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
        });

        let manager = RealtimeConversationManager::default();
        let result = manager
            .start(RealtimeConnectionConfig {
                api_key: "bad-key".into(),
                base_url: format!("ws://{address}/v1"),
                model: "gpt-realtime".into(),
                params: ConversationStartParams::default(),
            })
            .await;
        let error = match result {
            Ok(_) => panic!("provider handshake unexpectedly succeeded"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("invalid realtime credentials"));
        server.await.unwrap();
    }
}
