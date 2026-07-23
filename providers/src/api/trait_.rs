//! 供应商核心 trait 与共享数据类型。
//!
//! 规范消息类型为 [`crate::types::Message`]、[`crate::types::StreamChunk`]。
//! 本模块保留 trait 定义（`ChatProvider`、`AiProvider` 等）及活跃辅助类型。

use async_trait::async_trait;

// ── 新类型 re-exports ──
pub use crate::types::message::{
    AssistantContent, Message, Role, ToolCall, ToolDefinition, UserContent,
};
pub use crate::types::request::ProviderConfig;
pub use crate::types::stream::{CompletionStream, PauseControl, StreamChunk, Usage};

// ── 旧类型保留（interactions_chat / responses 等活跃代码仍使用） ──

/// 原生 function calling 的一次工具调用（OpenAI 风格语义）。
#[derive(Debug, Clone)]
pub struct ChatToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub signature: Option<String>,
}

/// 单条聊天消息，兼容多轮对话与工具调用。
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub parts: Option<Vec<ChatContentPart>>,
    pub tool_calls: Option<Vec<ChatToolCall>>,
    pub tool_call_id: Option<String>,
    pub name: Option<String>,
    pub reasoning: Option<String>,
    pub thought_signature: Option<String>,
    pub is_error: bool,
}

/// OpenAI / Gemini 兼容 content 数组元素。
#[derive(Debug, Clone)]
pub enum ChatContentPart {
    Text { text: String },
    ImageUrl { url: String },
    AudioUrl { url: String, mime_type: String },
    VideoUrl { url: String, mime_type: String },
    DocumentUrl { url: String, mime_type: String },
}

impl ChatMessage {
    pub fn text(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            parts: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning: None,
            thought_signature: None,
            is_error: false,
        }
    }

    pub fn user_parts(text: impl Into<String>, parts: Vec<ChatContentPart>) -> Self {
        let content = text.into();
        Self {
            role: "user".into(),
            content,
            parts: Some(parts),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning: None,
            thought_signature: None,
            is_error: false,
        }
    }
}

/// 流式 `delta.tool_calls` 片段。
#[derive(Debug, Clone, Default)]
pub struct ToolCallDeltaChunk {
    pub index: u32,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: Option<String>,
    pub signature: Option<String>,
}

/// 流式聊天响应分片（旧格式，interactions_chat / responses 仍使用）。
#[derive(Debug, Clone, Default)]
pub struct ChatChunk {
    pub token: Option<String>,
    pub reasoning: Option<String>,
    pub finish_reason: Option<String>,
    pub tool_call_deltas: Vec<ToolCallDeltaChunk>,
    pub usage: Option<Usage>,
    pub interaction_id: Option<String>,
    pub thought_signature: Option<String>,
    pub citations: Option<Vec<serde_json::Value>>,
}

/// 聊天流式响应的类型别名（旧格式）。
pub type ChatStream =
    std::pin::Pin<Box<dyn futures::Stream<Item = anyhow::Result<ChatChunk>> + Send>>;

/// 生成的图片二进制与 MIME 类型。
#[derive(Debug, Clone)]
pub struct GeneratedImage {
    pub data: Vec<u8>,
    pub mime_type: String,
}

/// 生成的音频（TTS / 音乐共用）。
#[derive(Debug, Clone)]
pub struct GeneratedAudio {
    pub data: Vec<u8>,
    pub mime_type: String,
    pub duration_ms: Option<u64>,
}

/// 生成的视频。
#[derive(Debug, Clone)]
pub struct GeneratedVideo {
    pub url: Option<String>,
    pub task_id: Option<String>,
    pub data: Option<Vec<u8>>,
    pub mime_type: String,
}

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
        messages: Vec<ChatMessage>,
        tools: Vec<serde_json::Value>,
        config: &ProviderConfig,
    ) -> anyhow::Result<ChatStream>;
}

/// 图片生成能力。
#[async_trait]
pub trait ImageGenProvider: Send + Sync {
    async fn generate_image(
        &self,
        prompt: &str,
        config: &ProviderConfig,
    ) -> anyhow::Result<Vec<GeneratedImage>>;
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
