use agent_protocol::{
    RealtimeAudioFormat, RealtimeAudioFrame, RealtimeConversationVersion, RealtimeEvent,
    RealtimeHandoffRequested, RealtimeTranscriptDelta, RealtimeTranscriptDone,
};
use anyhow::{Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use serde_json::{Map, Value};

const HANDOFF_TOOL: &str = "background_agent";
const NOOP_TOOL: &str = "remain_silent";
const TOOL_ARGUMENT_KEYS: [&str; 5] = ["input_transcript", "input", "text", "prompt", "query"];

pub fn parse_realtime_event(
    version: RealtimeConversationVersion,
    payload: &str,
) -> Result<Option<RealtimeEvent>> {
    let value: Value = serde_json::from_str(payload).context("invalid realtime event JSON")?;
    Ok(match version {
        RealtimeConversationVersion::V2 => parse_v2(&value)?,
        RealtimeConversationVersion::V3 => parse_v3(&value)?,
    })
}

fn parse_v2(value: &Value) -> Result<Option<RealtimeEvent>> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Ok(match kind {
        "session.created" | "session.updated" => Some(session_updated(value)),
        "response.output_audio.delta" | "response.audio.delta" => audio(value, "delta")?,
        "conversation.item.input_audio_transcription.delta" => {
            transcript_delta(value, "delta").map(RealtimeEvent::InputTranscriptDelta)
        }
        "conversation.item.input_audio_transcription.completed" => {
            transcript_done(value, "transcript").map(RealtimeEvent::InputTranscriptDone)
        }
        "response.output_text.delta" | "response.output_audio_transcript.delta" => {
            transcript_delta(value, "delta").map(RealtimeEvent::OutputTranscriptDelta)
        }
        "response.output_text.done" => {
            transcript_done(value, "text").map(RealtimeEvent::OutputTranscriptDone)
        }
        "response.output_audio_transcript.done" => {
            transcript_done(value, "transcript").map(RealtimeEvent::OutputTranscriptDone)
        }
        "input_audio_buffer.speech_started" => Some(RealtimeEvent::InputAudioSpeechStarted {
            item_id: string(value, "item_id"),
        }),
        "conversation.item.added" | "conversation.item.created" => value
            .get("item")
            .cloned()
            .map(RealtimeEvent::ConversationItemAdded),
        "conversation.item.done" => value
            .get("item")
            .and_then(Value::as_object)
            .and_then(parse_done_item),
        "response.created" => Some(RealtimeEvent::ResponseCreated {
            response_id: response_id(value),
        }),
        "response.cancelled" => Some(RealtimeEvent::ResponseCancelled {
            response_id: response_id(value),
        }),
        "response.done" => Some(RealtimeEvent::ResponseDone {
            response_id: response_id(value),
        }),
        "error" => Some(RealtimeEvent::Error(error_message(value))),
        _ => None,
    })
}

fn parse_v3(value: &Value) -> Result<Option<RealtimeEvent>> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Ok(match kind {
        "session.started" | "session.updated" => Some(session_updated(value)),
        "output_audio.delta" => audio(value, "audio")?,
        "input_transcript.added" => transcript_item(value)
            .map(|delta| RealtimeEvent::InputTranscriptDelta(RealtimeTranscriptDelta { delta })),
        "output_transcript.added" => transcript_item(value)
            .map(|delta| RealtimeEvent::OutputTranscriptDelta(RealtimeTranscriptDelta { delta })),
        "turn.done" => parse_turn_done(value),
        "delegation.created" => parse_delegation(value),
        "error" => Some(RealtimeEvent::Error(error_message(value))),
        _ => None,
    })
}

fn session_updated(value: &Value) -> RealtimeEvent {
    RealtimeEvent::SessionUpdated {
        realtime_session_id: value
            .pointer("/session/id")
            .or_else(|| value.get("session_id"))
            .and_then(Value::as_str)
            .map(str::to_string),
        instructions: value
            .pointer("/session/instructions")
            .or_else(|| value.get("instructions"))
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

fn audio(value: &Value, field: &str) -> Result<Option<RealtimeEvent>> {
    let Some(encoded) = value.get(field).and_then(Value::as_str) else {
        return Ok(None);
    };
    let data = BASE64_STANDARD
        .decode(encoded)
        .context("invalid realtime audio base64")?;
    Ok(Some(RealtimeEvent::AudioOut(RealtimeAudioFrame {
        data,
        sample_rate: value
            .get("sample_rate")
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(agent_protocol::DEFAULT_REALTIME_SAMPLE_RATE),
        num_channels: value
            .get("channels")
            .or_else(|| value.get("num_channels"))
            .and_then(Value::as_u64)
            .and_then(|value| u16::try_from(value).ok())
            .unwrap_or(1),
        format: RealtimeAudioFormat::Pcm16,
    })))
}

fn transcript_delta(value: &Value, field: &str) -> Option<RealtimeTranscriptDelta> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(|delta| RealtimeTranscriptDelta {
            delta: delta.into(),
        })
}

fn transcript_done(value: &Value, field: &str) -> Option<RealtimeTranscriptDone> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(|text| RealtimeTranscriptDone { text: text.into() })
}

