//! MiniMax — OpenAI 兼容聊天 + 多媒体能力。

use reqwest::header::HeaderMap;

use crate::compat::{OpenAICompatible, OpenAICompletionModel, ThinkingFormat};
use crate::traits::{
    Capabilities, Capable, EmbeddingModel, FromClient, ImageGenModel, ModelBase, MusicGenModel,
    ProviderClient, ProviderExt, TTSModel, VideoGenModel,
};
use crate::types::media::{
    Embedding, GeneratedAudio, GeneratedImage, GeneratedVideo, ImageGenConfig, MusicGenConfig,
    TTSConfig, VideoGenConfig,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct MiniMax;

impl ProviderExt for MiniMax {
    const NAME: &'static str = "minimax";
    const BASE_URL: &'static str = "https://api.minimaxi.com/v1";
    fn auth_headers(&self, key: &str) -> HeaderMap {
        crate::impls::openai::bearer_headers(key)
    }
}

impl OpenAICompatible for MiniMax {
    const STREAM_USAGE: bool = true;
    const SUPPORTS_RESPONSES: bool = true;
    const THINKING_FORMAT: ThinkingFormat = ThinkingFormat::MiniMaxAdaptive;
}

impl Capabilities for MiniMax {
    type Chat = Capable<OpenAICompletionModel<Self>>;
    type Embedding = Capable<MiniMaxEmbeddingModel>;
    type ImageGen = Capable<MiniMaxImageModel>;
    type VideoGen = Capable<MiniMaxVideoModel>;
    type TTS = Capable<MiniMaxTTSModel>;
    type MusicGen = Capable<MiniMaxMusicModel>;
}

// ─── Embedding Model ────────────────────────────────────

#[derive(Clone)]
pub struct MiniMaxEmbeddingModel(ModelBase);

impl FromClient<MiniMax> for MiniMaxEmbeddingModel {
    fn from_client(client: &ProviderClient<MiniMax>, model: &str) -> Self {
        Self(ModelBase::from_client(client, model))
    }
}

#[async_trait::async_trait]
impl EmbeddingModel for MiniMaxEmbeddingModel {
    async fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Embedding>> {
        let config = self.0.to_provider_config();
        let vectors = crate::openai::embeddings_http::openai_batch_embed(
            self.0.http(),
            texts,
            self.0.model(),
            &config,
        )
        .await?;
        Ok(vectors
            .into_iter()
            .map(|v| Embedding { values: v })
            .collect())
    }
}

// ─── Image Generation Model ────────────────────────────

#[derive(Clone)]
pub struct MiniMaxImageModel(ModelBase);

impl FromClient<MiniMax> for MiniMaxImageModel {
    fn from_client(client: &ProviderClient<MiniMax>, model: &str) -> Self {
        Self(ModelBase::from_client(client, model))
    }
}

#[async_trait::async_trait]
impl ImageGenModel for MiniMaxImageModel {
    async fn generate(
        &self,
        prompt: &str,
        config: &ImageGenConfig,
    ) -> anyhow::Result<Vec<GeneratedImage>> {
        let provider_config = self.0.to_provider_config();
        let req = crate::minimax::image_http::MiniMaxImageRequest {
            model: self.0.model().to_string(),
            prompt: prompt.to_string(),
            aspect_ratio: config
                .aspect_ratio
                .clone()
                .unwrap_or_else(|| "1:1".to_string()),
            width: config.width,
            height: config.height,
            n: if config.n > 0 { config.n } else { 1 },
            ..Default::default()
        };
        crate::minimax::image_http::minimax_generate_image(self.0.http(), &provider_config, &req)
            .await
    }
}

// ─── Video Generation Model ────────────────────────────

#[derive(Clone)]
pub struct MiniMaxVideoModel(ModelBase);

impl FromClient<MiniMax> for MiniMaxVideoModel {
    fn from_client(client: &ProviderClient<MiniMax>, model: &str) -> Self {
        Self(ModelBase::from_client(client, model))
    }
}

#[async_trait::async_trait]
impl VideoGenModel for MiniMaxVideoModel {
    async fn generate(
        &self,
        prompt: &str,
        config: &VideoGenConfig,
    ) -> anyhow::Result<GeneratedVideo> {
        let provider_config = self.0.to_provider_config();
        let req = crate::minimax::video_http::MiniMaxVideoRequest {
            model: self.0.model().to_string(),
            prompt: prompt.to_string(),
            first_frame_image: config.first_frame_image.clone(),
            last_frame_image: config.last_frame_image.clone(),
            duration: if config.duration_seconds > 0 {
                config.duration_seconds
            } else {
                6
            },
            resolution: if config.resolution.is_empty() {
                "768P".to_string()
            } else {
                config.resolution.clone()
            },
            ..Default::default()
        };
        let model_name = self.0.model().to_string();
        let task_id =
            crate::minimax::video_http::minimax_create_video(self.0.http(), &provider_config, &req)
                .await?;

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10 * 60);
        loop {
            if std::time::Instant::now() > deadline {
                anyhow::bail!("MiniMax 视频生成超时（task_id={task_id}）");
            }
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;

            let status = crate::minimax::video_http::minimax_query_video(
                self.0.http(),
                &provider_config,
                &task_id,
                &model_name,
            )
            .await?;
            if status.status == crate::minimax::video_http::VideoTaskStatus::Success {
                if let Some(ref url) = status.download_url {
                    let result =
                        crate::minimax::video_http::minimax_download_video_url(self.0.http(), url)
                            .await?;
                    return Ok(GeneratedVideo {
                        data: result.data,
                        mime_type: result.mime_type,
                        width: result.width,
                        height: result.height,
                    });
                }
                if let Some(ref file_id) = status.file_id {
                    let result = crate::minimax::video_http::minimax_download_video(
                        self.0.http(),
                        &provider_config,
                        file_id,
                    )
                    .await?;
                    return Ok(GeneratedVideo {
                        data: result.data,
                        mime_type: result.mime_type,
                        width: status.video_width.unwrap_or(result.width),
                        height: status.video_height.unwrap_or(result.height),
                    });
                }
                anyhow::bail!("MiniMax 视频生成成功但无下载途径");
            }
            if !status.status.is_pending() {
                anyhow::bail!("MiniMax 视频生成失败: {:?}", status.status);
            }
        }
    }
}

