//! 协议管线注册表 — 按 provider id 构造并缓存 trait-based 模型实例。

use std::collections::HashMap;

use crate::traits::client::{ChatClient, EmbedClient, ImageGenClient, TTSClient, VideoGenClient, MusicGenClient, ProviderClient};
use crate::traits::dyn_provider::DynProvider;

/// Provider 注册表 — 基于 trait 系统。
#[derive(Clone)]
pub struct Registry {
    providers: HashMap<String, DynProvider>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    /// 注册 OpenAI 兼容 provider（仅 Chat）。
    pub fn register_openai_compat<Ext>(
        &mut self,
        api_key: &str,
        base_url: Option<&str>,
        model: &str,
    ) where
        Ext: crate::compat::OpenAICompatible
            + crate::traits::ProviderExt
            + crate::traits::Capabilities<Chat = crate::traits::Capable<crate::compat::OpenAICompletionModel<Ext>>>
            + Default
            + Copy
            + 'static,
    {
        let mut client = ProviderClient::new(api_key, Ext::default());
        if let Some(url) = base_url {
            client = client.with_base_url(url);
        }
        let completion_model = client.completion_model(model);
        let provider = DynProvider::new(Ext::NAME, Ext::NAME)
            .with_completion(completion_model);
        self.providers.insert(Ext::NAME.to_string(), provider);
    }

    /// 注册 OpenAI provider（Chat + Embedding + ImageGen + TTS）。
    pub fn register_openai(&mut self, api_key: &str, base_url: Option<&str>, model: &str) {
        use crate::impls::openai::OpenAI;
        let mut client = ProviderClient::new(api_key, OpenAI);
        if let Some(url) = base_url {
            client = client.with_base_url(url);
        }
        let provider = DynProvider::new("openai", "openai")
            .with_completion(client.completion_model(model))
            .with_embedding(client.embedding_model(model))
            .with_image_gen(client.image_model(model))
            .with_tts(client.tts_model(model));
        self.providers.insert("openai".to_string(), provider);
    }

    /// 注册 Anthropic provider（仅 Chat）。
    pub fn register_anthropic(&mut self, api_key: &str, base_url: Option<&str>, model: &str) {
        use crate::impls::anthropic::Anthropic;
        let mut client = ProviderClient::new(api_key, Anthropic);
        if let Some(url) = base_url {
            client = client.with_base_url(url);
        }
        let provider = DynProvider::new("anthropic", "anthropic")
            .with_completion(client.completion_model(model));
        self.providers.insert("claude".to_string(), provider.clone());
        self.providers.insert("anthropic".to_string(), provider);
    }

    /// 注册 Google provider（全能力）。
    pub fn register_google(&mut self, api_key: &str, base_url: Option<&str>, model: &str) {
        use crate::impls::google::Google;
        let mut client = ProviderClient::new(api_key, Google);
        if let Some(url) = base_url {
            client = client.with_base_url(url);
        }
        let provider = DynProvider::new("google", "google")
            .with_completion(client.completion_model(model))
            .with_embedding(client.embedding_model(model))
            .with_image_gen(client.image_model(model))
            .with_video_gen(client.video_model(model))
            .with_tts(client.tts_model(model))
            .with_music_gen(client.music_model(model));
        self.providers.insert("google".to_string(), provider);
    }

    /// 注册 MiniMax provider（全能力）。
    pub fn register_minimax(&mut self, api_key: &str, base_url: Option<&str>, model: &str) {
        use crate::impls::minimax_new::MiniMaxNew;
        let mut client = ProviderClient::new(api_key, MiniMaxNew);
        if let Some(url) = base_url {
            client = client.with_base_url(url);
        }
        let provider = DynProvider::new("minimax", "minimax")
            .with_completion(client.completion_model(model))
            .with_embedding(client.embedding_model(model))
            .with_image_gen(client.image_model(model))
            .with_video_gen(client.video_model(model))
            .with_tts(client.tts_model(model))
            .with_music_gen(client.music_model(model));
        self.providers.insert("minimax".to_string(), provider);
    }

