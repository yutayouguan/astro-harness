//! Google Gemini Interactions 聊天适配器。
//!
//! 将内部 [`ChatMessage`] / OpenAI tools 编成 `POST /v1beta/interactions`，
//! 并把 SSE（`step.delta` 等）映射回 [`ChatChunk`]。
//!
//! 首轮将完整会话历史编成 `input`；工具多轮若配置了
//! [`ProviderConfig::previous_interaction_id`]，则只提交尾部 `function_result`，
//! 由服务端保留上一轮 `thought`/`signature`（Gemini 3 严格模式必需）。

use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
use reqwest::{Client, Response};
use serde_json::{json, Value};
use std::time::Duration;

use crate::http_stream::merge_additional_params;
use super::interactions_http::interactions_url;
use crate::streaming::Usage;
use crate::trait_::{
    ChatChunk, ChatContentPart, ChatMessage, ChatStream, ProviderConfig, ToolCallDeltaChunk,
};

const API_REVISION: &str = "2026-05-20";
const MAX_CONNECT_ATTEMPTS: usize = 3;

async fn send_interactions_chat_request(
    client: &Client,
    url: &str,
    api_key: &str,
    body: &Value,
) -> Result<Response> {
    let mut last_error = None;
    for attempt in 1..=MAX_CONNECT_ATTEMPTS {
        match client
            .post(url)
            .header("content-type", "application/json")
            .header("x-goog-api-key", api_key)
            .header("Api-Revision", API_REVISION)
            .json(body)
            .send()
            .await
        {
            Ok(response) => return Ok(response),
            Err(error)
                if attempt < MAX_CONNECT_ATTEMPTS
                    && (error.is_connect() || error.is_timeout()) =>
            {
                last_error = Some(error);
                tokio::time::sleep(Duration::from_millis(250 * attempt as u64)).await;
            }
            Err(error) => return Err(error).context("发送 Google Interactions 请求失败"),
        }
    }
    Err(last_error.expect("retry loop must retain the final connection error"))
        .context("发送 Google Interactions 请求失败")
}

/// OpenAI tools 数组 → Interactions `tools`（扁平 `{type,name,description,parameters}`）。
pub fn openai_tools_to_interactions(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|t| {
            let f = t.get("function").unwrap_or(t);
            let name = f.get("name")?.as_str()?;
            Some(json!({
                "type": "function",
                "name": name,
                "description": f.get("description").cloned().unwrap_or(json!("")),
                "parameters": f.get("parameters").cloned().unwrap_or(json!({
                    "type": "object",
                    "properties": {}
                })),
            }))
        })
        .collect()
}

/// 从 data URL / http(s) 拼 Interactions image content 块。
fn image_content_from_url(url: &str) -> Value {
    let url = url.trim();
    if let Some(rest) = url.strip_prefix("data:") {
        // data:image/png;base64,....
        if let Some((meta, b64)) = rest.split_once(',') {
            let mime = meta
                .split(';')
                .next()
                .unwrap_or("image/png")
                .trim()
                .to_string();
            return json!({
                "type": "image",
                "mime_type": mime,
                "data": b64,
            });
        }
    }
    json!({
        "type": "image",
        "uri": url,
    })
}

fn user_content_parts(m: &ChatMessage) -> Vec<Value> {
    if let Some(ref parts) = m.parts {
        if !parts.is_empty() {
            return parts
                .iter()
                .map(|p| match p {
                    ChatContentPart::Text { text } => json!({ "type": "text", "text": text }),
                    ChatContentPart::ImageUrl { url } => image_content_from_url(url),
                })
                .collect();
        }
    }
    vec![json!({ "type": "text", "text": m.content })]
}

fn encode_function_result(m: &ChatMessage) -> Value {
    let call_id = m
        .tool_call_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("");
    let name = m
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("tool");
    json!({
        "type": "function_result",
        "call_id": call_id,
        "name": name,
        "result": [{ "type": "text", "text": m.content }],
    })
}

/// 取消息列表尾部连续的 `tool` 角色 → Interactions `function_result` 步骤。
pub fn trailing_function_results(messages: &[ChatMessage]) -> Vec<Value> {
    let mut out = Vec::new();
    for m in messages.iter().rev() {
        if m.role == "tool" {
            out.push(encode_function_result(m));
        } else {
            break;
        }
    }
    out.reverse();
    out
}

