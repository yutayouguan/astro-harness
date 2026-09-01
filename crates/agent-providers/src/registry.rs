//! 协议管线注册表 — 按 provider id 构造并缓存 trait-based 模型实例。

use std::collections::HashMap;

use crate::traits::client::{
    ChatClient, EmbedClient, ImageGenClient, MusicGenClient, ProviderClient, TTSClient,
    VideoGenClient,
};
use crate::traits::dyn_provider::DynProvider;

pub fn shared_http_client() -> reqwest::Client {
    use std::sync::OnceLock;
    use std::time::Duration;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .pool_max_idle_per_host(8)
                .connect_timeout(Duration::from_secs(30))
                .timeout(Duration::from_secs(180))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new())
        })
        .clone()
}

/// Provider 注册表 — 基于 trait 系统。
///
/// 内部持有共享的 `reqwest::Client`，所有 provider 复用同一连接池。
#[derive(Clone)]
pub struct Registry {
    http: reqwest::Client,
    providers: HashMap<String, DynProvider>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            http: shared_http_client(),
            providers: HashMap::new(),
        }
    }

    fn make_client<Ext: crate::traits::ProviderExt>(
        &self,
        api_key: &str,
        base_url: Option<&str>,
        ext: Ext,
    ) -> ProviderClient<Ext> {
        let mut client = ProviderClient::new(api_key, ext).with_http_client(self.http.clone());
        if let Some(url) = base_url {
            client = client.with_base_url(url);
        }
        client
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
            + crate::traits::Capabilities<
                Chat = crate::traits::Capable<crate::compat::OpenAICompletionModel<Ext>>,
            > + Default
            + Copy
            + 'static,
    {
        let client = self.make_client(api_key, base_url, Ext::default());
        let provider = DynProvider::new(Ext::NAME, Ext::NAME)
            .with_chat_completion(client.chat_completion_model(model));
        self.providers.insert(Ext::NAME.to_string(), provider);
    }

    /// 注册 OpenAI 兼容 provider（Chat + Embedding + ImageGen + TTS）。
    ///
    /// 适用于使用 OpenAI 兼容 API 的国产厂商（智谱、百炼、火山、混元等）。
    pub fn register_compat_with_media<Ext>(
        &mut self,
        api_key: &str,
        base_url: Option<&str>,
        model: &str,
    ) where
        Ext: crate::compat::OpenAICompatible
            + crate::traits::ProviderExt
            + crate::traits::Capabilities<
                Chat = crate::traits::Capable<crate::compat::OpenAICompletionModel<Ext>>,
                Embedding = crate::traits::Capable<crate::compat::media::CompatEmbeddingModel>,
                ImageGen = crate::traits::Capable<crate::compat::media::CompatImageGenModel>,
                TTS = crate::traits::Capable<crate::compat::media::CompatTTSModel>,
            > + Default
            + Copy
            + 'static,
    {
        let client = self.make_client(api_key, base_url, Ext::default());
        let provider = DynProvider::new(Ext::NAME, Ext::NAME)
            .with_chat_completion(client.chat_completion_model(model))
            .with_embedding(client.embedding_model(model))
            .with_image_gen(client.image_model(model))
            .with_tts(client.tts_model(model));
        self.providers.insert(Ext::NAME.to_string(), provider);
    }

    /// 注册 OpenAI provider（Chat + Embedding + ImageGen + TTS）。
    pub fn register_openai(&mut self, api_key: &str, base_url: Option<&str>, model: &str) {
        use crate::impls::openai::OpenAI;
        let client = self.make_client(api_key, base_url, OpenAI);
        let provider = DynProvider::new("openai", "openai")
            .with_chat_completion(client.chat_completion_model(model))
            .with_embedding(client.embedding_model(model))
            .with_image_gen(client.image_model(model))
            .with_tts(client.tts_model(model));
        self.providers.insert("openai".to_string(), provider);
    }

    /// 注册 Azure provider（Chat + Embedding + Azure AI Foundry OpenAI v1 ImageGen）。
    pub fn register_azure(&mut self, api_key: &str, base_url: Option<&str>, model: &str) {
        use crate::impls::azure::Azure;
        let endpoint = base_url
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| crate::profile::default_base_for("azure"));
        let normalized_base = crate::impls::azure::azure_openai_v1_base(endpoint);
        let client = self.make_client(api_key, Some(&normalized_base), Azure);
        let provider = DynProvider::new("azure", "azure")
            .with_chat_completion(client.chat_completion_model(model))
            .with_embedding(
                client.embedding_model(
                    crate::profile::resolve("azure")
                        .expect("built-in Azure profile")
                        .default_embedding_model,
                ),
            )
            .with_image_gen(client.image_model(crate::image_gen::default_image_model("azure")));
        self.providers.insert("azure".to_string(), provider);
    }

    /// 注册 Anthropic provider（仅 Chat）。
    pub fn register_anthropic(&mut self, api_key: &str, base_url: Option<&str>, model: &str) {
        use crate::impls::anthropic::Anthropic;
        let client = self.make_client(api_key, base_url, Anthropic);
        let provider = DynProvider::new("anthropic", "anthropic")
            .with_chat_completion(client.chat_completion_model(model));
        self.providers
            .insert("claude".to_string(), provider.clone());
        self.providers.insert("anthropic".to_string(), provider);
    }

    /// 注册 Google provider（全能力）。
    pub fn register_google(&mut self, api_key: &str, base_url: Option<&str>, model: &str) {
        use crate::impls::google::Google;
        let client = self.make_client(api_key, base_url, Google);
        let provider = DynProvider::new("google", "google")
            .with_chat_completion(client.chat_completion_model(model))
            .with_embedding(client.embedding_model(model))
            .with_image_gen(client.image_model(model))
            .with_video_gen(client.video_model(model))
            .with_tts(client.tts_model(model))
            .with_music_gen(client.music_model(model));
        self.providers.insert("google".to_string(), provider);
    }

    /// 注册 MiniMax provider（全能力）。
    pub fn register_minimax(&mut self, api_key: &str, base_url: Option<&str>, model: &str) {
        use crate::impls::minimax_chat::MiniMax;
        let client = self.make_client(api_key, base_url, MiniMax);
        let provider = DynProvider::new("minimax", "minimax")
            .with_chat_completion(client.chat_completion_model(model))
            .with_embedding(client.embedding_model(model))
            .with_image_gen(client.image_model(model))
            .with_video_gen(client.video_model(model))
            .with_tts(client.tts_model(model))
            .with_music_gen(client.music_model(model));
        self.providers.insert("minimax".to_string(), provider);
    }

    /// 为已注册 provider 挂载 Agent 原生 Responses 模型。
    ///
    /// Chat/media 能力继续独立存在，不参与 Agent target/fallback 链。
    /// 通过 `Ext::responses_base_url()` 处理 base_url 变换（如 Azure）。
    pub fn attach_responses<Ext>(
        &mut self,
        id: &str,
        api_key: &str,
        base_url: Option<&str>,
        model: &str,
    ) where
        Ext: crate::compat::OpenAIResponsesCompatible
            + crate::traits::ProviderExt
            + Default
            + Copy
            + 'static,
    {
        use crate::traits::FromClient;
        let ext = Ext::default();
        let responses_base = base_url.map(|b| ext.responses_base_url(b));
        let effective_base = responses_base.as_deref().or(base_url);
        let client = self.make_client(api_key, effective_base, ext);
        let responses = crate::compat::OpenAIResponsesModel::<Ext>::from_client(&client, model);
        if let Some(provider) = self.providers.get_mut(id) {
            provider.replace_responses(responses);
        }
    }

    /// 注册 Gemini Native provider（仅 Chat — streamGenerateContent）。
    pub fn register_gemini_native(&mut self, api_key: &str, base_url: Option<&str>, model: &str) {
        use crate::impls::gemini_native::GeminiNative;
        let client = self.make_client(api_key, base_url, GeminiNative);
        let provider = DynProvider::new("gemini-native", "gemini-native")
            .with_chat_completion(client.chat_completion_model(model));
        self.providers.insert("gemini-native".to_string(), provider);
    }

    /// 注册 TOML 自定义 provider（Responses API only）。
    pub fn register_custom(
        &mut self,
        id: &str,
        config: &crate::custom::CustomProviderConfig,
        api_key: &str,
        model: &str,
    ) {
        let model = if model.is_empty() {
            &config.default_model
        } else {
            model
        };
        let responses = crate::custom::ConfigDrivenResponsesModel::new(
            self.http.clone(),
            config,
            api_key.to_string(),
            model.to_string(),
            id.to_string(),
        );
        let provider = DynProvider::new(id, "custom").with_responses(responses);
        self.providers.insert(id.to_string(), provider);
    }

    /// 按 id 查找 provider。
    pub fn get(&self, id: &str) -> Option<&DynProvider> {
        let normalized = crate::profile::normalize_provider_id(id);
        self.providers.get(normalized)
    }

    /// 为已注册 provider 增加一个等价查找 id。
    pub fn register_alias(&mut self, alias: &str, target: &str) -> bool {
        let Some(provider) = self.providers.get(target).cloned() else {
            return false;
        };
        self.providers.insert(alias.to_string(), provider);
        true
    }

    pub fn provider_ids(&self) -> Vec<&str> {
        self.providers.keys().map(|s| s.as_str()).collect()
    }

    pub fn responses_model(
        &self,
        id: &str,
    ) -> Option<&dyn crate::traits::dyn_provider::DynResponsesModel> {
        self.get(id)?.responses_model()
    }

    pub fn chat_completion_model(
        &self,
        id: &str,
    ) -> Option<&dyn crate::traits::dyn_provider::DynChatCompletionModel> {
        self.get(id)?.chat_completion_model()
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
    fn chat_completion_model_lookup() {
        let mut reg = Registry::new();
        reg.register_anthropic("test-key", None, "claude-opus-4-8");
        assert!(reg.chat_completion_model("anthropic").is_some());
        assert!(reg.responses_model("anthropic").is_none());
    }

    #[test]
    fn openai_has_all_media_capabilities() {
        let mut reg = Registry::new();
        reg.register_openai("test-key", None, "gpt-4o");
        let p = reg.get("openai").unwrap();
        assert!(p.chat_completion_model().is_some());
        assert!(p.responses_model().is_none());
        assert!(p.embedding_model().is_some());
        assert!(p.image_gen_model().is_some());
        assert!(p.tts_model().is_some());
    }

    #[test]
    fn azure_has_chat_embedding_and_image_capabilities() {
        let mut reg = Registry::new();
        reg.register_azure(
            "test-key",
            Some("https://example.services.ai.azure.com/openai/v1"),
            "gpt-image-2",
        );
        let provider = reg.get("azure").expect("azure should be registered");
        assert!(provider.chat_completion_model().is_some());
        assert!(provider.image_gen_model().is_some());
        assert!(provider.embedding_model().is_some());
        assert!(provider.tts_model().is_none());
    }

    #[test]
    fn responses_and_chat_models_are_independent_slots() {
        let mut reg = Registry::new();
        reg.register_openai("test-key", None, "gpt-5.6");
        reg.attach_responses::<crate::impls::openai::OpenAI>("openai", "test-key", None, "gpt-5.6");
        let provider = reg.get("openai").unwrap();
        assert!(provider.responses_model().is_some());
        assert!(provider.chat_completion_model().is_some());
    }

    #[test]
    fn google_has_all_capabilities() {
        let mut reg = Registry::new();
        reg.register_google("test-key", None, "gemini-3.5-flash");
        let p = reg.get("google").unwrap();
        assert!(p.chat_completion_model().is_some());
        assert!(p.responses_model().is_none());
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
        assert!(p.chat_completion_model().is_some());
        assert!(p.responses_model().is_none());
        assert!(p.embedding_model().is_some());
        assert!(p.image_gen_model().is_some());
        assert!(p.video_gen_model().is_some());
        assert!(p.tts_model().is_some());
        assert!(p.music_gen_model().is_some());
    }
}
