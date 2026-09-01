use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DEFAULT_REALTIME_MODEL: &str = "gpt-realtime";
pub const DEFAULT_REALTIME_SAMPLE_RATE: u32 = 24_000;

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
    System,
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
    /// Overrides the active chat target model for this realtime session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default)]
    pub output_modality: RealtimeOutputModality,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default)]
    pub include_startup_context: bool,
    #[serde(default)]
    pub initial_items: Vec<ConversationTextParams>,
    #[serde(default)]
    pub turn_detection: RealtimeTurnDetection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub noise_reduction: Option<RealtimeNoiseReduction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_audio_transcription_model: Option<String>,
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
pub struct RealtimeConversationStartedEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub realtime_session_id: Option<String>,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RealtimeConversationRealtimeEvent {
    pub payload: Value,
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
    pub fn builtin() -> Self {
        Self {
            voices: [
                "alloy", "ash", "ballad", "cedar", "coral", "echo", "marin", "sage", "shimmer",
                "verse",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            default_voice: "marin".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RealtimeConversationListVoicesResponseEvent {
    pub voices: RealtimeVoicesList,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_params_default_to_audio_with_server_vad() {
        let params = ConversationStartParams::default();
        assert_eq!(params.output_modality, RealtimeOutputModality::Audio);
        assert_eq!(params.turn_detection, RealtimeTurnDetection::ServerVad);
        assert!(params.include_startup_context);
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