/// 将内部消息历史编成 Interactions `input` 步骤数组，并抽出 `system_instruction`。
pub fn messages_to_interactions_input(messages: &[ChatMessage]) -> (Option<String>, Vec<Value>) {
    let mut system_parts: Vec<String> = Vec::new();
    let mut input: Vec<Value> = Vec::new();

    for m in messages {
        match m.role.as_str() {
            "system" => {
                let t = m.content.trim();
                if !t.is_empty() {
                    system_parts.push(t.to_string());
                }
            }
            "user" => {
                input.push(json!({
                    "type": "user_input",
                    "content": user_content_parts(m),
                }));
            }
            "assistant" => {
                if let Some(ref calls) = m.tool_calls {
                    if !calls.is_empty() {
                        if !m.content.trim().is_empty() {
                            input.push(json!({
                                "type": "model_output",
                                "content": [{ "type": "text", "text": m.content }],
                            }));
                        }
                        for c in calls {
                            input.push(json!({
                                "type": "function_call",
                                "id": c.id,
                                "name": c.name,
                                "arguments": c.arguments,
                            }));
                        }
                        continue;
                    }
                }
                input.push(json!({
                    "type": "model_output",
                    "content": [{ "type": "text", "text": m.content }],
                }));
            }
            "tool" => {
                input.push(encode_function_result(m));
            }
            _ => {
                // 未知角色当作 user
                input.push(json!({
                    "type": "user_input",
                    "content": user_content_parts(m),
                }));
            }
        }
    }

    let system = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };
    (system, input)
}

fn thinking_level(config: &ProviderConfig) -> Option<&'static str> {
    if !config.thinking_enabled {
        return None;
    }
    Some(match config.reasoning_effort.trim() {
        "max" | "xhigh" | "high" => "high",
        "minimal" | "min" => "minimal",
        "low" => "low",
        _ => "medium",
    })
}

/// 拼装 Interactions 聊天请求体（默认 store=true + `stream=true`）。
///
/// 若 `previous_interaction_id` 有值且历史尾部有 `tool` 结果，则只提交增量
/// `function_result`（服务端保留 thought/signature）；否则回退为全量历史。
pub fn build_interactions_chat_body(
    messages: &[ChatMessage],
    tools: &[Value],
    config: &ProviderConfig,
) -> Value {
    let prev = config
        .previous_interaction_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let (system, input, use_prev) = if let Some(_prev_id) = prev {
        let results = trailing_function_results(messages);
        if results.is_empty() {
            // 无 tool 结果时无法安全续写，回退全量历史（不带 previous_interaction_id）
            let (sys, input) = messages_to_interactions_input(messages);
            (sys, input, false)
        } else {
            (None, results, true)
        }
    } else {
        let (sys, input) = messages_to_interactions_input(messages);
        (sys, input, false)
    };

    let mut body = json!({
        "model": config.model,
        "input": input,
        "stream": true,
    });
    if use_prev {
        if let Some(prev_id) = prev {
            body["previous_interaction_id"] = json!(prev_id);
        }
    }
    if let Some(sys) = system {
        body["system_instruction"] = json!(sys);
    }
    let ix_tools = openai_tools_to_interactions(tools);
    if !ix_tools.is_empty() {
        body["tools"] = Value::Array(ix_tools);
    }

    let mut gen = serde_json::Map::new();
    // Gemini 3.5 文档建议去掉 temperature；保留 max_tokens 若上游支持
    if config.max_tokens > 0 {
        gen.insert("max_output_tokens".into(), json!(config.max_tokens));
    }
    if let Some(level) = thinking_level(config) {
        gen.insert("thinking_level".into(), json!(level));
    }
    if !gen.is_empty() {
        body["generation_config"] = Value::Object(gen);
    }

    merge_additional_params(&mut body, &config.additional_params);
    body
}