fn transcript_item(value: &Value) -> Option<String> {
    value
        .pointer("/item/text")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn parse_turn_done(value: &Value) -> Option<RealtimeEvent> {
    let role = value.pointer("/turn/role")?.as_str()?;
    let text = value.pointer("/turn/transcript")?.as_str()?.to_string();
    let done = RealtimeTranscriptDone { text };
    match role {
        "user" => Some(RealtimeEvent::InputTranscriptDone(done)),
        "assistant" => Some(RealtimeEvent::OutputTranscriptDone(done)),
        _ => None,
    }
}

fn parse_delegation(value: &Value) -> Option<RealtimeEvent> {
    let item = value.get("item")?.as_object()?;
    if item.get("type").and_then(Value::as_str) != Some("delegation")
        || item.get("target").and_then(Value::as_str) != Some("client")
    {
        return None;
    }
    let item_id = item.get("id")?.as_str()?.to_string();
    let input_transcript = item
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|content| content.get("type").and_then(Value::as_str) == Some("input_text"))
        .filter_map(|content| content.get("text").and_then(Value::as_str))
        .collect::<String>();
    Some(RealtimeEvent::HandoffRequested(RealtimeHandoffRequested {
        handoff_id: item_id.clone(),
        item_id,
        input_transcript,
        active_transcript: Vec::new(),
    }))
}

fn parse_done_item(item: &Map<String, Value>) -> Option<RealtimeEvent> {
    if item.get("type").and_then(Value::as_str) == Some("function_call") {
        let name = item.get("name").and_then(Value::as_str);
        let call_id = item
            .get("call_id")
            .or_else(|| item.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let item_id = item
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or(&call_id)
            .to_string();
        if name == Some(HANDOFF_TOOL) {
            let arguments = item
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or_default();
            return Some(RealtimeEvent::HandoffRequested(RealtimeHandoffRequested {
                handoff_id: call_id,
                item_id,
                input_transcript: extract_handoff_input(arguments),
                active_transcript: Vec::new(),
            }));
        }
        if name == Some(NOOP_TOOL) {
            return Some(RealtimeEvent::NoopRequested { call_id, item_id });
        }
    }
    item.get("id")
        .and_then(Value::as_str)
        .map(|item_id| RealtimeEvent::ConversationItemDone {
            item_id: item_id.into(),
        })
}

fn extract_handoff_input(arguments: &str) -> String {
    if let Ok(value) = serde_json::from_str::<Value>(arguments) {
        for key in TOOL_ARGUMENT_KEYS {
            if let Some(text) = value.get(key).and_then(Value::as_str) {
                if !text.trim().is_empty() {
                    return text.trim().to_string();
                }
            }
        }
    }
    arguments.to_string()
}

fn response_id(value: &Value) -> String {
    value
        .pointer("/response/id")
        .or_else(|| value.get("response_id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn string(value: &Value, field: &str) -> String {
    value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn error_message(value: &Value) -> String {
    value
        .pointer("/error/message")
        .or_else(|| value.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("realtime provider error")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_function_call_becomes_typed_handoff() {
        let payload = serde_json::json!({
            "type": "conversation.item.done",
            "item": {
                "id": "item-1", "call_id": "call-1", "type": "function_call",
                "name": "background_agent", "arguments": "{\"query\":\"inspect repo\"}"
            }
        });
        let event = parse_realtime_event(RealtimeConversationVersion::V2, &payload.to_string())
            .unwrap()
            .unwrap();
        assert!(
            matches!(event, RealtimeEvent::HandoffRequested(request) if request.input_transcript == "inspect repo")
        );
    }

    #[test]
    fn v3_turn_done_is_typed_transcript() {
        let payload = serde_json::json!({
            "type": "turn.done",
            "turn": {"role": "assistant", "transcript": "done"}
        });
        assert_eq!(
            parse_realtime_event(RealtimeConversationVersion::V3, &payload.to_string()).unwrap(),
            Some(RealtimeEvent::OutputTranscriptDone(
                RealtimeTranscriptDone {
                    text: "done".into()
                }
            ))
        );
    }
}
