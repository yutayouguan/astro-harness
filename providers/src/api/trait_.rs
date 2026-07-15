//! 供应商核心 trait 与共享数据类型。
//!
//! 定义聊天消息、流式分片、配置、认证方式，以及
//! [`ChatProvider`]、[`ImageGenProvider`]、[`VerifyProvider`]、[`AiProvider`] 等能力接口。

use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

/// 原生 function calling 的一次工具调用（OpenAI 风格语义）。
#[derive(Debug, Clone)]
pub struct ChatToolCall {
    /// 工具调用唯一标识，用于关联 tool 角色回复。
    pub id: String,
    /// 被调用的函数名。
    pub name: String,
    /// 已解析的 JSON 对象；序列化到上游时会再 stringify。
    pub arguments: serde_json::Value,
}

/// 单条聊天消息，兼容多轮对话与工具调用。
#[derive(Debug, Clone)]
pub struct ChatMessage {
    /// 角色：`system` / `user` / `assistant` / `tool`。
    pub role: String,
    /// 文本内容；纯 tool_calls 时可为空；有 `parts` 时作摘要。
    pub content: String,
    /// 多模态 parts（OpenAI 兼容 text + image_url）；`None` 则 content 为纯字符串。
    pub parts: Option<Vec<ChatContentPart>>,
    /// assistant 消息附带的工具调用列表。
    pub tool_calls: Option<Vec<ChatToolCall>>,
    /// tool 角色消息对应的 `tool_call_id`。
    pub tool_call_id: Option<String>,
    /// tool 角色消息对应的函数名（部分厂商需要）。
    pub name: Option<String>,
}

/// OpenAI 兼容 content 数组元素。
#[derive(Debug, Clone)]
pub enum ChatContentPart {
    /// 文本。
    Text { text: String },
    /// 图片（data URL 或 http(s)）。
    ImageUrl { url: String },
}

impl ChatMessage {
    /// 构造纯文本消息（无工具调用字段）。
    pub fn text(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            parts: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }

    /// 构造带多模态 parts 的用户消息。
    pub fn user_parts(text: impl Into<String>, parts: Vec<ChatContentPart>) -> Self {
        let content = text.into();
        Self {
            role: "user".into(),
            content,
            parts: Some(parts),
            tool_calls: None,
            tool_call_id: None,
            name: None,
        }
    }
}

/// 流式 `delta.tool_calls` 片段（与 tools::ToolCallDelta 对齐）。
#[derive(Debug, Clone, Default)]
pub struct ToolCallDeltaChunk {
    /// 工具调用在数组中的索引。
    pub index: u32,
    /// 工具调用 ID 增量（首次出现时下发）。
    pub id: Option<String>,
    /// 函数名增量。
    pub name: Option<String>,
    /// 参数字符串增量（JSON 片段）。
    pub arguments: Option<String>,
}

/// 流式聊天响应分片。
#[derive(Debug, Clone, Default)]
pub struct ChatChunk {
    /// 正文 token 增量。
    pub token: Option<String>,
    /// DeepSeek V4 等：thinking 模式下的 reasoning_content 增量。
    pub reasoning: Option<String>,
    /// 结束原因，如 `stop`、`tool_calls`；错误时为 `error:...`。
    pub finish_reason: Option<String>,
    /// 工具调用增量列表。
    pub tool_call_deltas: Vec<ToolCallDeltaChunk>,
    /// 本轮 token 用量（部分上游在末包或独立包下发）。
    pub usage: Option<crate::streaming::Usage>,
}

/// 生成的图片二进制与 MIME 类型。
#[derive(Debug, Clone)]
pub struct GeneratedImage {
    /// 图片原始字节。
    pub data: Vec<u8>,
    /// MIME 类型，如 `image/png`。
    pub mime_type: String,
}

