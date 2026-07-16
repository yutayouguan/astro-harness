//! Google Gemini 原生 streamGenerateContent 流式聊天。
//!
//! 与 Interactions API 隔离，直接对接 `POST /v1beta/models/{model}:streamGenerateContent`，
//! 支持 `function_declarations` 工具调用、`system_instruction`、思考（thought）部分。

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::http_stream::merge_additional_params;
use crate::streaming::Usage;
use crate::tool_format::openai_tools_to_gemini_native;
use crate::trait_::{
    ChatChunk, ChatContentPart, ChatMessage, ChatStream, ProviderConfig, ToolCallDeltaChunk,
};
use super::veo_http::google_native_base;

/// 构造 streamGenerateContent SSE 接入点（API key 作 query 参数）。
fn stream_generate_content_url(config: &ProviderConfig, model: &str) -> String {
    let base = google_native_base(config);
    format!(
        "{base}/v1beta/models/{model}:streamGenerateContent?alt=sse&key={}",
        config.api_key
    )
}

/// 将内部消息列表转为 Gemini native `contents` 数组，并抽出 `system_instruction` 文本。
fn to_gemini_contents(messages: &[ChatMessage]) -> (Option<String>, Vec<Value>) {
    let mut system_parts: Vec<String> = Vec::new();
    let mut contents: Vec<Value> = Vec::new();

    for m in messages {
        match m.role.as_str() {
            "system" => {
                let t = m.content.trim();
                if !t.is_empty() {
                    system_parts.push(t.to_string());
                }
            }
            "user" => {
                let parts = build_user_parts(m);
                contents.push(json!({ "role": "user", "parts": parts }));
            }
            "assistant" => {
                let mut parts: Vec<Value> = Vec::new();
                if !m.content.is_empty() {
                    parts.push(json!({ "text": m.content }));
                }
                if let Some(ref calls) = m.tool_calls {
                    for c in calls {
                        let args = normalize_args(&c.arguments);
                        parts.push(json!({
                            "functionCall": {
                                "name": c.name,
                                "args": args,
                            }
                        }));
                    }
                }
                if !parts.is_empty() {
                    contents.push(json!({ "role": "model", "parts": parts }));
                }
            }
            "tool" => {
                let name = m.name.as_deref().unwrap_or("");
                let response = serde_json::from_str::<Value>(&m.content)
                    .unwrap_or_else(|_| json!({ "content": m.content }));
                let part = json!({
                    "functionResponse": {
                        "name": name,
                        "response": response,
                    }
                });
                // Gemini requires all functionResponses for one model turn in a single user turn
                if !try_append_fn_response(&mut contents, part.clone()) {
                    contents.push(json!({ "role": "user", "parts": [part] }));
                }
            }
            _ => {
                if !m.content.is_empty() {
                    contents.push(json!({
                        "role": "user",
                        "parts": [{ "text": m.content }],
                    }));
                }
            }
        }
    }

    let system = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };

    (system, contents)
}

fn build_user_parts(m: &ChatMessage) -> Vec<Value> {
    if let Some(ref parts) = m.parts {
        if !parts.is_empty() {
            return parts.iter().map(part_to_gemini).collect();
        }
    }
    vec![json!({ "text": m.content })]
}

fn part_to_gemini(p: &ChatContentPart) -> Value {
    match p {
        ChatContentPart::Text { text } => json!({ "text": text }),
        ChatContentPart::ImageUrl { url } => {
            if let Some(rest) = url.strip_prefix("data:") {
                if let Some((meta, data)) = rest.split_once(',') {
                    let mime = meta.split(';').next().unwrap_or("image/jpeg");
                    return json!({ "inlineData": { "mimeType": mime, "data": data } });
                }
            }
            json!({ "fileData": { "fileUri": url } })
        }
    }
}

fn normalize_args(args: &Value) -> Value {
    if let Some(s) = args.as_str() {
        serde_json::from_str(s).unwrap_or_else(|_| json!({}))
    } else {
        args.clone()
    }
}