    /// 注册 Responses API provider（OpenAI / MiniMax Responses）。
    pub fn register_responses(
        &mut self,
        id: &'static str,
        api_key: &str,
        base_url: Option<&str>,
        model: &str,
    ) {
        let resolved_base = base_url
            .unwrap_or(crate::profile::default_base_for(id))
            .to_string();
        let http = reqwest::Client::new();
        let completion = crate::impls::openai_responses::ResponsesCompletionModel::new(
            http,
            resolved_base,
            api_key.to_string(),
            model.to_string(),
            id,
        );
        let provider = DynProvider::new(id, id).with_completion(completion);
        self.providers.insert(id.to_string(), provider);
    }

    /// 注册 Gemini Native provider（仅 Chat — streamGenerateContent）。
    pub fn register_gemini_native(&mut self, api_key: &str, base_url: Option<&str>, model: &str) {
        use crate::impls::gemini_native::GeminiNative;
        let mut client = ProviderClient::new(api_key, GeminiNative);
        if let Some(url) = base_url {
            client = client.with_base_url(url);
        }
        let provider = DynProvider::new("gemini-native", "gemini-native")
            .with_completion(client.completion_model(model));
        self.providers.insert("gemini-native".to_string(), provider);
    }

    /// 按 id 查找 provider。
    pub fn get(&self, id: &str) -> Option<&DynProvider> {
        let normalized = match id {
            "minimax-anthropic" | "minmax" | "minmax-anthropic" => "minimax",
            "claude" | "anthropic" => "anthropic",
            other => other,
        };
        self.providers.get(normalized)
    }

    pub fn provider_ids(&self) -> Vec<&str> {
        self.providers.keys().map(|s| s.as_str()).collect()
    }

    pub fn completion_model(&self, id: &str) -> Option<&dyn crate::traits::dyn_provider::DynCompletionModel> {
        self.get(id)?.completion_model()
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_lookup() {
        let mut reg = Registry::new();
        reg.register_anthropic("test-key", None, "claude-opus-4-8");
        assert!(reg.get("anthropic").is_some());
        assert!(reg.get("claude").is_some());
        assert!(reg.get("openai").is_none());
    }

    #[test]
    fn completion_model_lookup() {
        let mut reg = Registry::new();
        reg.register_anthropic("test-key", None, "claude-opus-4-8");
        assert!(reg.completion_model("anthropic").is_some());
        assert!(reg.completion_model("openai").is_none());
    }

    #[test]
    fn openai_has_all_media_capabilities() {
        let mut reg = Registry::new();
        reg.register_openai("test-key", None, "gpt-4o");
        let p = reg.get("openai").unwrap();
        assert!(p.completion_model().is_some());
        assert!(p.embedding_model().is_some());
        assert!(p.image_gen_model().is_some());
        assert!(p.tts_model().is_some());
    }

    #[test]
    fn google_has_all_capabilities() {
        let mut reg = Registry::new();
        reg.register_google("test-key", None, "gemini-3.5-flash");
        let p = reg.get("google").unwrap();
        assert!(p.completion_model().is_some());
        assert!(p.embedding_model().is_some());
        assert!(p.image_gen_model().is_some());
        assert!(p.video_gen_model().is_some());
        assert!(p.tts_model().is_some());
        assert!(p.music_gen_model().is_some());
    }

    #[test]
    fn minimax_has_all_capabilities() {
        let mut reg = Registry::new();
        reg.register_minimax("test-key", None, "MiniMax-M2.5");
        let p = reg.get("minimax").unwrap();
        assert!(p.completion_model().is_some());
        assert!(p.embedding_model().is_some());
        assert!(p.image_gen_model().is_some());
        assert!(p.video_gen_model().is_some());
        assert!(p.tts_model().is_some());
        assert!(p.music_gen_model().is_some());
    }
}
