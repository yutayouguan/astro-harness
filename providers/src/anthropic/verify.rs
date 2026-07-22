//! Anthropic Messages API 连通性探测。

use reqwest::Client;
use serde_json::json;

use super::defaults;
use crate::trait_::ProviderConfig;

/// 向 Anthropic Messages API 发送最小请求以验证连通性。
pub async fn probe_anthropic(
    client: &Client,
    model: &str,
    endpoint: &str,
    config: &ProviderConfig,
) -> Result<String, String> {
    let url = format!(
        "{}/v1/messages",
        endpoint.trim_end_matches('/')
    );
    let body = json!({
        "model": model,
        "max_tokens": 1,
        "messages": [{"role": "user", "content": "ping"}]
    });
    let resp = client
        .post(&url)
        .header("x-api-key", &config.api_key)
        .header("anthropic-version", defaults::ANTHROPIC_VERSION)
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
