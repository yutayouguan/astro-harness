use agent_protocol::{
    CodexResponseHandoffMode, ConversationStartParams, ConversationTextParams,
    ConversationTextRole, RealtimeAudioFrame, RealtimeConversationVersion, RealtimeNoiseReduction,
    RealtimeOutputModality, RealtimeTurnDetection,
};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use serde_json::{json, Value};

pub const BACKGROUND_AGENT_TOOL: &str = "background_agent";
pub const REMAIN_SILENT_TOOL: &str = "remain_silent";

const BACKGROUND_AGENT_TOOL_DESCRIPTION: &str = "Send a user request to the background agent. Use this as the default action. Do not rephrase the user's ask or rewrite it in your own words; pass along the user's own words. If the background agent is idle, this starts a new task and returns the final result to the user. If the background agent is already working on a task, this sends the request as guidance to steer that previous task.";
const REMAIN_SILENT_TOOL_DESCRIPTION: &str = "Call this when the best response is to say nothing. Use it instead of speaking after hidden system or control messages, after background agent updates, or whenever acknowledging aloud would be distracting. This tool has no user-visible effect.";

pub fn session_config(model: &str, params: &ConversationStartParams) -> Value {
    match params.version {
        RealtimeConversationVersion::V2 => v2_session(model, params),
        RealtimeConversationVersion::V3 => v3_session(model, params),
    }
}

pub fn session_update(model: &str, params: &ConversationStartParams) -> Value {
    let session = session_config(model, params);
    match params.version {
        RealtimeConversationVersion::V2 => json!({"type": "session.update", "session": session}),
        RealtimeConversationVersion::V3 => json!({"type": "session.update", "session": session}),
    }
}

fn v2_session(model: &str, params: &ConversationStartParams) -> Value {
    let modalities = match params.output_modality {
        RealtimeOutputModality::Text => vec!["text"],
        RealtimeOutputModality::Audio => vec!["audio"],
    };
    let turn_detection = match params.turn_detection {
        RealtimeTurnDetection::Disabled => Value::Null,
        RealtimeTurnDetection::ServerVad => json!({
            "type": "server_vad", "create_response": true,
            "interrupt_response": true, "silence_duration_ms": 500
        }),
        RealtimeTurnDetection::SemanticVad => json!({
            "type": "semantic_vad", "create_response": true, "interrupt_response": true
        }),
    };
    let noise_reduction = params.noise_reduction.map(|mode| match mode {
        RealtimeNoiseReduction::NearField => json!({"type": "near_field"}),
        RealtimeNoiseReduction::FarField => json!({"type": "far_field"}),
    });
    let tools = if params.client_managed_handoffs {
        Vec::new()
    } else {
        vec![
            json!({
                "type": "function", "name": BACKGROUND_AGENT_TOOL,
                "description": BACKGROUND_AGENT_TOOL_DESCRIPTION,
                "parameters": {"type": "object", "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "The user's request to delegate to the background agent."
                    }
                }, "required": ["prompt"], "additionalProperties": false}
            }),
            json!({
                "type": "function", "name": REMAIN_SILENT_TOOL,
                "description": REMAIN_SILENT_TOOL_DESCRIPTION,
                "parameters": {"type": "object", "properties": {}, "additionalProperties": false}
            }),
        ]
    };
    json!({
        "type": "realtime", "model": model, "instructions": params.instructions,
        "output_modalities": modalities,
        "audio": {
            "input": {
                "format": {"type": "audio/pcm", "rate": 24_000},
                "noise_reduction": noise_reduction,
                "turn_detection": turn_detection
            },
            "output": {
                "format": {"type": "audio/pcm", "rate": 24_000},
                "voice": params.voice.as_deref().unwrap_or("marin")
            }
        },
        "tools": tools,
        "tool_choice": if tools.is_empty() { Value::Null } else { json!("auto") }
    })
}

fn v3_session(model: &str, params: &ConversationStartParams) -> Value {
    let initial_items = params
        .initial_items
        .iter()
        .map(message_item)
        .collect::<Vec<_>>();
    json!({
        "model": model,
        "instructions": params.instructions,
        "audio": {"output": {"voice": params.voice.as_deref().unwrap_or("cove")}},
        "delegation": {"type": "client"},
        "initial_items": initial_items
    })
}

pub fn audio_append(version: RealtimeConversationVersion, frame: &RealtimeAudioFrame) -> Value {
    json!({
        "type": match version {
            RealtimeConversationVersion::V2 => "input_audio_buffer.append",
            RealtimeConversationVersion::V3 => "input_audio.append",
        },
        "audio": BASE64_STANDARD.encode(&frame.data)
    })
}

pub fn text_events(
    version: RealtimeConversationVersion,
    params: &ConversationTextParams,
) -> Vec<Value> {
    match version {
        RealtimeConversationVersion::V2 => vec![
            json!({"type": "conversation.item.create", "item": message_item(params)}),
            json!({"type": "response.create"}),
        ],
        RealtimeConversationVersion::V3 => context_append_chunks(&params.text)
            .into_iter()
            .map(|chunk| context_append(None, &chunk, None))
            .collect(),
    }
}

