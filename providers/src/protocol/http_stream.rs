//! 真实 HTTP 流式聊天：Chat Completions / Anthropic Messages / Azure quirk。
//!
//! 按 [`crate::profile::ApiMode`] 分发；不再保留 Google native / Ollama NDJSON。

use anyhow::{anyhow, Context, Result};
use futures::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::profile::{self, ApiMode};
use crate::streaming::Usage;
use crate::tool_format::openai_tools_to_anthropic;
use crate::trait_::{
    ChatChunk, ChatContentPart, ChatMessage, ChatStream, ProviderConfig, ToolCallDeltaChunk,
};

/// 将 `additional_params` 浅合并进请求体（对象字段覆盖同名键；非对象则忽略）
pub fn merge_additional_params(body: &mut Value, params: &Value) {
    let (Some(obj), Some(extra)) = (body.as_object_mut(), params.as_object()) else {
        return;
    };
    for (k, v) in extra {
        obj.insert(k.clone(), v.clone());
    }
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

/// 去掉 endpoint 末尾斜杠。
fn trim_slash(endpoint: &str) -> String {
    endpoint.trim_end_matches('/').to_string()
}

/// 规范化 OpenAI 兼容 API 基址（自动补 `/v1` 等后缀）。
///
/// Gemini OpenAI 兼容基址以 `/openai` 结尾，不得再追加 `/v1`。
/// 规范化 OpenAI 兼容 API 基址。
///
/// Google Gemini 聊天已迁 Interactions，勿再经此函数拼 `/v1beta/openai`。
pub fn openai_compatible_base(endpoint: &str) -> String {
    let base = trim_slash(endpoint);
    if base.ends_with("/v1")
        || base.ends_with("/v3")
        || base.ends_with("/v4")
        || base.ends_with("/openai")
        || base.contains("/paas/v4")
        || base.contains("/v1beta/openai")
    {
        base
    } else if base.is_empty() {
        crate::openai::DEFAULT_API_BASE.to_string()
    } else {
        format!("{base}/v1")
    }
}

/// 从 Azure endpoint 提取资源根路径（去掉 `/openai`、`/v1` 后缀）。
pub fn azure_base(endpoint: &str) -> String {
    trim_slash(endpoint)
        .trim_end_matches("/openai")
        .trim_end_matches("/v1")
        .to_string()
}

/// 返回各内置供应商的默认 API 基址（表驱动）。
pub fn default_base_for(provider: &str) -> &'static str {
    profile::default_base_for(provider)
}

/// 按 [`ApiMode`] 分发流式聊天（Azure quirk 走专用 URL）。
pub async fn chat_stream_for_provider(
    client: &Client,
    provider: &str,
    messages: Vec<ChatMessage>,
    tools: Vec<Value>,
    config: &ProviderConfig,
) -> Result<ChatStream> {
    let p = profile::resolve_or_openai_compat(provider);
    // 未知 id 仍用调用方传入的 provider 字符串拼 URL / 日志
    let id = if profile::resolve(provider).is_some() {
        p.id
    } else {
        provider
    };
    match p.api_mode {
        ApiMode::ChatCompletions if p.azure_deployment_style => {
            azure_chat_stream(client, messages, tools, config).await
        }
        ApiMode::ChatCompletions => {
            openai_compatible_chat_stream(client, id, messages, tools, config).await
        }
        ApiMode::AnthropicMessages => anthropic_chat_stream(client, messages, tools, config).await,
        ApiMode::Interactions => {
            crate::google::interactions_chat::interactions_chat_stream(
                client, messages, tools, config,
            )
            .await
        }
        ApiMode::Responses => {
            crate::openai::responses::responses_chat_stream(client, id, messages, tools, config)
                .await
        }
        ApiMode::GeminiNative => {
            crate::google::native_chat::gemini_native_chat_stream(client, messages, tools, config)
                .await
        }
    }
}

/// 优先使用配置中的 `base_url`，否则回退到供应商默认值。
fn resolve_base(config: &ProviderConfig, provider: &str) -> String {
    config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_base_for(provider))
        .to_string()
}

/// 将工具参数 JSON 转为 OpenAI 请求所需的字符串形式。
fn args_to_openai_string(args: &Value) -> String {
    match args {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// 将内部 [`ChatMessage`] 列表转为 OpenAI 兼容 `messages` 数组。
fn to_openai_messages(messages: &[ChatMessage]) -> Vec<Value> {
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
    })
}