/// 若最后一个 turn 是含 functionResponse 的 user turn，则追加 part 并返回 true。
fn try_append_fn_response(contents: &mut Vec<Value>, part: Value) -> bool {
    let is_fn_resp_turn = contents.last().map_or(false, |last| {
        last.get("role").and_then(|r| r.as_str()) == Some("user")
            && last
                .get("parts")
                .and_then(|p| p.as_array())
                .map_or(false, |arr| {
                    arr.iter().any(|p| p.get("functionResponse").is_some())
                })
    });
    if is_fn_resp_turn {
        if let Some(last) = contents.last_mut() {
            if let Some(arr) = last.get_mut("parts").and_then(|p| p.as_array_mut()) {
                arr.push(part);
                return true;
            }
        }
    }
    false
}

/// 构造 Gemini native `generateContent` 请求体。
fn build_gemini_native_chat_body(
    messages: &[ChatMessage],
    tools: &[Value],
    config: &ProviderConfig,
) -> Value {
    let (system_text, contents) = to_gemini_contents(messages);

    let mut gen = serde_json::Map::new();
    gen.insert("temperature".into(), json!(config.temperature));
    if config.max_tokens > 0 {
        gen.insert("maxOutputTokens".into(), json!(config.max_tokens));
    }

    let mut body = json!({
        "contents": contents,
        "generationConfig": Value::Object(gen),
    });

    if let Some(sys) = system_text {
        body["system_instruction"] = json!({ "parts": [{ "text": sys }] });
    }

    let gemini_tools = openai_tools_to_gemini_native(tools);
    if !gemini_tools.is_empty() {
        body["tools"] = Value::Array(gemini_tools);
    }

    merge_additional_params(&mut body, &config.additional_params);
    body
}

/// 解析 Gemini native SSE `data:` 负载为 [`ChatChunk`]（公开以便单测）。
pub fn extract_gemini_native_delta(data: &str) -> Option<ChatChunk> {
    let v: Value = serde_json::from_str(data).ok()?;

    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Gemini native 错误");
        return Some(ChatChunk {
            finish_reason: Some(format!("error:{msg}")),
            ..Default::default()
        });
    }

    let candidate = v.get("candidates")?.as_array()?.first()?;

    let finish_reason = candidate
        .get("finishReason")
        .and_then(|r| r.as_str())
        .map(|r| match r {
            "STOP" => "stop".to_string(),
            "FUNCTION_CALL" => "tool_calls".to_string(),
            "MAX_TOKENS" => "length".to_string(),
            other => other.to_ascii_lowercase(),
        });

    let parts = candidate
        .pointer("/content/parts")
        .and_then(|p| p.as_array());

    let mut token: Option<String> = None;
    let mut reasoning: Option<String> = None;
    let mut tool_call_deltas: Vec<ToolCallDeltaChunk> = Vec::new();

    if let Some(parts) = parts {
        for (idx, part) in parts.iter().enumerate() {
            if let Some(fc) = part.get("functionCall") {
                let name = fc
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                let args = fc.get("args").cloned().unwrap_or_else(|| json!({}));
                let id = format!("fc_{idx}_{name}");
                tool_call_deltas.push(ToolCallDeltaChunk {
                    index: idx as u32,
                    id: Some(id),
                    name: Some(name),
                    arguments: Some(args.to_string()),
                });
            } else if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                if !text.is_empty() {
                    // Gemini 2.5: thinking parts carry `"thought": true`
                    let is_thought = part
                        .get("thought")
                        .and_then(|b| b.as_bool())
                        .unwrap_or(false);
                    if is_thought {
                        reasoning.get_or_insert_with(String::new).push_str(text);
                    } else {
                        token.get_or_insert_with(String::new).push_str(text);
                    }
                }
            }
        }
    }

    let usage = v.get("usageMetadata").and_then(|u| {
        let prompt = u
            .get("promptTokenCount")
            .and_then(|x| x.as_u64())
            .unwrap_or(0) as u32;
        let output = u
            .get("candidatesTokenCount")
            .and_then(|x| x.as_u64())
            .unwrap_or(0) as u32;
        let cache_read = u
            .get("cachedContentTokenCount")
            .and_then(|x| x.as_u64())
            .unwrap_or(0) as u32;
        if prompt == 0 && output == 0 {
            return None;
        }
        Some(Usage {
            input_tokens: prompt.saturating_sub(cache_read),
            output_tokens: output,
            cache_read_tokens: cache_read,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            request_count: 1,
        })
    });

    if token.is_none()
        && reasoning.is_none()
        && tool_call_deltas.is_empty()
        && finish_reason.is_none()
        && usage.is_none()
    {
        return None;
    }

    Some(ChatChunk {
        token,
        reasoning,
        finish_reason,
        tool_call_deltas,
        usage,
        interaction_id: None,
    })
}

