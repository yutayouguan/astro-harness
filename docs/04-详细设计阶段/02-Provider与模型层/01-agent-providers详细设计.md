# agent-providers 详细设计

> **Agent Harness 边界（2026-08-29）**：Provider 层不拥有 turn loop、工具权限或恢复策略。它接收带 `PromptContract` 投影、history 和 `Vec<ToolDefinition>` 的 `CompletionRequest`，输出统一 `StreamChunk`。Function/Freeform/Namespace/ToolSearch/WebSearch 必须在支持的 Provider 中原生传输，仅在不支持时按可保真语义降级。

> 阶段：详细设计 | 状态：草稿 | 说明：各 Provider 客户端实现、ProviderRegistry 路由、流式响应

## 1. 架构概述

`crates/agent-providers` 实现所有 AI Provider 的专属客户端。每个 Provider 均为独立结构体，不共享基类——差异化处理（认证、错误解析、特殊字段、多媒体接口）各自封装，避免 OpenAI 兼容层的过度抽象。

```text
agent-providers/
├── src/
│   ├── lib.rs              # 公开 ProviderRegistry + 所有 Trait 导出
│   ├── registry.rs         # ProviderRegistry：路由、fallback、多模态分发
│   ├── error.rs            # ProviderError 枚举
│   ├── types.rs            # 共享请求/响应结构（ChatRequest / ChatResponse / …）
│   ├── anthropic/
│   │   └── mod.rs          # AnthropicClient
│   ├── openai/
│   │   ├── chat.rs         # OpenAIChatClient
│   │   ├── responses.rs    # OpenAIResponsesClient
│   │   └── compat.rs       # OpenAICompatClient（纯 base_url/key 替换）
│   ├── deepseek/
│   │   └── mod.rs          # DeepSeekClient（reasoning_content 回传）
│   ├── minimax/
│   │   ├── mod.rs          # MiniMaxClient（chat + reasoning_split + base_resp）
│   │   ├── tts.rs          # speech-2.8-hd 流式合成
│   │   ├── asr.rs          # asr-01
│   │   ├── image.rs        # image-01 / image-01-live
│   │   ├── video.rs        # video-01（Task 模式轮询）
│   │   ├── music.rs        # music-3.0 / music-cover（含歌词生成）
│   │   └── voice.rs        # 音色设计 + 声音克隆
│   ├── google/
│   │   ├── generate.rs     # GoogleGenerateContentClient（Gemini / Imagen 3 / Veo 3.1）
│   │   └── interactions.rs # GoogleInteractionsClient（Lyria 3 音乐）
│   └── ollama/
│       └── mod.rs          # OllamaClient（本地 :11434）
```

---

## 2. 共享 Trait 体系

```rust
// 文本对话（所有 Provider 必须实现）
pub trait TextClient: Send + Sync {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, ProviderError>;
    fn chat_stream(&self, req: ChatRequest) -> BoxStream<'static, Result<StreamChunk, ProviderError>>;
    fn provider_id(&self) -> &str;
    fn max_context_tokens(&self) -> u32;
}

// 多媒体输入（图片/视频作为对话输入）
pub trait MultimodalClient: TextClient {
    fn supported_modalities(&self) -> &[InputModality];
}

// TTS 语音合成
pub trait TtsClient: Send + Sync {
    fn stream_synthesize(&self, req: TtsRequest) -> BoxStream<'static, Result<Bytes, ProviderError>>;
}

// ASR 语音识别
pub trait AsrClient: Send + Sync {
    async fn transcribe(&self, req: AsrRequest) -> Result<AsrResponse, ProviderError>;
}

// 图像生成
pub trait ImageClient: Send + Sync {
    async fn generate(&self, req: ImageRequest) -> Result<ImageResponse, ProviderError>;
}

// 视频生成（异步轮询）
pub trait VideoClient: Send + Sync {
    async fn submit(&self, req: VideoRequest) -> Result<String, ProviderError>;
    async fn poll(&self, task_id: &str) -> Result<MediaPollResult, ProviderError>;
}

// 音乐生成（异步提交 + 轮询）
pub trait MusicClient: Send + Sync {
    async fn submit(&self, req: MusicRequest) -> Result<String, ProviderError>;  // 返回 task_id
    async fn poll(&self, task_id: &str) -> Result<MusicPollResult, ProviderError>;
    async fn generate_lyrics(&self, prompt: &str) -> Result<LyricsResponse, ProviderError>;
    fn max_duration_secs(&self) -> u32 { 300 }
}

// 嵌入向量
pub trait EmbeddingClient: Send + Sync {
    async fn embed(&self, req: EmbeddingRequest) -> Result<Vec<Vec<f32>>, ProviderError>;
    fn embedding_dim(&self) -> usize;
}
```

