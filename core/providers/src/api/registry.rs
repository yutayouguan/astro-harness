//! 供应商注册表：按 id 查找并路由到内部 Provider 实现。

use std::collections::HashMap;
use std::sync::Arc;

use crate::profile::{ImageGenMode, ProviderProfile, PROFILES};
use crate::trait_::{
    AiProvider, AuthKind, ChatProvider, CompletionStream, GeneratedAudio, GeneratedImage,
    GeneratedVideo, Message, ProviderConfig, VerifyProvider, VerifyResult,
};
use crate::verify;
use async_trait::async_trait;

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

/// 由静态 [`ProviderProfile`] 驱动的供应商实现（取代旧 `ProfileBackedProvider`）。
///
/// Chat 通过新 trait 管线分发（[`crate::new_dispatch`]），
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

        crate::new_dispatch::chat_stream_direct(self.profile.id, request, config).await
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
                    duration_ms: Some(result.duration_ms),
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
                    duration_ms: None,
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
                Ok(GeneratedVideo {
                    url: None,
                    task_id: Some(task_id),
                    data: None,
                    mime_type: "video/mp4".to_string(),
                })
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
                    duration_ms: Some(result.duration_ms),
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
