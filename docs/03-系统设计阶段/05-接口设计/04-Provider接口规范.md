# Provider 接口规范

> 阶段：系统设计 | 状态：定稿 | 说明：8 个核心 trait、流式响应、Fallback

## 1. 核心 Trait 定义

```rust
/// 纯文本对话能力，所有 Provider 必须实现
#[async_trait]
pub trait TextClient: Send + Sync {
    /// 发送单轮或多轮对话请求，返回完整响应
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, ProviderError>;

    /// 流式对话，返回 async Stream，每帧为一个 StreamChunk
    fn chat_stream(
        &self,
        req: ChatRequest,
    ) -> BoxStream<'static, Result<StreamChunk, ProviderError>>;

    /// 返回 Provider 标识（如 "anthropic/claude-sonnet-4-5"）
    fn provider_id(&self) -> &str;

    /// 返回该 Provider 支持的最大上下文 token 数
    fn max_context_tokens(&self) -> u32;

    /// 返回该 Provider 的能力声明
    fn capabilities(&self) -> TextCapabilities;
}

pub struct TextCapabilities {
    pub supports_vision: bool,
    pub supports_audio_input: bool,
    pub supports_tool_use: bool,
    pub supports_system_prompt: bool,
    pub supports_streaming: bool,
    pub supports_reasoning: bool,
}

/// 多模态能力（图像、音频输入），继承 TextClient
#[async_trait]
pub trait MultimodalClient: TextClient {
    /// 支持的输入模态列表
    fn supported_modalities(&self) -> &[Modality];

    /// 校验附件是否符合该 Provider 的格式与大小限制
    fn validate_attachment(&self, att: &Attachment) -> Result<(), ProviderError>;
}

/// 向量嵌入能力
#[async_trait]
pub trait EmbeddingClient: Send + Sync {
    async fn embed(&self, req: EmbeddingRequest) -> Result<Vec<Vec<f32>>, ProviderError>;
    fn embedding_dim(&self) -> usize;
}

/// TTS 语音合成（流式）；MiniMax 专属，Google 通过 Interactions API
#[async_trait]
pub trait TtsClient: Send + Sync {
    fn stream_synthesize(
        &self,
        req: TtsRequest,
    ) -> BoxStream<'static, Result<Bytes, ProviderError>>;
    fn provider_id(&self) -> &str;
}

/// ASR 语音识别
#[async_trait]
pub trait AsrClient: Send + Sync {
    async fn transcribe(&self, req: AsrRequest) -> Result<AsrResponse, ProviderError>;
}

/// 图像生成（文生图 / 图生图）；Google Imagen 3 + MiniMax image-01
#[async_trait]
pub trait ImageClient: Send + Sync {
    async fn generate(&self, req: ImageRequest) -> Result<ImageResponse, ProviderError>;
}

/// 视频生成（异步提交 + 轮询）；Google Veo 3.1 + MiniMax video-01
#[async_trait]
pub trait VideoClient: Send + Sync {
    async fn submit(&self, req: VideoRequest) -> Result<String, ProviderError>; // 返回 task_id
    async fn poll(&self, task_id: &str) -> Result<MediaPollResult, ProviderError>;
}

/// 音乐生成（异步提交 + 轮询）；Google Lyria 3（Interactions API）+ MiniMax music-3.0
#[async_trait]
pub trait MusicClient: Send + Sync {
    async fn submit(&self, req: MusicRequest) -> Result<String, ProviderError>;  // 返回 task_id
    async fn poll(&self, task_id: &str) -> Result<MusicPollResult, ProviderError>;
    async fn generate_lyrics(&self, prompt: &str) -> Result<LyricsResponse, ProviderError>;
    fn max_duration_secs(&self) -> u32 { 300 }
}
```

## 2. 请求 / 响应结构

