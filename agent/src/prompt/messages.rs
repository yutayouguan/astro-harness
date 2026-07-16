//! 会话消息 → Provider 消息转换。
//!
//! 将应用内 `common::message::Message` 序列转为各 LLM Provider 统一的 `ChatMessage` 格式，
//! 并补齐 system 前缀与 tool 角色的 `name` 回溯（Provider 要求 tool 消息关联原调用名）。

use common::message::{Message, MessageContent, Role};
use providers::trait_::{
    ChatContentPart, ChatMessage as ProviderMessage, ChatToolCall,
};

/// 将会话历史与 system prompt 转为 Provider 可消费的聊天消息列表。
///
/// 首条固定为 `system` 角色；tool 消息会从历史中反向查找对应 `tool_call_id` 以填充 `name`。
/// 缺少 `tool_call_id` 的 tool 消息会被跳过（上游 OpenAI 兼容接口会因此 400）。
pub fn to_provider_messages(system_prompt: &str, session: &[Message]) -> Vec<ProviderMessage> {
    let mut messages = vec![ProviderMessage::text("system", system_prompt)];

    for message in session {
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
                })
                .collect()
        });
        let tool_name = if message.role == Role::Tool {
            message.tool_call_id.as_ref().and_then(|id| {
                session.iter().rev().find_map(|m| {
                    m.tool_calls
                        .as_ref()?
                        .iter()
                        .find(|c| &c.id == id)
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
                let parts: Vec<ChatContentPart> = ps
                    .iter()
                    .filter_map(|p| {
                        if p.kind == "text" {
                            Some(ChatContentPart::Text {
                                text: p.text.clone().unwrap_or_default(),
                            })
                        } else if p.kind == "image_url" {
                            p.image_url.as_ref().map(|u| ChatContentPart::ImageUrl {
                                url: u.url.clone(),
                            })
                        } else {
                            None
                        }
                    })
                    .collect();
                (text, Some(parts).filter(|v| !v.is_empty()))
            }
        };
        messages.push(ProviderMessage {
            role: role.to_string(),
            content,
            parts,
            tool_calls,
            tool_call_id: message.tool_call_id.clone(),
            name: tool_name,
        });
    }

    messages
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::message::ToolCall;
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
    fn tool_without_id_is_skipped() {
        let session = vec![
            Message::assistant_with_tools(
                "",
                vec![ToolCall {
                    id: "c1".into(),
                    name: "a".into(),
                    arguments: json!({}),
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
    fn tool_message_uses_compressed_content_for_provider() {
        let assistant = Message::assistant_with_tools(
            "",
            vec![ToolCall {
                id: "c1".into(),
                name: "search".into(),
                arguments: json!({}),
            }],
        );
        let mut tool = Message::tool_with_id("c1", "original long result");
        tool.compressed_content = Some("compressed result".into());

        let msgs = to_provider_messages("sys", &[assistant, tool]);
        let tool_msg = msgs.iter().find(|m| m.role == "tool").expect("tool");
        assert_eq!(tool_msg.content, "compressed result");
    }
}