/// Gemini native 流式聊天主入口。
pub async fn gemini_native_chat_stream(
    client: &Client,
    messages: Vec<ChatMessage>,
    tools: Vec<Value>,
    config: &ProviderConfig,
) -> Result<ChatStream> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    let url = stream_generate_content_url(config, config.model.trim());
    let body = build_gemini_native_chat_body(&messages, &tools, config);

    let response = client
        .post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("Gemini native 连接失败: {url}"))?;

    crate::http_stream::sse_chat_stream(response, Arc::new(extract_gemini_native_delta)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trait_::ChatToolCall;

    #[test]
    fn to_gemini_contents_separates_system() {
        let messages = vec![
            ChatMessage::text("system", "You are helpful."),
            ChatMessage::text("user", "hello"),
        ];
        let (sys, contents) = to_gemini_contents(&messages);
        assert_eq!(sys.as_deref(), Some("You are helpful."));
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[0]["parts"][0]["text"], "hello");
    }

    #[test]
    fn to_gemini_contents_assistant_text() {
        let messages = vec![
            ChatMessage::text("user", "hi"),
            ChatMessage::text("assistant", "Hello!"),
        ];
        let (_, contents) = to_gemini_contents(&messages);
        assert_eq!(contents[1]["role"], "model");
        assert_eq!(contents[1]["parts"][0]["text"], "Hello!");
    }

    #[test]
    fn to_gemini_contents_encodes_function_call_and_response() {
        let messages = vec![
            ChatMessage::text("user", "weather?"),
            ChatMessage {
                role: "assistant".into(),
                content: String::new(),
                parts: None,
                tool_calls: Some(vec![ChatToolCall {
                    id: "fc_1".into(),
                    name: "get_weather".into(),
                    arguments: json!({ "location": "Boston" }),
                }]),
                tool_call_id: None,
                name: None,
            },
            ChatMessage {
                role: "tool".into(),
                content: r#"{"temperature": "52F"}"#.into(),
                parts: None,
                tool_calls: None,
                tool_call_id: Some("fc_1".into()),
                name: Some("get_weather".into()),
            },
        ];
        let (_, contents) = to_gemini_contents(&messages);
        assert_eq!(contents[1]["role"], "model");
        assert!(contents[1]["parts"][0].get("functionCall").is_some());
        assert_eq!(contents[2]["role"], "user");
        assert!(contents[2]["parts"][0].get("functionResponse").is_some());
    }

    #[test]
    fn parallel_tool_responses_grouped_in_single_user_turn() {
        let messages = vec![
            ChatMessage::text("user", "weather?"),
            ChatMessage {
                role: "assistant".into(),
                content: String::new(),
                parts: None,
                tool_calls: Some(vec![
                    ChatToolCall {
                        id: "a".into(),
                        name: "get_weather".into(),
                        arguments: json!({ "city": "A" }),
                    },
                    ChatToolCall {
                        id: "b".into(),
                        name: "get_weather".into(),
                        arguments: json!({ "city": "B" }),
                    },
                ]),
                tool_call_id: None,
                name: None,
            },
            ChatMessage {
                role: "tool".into(),
                content: "20C".into(),
                parts: None,
                tool_calls: None,
                tool_call_id: Some("a".into()),
                name: Some("get_weather".into()),
            },
            ChatMessage {
                role: "tool".into(),
                content: "25C".into(),
                parts: None,
                tool_calls: None,
                tool_call_id: Some("b".into()),
                name: Some("get_weather".into()),
            },
        ];
        let (_, contents) = to_gemini_contents(&messages);
        let last = contents.last().unwrap();
        assert_eq!(last["role"], "user");
        assert_eq!(last["parts"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn extract_text_chunk() {
        let data = r#"{"candidates":[{"content":{"role":"model","parts":[{"text":"Hello!"}]},"finishReason":"STOP"}]}"#;
        let chunk = extract_gemini_native_delta(data).unwrap();
        assert_eq!(chunk.token.as_deref(), Some("Hello!"));
        assert_eq!(chunk.finish_reason.as_deref(), Some("stop"));
        assert!(chunk.tool_call_deltas.is_empty());
    }

    #[test]
    fn extract_function_call_chunk() {
        let data = r#"{"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"name":"get_weather","args":{"city":"Boston"}}}]},"finishReason":"FUNCTION_CALL"}]}"#;
        let chunk = extract_gemini_native_delta(data).unwrap();
        assert_eq!(chunk.finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(chunk.tool_call_deltas.len(), 1);
        assert_eq!(
            chunk.tool_call_deltas[0].name.as_deref(),
            Some("get_weather")
        );
        let args = chunk.tool_call_deltas[0].arguments.as_deref().unwrap_or("");
        assert!(args.contains("Boston"));
    }

    #[test]
    fn extract_thought_chunk() {
        let data = r#"{"candidates":[{"content":{"role":"model","parts":[{"text":"thinking...","thought":true},{"text":"result"}]}}]}"#;
        let chunk = extract_gemini_native_delta(data).unwrap();
        assert_eq!(chunk.reasoning.as_deref(), Some("thinking..."));
        assert_eq!(chunk.token.as_deref(), Some("result"));
    }

    #[test]
    fn extract_usage_metadata() {
        let data = r#"{"candidates":[{"content":{"role":"model","parts":[{"text":"hi"}]}}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":5,"cachedContentTokenCount":2}}"#;
        let chunk = extract_gemini_native_delta(data).unwrap();
        let usage = chunk.usage.unwrap();
        assert_eq!(usage.input_tokens, 8); // 10 - 2 cached
        assert_eq!(usage.output_tokens, 5);
        assert_eq!(usage.cache_read_tokens, 2);
    }

    #[test]
    fn extract_error_chunk() {
        let data = r#"{"error":{"code":400,"message":"Invalid request","status":"INVALID_ARGUMENT"}}"#;
        let chunk = extract_gemini_native_delta(data).unwrap();
        assert!(chunk.finish_reason.as_deref().unwrap().starts_with("error:"));
        assert!(chunk.finish_reason.as_deref().unwrap().contains("Invalid request"));
    }

    #[test]
    fn build_body_has_system_instruction_and_tools() {
        let messages = vec![
            ChatMessage::text("system", "Be concise."),
            ChatMessage::text("user", "hi"),
        ];
        let tools = vec![json!({
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get weather",
                "parameters": { "type": "object", "properties": {} }
            }
        })];
        let config = ProviderConfig::default();
        let body = build_gemini_native_chat_body(&messages, &tools, &config);
        assert!(body.get("system_instruction").is_some());
        assert_eq!(body["system_instruction"]["parts"][0]["text"], "Be concise.");
        let tool_arr = body["tools"].as_array().unwrap();
        assert!(!tool_arr.is_empty());
        assert!(tool_arr[0].get("function_declarations").is_some());
    }
}
