//! OpenAI / Azure 连通性探测。

use reqwest::Client;
use serde_json::json;

use super::azure::{azure_base, AZURE_API_VERSION};
use super::chat::openai_compatible_base;
use crate::trait_::ProviderConfig;

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
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let msg = json["error"]["message"].as_str().unwrap_or("未知错误");
        return Err(format!("失败 ({status}): {msg}"));
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
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let msg = json["error"]["message"].as_str().unwrap_or("未知错误");
        return Err(format!("失败 ({status}): {msg}"));
    }
    Ok("调用成功".to_string())
}
