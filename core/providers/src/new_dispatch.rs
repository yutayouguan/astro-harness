//! 新分发入口 — 基于 trait 系统的 `chat_stream_direct` 统一入口。
//!
//! 直接接受新 `Message` 和 `CompletionRequest`，返回 `CompletionStream`。

use anyhow::Result;

use crate::types::request::{CompletionRequest, ProviderConfig};
use crate::types::stream::CompletionStream;

/// 新管线分发（新类型签名）。
///
/// 直接接受 `CompletionRequest`，返回新 `CompletionStream`。
pub async fn chat_stream_direct(
    provider: &str,
    request: CompletionRequest,
    config: &ProviderConfig,
) -> Result<CompletionStream> {
    let provider = normalize_provider_id(provider);
    let mut reg = crate::new_registry::NewRegistry::new();
    register_provider(&mut reg, provider, config);

    let dyn_model = reg
        .completion_model(provider)
        .ok_or_else(|| anyhow::anyhow!("未知 provider: {provider}"))?;

    dyn_model.stream(request).await
}

fn normalize_provider_id(id: &str) -> &str {
    match id {
        "minimax-responses" | "minimax-anthropic" | "minmax" | "minmax-anthropic" => "minimax",
        "anthropic" => "claude",
        other => other,
    }
}

/// 根据 provider id 注册到新注册表。
fn register_provider(
    reg: &mut crate::new_registry::NewRegistry,
    provider: &str,
    config: &ProviderConfig,
) {
    let key = &config.api_key;
    let base = config.base_url.as_deref().filter(|s| !s.trim().is_empty());
    let model = &config.model;

    match provider {
        "anthropic" | "claude" => reg.register_anthropic(key, base, model),
        "google" => reg.register_google(key, base, model),
        "openai" => reg.register_openai(key, base, model),
        "deepseek" => register_compat::<crate::impls::deepseek::DeepSeek>(reg, key, base, model),
        "azure" => register_compat::<crate::impls::azure::Azure>(reg, key, base, model),
        "zhipu" => register_compat::<crate::impls::zhipu::Zhipu>(reg, key, base, model),
        "moonshot" => register_compat::<crate::impls::moonshot::Moonshot>(reg, key, base, model),
        "ollama" => register_compat::<crate::impls::ollama::Ollama>(reg, key, base, model),
        "nvidia" => register_compat::<crate::impls::nvidia::Nvidia>(reg, key, base, model),
        "bailian" => register_compat::<crate::impls::bailian::Bailian>(reg, key, base, model),
        "volcengine" => register_compat::<crate::impls::volcengine::Volcengine>(reg, key, base, model),
        "openrouter" => register_compat::<crate::impls::openrouter::OpenRouter>(reg, key, base, model),
        "minimax" | "minmax" => reg.register_minimax(key, base, model),
        "hunyuan" => register_compat::<crate::impls::hunyuan::Hunyuan>(reg, key, base, model),
        "mimo" => register_compat::<crate::impls::mimo::Mimo>(reg, key, base, model),
        "gemini-native" => reg.register_gemini_native(key, base, model),
        _ => register_compat::<crate::impls::openai::OpenAI>(reg, key, base, model),
    }
}

fn register_compat<Ext>(
    reg: &mut crate::new_registry::NewRegistry,
    api_key: &str,
    base_url: Option<&str>,
    model: &str,
)
where
    Ext: crate::compat::OpenAICompatible
        + crate::traits::ProviderExt
        + crate::traits::Capabilities<Chat = crate::traits::Capable<crate::compat::OpenAICompletionModel<Ext>>>
        + Default
        + Copy
        + 'static,
{
    reg.register_openai_compat::<Ext>(api_key, base_url, model);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_all_providers() {
        let config = ProviderConfig::default();
        let providers = [
            "openai", "anthropic", "claude", "deepseek", "google",
            "azure", "zhipu", "moonshot", "ollama", "nvidia",
            "bailian", "volcengine", "openrouter", "minimax", "hunyuan",
            "mimo", "gemini-native",
        ];
        for id in providers {
            let mut reg = crate::new_registry::NewRegistry::new();
            register_provider(&mut reg, id, &config);
            assert!(
                reg.completion_model(id).is_some(),
                "provider {id} should have completion model"
            );
        }
    }

    #[test]
    fn minimax_variant_ids_resolve() {
        let config = ProviderConfig::default();
        for id in ["minimax-responses", "minimax-anthropic", "minmax", "minmax-anthropic"] {
            let normalized = normalize_provider_id(id);
            let mut reg = crate::new_registry::NewRegistry::new();
            register_provider(&mut reg, normalized, &config);
            assert!(
                reg.completion_model(normalized).is_some(),
                "provider alias {id} (normalized to {normalized}) should resolve"
            );
        }
    }
}
