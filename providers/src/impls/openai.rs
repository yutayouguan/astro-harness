//! OpenAI — 基础 OpenAI 兼容 + Bearer auth 辅助 + 连通性探测。

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use reqwest::Client;
use serde_json::{json, Value};

use crate::compat::{OpenAICompatible, OpenAICompletionModel};
use crate::traits::{Capable, Capabilities, Nothing, ProviderExt};

/// Bearer token 认证 header（OpenAI 及大多数兼容厂商共用）。
pub fn bearer_headers(api_key: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if !api_key.is_empty() {
        if let Ok(val) = HeaderValue::from_str(&format!("Bearer {api_key}")) {
            headers.insert(AUTHORIZATION, val);
        }
    }
    headers
}

#[derive(Debug, Clone, Copy)]
pub struct OpenAI;

impl ProviderExt for OpenAI {
    const NAME: &'static str = "openai";
    const BASE_URL: &'static str = "https://api.openai.com/v1";
    fn auth_headers(&self, key: &str) -> HeaderMap {
        bearer_headers(key)
    }
}

impl OpenAICompatible for OpenAI {
    const STREAM_USAGE: bool = true;
}

impl Capabilities for OpenAI {
    type Chat = Capable<OpenAICompletionModel<Self>>;
    type Embedding = Nothing; // TODO: Phase 3 — Capable<OpenAIEmbeddingModel>
    type ImageGen = Nothing;  // TODO: Phase 3 — Capable<OpenAIImageModel>
    type VideoGen = Nothing;
    type TTS = Nothing;       // TODO: Phase 3 — Capable<OpenAITTSModel>
    type MusicGen = Nothing;
    type ASR = Nothing;
}

// ─── 连通性探测 ─────────────────────────────────────────

/// 从各厂商错误响应中提取人类可读消息。
pub(crate) fn extract_error_message(v: &Value) -> &str {
    v.pointer("/error/message")
        .and_then(|m| m.as_str())
        .or_else(|| v.pointer("/base_resp/status_msg").and_then(|m| m.as_str()))
        .or_else(|| v.get("error").and_then(|e| e.as_str()))
        .or_else(|| v.get("message").and_then(|m| m.as_str()))
        .filter(|s| !s.is_empty())
        .unwrap_or("未知错误")
}

/// OpenAI 兼容 chat/completions 最小探测。
pub async fn probe_openai_compat(
    client: &Client,
    endpoint: &str,
    model: &str,
    api_key: &str,
) -> Result<String, String> {
    let url = format!(
        "{}/chat/completions",
        crate::compat::openai_compatible_base(endpoint)
    );
    let body = json!({
        "model": model,
        "max_tokens": 1,
        "messages": [{"role": "user", "content": "ping"}]
    });
    let mut req = client.post(&url).json(&body);
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("连接模型服务失败: {e}"))?;
    let status = resp.status();
    let json: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("失败 ({status}): {}", extract_error_message(&json)));
    }
    Ok("调用成功".to_string())
}

/// Responses API 最小探测（`POST {base}/responses`）。
pub async fn probe_openai_responses(
    client: &Client,
    endpoint: &str,
    model: &str,
    api_key: &str,
) -> Result<String, String> {
    let url = format!(
        "{}/responses",
        crate::compat::openai_compatible_base(endpoint)
    );
    let body = json!({
        "model": model,
        "input": "ping",
        "max_output_tokens": 1,
    });
    let mut req = client.post(&url).json(&body);
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("连接 Responses API 失败: {e}"))?;
    let status = resp.status();
    let json: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("失败 ({status}): {}", extract_error_message(&json)));
    }
    Ok("调用成功".to_string())
}