---

## 3. 各客户端实现细节

### 3.1 AnthropicClient

- API：`https://api.anthropic.com/v1/messages`
- 认证：`x-api-key` 请求头 + `anthropic-version: 2023-06-01`
- Extended Thinking：请求体附加 `thinking: { type: "enabled", budget_tokens: N }`；流式时 `thinking` block 先于 `text` block 到达，需合并为 `StreamChunk.thinking_delta`
- Computer Use：工具 `computer_20241022`，截图返回 base64 image block

### 3.2 OpenAIChatClient / OpenAIResponsesClient / OpenAICompatClient

- **OpenAIChatClient**：`/v1/chat/completions`；实现 `TextClient + MultimodalClient + EmbeddingClient + AsrClient + ImageClient`
- **OpenAIResponsesClient**：`/v1/responses`；内置 `web_search` / `code_interpreter` 工具；`store: false`（不持久化到 OpenAI 端）
- **OpenAICompatClient**：仅替换 `base_url` 和 `Authorization` 头，无额外逻辑；用于 Mistral / Groq / Cerebras 等标准兼容厂商

### 3.3 DeepSeekClient

- API：`https://api.deepseek.com/v1/chat/completions`
- **reasoning_content 回传**：DeepSeek R1/R2 响应中 `reasoning_content` 字段包含思考过程；在工具调用链中，上一轮 assistant message 必须携带完整 `raw_message`（含 `reasoning_content`）原样插入下一轮请求，否则 API 返回 400
- `AgentContext.message_history` 存储完整 `raw_message` 对象；`DeepSeekClient` 在构建请求时检测前一条 assistant message 是否含 `reasoning_content`，是则直接注入，无需上层感知

```rust
fn build_messages(history: &[RawMessage]) -> Vec<serde_json::Value> {
    history.iter().map(|m| {
        if m.role == "assistant" && m.reasoning_content.is_some() {
            m.raw.clone() // 原样插入，保留 reasoning_content
        } else {
            m.to_standard_message()
        }
    }).collect()
}
```

### DeepSeek 推理模型约束

DeepSeek R1（`deepseek-reasoner`）不支持在 `reasoning_content` 模式下同时使用 tool_use。`DeepSeekClient` 在发起请求前自动处理此约束：

```rust
impl TextClient for DeepSeekClient {
    async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, ProviderError> {
        let mut req = request.clone();
        
        // 推理模型约束：reasoning 模式下禁用 tool_use
        if self.is_reasoning_model(&req.model) && !req.tools.is_empty() {
            // 策略 1：自动降级到 deepseek-chat（支持 tool_use）
            req.model = "deepseek-chat".to_string();
            tracing::warn!("deepseek-reasoner 不支持 tool_use，已自动降级到 deepseek-chat");
        }
        
        self.call_api(req).await
    }
    
    fn capabilities(&self) -> TextCapabilities {
        TextCapabilities {
            supports_tool_use: !self.is_reasoning_model(&self.default_model),
            supports_reasoning: self.is_reasoning_model(&self.default_model),
            ..Default::default()
        }
    }
}
```

> **路由层集成**：`ProviderRouter` 在选择模型时参考 `TextCapabilities.supports_tool_use`，当任务需要工具调用时自动跳过不支持的推理模型。

### 3.4 MiniMaxClient

MiniMax 有多处非标处理，全部集中在 `MiniMaxClient`：

**Chat Completions 特殊处理：**

```rust
// 1. 始终注入 reasoning_split
req_body["reasoning_split"] = json!(true);

// 2. max_completion_tokens 替换 max_tokens
req_body["max_completion_tokens"] = req_body.remove("max_tokens");

// 3. M2.x 无法禁用 thinking，不附加 thinking.type=disabled

// 4. base_resp 错误码解析（在 200 响应体中）
if let Some(base_resp) = resp.get("base_resp") {
    match base_resp["status_code"].as_i64() {
        Some(1002) => return Err(ProviderError::RateLimited { retry_after_secs: Some(60) }),
        Some(1004) => return Err(ProviderError::AuthenticationFailed { message: "MiniMax auth failed".into() }),
        Some(1008) => return Err(ProviderError::AuthenticationFailed { message: "MiniMax quota exhausted".into() }),
        Some(1027) => return Err(ProviderError::ContentFiltered { reason: Some("MiniMax content filter".into()) }),
        Some(1039) => return Err(ProviderError::ContextTooLong { required: 0, limit: 0 }),  // 具体数值由响应体提取
        Some(2013) => return Err(ProviderError::InvalidRequest(msg)),
        _ => {}
    }
}
```

