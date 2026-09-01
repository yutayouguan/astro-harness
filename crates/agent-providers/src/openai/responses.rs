//! OpenAI Responses API — 消息转换与 SSE 解析。
//!
//! 供 [`crate::compat::OpenAIResponsesModel`] 和 [`crate::custom::ConfigDrivenResponsesModel`] 使用。

use serde_json::Value;

#[cfg(test)]
use serde_json::json;

use crate::compat::parse_openai_usage;
use crate::types::stream::StreamChunk;

/// Serialize the Agent's canonical Responses history without passing through
/// a chat-completions message model. Local-only metadata is removed at the
/// wire boundary; a compressed tool view may replace `output` without
/// destroying the persisted raw value.
pub fn to_native_responses_input(
    items: &[agent_protocol::ResponseItem],
) -> serde_json::Result<Vec<Value>> {
    items
        .iter()
        .map(|item| {
            let mut value = serde_json::to_value(item)?;
            if let Some(object) = value.as_object_mut() {
                let metadata = object.remove("internal_chat_message_metadata_passthrough");
                if matches!(
                    object.get("type").and_then(Value::as_str),
                    Some("function_call_output" | "custom_tool_call_output")
                ) {
                    if let Some(compressed) = metadata
                        .as_ref()
                        .and_then(|value| value.get("astro_compressed_output"))
                        .cloned()
                    {
                        object.insert("output".into(), compressed);
                    }
                }
            }
            Ok(value)
        })
        .collect()
}

#[cfg(test)]
mod input_tests {
    use super::*;

    #[test]
    fn native_input_preserves_distinct_item_and_call_ids() {
        let input = to_native_responses_input(&[agent_protocol::ResponseItem::FunctionCall {
            id: Some("item_1".into()),
            name: "lookup".into(),
            namespace: Some("mcp".into()),
            arguments: "{}".into(),
            encrypted_function_args: Some(vec!["opaque".into()]),
            call_id: "call_1".into(),
            internal_chat_message_metadata_passthrough: Some(json!({"local": true})),
        }])
        .unwrap();

        assert_eq!(input[0]["id"], "item_1");
        assert_eq!(input[0]["call_id"], "call_1");
        assert_eq!(input[0]["namespace"], "mcp");
        assert_eq!(input[0]["encrypted_function_args"][0], "opaque");
        assert!(input[0]
            .get("internal_chat_message_metadata_passthrough")
            .is_none());
    }

    #[test]
    fn native_input_uses_compressed_tool_view_only_on_wire() {
        let raw = agent_protocol::ResponseItem::FunctionCallOutput {
            id: Some("out_1".into()),
            call_id: Some("call_1".into()),
            name: Some("lookup".into()),
            namespace: None,
            output: agent_protocol::FunctionCallOutputPayload::from_text("raw output".into()),
            internal_chat_message_metadata_passthrough: Some(json!({
                "astro_compressed_output": "short view"
            })),
        };
        let input = to_native_responses_input(std::slice::from_ref(&raw)).unwrap();
        assert_eq!(input[0]["output"], "short view");
        assert!(matches!(
            raw,
            agent_protocol::ResponseItem::FunctionCallOutput { output, .. }
                if output.text_content() == Some("raw output")
        ));
    }
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
                let item_type = item.get("type").and_then(|t| t.as_str());
                if matches!(
                    item_type,
                    Some("function_call" | "custom_tool_call" | "tool_search_call")
                ) {
                    let id = item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = if item_type == Some("tool_search_call") {
                        "tool_search".to_string()
                    } else {
                        item.get("name")
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .to_string()
                    };
                    let index = v.get("output_index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                    let mut chunks = vec![StreamChunk::ToolCallStart {
                        index,
                        id,
                        name,
                        signature: None,
                    }];
                    if item_type == Some("tool_search_call") {
                        if let Some(arguments) = item.get("arguments") {
                            chunks.push(StreamChunk::ToolCallDelta {
                                index,
                                arguments: arguments.to_string(),
                            });
                        }
                    }
                    return chunks;
                }
            }
        }

        "response.output_item.done" => {
            if let Some(item) = v.get("item") {
                let native_item =
                    serde_json::from_value::<agent_protocol::ResponseItem>(item.clone())
                        .ok()
                        .map(StreamChunk::ResponseItemDone);
                let item_type = item.get("type").and_then(|t| t.as_str());
                if matches!(
                    item_type,
                    Some("function_call" | "custom_tool_call" | "tool_search_call")
                ) {
                    let index = v.get("output_index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                    let arguments = match item_type {
                        Some("custom_tool_call") => item
                            .get("input")
                            .and_then(Value::as_str)
                            .filter(|input| !input.is_empty())
                            .and_then(|input| serde_json::to_string(input).ok()),
                        Some("tool_search_call") => item.get("arguments").map(Value::to_string),
                        _ => item
                            .get("arguments")
                            .and_then(Value::as_str)
                            .filter(|args| !args.is_empty())
                            .map(str::to_string),
                    };
                    if let Some(arguments) = arguments {
                        let mut chunks = vec![StreamChunk::ToolCallDelta { index, arguments }];
                        if let Some(native_item) = native_item {
                            chunks.push(native_item);
                        }
                        return chunks;
                    }
                }
                if let Some(native_item) = native_item {
                    one!(native_item);
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
                        matches!(
                            item.get("type").and_then(|t| t.as_str()),
                            Some("function_call" | "custom_tool_call" | "tool_search_call")
                        )
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

    #[test]
    fn output_item_done_emits_native_response_item() {
        let chunks = extract_responses_chunks(
            r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","id":"item_1","call_id":"call_1","name":"lookup","namespace":"mcp","arguments":"{}","encrypted_function_args":["opaque"],"status":"completed"}}"#,
        );
        assert!(chunks.iter().any(|chunk| matches!(
            chunk,
            StreamChunk::ResponseItemDone(agent_protocol::ResponseItem::FunctionCall {
                id: Some(id),
                call_id,
                namespace: Some(namespace),
                encrypted_function_args: Some(encrypted),
                ..
            }) if id == "item_1"
                && call_id == "call_1"
                && namespace == "mcp"
                && encrypted == &["opaque"]
        )));
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
    fn extract_custom_tool_call_done_as_string_argument() {
        let data = r#"{"type":"response.output_item.done","output_index":2,"item":{"type":"custom_tool_call","call_id":"patch_1","name":"apply_patch","input":"*** Begin Patch\n*** End Patch"}}"#;
        let chunks = extract_responses_chunks(data);
        assert!(matches!(
            &chunks[0],
            StreamChunk::ToolCallDelta { index: 2, arguments }
                if serde_json::from_str::<String>(arguments).unwrap().starts_with("*** Begin Patch")
        ));
    }

    #[test]
    fn extract_tool_search_call_with_arguments() {
        let data = r#"{"type":"response.output_item.added","output_index":1,"item":{"type":"tool_search_call","call_id":"search_1","execution":"client","arguments":{"query":"calendar","limit":2}}}"#;
        let chunks = extract_responses_chunks(data);
        assert!(matches!(
            &chunks[0],
            StreamChunk::ToolCallStart { index: 1, id, name, .. }
                if id == "search_1" && name == "tool_search"
        ));
        assert!(matches!(
            &chunks[1],
            StreamChunk::ToolCallDelta { index: 1, arguments }
                if serde_json::from_str::<Value>(arguments).unwrap()["query"] == "calendar"
        ));
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