/// 从 JSON 响应体提取 `/error/message` 或顶层 `error` 字符串，用于 Google API 错误格式化。
/// 返回 `None` 表示响应中没有错误字段（非错误响应或未知格式）。
pub(crate) fn json_error_option(v: &Value) -> Option<&str> {
    v.pointer("/error/message")
        .and_then(|m| m.as_str())
        .or_else(|| v.get("error").and_then(|e| e.as_str()))
}

/// [`json_error_option`] 的带默认值版本，等价于 `.unwrap_or(default)`。
pub(crate) fn json_error_message<'a>(v: &'a Value, default: &'a str) -> &'a str {
    json_error_option(v).unwrap_or(default)
}

/// 将 reqwest 字节流解析为 SSE `data:` 行，并用 `extract` 转为 [`ChatChunk`]。
/// 检查 HTTP 响应状态；非 2xx 时消耗响应体并返回结构化错误，成功时原样返回响应。
pub(crate) async fn check_response_status(
    response: reqwest::Response,
) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    let msg = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .and_then(|m| m.as_str())
                .map(str::to_string)
                .or_else(|| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
        })
        .unwrap_or(body);
    Err(anyhow!("上游 HTTP {status}: {msg}"))
}

pub(crate) async fn sse_chat_stream(
    response: reqwest::Response,
    extract: Arc<dyn Fn(&str) -> Option<ChatChunk> + Send + Sync>,
) -> Result<ChatStream> {
    let response = check_response_status(response).await?;
    let byte_stream = response.bytes_stream();
    let stream = futures::stream::unfold(
        (byte_stream, String::new(), false, extract),
        |(mut byte_stream, mut buf, done, extract)| async move {
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
                    if let Some(data) = trimmed.strip_prefix("data:") {
                        let data = data.trim();
                        if data == "[DONE]" {
                            // 正常结束：不注入假 finish_reason:stop，避免干扰后续 tool_calls 判定
                            return None;
                        }
                        if let Some(chunk) = extract(data) {
                            if chunk
                                .finish_reason
                                .as_deref()
                                .is_some_and(|f| f.starts_with("error:"))
                            {
                                let msg = chunk
                                    .finish_reason
                                    .unwrap()
                                    .trim_start_matches("error:")
                                    .to_string();
                                return Some((
                                    Err(anyhow!(msg)),
                                    (byte_stream, buf, true, extract),
                                ));
                            }
                            return Some((Ok(chunk), (byte_stream, buf, false, extract)));
                        }
                    }
                    continue;
                }

                match byte_stream.next().await {
                    Some(Ok(bytes)) => {
                        buf.push_str(&String::from_utf8_lossy(&bytes));
                    }
                    Some(Err(err)) => {
                        return Some((Err(err.into()), (byte_stream, buf, true, extract)));
                    }
                    None => {
                        if !buf.trim().is_empty() {
                            let line = buf.trim().to_string();
                            buf.clear();
                            if let Some(data) = line.strip_prefix("data:") {
                                let data = data.trim();
                                if data != "[DONE]" {
                                    if let Some(chunk) = extract(data) {
                                        return Some((
                                            Ok(chunk),
                                            (byte_stream, buf, true, extract),
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

/// 判断该供应商是否支持 `stream_options.include_usage`。
fn supports_stream_include_usage(provider: &str) -> bool {
    // 部分兼容网关会拒 stream_options；仅对确认支持的上游开启
    matches!(
        provider,
        "openai" | "azure" | "deepseek" | "openrouter" | "nvidia" | "moonshot" | "mimo" | "ollama"
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
            let effort = match config.reasoning_effort.trim() {
                "max" | "xhigh" => "max",
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

/// 解析 Anthropic SSE 事件 JSON 为 [`ChatChunk`]。
fn extract_anthropic_delta(data: &str) -> Option<ChatChunk> {
    let v: Value = serde_json::from_str(data).ok()?;
    let event_type = v.get("type")?.as_str()?;
    match event_type {
        "content_block_start" => {
            let block = v.get("content_block")?;
            if block.get("type").and_then(|t| t.as_str()) != Some("tool_use") {
                return None;
            }
            let index = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
            Some(ChatChunk {
                tool_call_deltas: vec![ToolCallDeltaChunk {
                    index,
                    id: block.get("id").and_then(|s| s.as_str()).map(str::to_string),
                    name: block
                        .get("name")
                        .and_then(|s| s.as_str())
                        .map(str::to_string),
                    arguments: None,
                    signature: None,
                }],
                ..Default::default()
            })
        }
        "content_block_delta" => {
            let delta = v.get("delta")?;
            let index = v.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as u32;
            let delta_type = delta.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if delta_type == "input_json_delta" {
                let partial = delta
                    .get("partial_json")
                    .and_then(|s| s.as_str())
                    .map(str::to_string);
                return Some(ChatChunk {
                    tool_call_deltas: vec![ToolCallDeltaChunk {
                        index,
                        id: None,
                        name: None,
                        arguments: partial,
                        signature: None,
                    }],
                    ..Default::default()
                });
            }
            let token = delta
                .get("text")
                .and_then(|t| t.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_string)?;
            Some(ChatChunk {
                token: Some(token),
                ..Default::default()
            })
        }
        "message_delta" => {
            let finish = v
                .pointer("/delta/stop_reason")
                .and_then(|s| s.as_str())
                .map(str::to_string);
            let usage = v.get("usage").and_then(|u| {
                let out = u.get("output_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                let input = u.get("input_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                if out == 0 && input == 0 {
                    None
                } else {
                    Some(Usage::from_parts(input, out))
                }
            });
            if finish.is_none() && usage.is_none() {
                return None;
            }
            Some(ChatChunk {
                finish_reason: finish,
                usage,
                ..Default::default()
            })
        }
        "error" => {
            let msg = v
                .pointer("/error/message")
                .and_then(|m| m.as_str())
                .unwrap_or("Anthropic 错误");
            Some(ChatChunk {
                finish_reason: Some(format!("error:{msg}")),
                ..Default::default()
            })
        }
        _ => None,
    }
}

/// 将消息列表转为 Anthropic `(system, messages)` 二元组。
fn to_anthropic_messages(messages: &[ChatMessage]) -> (String, Vec<Value>) {
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
fn anthropic_user_content(m: &ChatMessage) -> Value {
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

pub(crate) fn parse_data_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(";base64,")?;
    let media_type = meta.trim();
    if media_type.is_empty() || data.is_empty() {
        return None;
    }
    Some((media_type.to_string(), data.to_string()))
}

/// Anthropic Messages API 流式
pub async fn anthropic_chat_stream(
    client: &Client,
    messages: Vec<ChatMessage>,
    tools: Vec<Value>,
    config: &ProviderConfig,
) -> Result<ChatStream> {
    if config.api_key.is_empty() {
        return Err(anyhow!("缺少 Anthropic API Key"));
    }
    let base = trim_slash(&resolve_base(config, "claude"));
    let url = format!("{base}/v1/messages");

    let (system, api_messages) = to_anthropic_messages(&messages);

    let mut body = json!({
        "model": config.model,
        "max_tokens": config.max_tokens,
        "stream": true,
        "messages": api_messages,
    });
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    let anthropic_tools = openai_tools_to_anthropic(&tools);
    if !anthropic_tools.is_empty() {
        body["tools"] = Value::Array(anthropic_tools);
    }

    merge_additional_params(&mut body, &config.additional_params);
    let response = client
        .post(&url)
        .header("x-api-key", &config.api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 Anthropic 失败: {url}"))?;

    sse_chat_stream(response, Arc::new(extract_anthropic_delta)).await
}

/// Azure OpenAI REST API 版本号。
pub const AZURE_API_VERSION: &str = "2024-06-01";

/// Azure OpenAI 流式 chat completions（ChatCompletions + URL/Auth quirk）。
pub async fn azure_chat_stream(
    client: &Client,
    messages: Vec<ChatMessage>,
    tools: Vec<Value>,
    config: &ProviderConfig,
) -> Result<ChatStream> {
    if config.api_key.is_empty() {
        return Err(anyhow!("缺少 Azure OpenAI API Key"));
    }
    let base = azure_base(&resolve_base(config, "azure"));
    if base.is_empty() || base.contains("YOUR_RESOURCE") {
        return Err(anyhow!("请配置有效的 Azure OpenAI endpoint"));
    }
    let deployment = &config.model;
    let url = format!(
        "{base}/openai/deployments/{deployment}/chat/completions?api-version={AZURE_API_VERSION}"
    );

    let mut body = json!({
        "messages": to_openai_messages(&messages),
        "stream": true,
        "temperature": config.temperature,
        "max_tokens": config.max_tokens,
    });
    // Azure OpenAI 支持 stream_options.include_usage
    body["stream_options"] = json!({ "include_usage": true });
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
        body["tool_choice"] = json!("auto");
    }

    merge_additional_params(&mut body, &config.additional_params);
    let response = client
        .post(&url)
        .header("api-key", &config.api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 Azure OpenAI 失败: {url}"))?;

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
        assert!(!supports_stream_include_usage("minimax"));
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
