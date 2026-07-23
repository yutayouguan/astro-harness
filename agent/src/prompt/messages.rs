//! 会话消息 → Provider 消息转换。
//!
//! 将应用内 `common::message::Message` 序列转为各 LLM Provider 统一的消息格式。
//! 支持旧 `ChatMessage` 和新 `providers::types::Message` 两种输出。

use common::message::{Message, MessageContent, Role};
use providers::trait_::{ChatContentPart, ChatMessage as ProviderMessage, ChatToolCall};
use providers::types::message as new_msg;

/// 将会话历史与 system prompt 转为 Provider 可消费的聊天消息列表。
///
/// 首条固定为 `system` 角色；tool 消息会从历史中反向查找对应 `tool_call_id` 以填充 `name`。
/// 发送前会 [`sanitize_tool_pairs`](super::sanitize::sanitize_tool_pairs)：去掉悬挂
/// `tool_calls` 与孤儿 tool 消息，避免上游 400。
pub fn to_provider_messages(system_prompt: &str, session: &[Message]) -> Vec<ProviderMessage> {
    let session = super::sanitize::sanitized_tool_pairs(session);
    let mut messages = vec![ProviderMessage::text("system", system_prompt)];

    for message in &session {
        if message.role == Role::Tool {
            let ok = message
                .tool_call_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .is_some();
            if !ok {
                tracing::warn!("skip tool message without tool_call_id");
                continue;
            }
        }

        let role = match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
            Role::Tool => "tool",
        };
        let tool_calls = message.tool_calls.as_ref().map(|calls| {
            calls
                .iter()
                .map(|c| ChatToolCall {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    arguments: c.arguments.clone(),
                    signature: c.signature.clone(),
                })
                .collect()
        });
        let tool_name = if message.role == Role::Tool {
            message.tool_call_id.as_ref().and_then(|id| {
                session.iter().rev().find_map(|m| {
                    m.tool_calls
                        .as_ref()?
                        .iter()
                        .find(|c| c.id == *id)
                        .map(|c| c.name.clone())
                })
            })
        } else {
            None
        };
        let (content, parts) = match &message.content {
            MessageContent::Text(s) => {
                let content = if message.role == Role::Tool {
                    message
                        .compressed_content
                        .as_ref()
                        .filter(|s| !s.trim().is_empty())
                        .cloned()
                        .unwrap_or_else(|| s.clone())
                } else {
                    s.clone()
                };
                (content, None)
            }
            MessageContent::Parts(ps) => {
                let text = message.content_text();
                let parts: Vec<ChatContentPart> =
                    ps.iter().filter_map(content_part_to_chat).collect();
                (text, Some(parts).filter(|v| !v.is_empty()))
            }
        };
        let parts = merge_media_parts(parts, &message.media);
        messages.push(ProviderMessage {
            role: role.to_string(),
            content,
            parts,
            tool_calls,
            tool_call_id: message.tool_call_id.clone(),
            name: tool_name,
            reasoning: message.reasoning.clone(),
            thought_signature: message.thought_signature.clone(),
            is_error: message.role == Role::Tool
                && message
                    .content_text()
                    .starts_with("Error"),
        });
    }

    messages
}

fn content_part_to_chat(p: &common::message::ContentPart) -> Option<ChatContentPart> {
    match p.kind.as_str() {
        "text" => Some(ChatContentPart::Text {
            text: p.text.clone().unwrap_or_default(),
        }),
        "image_url" => p
            .image_url
            .as_ref()
            .map(|u| ChatContentPart::ImageUrl { url: u.url.clone() }),
        "audio_url" => p.audio_url.as_ref().map(|u| ChatContentPart::AudioUrl {
            url: u.url.clone(),
            mime_type: u.mime_type.clone(),
        }),
        "video_url" => p.video_url.as_ref().map(|u| ChatContentPart::VideoUrl {
            url: u.url.clone(),
            mime_type: u.mime_type.clone(),
        }),
        _ => None,
    }
}