**多媒体专属接口（均非 OpenAI 兼容路由）：**

| 能力 | 接口路径 | 模型 | 实现文件 |
| --- | --- | --- | --- |
| TTS | `POST /v1/t2a_v2` | speech-2.8-hd/turbo | `tts.rs` |
| ASR | `POST /v1/asr` | asr-01 | `asr.rs` |
| 图像生成 | `POST /v1/image_generation` | image-01 / image-01-live | `image.rs` |
| 视频生成 | `POST /v1/video_generation` | video-01（Task 轮询） | `video.rs` |
| 音乐生成 | `POST /v1/music_generation` | music-3.0 / music-cover | `music.rs` |
| 歌词生成 | `POST /v1/lyrics_generation` | — | `music.rs` |
| 翻唱前处理 | `POST /v1/music_cover_preprocess` | — | `music.rs` |
| 音色设计 | `POST /v1/voice_design` | — | `voice.rs` |
| 声音克隆 | `POST /v1/files/upload` (purpose=voice_clone) | — | `voice.rs` |

**TTS 流式实现：**

```rust
// tts.rs — 解析 SSE hex 流
async fn stream_synthesize(&self, req: TtsRequest) -> BoxStream<'static, Result<Bytes, ProviderError>> {
    Box::pin(async_stream::try_stream! {
        let mut stream = self.http.post("/v1/t2a_v2")
            .json(&build_tts_body(&req))
            .send().await?.bytes_stream();

        while let Some(line) = read_sse_line(&mut stream).await? {
            let chunk: TtsChunk = serde_json::from_str(&line)?;
            if chunk.data.audio.is_empty() { break; }
            // status=1 传输中，status=2 完成
            let audio = hex::decode(&chunk.data.audio)?;
            yield Bytes::from(audio);
            if chunk.data.status == 2 { break; }
        }
    })
}
```

**music-cover 两步翻唱流程：**

```rust
// 1. 预处理：提取音频特征 + ASR 歌词
let preprocess = self.music_cover_preprocess(audio_url).await?;
// cover_feature_id 有效期 24h，可修改 formatted_lyrics 后传入步骤 2

// 2. 翻唱生成
let music_req = MusicCoverRequest {
    model: "music-cover",
    cover_feature_id: preprocess.cover_feature_id,
    lyrics: custom_lyrics.unwrap_or(preprocess.formatted_lyrics),
    ..Default::default()
};
```

### 3.5 GoogleGenerateContentClient

- 路由：`https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent`
- 支持：Gemini（文本/多模态）、Imagen 3（图像）、Veo 3.1（视频）

**Veo 3.1 LongRunning 轮询：**

```rust
// 提交任务，返回 operation name
let op: Operation = self.http
    .post(format!("models/{model}:predictLongRunning"))
    .json(&req).send().await?.json().await?;

// 轮询直到 done=true，间隔 10s
loop {
    let status: Operation = self.http
        .get(format!("operations/{}", op.name))
        .send().await?.json().await?;
    if status.done { return Ok(extract_video_urls(status)); }
    tokio::time::sleep(Duration::from_secs(10)).await;
}
```

### 3.6 GoogleInteractionsClient

- 路由：`https://generativelanguage.googleapis.com/v1alpha/models/{model}:predict`（Interactions API）
- Lyria 3 音乐生成：`lyria-3-pro-preview`（完整歌曲 MP3/WAV）/ `lyria-3-clip-preview`（30秒 MP3）
- 支持文本 prompt + 最多 10 张图片参考输入
- 响应包含 SynthID 水印元数据；音频以 base64 编码返回，下载后存入 `media/music/`

### 3.7 OllamaClient

- API：`http://localhost:11434/api/chat`（OpenAI 兼容模式：`/v1/chat/completions`）
- 优先使用 OpenAI 兼容路径；遇到 404 则降级到原生 `/api/chat`
- 启动检测：调用 `GET /api/version`，未响应则尝试启动 sidecar

---

## 4. ProviderRegistry

