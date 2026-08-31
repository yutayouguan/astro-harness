//! Message → OpenAI Chat Completions wire format 转换。

use serde_json::{json, Value};

use crate::types::message::{AssistantContent, Message, UserContent};

/// 将统一 Message 列表转为 OpenAI `messages` JSON 数组。
pub fn to_openai_messages(messages: &[Message]) -> Vec<Value> {
    to_openai_messages_with_developer_role(messages, true)
}

pub fn to_openai_messages_with_developer_role(
    messages: &[Message],
    supports_developer_role: bool,
) -> Vec<Value> {
    messages
        .iter()
        .filter_map(|message| message_to_openai(message, supports_developer_role))
        .collect()
}

fn message_to_openai(msg: &Message, supports_developer_role: bool) -> Option<Value> {
    match msg {
        Message::System { content } => Some(json!({"role": "system", "content": content})),
        Message::Developer { content } => Some(json!({
            "role": if supports_developer_role { "developer" } else { "system" },
            "content": content,
        })),

        Message::User { content } => {
            if content.len() == 1 {
                if let UserContent::Text { text } = &content[0] {
                    return Some(json!({"role": "user", "content": text}));
                }
            }
            let parts: Vec<Value> = content.iter().map(user_content_to_openai).collect();
            Some(json!({"role": "user", "content": parts}))
        }

        Message::Assistant { content } => {
            let mut obj = json!({"role": "assistant"});
            let mut text_parts = Vec::new();
            let mut thinking_parts = Vec::new();
            let mut tool_calls = Vec::new();

            for c in content {
                match c {
                    AssistantContent::Text { text } => text_parts.push(text.clone()),
                    AssistantContent::ToolCall(tc) => {
                        let args = match &tc.arguments {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        tool_calls.push(json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.name,
                                "arguments": args,
                            }
                        }));
                    }
                    AssistantContent::Thinking { text, .. } => {
                        thinking_parts.push(text.clone());
                    }
                }
            }

            let text = text_parts.join("");
            if tool_calls.is_empty() {
                obj["content"] = json!(text);
            } else {
                obj["content"] = if text.is_empty() {
                    Value::Null
                } else {
                    json!(text)
                };
                obj["tool_calls"] = Value::Array(tool_calls);
            }

            // 保留 thinking：MiniMax/DeepSeek 多轮工具调用要求思维链连续
            let reasoning = thinking_parts.join("");
            if !reasoning.is_empty() {
                obj["reasoning_content"] = json!(&reasoning);
                // MiniMax 期望 reasoning_details 数组格式
                obj["reasoning_details"] = json!([{
                    "type": "reasoning.text",
                    "text": reasoning,
                }]);
            }

            Some(obj)
        }

        Message::Tool {
            tool_call_id,
            content,
            ..
        } => {
            if tool_call_id.trim().is_empty() {
                return None;
            }
            Some(json!({
                "role": "tool",
                "tool_call_id": tool_call_id,
                "content": content,
            }))
        }
    }
}

fn user_content_to_openai(content: &UserContent) -> Value {
    match content {
        UserContent::Text { text } => json!({"type": "text", "text": text}),
        UserContent::Image { url } => json!({
            "type": "image_url",
            "image_url": {"url": url}
        }),
        UserContent::Audio { url, .. } => json!({
            "type": "text",
            "text": format!("[audio attached: {url}]")
        }),
        UserContent::Video { url, .. } => json!({
            "type": "text",
            "text": format!("[video attached: {url}]")
        }),
        UserContent::Document { url, .. } => json!({
            "type": "text",
            "text": format!("[document attached: {url}]")
        }),
        UserContent::ToolResult { .. } => json!({"type": "text", "text": ""}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_text_messages() {
        let msgs = to_openai_messages(&[
            Message::system("You are helpful."),
            Message::developer("Follow repository instructions."),
            Message::user_text("Hello"),
        ]);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "You are helpful.");
        assert_eq!(msgs[1]["role"], "developer");
        assert_eq!(msgs[1]["content"], "Follow repository instructions.");
        assert_eq!(msgs[2]["role"], "user");
        assert_eq!(msgs[2]["content"], "Hello");
    }

    #[test]
    fn legacy_chat_protocol_lowers_developer_to_system() {
        let messages =
            to_openai_messages_with_developer_role(&[Message::developer("dynamic policy")], false);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "dynamic policy");
    }

    #[test]
    fn multimodal_user() {
        let msgs = to_openai_messages(&[Message::user(vec![
            UserContent::Text {
                text: "Look".into(),
            },
            UserContent::Image {
                url: "data:image/png;base64,abc".into(),
            },
        ])]);
        let content = msgs[0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "image_url");
    }

    #[test]
    fn tool_call_empty_content_null() {
        let msgs = to_openai_messages(&[Message::assistant(vec![AssistantContent::ToolCall(
            crate::types::ToolCall {
                id: "call_1".into(),
                name: "read".into(),
                arguments: json!({"path": "f.rs"}),
                signature: None,
            },
        )])]);
        assert!(msgs[0]["content"].is_null());
        assert_eq!(msgs[0]["tool_calls"][0]["function"]["name"], "read");
    }

    #[test]
    fn mixed_text_tool_call_and_result_keep_chat_pairing() {
        let msgs = to_openai_messages(&[
            Message::assistant(vec![
                AssistantContent::Text {
                    text: "Reading now.".into(),
                },
                AssistantContent::ToolCall(crate::types::ToolCall {
                    id: "call_1".into(),
                    name: "read".into(),
                    arguments: json!({"path": "f.rs"}),
                    signature: None,
                }),
            ]),
            Message::tool_result("call_1", "done", false),
        ]);

        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0]["role"], "assistant");
        assert_eq!(msgs[0]["content"], "Reading now.");
        assert_eq!(msgs[0]["tool_calls"][0]["id"], "call_1");
        assert_eq!(msgs[1]["role"], "tool");
        assert_eq!(msgs[1]["tool_call_id"], "call_1");
    }
}