```rust
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,              // role + content（支持多模态 Part）
    pub temperature: Option<f32>,
    pub max_completion_tokens: Option<u32>,  // MiniMax 使用此字段（max_tokens 已废弃）
    pub system_prompt: Option<String>,       // 独立于 messages，各 Provider 自行决定传递方式
    pub tools: Vec<ToolDefinition>,
    pub tool_choice: Option<ToolChoice>,     // 工具选择策略
    pub stream: bool,
}

/// 工具选择策略
pub enum ToolChoice {
    Auto,                    // LLM 自行决定是否调用工具（默认）
    Required,                // 强制 LLM 必须调用至少一个工具
    None,                    // 禁止 LLM 调用工具
    Specific(String),        // 强制调用指定工具
}

> **Provider 特定参数隔离**：Provider 独有的 API 参数（如 MiniMax 的 `reasoning_split`、Google 的 `safety_settings`）不在 `ChatRequest` 中暴露，而是由各 Provider 实现在构建 HTTP 请求时自行注入。统一请求结构只包含所有 Provider 共通的字段。

pub struct ChatResponse {
    pub id: String,
    pub content: String,
    pub reasoning_content: Option<String>,   // DeepSeek / MiniMax 思考内容
    pub tool_calls: Vec<ToolCall>,
    pub usage: TokenUsage,
    pub finish_reason: FinishReason,         // Stop | ToolUse | Length | ContentFilter
    pub raw_message: serde_json::Value,      // 完整原始响应，用于 reasoning_content 回传
}

pub struct StreamChunk {
    pub delta: String,                       // 正文增量
    pub thinking_delta: Option<String>,      // thinking 增量（DeepSeek / MiniMax）
    pub tool_call_delta: Option<ToolCallDelta>,
    pub finish_reason: Option<FinishReason>,
}

pub struct TtsRequest {
    pub model: String,                       // e.g. "speech-2.8-hd"
    pub text: String,
    pub voice_id: String,
    pub output_format: AudioFormat,          // Mp3 | Pcm
    pub speed: Option<f32>,
    pub emotion: Option<String>,             // MiniMax 情感标签
}

pub struct AsrRequest {
    pub model: String,
    pub audio_url: Option<String>,
    pub audio_bytes: Option<Vec<u8>>,
    pub language: Option<String>,
}

pub struct AsrResponse {
    pub text: String,
    pub segments: Vec<AsrSegment>,           // 时间戳片段
}

pub struct ImageRequest {
    pub model: String,                       // "imagen-3.0-generate-002" / "image-01"
    pub prompt: String,
    pub n: u8,
    pub aspect_ratio: Option<String>,
    pub subject_reference: Option<String>,   // MiniMax 图生图参考图 URL / base64
    pub style: Option<String>,               // MiniMax image-01-live 画风
}

pub struct ImageResponse {
    pub urls: Vec<String>,                   // 云端临时 URL（有效期 24h）
    pub local_paths: Vec<String>,            // 下载后本地缓存路径
}

pub struct VideoRequest {
    pub model: String,                       // "veo-3.1-generate-preview" / "video-01"
    pub prompt: String,
    pub aspect_ratio: Option<String>,        // "16:9" | "9:16"
    pub resolution: Option<String>,
}

pub struct MusicRequest {
    pub model: String,                       // "music-3.0" / "lyria-3-pro-preview"
    pub prompt: String,
    pub lyrics: Option<String>,
    pub is_instrumental: bool,
    pub reference_audio_url: Option<String>, // MiniMax music-cover 翻唱参考
}

pub struct MusicResponse {
    pub url: Option<String>,
    pub local_path: Option<String>,
    pub duration_ms: Option<u64>,
}

pub struct MediaPollResult {
    pub status: MediaTaskStatus,             // Pending | Processing | Done | Failed
    pub progress_pct: Option<u8>,
    pub output_urls: Vec<String>,
}

pub struct EmbeddingRequest {
    pub model: String,
    pub inputs: Vec<String>,
    pub encoding_format: EncodingFormat,
}

pub struct EmbeddingRequest {
    pub model: String,
    pub inputs: Vec<String>,
    pub encoding_format: EncodingFormat,  // Float | Base64
}
```

### System Prompt 处理策略

`ChatRequest.system_prompt` 独立于 `messages` 数组，各 Provider 自行决定如何传递：

| Provider | 处理方式 |
| ---- | ---- |
| Anthropic (Claude) | 放入请求体顶层 `system` 参数（Claude API 不允许 system role message） |
| OpenAI | 作为 `messages[0]` 的 `role: "system"` 消息插入 |
| Google (Gemini) | 放入 `systemInstruction` 参数 |
| DeepSeek | 同 OpenAI（兼容 API） |
| MiniMax | 同 OpenAI（兼容 API） |
| Ollama | 同 OpenAI（兼容 API） |

> 这样 Agent 核心只需设置 `TurnContext.system_prompt`，无需关心各 Provider 的差异。

### Claude Prompt Caching 支持

Anthropic 的 Prompt Caching 通过在 content block 上标记 `cache_control: { type: "ephemeral" }` 实现，可节省 90%+ 重复前缀的 Token 费用。

在 `AnthropicClient` 中自动启用：

- System prompt 标记为 cacheable
- 冻结注入的 MEMORY.md/USER.md 内容标记为 cacheable
- 工具 Schema 列表标记为 cacheable

这些缓存决策由 `AnthropicClient` 内部处理，不影响 `ChatRequest` 统一结构。

## 3. Provider 实现清单

每个 Provider 均为独立专属客户端，不共享基类。

