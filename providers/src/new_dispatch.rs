//! 新分发入口 — 基于 trait 系统的 `chat_stream_for_provider` 替代。
//!
//! 接受旧类型签名（兼容下游），内部通过 bridge 转为新类型、走新管线。

use anyhow::Result;
use futures::StreamExt;
use serde_json::Value;

use crate::bridge;
use crate::trait_::{ChatMessage, ChatStream, ProviderConfig};
use crate::types::request::{CompletionRequest, ThinkingConfig};

/// 新管线分发（旧签名兼容）。
///
/// 将旧 `ChatMessage` 转为新 `Message`，通过 `NewRegistry` 查找 provider，
/// 获取 `CompletionStream`（新 StreamChunk），再转回旧 `ChatChunk`。
pub async fn chat_stream_new(
    provider: &str,
    messages: Vec<ChatMessage>,
    tools: Vec<Value>,
    config: &ProviderConfig,
) -> Result<ChatStream> {
    // 1. 旧消息 → 新消息
    let new_messages: Vec<crate::types::Message> = messages
        .iter()
        .map(bridge::legacy_to_message)
        .collect();

    // 2. 旧工具 → 新工具定义
    let new_tools: Vec<crate::types::ToolDefinition> = tools
        .iter()
        .filter_map(|t| {
            let f = t.get("function").unwrap_or(t);
            Some(crate::types::ToolDefinition {
                name: f.get("name")?.as_str()?.to_string(),
                description: f
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string(),
                parameters: f
                    .get("parameters")
                    .cloned()
                    .unwrap_or(serde_json::json!({"type": "object", "properties": {}})),
            })
        })
        .collect();

    // 3. 构造 CompletionRequest
    let request = CompletionRequest {
        model: config.model.clone(),
        messages: new_messages,
        tools: new_tools,
        temperature: Some(config.temperature),
        max_tokens: Some(config.max_tokens),
        thinking: if config.thinking_enabled {
            Some(ThinkingConfig {
                enabled: true,
                budget_tokens: None,
                effort: config.reasoning_effort.clone(),
            })
        } else {
            None
        },
        additional_params: config.additional_params.clone(),
        previous_interaction_id: config.previous_interaction_id.clone(),
    };

    // 4. 建立 registry 并注册 provider
    let mut reg = crate::new_registry::NewRegistry::new();
    register_provider(&mut reg, provider, config);

    // 5. 查找模型并发起流式请求
    let dyn_model = reg
        .completion_model(provider)
        .ok_or_else(|| anyhow::anyhow!("未知 provider: {provider}"))?;

    let new_stream = dyn_model.stream(request).await?;

    // 6. 新 StreamChunk → 旧 ChatChunk
    let legacy_stream = new_stream.filter_map(|result| async move {
        match result {
            Ok(chunk) => bridge::stream_chunk_to_legacy(&chunk).map(Ok),
            Err(e) => Some(Err(e)),
        }
    });

    Ok(Box::pin(legacy_stream))
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
        // OpenAI 兼容厂商 — 使用泛型方法
        "openai" => register_compat::<crate::impls::openai::OpenAI>(reg, key, base, model),
        "deepseek" => register_compat::<crate::impls::deepseek::DeepSeek>(reg, key, base, model),
        "azure" => register_compat::<crate::impls::azure::Azure>(reg, key, base, model),
        "zhipu" => register_compat::<crate::impls::zhipu::Zhipu>(reg, key, base, model),
        "moonshot" => register_compat::<crate::impls::moonshot::Moonshot>(reg, key, base, model),
        "ollama" => register_compat::<crate::impls::ollama::Ollama>(reg, key, base, model),
        "nvidia" => register_compat::<crate::impls::nvidia::Nvidia>(reg, key, base, model),
        "bailian" => register_compat::<crate::impls::bailian::Bailian>(reg, key, base, model),
        "volcengine" => register_compat::<crate::impls::volcengine::Volcengine>(reg, key, base, model),
        "openrouter" => register_compat::<crate::impls::openrouter::OpenRouter>(reg, key, base, model),
        "minimax" | "minmax" => register_compat::<crate::impls::minimax_new::MiniMaxNew>(reg, key, base, model),
        "hunyuan" => register_compat::<crate::impls::hunyuan::Hunyuan>(reg, key, base, model),
        // 未知 → OpenAI 兼容回退
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
}