```rust
pub struct ProviderRegistry {
    // 文本对话客户端（含 DeepSeek / MiniMax / Anthropic 等）
    text_clients: HashMap<String, Arc<dyn TextClient>>,
    // 多媒体专属客户端（按 modality 分发）
    tts_client: Arc<dyn TtsClient>,      // 默认：minimax/speech-2.8-hd
    asr_client: Arc<dyn AsrClient>,      // 默认：google/latest_long
    image_client: Arc<dyn ImageClient>,  // 默认：google/imagen-3.0-generate-002
    video_client: Arc<dyn VideoClient>,  // 默认：google/veo-3.1-generate-preview
    music_client: Arc<dyn MusicClient>,  // 默认：google/lyria-3-pro-preview
    embedding_client: Arc<dyn EmbeddingClient>,
    fallback_chains: HashMap<String, Vec<String>>, // modality → ordered provider slugs
}

impl ProviderRegistry {
    pub async fn from_config(providers_toml: &Path, secrets_dir: &Path) -> anyhow::Result<Self> {
        // 读取 providers.toml + secrets/*.key（不接受明文 key 参数）
        let cfg = ProviderConfig::load(providers_toml)?;
        let keys = SecretsStore::load(secrets_dir)?;
        // 按配置实例化各专属客户端
        // ...
    }

    pub fn text_client(&self, provider_model: &str) -> Result<Arc<dyn TextClient>, ProviderError> {
        self.text_clients.get(provider_model)
            .cloned()
            .ok_or_else(|| ProviderError::ModelNotAvailable { model: provider_model.into() })
    }

    pub fn tts(&self, _hint: &str) -> Arc<dyn TtsClient> {
        self.tts_client.clone() // 当前版本固定路由，后续支持 hint 覆盖
    }
}
```

**Fallback 规则（基于 `is_retryable()` / `should_failover()` 语义，见 §5）：**

- `Network` / `ConnectionTimeout` / `ServerError(5xx)` → 可重试 + 可 failover：同 Provider 立即重试，重试耗尽后按 `fallback_chains` 切换
- `RateLimited` → 可重试但**不触发 failover**：退避等待后同 Provider 重试（等待退避更经济）
- `ModelNotAvailable` → 不可重试但可 failover：直接切换 Provider
- `AuthenticationFailed` / `ContentFiltered` / `ContextTooLong` / `InvalidRequest` / `Unsupported` → 不可重试、不可 failover：直接返回错误

---

## 5. 错误处理与重试

> ProviderError 统一定义于 `agent-types` crate，此处展示权威定义供所有 Provider 实现者参考。

```rust
// crates/agent-types/src/error.rs

/// Provider 错误类型（定义在 agent-types crate）
/// 所有 Provider 实现使用此统一枚举
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    // === 网络层 ===
    #[error("网络错误: {0}")]
    Network(#[from] reqwest::Error),
    
    #[error("连接超时: {timeout_ms}ms")]
    ConnectionTimeout { timeout_ms: u64 },
    
    // === 认证 ===
    #[error("认证失败: {message}")]
    AuthenticationFailed { message: String },
    
    // === 服务端 ===
    #[error("服务端错误 {code}: {message}")]
    ServerError { code: u16, message: String },
    
    #[error("速率限制，建议等待 {retry_after_secs}s")]
    RateLimited { retry_after_secs: Option<u64> },
    
    // === 请求层 ===
    #[error("上下文超长: 需要 {required} tokens，模型上限 {limit}")]
    ContextTooLong { required: u64, limit: u64 },
    
    #[error("内容被过滤")]
    ContentFiltered { reason: Option<String> },
    
    #[error("不支持的操作: {0}")]
    Unsupported(String),
    
    #[error("无效请求: {0}")]
    InvalidRequest(String),
    
    // === 模型层 ===
    #[error("模型不可用: {model}")]
    ModelNotAvailable { model: String },
    
    // === 内部 ===
    #[error("响应解析失败: {0}")]
    ParseError(String),
    
    #[error("内部错误: {0}")]
    Internal(String),
}

impl ProviderError {
    /// 判断是否可重试
    pub fn is_retryable(&self) -> bool {
        matches!(self,
            Self::Network(_) |
            Self::ConnectionTimeout { .. } |
            Self::ServerError { code, .. } if *code >= 500 |
            Self::RateLimited { .. }
        )
    }
    
    /// 判断是否应触发 Provider 切换（fallback）
    pub fn should_failover(&self) -> bool {
        matches!(self,
            Self::Network(_) |
            Self::ConnectionTimeout { .. } |
            Self::ServerError { code, .. } if *code >= 500 |
            Self::ModelNotAvailable { .. }
        )
        // 注意：RateLimited 优先退避等待，不立即切换 Provider
    }
}
```

> **统一定义说明**：此定义合并了接口规范、详细设计和故障转移设计三份文档的 `ProviderError`。`is_retryable()` 和 `should_failover()` 的语义明确区分：`RateLimited` 可重试但不触发 failover（等待退避更经济）。

重试策略：指数退避，初始间隔 1s，最大 3 次，`RateLimited` 时遵守 `retry_after_secs`。
