//! OpenAI Responses API 流式适配器。
//!
//! POST `{base}/responses` + SSE → [`ChatStream`]。
//! 分发入口在 [`crate::http_stream::chat_stream_for_provider`]，
//! 通过 [`crate::profile::ApiMode::Responses`] 路由到本模块。

use std::sync::Arc;

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::{json, Value};

use super::chat::{openai_compatible_base, parse_openai_usage};
use crate::http_stream::{merge_additional_params, resolve_base, sse_chat_stream};
use crate::trait_::{
    ChatChunk, ChatContentPart, ChatMessage, ChatStream, ProviderConfig, ToolCallDeltaChunk,
};

// ---------------------------------------------------------------------------
// 消息 / 工具转换
// ---------------------------------------------------------------------------

/// 将内部 [`ChatMessage`] 转为 Responses API `input` 数组。
///
/// Responses API 与 Chat Completions 的主要区别：
/// - tool 角色 → `{ type: "function_call_output", call_id, output }`
/// - assistant + tool_calls → 展开为 `{ type: "function_call", id, name, arguments }` 项
/// - 其余走 `{ role, content }` 不变
fn to_responses_input(messages: &[ChatMessage]) -> Vec<Value> {
    let mut input = Vec::with_capacity(messages.len());
    for m in messages {
        if m.role == "tool" {
            let call_id = m
                .tool_call_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("");
            input.push(json!({
                "type": "function_call_output",
                "call_id": call_id,
                "output": m.content,
            }));
            continue;
        }

        if m.role == "assistant" {
            if let Some(ref calls) = m.tool_calls {
                for c in calls {
                    let args = match &c.arguments {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    input.push(json!({
                        "type": "function_call",
                        "id": c.id,
                        "call_id": c.id,
                        "name": c.name,
                        "arguments": args,
                    }));
                }
                if m.content.is_empty() {
                    continue;
                }
            }
        }

        let content = build_content(m);
        input.push(json!({
            "role": m.role,
            "content": content,
        }));
    }
    input
}

fn build_content(m: &ChatMessage) -> Value {
    if let Some(ref parts) = m.parts {
        if !parts.is_empty() {
            let arr: Vec<Value> = parts
                .iter()
                .map(|p| match p {
                    ChatContentPart::Text { text } => {
                        json!({ "type": "input_text", "text": text })
                    }
                    ChatContentPart::ImageUrl { url } => json!({
                        "type": "input_image",
                        "image_url": url,
                    }),
                    ChatContentPart::AudioUrl { url, .. } => json!({
                        "type": "input_audio",
                        "data": url,
                    }),
                    ChatContentPart::VideoUrl { url, .. } => json!({
                        "type": "input_video",
                        "video_url": url,
                    }),
                    ChatContentPart::DocumentUrl { url, .. } => json!({
                        "type": "input_file",
                        "file_url": url,
                    }),
                })
                .collect();
            return Value::Array(arr);
        }
    }
    json!(m.content)
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

/// 解析 Responses API SSE `data:` 负载为 [`ChatChunk`]。
fn extract_responses_delta(data: &str) -> Option<ChatChunk> {
    let v: Value = serde_json::from_str(data).ok()?;

    let event_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match event_type {
        // ── 文本 ──
        "response.output_text.delta" => {
            let delta = v.get("delta").and_then(|d| d.as_str())?;
            if delta.is_empty() {
                return None;
            }
            Some(ChatChunk {
                token: Some(delta.to_string()),
                ..Default::default()
            })
        }

        // ── reasoning ──
        "response.reasoning_summary_text.delta" => {
            let delta = v.get("delta").and_then(|d| d.as_str())?;
            if delta.is_empty() {
                return None;
            }
            Some(ChatChunk {
                reasoning: Some(delta.to_string()),
                ..Default::default()
            })
        }

        // ── refusal（模型拒绝回答，作为文本下发） ──
        "response.refusal.delta" => {
            let delta = v.get("delta").and_then(|d| d.as_str())?;
            if delta.is_empty() {
                return None;
            }
            Some(ChatChunk {
                token: Some(delta.to_string()),
                ..Default::default()
            })
        }

        // ── 新建 function_call 输出项 ──
        "response.output_item.added" => {
            let item = v.get("item")?;
            if item.get("type").and_then(|t| t.as_str()) != Some("function_call") {
                return None;
            }
            let index = v
                .get("output_index")
                .and_then(|i| i.as_u64())
                .unwrap_or(0) as u32;
            let id = item
                .get("call_id")
                .or_else(|| item.get("id"))
                .and_then(|s| s.as_str())
                .map(str::to_string);
            let name = item.get("name").and_then(|s| s.as_str()).map(str::to_string);
            Some(ChatChunk {
                tool_call_deltas: vec![ToolCallDeltaChunk {
                    index,
                    id,
                    name,
                    arguments: None,
                    signature: None,
                }],
                ..Default::default()
            })
        }

        // ── function_call 完成（兜底：即使 delta 丢包也能恢复完整调用） ──
        "response.output_item.done" => {
            let item = v.get("item")?;
            if item.get("type").and_then(|t| t.as_str()) != Some("function_call") {
                return None;
            }
            let index = v
                .get("output_index")
                .and_then(|i| i.as_u64())
                .unwrap_or(0) as u32;
            let id = item
                .get("call_id")
                .or_else(|| item.get("id"))
                .and_then(|s| s.as_str())
                .map(str::to_string);
            let name = item.get("name").and_then(|s| s.as_str()).map(str::to_string);
            let arguments = item
                .get("arguments")
                .and_then(|a| a.as_str())
                .map(str::to_string);
            Some(ChatChunk {
                tool_call_deltas: vec![ToolCallDeltaChunk {
                    index,
                    id,
                    name,
                    arguments,
                    signature: None,
                }],
                ..Default::default()
            })
        }

        // ── function call 参数 delta ──
        "response.function_call_arguments.delta" => {
            let delta = v.get("delta").and_then(|d| d.as_str())?;
            let index = v
                .get("output_index")
                .and_then(|i| i.as_u64())
                .unwrap_or(0) as u32;
            Some(ChatChunk {
                tool_call_deltas: vec![ToolCallDeltaChunk {
                    index,
                    id: None,
                    name: None,
                    arguments: Some(delta.to_string()),
                    signature: None,
                }],
                ..Default::default()
            })
        }

        // ── function call 参数完成（不发 finish_reason，由 completed 统一处理） ──
        "response.function_call_arguments.done" => None,

        // ── 响应完成 ──
        "response.completed" => {
            let resp = v.get("response");
            let usage = resp.and_then(parse_openai_usage);
            let has_tool_calls = resp
                .and_then(|r| r.get("output"))
                .and_then(|o| o.as_array())
                .map(|arr| {
                    arr.iter()
                        .any(|item| item.get("type").and_then(|t| t.as_str()) == Some("function_call"))
                })
                .unwrap_or(false);
            let finish = if has_tool_calls {
                "tool_calls"
            } else {
                "stop"
            };
            Some(ChatChunk {
                finish_reason: Some(finish.to_string()),
                usage,
                ..Default::default()
            })
        }

        // ── 错误 / 失败 ──
        "response.failed" => {
            let msg = v
                .pointer("/response/status_details/error/message")
                .or_else(|| v.pointer("/response/error/message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Responses API 请求失败");
            Some(ChatChunk {
                finish_reason: Some(format!("error:{msg}")),
                ..Default::default()
            })
        }

        "response.incomplete" => Some(ChatChunk {
            finish_reason: Some("stop".to_string()),
            ..Default::default()
        }),

        "error" => {
            let msg = v
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("未知错误");
            Some(ChatChunk {
                finish_reason: Some(format!("error:{msg}")),
                ..Default::default()
            })
        }

        // 生命周期 / 边界事件 — 不需要转为 ChatChunk
        "response.created"
        | "response.in_progress"
        | "response.queued"
        | "response.output_text.done"
        | "response.content_part.added"
        | "response.content_part.done"
        | "response.reasoning_summary_part.added"
        | "response.reasoning_summary_part.done"
        | "response.reasoning_summary_text.done"
        | "response.refusal.done" => None,

        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 主入口
// ---------------------------------------------------------------------------

/// OpenAI Responses API 流式聊天。
pub async fn responses_chat_stream(
    client: &Client,
    provider: &str,
    messages: Vec<ChatMessage>,
    tools: Vec<Value>,
    config: &ProviderConfig,
) -> Result<ChatStream> {
    if config.api_key.trim().is_empty() {
        return Err(anyhow!("Responses API Key 为空"));
    }

    let base = openai_compatible_base(&resolve_base(config, provider));
    let url = format!("{base}/responses");

    let is_openai = provider.starts_with("openai");

    let input = to_responses_input(&messages);

    let instructions = messages
        .iter()
        .find(|m| m.role == "system")
        .map(|m| m.content.clone());

    let mut body = json!({
        "model": config.model,
        "input": input,
        "stream": true,
    });
    // `store` / `parallel_tool_calls` 仅 OpenAI 官方支持为请求参数
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

    sse_chat_stream(response, Arc::new(extract_responses_delta)).await
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trait_::ChatToolCall;

    // ── 消息转换 ──

    #[test]
    fn to_responses_input_user_and_assistant() {
        let msgs = vec![
            ChatMessage::text("user", "hello"),
            ChatMessage::text("assistant", "hi there"),
        ];
        let input = to_responses_input(&msgs);
        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"], "hello");
        assert_eq!(input[1]["role"], "assistant");
    }

    #[test]
    fn to_responses_input_tool_result() {
        let msgs = vec![ChatMessage {
            role: "tool".into(),
            content: r#"{"result": 42}"#.into(),
            tool_call_id: Some("call_abc".into()),
            name: Some("get_data".into()),
            parts: None,
            tool_calls: None,
            reasoning: None,
            thought_signature: None,
            is_error: false,
        }];
        let input = to_responses_input(&msgs);
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "function_call_output");
        assert_eq!(input[0]["call_id"], "call_abc");
    }

    #[test]
    fn to_responses_input_assistant_tool_calls() {
        let msgs = vec![ChatMessage {
            role: "assistant".into(),
            content: String::new(),
            tool_calls: Some(vec![ChatToolCall {
                id: "call_123".into(),
                name: "get_weather".into(),
                arguments: json!({"location": "Paris"}),
                signature: None,
            }]),
            tool_call_id: None,
            name: None,
            parts: None,
            reasoning: None,
            thought_signature: None,
            is_error: false,
        }];
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
        let msg = ChatMessage::user_parts(
            "what is in this file?",
            vec![
                ChatContentPart::Text {
                    text: "what is in this file?".into(),
                },
                ChatContentPart::DocumentUrl {
                    url: "https://example.com/doc.pdf".into(),
                    mime_type: "application/pdf".into(),
                },
            ],
        );
        let content = build_content(&msg);
        let arr = content.as_array().unwrap();
        assert_eq!(arr[0]["type"], "input_text");
        assert_eq!(arr[1]["type"], "input_file");
        assert_eq!(arr[1]["file_url"], "https://example.com/doc.pdf");
    }

    #[test]
    fn build_content_audio_as_input_audio() {
        let msg = ChatMessage::user_parts(
            "transcribe",
            vec![ChatContentPart::AudioUrl {
                url: "data:audio/mp3;base64,AAAA".into(),
                mime_type: "audio/mp3".into(),
            }],
        );
        let content = build_content(&msg);
        let arr = content.as_array().unwrap();
        assert_eq!(arr[0]["type"], "input_audio");
        assert_eq!(arr[0]["data"], "data:audio/mp3;base64,AAAA");
    }

    #[test]
    fn build_content_image_as_input_image() {
        let msg = ChatMessage::user_parts(
            "describe",
            vec![ChatContentPart::ImageUrl {
                url: "https://example.com/img.jpg".into(),
            }],
        );
        let content = build_content(&msg);
        let arr = content.as_array().unwrap();
        assert_eq!(arr[0]["type"], "input_image");
        assert_eq!(arr[0]["image_url"], "https://example.com/img.jpg");
    }

    #[test]
    fn system_message_extracted_as_instructions() {
        let msgs = vec![
            ChatMessage::text("system", "You are helpful"),
            ChatMessage::text("user", "hello"),
        ];
        let input = to_responses_input(&msgs);
        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["role"], "system");
    }

    // ── 文本 delta ──

    #[test]
    fn extract_text_delta() {
        let data = r#"{"type":"response.output_text.delta","delta":"Hello"}"#;
        let chunk = extract_responses_delta(data).unwrap();
        assert_eq!(chunk.token.as_deref(), Some("Hello"));
    }

    // ── reasoning delta ──

    #[test]
    fn extract_reasoning_delta() {
        let data = r#"{"type":"response.reasoning_summary_text.delta","delta":"Let me think..."}"#;
        let chunk = extract_responses_delta(data).unwrap();
        assert_eq!(chunk.reasoning.as_deref(), Some("Let me think..."));
        assert!(chunk.token.is_none());
    }

    #[test]
    fn extract_reasoning_empty_delta_returns_none() {
        let data = r#"{"type":"response.reasoning_summary_text.delta","delta":""}"#;
        assert!(extract_responses_delta(data).is_none());
    }

    // ── refusal delta ──

    #[test]
    fn extract_refusal_delta() {
        let data = r#"{"type":"response.refusal.delta","delta":"I cannot help with that."}"#;
        let chunk = extract_responses_delta(data).unwrap();
        assert_eq!(chunk.token.as_deref(), Some("I cannot help with that."));
    }

    // ── function call ──

    #[test]
    fn extract_function_call_added() {
        let data = r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","call_id":"call_1","name":"search"}}"#;
        let chunk = extract_responses_delta(data).unwrap();
        assert_eq!(chunk.tool_call_deltas.len(), 1);
        assert_eq!(chunk.tool_call_deltas[0].id.as_deref(), Some("call_1"));
        assert_eq!(chunk.tool_call_deltas[0].name.as_deref(), Some("search"));
    }

    #[test]
    fn extract_function_call_args_delta() {
        let data = r#"{"type":"response.function_call_arguments.delta","output_index":0,"delta":"{\"q\":"}"#;
        let chunk = extract_responses_delta(data).unwrap();
        assert_eq!(chunk.tool_call_deltas.len(), 1);
        assert_eq!(
            chunk.tool_call_deltas[0].arguments.as_deref(),
            Some("{\"q\":")
        );
    }

    #[test]
    fn extract_function_call_args_done_returns_none() {
        let data = r#"{"type":"response.function_call_arguments.done","output_index":0,"arguments":"{\"q\":\"rust\"}"}"#;
        assert!(extract_responses_delta(data).is_none());
    }

    #[test]
    fn extract_output_item_done_function_call() {
        let data = r#"{"type":"response.output_item.done","output_index":1,"item":{"type":"function_call","id":"fc_1","call_id":"call_2","name":"read_file","arguments":"{\"path\":\"src/main.rs\"}","status":"completed"}}"#;
        let chunk = extract_responses_delta(data).unwrap();
        assert_eq!(chunk.tool_call_deltas.len(), 1);
        assert_eq!(chunk.tool_call_deltas[0].id.as_deref(), Some("call_2"));
        assert_eq!(
            chunk.tool_call_deltas[0].name.as_deref(),
            Some("read_file")
        );
        assert_eq!(
            chunk.tool_call_deltas[0].arguments.as_deref(),
            Some("{\"path\":\"src/main.rs\"}")
        );
    }

    #[test]
    fn extract_output_item_done_message_returns_none() {
        let data = r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"hi"}]}}"#;
        assert!(extract_responses_delta(data).is_none());
    }

    // ── 完成 / 错误 ──

    #[test]
    fn extract_completed_text_only() {
        let data = r#"{"type":"response.completed","response":{"output":[{"type":"message"}],"usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}}}"#;
        let chunk = extract_responses_delta(data).unwrap();
        assert_eq!(chunk.finish_reason.as_deref(), Some("stop"));
        assert!(chunk.usage.is_some());
    }

    #[test]
    fn extract_completed_with_tool_calls() {
        let data = r#"{"type":"response.completed","response":{"output":[{"type":"function_call","call_id":"c1","name":"f","arguments":"{}"}],"usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}}}"#;
        let chunk = extract_responses_delta(data).unwrap();
        assert_eq!(chunk.finish_reason.as_deref(), Some("tool_calls"));
    }

    #[test]
    fn extract_failed() {
        let data =
            r#"{"type":"response.failed","response":{"status_details":{"error":{"message":"rate limit"}}}}"#;
        let chunk = extract_responses_delta(data).unwrap();
        assert!(chunk
            .finish_reason
            .as_deref()
            .unwrap()
            .starts_with("error:"));
    }

    #[test]
    fn extract_error_event() {
        let data = r#"{"type":"error","message":"invalid request"}"#;
        let chunk = extract_responses_delta(data).unwrap();
        assert_eq!(
            chunk.finish_reason.as_deref(),
            Some("error:invalid request")
        );
    }

    // ── 忽略的事件 ──

    #[test]
    fn extract_lifecycle_events_return_none() {
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
            assert!(
                extract_responses_delta(event).is_none(),
                "expected None for {event}"
            );
        }
    }

    #[test]
    fn extract_unknown_event_returns_none() {
        let data = r#"{"type":"response.some_future_event","data":"x"}"#;
        assert!(extract_responses_delta(data).is_none());
    }
}
