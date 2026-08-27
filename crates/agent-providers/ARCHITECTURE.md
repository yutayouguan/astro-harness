# Providers Crate Architecture Redesign

> 借鉴 Rig 框架，用 trait-based zero-cost 抽象替代当前的 match-on-string 分发。

## 1. 核心类型系统

### 1.1 消息模型

```rust
/// 角色枚举（替代 `role: String`）
pub enum Role { System, Developer, User, Assistant, Tool }

/// 用户消息内容（多模态）
pub enum UserContent {
    Text(String),
    Image { url: String },
    Audio { url: String, mime_type: String },
    Video { url: String, mime_type: String },
    Document { url: String, mime_type: String },
    ToolResult { tool_call_id: String, content: String, is_error: bool },
}

/// 助手消息内容
pub enum AssistantContent {
    Text(String),
    ToolCall(ToolCall),
    Thinking { text: String, signature: Option<String> },
}

/// 统一消息
pub enum Message {
    System { content: String },
    Developer { content: String },
    User { content: Vec<UserContent> },
    Assistant { content: Vec<AssistantContent> },
}

/// 工具调用
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub signature: Option<String>,  // Gemini 3 strict mode
}

/// 工具定义（provider-agnostic JSON Schema）
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}
```

### 1.2 请求/响应

```rust
/// 统一聊天请求
pub struct CompletionRequest {
    pub model: String,
    pub instructions: String,      // 稳定基础指令
    pub input: Vec<Message>,       // 带角色的动态上下文与对话历史
    pub tools: Vec<ToolDefinition>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub thinking: Option<ThinkingConfig>,
    pub additional_params: serde_json::Value,
}

pub struct ThinkingConfig {
    pub enabled: bool,
    pub budget_tokens: Option<u32>,
    pub effort: String,  // "high" / "max" / ...
}

/// 流式分片
pub enum StreamChunk {
    Text(String),
    Thinking(String),
    ThoughtSignature(String),
    ToolCallStart { index: u32, id: String, name: String },
    ToolCallDelta { index: u32, arguments: String },
    Usage(Usage),
    Citation(serde_json::Value),
    Done { finish_reason: String },
    Error(String),
}

/// 流式响应
pub type CompletionStream = Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>>;
```

## 2. 能力 trait 系统

### 2.1 能力标记（编译期检查）

```rust
/// 标记 trait
pub trait Capability {}

/// 有此能力
pub struct Capable<M>(PhantomData<M>);
impl<M> Capability for Capable<M> {}

/// 无此能力
pub struct Nothing;
impl Capability for Nothing {}

/// 厂商能力声明
pub trait Capabilities {
    type Chat: Capability;         // 聊天补全
    type Embedding: Capability;    // 向量嵌入
    type ImageGen: Capability;     // 图片生成
    type VideoGen: Capability;     // 视频生成
    type TTS: Capability;          // 语音合成
    type MusicGen: Capability;     // 音乐生成
    type ASR: Capability;          // 语音识别
}
```

### 2.2 模型 trait

```rust
/// 聊天补全模型
#[async_trait]
pub trait CompletionModel: Send + Sync {
    async fn stream(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionStream>;
}

/// 嵌入模型
#[async_trait]
pub trait EmbeddingModel: Send + Sync {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

/// 图片生成模型
#[async_trait]
pub trait ImageGenModel: Send + Sync {
    async fn generate(&self, prompt: &str, config: &ImageGenConfig) -> Result<Vec<GeneratedImage>>;
}

/// TTS 模型
#[async_trait]
pub trait TTSModel: Send + Sync {
    async fn synthesize(&self, text: &str, config: &TTSConfig) -> Result<AudioResult>;
}

// ... VideoGen, MusicGen, ASR 类似
```

## 3. 泛型客户端 `Client<Ext>`

