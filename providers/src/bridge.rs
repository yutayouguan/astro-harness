//! 新旧类型桥接层 — 渐进式迁移期间使用。
//!
//! 将旧 `ChatMessage` / `ChatChunk` 与新 `Message` / `StreamChunk` 互转。

use crate::types::message::*;
use crate::types::stream::StreamChunk;

/// 旧 `ChatMessage` → 新 `Message`。
pub fn legacy_to_message(old: &crate::trait_::ChatMessage) -> Message {
    match old.role.as_str() {
        "system" => Message::system(&old.content),
        "tool" => Message::Tool {
            tool_call_id: old.tool_call_id.clone().unwrap_or_default(),
            content: old.content.clone(),
            is_error: old.is_error,
        },
        "assistant" => {
            let mut content = Vec::new();
            if let (Some(reasoning), sig) = (&old.reasoning, &old.thought_signature) {
                content.push(AssistantContent::Thinking {
                    text: reasoning.clone(),
                    signature: sig.clone(),
                });
            } else if let Some(sig) = &old.thought_signature {
                content.push(AssistantContent::Thinking {
                    text: String::new(),
                    signature: Some(sig.clone()),
                });
            }
            if !old.content.is_empty() {
                content.push(AssistantContent::Text {
                    text: old.content.clone(),
                });
            }
            if let Some(ref calls) = old.tool_calls {
                for c in calls {
                    content.push(AssistantContent::ToolCall(ToolCall {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        arguments: c.arguments.clone(),
                        signature: c.signature.clone(),
                    }));
                }
            }
            if content.is_empty() {
                content.push(AssistantContent::Text {
                    text: String::new(),
                });
            }
            Message::Assistant { content }
        }
        _ => {
            // user or unknown → user
            let mut parts = Vec::new();
            if let Some(ref old_parts) = old.parts {
                for p in old_parts {
                    match p {
                        crate::trait_::ChatContentPart::Text { text } => {
                            parts.push(UserContent::Text { text: text.clone() });
                        }
                        crate::trait_::ChatContentPart::ImageUrl { url } => {
                            parts.push(UserContent::Image { url: url.clone() });
                        }
                        crate::trait_::ChatContentPart::AudioUrl { url, mime_type } => {
                            parts.push(UserContent::Audio {
                                url: url.clone(),
                                mime_type: mime_type.clone(),
                            });
                        }
                        crate::trait_::ChatContentPart::VideoUrl { url, mime_type } => {
                            parts.push(UserContent::Video {
                                url: url.clone(),
                                mime_type: mime_type.clone(),
                            });
                        }
                        crate::trait_::ChatContentPart::DocumentUrl { url, mime_type } => {
                            parts.push(UserContent::Document {
                                url: url.clone(),
                                mime_type: mime_type.clone(),
                            });
                        }
                    }
                }
            }
            if parts.is_empty() {
                parts.push(UserContent::Text {
                    text: old.content.clone(),
                });
            }
            Message::User { content: parts }
        }
    }
}

