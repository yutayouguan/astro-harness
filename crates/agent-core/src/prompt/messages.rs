//! 会话消息 → Provider 消息转换。
//!
//! 将应用内 `types::message::Message` 序列转为统一的 `providers::types::message::Message`。

use providers::types::message::{
    AssistantContent, Message as ProviderMessage, ToolCall as ProviderToolCall, UserContent,
};
use types::message::{Message, MessageContent, Role};

/// 将会话历史与 system prompt 转为 Provider 可消费的聊天消息列表。
///
/// 首条固定为 `system` 角色；tool 消息会从历史中反向查找对应 `tool_call_id` 以填充 `name`。
/// 发送前会 [`sanitize_tool_pairs`](super::sanitize::sanitize_tool_pairs)：去掉悬挂
/// `tool_calls` 与孤儿 tool 消息，避免上游 400。
pub fn to_provider_messages(system_prompt: &str, session: &[Message]) -> Vec<ProviderMessage> {
    let session = super::sanitize::sanitized_tool_pairs(session);
    let mut messages = vec![ProviderMessage::system(system_prompt)];

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

        match message.role {
            Role::System => {
                messages.push(ProviderMessage::system(message.content_text()));
            }
            Role::Tool => {
                let content = message.provider_view_text().into_owned();
                let is_error = content.starts_with("Error");
                messages.push(ProviderMessage::Tool {
                    tool_call_id: message.tool_call_id.clone().unwrap_or_default(),
                    content,
                    is_error,
                });
            }
            Role::Assistant => {
                let mut content_parts = Vec::new();

                // Thinking / reasoning
                if let Some(reasoning) = &message.reasoning {
                    content_parts.push(AssistantContent::Thinking {
                        text: reasoning.clone(),
                        signature: message.thought_signature.clone(),
                    });
                } else if let Some(sig) = &message.thought_signature {
                    content_parts.push(AssistantContent::Thinking {
                        text: String::new(),
                        signature: Some(sig.clone()),
                    });
                }

                // Text content
                let text = message.content_text();
                if !text.is_empty() {
                    content_parts.push(AssistantContent::Text { text });
                }

                // Tool calls
                if let Some(ref calls) = message.tool_calls {
                    for c in calls {
                        content_parts.push(AssistantContent::ToolCall(ProviderToolCall {
                            id: c.id.clone(),
                            name: c.name.clone(),
                            arguments: c.arguments.clone(),
                            signature: c.signature.clone(),
                        }));
                    }
                }

                if content_parts.is_empty() {
                    content_parts.push(AssistantContent::Text {
                        text: String::new(),
                    });
                }
                messages.push(ProviderMessage::Assistant {
                    content: content_parts,
                });
            }
            Role::User => {
                let mut parts = build_user_parts(message);
                // Merge media assets
                parts = merge_media_user_parts(parts, &message.media);
                messages.push(ProviderMessage::User { content: parts });
            }
        }
    }

    messages
}

/// Build user content parts from a session message.
fn build_user_parts(message: &Message) -> Vec<UserContent> {
    match &message.content {
        MessageContent::Text(s) => {
            vec![UserContent::Text { text: s.clone() }]
        }
        MessageContent::Parts(ps) => {
            let mut parts: Vec<UserContent> = ps.iter().filter_map(content_part_to_user).collect();
            if parts.is_empty() {
                parts.push(UserContent::Text {
                    text: message.content_text(),
                });
            }
            parts
        }
    }
}

fn content_part_to_user(p: &types::message::ContentPart) -> Option<UserContent> {
    match p.kind.as_str() {
        "text" => Some(UserContent::Text {
            text: p.text.clone().unwrap_or_default(),
        }),
        "image_url" => p
            .image_url
            .as_ref()
            .map(|u| UserContent::Image { url: u.url.clone() }),
        "audio_url" => p.audio_url.as_ref().map(|u| UserContent::Audio {
            url: u.url.clone(),
            mime_type: u.mime_type.clone(),
        }),
        "video_url" => p.video_url.as_ref().map(|u| UserContent::Video {
            url: u.url.clone(),
            mime_type: u.mime_type.clone(),
        }),
        _ => None,
    }
}

