//! OpenAI / Azure / Responses API 连通性探测。

use reqwest::Client;
use serde_json::{json, Value};

use super::azure::{azure_base, AZURE_API_VERSION};
use super::chat::openai_compatible_base;
use crate::trait_::ProviderConfig;

/// 从各厂商错误响应中提取人类可读消息。
///
/// 覆盖格式：
/// - OpenAI / Azure: `{"error":{"message":"..."}}`
/// - MiniMax base_resp: `{"base_resp":{"status_msg":"..."}}`
/// - Anthropic 兼容: `{"error":{"type":"...","message":"..."}}`
/// - 纯字符串: `{"error":"..."}`
fn extract_error_message(v: &Value) -> &str {
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
    let url = format!("{}/chat/completions", openai_compatible_base(endpoint));
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
    let url = format!("{}/responses", openai_compatible_base(endpoint));
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

/// Azure OpenAI 连通性探测（deployment URL + api-key 认证）。
pub async fn probe_azure(
    client: &Client,
    model: &str,
    config: &ProviderConfig,
) -> Result<String, String> {
    let endpoint = config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("");
    let base = azure_base(endpoint);
    let url = format!(
        "{base}/openai/deployments/{model}/chat/completions?api-version={AZURE_API_VERSION}"
    );
    let body = json!({
        "max_tokens": 1,
        "messages": [{"role": "user", "content": "ping"}]
    });
    let resp = client
        .post(&url)
        .header("api-key", &config.api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("连接 Azure OpenAI 失败: {e}"))?;
    let status = resp.status();
    let json: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("失败 ({status}): {}", extract_error_message(&json)));
    }
    Ok("调用成功".to_string())
}
