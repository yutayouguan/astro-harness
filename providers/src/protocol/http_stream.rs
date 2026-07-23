//! 共享基础设施 + 中央分发入口。
//!
//! 聊天分发已迁移到 trait-based `new_dispatch`。
//! 本模块保留 SSE 工具函数供旧代码路径使用。

use anyhow::{anyhow, Result};
use futures::StreamExt;
use reqwest::Client;
use serde_json::Value;
use std::sync::Arc;

use crate::trait_::{ChatChunk, ChatMessage, ChatStream, ProviderConfig};

/// 将 `additional_params` 浅合并进请求体（对象字段覆盖同名键；非对象则忽略）
pub fn merge_additional_params(body: &mut Value, params: &Value) {
    let (Some(obj), Some(extra)) = (body.as_object_mut(), params.as_object()) else {
        return;
    };
    for (k, v) in extra {
        obj.insert(k.clone(), v.clone());
    }
}

/// 去掉 endpoint 末尾斜杠。
pub(crate) fn trim_slash(endpoint: &str) -> String {
    endpoint.trim_end_matches('/').to_string()
}

/// 返回各内置供应商的默认 API 基址（表驱动）。
pub fn default_base_for(provider: &str) -> &'static str {
    crate::profile::default_base_for(provider)
}

/// 优先使用配置中的 `base_url`，否则回退到供应商默认值。
pub(crate) fn resolve_base(config: &ProviderConfig, provider: &str) -> String {
    config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_base_for(provider))
        .to_string()
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

/// 从 JSON 响应体提取 `/error/message` 或顶层 `error` 字符串。
pub(crate) fn json_error_option(v: &Value) -> Option<&str> {
    v.pointer("/error/message")
        .and_then(|m| m.as_str())
        .or_else(|| v.get("error").and_then(|e| e.as_str()))
}

/// [`json_error_option`] 的带默认值版本。
pub(crate) fn json_error_message<'a>(v: &'a Value, default: &'a str) -> &'a str {
    json_error_option(v).unwrap_or(default)
}

/// 检查 HTTP 响应状态；非 2xx 时消耗响应体并返回结构化错误。
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

type ChatChunkExtract = Arc<dyn Fn(&str) -> Option<ChatChunk> + Send + Sync>;

/// 将 reqwest 字节流解析为 SSE `data:` 行，并用 `extract` 转为 [`ChatChunk`]。
pub(crate) async fn sse_chat_stream(
    response: reqwest::Response,
    extract: ChatChunkExtract,
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

/// 按 provider id 分发流式聊天。
///
/// **新管线**：通过 trait-based `NewRegistry` 分发。
/// 旧 `ApiMode` match 已移除 — 所有厂商通过统一 trait 系统处理。
///
/// 内部将旧 `ChatMessage` 内联转为新 `Message`，调用 `new_dispatch::chat_stream_new`。
pub async fn chat_stream_for_provider(
    _client: &Client,
    provider: &str,
    messages: Vec<ChatMessage>,
    tools: Vec<Value>,
    config: &ProviderConfig,
) -> Result<ChatStream> {
    // 直接委托给 new_dispatch（内含内联转换，不走 bridge）
    crate::new_dispatch::chat_stream_new(provider, messages, tools, config).await
}