fn parse_usage(v: &Value) -> Option<Usage> {
    let u = v
        .pointer("/interaction/usage")
        .or_else(|| v.get("usage"))?;
    if u.is_null() {
        return None;
    }
    let prompt = u
        .get("prompt_tokens")
        .or_else(|| u.get("input_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    let output = u
        .get("completion_tokens")
        .or_else(|| u.get("output_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    let reasoning = u
        .get("reasoning_tokens")
        .or_else(|| u.pointer("/completion_tokens_details/reasoning_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    if prompt == 0 && output == 0 && reasoning == 0 {
        return None;
    }
    Some(Usage {
        input_tokens: prompt,
        output_tokens: output,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: reasoning,
        request_count: 1,
    })
}

fn extract_interaction_id(v: &Value) -> Option<String> {
    v.pointer("/interaction/id")
        .or_else(|| v.get("id"))
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 将 step 上的 arguments 转为非空 JSON 字符串；空对象/空串返回 None，避免污染增量拼接。
fn meaningful_arguments_json(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => {
            let t = s.trim();
            if t.is_empty() || t == "{}" || t == "null" {
                None
            } else {
                Some(s.clone())
            }
        }
        Value::Object(m) if m.is_empty() => None,
        other => Some(other.to_string()),
    }
}

/// 流式解析状态：跟踪当前 function_call step 的 index → tool_call 槽位。
#[derive(Default)]
pub struct StreamState {
    /// step.index → 分配给 ChatChunk 的 tool_call index
    fc_slots: Vec<(u64, u32)>,
    next_fc_index: u32,
    saw_function_call: bool,
    interaction_id: Option<String>,
}

impl StreamState {
    fn slot_for(&mut self, step_index: u64) -> u32 {
        if let Some((_, slot)) = self.fc_slots.iter().find(|(i, _)| *i == step_index) {
            return *slot;
        }
        let slot = self.next_fc_index;
        self.next_fc_index += 1;
        self.fc_slots.push((step_index, slot));
        slot
    }

    fn note_interaction_id(&mut self, v: &Value) {
        if let Some(id) = extract_interaction_id(v) {
            self.interaction_id = Some(id);
        }
    }
}

/// 将单条 SSE JSON 负载转为零或多个 [`ChatChunk`]（公开以便单测）。
pub fn extract_interactions_chat_events(data: &str, state: &mut StreamState) -> Vec<ChatChunk> {
    let Ok(v) = serde_json::from_str::<Value>(data) else {
        return Vec::new();
    };
    extract_interactions_chat_value(&v, state)
}

fn event_type(v: &Value) -> &str {
    v.get("event_type")
        .or_else(|| v.get("type"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
}

fn extract_interactions_chat_value(v: &Value, state: &mut StreamState) -> Vec<ChatChunk> {
    let et = event_type(v);
    let mut out = Vec::new();

    state.note_interaction_id(v);

    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .or_else(|| err.as_str())
            .unwrap_or("上游 Interactions 错误");
        out.push(ChatChunk {
            finish_reason: Some(format!("error:{msg}")),
            interaction_id: state.interaction_id.clone(),
            ..Default::default()
        });
        return out;
    }

    match et {
        "interaction.created" | "interaction.in_progress" => {
            // 仅记录 interaction_id，无可见内容
        }
        "step.start" => {
            let step = v.get("step");
            let step_type = step
                .and_then(|s| s.get("type"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            if step_type == "function_call" {
                state.saw_function_call = true;
                let step_index = v
                    .get("index")
                    .and_then(|i| i.as_u64())
                    .unwrap_or(state.next_fc_index as u64);
                let slot = state.slot_for(step_index);
                let id = step
                    .and_then(|s| s.get("id"))
                    .and_then(|x| x.as_str())
                    .map(str::to_string);
                let name = step
                    .and_then(|s| s.get("name"))
                    .and_then(|x| x.as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                // 完整 arguments（非流式增量时可能直接出现在 step 上）；跳过空对象
                let arguments = step
                    .and_then(|s| s.get("arguments"))
                    .and_then(meaningful_arguments_json);
                out.push(ChatChunk {
                    tool_call_deltas: vec![ToolCallDeltaChunk {
                        index: slot,
                        id,
                        name,
                        arguments,
                    }],
                    interaction_id: state.interaction_id.clone(),
                    ..Default::default()
                });
            }
        }
        "step.delta" => {
            let delta = v.get("delta");
            let dtype = delta
                .and_then(|d| d.get("type"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            match dtype {
                "text" => {
                    if let Some(text) = delta
                        .and_then(|d| d.get("text"))
                        .and_then(|t| t.as_str())
                        .filter(|s| !s.is_empty())
                    {
                        out.push(ChatChunk {
                            token: Some(text.to_string()),
                            interaction_id: state.interaction_id.clone(),
                            ..Default::default()
                        });
                    }
                }
                "thought" | "thought_summary" => {
                    let text = delta
                        .and_then(|d| d.get("text"))
                        .or_else(|| delta.and_then(|d| d.get("summary")))
                        .and_then(|t| t.as_str())
                        .filter(|s| !s.is_empty());
                    if let Some(text) = text {
                        out.push(ChatChunk {
                            reasoning: Some(text.to_string()),
                            interaction_id: state.interaction_id.clone(),
                            ..Default::default()
                        });
                    }
                }
                "arguments" => {
                    let step_index = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
                    let slot = state.slot_for(step_index);
                    state.saw_function_call = true;
                    let partial = delta
                        .and_then(|d| {
                            d.get("partial_arguments")
                                .or_else(|| d.get("arguments"))
                                .or_else(|| d.get("text"))
                        })
                        .and_then(meaningful_arguments_json);
                    if let Some(arguments) = partial {
                        out.push(ChatChunk {
                            tool_call_deltas: vec![ToolCallDeltaChunk {
                                index: slot,
                                id: None,
                                name: None,
                                arguments: Some(arguments),
                            }],
                            interaction_id: state.interaction_id.clone(),
                            ..Default::default()
                        });
                    }
                }
                _ => {}
            }
        }
        "interaction.requires_action" => {
            state.saw_function_call = true;
            out.push(ChatChunk {
                finish_reason: Some("tool_calls".into()),
                usage: parse_usage(v),
                interaction_id: state.interaction_id.clone(),
                ..Default::default()
            });
        }
        "interaction.completed" => {
            let finish = if state.saw_function_call {
                "tool_calls"
            } else {
                "stop"
            };
            out.push(ChatChunk {
                finish_reason: Some(finish.into()),
                usage: parse_usage(v),
                interaction_id: state.interaction_id.clone(),
                ..Default::default()
            });
        }
        "interaction.failed" | "interaction.error" | "error" => {
            let msg = v
                .pointer("/interaction/error/message")
                .or_else(|| v.pointer("/error/message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Interactions 失败");
            out.push(ChatChunk {
                finish_reason: Some(format!("error:{msg}")),
                interaction_id: state.interaction_id.clone(),
                ..Default::default()
            });
        }
        _ => {}
    }

    out
}

/// Google Interactions 流式聊天。
pub async fn interactions_chat_stream(
    client: &Client,
    messages: Vec<ChatMessage>,
    tools: Vec<Value>,
    config: &ProviderConfig,
) -> Result<ChatStream> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    let url = interactions_url(config);
    let body = build_interactions_chat_body(&messages, &tools, config);

    let response = send_interactions_chat_request(client, &url, config.api_key.trim(), &body)
        .await
        .with_context(|| format!("连接 Google Interactions 失败: {url}"))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| {
                v.pointer("/error/message")
                    .and_then(|m| m.as_str())
                    .map(str::to_string)
                    .or_else(|| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
            })
            .unwrap_or(text);
        return Err(anyhow!("上游 HTTP {status}: {msg}"));
    }

    let byte_stream = response.bytes_stream();
    let stream = futures::stream::unfold(
        (byte_stream, String::new(), false, StreamState::default()),
        |(mut byte_stream, mut buf, done, mut state)| async move {
            if done {
                return None;
            }
            loop {
                if let Some(nl) = buf.find('\n') {
                    let line = buf[..nl].trim_end_matches('\r').to_string();
                    buf = buf[nl + 1..].to_string();
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with(':') {
                        continue;
                    }
                    // 忽略 SSE event: 行，只处理 data:
                    if !trimmed.starts_with("data:") {
                        continue;
                    }
                    let data = trimmed.strip_prefix("data:").unwrap_or("").trim();
                    if data == "[DONE]" {
                        return None;
                    }
                    let chunks = extract_interactions_chat_events(data, &mut state);
                    for chunk in &chunks {
                        if chunk
                            .finish_reason
                            .as_deref()
                            .is_some_and(|f| f.starts_with("error:"))
                        {
                            let msg = chunk
                                .finish_reason
                                .as_deref()
                                .unwrap_or("error")
                                .trim_start_matches("error:")
                                .to_string();
                            return Some((Err(anyhow!(msg)), (byte_stream, buf, true, state)));
                        }
                    }
                    if !chunks.is_empty() {
                        let merged = merge_chunks(chunks);
                        return Some((Ok(merged), (byte_stream, buf, false, state)));
                    }
                    continue;
                }

                match byte_stream.next().await {
                    Some(Ok(bytes)) => {
                        buf.push_str(&String::from_utf8_lossy(&bytes));
                    }
                    Some(Err(err)) => {
                        return Some((Err(err.into()), (byte_stream, buf, true, state)));
                    }
                    None => {
                        if !buf.trim().is_empty() {
                            let line = buf.trim().to_string();
                            buf.clear();
                            if let Some(data) = line.strip_prefix("data:") {
                                let data = data.trim();
                                if data != "[DONE]" {
                                    let chunks = extract_interactions_chat_events(data, &mut state);
                                    if !chunks.is_empty() {
                                        return Some((
                                            Ok(merge_chunks(chunks)),
                                            (byte_stream, buf, true, state),
                                        ));
                                    }
                                }
                            }
                        }
                        return None;
                    }
                }
            }
        },
    );

    Ok(Box::pin(stream))
}

fn merge_chunks(chunks: Vec<ChatChunk>) -> ChatChunk {
    if chunks.len() == 1 {
        return chunks.into_iter().next().unwrap();
    }
    let mut out = ChatChunk::default();
    for c in chunks {
        if let Some(t) = c.token {
            out.token = Some(match out.token.take() {
                Some(prev) => prev + &t,
                None => t,
            });
        }
        if let Some(r) = c.reasoning {
            out.reasoning = Some(match out.reasoning.take() {
                Some(prev) => prev + &r,
                None => r,
            });
        }
        out.tool_call_deltas.extend(c.tool_call_deltas);
        if c.finish_reason.is_some() {
            out.finish_reason = c.finish_reason;
        }
        if c.usage.is_some() {
            out.usage = c.usage;
        }
        if c.interaction_id.is_some() {
            out.interaction_id = c.interaction_id;
        }
    }
    out
}

/// Interactions 连通性探测（非流式最小请求）。
pub async fn probe_interactions(
    client: &Client,
    model: &str,
    config: &ProviderConfig,
) -> Result<String, String> {
    if config.api_key.trim().is_empty() {
        return Err("Google API Key 为空".into());
    }
    let url = interactions_url(config);
    let body = json!({
        "model": model,
        "input": "ping",
        "generation_config": { "max_output_tokens": 1 }
    });
    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .header("x-goog-api-key", config.api_key.trim())
        .header("Api-Revision", API_REVISION)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("连接 Google Interactions 失败: {e}"))?;
    let status = resp.status();
    let json: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let msg = json
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误");
        return Err(format!("失败 ({status}): {msg}"));
    }
    Ok("调用成功".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trait_::ChatToolCall;

    #[test]
    fn tools_conversion_flattens_openai_function() {
        let tools = vec![json!({
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "weather",
                "parameters": {
                    "type": "object",
                    "properties": { "location": { "type": "string" } },
                    "required": ["location"]
                }
            }
        })];
        let out = openai_tools_to_interactions(&tools);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["type"], "function");
        assert_eq!(out[0]["name"], "get_weather");
        assert_eq!(out[0]["parameters"]["required"][0], "location");
    }

    #[test]
    fn messages_encode_system_user_and_tools() {
        let messages = vec![
            ChatMessage::text("system", "You are helpful."),
            ChatMessage::text("user", "weather in Boston?"),
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
                content: "52F".into(),
                parts: None,
                tool_calls: None,
                tool_call_id: Some("fc_1".into()),
                name: Some("get_weather".into()),
            },
            ChatMessage::text("user", "thanks"),
        ];
        let (sys, input) = messages_to_interactions_input(&messages);
        assert_eq!(sys.as_deref(), Some("You are helpful."));
        assert_eq!(input[0]["type"], "user_input");
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[1]["id"], "fc_1");
        assert_eq!(input[2]["type"], "function_result");
        assert_eq!(input[2]["call_id"], "fc_1");
        assert_eq!(input[3]["type"], "user_input");
    }

    #[test]
    fn stream_text_and_thought_deltas() {
        let mut state = StreamState::default();
        let chunks = extract_interactions_chat_events(
            r#"{"event_type":"step.delta","index":1,"delta":{"type":"text","text":"Hi"}}"#,
            &mut state,
        );
        assert_eq!(chunks[0].token.as_deref(), Some("Hi"));

        let chunks = extract_interactions_chat_events(
            r#"{"type":"step.delta","index":0,"delta":{"type":"thought","text":"think"}}"#,
            &mut state,
        );
        assert_eq!(chunks[0].reasoning.as_deref(), Some("think"));
    }

    #[test]
    fn stream_function_call_lifecycle() {
        let mut state = StreamState::default();
        let start = extract_interactions_chat_events(
            r#"{"event_type":"step.start","index":1,"step":{"type":"function_call","id":"fc_1","name":"get_weather"}}"#,
            &mut state,
        );
        assert_eq!(start[0].tool_call_deltas[0].id.as_deref(), Some("fc_1"));
        assert_eq!(
            start[0].tool_call_deltas[0].name.as_deref(),
            Some("get_weather")
        );
        assert!(start[0].tool_call_deltas[0].arguments.is_none());

        let args = extract_interactions_chat_events(
            r#"{"event_type":"step.delta","index":1,"delta":{"type":"arguments","partial_arguments":"{\"location\":\"Boston\"}"}}"#,
            &mut state,
        );
        assert_eq!(
            args[0].tool_call_deltas[0].arguments.as_deref(),
            Some(r#"{"location":"Boston"}"#)
        );

        let done = extract_interactions_chat_events(
            r#"{"event_type":"interaction.requires_action","interaction":{"id":"ix_1","status":"requires_action"}}"#,
            &mut state,
        );
        assert_eq!(done[0].finish_reason.as_deref(), Some("tool_calls"));
        assert_eq!(done[0].interaction_id.as_deref(), Some("ix_1"));
    }

    #[test]
    fn step_start_skips_empty_arguments_object() {
        let mut state = StreamState::default();
        let start = extract_interactions_chat_events(
            r#"{"event_type":"step.start","index":0,"step":{"type":"function_call","id":"fc_1","name":"f","arguments":{}}}"#,
            &mut state,
        );
        assert!(start[0].tool_call_deltas[0].arguments.is_none());
    }

    #[test]
    fn interaction_error_maps_to_finish_reason() {
        let mut state = StreamState::default();
        let chunks = extract_interactions_chat_events(
            r#"{"event_type":"interaction.error","interaction":{"id":"ix","error":{"message":"boom"}}}"#,
            &mut state,
        );
        assert_eq!(chunks[0].finish_reason.as_deref(), Some("error:boom"));
        assert_eq!(chunks[0].interaction_id.as_deref(), Some("ix"));
    }

    #[test]
    fn build_body_sets_thinking_and_defaults_store() {
        let messages = vec![ChatMessage::text("user", "hi")];
        let mut config = ProviderConfig::default();
        config.model = "gemini-3.5-flash".into();
        config.thinking_enabled = true;
        config.reasoning_effort = "high".into();
        let body = build_interactions_chat_body(&messages, &[], &config);
        assert_eq!(body["stream"], true);
        assert!(body.get("store").is_none() || body["store"] == true);
        assert!(body.get("previous_interaction_id").is_none());
        assert_eq!(body["generation_config"]["thinking_level"], "high");
    }

    #[test]
    fn build_body_with_previous_interaction_sends_only_tool_results() {
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
                content: "52F".into(),
                parts: None,
                tool_calls: None,
                tool_call_id: Some("fc_1".into()),
                name: Some("get_weather".into()),
            },
        ];
        let mut config = ProviderConfig::default();
        config.model = "gemini-3.5-flash".into();
        config.previous_interaction_id = Some("ix_prev".into());
        let body = build_interactions_chat_body(&messages, &[], &config);
        assert_eq!(body["previous_interaction_id"], "ix_prev");
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "function_result");
        assert_eq!(input[0]["call_id"], "fc_1");
        assert!(body.get("system_instruction").is_none());
    }

    #[test]
    fn trailing_function_results_stops_at_non_tool() {
        let messages = vec![
            ChatMessage {
                role: "tool".into(),
                content: "old".into(),
                parts: None,
                tool_calls: None,
                tool_call_id: Some("a".into()),
                name: Some("t".into()),
            },
            ChatMessage::text("user", "again"),
            ChatMessage {
                role: "tool".into(),
                content: "new".into(),
                parts: None,
                tool_calls: None,
                tool_call_id: Some("b".into()),
                name: Some("t".into()),
            },
        ];
        let results = trailing_function_results(&messages);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["call_id"], "b");
    }
}
