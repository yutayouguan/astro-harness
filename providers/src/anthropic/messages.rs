//! 内部 [`ChatMessage`] 与 Anthropic Messages API 请求格式之间的转换。

use serde_json::{json, Value};

use crate::http_stream::parse_data_url;
use crate::trait_::{ChatContentPart, ChatMessage};

/// 将消息列表转为 Anthropic `(system, messages)` 二元组。
pub fn to_anthropic_messages(messages: &[ChatMessage]) -> (String, Vec<Value>) {
    let mut system = String::new();
    let mut api_messages = Vec::new();
    let mut pending_tool_results: Vec<Value> = Vec::new();

    let flush_tool_results = |pending: &mut Vec<Value>, out: &mut Vec<Value>| {
        if pending.is_empty() {
            return;
        }
        out.push(json!({
            "role": "user",
            "content": Value::Array(std::mem::take(pending)),
        }));
    };

    for m in messages {
        match m.role.as_str() {
            "system" => {
                if !system.is_empty() {
                    system.push('\n');
                }
                system.push_str(&m.content);
            }
            "tool" => {
                let id = m.tool_call_id.clone().unwrap_or_default();
                pending_tool_results.push(json!({
                    "type": "tool_result",
                    "tool_use_id": id,
                    "content": m.content,
                }));
            }
            "assistant" => {
                flush_tool_results(&mut pending_tool_results, &mut api_messages);
                let mut content_blocks = Vec::new();
                if !m.content.is_empty() {
                    content_blocks.push(json!({
                        "type": "text",
                        "text": m.content,
                    }));
                }
                if let Some(ref calls) = m.tool_calls {
                    for c in calls {
                        let input = if c.arguments.is_string() {
                            serde_json::from_str(c.arguments.as_str().unwrap_or("{}"))
                                .unwrap_or(json!({}))
                        } else {
                            c.arguments.clone()
                        };
                        content_blocks.push(json!({
                            "type": "tool_use",
                            "id": c.id,
                            "name": c.name,
                            "input": input,
                        }));
                    }
                }
                if content_blocks.is_empty() {
                    content_blocks.push(json!({ "type": "text", "text": "" }));
                }
                api_messages.push(json!({
                    "role": "assistant",
                    "content": content_blocks,
                }));
            }
            _ => {
                flush_tool_results(&mut pending_tool_results, &mut api_messages);
                let content = anthropic_user_content(m);
                api_messages.push(json!({
                    "role": "user",
                    "content": content,
                }));
            }
        }
    }
    flush_tool_results(&mut pending_tool_results, &mut api_messages);
    (system, api_messages)
}

/// Anthropic user content：纯字符串或 text + image base64 blocks。
pub fn anthropic_user_content(m: &ChatMessage) -> Value {
    let Some(parts) = m.parts.as_ref().filter(|p| !p.is_empty()) else {
        return json!(m.content);
    };
    let mut blocks = Vec::new();
    for p in parts {
        match p {
            ChatContentPart::Text { text } => {
                blocks.push(json!({ "type": "text", "text": text }));
            }
            ChatContentPart::ImageUrl { url } => {
                if let Some((media_type, data)) = parse_data_url(url) {
                    blocks.push(json!({
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": media_type,
                            "data": data,
                        }
                    }));
                } else if url.starts_with("http://") || url.starts_with("https://") {
                    blocks.push(json!({
                        "type": "image",
                        "source": {
                            "type": "url",
                            "url": url,
                        }
                    }));
                }
            }
            ChatContentPart::AudioUrl { mime_type, .. } => {
                blocks.push(json!({
                    "type": "text",
                    "text": format!(
                        "[audio attached: {}]",
                        if mime_type.trim().is_empty() { "audio/*" } else { mime_type }
                    )
                }));
            }
            ChatContentPart::VideoUrl { mime_type, .. } => {
                blocks.push(json!({
                    "type": "text",
                    "text": format!(
                        "[video attached: {}]",
                        if mime_type.trim().is_empty() { "video/*" } else { mime_type }
                    )
                }));
            }
        }
    }
    if blocks.is_empty() {
        json!(m.content)
    } else {
        Value::Array(blocks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trait_::ChatContentPart;

    #[test]
    fn anthropic_user_content_parses_data_url() {
        let m = ChatMessage::user_parts(
            "x",
            vec![
                ChatContentPart::Text { text: "x".into() },
                ChatContentPart::ImageUrl {
                    url: "data:image/jpeg;base64,zzz".into(),
                },
            ],
        );
        let v = anthropic_user_content(&m);
        let blocks = v.as_array().expect("blocks");
        assert_eq!(blocks[1]["type"], "image");
        assert_eq!(blocks[1]["source"]["media_type"], "image/jpeg");
        assert_eq!(blocks[1]["source"]["data"], "zzz");
    }
}
