//! OpenAI Responses API 流式适配器。
//!
//! POST `{base}/responses` + SSE → [`CompletionStream`]。

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::{json, Value};

use crate::compat::{openai_compatible_base, parse_openai_usage};
use crate::shared::http::{merge_additional_params, resolve_base};
use crate::types::message::{AssistantContent, Message, ToolCall, UserContent};
use crate::types::stream::{CompletionStream, StreamChunk};

// ---------------------------------------------------------------------------
// 消息 / 工具转换
// ---------------------------------------------------------------------------

/// 将 [`Message`] 转为 Responses API `input` 数组。
///
/// Responses API 与 Chat Completions 的主要区别：
/// - tool 角色 → `{ type: "function_call_output", call_id, output }`
/// - assistant + tool_calls → 展开为 `{ type: "function_call", id, name, arguments }` 项
/// - 其余走 `{ role, content }` 不变
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
                        id, name, arguments, ..
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

            Message::System { content } => {
                input.push(json!({
                    "role": "system",
                    "content": json!(content),
                }));
            }
        }
    }
    input
}

pub fn build_content_from_user(parts: &[UserContent]) -> Value {
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
                "type": "input_video",
                "video_url": url,
            }),
            UserContent::Document { url, .. } => json!({
                "type": "input_file",
                "file_url": url,
            }),
            UserContent::ToolResult { .. } => {
                // ToolResult 已在 Message::Tool 分支处理，不应出现在 User 中
                json!({ "type": "input_text", "text": "[tool result]" })
            }
        })
        .collect();
    Value::Array(arr)
}