/// 单次模型调用的运行时配置。
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    /// API 密钥。
    pub api_key: String,
    /// 自定义 API 基址；为空时使用供应商默认。
    pub base_url: Option<String>,
    /// 模型名称或部署 ID。
    pub model: String,
    /// 采样温度。
    pub temperature: f32,
    /// 最大生成 token 数。
    pub max_tokens: u32,
    /// DeepSeek V4 等：是否开启 thinking。
    pub thinking_enabled: bool,
    /// DeepSeek：`high` | `max`（仅 thinking 开启时生效）。
    pub reasoning_effort: String,
    /// Provider 扩展参数（对齐 Rig additional_params），合并进请求 JSON。
    pub additional_params: serde_json::Value,
}

impl Default for ProviderConfig {
    /// 返回适用于 OpenAI 风格模型的默认配置。
    fn default() -> Self {
        ProviderConfig {
            api_key: String::new(),
            base_url: None,
            model: "gpt-5.6".to_string(),
            temperature: 0.7,
            max_tokens: 4096,
            thinking_enabled: false,
            reasoning_effort: "high".to_string(),
            additional_params: serde_json::Value::Null,
        }
    }
}

/// 提供商认证方式（运行时，非编译期泛型）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    /// `Authorization: Bearer <key>`
    Bearer,
    /// Anthropic `x-api-key`
    AnthropicKey,
    /// Google 原生 `?key=`（出图等仍可能使用；chat 已改 Bearer）
    GoogleQuery,
    /// Azure `api-key` header
    AzureHeader,
    /// 本地 Ollama 等无需密钥
    None,
}

impl AuthKind {
    /// 根据 provider id 推断认证方式（表驱动）。
    pub fn for_provider(provider_id: &str) -> Self {
        crate::profile::resolve(provider_id)
            .map(|p| p.auth)
            .unwrap_or(Self::Bearer)
    }
}

/// 连通性探测结果。
#[derive(Debug, Clone)]
pub struct VerifyResult {
    /// 探测是否成功。
    pub ok: bool,
    /// 往返耗时（毫秒）。
    pub latency_ms: u64,
    /// 被探测的模型名。
    pub model: String,
    /// 人类可读的状态说明。
    pub message: String,
}

/// 聊天流式响应的类型别名。
pub type ChatStream = Pin<Box<dyn Stream<Item = anyhow::Result<ChatChunk>> + Send>>;

/// 流式聊天能力。
#[async_trait]
pub trait ChatProvider: Send + Sync {
    /// 发起流式聊天请求，返回 token / 工具调用 / usage 分片流。
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
    /// 根据提示词生成一张或多张图片。
    async fn generate_image(
        &self,
        prompt: &str,
        config: &ProviderConfig,
    ) -> anyhow::Result<Vec<GeneratedImage>>;
}

/// 连通性探测能力。
#[async_trait]
pub trait VerifyProvider: Send + Sync {
    /// 用最小请求探测指定模型是否可用。
    async fn verify(&self, model: &str, config: &ProviderConfig) -> VerifyResult;
}

/// 门面：Chat + Verify；图片生成仍通过本 trait 的默认/覆盖方法暴露给 dyn。
#[async_trait]
pub trait AiProvider: ChatProvider + VerifyProvider + Send + Sync {
    /// 供应商标识符，如 `openai`、`claude`。
    fn name(&self) -> &str;

    /// 是否支持原生 function calling。
    fn supports_tools(&self) -> bool {
        true
    }

    /// 是否支持图片生成。
    fn supports_image_gen(&self) -> bool {
        false
    }

    /// 是否支持 embedding（预留能力位）。
    fn supports_embedding(&self) -> bool {
        false
    }

    /// 默认推荐模型名。
    fn default_model(&self) -> &str;

    /// 本供应商使用的认证方式。
    fn auth_kind(&self) -> AuthKind {
        AuthKind::for_provider(self.name())
    }

    /// 生成图片；默认实现返回不支持错误，由具体供应商覆盖。
    async fn generate_image(
        &self,
        _prompt: &str,
        _config: &ProviderConfig,
    ) -> anyhow::Result<Vec<GeneratedImage>> {
        anyhow::bail!("{} 不支持图片生成", self.name())
    }
}
