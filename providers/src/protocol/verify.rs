//! 连通性探测：按 [`crate::profile::ApiMode`] 路由。

use reqwest::Client;
use serde_json::json;

use crate::http_stream::{azure_base, openai_compatible_base, AZURE_API_VERSION};
use crate::profile::{self, ApiMode};
use crate::trait_::{ProviderConfig, VerifyResult};

/// 优先使用配置中的 `base_url`，否则使用给定 fallback。
fn resolve_endpoint(config: &ProviderConfig, fallback: &str) -> String {
    config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

/// OpenAI 兼容 chat/completions 最小探测。
async fn probe_chat_completions(
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

/// 按 provider_id 路由协议并探测（网络/协议错误收敛为 `ok: false`）。
pub async fn probe(
    client: &Client,
    provider_id: &str,
    model: &str,
    config: &ProviderConfig,
) -> VerifyResult {
    let started = std::time::Instant::now();
    let model = model.to_string();
    let fail = |message: String| VerifyResult {
        ok: false,
        latency_ms: started.elapsed().as_millis() as u64,
        model: model.clone(),
        message,
    };

    let message = if let Some(p) = profile::resolve(provider_id) {
        match (p.api_mode, p.azure_deployment_style) {
            (ApiMode::AnthropicMessages, _) => {
                let endpoint = resolve_endpoint(config, p.default_base_url);
                match crate::anthropic::verify::probe_anthropic(
                    client, &model, &endpoint, config,
                )
                .await
                {
                    Ok(m) => m,
                    Err(e) => return fail(e),
                }
            }
            (ApiMode::ChatCompletions, true) => {
                let endpoint = resolve_endpoint(config, "");
                let base = azure_base(&endpoint);
                let url = format!(
                    "{base}/openai/deployments/{model}/chat/completions?api-version={AZURE_API_VERSION}"
                );
                let body = json!({
                    "max_tokens": 1,
                    "messages": [{"role": "user", "content": "ping"}]
                });
                let resp = match client
                    .post(&url)
                    .header("api-key", &config.api_key)
                    .header("content-type", "application/json")
                    .json(&body)
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(e) => return fail(format!("连接 Azure OpenAI 失败: {e}")),
                };
                let status = resp.status();
                let json: serde_json::Value = match resp.json().await {
                    Ok(v) => v,
                    Err(e) => return fail(e.to_string()),
                };
                if !status.is_success() {
                    let msg = json["error"]["message"].as_str().unwrap_or("未知错误");
                    return fail(format!("失败 ({status}): {msg}"));
                }
                "调用成功".to_string()
            }
            (ApiMode::Responses, _) => {
                return fail("ApiMode::Responses 探测尚未接线".into());
            }
            (ApiMode::Interactions, _) => {
                match crate::google::interactions_chat::probe_interactions(client, &model, config)
                    .await
                {
                    Ok(m) => m,
                    Err(e) => return fail(e),
                }
            }
            (ApiMode::GeminiNative, _) => {
                // 原生 generateContent 不提供单独探测端点，复用 OpenAI 兼容路径
                let endpoint = resolve_endpoint(config, p.default_base_url);
                match probe_chat_completions(client, &endpoint, &model, &config.api_key).await {
                    Ok(m) => m,
                    Err(e) => return fail(e),
                }
            }
            (ApiMode::ChatCompletions, false) => {
                let endpoint = resolve_endpoint(config, p.default_base_url);
                match probe_chat_completions(client, &endpoint, &model, &config.api_key).await {
                    Ok(m) => m,
                    Err(e) => return fail(e),
                }
            }
        }
    } else {
        // custom / 未知 id：OpenAI 兼容
        let endpoint = resolve_endpoint(config, crate::openai::DEFAULT_API_BASE);
        match probe_chat_completions(client, &endpoint, &model, &config.api_key).await {
            Ok(m) => m,
            Err(e) => return fail(e),
        }
    };

    VerifyResult {
        ok: true,
        latency_ms: started.elapsed().as_millis() as u64,
        model,
        message,
    }
}