/// 将 `Message.media` 中可入模的 data/remote URI 并入 parts（workspace 路径跳过）。
fn merge_media_parts(
    existing: Option<Vec<ChatContentPart>>,
    media: &[common::MediaAsset],
) -> Option<Vec<ChatContentPart>> {
    let mut parts = existing.unwrap_or_default();
    for asset in media {
        if let Some(p) = media_asset_to_part(asset) {
            // 避免与 Parts 里已有同 URL 重复
            let url = match &p {
                ChatContentPart::ImageUrl { url }
                | ChatContentPart::AudioUrl { url, .. }
                | ChatContentPart::VideoUrl { url, .. }
                | ChatContentPart::DocumentUrl { url, .. } => Some(url.as_str()),
                ChatContentPart::Text { .. } => None,
            };
            let dup = url.is_some_and(|u| {
                parts.iter().any(|e| match e {
                    ChatContentPart::ImageUrl { url }
                    | ChatContentPart::AudioUrl { url, .. }
                    | ChatContentPart::VideoUrl { url, .. }
                    | ChatContentPart::DocumentUrl { url, .. } => url == u,
                    _ => false,
                })
            });
            if !dup {
                parts.push(p);
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts)
    }
}

fn media_asset_to_part(asset: &common::MediaAsset) -> Option<ChatContentPart> {
    use common::{MediaKind, MediaRef};
    let (url, mime_from_data) = match &asset.reference {
        MediaRef::DataUrl(u) => {
            let mime = u
                .strip_prefix("data:")
                .and_then(|rest| rest.split(';').next())
                .map(str::to_string);
            (u.clone(), mime)
        }
        MediaRef::RemoteUri(u) => (u.clone(), None),
        MediaRef::WorkspacePath(_) => return None,
    };
    let mime = if asset.mime_type.trim().is_empty() {
        mime_from_data.unwrap_or_default()
    } else {
        asset.mime_type.clone()
    };
    match asset.kind {
        MediaKind::Image => Some(ChatContentPart::ImageUrl { url }),
        MediaKind::Audio => Some(ChatContentPart::AudioUrl {
            url,
            mime_type: mime,
        }),
        MediaKind::Video => Some(ChatContentPart::VideoUrl {
            url,
            mime_type: mime,
        }),
        MediaKind::File if mime.contains("pdf") => Some(ChatContentPart::DocumentUrl {
            url,
            mime_type: mime,
        }),
        MediaKind::File => Some(ChatContentPart::Text {
            text: format!("[file attached: {mime}]"),
        }),
    }
}

/// 将会话历史转为新 `providers::types::Message` 格式。
///
/// 直接从 session 构建新消息类型，不经 bridge。
pub fn to_new_messages(system_prompt: &str, session: &[Message]) -> Vec<new_msg::Message> {
    let old_msgs = to_provider_messages(system_prompt, session);
    old_msgs.iter().map(|old| legacy_to_new_message(old)).collect()
}

/// 旧 `ProviderMessage` → 新 `providers::types::Message`（内联转换，不依赖 bridge）。
fn legacy_to_new_message(old: &ProviderMessage) -> new_msg::Message {
    match old.role.as_str() {
        "system" => new_msg::Message::system(&old.content),
        "tool" => new_msg::Message::Tool {
            tool_call_id: old.tool_call_id.clone().unwrap_or_default(),
            content: old.content.clone(),
            is_error: old.is_error,
        },
        "assistant" => {
            let mut content = Vec::new();
            if let (Some(reasoning), sig) = (&old.reasoning, &old.thought_signature) {
                content.push(new_msg::AssistantContent::Thinking {
                    text: reasoning.clone(),
                    signature: sig.clone(),
                });
            } else if let Some(sig) = &old.thought_signature {
                content.push(new_msg::AssistantContent::Thinking {
                    text: String::new(),
                    signature: Some(sig.clone()),
                });
            }
            if !old.content.is_empty() {
                content.push(new_msg::AssistantContent::Text {
                    text: old.content.clone(),
                });
            }
            if let Some(ref calls) = old.tool_calls {
                for c in calls {
                    content.push(new_msg::AssistantContent::ToolCall(new_msg::ToolCall {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        arguments: c.arguments.clone(),
                        signature: c.signature.clone(),
                    }));
                }
            }
            if content.is_empty() {
                content.push(new_msg::AssistantContent::Text {
                    text: String::new(),
                });
            }
            new_msg::Message::Assistant { content }
        }
        _ => {
            let mut parts = Vec::new();
            if let Some(ref old_parts) = old.parts {
                for p in old_parts {
                    match p {
                        ChatContentPart::Text { text } => {
                            parts.push(new_msg::UserContent::Text { text: text.clone() });
                        }
                        ChatContentPart::ImageUrl { url } => {
                            parts.push(new_msg::UserContent::Image { url: url.clone() });
                        }
                        ChatContentPart::AudioUrl { url, mime_type } => {
                            parts.push(new_msg::UserContent::Audio {
                                url: url.clone(),
                                mime_type: mime_type.clone(),
                            });
                        }
                        ChatContentPart::VideoUrl { url, mime_type } => {
                            parts.push(new_msg::UserContent::Video {
                                url: url.clone(),
                                mime_type: mime_type.clone(),
                            });
                        }
                        ChatContentPart::DocumentUrl { url, mime_type } => {
                            parts.push(new_msg::UserContent::Document {
                                url: url.clone(),
                                mime_type: mime_type.clone(),
                            });
                        }
                    }
                }
            }
            if parts.is_empty() {
                parts.push(new_msg::UserContent::Text {
                    text: old.content.clone(),
                });
            }
            new_msg::Message::User { content: parts }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::message::{Message, MessageContent, ToolCall};
    use common::{MediaAsset, MediaKind, MediaRef};
    use serde_json::json;

    #[test]
    fn multimodal_parts_map_to_provider_parts() {
        let session = vec![Message::user_with_images(
            "描述",
            &["data:image/png;base64,xx".into()],
        )];
        let msgs = to_provider_messages("sys", &session);
        assert_eq!(msgs.len(), 2);
        let user = &msgs[1];
        assert_eq!(user.role, "user");
        let parts = user.parts.as_ref().expect("parts");
        assert!(matches!(
            &parts[0],
            ChatContentPart::Text { text } if text == "描述"
        ));
        assert!(matches!(
            &parts[1],
            ChatContentPart::ImageUrl { url } if url.starts_with("data:image/png")
        ));
    }

    #[test]
    fn audio_video_parts_and_media_field_map() {
        let mut msg = Message::user("听这段并看视频");
        msg.content = MessageContent::Parts(vec![
            common::message::ContentPart::text("听这段并看视频"),
            common::message::ContentPart::audio_url("data:audio/wav;base64,AQID", "audio/wav"),
            common::message::ContentPart::video_url("data:video/mp4;base64,AQID", "video/mp4"),
        ]);
        msg.media.push(MediaAsset {
            kind: MediaKind::Audio,
            mime_type: "audio/mpeg".into(),
            reference: MediaRef::DataUrl("data:audio/mpeg;base64,zzzz".into()),
            label: None,
            id: None,
        });
        let msgs = to_provider_messages("sys", &[msg]);
        let user = msgs.iter().find(|m| m.role == "user").unwrap();
        let parts = user.parts.as_ref().unwrap();
        assert!(parts
            .iter()
            .any(|p| matches!(p, ChatContentPart::AudioUrl { .. })));
        assert!(parts
            .iter()
            .any(|p| matches!(p, ChatContentPart::VideoUrl { .. })));
        assert_eq!(
            parts
                .iter()
                .filter(|p| matches!(p, ChatContentPart::AudioUrl { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn tool_without_id_is_skipped() {
        let session = vec![
            Message::assistant_with_tools(
                "",
                vec![ToolCall {
                    id: "c1".into(),
                    name: "a".into(),
                    arguments: json!({}),
                    signature: None,
                }],
            ),
            Message::tool("orphan"),
            Message::tool_with_id("c1", "ok"),
        ];
        let msgs = to_provider_messages("sys", &session);
        let tools: Vec<_> = msgs.iter().filter(|m| m.role == "tool").collect();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].tool_call_id.as_deref(), Some("c1"));
    }

    #[test]
    fn dangling_assistant_tool_calls_are_stripped() {
        let session = vec![
            Message::assistant_with_tools(
                "partial",
                vec![
                    ToolCall {
                        id: "c1".into(),
                        name: "a".into(),
                        arguments: json!({}),
                        signature: None,
                    },
                    ToolCall {
                        id: "c2".into(),
                        name: "b".into(),
                        arguments: json!({}),
                        signature: None,
                    },
                ],
            ),
            Message::tool_with_id("c1", "ok"),
        ];
        let msgs = to_provider_messages("sys", &session);
        let assistant = msgs.iter().find(|m| m.role == "assistant").unwrap();
        let calls = assistant.tool_calls.as_ref().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "c1");
    }

    #[test]
    fn tool_message_uses_compressed_content_for_provider() {
        let assistant = Message::assistant_with_tools(
            "",
            vec![ToolCall {
                id: "c1".into(),
                name: "search".into(),
                arguments: json!({}),
                signature: None,
            }],
        );
        let mut tool = Message::tool_with_id("c1", "original long result");
        tool.compressed_content = Some("compressed result".into());

        let msgs = to_provider_messages("sys", &[assistant, tool]);
        let tool_msg = msgs.iter().find(|m| m.role == "tool").expect("tool");
        assert_eq!(tool_msg.content, "compressed result");
    }
}