/// 将 `Message.media` 中可入模的 data/remote URI 并入 parts（workspace 路径跳过）。
fn merge_media_user_parts(
    mut parts: Vec<UserContent>,
    media: &[types::MediaAsset],
) -> Vec<UserContent> {
    for asset in media {
        if let Some(p) = media_asset_to_user_content(asset) {
            // 避免与 Parts 里已有同 URL 重复
            let url = match &p {
                UserContent::Image { url }
                | UserContent::Audio { url, .. }
                | UserContent::Video { url, .. }
                | UserContent::Document { url, .. } => Some(url.as_str()),
                _ => None,
            };
            let dup = url.is_some_and(|u| {
                parts.iter().any(|e| match e {
                    UserContent::Image { url }
                    | UserContent::Audio { url, .. }
                    | UserContent::Video { url, .. }
                    | UserContent::Document { url, .. } => url == u,
                    _ => false,
                })
            });
            if !dup {
                parts.push(p);
            }
        }
    }
    parts
}

fn media_asset_to_user_content(asset: &types::MediaAsset) -> Option<UserContent> {
    use types::{MediaKind, MediaRef};
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
        MediaKind::Image => Some(UserContent::Image { url }),
        MediaKind::Audio => Some(UserContent::Audio {
            url,
            mime_type: mime,
        }),
        MediaKind::Video => Some(UserContent::Video {
            url,
            mime_type: mime,
        }),
        MediaKind::File if mime.contains("pdf") => Some(UserContent::Document {
            url,
            mime_type: mime,
        }),
        MediaKind::File => Some(UserContent::Text {
            text: format!("[file attached: {mime}]"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use types::message::{Message, MessageContent, ToolCall};
    use types::{MediaAsset, MediaKind, MediaRef};

    #[test]
    fn multimodal_parts_map_to_provider_parts() {
        let session = vec![Message::user_with_images(
            "描述",
            &["data:image/png;base64,xx".into()],
        )];
        let msgs = to_provider_messages("sys", &session);
        assert_eq!(msgs.len(), 2);
        let user = &msgs[1];
        assert_eq!(user.role(), providers::types::message::Role::User);
        if let ProviderMessage::User { content } = user {
            assert!(content
                .iter()
                .any(|c| matches!(c, UserContent::Text { text } if text == "描述")));
            assert!(content.iter().any(
                |c| matches!(c, UserContent::Image { url } if url.starts_with("data:image/png"))
            ));
        } else {
            panic!("expected User message");
        }
    }

    #[test]
    fn audio_video_parts_and_media_field_map() {
        let mut msg = Message::user("听这段并看视频");
        msg.content = MessageContent::Parts(vec![
            types::message::ContentPart::text("听这段并看视频"),
            types::message::ContentPart::audio_url("data:audio/wav;base64,AQID", "audio/wav"),
            types::message::ContentPart::video_url("data:video/mp4;base64,AQID", "video/mp4"),
        ]);
        msg.media.push(MediaAsset {
            kind: MediaKind::Audio,
            mime_type: "audio/mpeg".into(),
            reference: MediaRef::DataUrl("data:audio/mpeg;base64,zzzz".into()),
            label: None,
            id: None,
        });
        let msgs = to_provider_messages("sys", &[msg]);
        let user = msgs
            .iter()
            .find(|m| m.role() == providers::types::message::Role::User)
            .unwrap();
        if let ProviderMessage::User { content } = user {
            assert!(content
                .iter()
                .any(|c| matches!(c, UserContent::Audio { .. })));
            assert!(content
                .iter()
                .any(|c| matches!(c, UserContent::Video { .. })));
            assert_eq!(
                content
                    .iter()
                    .filter(|c| matches!(c, UserContent::Audio { .. }))
                    .count(),
                2
            );
        }
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
        let tools: Vec<_> = msgs
            .iter()
            .filter(|m| m.role() == providers::types::message::Role::Tool)
            .collect();
        assert_eq!(tools.len(), 1);
        if let ProviderMessage::Tool { tool_call_id, .. } = &tools[0] {
            assert_eq!(tool_call_id, "c1");
        }
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
        let assistant = msgs
            .iter()
            .find(|m| m.role() == providers::types::message::Role::Assistant)
            .unwrap();
        if let ProviderMessage::Assistant { content } = assistant {
            let calls: Vec<_> = content
                .iter()
                .filter_map(|c| match c {
                    AssistantContent::ToolCall(tc) => Some(tc),
                    _ => None,
                })
                .collect();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].id, "c1");
        }
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
        let tool_msg = msgs
            .iter()
            .find(|m| m.role() == providers::types::message::Role::Tool)
            .expect("tool");
        assert_eq!(tool_msg.text_content(), "compressed result");
    }
}
