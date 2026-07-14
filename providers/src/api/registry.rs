//! 供应商注册表：按 id 查找并路由到 [`ProfileBackedProvider`]。

use std::collections::HashMap;
use std::sync::Arc;

use crate::profile::PROFILES;
use crate::trait_::{AiProvider, ProviderConfig, VerifyResult};
use crate::vendors::profile_backed::ProfileBackedProvider;

/// 内置供应商实例的注册表。
#[derive(Clone)]
pub struct ProviderRegistry {
    /// provider id → 共享的 [`AiProvider`] 实现。
    providers: HashMap<String, Arc<dyn AiProvider>>,
}

impl ProviderRegistry {
    /// 构造并注册所有内置 profile。
    pub fn new() -> Self {
        let mut map: HashMap<String, Arc<dyn AiProvider>> = HashMap::new();
        for profile in PROFILES {
            map.insert(
                profile.id.to_string(),
                Arc::new(ProfileBackedProvider::new(profile)),
            );
        }
        ProviderRegistry { providers: map }
    }

    /// 注册或覆盖一个供应商（测试用自定义 Provider / 运行时注入）。
    pub fn insert(&mut self, name: impl Into<String>, provider: Arc<dyn AiProvider>) {
        self.providers.insert(name.into(), provider);
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
