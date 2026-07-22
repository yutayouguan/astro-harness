//! OpenAI Responses API 流式适配器。
//!
//! POST `{base}/responses` + SSE → [`ChatStream`]。
//! 分发入口已在 [`crate::http_stream::chat_stream_for_provider`] 接线。

use std::sync::Arc;

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::{json, Value};

use crate::http_stream::{
    merge_additional_params, openai_compatible_base, parse_openai_usage, resolve_base,
    sse_chat_stream,
};
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
                    ChatContentPart::AudioUrl { mime_type, .. } => json!({
                        "type": "input_text",
                        "text": format!(
                            "[audio attached: {}]",
                            if mime_type.trim().is_empty() { "audio/*" } else { mime_type }
                        )
                    }),
                    ChatContentPart::VideoUrl { mime_type, .. } => json!({
                        "type": "input_text",
                        "text": format!(
                            "[video attached: {}]",
                            if mime_type.trim().is_empty() { "video/*" } else { mime_type }
                        )
                    }),
                    ChatContentPart::DocumentUrl { mime_type, .. } => json!({
                        "type": "input_text",
                        "text": format!(
                            "[document attached: {}]",
                            if mime_type.trim().is_empty() { "application/pdf" } else { mime_type }
                        )
                    }),
                })
                .collect();
            return Value::Array(arr);
        }
    }
    json!(m.content)
}

/// Chat Completions 工具 schema → Responses API 工具 schema。
///
/// Chat Completions: `{ type:"function", function:{ name, description, parameters, strict } }`
/// Responses API:    `{ type:"function", name, description, parameters, strict }`
fn to_responses_tools(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|t| {
            let func = t.get("function")?;
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
            Some(Value::Object(out))
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
        // 文本 delta
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

        // 新建 function_call 输出项
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

        // function call 参数 delta
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

        // function call 参数完成
        "response.function_call_arguments.done" => {
            Some(ChatChunk {
                finish_reason: Some("tool_calls".to_string()),
                ..Default::default()
            })
        }

        // 响应完成
        "response.completed" => {
            let usage = v
                .get("response")
                .and_then(parse_openai_usage);
            Some(ChatChunk {
                finish_reason: Some("stop".to_string()),
                usage,
                ..Default::default()
            })
        }

        // 错误 / 失败
        "response.failed" => {
            let msg = v
                .pointer("/response/status_details/error/message")
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
        return Err(anyhow!("OpenAI Responses API Key 为空"));
    }

    let base = openai_compatible_base(&resolve_base(config, provider));
    let url = format!("{base}/responses");

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
    }

    merge_additional_params(&mut body, &config.additional_params);

    let response = client
        .post(&url)
        .bearer_auth(config.api_key.trim())
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow!("连接 OpenAI Responses API 失败: {url}: {e}"))?;

    sse_chat_stream(response, Arc::new(extract_responses_delta)).await
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trait_::ChatToolCall;

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
    fn extract_text_delta() {
        let data = r#"{"type":"response.output_text.delta","delta":"Hello"}"#;
        let chunk = extract_responses_delta(data).unwrap();
        assert_eq!(chunk.token.as_deref(), Some("Hello"));
    }

    #[test]
    fn extract_function_call_added() {
        let data = r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","call_id":"call_1","name":"search"}}"#;
        let chunk = extract_responses_delta(data).unwrap();
        assert_eq!(chunk.tool_call_deltas.len(), 1);
        assert_eq!(
            chunk.tool_call_deltas[0].id.as_deref(),
            Some("call_1")
        );
        assert_eq!(
            chunk.tool_call_deltas[0].name.as_deref(),
            Some("search")
        );
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
    fn extract_completed() {
        let data = r#"{"type":"response.completed","response":{"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}}"#;
        let chunk = extract_responses_delta(data).unwrap();
        assert_eq!(chunk.finish_reason.as_deref(), Some("stop"));
        assert!(chunk.usage.is_some());
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
    fn extract_unknown_event_returns_none() {
        let data = r#"{"type":"response.output_text.done","text":"hi"}"#;
        assert!(extract_responses_delta(data).is_none());
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
}
