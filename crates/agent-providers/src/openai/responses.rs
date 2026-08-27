//! OpenAI Responses API — 消息转换与 SSE 解析。
//!
//! 供 [`crate::compat::OpenAIResponsesModel`] 和 [`crate::custom::ConfigDrivenCompletionModel`] 使用。

use serde_json::{json, Value};

use crate::compat::parse_openai_usage;
use crate::types::message::{AssistantContent, Message, ToolCall, UserContent};
use crate::types::stream::StreamChunk;

// ---------------------------------------------------------------------------
// 消息转换
// ---------------------------------------------------------------------------

/// 将 [`Message`] 转为 Responses API `input` 数组。
pub fn to_responses_input(messages: &[Message]) -> Vec<Value> {
    let mut input = Vec::with_capacity(messages.len());
    for m in messages {
        match m {
            Message::Tool {
                tool_call_id,
                content,
                ..
            } => {
                let call_id = if tool_call_id.trim().is_empty() {
                    ""
                } else {
                    tool_call_id.as_str()
                };
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": content,
                }));
            }

            Message::Assistant { content } => {
                let mut has_tool_calls = false;
                for part in content {
                    if let AssistantContent::ToolCall(ToolCall {
                        id,
                        name,
                        arguments,
                        ..
                    }) = part
                    {
                        has_tool_calls = true;
                        let args = match arguments {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        input.push(json!({
                            "type": "function_call",
                            "id": id,
                            "call_id": id,
                            "name": name,
                            "arguments": args,
                        }));
                    }
                }
                let text = m.text_content();
                if has_tool_calls && text.is_empty() {
                    continue;
                }
                input.push(json!({
                    "role": "assistant",
                    "content": json!(text),
                }));
            }

            Message::User { content } => {
                let json_content = build_content_from_user(content);
                input.push(json!({
                    "role": "user",
                    "content": json_content,
                }));
            }

            Message::Developer { content } => {
                input.push(json!({
                    "role": "developer",
                    "content": content,
                }));
            }

            Message::System { .. } => {
                // 跳过：由调用方通过顶层 `instructions` 字段发送
            }
        }
    }
    input
}

#[cfg(test)]
mod input_tests {
    use super::*;

    #[test]
    fn developer_context_is_a_role_bearing_input_item() {
        let input = to_responses_input(&[
            Message::system("stable base"),
            Message::developer("dynamic policy"),
            Message::user_text("hello"),
        ]);

        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["role"], "developer");
        assert_eq!(input[0]["content"], "dynamic policy");
        assert_eq!(input[1]["role"], "user");
    }
}

fn build_content_from_user(parts: &[UserContent]) -> Value {
    if parts.len() == 1 {
        if let UserContent::Text { text } = &parts[0] {
            return json!(text);
        }
    }
    let arr: Vec<Value> = parts
        .iter()
        .map(|p| match p {
            UserContent::Text { text } => {
                json!({ "type": "input_text", "text": text })
            }
            UserContent::Image { url } => json!({
                "type": "input_image",
                "image_url": url,
            }),
            UserContent::Audio { url, .. } => json!({
                "type": "input_audio",
                "data": url,
            }),
            UserContent::Video { url, .. } => json!({
                "type": "input_file",
                "file_url": url,
            }),
            UserContent::Document { url, .. } => json!({
                "type": "input_file",
                "file_url": url,
            }),
            UserContent::ToolResult { .. } => {
                json!({ "type": "input_text", "text": "[tool result]" })
            }
        })
        .collect();
    Value::Array(arr)
}

// ---------------------------------------------------------------------------
// SSE 事件解析
// ---------------------------------------------------------------------------

