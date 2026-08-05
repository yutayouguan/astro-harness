//! 协议管线注册表 — 按 provider id 构造并缓存 trait-based 模型实例。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::facade::{AiProvider, ChatProvider, VerifyProvider};
use crate::profile::{AuthKind, ImageGenMode, ProviderProfile, PROFILES};
use crate::shared::verify::VerifyResult;
use crate::traits::client::{ChatClient, EmbedClient, ImageGenClient, TTSClient, VideoGenClient, MusicGenClient, ProviderClient};
use crate::traits::dyn_provider::DynProvider;
use crate::types::media::{GeneratedAudio, GeneratedImage, GeneratedVideo};
use crate::types::message::Message;
use crate::types::request::ProviderConfig;
use crate::types::stream::CompletionStream;
use crate::verify;

fn shared_http_client() -> reqwest::Client {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .pool_max_idle_per_host(8)
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
            + crate::traits::Capabilities<Chat = crate::traits::Capable<crate::compat::OpenAICompletionModel<Ext>>>
            + Default
            + Copy
            + 'static,
    {
        let client = self.make_client(api_key, base_url, Ext::default());
        let provider = DynProvider::new(Ext::NAME, Ext::NAME)
            .with_completion(client.completion_model(model));
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
            >
            + Default
            + Copy
            + 'static,
    {
        let client = self.make_client(api_key, base_url, Ext::default());
        let provider = DynProvider::new(Ext::NAME, Ext::NAME)
            .with_completion(client.completion_model(model))
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
            .with_completion(client.completion_model(model))
            .with_embedding(client.embedding_model(model))
            .with_image_gen(client.image_model(model))
            .with_tts(client.tts_model(model));
        self.providers.insert("openai".to_string(), provider);
    }

    /// 注册 Anthropic provider（仅 Chat）。
    pub fn register_anthropic(&mut self, api_key: &str, base_url: Option<&str>, model: &str) {
        use crate::impls::anthropic::Anthropic;
        let client = self.make_client(api_key, base_url, Anthropic);
        let provider = DynProvider::new("anthropic", "anthropic")
            .with_completion(client.completion_model(model));
        self.providers.insert("claude".to_string(), provider.clone());
        self.providers.insert("anthropic".to_string(), provider);
    }

    /// 注册 Google provider（全能力）。
    pub fn register_google(&mut self, api_key: &str, base_url: Option<&str>, model: &str) {
        use crate::impls::google::Google;
        let client = self.make_client(api_key, base_url, Google);
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
        use crate::impls::minimax_chat::MiniMax;
        let client = self.make_client(api_key, base_url, MiniMax);
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
        let completion = crate::impls::openai_responses::ResponsesCompletionModel::new(
            self.http.clone(),
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
        let client = self.make_client(api_key, base_url, GeminiNative);
        let provider = DynProvider::new("gemini-native", "gemini-native")
            .with_completion(client.completion_model(model));
        self.providers.insert("gemini-native".to_string(), provider);
    }

    /// 按 id 查找 provider。
    pub fn get(&self, id: &str) -> Option<&DynProvider> {
        let normalized = crate::profile::normalize_provider_id(id);
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

// ─── 旧 API 兼容：ProviderRegistry（门面 trait 驱动）────────────────

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
                Arc::new(RegistryProvider::new(profile)),
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
        self.providers.get(name).cloned().or_else(|| {
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
    pub async fn verify(&self, name: &str, model: &str, config: &ProviderConfig) -> VerifyResult {
        if let Some(provider) = self.get(name) {
            return provider.verify(model, config).await;
        }
        let client = reqwest::Client::new();
        verify::probe(&client, name, model, config).await
    }
}

impl Default for ProviderRegistry {
    /// 等价于 [`ProviderRegistry::new`]。
    fn default() -> Self {
        Self::new()
    }
}

// ─── 内部 Provider 实现 ─────────────────────────────────────

/// 由静态 [`ProviderProfile`] 驱动的供应商实现。
///
/// Chat 通过 trait 管线分发（[`crate::dispatch`]），
/// Verify 通过 `impls/` 模块探测，
/// 媒体能力（Image / TTS / Video / Music / Embed）通过 profile 路由到专用 HTTP 模块。
struct RegistryProvider {
    client: reqwest::Client,
    profile: &'static ProviderProfile,
}

impl RegistryProvider {
    fn new(profile: &'static ProviderProfile) -> Self {
        Self {
            client: reqwest::Client::new(),
            profile,
        }
    }
}

#[async_trait]
impl ChatProvider for RegistryProvider {
    async fn chat_stream(
        &self,
        messages: Vec<Message>,
        tools: Vec<serde_json::Value>,
        config: &ProviderConfig,
    ) -> anyhow::Result<CompletionStream> {
        use crate::types::request::{CompletionRequest, ThinkingConfig};
        use crate::types::message::ToolDefinition;

        // tools JSON → ToolDefinition
        let new_tools: Vec<ToolDefinition> = tools
            .iter()
            .filter_map(|t| {
                let f = t.get("function").unwrap_or(t);
                Some(ToolDefinition {
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

        let request = CompletionRequest {
            model: config.model.clone(),
            messages,
            tools: new_tools,
            temperature: Some(config.temperature),
            max_tokens: Some(config.max_tokens),
            thinking: Some(ThinkingConfig {
                enabled: config.thinking_enabled,
                budget_tokens: None,
                effort: config.reasoning_effort.clone(),
            }),
            additional_params: config.additional_params.clone(),
            previous_interaction_id: config.previous_interaction_id.clone(),
        };

        crate::dispatch::chat_stream_direct(self.profile.id, request, config).await
    }
}

#[async_trait]
impl VerifyProvider for RegistryProvider {
    async fn verify(&self, model: &str, config: &ProviderConfig) -> VerifyResult {
        verify::probe(&self.client, self.profile.id, model, config).await
    }
}

#[async_trait]
impl AiProvider for RegistryProvider {
    fn name(&self) -> &str {
        self.profile.id
    }

    fn default_model(&self) -> &str {
        self.profile.default_model
    }

    fn supports_image_gen(&self) -> bool {
        self.profile.supports_image_gen
    }

    fn supports_embedding(&self) -> bool {
        self.profile.supports_embedding
    }

    fn auth_kind(&self) -> AuthKind {
        self.profile.auth
    }

    async fn generate_image(
        &self,
        prompt: &str,
        config: &ProviderConfig,
    ) -> anyhow::Result<Vec<GeneratedImage>> {
        let mode = self
            .profile
            .image_mode
            .ok_or_else(|| anyhow::anyhow!("{} 不支持图片生成", self.profile.id))?;
        let mut cfg = config.clone();
        if cfg.model.trim().is_empty() {
            cfg.model = if self.profile.default_image_model.is_empty() {
                "gpt-image-2".to_string()
            } else {
                self.profile.default_image_model.to_string()
            };
        }
        match mode {
            ImageGenMode::OpenAi => {
                crate::image_http::openai_generate_image(&self.client, prompt, &cfg).await
            }
            ImageGenMode::GoogleInteractions => {
                use crate::interactions_http::{google_interactions_image, InteractionImageRequest};
                let request = InteractionImageRequest {
                    prompt: prompt.to_string(),
                    ..Default::default()
                };
                let result = google_interactions_image(&self.client, &cfg, &request).await?;
                Ok(vec![result.image])
            }
            ImageGenMode::MiniMax => {
                let request = crate::minimax::image_http::MiniMaxImageRequest {
                    prompt: prompt.to_string(),
                    ..Default::default()
                };
                crate::minimax::image_http::minimax_generate_image(&self.client, &cfg, &request)
                    .await
            }
        }
    }

    async fn text_to_speech(
        &self,
        text: &str,
        config: &ProviderConfig,
    ) -> anyhow::Result<GeneratedAudio> {
        if !self.profile.supports_tts() {
            anyhow::bail!("{} 不支持语音合成 (TTS)", self.profile.id);
        }
        let mut cfg = config.clone();
        if cfg.model.trim().is_empty() {
            cfg.model = self.profile.default_tts_model.to_string();
        }
        match self.profile.id {
            "minimax" => {
                let req = crate::minimax::tts_http::MiniMaxTtsRequest {
                    text: text.to_string(),
                    ..Default::default()
                };
                let result =
                    crate::minimax::tts_http::minimax_tts(&self.client, &cfg, &req).await?;
                Ok(GeneratedAudio {
                    data: result.audio_bytes,
                    mime_type: result.mime_type,
                    duration_ms: result.duration_ms,
                })
            }
            _ => {
                let req = crate::tts_http::OpenAiTtsRequest {
                    model: cfg.model.clone(),
                    input: text.to_string(),
                    voice: "alloy".to_string(),
                    response_format: "mp3".to_string(),
                    speed: 1.0,
                };
                let result = crate::tts_http::openai_tts(&self.client, &cfg, &req).await?;
                Ok(GeneratedAudio {
                    data: result.audio_bytes,
                    mime_type: result.mime_type,
                    duration_ms: 0,
                })
            }
        }
    }

    async fn generate_video(
        &self,
        prompt: &str,
        config: &ProviderConfig,
    ) -> anyhow::Result<GeneratedVideo> {
        if !self.profile.supports_video() {
            anyhow::bail!("{} 不支持视频生成", self.profile.id);
        }
        let mut cfg = config.clone();
        if cfg.model.trim().is_empty() {
            cfg.model = self.profile.default_video_model.to_string();
        }
        match self.profile.id {
            "minimax" => {
                let req = crate::minimax::video_http::MiniMaxVideoRequest {
                    prompt: prompt.to_string(),
                    model: cfg.model.clone(),
                    ..Default::default()
                };
                let task_id = crate::minimax::video_http::minimax_create_video(
                    &self.client,
                    &cfg,
                    &req,
                )
                .await?;
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10 * 60);
                loop {
                    if std::time::Instant::now() > deadline {
                        anyhow::bail!("MiniMax 视频生成超时（task_id={task_id}）");
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    let status = crate::minimax::video_http::minimax_query_video(
                        &self.client, &cfg, &task_id,
                    ).await?;
                    if status.status == crate::minimax::video_http::VideoTaskStatus::Success {
                        if let Some(file_id) = status.file_id {
                            let result = crate::minimax::video_http::minimax_download_video(
                                &self.client, &cfg, &file_id,
                            ).await?;
                            break Ok(GeneratedVideo {
                                data: result.data,
                                mime_type: result.mime_type,
                                width: status.video_width.unwrap_or(0),
                                height: status.video_height.unwrap_or(0),
                            });
                        }
                        anyhow::bail!("MiniMax 视频生成成功但无 file_id");
                    }
                    if !status.status.is_pending() {
                        anyhow::bail!("MiniMax 视频生成失败: {:?}", status.status);
                    }
                }
            }
            _ => {
                anyhow::bail!(
                    "{} 的视频生成 API 暂未对接，请使用 MiniMax",
                    self.profile.id
                )
            }
        }
    }

    async fn generate_music(
        &self,
        prompt: &str,
        config: &ProviderConfig,
    ) -> anyhow::Result<GeneratedAudio> {
        if !self.profile.supports_music() {
            anyhow::bail!("{} 不支持音乐生成", self.profile.id);
        }
        let mut cfg = config.clone();
        if cfg.model.trim().is_empty() {
            cfg.model = self.profile.default_music_model.to_string();
        }
        match self.profile.id {
            "minimax" => {
                let req = crate::minimax::music_http::MiniMaxMusicRequest {
                    prompt: prompt.to_string(),
                    ..Default::default()
                };
                let result = crate::minimax::music_http::minimax_generate_music(
                    &self.client,
                    &cfg,
                    &req,
                )
                .await?;
                Ok(GeneratedAudio {
                    data: result.audio_bytes,
                    mime_type: result.mime_type,
                    duration_ms: result.duration_ms,
                })
            }
            _ => {
                anyhow::bail!(
                    "{} 的音乐生成 API 暂未对接，请使用 MiniMax",
                    self.profile.id
                )
            }
        }
    }

    async fn embed(
        &self,
        texts: &[String],
        config: &ProviderConfig,
    ) -> anyhow::Result<Vec<Vec<f32>>> {
        if !self.profile.supports_embedding {
            anyhow::bail!("{} 不支持文本嵌入", self.profile.id);
        }
        let mut cfg = config.clone();
        if cfg.model.trim().is_empty() {
            cfg.model = if self.profile.default_embedding_model.is_empty() {
                "text-embedding-3-small".to_string()
            } else {
                self.profile.default_embedding_model.to_string()
            };
        }
        crate::embeddings_http::openai_batch_embed(&self.client, texts, &cfg.model, &cfg).await
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
