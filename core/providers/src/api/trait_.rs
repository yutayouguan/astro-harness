//! 供应商核心 trait 与共享数据类型。
//!
//! 规范消息类型为 [`crate::types::Message`]、[`crate::types::StreamChunk`]。

use async_trait::async_trait;

// ── 新类型 re-exports ──
pub use crate::types::message::{
    AssistantContent, Message, Role, ToolCall, ToolDefinition, UserContent,
};
pub use crate::types::request::ProviderConfig;
pub use crate::types::stream::{CompletionStream, PauseControl, StreamChunk, Usage};

// ── 媒体生成结果类型（统一使用 types::media） ──
pub use crate::types::media::{GeneratedAudio, GeneratedImage, GeneratedVideo};

/// 提供商认证方式（运行时，非编译期泛型）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    Bearer,
    AnthropicKey,
    GoogleApiKey,
    AzureHeader,
    None,
}

impl AuthKind {
    pub fn for_provider(provider_id: &str) -> Self {
        crate::profile::resolve(provider_id)
            .map(|p| p.auth)
            .unwrap_or(Self::Bearer)
    }
}

/// 连通性探测结果。
#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub model: String,
    pub message: String,
}

// ── Trait 定义 ──

/// 流式聊天能力。
#[async_trait]
pub trait ChatProvider: Send + Sync {
    async fn chat_stream(
        &self,
        messages: Vec<Message>,
        tools: Vec<serde_json::Value>,
        config: &ProviderConfig,
    ) -> anyhow::Result<CompletionStream>;
}

/// 连通性探测能力。
#[async_trait]
pub trait VerifyProvider: Send + Sync {
    async fn verify(&self, model: &str, config: &ProviderConfig) -> VerifyResult;
}

/// 门面 trait：Chat + Verify + 媒体能力。
#[async_trait]
pub trait AiProvider: ChatProvider + VerifyProvider + Send + Sync {
    fn name(&self) -> &str;

    fn supports_tools(&self) -> bool {
        true
    }

    fn supports_image_gen(&self) -> bool {
        false
    }

    fn supports_embedding(&self) -> bool {
        false
    }

    fn default_model(&self) -> &str;

    fn auth_kind(&self) -> AuthKind {
        AuthKind::for_provider(self.name())
    }

    async fn generate_image(
        &self,
        _prompt: &str,
        _config: &ProviderConfig,
    ) -> anyhow::Result<Vec<GeneratedImage>> {
        anyhow::bail!("{} 不支持图片生成", self.name())
    }

    async fn text_to_speech(
        &self,
        text: &str,
        _config: &ProviderConfig,
    ) -> anyhow::Result<GeneratedAudio> {
        let _ = text;
        anyhow::bail!("{} 不支持语音合成 (TTS)", self.name())
    }

    async fn generate_video(
        &self,
        prompt: &str,
        _config: &ProviderConfig,
    ) -> anyhow::Result<GeneratedVideo> {
        let _ = prompt;
        anyhow::bail!("{} 不支持视频生成", self.name())
    }

    async fn generate_music(
        &self,
        prompt: &str,
        _config: &ProviderConfig,
    ) -> anyhow::Result<GeneratedAudio> {
        let _ = prompt;
        anyhow::bail!("{} 不支持音乐生成", self.name())
    }

    async fn embed(
        &self,
        texts: &[String],
        _config: &ProviderConfig,
    ) -> anyhow::Result<Vec<Vec<f32>>> {
        let _ = texts;
        anyhow::bail!("{} 不支持文本嵌入", self.name())
    }
}
