//! OpenAI Chat Completions 流式聊天 + SSE 解析。

use std::sync::Arc;

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};

use crate::streaming::Usage;
use crate::trait_::{
    ChatChunk, ChatContentPart, ChatMessage, ChatStream, ProviderConfig, ToolCallDeltaChunk,
};
use crate::http_stream::{merge_additional_params, resolve_base, sse_chat_stream};

/// 规范化 OpenAI 兼容 API 基址（自动补 `/v1` 等后缀）。
///
/// Gemini OpenAI 兼容基址以 `/openai` 结尾，不得再追加 `/v1`。
/// Google Gemini 聊天已迁 Interactions，勿再经此函数拼 `/v1beta/openai`。
pub fn openai_compatible_base(endpoint: &str) -> String {
    let base = crate::http_stream::trim_slash(endpoint);
    if base.ends_with("/v1")
        || base.ends_with("/v3")
        || base.ends_with("/v4")
        || base.ends_with("/openai")
        || base.contains("/paas/v4")
        || base.contains("/v1beta/openai")
    {
        base
    } else if base.is_empty() {
        super::DEFAULT_API_BASE.to_string()
    } else {
        format!("{base}/v1")
    }
}

/// 将工具参数 JSON 转为 OpenAI 请求所需的字符串形式。
fn args_to_openai_string(args: &Value) -> String {
    match args {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// 将内部 [`ChatMessage`] 列表转为 OpenAI 兼容 `messages` 数组。
pub(crate) fn to_openai_messages(messages: &[ChatMessage]) -> Vec<Value> {
    messages
        .iter()
        .filter_map(|m| {
            let mut obj = serde_json::Map::new();
            obj.insert("role".into(), json!(m.role));

            if m.role == "tool" {
                let id = m
                    .tool_call_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())?;
                obj.insert("tool_call_id".into(), json!(id));
                if let Some(ref name) = m.name {
                    if !name.is_empty() {
                        obj.insert("name".into(), json!(name));
                    }
                }
                obj.insert("content".into(), json!(m.content));
                return Some(Value::Object(obj));
            }

            if let Some(ref calls) = m.tool_calls {
                // 纯 tool_calls 时 content 可为 null（DeepSeek / OpenAI 惯例）
                if m.content.is_empty() {
                    obj.insert("content".into(), Value::Null);
                } else {
                    obj.insert("content".into(), json!(m.content));
                }
                let tool_calls: Vec<Value> = calls
                    .iter()
                    .map(|c| {
                        json!({
                            "id": c.id,
                            "type": "function",
                            "function": {
                                "name": c.name,
                                "arguments": args_to_openai_string(&c.arguments),
                            }
                        })
                    })
                    .collect();
                obj.insert("tool_calls".into(), Value::Array(tool_calls));
            } else if let Some(ref parts) = m.parts {
                if !parts.is_empty() {
                    let arr: Vec<Value> = parts
                        .iter()
                        .map(|p| match p {
                            ChatContentPart::Text { text } => {
                                json!({ "type": "text", "text": text })
                            }
                            ChatContentPart::ImageUrl { url } => json!({
                                "type": "image_url",
                                "image_url": { "url": url }
                            }),
                            // OpenAI chat completions 无通用 audio/video part：回落为文本标注
                            ChatContentPart::AudioUrl { mime_type, .. } => json!({
                                "type": "text",
                                "text": format!(
                                    "[audio attached: {}]",
                                    if mime_type.trim().is_empty() { "audio/*" } else { mime_type }
                                )
                            }),
                            ChatContentPart::VideoUrl { mime_type, .. } => json!({
                                "type": "text",
                                "text": format!(
                                    "[video attached: {}]",
                                    if mime_type.trim().is_empty() { "video/*" } else { mime_type }
                                )
                            }),
                            ChatContentPart::DocumentUrl { mime_type, .. } => json!({
                                "type": "text",
                                "text": format!(
                                    "[document attached: {}]",
                                    if mime_type.trim().is_empty() { "application/pdf" } else { mime_type }
                                )
                            }),
                        })
                        .collect();
                    obj.insert("content".into(), Value::Array(arr));
                } else {
                    obj.insert("content".into(), json!(m.content));
                }
            } else {
                obj.insert("content".into(), json!(m.content));
            }

            Some(Value::Object(obj))
        })
        .collect()
}

/// 从 OpenAI choice 对象解析 `delta.tool_calls` 增量。
fn parse_openai_tool_call_deltas(choice: &Value) -> Vec<ToolCallDeltaChunk> {
    let Some(arr) = choice
        .pointer("/delta/tool_calls")
        .or_else(|| choice.pointer("/message/tool_calls"))
        .and_then(|v| v.as_array())
    else {
        return Vec::new();
    };
    arr.iter()
        .map(|tc| {
            let index = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
            ToolCallDeltaChunk {
                index,
                id: tc.get("id").and_then(|s| s.as_str()).map(str::to_string),
                name: tc
                    .pointer("/function/name")
                    .and_then(|s| s.as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
                arguments: tc
                    .pointer("/function/arguments")
                    .and_then(|s| s.as_str())
                    .map(str::to_string),
                signature: None,
            }
        })
        .collect()
}

/// 从 OpenAI 风格 JSON 解析 usage（供单测与 SSE 共用）
pub fn parse_openai_usage(v: &Value) -> Option<Usage> {
    let u = v.get("usage")?;
    if u.is_null() {
        return None;
    }
    let prompt_total = u
        .get("prompt_tokens")
        .or_else(|| u.get("input_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    let output = u
        .get("completion_tokens")
        .or_else(|| u.get("output_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    let details = u.get("prompt_tokens_details");
    let mut cache_read = details
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    if cache_read == 0 {
        cache_read = u
            .get("cache_read_input_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0) as u32;
    }
    let mut cache_write = details
        .and_then(|d| d.get("cache_write_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    if cache_write == 0 {
        cache_write = u
            .get("cache_creation_input_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0) as u32;
    }
    let input = prompt_total
        .saturating_sub(cache_read)
        .saturating_sub(cache_write);
    let reasoning = u
        .get("completion_tokens_details")
        .or_else(|| u.get("output_tokens_details"))
        .and_then(|d| d.get("reasoning_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 && reasoning == 0 {
        return None;
    }
    Some(Usage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        reasoning_tokens: reasoning,
        request_count: 1,
    })
}

/// OpenAI 兼容 SSE `data:` 负载解析（含 usage-only 末包）
pub fn extract_openai_delta(data: &str) -> Option<ChatChunk> {
    let v: Value = serde_json::from_str(data).ok()?;
    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("上游 API 错误");
        return Some(ChatChunk {
            finish_reason: Some(format!("error:{msg}")),
            ..Default::default()
        });
    }
    let usage = parse_openai_usage(&v);
    let choice = v
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first());
    let Some(choice) = choice else {
        return usage.map(|u| ChatChunk {
            usage: Some(u),
            ..Default::default()
        });
    };
    let finish = choice
        .get("finish_reason")
        .and_then(|f| f.as_str())
        .map(str::to_string);
    let token = choice
        .pointer("/delta/content")
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            choice
                .pointer("/message/content")
                .and_then(|c| c.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        });
    let reasoning = choice
        .pointer("/delta/reasoning_content")
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| {
            choice
                .pointer("/message/reasoning_content")
                .and_then(|c| c.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        });
    let tool_call_deltas = parse_openai_tool_call_deltas(choice);
    if token.is_none()
        && reasoning.is_none()
        && finish.is_none()
        && tool_call_deltas.is_empty()
        && usage.is_none()
    {
        return None;
    }
    Some(ChatChunk {
        token,
        reasoning,
        finish_reason: finish,
        tool_call_deltas,
        usage,
        interaction_id: None,
        thought_signature: None,
        citations: None,
    })
}

/// 判断该供应商是否支持 `stream_options.include_usage`。
fn supports_stream_include_usage(provider: &str) -> bool {
    // 部分兼容网关会拒 stream_options；仅对确认支持的上游开启
    matches!(
        provider,
        "openai"
            | "azure"
            | "deepseek"
            | "openrouter"
            | "nvidia"
            | "moonshot"
            | "mimo"
            | "minimax"
            | "ollama"
    )
}

/// OpenAI 兼容：`POST {base}/chat/completions` + SSE
pub async fn openai_compatible_chat_stream(
    client: &Client,
    provider: &str,
    messages: Vec<ChatMessage>,
    tools: Vec<Value>,
    config: &ProviderConfig,
) -> Result<ChatStream> {
    let base = openai_compatible_base(&resolve_base(config, provider));
    let url = format!("{base}/chat/completions");

    let mut body = json!({
        "model": config.model,
        "messages": to_openai_messages(&messages),
        "stream": true,
        "temperature": config.temperature,
        "max_tokens": config.max_tokens,
    });
    if supports_stream_include_usage(provider) {
        body["stream_options"] = json!({ "include_usage": true });
    }
    // 原生 function calling：下发 OpenAI tools；无 tools 时仍可走 XML <tool_call> 回退
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
        body["tool_choice"] = json!("auto");
    }
    // DeepSeek V4：按请求开关 thinking，并把 reasoning_content 流式回传
    if provider == "deepseek" {
        body["thinking"] = json!({
            "type": if config.thinking_enabled { "enabled" } else { "disabled" }
        });
        if config.thinking_enabled {
            // DeepSeek V4：官方档位为 high / xhigh；max 映射到 xhigh
            let effort = match config.reasoning_effort.trim() {
                "max" | "xhigh" => "xhigh",
                "high" => "high",
                other if !other.is_empty() => other,
                _ => "high",
            };
            body["reasoning_effort"] = json!(effort);
        }
    }

    merge_additional_params(&mut body, &config.additional_params);
    let mut req = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&body);
    if !config.api_key.is_empty() {
        req = req.bearer_auth(&config.api_key);
    }

    let response = req
        .send()
        .await
        .with_context(|| format!("连接 {provider} 失败: {url}"))?;

    sse_chat_stream(response, Arc::new(extract_openai_delta)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streaming::Usage;

    #[test]
    fn extract_openai_usage_only_chunk() {
        let data = r#"{"id":"x","choices":[],"usage":{"prompt_tokens":12,"completion_tokens":34,"total_tokens":46}}"#;
        let chunk = extract_openai_delta(data).expect("usage chunk");
        assert_eq!(
            chunk.usage,
            Some(Usage {
                input_tokens: 12,
                output_tokens: 34,
                request_count: 1,
                ..Default::default()
            })
        );
        assert!(chunk.token.is_none());
    }

    #[test]
    fn extract_openai_token_with_usage() {
        let data = r#"{"choices":[{"delta":{"content":"hi"},"finish_reason":null}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#;
        let chunk = extract_openai_delta(data).expect("token");
        assert_eq!(chunk.token.as_deref(), Some("hi"));
        assert_eq!(chunk.usage.map(|u| u.total_tokens()), Some(2));
    }

    #[test]
    fn parse_usage_null_returns_none() {
        let v = json!({"usage": null});
        assert!(parse_openai_usage(&v).is_none());
    }

    #[test]
    fn stream_include_usage_whitelist() {
        assert!(supports_stream_include_usage("openai"));
        assert!(supports_stream_include_usage("deepseek"));
        assert!(supports_stream_include_usage("azure"));
        assert!(!supports_stream_include_usage("google"));
        assert!(!supports_stream_include_usage("zhipu"));
        assert!(!supports_stream_include_usage("bailian"));
        assert!(!supports_stream_include_usage("volcengine"));
        assert!(supports_stream_include_usage("minimax"));
    }

    #[test]
    fn to_openai_messages_string_content_when_no_parts() {
        let msgs = to_openai_messages(&[ChatMessage::text("user", "hi")]);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "hi");
    }

    #[test]
    fn to_openai_messages_array_when_parts() {
        let msgs = to_openai_messages(&[ChatMessage::user_parts(
            "看图",
            vec![
                ChatContentPart::Text {
                    text: "看图".into(),
                },
                ChatContentPart::ImageUrl {
                    url: "data:image/png;base64,abc".into(),
                },
            ],
        )]);
        let content = msgs[0]["content"].as_array().expect("array content");
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "看图");
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(content[1]["image_url"]["url"], "data:image/png;base64,abc");
    }

    #[test]
    fn gemini_openai_base_not_suffixed_with_v1() {
        let base =
            openai_compatible_base("https://generativelanguage.googleapis.com/v1beta/openai");
        assert_eq!(
            base,
            "https://generativelanguage.googleapis.com/v1beta/openai"
        );
        assert!(!base.ends_with("/openai/v1"));
    }

    #[test]
    fn gemini_bare_host_no_longer_remapped_to_openai_compat() {
        // Google chat 已迁 Interactions；openai_compatible_base 不再特判 Gemini host
        assert_eq!(
            openai_compatible_base("https://generativelanguage.googleapis.com"),
            "https://generativelanguage.googleapis.com/v1"
        );
        assert_eq!(
            openai_compatible_base("https://generativelanguage.googleapis.com/v1beta"),
            "https://generativelanguage.googleapis.com/v1beta/v1"
        );
    }

    #[test]
    fn parse_openai_usage_splits_cached_prompt_tokens() {
        let v = serde_json::json!({
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 20,
                "total_tokens": 120,
                "prompt_tokens_details": { "cached_tokens": 40, "cache_write_tokens": 10 }
            }
        });
        let u = parse_openai_usage(&v).expect("usage");
        assert_eq!(u.input_tokens, 50); // 100 - 40 - 10
        assert_eq!(u.output_tokens, 20);
        assert_eq!(u.cache_read_tokens, 40);
        assert_eq!(u.cache_write_tokens, 10);
        assert_eq!(u.prompt_tokens(), 100);
        assert_eq!(u.completion_tokens(), 20);
    }
}