/// 解析 Responses API SSE `data:` 负载为一组 [`StreamChunk`]。
pub fn extract_responses_chunks(data: &str) -> Vec<StreamChunk> {
    let Some(v) = serde_json::from_str::<Value>(data).ok() else {
        return Vec::new();
    };

    let event_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");

    macro_rules! one {
        ($chunk:expr) => {
            return vec![$chunk]
        };
    }

    match event_type {
        "response.output_text.delta" => {
            if let Some(delta) = v
                .get("delta")
                .and_then(|d| d.as_str())
                .filter(|s| !s.is_empty())
            {
                one!(StreamChunk::Text(delta.to_string()));
            }
        }

        "response.reasoning_summary_text.delta" => {
            if let Some(delta) = v
                .get("delta")
                .and_then(|d| d.as_str())
                .filter(|s| !s.is_empty())
            {
                one!(StreamChunk::Thinking(delta.to_string()));
            }
        }

        "response.refusal.delta" => {
            if let Some(delta) = v
                .get("delta")
                .and_then(|d| d.as_str())
                .filter(|s| !s.is_empty())
            {
                one!(StreamChunk::Text(delta.to_string()));
            }
        }

        "response.output_item.added" => {
            if let Some(item) = v.get("item") {
                if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                    let id = item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = item
                        .get("name")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string();
                    let index = v.get("output_index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                    one!(StreamChunk::ToolCallStart {
                        index,
                        id,
                        name,
                        signature: None,
                    });
                }
            }
        }

        "response.output_item.done" => {
            if let Some(item) = v.get("item") {
                if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                    let index = v.get("output_index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                    if let Some(args) = item
                        .get("arguments")
                        .and_then(|a| a.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        one!(StreamChunk::ToolCallDelta {
                            index,
                            arguments: args.to_string()
                        });
                    }
                }
            }
        }

        "response.function_call_arguments.delta" => {
            if let Some(delta) = v.get("delta").and_then(|d| d.as_str()) {
                let index = v.get("output_index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                one!(StreamChunk::ToolCallDelta {
                    index,
                    arguments: delta.to_string()
                });
            }
        }

        "response.function_call_arguments.done" => {}

        "response.completed" => {
            let resp = v.get("response");
            let usage = resp.and_then(parse_openai_usage);
            let has_tool_calls = resp
                .and_then(|r| r.get("output"))
                .and_then(|o| o.as_array())
                .map(|arr| {
                    arr.iter().any(|item| {
                        item.get("type").and_then(|t| t.as_str()) == Some("function_call")
                    })
                })
                .unwrap_or(false);
            let finish = if has_tool_calls { "tool_calls" } else { "stop" };
            let mut chunks = Vec::with_capacity(2);
            if let Some(u) = usage {
                chunks.push(StreamChunk::Usage(u));
            }
            chunks.push(StreamChunk::Done {
                finish_reason: finish.to_string(),
            });
            return chunks;
        }

        "response.failed" => {
            let msg = v
                .pointer("/response/status_details/error/message")
                .or_else(|| v.pointer("/response/error/message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Responses API 请求失败");
            one!(StreamChunk::Error(msg.to_string()));
        }

        "response.incomplete" => {
            one!(StreamChunk::Done {
                finish_reason: "length".to_string()
            });
        }

        "error" => {
            let msg = v
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("未知错误");
            one!(StreamChunk::Error(msg.to_string()));
        }

        _ => {}
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::message::{AssistantContent, Message, ToolCall, UserContent};

    // ── 消息转换 ──

    #[test]
    fn to_responses_input_user_and_assistant() {
        let msgs = vec![
            Message::user_text("hello"),
            Message::assistant_text("hi there"),
        ];
        let input = to_responses_input(&msgs);
        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"], "hello");
        assert_eq!(input[1]["role"], "assistant");
    }

    #[test]
    fn to_responses_input_tool_result() {
        let msgs = vec![Message::tool_result("call_abc", r#"{"result": 42}"#, false)];
        let input = to_responses_input(&msgs);
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "function_call_output");
        assert_eq!(input[0]["call_id"], "call_abc");
    }

    #[test]
    fn to_responses_input_assistant_tool_calls() {
        let msgs = vec![Message::assistant(vec![AssistantContent::ToolCall(
            ToolCall {
                id: "call_123".into(),
                name: "get_weather".into(),
                arguments: json!({"location": "Paris"}),
                signature: None,
            },
        )])];
        let input = to_responses_input(&msgs);
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "function_call");
        assert_eq!(input[0]["name"], "get_weather");
        assert_eq!(input[0]["call_id"], "call_123");
    }

    // ── 内容构建 ──

    #[test]
    fn build_content_document_as_input_file() {
        let parts = vec![
            UserContent::Text {
                text: "what is in this file?".into(),
            },
            UserContent::Document {
                url: "https://example.com/doc.pdf".into(),
                mime_type: "application/pdf".into(),
            },
        ];
        let content = build_content_from_user(&parts);
        let arr = content.as_array().unwrap();
        assert_eq!(arr[0]["type"], "input_text");
        assert_eq!(arr[1]["type"], "input_file");
        assert_eq!(arr[1]["file_url"], "https://example.com/doc.pdf");
    }

    #[test]
    fn build_content_audio_as_input_audio() {
        let parts = vec![UserContent::Audio {
            url: "data:audio/mp3;base64,AAAA".into(),
            mime_type: "audio/mp3".into(),
        }];
        let content = build_content_from_user(&parts);
        let arr = content.as_array().unwrap();
        assert_eq!(arr[0]["type"], "input_audio");
    }

    #[test]
    fn build_content_image_as_input_image() {
        let parts = vec![UserContent::Image {
            url: "https://example.com/img.jpg".into(),
        }];
        let content = build_content_from_user(&parts);
        let arr = content.as_array().unwrap();
        assert_eq!(arr[0]["type"], "input_image");
    }

    #[test]
    fn system_message_skipped() {
        let msgs = vec![
            Message::system("You are helpful"),
            Message::user_text("hello"),
        ];
        let input = to_responses_input(&msgs);
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "user");
    }

    // ── SSE 解析 ──

    #[test]
    fn extract_text_delta() {
        let data = r#"{"type":"response.output_text.delta","delta":"Hello"}"#;
        let chunks = extract_responses_chunks(data);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Text(t) if t == "Hello"));
    }

    #[test]
    fn extract_reasoning_delta() {
        let data = r#"{"type":"response.reasoning_summary_text.delta","delta":"Let me think..."}"#;
        let chunks = extract_responses_chunks(data);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Thinking(t) if t == "Let me think..."));
    }

    #[test]
    fn extract_reasoning_empty_returns_empty() {
        let data = r#"{"type":"response.reasoning_summary_text.delta","delta":""}"#;
        assert!(extract_responses_chunks(data).is_empty());
    }

    #[test]
    fn extract_function_call_added() {
        let data = r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","call_id":"call_1","name":"search"}}"#;
        let chunks = extract_responses_chunks(data);
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], StreamChunk::ToolCallStart { index: 0, ref id, ref name, .. } if id == "call_1" && name == "search")
        );
    }

    #[test]
    fn extract_function_call_args_delta() {
        let data = r#"{"type":"response.function_call_arguments.delta","output_index":0,"delta":"{\"q\":"}"#;
        let chunks = extract_responses_chunks(data);
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], StreamChunk::ToolCallDelta { index: 0, ref arguments } if arguments == "{\"q\":")
        );
    }

    #[test]
    fn extract_completed_returns_usage_and_done() {
        let data = r#"{"type":"response.completed","response":{"output":[{"type":"message"}],"usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}}}"#;
        let chunks = extract_responses_chunks(data);
        assert_eq!(chunks.len(), 2);
        assert!(matches!(&chunks[0], StreamChunk::Usage(_)));
        assert!(
            matches!(&chunks[1], StreamChunk::Done { ref finish_reason } if finish_reason == "stop")
        );
    }

    #[test]
    fn extract_completed_with_tool_calls() {
        let data = r#"{"type":"response.completed","response":{"output":[{"type":"function_call","call_id":"c1","name":"f","arguments":"{}"}],"usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}}}"#;
        let chunks = extract_responses_chunks(data);
        assert_eq!(chunks.len(), 2);
        assert!(
            matches!(&chunks[1], StreamChunk::Done { ref finish_reason } if finish_reason == "tool_calls")
        );
    }

    #[test]
    fn extract_completed_no_usage() {
        let data = r#"{"type":"response.completed","response":{"output":[{"type":"message"}]}}"#;
        let chunks = extract_responses_chunks(data);
        assert_eq!(chunks.len(), 1);
        assert!(
            matches!(&chunks[0], StreamChunk::Done { ref finish_reason } if finish_reason == "stop")
        );
    }

    #[test]
    fn extract_failed() {
        let data = r#"{"type":"response.failed","response":{"status_details":{"error":{"message":"rate limit"}}}}"#;
        let chunks = extract_responses_chunks(data);
        assert!(matches!(&chunks[0], StreamChunk::Error(ref msg) if msg == "rate limit"));
    }

    #[test]
    fn extract_error_event() {
        let data = r#"{"type":"error","message":"invalid request"}"#;
        let chunks = extract_responses_chunks(data);
        assert!(matches!(&chunks[0], StreamChunk::Error(ref msg) if msg == "invalid request"));
    }

    #[test]
    fn lifecycle_events_return_empty() {
        for event in [
            r#"{"type":"response.created","response":{}}"#,
            r#"{"type":"response.in_progress","response":{}}"#,
            r#"{"type":"response.output_text.done","text":"hi"}"#,
            r#"{"type":"response.content_part.added"}"#,
            r#"{"type":"response.content_part.done"}"#,
            r#"{"type":"response.function_call_arguments.done","output_index":0,"arguments":"{}"}"#,
        ] {
            assert!(
                extract_responses_chunks(event).is_empty(),
                "expected empty for {event}"
            );
        }
    }
}
