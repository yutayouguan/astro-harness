use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DEFAULT_REALTIME_MODEL: &str = "gpt-realtime";
pub const DEFAULT_REALTIME_SAMPLE_RATE: u32 = 24_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RealtimeConversationVersion {
    #[default]
    V2,
    V3,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ConversationStartTransport {
    #[default]
    Websocket,
    Webrtc {
        sdp: String,
    },
    ExistingCall {
        call_id: String,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexResponseHandoffMode {
    #[default]
    Thinking,
    Commentary,
    BemTags,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RealtimeOutputModality {
    Text,
    #[default]
    Audio,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationTextRole {
    #[default]
    User,
    Developer,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationTextParams {
    pub text: String,
    #[serde(default)]
    pub role: ConversationTextRole,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RealtimeTurnDetection {
    Disabled,
    #[default]
    ServerVad,
    SemanticVad,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RealtimeNoiseReduction {
    NearField,
    FarField,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversationStartParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub output_modality: RealtimeOutputModality,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default = "default_include_startup_context")]
    pub include_startup_context: bool,
    #[serde(default)]
    pub initial_items: Vec<ConversationTextParams>,
    #[serde(default)]
    pub turn_detection: RealtimeTurnDetection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noise_reduction: Option<RealtimeNoiseReduction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_audio_transcription_model: Option<String>,
    #[serde(default)]
    pub transport: ConversationStartTransport,
    #[serde(default)]
    pub version: RealtimeConversationVersion,
    #[serde(default)]
    pub client_managed_handoffs: bool,
    #[serde(default)]
    pub codex_response_handoff_mode: CodexResponseHandoffMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_response_handoff_channel_prefixes: Option<BTreeMap<String, Vec<String>>>,
    #[serde(default)]
    pub flush_transcript_tail_on_session_end: bool,
}

const fn default_include_startup_context() -> bool {
    true
}

impl Default for ConversationStartParams {
    fn default() -> Self {
        Self {
            model: None,
            output_modality: RealtimeOutputModality::Audio,
            voice: None,
            instructions: None,
            include_startup_context: true,
            initial_items: Vec::new(),
            turn_detection: RealtimeTurnDetection::ServerVad,
            noise_reduction: None,
            input_audio_transcription_model: Some("gpt-4o-mini-transcribe".into()),
            transport: ConversationStartTransport::Websocket,
            version: RealtimeConversationVersion::V2,
            client_managed_handoffs: false,
            codex_response_handoff_mode: CodexResponseHandoffMode::Thinking,
            codex_response_handoff_channel_prefixes: None,
            flush_transcript_tail_on_session_end: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RealtimeAudioFormat {
    #[default]
    Pcm16,
}

impl RealtimeAudioFormat {
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::Pcm16 => "pcm16",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealtimeAudioFrame {
    pub data: Vec<u8>,
    #[serde(default = "default_sample_rate")]
    pub sample_rate: u32,
    #[serde(default = "default_num_channels")]
    pub num_channels: u16,
    #[serde(default)]
    pub format: RealtimeAudioFormat,
}

fn default_sample_rate() -> u32 {
    DEFAULT_REALTIME_SAMPLE_RATE
}

fn default_num_channels() -> u16 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationAudioParams {
    pub frame: RealtimeAudioFrame,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationSpeechParams {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealtimeTranscriptDelta {
    pub delta: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealtimeTranscriptDone {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealtimeTranscriptEntry {
    pub role: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealtimeHandoffRequested {
    pub handoff_id: String,
    pub item_id: String,
    #[serde(default)]
    pub input_transcript: String,
    #[serde(default)]
    pub active_transcript: Vec<RealtimeTranscriptEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum RealtimeEvent {
    SessionUpdated {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        realtime_session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        instructions: Option<String>,
    },
    InputAudioSpeechStarted {
        #[serde(default)]
        item_id: String,
    },
    InputTranscriptDelta(RealtimeTranscriptDelta),
    InputTranscriptDone(RealtimeTranscriptDone),
    OutputTranscriptDelta(RealtimeTranscriptDelta),
    OutputTranscriptDone(RealtimeTranscriptDone),
    AudioOut(RealtimeAudioFrame),
    ResponseCreated {
        #[serde(default)]
        response_id: String,
    },
    ResponseCancelled {
        #[serde(default)]
        response_id: String,
    },
    ResponseDone {
        #[serde(default)]
        response_id: String,
    },
    ConversationItemAdded(Value),
    ConversationItemDone {
        #[serde(default)]
        item_id: String,
    },
    HandoffRequested(RealtimeHandoffRequested),
    NoopRequested {
        #[serde(default)]
        call_id: String,
        #[serde(default)]
        item_id: String,
    },
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealtimeConversationStartedEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub realtime_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    pub model: String,
    pub version: RealtimeConversationVersion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealtimeConversationSdpEvent {
    pub sdp: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeConversationRealtimeEvent {
    pub payload: RealtimeEvent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealtimeConversationClosedEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealtimeVoicesList {
    pub voices: Vec<String>,
    pub default_voice: String,
}

impl RealtimeVoicesList {
    pub fn builtin(version: RealtimeConversationVersion) -> Self {
        let (voices, default_voice) = match version {
            RealtimeConversationVersion::V2 => (
                vec![
                    "alloy", "ash", "ballad", "cedar", "coral", "echo", "marin", "sage", "shimmer",
                    "verse",
                ],
                "marin",
            ),
            RealtimeConversationVersion::V3 => (
                vec![
                    "juniper", "maple", "spruce", "ember", "vale", "breeze", "arbor", "sol", "cove",
                ],
                "cove",
            ),
        };
        Self {
            voices: voices.into_iter().map(str::to_string).collect(),
            default_voice: default_voice.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealtimeConversationListVoicesResponseEvent {
    pub voices: RealtimeVoicesList,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RealtimeTranscriptRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BemItemPresentation {
    WholeItem,
    InlineMarkdown,
    InlineVisualization { index: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RealtimeSessionOutcome {
    Ended,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum RealtimeItemContent {
    RealtimeSessionStarted,
    TranscriptSegment {
        role: RealtimeTranscriptRole,
        text: String,
    },
    BemItemPromoted {
        turn_id: String,
        item_id: String,
        presentation: BemItemPresentation,
    },
    RealtimeSessionClosed {
        outcome: RealtimeSessionOutcome,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealtimeItem {
    pub id: String,
    pub realtime_session_id: String,
    #[serde(flatten)]
    pub content: RealtimeItemContent,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_params_default_to_public_v2_websocket() {
        let params = ConversationStartParams::default();
        assert_eq!(params.output_modality, RealtimeOutputModality::Audio);
        assert_eq!(params.turn_detection, RealtimeTurnDetection::ServerVad);
        assert_eq!(params.version, RealtimeConversationVersion::V2);
        assert_eq!(params.transport, ConversationStartTransport::Websocket);
        assert!(params.include_startup_context);
    }

    #[test]
    fn omitted_startup_context_preserves_the_public_default() {
        let params: ConversationStartParams = serde_json::from_str("{}").unwrap();
        assert!(params.include_startup_context);
    }

    #[test]
    fn typed_event_roundtrips() {
        let event = RealtimeEvent::HandoffRequested(RealtimeHandoffRequested {
            handoff_id: "handoff-1".into(),
            item_id: "item-1".into(),
            input_transcript: "inspect this".into(),
            active_transcript: Vec::new(),
        });
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(serde_json::from_str::<RealtimeEvent>(&json).unwrap(), event);
    }

    #[test]
    fn pcm_frame_roundtrips_without_losing_binary_data() {
        let frame = RealtimeAudioFrame {
            data: vec![0, 1, 254, 255],
            sample_rate: 24_000,
            num_channels: 1,
            format: RealtimeAudioFormat::Pcm16,
        };
        let json = serde_json::to_string(&frame).unwrap();
        let restored: RealtimeAudioFrame = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, frame);
    }
}
