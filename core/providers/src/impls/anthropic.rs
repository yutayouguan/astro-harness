//! Anthropic Messages API — 原生 CompletionModel 实现。
//!
//! 消息格式、SSE 事件、认证方式均与 OpenAI 不同，不走 compat 层。

use anyhow::{anyhow, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Client as HttpClient;
use serde_json::{json, Value};

use crate::traits::{Capable, Capabilities, CompletionModel, FromClient, Nothing, ProviderClient, ProviderExt};
use crate::types::{CompletionRequest, CompletionStream};

const ANTHROPIC_VERSION: &str = "2024-10-22";
const ANTHROPIC_BETA: &str = "prompt-caching-2024-07-31,pdfs-2024-09-25,token-counting-2024-11-01,interleaved-thinking-2025-05-14";
const THINKING_BUDGET_HIGH: u32 = 10_240;
const THINKING_BUDGET_MAX: u32 = 32_768;

// ─── Provider Extension ─────────────────────────────────

#[derive(Debug, Clone, Copy, Default)]
pub struct Anthropic;

impl ProviderExt for Anthropic {
    const NAME: &'static str = "anthropic";
    const BASE_URL: &'static str = "https://api.anthropic.com";

    fn auth_headers(&self, key: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Ok(v) = HeaderValue::from_str(key) {
            h.insert("x-api-key", v);
        }
        if let Ok(v) = HeaderValue::from_str(ANTHROPIC_VERSION) {
            h.insert("anthropic-version", v);
        }
        if let Ok(v) = HeaderValue::from_str(ANTHROPIC_BETA) {
            h.insert("anthropic-beta", v);
        }
        h
    }
}

impl Capabilities for Anthropic {
    type Chat = Capable<AnthropicCompletionModel>;
    type Embedding = Nothing;
    type ImageGen = Nothing;
    type VideoGen = Nothing;
    type TTS = Nothing;
    type MusicGen = Nothing;
    type ASR = Nothing;
}

// ─── Completion Model ────────────────────────────────────

pub struct AnthropicCompletionModel {
    http: HttpClient,
    base_url: String,
    api_key: String,
    model: String,
}

impl Clone for AnthropicCompletionModel {
    fn clone(&self) -> Self {
        Self {
            http: self.http.clone(),
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
            model: self.model.clone(),
        }
    }
}

impl FromClient<Anthropic> for AnthropicCompletionModel {
    fn from_client(client: &ProviderClient<Anthropic>, model: &str) -> Self {
        Self {
            http: client.http.clone(),
            base_url: client.base_url.clone(),
            api_key: client.api_key.clone(),
            model: model.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl CompletionModel for AnthropicCompletionModel {
    async fn stream(&self, request: CompletionRequest) -> Result<CompletionStream> {
        if self.api_key.is_empty() {
            return Err(anyhow!("缺少 Anthropic API Key"));
        }
        let base = self
            .base_url
            .trim_end_matches('/')
            .trim_end_matches("/v1");
        let url = format!("{base}/v1/messages");

        let (system, api_messages) = to_anthropic_messages(&request.messages);

        let model = if request.model.is_empty() { &self.model } else { &request.model };
        let mut body = json!({
            "model": model,
            "max_tokens": request.max_tokens.unwrap_or(4096),
            "stream": true,
            "messages": api_messages,
        });

        if let Some(temp) = request.temperature {
            body["temperature"] = json!(temp);
        }

        if !system.is_null() {
            body["system"] = system;
        }

        // Extended Thinking
        if let Some(ref tc) = request.thinking {
            if tc.enabled {
                let raw_budget = match tc.effort.trim() {
                    "max" => THINKING_BUDGET_MAX,
                    "high" | "" => THINKING_BUDGET_HIGH,
                    other => other.parse::<u32>().unwrap_or(THINKING_BUDGET_HIGH),
                };
                let max = request.max_tokens.unwrap_or(4096);
                let budget = raw_budget.clamp(1024, max.saturating_sub(1).max(1024));
                body["thinking"] = json!({"type": "enabled", "budget_tokens": budget});
                body.as_object_mut().unwrap().remove("temperature");
            }
        }

        // Tools
        if !request.tools.is_empty() {
            let tools: Vec<Value> = request.tools.iter().map(|t| {
                json!({"name": t.name, "description": t.description, "input_schema": t.parameters})
            }).collect();
            body["tools"] = Value::Array(tools);
        }

        if let Some(tc) = request.additional_params.get("tool_choice") {
            body["tool_choice"] = tc.clone();
        }

        // additional_params merge
        if let Some(extra) = request.additional_params.as_object() {
            if let Some(obj) = body.as_object_mut() {
                for (k, v) in extra {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }

        let auth = Anthropic.auth_headers(&self.api_key);
        let response = self.http
            .post(&url)
            .headers(auth)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .with_context(|| format!("连接 Anthropic 失败: {url}"))?;

        crate::shared::sse::sse_stream(response, crate::shared::sse::wrap_single_extract(extract_anthropic_delta)).await
    }
}

// ─── Message Conversion ──────────────────────────────────

/// 公开供 token_count 等旧模块调用。
pub fn to_anthropic_messages_public(messages: &[crate::types::Message]) -> (Value, Vec<Value>) {
    to_anthropic_messages(messages)
}

fn to_anthropic_messages(messages: &[crate::types::Message]) -> (Value, Vec<Value>) {
    use crate::types::message::*;
    let mut system = String::new();
    let mut api_msgs = Vec::new();
    let mut pending_tool_results: Vec<Value> = Vec::new();

    let flush = |pending: &mut Vec<Value>, out: &mut Vec<Value>| {
        if pending.is_empty() { return; }
        out.push(json!({"role": "user", "content": Value::Array(std::mem::take(pending))}));
    };

    for m in messages {
        match m {
            Message::System { content } => {
                if !system.is_empty() { system.push('\n'); }
                system.push_str(content);
            }
            Message::Tool { tool_call_id, content, is_error } => {
                let mut block = json!({"type": "tool_result", "tool_use_id": tool_call_id, "content": content});
                if *is_error { block["is_error"] = json!(true); }
                pending_tool_results.push(block);
            }
            Message::Assistant { content } => {
                flush(&mut pending_tool_results, &mut api_msgs);
                let mut blocks = Vec::new();
                for c in content {
                    match c {
                        AssistantContent::Thinking { text, signature } => {
                            let mut b = json!({"type": "thinking", "thinking": text});
                            if let Some(sig) = signature { b["signature"] = json!(sig); }
                            blocks.push(b);
                        }
                        AssistantContent::Text { text } => {
                            if !text.is_empty() {
                                blocks.push(json!({"type": "text", "text": text}));
                            }
                        }
                        AssistantContent::ToolCall(tc) => {
                            let input = if tc.arguments.is_string() {
                                serde_json::from_str(tc.arguments.as_str().unwrap_or("{}")).unwrap_or(json!({}))
                            } else {
                                tc.arguments.clone()
                            };
                            blocks.push(json!({"type": "tool_use", "id": tc.id, "name": tc.name, "input": input}));
                        }
                    }
                }
                if blocks.is_empty() { blocks.push(json!({"type": "text", "text": ""})); }
                api_msgs.push(json!({"role": "assistant", "content": blocks}));
            }
            Message::User { content } => {
                flush(&mut pending_tool_results, &mut api_msgs);
                let blocks = anthropic_user_content(content);
                api_msgs.push(json!({"role": "user", "content": blocks}));
            }
        }
    }
    flush(&mut pending_tool_results, &mut api_msgs);

    // prompt caching on system + last user message
    let system_value = if system.is_empty() {
        Value::Null
    } else {
        json!([{"type": "text", "text": system, "cache_control": {"type": "ephemeral"}}])
    };
    if let Some(last_user) = api_msgs.iter_mut().rev().find(|m| m["role"] == "user") {
        if let Some(blocks) = last_user.get_mut("content").and_then(|c| c.as_array_mut()) {
            if let Some(last_block) = blocks.last_mut() {
                if let Some(obj) = last_block.as_object_mut() {
                    obj.insert("cache_control".into(), json!({"type": "ephemeral"}));
                }
            }
        }
    }
    (system_value, api_msgs)
}

fn anthropic_user_content(content: &[crate::types::UserContent]) -> Value {
    use crate::types::message::UserContent;
    if content.len() == 1 {
        if let UserContent::Text { text } = &content[0] {
            return json!(text);
        }
    }
    let blocks: Vec<Value> = content.iter().map(|c| match c {
        UserContent::Text { text } => json!({"type": "text", "text": text}),
        UserContent::Image { url } => {
            if url.starts_with("data:") {
                if let Some((media, data)) = parse_data_url(url) {
                    return json!({"type": "image", "source": {"type": "base64", "media_type": media, "data": data}});
                }
            }
            json!({"type": "image", "source": {"type": "url", "url": url}})
        }
        UserContent::Document { url, .. } => {
            if url.starts_with("data:") {
                if let Some((media, data)) = parse_data_url(url) {
                    return json!({"type": "document", "source": {"type": "base64", "media_type": media, "data": data}});
                }
            }
            json!({"type": "document", "source": {"type": "url", "url": url}})
        }
        UserContent::Audio { mime_type, .. } => json!({"type": "text", "text": format!("[audio: {mime_type}]")}),
        UserContent::Video { mime_type, .. } => json!({"type": "text", "text": format!("[video: {mime_type}]")}),
        UserContent::ToolResult { .. } => json!({"type": "text", "text": ""}),
    }).collect();
    Value::Array(blocks)
}

fn parse_data_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(";base64,")?;
    let media_type = meta.trim();
    if media_type.is_empty() || data.is_empty() { return None; }
    Some((media_type.to_string(), data.to_string()))
}

// ─── SSE Parsing ─────────────────────────────────────────

fn extract_anthropic_delta(data: &str) -> Option<crate::types::StreamChunk> {
    use crate::types::stream::StreamChunk;
    let v: Value = serde_json::from_str(data).ok()?;
    let event_type = v.get("type")?.as_str()?;

    match event_type {
        "message_start" => {
            let u = v.pointer("/message/usage")?;
            let usage = parse_anthropic_usage(u)?;
            Some(StreamChunk::Usage(usage))
        }
        "content_block_start" => {
            let block = v.get("content_block")?;
            let bt = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
            let index = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
            match bt {
                "tool_use" | "server_tool_use" => Some(StreamChunk::ToolCallStart {
                    index,
                    id: block.get("id").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                    name: block.get("name").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                }),
                "thinking" => {
                    let sig = block.get("signature").and_then(|s| s.as_str()).filter(|s| !s.is_empty()).map(str::to_string);
                    sig.map(StreamChunk::ThoughtSignature)
                }
                _ => None,
            }
        }
        "content_block_delta" => {
            let delta = v.get("delta")?;
            let dt = delta.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match dt {
                "input_json_delta" => {
                    let partial = delta.get("partial_json").and_then(|s| s.as_str()).map(str::to_string)?;
                    let index = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
                    Some(StreamChunk::ToolCallDelta { index, arguments: partial })
                }
                "thinking_delta" => {
                    let text = delta.get("thinking").and_then(|t| t.as_str()).filter(|s| !s.is_empty()).map(str::to_string)?;
                    Some(StreamChunk::Thinking(text))
                }
                "signature_delta" => {
                    let sig = delta.get("signature").and_then(|s| s.as_str()).filter(|s| !s.is_empty()).map(str::to_string)?;
                    Some(StreamChunk::ThoughtSignature(sig))
                }
                "citations_delta" => {
                    let citation = delta.get("citation")?;
                    Some(StreamChunk::Citation(citation.clone()))
                }
                _ => {
                    let text = delta.get("text").and_then(|t| t.as_str()).filter(|s| !s.is_empty()).map(str::to_string)?;
                    Some(StreamChunk::Text(text))
                }
            }
        }
        "message_delta" => {
            let finish = v.pointer("/delta/stop_reason").and_then(|s| s.as_str()).map(str::to_string);
            let usage = v.get("usage").and_then(parse_anthropic_usage);
            if let Some(f) = finish {
                return Some(StreamChunk::Done { finish_reason: f });
            }
            usage.map(StreamChunk::Usage)
        }
        "error" => {
            let msg = v.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or("Anthropic error");
            Some(StreamChunk::Error(msg.to_string()))
        }
        _ => None,
    }
}

fn parse_anthropic_usage(u: &Value) -> Option<crate::types::stream::Usage> {
    let input = u.get("input_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    let output = u.get("output_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    let cache_read = u.get("cache_read_input_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    let cache_write = u.get("cache_creation_input_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 { return None; }
    Some(crate::types::stream::Usage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        reasoning_tokens: 0,
        request_count: 1,
    })
}

// ─── 连通性探测 ─────────────────────────────────────────

/// 向 Anthropic Messages API 发送最小请求以验证连通性。
pub async fn probe_anthropic(
    client: &HttpClient,
    model: &str,
    endpoint: &str,
    config: &crate::trait_::ProviderConfig,
) -> Result<String, String> {
    let url = format!("{}/v1/messages", endpoint.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "max_tokens": 1,
        "messages": [{"role": "user", "content": "ping"}]
    });
    let resp = client
        .post(&url)
        .header("x-api-key", &config.api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("连接 Anthropic 失败: {e}"))?;
    let status = resp.status();
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let msg = json["error"]["message"].as_str().unwrap_or("未知错误");
        return Err(format!("失败 ({status}): {msg}"));
    }
    Ok("调用成功".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::client::ChatClient;
    use crate::types::StreamChunk;

    #[test]
    fn anthropic_has_chat() {
        let client = ProviderClient::new("test-key", Anthropic);
        let _model = client.completion_model("claude-opus-4-8");
    }

    #[test]
    fn system_with_cache_control() {
        let msgs = vec![crate::types::Message::system("You are helpful.")];
        let (sys, _) = to_anthropic_messages(&msgs);
        let blocks = sys.as_array().unwrap();
        assert_eq!(blocks[0]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn thinking_block_in_assistant() {
        let msgs = vec![crate::types::Message::assistant(vec![
            crate::types::AssistantContent::Thinking { text: "hmm".into(), signature: Some("sig".into()) },
            crate::types::AssistantContent::Text { text: "42".into() },
        ])];
        let (_, api) = to_anthropic_messages(&msgs);
        let content = api[0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "thinking");
        assert_eq!(content[0]["signature"], "sig");
        assert_eq!(content[1]["type"], "text");
    }

    #[test]
    fn sse_text_delta() {
        let data = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        match extract_anthropic_delta(data) {
            Some(StreamChunk::Text(t)) => assert_eq!(t, "Hello"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn sse_thinking_delta() {
        let data = r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"let me think"}}"#;
        match extract_anthropic_delta(data) {
            Some(StreamChunk::Thinking(t)) => assert_eq!(t, "let me think"),
            other => panic!("expected Thinking, got {other:?}"),
        }
    }
}
