//! 连通性探测：按 [`crate::profile::ApiMode`] 路由。

use reqwest::Client;

use crate::profile::{self, ApiMode};
use crate::types::request::ProviderConfig;

/// 连通性探测结果。
#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub model: String,
    pub message: String,
}

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
        let effective_mode = if config.api_mode == "responses" && p.supports_responses {
            ApiMode::Responses
        } else {
            p.api_mode
        };
        match (effective_mode, p.azure_deployment_style) {
            (ApiMode::AnthropicMessages, _) => {
                let endpoint = resolve_endpoint(config, p.default_base_url);
                match crate::impls::anthropic::probe_anthropic(client, &model, &endpoint, config)
                    .await
                {
                    Ok(m) => m,
                    Err(e) => return fail(e),
                }
            }
            (ApiMode::ChatCompletions, true) => {
                match crate::impls::azure::probe_azure(client, &model, config).await {
                    Ok(m) => m,
                    Err(e) => return fail(e),
                }
            }
            (ApiMode::Responses, _) => {
                let endpoint = resolve_endpoint(config, p.default_base_url);
                match crate::impls::openai::probe_openai_responses(
                    client,
                    &endpoint,
                    &model,
                    &config.api_key,
                )
                .await
                {
                    Ok(m) => m,
                    Err(e) => return fail(e),
                }
            }
            (ApiMode::Interactions, _) => {
                match crate::impls::google::probe_interactions(client, &model, config).await {
                    Ok(m) => m,
                    Err(e) => return fail(e),
                }
            }
            (ApiMode::GeminiNative, _) | (ApiMode::ChatCompletions, false) => {
                let endpoint = resolve_endpoint(config, p.default_base_url);
                match crate::impls::openai::probe_openai_compat(
                    client,
                    &endpoint,
                    &model,
                    &config.api_key,
                )
                .await
                {
                    Ok(m) => m,
                    Err(e) => return fail(e),
                }
            }
        }
    } else {
        let endpoint = resolve_endpoint(config, crate::openai::DEFAULT_API_BASE);
        match crate::impls::openai::probe_openai_compat(client, &endpoint, &model, &config.api_key)
            .await
        {
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
