//! 模型能力 trait — 各能力的异步接口定义。

use anyhow::Result;
use async_trait::async_trait;

use crate::types::{
    ChatCompletionRequest, CompletionStream, Embedding, GeneratedAudio, GeneratedImage,
    GeneratedVideo, ImageGenConfig, MusicGenConfig, Prompt, TTSConfig, VideoGenConfig,
};

/// Agent 原生 Responses 模型。
#[async_trait]
pub trait ResponsesModel: Send + Sync {
    async fn stream(&self, prompt: Prompt) -> Result<CompletionStream>;
}

/// 非 Agent Chat/Anthropic/Gemini 兼容模型。
#[async_trait]
pub trait ChatCompletionModel: Send + Sync {
    async fn stream(&self, request: ChatCompletionRequest) -> Result<CompletionStream>;
}

/// 嵌入模型。
#[async_trait]
pub trait EmbeddingModel: Send + Sync {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Embedding>>;
}

/// 图片生成模型。
#[async_trait]
pub trait ImageGenModel: Send + Sync {
    async fn generate(&self, prompt: &str, config: &ImageGenConfig) -> Result<Vec<GeneratedImage>>;
}

/// 视频生成模型。
#[async_trait]
pub trait VideoGenModel: Send + Sync {
    async fn generate(&self, prompt: &str, config: &VideoGenConfig) -> Result<GeneratedVideo>;
}

/// 语音合成模型。
#[async_trait]
pub trait TTSModel: Send + Sync {
    async fn synthesize(&self, text: &str, config: &TTSConfig) -> Result<GeneratedAudio>;
}

/// 音乐生成模型。
#[async_trait]
pub trait MusicGenModel: Send + Sync {
    async fn generate(&self, prompt: &str, config: &MusicGenConfig) -> Result<GeneratedAudio>;
}
