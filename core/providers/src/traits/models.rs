//! 模型能力 trait — 各能力的异步接口定义。

use anyhow::Result;
use async_trait::async_trait;

use crate::types::{
    CompletionRequest, CompletionStream, Embedding, GeneratedAudio, GeneratedImage, GeneratedVideo,
    ImageGenConfig, MusicGenConfig, TTSConfig, VideoGenConfig,
};

/// 聊天补全模型。
#[async_trait]
pub trait CompletionModel: Send + Sync {
    async fn stream(&self, request: CompletionRequest) -> Result<CompletionStream>;
}

/// 嵌入模型。
#[async_trait]
pub trait EmbeddingModel: Send + Sync {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Embedding>>;
}

/// 图片生成模型。
#[async_trait]
pub trait ImageGenModel: Send + Sync {
    async fn generate(
        &self,
        prompt: &str,
        config: &ImageGenConfig,
    ) -> Result<Vec<GeneratedImage>>;
}

/// 视频生成模型。
#[async_trait]
pub trait VideoGenModel: Send + Sync {
    async fn generate(
        &self,
        prompt: &str,
        config: &VideoGenConfig,
    ) -> Result<GeneratedVideo>;
}

/// 语音合成模型。
#[async_trait]
pub trait TTSModel: Send + Sync {
    async fn synthesize(
        &self,
        text: &str,
        config: &TTSConfig,
    ) -> Result<GeneratedAudio>;
}

/// 音乐生成模型。
#[async_trait]
pub trait MusicGenModel: Send + Sync {
    async fn generate(
        &self,
        prompt: &str,
        config: &MusicGenConfig,
    ) -> Result<GeneratedAudio>;
}

/// 连通性探测结果。
#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub model: String,
    pub message: String,
}