pub fn speech_event(version: RealtimeConversationVersion, text: &str) -> Value {
    match version {
        RealtimeConversationVersion::V2 => json!({
            "type": "response.create",
            "response": {"instructions": text, "output_modalities": ["audio"]}
        }),
        RealtimeConversationVersion::V3 => context_append(None, text, Some("speakable")),
    }
}

pub fn context_append_chunks(text: &str) -> Vec<String> {
    const MAX_BYTES: usize = 500;
    if text.len() <= MAX_BYTES {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + MAX_BYTES).min(text.len());
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
        chunks.push(text[start..end].to_string());
        start = end;
    }
    chunks
}

pub fn handoff_append(
    version: RealtimeConversationVersion,
    handoff_id: &str,
    text: &str,
    mode: CodexResponseHandoffMode,
    final_answer: bool,
) -> Value {
    match version {
        RealtimeConversationVersion::V2 => json!({
            "type": "conversation.item.create",
            "item": {
                "type": "message", "role": "user",
                "content": [{"type": "input_text", "text": format!("[BACKEND] {text}")}]
            }
        }),
        RealtimeConversationVersion::V3 => context_append(
            Some(handoff_id),
            text,
            match mode {
                CodexResponseHandoffMode::Thinking => None,
                CodexResponseHandoffMode::Commentary => Some("commentary"),
                CodexResponseHandoffMode::BemTags if final_answer => Some("speakable"),
                CodexResponseHandoffMode::BemTags => Some("commentary"),
            },
        ),
    }
}

pub fn handoff_complete(version: RealtimeConversationVersion, handoff_id: &str) -> Value {
    match version {
        RealtimeConversationVersion::V2 => json!({
            "type": "conversation.item.create",
            "item": {
                "type": "function_call_output", "call_id": handoff_id,
                "output": "Background agent finished. Use the preceding [BACKEND] messages as the result."
            }
        }),
        RealtimeConversationVersion::V3 => {
            json!({"type": "delegation.complete", "delegation_item_id": handoff_id})
        }
    }
}

pub fn function_call_output(call_id: &str, output: &str) -> Value {
    json!({
        "type": "conversation.item.create",
        "item": {
            "type": "function_call_output",
            "call_id": call_id,
            "output": output
        }
    })
}

fn context_append(handoff_id: Option<&str>, text: &str, channel: Option<&str>) -> Value {
    let mut value = if let Some(handoff_id) = handoff_id {
        json!({
            "type": "delegation.context.append", "delegation_item_id": handoff_id,
            "content": [{"type": "input_text", "text": text}]
        })
    } else {
        json!({"type": "session.context.append", "content": [{"type": "input_text", "text": text}]})
    };
    if let Some(channel) = channel {
        value["channel"] = json!(channel);
    }
    value
}

fn message_item(params: &ConversationTextParams) -> Value {
    let role = match params.role {
        ConversationTextRole::User => "user",
        ConversationTextRole::Developer => "developer",
        ConversationTextRole::Assistant => "assistant",
    };
    let content_type = if role == "assistant" {
        "output_text"
    } else {
        "input_text"
    };
    json!({
        "type": "message", "role": role,
        "content": [{"type": content_type, "text": params.text}]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_session_exposes_handoff_tools() {
        let session = session_config("gpt-realtime", &ConversationStartParams::default());
        assert_eq!(session["tools"][0]["name"], BACKGROUND_AGENT_TOOL);
        assert_eq!(session["tools"][1]["name"], REMAIN_SILENT_TOOL);
        assert!(session["audio"]["input"].get("transcription").is_none());
        assert_eq!(
            session["tools"][0]["parameters"]["required"],
            json!(["prompt"])
        );
    }

    #[test]
    fn v3_session_uses_native_client_delegation_without_sidecar_transcription() {
        let params = ConversationStartParams {
            version: RealtimeConversationVersion::V3,
            ..ConversationStartParams::default()
        };
        let session = session_config("gpt-live-1-codex", &params);
        assert_eq!(session["delegation"]["type"], "client");
        assert!(session.pointer("/audio/input/transcription").is_none());
    }

    #[test]
    fn remain_silent_completes_with_an_empty_function_output() {
        assert_eq!(
            function_call_output("call-1", ""),
            json!({
                "type": "conversation.item.create",
                "item": {
                    "type": "function_call_output",
                    "call_id": "call-1",
                    "output": ""
                }
            })
        );
    }

    #[test]
    fn v3_bem_final_uses_speakable_channel() {
        let event = handoff_append(
            RealtimeConversationVersion::V3,
            "handoff-1",
            "[FINAL] ready",
            CodexResponseHandoffMode::BemTags,
            true,
        );
        assert_eq!(event["channel"], "speakable");
    }

    #[test]
    fn v3_context_chunks_preserve_utf8_and_byte_limit() {
        let text = "你好".repeat(260);
        let chunks = context_append_chunks(&text);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|chunk| chunk.len() <= 500));
        assert_eq!(chunks.concat(), text);
    }
}