// ─── TTS Model ──────────────────────────────────────────

#[derive(Clone)]
pub struct MiniMaxTTSModel(ModelBase);

impl FromClient<MiniMax> for MiniMaxTTSModel {
    fn from_client(client: &ProviderClient<MiniMax>, model: &str) -> Self {
        Self(ModelBase::from_client(client, model))
    }
}

#[async_trait::async_trait]
impl TTSModel for MiniMaxTTSModel {
    async fn synthesize(
        &self,
        text: &str,
        tts_config: &TTSConfig,
    ) -> anyhow::Result<GeneratedAudio> {
        let provider_config = self.0.to_provider_config();
        let mut voice_setting = crate::minimax::tts_http::VoiceSetting::default();
        if !tts_config.voice_id.is_empty() {
            voice_setting.voice_id = tts_config.voice_id.clone();
        }
        if tts_config.speed > 0.0 {
            voice_setting.speed = tts_config.speed;
        }
        let req = crate::minimax::tts_http::MiniMaxTtsRequest {
            model: self.0.model().to_string(),
            text: text.to_string(),
            voice_setting,
            output_format: "hex".to_string(),
            ..Default::default()
        };
        let result =
            crate::minimax::tts_http::minimax_tts(self.0.http(), &provider_config, &req).await?;
        Ok(GeneratedAudio {
            data: result.audio_bytes,
            mime_type: result.mime_type,
            duration_ms: result.duration_ms,
        })
    }
}

// ─── Music Generation Model ────────────────────────────

#[derive(Clone)]
pub struct MiniMaxMusicModel(ModelBase);

impl FromClient<MiniMax> for MiniMaxMusicModel {
    fn from_client(client: &ProviderClient<MiniMax>, model: &str) -> Self {
        Self(ModelBase::from_client(client, model))
    }
}

#[async_trait::async_trait]
impl MusicGenModel for MiniMaxMusicModel {
    async fn generate(
        &self,
        prompt: &str,
        config: &MusicGenConfig,
    ) -> anyhow::Result<GeneratedAudio> {
        let provider_config = self.0.to_provider_config();
        let req = crate::minimax::music_http::MiniMaxMusicRequest {
            model: self.0.model().to_string(),
            prompt: prompt.to_string(),
            lyrics: config.lyrics.clone().unwrap_or_default(),
            is_instrumental: config.is_instrumental,
            output_format: "url".to_string(),
            ..Default::default()
        };
        let result = crate::minimax::music_http::minimax_generate_music(
            self.0.http(),
            &provider_config,
            &req,
        )
        .await?;
        Ok(GeneratedAudio {
            data: result.audio_bytes,
            mime_type: result.mime_type,
            duration_ms: result.duration_ms,
        })
    }
}
