//! 供应商注册表：按 id 查找并路由到具体 [`AiProvider`] 实现。

use std::collections::HashMap;
use std::sync::Arc;
use crate::trait_::{AiProvider, ProviderConfig, VerifyResult};
use crate::vendors::{
    azure::AzureProvider, bailian::BailianProvider, claude::ClaudeProvider,
    deepseek::DeepSeekProvider, google::GoogleProvider, mimo::MimoProvider,
    minimax::MiniMaxProvider, moonshot::MoonshotProvider, nvidia::NvidiaProvider,
    ollama::OllamaProvider, openai::OpenAiProvider, openrouter::OpenRouterProvider,
    volcengine::VolcengineProvider, zhipu::ZhipuProvider,
};

/// 内置供应商实例的注册表。
#[derive(Clone)]
pub struct ProviderRegistry {
    /// provider id → 共享的 [`AiProvider`] 实现。
    providers: HashMap<String, Arc<dyn AiProvider>>,
}

impl ProviderRegistry {
    /// 构造并注册所有内置供应商。
    pub fn new() -> Self {
        let mut map: HashMap<String, Arc<dyn AiProvider>> = HashMap::new();
        map.insert("google".to_string(), Arc::new(GoogleProvider::new()));
        map.insert("openai".to_string(), Arc::new(OpenAiProvider::new()));
        map.insert("claude".to_string(), Arc::new(ClaudeProvider::new()));
        map.insert("deepseek".to_string(), Arc::new(DeepSeekProvider::new()));
        map.insert("minimax".to_string(), Arc::new(MiniMaxProvider::new()));
        map.insert("openrouter".to_string(), Arc::new(OpenRouterProvider::new()));
        map.insert("bailian".to_string(), Arc::new(BailianProvider::new()));
        map.insert("nvidia".to_string(), Arc::new(NvidiaProvider::new()));
        map.insert("moonshot".to_string(), Arc::new(MoonshotProvider::new()));
        map.insert("volcengine".to_string(), Arc::new(VolcengineProvider::new()));
        map.insert("zhipu".to_string(), Arc::new(ZhipuProvider::new()));
        map.insert("azure".to_string(), Arc::new(AzureProvider::new()));
        map.insert("mimo".to_string(), Arc::new(MimoProvider::new()));
        map.insert("ollama".to_string(), Arc::new(OllamaProvider::new()));
        ProviderRegistry { providers: map }
    }

    /// 按名称获取供应商；支持 `minmax`→`minimax`、`anthropic`→`claude` 别名。
    pub fn get(&self, name: &str) -> Option<Arc<dyn AiProvider>> {
        self.providers
            .get(name)
            .cloned()
            .or_else(|| {
                if name == "minmax" {
                    self.providers.get("minimax").cloned()
                } else if name == "anthropic" {
                    self.providers.get("claude").cloned()
                } else {
                    None
                }
            })
    }

    /// 列出所有已注册供应商 id。
    pub fn list(&self) -> Vec<&str> {
        self.providers.keys().map(String::as_str).collect()
    }

    /// 连通性探测；未知 provider 时仍按 openai 兼容协议用 `verify::probe` 兜底（如 custom）。
    pub async fn verify(
        &self,
        name: &str,
        model: &str,
        config: &ProviderConfig,
    ) -> VerifyResult {
        if let Some(provider) = self.get(name) {
            return provider.verify(model, config).await;
        }
        // custom / 未知 id：走 OpenAI 兼容探测
        let client = reqwest::Client::new();
        crate::verify::probe(&client, name, model, config).await
    }
}

impl Default for ProviderRegistry {
    /// 等价于 [`ProviderRegistry::new`]。
    fn default() -> Self {
        Self::new()
    }
}