```rust
pub struct Client<Ext> {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    ext: Ext,
}

/// 厂商扩展必须实现
pub trait ProviderExt: Send + Sync + Clone {
    /// 厂商名称
    const NAME: &'static str;
    /// 认证方式
    fn auth_headers(&self, api_key: &str) -> HeaderMap;
}

/// 编译期能力绑定（blanket impl）
impl<Ext, M> ChatClient for Client<Ext>
where
    Ext: Capabilities<Chat = Capable<M>>,
    M: CompletionModel,
{
    type Model = M;
    fn chat_model(&self, model: &str) -> M { ... }
}
```

## 4. OpenAI 兼容厂商接入（一行接厂商）

```rust
/// OpenAI 兼容厂商只需实现此 trait
pub trait OpenAICompatible: ProviderExt {
    /// 默认基址
    const BASE_URL: &'static str;
    /// 是否支持 stream_options.include_usage
    const STREAM_USAGE: bool = true;
    /// 是否支持原生 function calling
    const SUPPORTS_TOOLS: bool = true;

    /// 请求体微调（线路格式差异修补）
    fn finalize_body(&self, _body: &mut serde_json::Value) {}
}

// ────── 具体厂商 ──────

pub struct DeepSeek;
impl ProviderExt for DeepSeek {
    const NAME: &'static str = "deepseek";
    fn auth_headers(&self, key: &str) -> HeaderMap { bearer(key) }
}
impl OpenAICompatible for DeepSeek {
    const BASE_URL: &'static str = "https://api.deepseek.com/v1";
    fn finalize_body(&self, body: &mut Value) {
        // DeepSeek V4: thinking 参数
        if let Some(thinking) = body.get("thinking_config") { ... }
    }
}
impl Capabilities for DeepSeek {
    type Chat = Capable<OpenAICompletionModel<Self>>;
    type Embedding = Nothing;
    type ImageGen = Nothing;
    type VideoGen = Nothing;
    type TTS = Nothing;
    type MusicGen = Nothing;
    type ASR = Nothing;
}

// 一行接入 — 只需 3 个 impl block
```

## 5. 原生厂商实现

```rust
// Anthropic — 自己的消息转换 + SSE 解析
pub struct Anthropic;
impl ProviderExt for Anthropic {
    const NAME: &'static str = "anthropic";
    fn auth_headers(&self, key: &str) -> HeaderMap {
        headers! { "x-api-key" => key, "anthropic-version" => ANTHROPIC_VERSION }
    }
}
impl Capabilities for Anthropic {
    type Chat = Capable<AnthropicCompletionModel>;
    type Embedding = Nothing;
    type ImageGen = Nothing;
    // ...
}
// AnthropicCompletionModel 实现 CompletionModel trait
// 内部处理 Message → Anthropic wire format 转换

// Google — Interactions API
pub struct Google;
impl Capabilities for Google {
    type Chat = Capable<InteractionsCompletionModel>;
    type Embedding = Capable<GeminiEmbeddingModel>;
    type ImageGen = Capable<InteractionsImageModel>;
    type VideoGen = Capable<VeoVideoModel>;
    type TTS = Capable<GeminiTTSModel>;
    type MusicGen = Capable<LyriaMusicModel>;
    type ASR = Nothing;  // 编译期阻止调用
}

// MiniMax — 多能力
pub struct MiniMax;
impl Capabilities for MiniMax {
    type Chat = Capable<OpenAICompletionModel<Self>>;
    type Embedding = Capable<OpenAIEmbeddingModel<Self>>;
    type ImageGen = Capable<MiniMaxImageModel>;
    type VideoGen = Capable<MiniMaxVideoModel>;
    type TTS = Capable<MiniMaxTTSModel>;
    type MusicGen = Capable<MiniMaxMusicModel>;
    type ASR = Nothing;
}
```

## 6. 模块结构

