//! OpenAI 兼容媒体模型 — 泛型版本，任何 `ProviderExt` 厂商可直接复用。
//!
//! 与 `impls/openai.rs` 中的 `OpenAIEmbeddingModel` 等功能相同，
//! 但 `FromClient<Ext>` 是泛型的，不绑定特定厂商。

use crate::traits::{EmbeddingModel, FromClient, ImageGenModel, ModelBase, ProviderClient, ProviderExt, TTSModel};
use crate::types::media::{Embedding, GeneratedAudio, GeneratedImage, ImageGenConfig, TTSConfig};

// ─── Embedding ──────────────────────────────────────────

#[derive(Clone)]
pub struct CompatEmbeddingModel(ModelBase);

impl<Ext: ProviderExt> FromClient<Ext> for CompatEmbeddingModel {
    fn from_client(client: &ProviderClient<Ext>, model: &str) -> Self {
        Self(ModelBase::from_client(client, model))
    }
}

#[async_trait::async_trait]
impl EmbeddingModel for CompatEmbeddingModel {
    async fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Embedding>> {
        let cfg = self.0.to_provider_config();
        let vectors =
            crate::openai::embeddings_http::openai_batch_embed(self.0.http(), texts, self.0.model(), &cfg)
                .await?;
        Ok(vectors.into_iter().map(|v| Embedding { values: v }).collect())
    }
}

// ─── Image Generation ───────────────────────────────────

#[derive(Clone)]
pub struct CompatImageGenModel(ModelBase);

impl<Ext: ProviderExt> FromClient<Ext> for CompatImageGenModel {
    fn from_client(client: &ProviderClient<Ext>, model: &str) -> Self {
        Self(ModelBase::from_client(client, model))
    }
}

#[async_trait::async_trait]
impl ImageGenModel for CompatImageGenModel {
    async fn generate(
        &self,
        prompt: &str,
        _config: &ImageGenConfig,
    ) -> anyhow::Result<Vec<GeneratedImage>> {
        let cfg = self.0.to_provider_config();
        crate::openai::image_http::openai_generate_image(self.0.http(), prompt, &cfg).await
    }
}

// ─── TTS ────────────────────────────────────────────────

#[derive(Clone)]
pub struct CompatTTSModel(ModelBase);

impl<Ext: ProviderExt> FromClient<Ext> for CompatTTSModel {
    fn from_client(client: &ProviderClient<Ext>, model: &str) -> Self {
        Self(ModelBase::from_client(client, model))
    }
}

#[async_trait::async_trait]
impl TTSModel for CompatTTSModel {
    async fn synthesize(
        &self,
        text: &str,
        tts_config: &TTSConfig,
    ) -> anyhow::Result<GeneratedAudio> {
        let cfg = self.0.to_provider_config();
        let voice = if tts_config.voice_id.is_empty() {
            "alloy".to_string()
        } else {
            tts_config.voice_id.clone()
        };
        let req = crate::openai::tts_http::OpenAiTtsRequest {
            model: self.0.model().to_string(),
            input: text.to_string(),
            voice,
            speed: if tts_config.speed > 0.0 { tts_config.speed } else { 1.0 },
            ..Default::default()
        };
        let result = crate::openai::tts_http::openai_tts(self.0.http(), &cfg, &req).await?;
        Ok(GeneratedAudio {
            data: result.audio_bytes,
            mime_type: result.mime_type,
            duration_ms: 0,
        })
    }
}