/// 新 `Message` → 旧 `ChatMessage`。
pub fn message_to_legacy(msg: &Message) -> crate::trait_::ChatMessage {
    match msg {
        Message::System { content } => crate::trait_::ChatMessage::text("system", content),
        Message::Tool {
            tool_call_id,
            content,
            is_error,
        } => {
            let mut m = crate::trait_::ChatMessage::text("tool", content);
            m.tool_call_id = Some(tool_call_id.clone());
            m.is_error = *is_error;
            m
        }
        Message::User { content } => {
            let text = content
                .iter()
                .find_map(|c| match c {
                    UserContent::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            let parts: Vec<crate::trait_::ChatContentPart> = content
                .iter()
                .filter_map(|c| match c {
                    UserContent::Text { text } => {
                        Some(crate::trait_::ChatContentPart::Text { text: text.clone() })
                    }
                    UserContent::Image { url } => {
                        Some(crate::trait_::ChatContentPart::ImageUrl { url: url.clone() })
                    }
                    UserContent::Audio { url, mime_type } => {
                        Some(crate::trait_::ChatContentPart::AudioUrl {
                            url: url.clone(),
                            mime_type: mime_type.clone(),
                        })
                    }
                    UserContent::Video { url, mime_type } => {
                        Some(crate::trait_::ChatContentPart::VideoUrl {
                            url: url.clone(),
                            mime_type: mime_type.clone(),
                        })
                    }
                    UserContent::Document { url, mime_type } => {
                        Some(crate::trait_::ChatContentPart::DocumentUrl {
                            url: url.clone(),
                            mime_type: mime_type.clone(),
                        })
                    }
                    _ => None,
                })
                .collect();
            if parts.len() <= 1 && parts.iter().all(|p| matches!(p, crate::trait_::ChatContentPart::Text { .. })) {
                crate::trait_::ChatMessage::text("user", &text)
            } else {
                crate::trait_::ChatMessage::user_parts(&text, parts)
            }
        }
        Message::Assistant { content } => {
            let mut m = crate::trait_::ChatMessage::text("assistant", "");
            let mut texts = Vec::new();
            let mut tool_calls = Vec::new();
            for c in content {
                match c {
                    AssistantContent::Text { text } => texts.push(text.clone()),
                    AssistantContent::ToolCall(tc) => {
                        tool_calls.push(crate::trait_::ChatToolCall {
                            id: tc.id.clone(),
                            name: tc.name.clone(),
                            arguments: tc.arguments.clone(),
                            signature: tc.signature.clone(),
                        });
                    }
                    AssistantContent::Thinking { text, signature } => {
                        m.reasoning = Some(text.clone());
                        m.thought_signature = signature.clone();
                    }
                }
            }
            m.content = texts.join("");
            if !tool_calls.is_empty() {
                m.tool_calls = Some(tool_calls);
            }
            m
        }
    }
}

/// 新 `StreamChunk` → 旧 `ChatChunk`。
pub fn stream_chunk_to_legacy(chunk: &StreamChunk) -> Option<crate::trait_::ChatChunk> {
    match chunk {
        StreamChunk::Text(t) => Some(crate::trait_::ChatChunk {
            token: Some(t.clone()),
            ..Default::default()
        }),
        StreamChunk::Thinking(t) => Some(crate::trait_::ChatChunk {
            reasoning: Some(t.clone()),
            ..Default::default()
        }),
        StreamChunk::ThoughtSignature(s) => Some(crate::trait_::ChatChunk {
            thought_signature: Some(s.clone()),
            ..Default::default()
        }),
        StreamChunk::ToolCallStart { index, id, name } => Some(crate::trait_::ChatChunk {
            tool_call_deltas: vec![crate::trait_::ToolCallDeltaChunk {
                index: *index,
                id: Some(id.clone()),
                name: Some(name.clone()),
                arguments: None,
                signature: None,
            }],
            ..Default::default()
        }),
        StreamChunk::ToolCallDelta { index, arguments } => Some(crate::trait_::ChatChunk {
            tool_call_deltas: vec![crate::trait_::ToolCallDeltaChunk {
                index: *index,
                id: None,
                name: None,
                arguments: Some(arguments.clone()),
                signature: None,
            }],
            ..Default::default()
        }),
        StreamChunk::Usage(u) => Some(crate::trait_::ChatChunk {
            usage: Some(crate::streaming::Usage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                cache_read_tokens: u.cache_read_tokens,
                cache_write_tokens: u.cache_write_tokens,
                reasoning_tokens: u.reasoning_tokens,
                request_count: u.request_count,
            }),
            ..Default::default()
        }),
        StreamChunk::Citation(v) => Some(crate::trait_::ChatChunk {
            citations: Some(vec![v.clone()]),
            ..Default::default()
        }),
        StreamChunk::Done { finish_reason } => Some(crate::trait_::ChatChunk {
            finish_reason: Some(finish_reason.clone()),
            ..Default::default()
        }),
        StreamChunk::Error(msg) => Some(crate::trait_::ChatChunk {
            finish_reason: Some(format!("error:{msg}")),
            ..Default::default()
        }),
        StreamChunk::InteractionId(id) => Some(crate::trait_::ChatChunk {
            interaction_id: Some(id.clone()),
            ..Default::default()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_system() {
        let old = crate::trait_::ChatMessage::text("system", "Be helpful");
        let new = legacy_to_message(&old);
        assert_eq!(new.role(), Role::System);
        let back = message_to_legacy(&new);
        assert_eq!(back.role, "system");
        assert_eq!(back.content, "Be helpful");
    }

    #[test]
    fn roundtrip_user_text() {
        let old = crate::trait_::ChatMessage::text("user", "Hello");
        let new = legacy_to_message(&old);
        assert_eq!(new.role(), Role::User);
        assert_eq!(new.text_content(), "Hello");
        let back = message_to_legacy(&new);
        assert_eq!(back.content, "Hello");
    }

    #[test]
    fn roundtrip_assistant_with_thinking() {
        let mut old = crate::trait_::ChatMessage::text("assistant", "42");
        old.reasoning = Some("let me think".into());
        old.thought_signature = Some("sig123".into());
        let new = legacy_to_message(&old);
        if let Message::Assistant { content } = &new {
            assert!(matches!(&content[0], AssistantContent::Thinking { .. }));
            assert!(matches!(&content[1], AssistantContent::Text { text } if text == "42"));
        } else {
            panic!("expected Assistant");
        }
        let back = message_to_legacy(&new);
        assert_eq!(back.reasoning.as_deref(), Some("let me think"));
        assert_eq!(back.thought_signature.as_deref(), Some("sig123"));
    }

    #[test]
    fn stream_chunk_text_roundtrip() {
        let chunk = StreamChunk::Text("hello".into());
        let legacy = stream_chunk_to_legacy(&chunk).unwrap();
        assert_eq!(legacy.token.as_deref(), Some("hello"));
    }
}