```
providers/src/
  lib.rs              — crate root, re-exports
  types/
    mod.rs            — Message, Role, UserContent, AssistantContent
    request.rs        — CompletionRequest, ThinkingConfig
    stream.rs         — StreamChunk, CompletionStream, Usage
    media.rs          — GeneratedImage, AudioResult, VideoResult
  traits/
    mod.rs            — CompletionModel, EmbeddingModel, ImageGenModel, TTSModel, ...
    capability.rs     — Capable<M>, Nothing, Capabilities trait
    client.rs         — Client<Ext>, ProviderExt, ChatClient blanket impl
  compat/
    mod.rs            — OpenAICompatible trait
    completion.rs     — OpenAICompletionModel<Ext> — 共享 Chat Completions 实现
    embedding.rs      — OpenAIEmbeddingModel<Ext> — 共享 Embeddings 实现
    sse.rs            — OpenAI SSE 解析
    messages.rs       — Message → OpenAI wire format
  anthropic/
    mod.rs            — Anthropic ext + Capabilities
    completion.rs     — AnthropicCompletionModel
    messages.rs       — Message → Anthropic wire format
    sse.rs            — Anthropic SSE 解析
    batch.rs          — Batch API
    token_count.rs    — Count tokens
  google/
    mod.rs            — Google ext + Capabilities
    interactions.rs   — InteractionsCompletionModel
    image.rs          — InteractionsImageModel
    tts.rs            — GeminiTTSModel
    video.rs          — VeoVideoModel
    music.rs          — LyriaMusicModel
    embedding.rs      — GeminiEmbeddingModel
    files.rs          — Files API
  minimax/
    mod.rs            — MiniMax ext + Capabilities
    image.rs          — MiniMaxImageModel
    video.rs          — MiniMaxVideoModel
    tts.rs            — MiniMaxTTSModel
    music.rs          — MiniMaxMusicModel
    files.rs          — Files API
    voice_clone.rs    — Voice clone
  vendors/            — 一行接入的 OpenAI 兼容厂商
    deepseek.rs       — 3 impl blocks
    zhipu.rs
    moonshot.rs
    ollama.rs
    nvidia.rs
    bailian.rs
    volcengine.rs
    openrouter.rs
    azure.rs          — Azure deployment URL quirk
  registry.rs         — ProviderRegistry（动态 dyn dispatch）
  profile.rs          — 静态配置表（fallback defaults）
```

## 7. 迁移状态

### Phase 1: 类型系统 + trait 骨架 ✅
- `types/` — Message, Role, UserContent, AssistantContent, CompletionRequest, StreamChunk
- `traits/` — CompletionModel, Capability, Capable/Nothing, Client<Ext>, DynProvider
- `compat/` — OpenAICompatible trait + OpenAICompletionModel<Ext>
- `shared/` — SSE 流解析基础设施

### Phase 2: 厂商实现 ✅
- `impls/` — 14 个厂商（11 OpenAI 兼容 + Anthropic/Google 原生 + MiniMax）

### Phase 3: 注册表 + 桥接 ✅
- `new_registry.rs` — 基于 trait 的动态注册表
- `bridge.rs` — 新旧类型双向转换
- `new_dispatch.rs` — 新管线聊天分发（旧签名兼容）

### Phase 4: 管线切换 ✅
- `chat_stream_for_provider()` 已委托给 `new_dispatch::chat_stream_new()`
- agent 层有 `map_new_provider_stream()` 和 `to_new_messages()` 新函数

### Phase 5: 旧代码淘汰（进行中）
- 旧 `api/trait_.rs` 类型保留作为兼容层（下游仍在用）
- 旧 `vendors/profile_backed.rs` 保留（verify/image_gen 路径仍在用）
- 旧 `anthropic/` `google/` `openai/` 模块保留（媒体/探测路径仍在用）
- 需要逐步将下游 15+ 文件的 import 从 `providers::trait_::ChatMessage` 迁移到 `providers::types::Message`

## 8. 不变量

1. 所有 `impl Capability for Capable<M>` 和 `impl Capability for Nothing` 是零大小类型
2. `Client<Ext>` 的 `Ext` 是零大小（常量方法，无运行时开销）
3. `OpenAICompletionModel<Ext>` 通过 `Ext::finalize_body()` 实现厂商差异，无 match 分支
4. 动态 dispatch 仅在 `ProviderRegistry`（需要按 string id 查找时），所有内部调用走静态 dispatch
