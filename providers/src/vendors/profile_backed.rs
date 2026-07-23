//! 表驱动的统一 [`AiProvider`]：按 [`ProviderProfile`] 分发协议。

use crate::http_stream::chat_stream_for_provider;
use crate::image_http::openai_generate_image;
use crate::interactions_http::{google_interactions_image, InteractionImageRequest};
use crate::profile::{resolve, ImageGenMode, ProviderProfile};
use crate::trait_::*;
use crate::verify;
use async_trait::async_trait;

/// 由静态 [`ProviderProfile`] 驱动的供应商实现。
pub struct ProfileBackedProvider {
    client: reqwest::Client,
    profile: &'static ProviderProfile,
}

impl ProfileBackedProvider {
    /// 使用给定 profile 构造。
    pub fn new(profile: &'static ProviderProfile) -> Self {
        Self {
            client: reqwest::Client::new(),
            profile,
        }
    }

    /// 按 id 查找 profile；未知 id 返回 `None`。
    pub fn try_from_id(id: &str) -> Option<Self> {
        resolve(id).map(Self::new)
    }
}

#[async_trait]
impl ChatProvider for ProfileBackedProvider {
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<serde_json::Value>,
        config: &ProviderConfig,
    ) -> anyhow::Result<ChatStream> {
        chat_stream_for_provider(&self.client, self.profile.id, messages, tools, config).await
    }
}

#[async_trait]
impl VerifyProvider for ProfileBackedProvider {
    async fn verify(&self, model: &str, config: &ProviderConfig) -> VerifyResult {
        verify::probe(&self.client, self.profile.id, model, config).await
    }
}

#[async_trait]
impl AiProvider for ProfileBackedProvider {
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
            ImageGenMode::OpenAi => openai_generate_image(&self.client, prompt, &cfg).await,
            ImageGenMode::GoogleInteractions => {
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
                let result = crate::minimax::tts_http::minimax_tts(&self.client, &cfg, &req).await?;
                Ok(GeneratedAudio { data: result.audio_bytes, mime_type: result.mime_type, duration_ms: Some(result.duration_ms) })
            }
            _ => {
                // OpenAI 兼容 TTS
                let req = crate::tts_http::OpenAiTtsRequest {
                    model: cfg.model.clone(),
                    input: text.to_string(),
                    voice: "alloy".to_string(),
                    response_format: "mp3".to_string(),
                    speed: 1.0,
                };
                let result = crate::tts_http::openai_tts(&self.client, &cfg, &req).await?;
                Ok(GeneratedAudio { data: result.audio_bytes, mime_type: result.mime_type, duration_ms: None })
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
                let task_id = crate::minimax::video_http::minimax_create_video(&self.client, &cfg, &req).await?;
                Ok(GeneratedVideo {
                    url: None, task_id: Some(task_id), data: None,
                    mime_type: "video/mp4".to_string(),
                })
            }
            _ => {
                anyhow::bail!("{} 的视频生成 API 暂未对接，请使用 MiniMax", self.profile.id)
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
                let result = crate::minimax::music_http::minimax_generate_music(&self.client, &cfg, &req).await?;
                Ok(GeneratedAudio { data: result.audio_bytes, mime_type: result.mime_type, duration_ms: Some(result.duration_ms) })
            }
            _ => {
                anyhow::bail!("{} 的音乐生成 API 暂未对接，请使用 MiniMax", self.profile.id)
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