/// 工具 schema 转换。
///
/// - Chat Completions 函数: `{ type:"function", function:{ name, … } }` → 展平
/// - Responses 原生函数: `{ type:"function", name, … }` → 透传
/// - 内置工具 (`file_search` / `web_search_preview` / `code_interpreter` 等) → 透传
fn to_responses_tools(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|t| {
            let ty = t.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if ty == "function" {
                if let Some(func) = t.get("function") {
                    let mut out = serde_json::Map::new();
                    out.insert("type".into(), json!("function"));
                    if let Some(name) = func.get("name") {
                        out.insert("name".into(), name.clone());
                    }
                    if let Some(desc) = func.get("description") {
                        out.insert("description".into(), desc.clone());
                    }
                    if let Some(params) = func.get("parameters") {
                        out.insert("parameters".into(), params.clone());
                    }
                    if let Some(strict) = func.get("strict") {
                        out.insert("strict".into(), strict.clone());
                    }
                    return Some(Value::Object(out));
                }
                return Some(t.clone());
            }
            if !ty.is_empty() {
                return Some(t.clone());
            }
            None
        })
        .collect()
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
        ($chunk:expr) => { return vec![$chunk] };
    }

    match event_type {
        "response.output_text.delta" => {
            if let Some(delta) = v.get("delta").and_then(|d| d.as_str()).filter(|s| !s.is_empty()) {
                one!(StreamChunk::Text(delta.to_string()));
            }
        }

        "response.reasoning_summary_text.delta" => {
            if let Some(delta) = v.get("delta").and_then(|d| d.as_str()).filter(|s| !s.is_empty()) {
                one!(StreamChunk::Thinking(delta.to_string()));
            }
        }

        "response.refusal.delta" => {
            if let Some(delta) = v.get("delta").and_then(|d| d.as_str()).filter(|s| !s.is_empty()) {
                one!(StreamChunk::Text(delta.to_string()));
            }
        }

        "response.output_item.added" => {
            if let Some(item) = v.get("item") {
                if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                    let id = item.get("call_id").or_else(|| item.get("id"))
                        .and_then(|s| s.as_str()).unwrap_or("").to_string();
                    let name = item.get("name").and_then(|s| s.as_str()).unwrap_or("").to_string();
                    let index = v.get("output_index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                    one!(StreamChunk::ToolCallStart { index, id, name });
                }
            }
        }

        "response.output_item.done" => {
            if let Some(item) = v.get("item") {
                if item.get("type").and_then(|t| t.as_str()) == Some("function_call") {
                    let index = v.get("output_index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                    if let Some(args) = item.get("arguments").and_then(|a| a.as_str()).filter(|s| !s.is_empty()) {
                        one!(StreamChunk::ToolCallDelta { index, arguments: args.to_string() });
                    }
                }
            }
        }

        "response.function_call_arguments.delta" => {
            if let Some(delta) = v.get("delta").and_then(|d| d.as_str()) {
                let index = v.get("output_index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                one!(StreamChunk::ToolCallDelta { index, arguments: delta.to_string() });
            }
        }

        "response.function_call_arguments.done" => {}

        "response.completed" => {
            let resp = v.get("response");
            let usage = resp.and_then(parse_openai_usage);
            let has_tool_calls = resp
                .and_then(|r| r.get("output"))
                .and_then(|o| o.as_array())
                .map(|arr| arr.iter().any(|item| item.get("type").and_then(|t| t.as_str()) == Some("function_call")))
                .unwrap_or(false);
            let finish = if has_tool_calls { "tool_calls" } else { "stop" };
            let mut chunks = Vec::with_capacity(2);
            if let Some(u) = usage {
                chunks.push(StreamChunk::Usage(u));
            }
            chunks.push(StreamChunk::Done { finish_reason: finish.to_string() });
            return chunks;
        }

        "response.failed" => {
            let msg = v.pointer("/response/status_details/error/message")
                .or_else(|| v.pointer("/response/error/message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Responses API 请求失败");
            one!(StreamChunk::Error(msg.to_string()));
        }

        "response.incomplete" => {
            one!(StreamChunk::Done { finish_reason: "stop".to_string() });
        }

        "error" => {
            let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("未知错误");
            one!(StreamChunk::Error(msg.to_string()));
        }

        _ => {}
    }

    Vec::new()
}

// ---------------------------------------------------------------------------
// 主入口
// ---------------------------------------------------------------------------

/// OpenAI Responses API 流式聊天。
pub async fn responses_chat_stream(
    client: &Client,
    provider: &str,
    messages: Vec<Message>,
    tools: Vec<Value>,
    config: &crate::types::request::ProviderConfig,
) -> Result<CompletionStream> {
    if config.api_key.trim().is_empty() {
        return Err(anyhow!("Responses API Key 为空"));
    }

    let base = openai_compatible_base(&resolve_base(config, provider));
    let url = format!("{base}/responses");

    let is_openai = provider.starts_with("openai");

    let input = to_responses_input(&messages);

    let instructions = messages.iter().find_map(|m| match m {
        Message::System { content } => Some(content.clone()),
        _ => None,
    });

    let mut body = json!({
        "model": config.model,
        "input": input,
        "stream": true,
    });
    if is_openai {
        body["store"] = json!(false);
    }
    if let Some(inst) = instructions {
        if !inst.is_empty() {
            body["instructions"] = json!(inst);
        }
    }
    if config.temperature >= 0.0 {
        body["temperature"] = json!(config.temperature);
    }
    if config.max_tokens > 0 {
        body["max_output_tokens"] = json!(config.max_tokens);
    }

    let resp_tools = to_responses_tools(&tools);
    if !resp_tools.is_empty() {
        body["tools"] = Value::Array(resp_tools);
        body["tool_choice"] = json!("auto");
        if is_openai {
            body["parallel_tool_calls"] = json!(true);
        }
    }

    if config.thinking_enabled {
        let effort = match config.reasoning_effort.trim() {
            "" | "high" => "high",
            other => other,
        };
        if is_openai {
            body["reasoning"] = json!({ "effort": effort, "summary": "auto" });
        } else {
            body["reasoning"] = json!({ "effort": effort });
        }
    }

    merge_additional_params(&mut body, &config.additional_params);

    let mut req = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&body);
    if !config.api_key.is_empty() {
        req = req.bearer_auth(config.api_key.trim());
    }

    let response = req
        .send()
        .await
        .map_err(|e| anyhow!("连接 Responses API 失败: {url}: {e}"))?;

    crate::shared::sse::sse_stream(response, std::sync::Arc::new(extract_responses_chunks)).await
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

    #[test]
    fn to_responses_tools_conversion() {
        let tools = vec![json!({
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get weather",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "location": { "type": "string" }
                    }
                },
                "strict": true
            }
        })];
        let resp = to_responses_tools(&tools);
        assert_eq!(resp.len(), 1);
        assert_eq!(resp[0]["type"], "function");
        assert_eq!(resp[0]["name"], "get_weather");
        assert_eq!(resp[0]["strict"], true);
        assert!(resp[0].get("function").is_none());
    }

    #[test]
    fn to_responses_tools_flat_function_passthrough() {
        let tools = vec![json!({
            "type": "function",
            "name": "search",
            "description": "Search the web",
            "parameters": { "type": "object", "properties": {} }
        })];
        let resp = to_responses_tools(&tools);
        assert_eq!(resp.len(), 1);
        assert_eq!(resp[0]["name"], "search");
    }

    #[test]
    fn to_responses_tools_builtin_passthrough() {
        let tools = vec![
            json!({ "type": "web_search_preview" }),
            json!({ "type": "file_search", "vector_store_ids": ["vs_123"], "max_num_results": 20 }),
            json!({ "type": "code_interpreter" }),
        ];
        let resp = to_responses_tools(&tools);
        assert_eq!(resp.len(), 3);
        assert_eq!(resp[0]["type"], "web_search_preview");
        assert_eq!(resp[1]["type"], "file_search");
        assert_eq!(resp[1]["vector_store_ids"][0], "vs_123");
        assert_eq!(resp[2]["type"], "code_interpreter");
    }

    #[test]
    fn to_responses_tools_mixed() {
        let tools = vec![
            json!({
                "type": "function",
                "function": { "name": "f1", "parameters": {} }
            }),
            json!({ "type": "web_search_preview" }),
        ];
        let resp = to_responses_tools(&tools);
        assert_eq!(resp.len(), 2);
        assert_eq!(resp[0]["type"], "function");
        assert_eq!(resp[0]["name"], "f1");
        assert_eq!(resp[1]["type"], "web_search_preview");
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
        assert_eq!(arr[0]["data"], "data:audio/mp3;base64,AAAA");
    }

    #[test]
    fn build_content_image_as_input_image() {
        let parts = vec![UserContent::Image {
            url: "https://example.com/img.jpg".into(),
        }];
        let content = build_content_from_user(&parts);
        let arr = content.as_array().unwrap();
        assert_eq!(arr[0]["type"], "input_image");
        assert_eq!(arr[0]["image_url"], "https://example.com/img.jpg");
    }

    #[test]
    fn system_message_extracted_as_instructions() {
        let msgs = vec![
            Message::system("You are helpful"),
            Message::user_text("hello"),
        ];
        let input = to_responses_input(&msgs);
        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["role"], "system");
    }

    // ── 文本 delta ──

    #[test]
    fn extract_text_delta() {
        let data = r#"{"type":"response.output_text.delta","delta":"Hello"}"#;
        let chunks = extract_responses_chunks(data);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Text(t) if t == "Hello"));
    }

    // ── reasoning delta ──

    #[test]
    fn extract_reasoning_delta() {
        let data = r#"{"type":"response.reasoning_summary_text.delta","delta":"Let me think..."}"#;
        let chunks = extract_responses_chunks(data);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Thinking(t) if t == "Let me think..."));
    }

    #[test]
    fn extract_reasoning_empty_delta_returns_empty() {
        let data = r#"{"type":"response.reasoning_summary_text.delta","delta":""}"#;
        assert!(extract_responses_chunks(data).is_empty());
    }

    // ── refusal delta ──

    #[test]
    fn extract_refusal_delta() {
        let data = r#"{"type":"response.refusal.delta","delta":"I cannot help with that."}"#;
        let chunks = extract_responses_chunks(data);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Text(t) if t == "I cannot help with that."));
    }

    // ── function call ──

    #[test]
    fn extract_function_call_added() {
        let data = r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","call_id":"call_1","name":"search"}}"#;
        let chunks = extract_responses_chunks(data);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::ToolCallStart { index: 0, ref id, ref name } if id == "call_1" && name == "search"));
    }

    #[test]
    fn extract_function_call_args_delta() {
        let data = r#"{"type":"response.function_call_arguments.delta","output_index":0,"delta":"{\"q\":"}"#;
        let chunks = extract_responses_chunks(data);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::ToolCallDelta { index: 0, ref arguments } if arguments == "{\"q\":"));
    }

    #[test]
    fn extract_function_call_args_done_returns_empty() {
        let data = r#"{"type":"response.function_call_arguments.done","output_index":0,"arguments":"{\"q\":\"rust\"}"}"#;
        assert!(extract_responses_chunks(data).is_empty());
    }

    #[test]
    fn extract_output_item_done_function_call() {
        let data = r#"{"type":"response.output_item.done","output_index":1,"item":{"type":"function_call","id":"fc_1","call_id":"call_2","name":"read_file","arguments":"{\"path\":\"src/main.rs\"}","status":"completed"}}"#;
        let chunks = extract_responses_chunks(data);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::ToolCallDelta { index: 1, ref arguments } if arguments == r#"{"path":"src/main.rs"}"#));
    }

    #[test]
    fn extract_output_item_done_message_returns_empty() {
        let data = r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hi"}]}}"#;
        assert!(extract_responses_chunks(data).is_empty());
    }

    // ── 完成 / 错误 ──

    #[test]
    fn extract_completed_returns_usage_and_done() {
        let data = r#"{"type":"response.completed","response":{"output":[{"type":"message"}],"usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}}}"#;
        let chunks = extract_responses_chunks(data);
        assert_eq!(chunks.len(), 2);
        assert!(matches!(&chunks[0], StreamChunk::Usage(_)));
        assert!(matches!(&chunks[1], StreamChunk::Done { ref finish_reason } if finish_reason == "stop"));
    }

    #[test]
    fn extract_completed_with_tool_calls_returns_usage_and_done() {
        let data = r#"{"type":"response.completed","response":{"output":[{"type":"function_call","call_id":"c1","name":"f","arguments":"{}"}],"usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}}}"#;
        let chunks = extract_responses_chunks(data);
        assert_eq!(chunks.len(), 2);
        assert!(matches!(&chunks[0], StreamChunk::Usage(_)));
        assert!(matches!(&chunks[1], StreamChunk::Done { ref finish_reason } if finish_reason == "tool_calls"));
    }

    #[test]
    fn extract_completed_no_usage_returns_done_only() {
        let data = r#"{"type":"response.completed","response":{"output":[{"type":"message"}]}}"#;
        let chunks = extract_responses_chunks(data);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Done { ref finish_reason } if finish_reason == "stop"));
    }

    #[test]
    fn extract_failed() {
        let data = r#"{"type":"response.failed","response":{"status_details":{"error":{"message":"rate limit"}}}}"#;
        let chunks = extract_responses_chunks(data);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Error(ref msg) if msg == "rate limit"));
    }

    #[test]
    fn extract_error_event() {
        let data = r#"{"type":"error","message":"invalid request"}"#;
        let chunks = extract_responses_chunks(data);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], StreamChunk::Error(ref msg) if msg == "invalid request"));
    }

    // ── 忽略的事件 ──

    #[test]
    fn extract_lifecycle_events_return_empty() {
        for event in [
            r#"{"type":"response.created","response":{}}"#,
            r#"{"type":"response.in_progress","response":{}}"#,
            r#"{"type":"response.output_text.done","text":"hi"}"#,
            r#"{"type":"response.content_part.added"}"#,
            r#"{"type":"response.content_part.done"}"#,
            r#"{"type":"response.reasoning_summary_part.added"}"#,
            r#"{"type":"response.reasoning_summary_part.done"}"#,
            r#"{"type":"response.reasoning_summary_text.done","text":"ok"}"#,
            r#"{"type":"response.refusal.done","refusal":"no"}"#,
        ] {
            assert!(extract_responses_chunks(event).is_empty(), "expected empty for {event}");
        }
    }

    #[test]
    fn extract_unknown_event_returns_empty() {
        let data = r#"{"type":"response.some_future_event","data":"x"}"#;
        assert!(extract_responses_chunks(data).is_empty());
    }
}
