//! 新注册表 — 按 provider id 动态查找，内部使用 trait-based 实现。

use std::collections::HashMap;

use crate::traits::client::{ChatClient, ProviderClient};
use crate::traits::dyn_provider::DynProvider;

/// 新 ProviderRegistry — 基于 trait 系统。
#[derive(Clone)]
pub struct NewRegistry {
    providers: HashMap<String, DynProvider>,
}

impl NewRegistry {
    /// 使用默认配置创建注册表（所有内置厂商，需要 API key 后才能实际使用）。
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    /// 注册一个 provider（传入 api_key 和可选 base_url 即可用）。
    pub fn register_openai_compat<Ext>(
        &mut self,
        api_key: &str,
        base_url: Option<&str>,
        model: &str,
    ) where
        Ext: crate::compat::OpenAICompatible
            + crate::traits::ProviderExt
            + crate::traits::Capabilities<Chat = crate::traits::Capable<crate::compat::OpenAICompletionModel<Ext>>>
            + Copy
            + 'static,
    {
        let ext = unsafe { std::mem::zeroed::<Ext>() };
        let mut client = ProviderClient::new(api_key, ext);
        if let Some(url) = base_url {
            client = client.with_base_url(url);
        }
        let completion_model = client.completion_model(model);
        let provider = DynProvider::new(Ext::NAME, Ext::NAME)
            .with_completion(completion_model);
        self.providers.insert(Ext::NAME.to_string(), provider);
    }

    /// 注册 Anthropic provider。
    pub fn register_anthropic(&mut self, api_key: &str, base_url: Option<&str>, model: &str) {
        use crate::impls::anthropic::Anthropic;
        let mut client = ProviderClient::new(api_key, Anthropic);
        if let Some(url) = base_url {
            client = client.with_base_url(url);
        }
        let completion_model = client.completion_model(model);
        let provider = DynProvider::new("anthropic", "anthropic")
            .with_completion(completion_model);
        self.providers.insert("claude".to_string(), provider.clone());
        self.providers.insert("anthropic".to_string(), provider);
    }

    /// 注册 Google provider。
    pub fn register_google(&mut self, api_key: &str, base_url: Option<&str>, model: &str) {
        use crate::impls::google::Google;
        let mut client = ProviderClient::new(api_key, Google);
        if let Some(url) = base_url {
            client = client.with_base_url(url);
        }
        let completion_model = client.completion_model(model);
        let provider = DynProvider::new("google", "google")
            .with_completion(completion_model);
        self.providers.insert("google".to_string(), provider);
    }

    /// 按 id 查找 provider。
    pub fn get(&self, id: &str) -> Option<&DynProvider> {
        let normalized = match id {
            "minimax-responses" | "minimax-anthropic" | "minmax" | "minmax-anthropic" => "minimax",
            "claude" | "anthropic" => "anthropic",
            other => other,
        };
        self.providers.get(normalized)
    }

    /// 列出所有已注册的 provider id。
    pub fn provider_ids(&self) -> Vec<&str> {
        self.providers.keys().map(|s| s.as_str()).collect()
    }

    /// 按 id 获取补全模型（运行时动态 dispatch）。
    pub fn completion_model(&self, id: &str) -> Option<&dyn crate::traits::dyn_provider::DynCompletionModel> {
        self.get(id)?.completion_model()
    }
}

impl Default for NewRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_lookup() {
        let mut reg = NewRegistry::new();
        reg.register_anthropic("test-key", None, "claude-opus-4-8");
        assert!(reg.get("anthropic").is_some());
        assert!(reg.get("claude").is_some());
        assert!(reg.get("openai").is_none());
    }

    #[test]
    fn completion_model_lookup() {
        let mut reg = NewRegistry::new();
        reg.register_anthropic("test-key", None, "claude-opus-4-8");
        assert!(reg.completion_model("anthropic").is_some());
        assert!(reg.completion_model("openai").is_none());
    }
}