| Client | Text | Multimodal | Embed | TTS | ASR | Image | Video | Music | 备注 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `AnthropicClient` | ✓ | ✓ | — | — | — | — | — | — | extended thinking、computer use |
| `OpenAIChatClient` | ✓ | ✓ | ✓ | — | ✓ | ✓ | — | — | Chat Completions API |
| `OpenAIResponsesClient` | ✓ | ✓ | — | — | — | — | — | — | Responses API，内置 web_search/code_interpreter |
| `OpenAICompatClient` | ✓ | — | — | — | — | — | — | — | 纯 base_url/key 替换，无额外逻辑 |
| `DeepSeekClient` | ✓ | — | — | — | — | — | — | — | reasoning_content 回传（tool call 链必须） |
| `MiniMaxClient` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | reasoning_split / base_resp / max_completion_tokens；所有多媒体走专属接口 |
| `GoogleGenerateContentClient` | ✓ | ✓ | ✓ | — | — | ✓ | ✓ | — | Gemini / Imagen 3 / Veo 3.1（LongRunning） |
| `GoogleInteractionsClient` | — | — | — | — | ✓ | — | — | ✓ | Lyria 3 音乐生成 via Interactions API |
| `OllamaClient` | ✓ | 部分 | ✓ | — | — | — | — | — | 本地离线，HTTP :11434 |

## 4. 错误类型

```rust
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("认证失败: {message}")]
    AuthenticationFailed { message: String },

    #[error("触发限流，重试间隔 {retry_after_secs:?}s")]
    RateLimited { retry_after_secs: Option<u64> },

    #[error("输入超过上下文窗口: {required} > {limit}")]
    ContextTooLong { required: u64, limit: u64 },

    #[error("内容安全策略拒绝: {reason:?}")]
    ContentFiltered { reason: Option<String> },

    #[error("网络错误: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Provider 内部错误: {code} {message}")]
    ServerError { code: u16, message: String },

    #[error("不支持的能力: {0}")]
    Unsupported(String),
}
```

## 5. 流式响应实现模式

使用 `async-stream` crate 将 SSE/HTTP chunked 响应转换为类型安全的 Stream：

```rust
fn chat_stream(&self, req: ChatRequest) -> BoxStream<'static, Result<StreamChunk, ProviderError>> {
    let client = self.http.clone();
    Box::pin(async_stream::try_stream! {
        let mut resp = client.post(&self.endpoint).json(&req).send().await?;
        let mut lines = resp.bytes_stream();
        while let Some(chunk) = lines.next().await {
            let line = parse_sse_line(chunk?)?;
            if line == "[DONE]" { break; }
            let delta: StreamChunk = serde_json::from_str(&line)?;
            yield delta;
        }
    })
}
```

## 6. Provider 路由与 Fallback 策略

路由器持有有序的 Provider 列表。主 Provider 失败时，按以下规则决定是否 fallback：

- `ServerError(5xx)` / `Network` / `Timeout` → 尝试下一个 Provider
- `RateLimited` → 不触发 failover，在当前 Provider 上退避重试
- `AuthenticationFailed` / `ContentFiltered` / `ContextTooLong` → 直接返回错误，不 fallback

```rust
pub struct ProviderRouter {
    providers: Vec<Arc<dyn TextClient>>,
    policy: FallbackPolicy,   // Sequential
}

impl ProviderRouter {
    pub async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, ProviderError> {
        for provider in &self.providers {
            match provider.chat(req.clone()).await {
                Ok(resp) => return Ok(resp),
                // RateLimited：在当前 Provider 上退避重试，不 failover
                Err(ProviderError::RateLimited { retry_after_secs }) => {
                    let wait = retry_after_secs.unwrap_or(60);
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                    return provider.chat(req).await;
                }
                // ServerError / Network / Timeout：failover 到下一个 Provider
                Err(e) if e.is_failover_eligible() => continue,
                Err(e) => return Err(e),
            }
        }
        Err(ProviderError::ServerError { code: 503, message: "所有 Provider 均不可用".into() })
    }
}
```

## 7. OpenRouter 统一网关

OpenRouter 兼容 OpenAI Chat Completions API，但需在请求头附加站点信息，并通过 `model` 字段路由到上游（如 `anthropic/claude-sonnet-4-5`）。特殊处理：

- **模型映射**：维护 `openrouter_model_map` 将内部 ID 转换为 OpenRouter slug
- **Price Header**：响应头 `x-openrouter-cost` 记录本次费用，写入计费日志
- **Fallback 优先级**：将 OpenRouter 置于本地 Ollama 之前，用于无直连 API Key 场景
- **工具调用差异**：部分上游通过 OpenRouter 不支持 `tool_choice: required`，需在路由层降级为提示词注入

```rust
pub struct OpenRouterClient {
    inner: OpenAiCompatClient,   // 复用 OpenAI 实现
    model_map: HashMap<String, String>,
}
```

OpenRouter 客户端实现 `TextClient` 和 `MultimodalClient`，不实现 `EmbeddingClient`（嵌入请求直连各 Provider）。
