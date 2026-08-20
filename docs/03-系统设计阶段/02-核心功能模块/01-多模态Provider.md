# 多模态 Provider 系统

> 阶段：系统设计 | 状态：定稿 | 说明：8 个 trait、路由与降级

## 能力矩阵

| 能力 | Anthropic | OpenAI | Google | DeepSeek | MiniMax |
| ---- | :-------: | :----: | :----: | :------: | :-----: |
| 文本生成 | ✓ | ✓ | ✓ | ✓ | ✓ |
| 视觉理解 | ✓ | ✓ | ✓ | ✓ | ✓ |
| 音频输入 | ✓ | ✓ | ✓ | — | ✓ |
| 视频输入 | — | — | ✓ | — | ✓ |
| Tool Use | ✓ | ✓ | ✓ | ✓ | ✓ |
| 嵌入模型 | — | ✓ | ✓ | ✓ | ✓ |
| 图像生成 | — | △ | ✓ | — | ✓ |
| TTS | — | △ | ✓ | — | ✓ |
| ASR | — | △ | ✓ | — | ✓ |
| 视频生成 | — | — | ✓ | — | ✓ |
| 音乐生成 | — | — | ✓ | — | ✓ |

> **多媒体能力主力：Google + MiniMax。** OpenAI 多媒体接口（DALL-E / Whisper / TTS-1，标注 △）保留为备选降级项，不作默认路由。
>
> Google 对应服务：Imagen 3（图像生成）、Veo 3.1（视频生成，8秒，720p/1080p/4k，含原生音频，LongRunning 轮询）、Lyria 3（音乐生成，走 Interactions API；Clip 30秒 / Pro 数分钟，44.1 kHz 立体声，支持文本+图片输入）、Cloud TTS Chirp3-HD（语音合成）、Cloud STT（语音识别）、text-embedding-004（嵌入）、Gemini（文本/多模态理解，含视频输入）
>
> MiniMax 对应服务：speech-2.8-hd（TTS，最新 HD，支持流式/情感标签/字幕/音色设计/声音克隆）、asr-01（ASR）、image-01 / image-01-live（图像生成，含图生图/画风/人物主体参考）、video-01（视频生成）、music-3.0（音乐生成）+ music-cover（**翻唱，唯一参考音频翻唱能力**）+ 歌词生成（`/v1/lyrics_generation`，独立接口）；M3 支持图片+视频输入
>
> ⚠️ MiniMax 所有多媒体接口（TTS/ASR/图像/视频/音乐/歌词/音色设计）均为**专属独立 endpoint**，不走 Chat Completions 兼容路由，`MiniMaxClient` 需为每个模态单独实现。

---

## Trait 体系

```text
ModalityClient (总入口)
├── TextClient          → 文本生成
│   └── MultimodalClient → 多模态输入（图像、音频），继承 TextClient（定义见 04-Provider接口规范.md）
├── EmbeddingClient     → 向量嵌入
├── ImageClient         → 图像生成/理解
├── AudioClient
│   ├── TtsClient       → 文字转语音
│   └── AsrClient       → 语音转文字
├── VideoClient         → 视频生成
└── MusicClient         → 音乐生成
```

---

## 统一类型

### 多媒体内容

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaContent {
    Text { text: String },
    Image(ImageContent),
    Audio(AudioContent),
    Video(VideoContent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaData {
    Url(String),
    Base64 { data: String, size_bytes: usize },
    Path(std::path::PathBuf),       // 延迟上传，调用时才读取
}
```

### 统一消息格式

```rust
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentPart>,  // 多模态内容列表
    pub name: Option<String>,       // agent 名（多 agent 场景）
}

#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text { text: String },
    Image(ImageContent),
    Audio(AudioContent),
    Video(VideoContent),
    ToolCall(ToolCallContent),
    ToolResult(ToolResultContent),
}
```

---

## 各模态 Trait 定义

### TextClient

```rust
#[async_trait]
pub trait TextClient: Send + Sync {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, ProviderError>;
    fn chat_stream(
        &self,
        req: ChatRequest,
    ) -> BoxStream<'static, Result<StreamChunk, ProviderError>>;
    fn provider_id(&self) -> &str;
    fn max_context_tokens(&self) -> u32;
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
```

### EmbeddingClient

```rust
#[async_trait]
pub trait EmbeddingClient: Send + Sync {
    async fn embed(&self, req: EmbeddingRequest) -> Result<Vec<Vec<f32>>, ProviderError>;
    fn embedding_dim(&self) -> usize;
}
```

### TtsClient / AsrClient

```rust
#[async_trait]
pub trait TtsClient: Send + Sync {
    fn stream_synthesize(
        &self,
        req: TtsRequest,
    ) -> BoxStream<'static, Result<Bytes, ProviderError>>;
}

#[async_trait]
pub trait AsrClient: Send + Sync {
    async fn transcribe(&self, req: AsrRequest) -> Result<AsrResponse>;
}

pub struct AsrResponse {
    pub text: String,
    pub segments: Vec<AsrSegment>,  // 带时间戳的片段
}
```

### ImageClient

```rust
#[async_trait]
pub trait ImageClient: Send + Sync {
    async fn generate(&self, req: ImageRequest) -> Result<ImageResponse, ProviderError>;
}
```

### VideoClient

```rust
// 视频生成是异步任务，统一用 submit + poll 模式轮询
#[async_trait]
pub trait VideoClient: Send + Sync {
    async fn submit(&self, req: VideoRequest) -> Result<String, ProviderError>;  // returns task_id
    async fn poll(&self, task_id: &str) -> Result<MediaPollResult, ProviderError>;
}

pub enum VideoGenStatus {
    Pending,
    Processing { progress: f32 },
    Done { url: String, duration_secs: f32 },
    Failed { reason: String },
}
```

### MusicClient

```rust
#[async_trait]
pub trait MusicClient: Send + Sync {
    async fn submit(&self, request: MusicRequest) -> Result<String, ProviderError>;  // returns task_id
    async fn poll(&self, task_id: &str) -> Result<MusicPollResult, ProviderError>;
    async fn generate_lyrics(&self, prompt: &str) -> Result<LyricsResponse, ProviderError>;
    fn max_duration_secs(&self) -> u32 { 300 }
}

pub struct MusicRequest {
    pub prompt: String,
    pub duration_secs: Option<u32>,   // 默认 30s
    pub vocal: bool,                   // 是否包含人声
    pub lyrics: Option<String>,        // 可选歌词
    pub genre: Option<String>,         // 音乐风格
    pub tempo: Option<u32>,            // BPM
}

pub struct MusicReferenceRequest {
    pub reference_audio: MediaData,
    pub prompt: String,
    pub duration_secs: Option<u32>,
}

pub struct LyricsResponse {
    pub lyrics: String,
    pub language: String,
}

pub type MusicTaskId = String;

pub enum MusicPollResult {
    Pending,
    Processing { progress: f32 },
    Done {
        audio_url: String,
        duration_secs: f32,
        format: String,               // "mp3", "wav"
    },
    Failed { reason: String },
}
```

---

## ProviderRegistry — 运行时路由

```rust
pub struct ProviderRegistry {
    text_clients:    HashMap<String, Arc<dyn TextClient>>,
    tts:             Arc<dyn TtsClient>,
    asr:             Arc<dyn AsrClient>,
    image:           Arc<dyn ImageClient>,
    video:           Arc<dyn VideoClient>,
    music:           Arc<dyn MusicClient>,
    embedding:       Arc<dyn EmbeddingClient>,
    fallback_chains: HashMap<String, Vec<String>>,
}

impl ProviderRegistry {
    pub fn text(&self, model: &str) -> Result<Arc<dyn TextClient>>;
    pub fn tts(&self) -> Arc<dyn TtsClient>;
    pub fn asr(&self) -> Arc<dyn AsrClient>;
    pub fn image(&self) -> Arc<dyn ImageClient>;
    pub fn video(&self) -> Arc<dyn VideoClient>;
    pub fn music(&self) -> Arc<dyn MusicClient>;
    pub fn embedding(&self) -> Arc<dyn EmbeddingClient>;
}
```

---

## 配置文件

```toml
# configs/providers.toml

[providers.anthropic]
api_key_env = "ANTHROPIC_API_KEY"
base_url    = "https://api.anthropic.com"
models      = ["claude-opus-4", "claude-sonnet-4-5", "claude-haiku-4-5"]

[providers.openai]
api_key_env = "OPENAI_API_KEY"
models.text      = ["gpt-4o", "gpt-4o-mini"]
models.embedding = ["text-embedding-3-small", "text-embedding-3-large"]
# 以下多媒体模型保留为降级备选，不作默认路由
models.image     = ["dall-e-3"]
models.tts       = ["tts-1", "tts-1-hd"]
models.asr       = ["whisper-1"]

[providers.deepseek]
api_key_env = "DEEPSEEK_API_KEY"
base_url    = "https://api.deepseek.com"
models      = ["deepseek-chat", "deepseek-reasoner"]

[providers.google]
api_key_env  = "GOOGLE_API_KEY"
base_url     = "https://generativelanguage.googleapis.com"
models.text      = ["gemini-2.5-pro", "gemini-2.5-flash", "gemini-2.0-flash"]
models.image     = ["imagen-3.0-generate-002"]
models.tts       = ["en-US-Chirp3-HD-Aoede"]          # Cloud TTS Chirp3-HD
models.asr       = ["latest_long"]                    # Cloud STT
models.embedding = ["text-embedding-004", "gemini-embedding-exp-03-07"]
models.video     = ["veo-3.1-generate-preview"]       # Veo 3.1：8秒，720p/1080p/4k，含原生音频
models.music     = ["lyria-3-pro-preview", "lyria-3-clip-preview"]  # Lyria 3 via Interactions API

[providers.minimax]
api_key_env  = "MINIMAX_API_KEY"
group_id = "your_group_id"        # 必填：MiniMax 多媒体接口 URL 中的 GroupId 参数
models.text  = ["MiniMax-M3", "MiniMax-M2.7"]   # M3 推荐，1M 上下文，支持图片+视频输入
models.tts   = ["speech-2.8-hd", "speech-2.8-turbo", "speech-2.6-hd"]  # speech-2.8-hd 推荐；支持流式/字幕/emotion tag
models.asr   = ["asr-01"]
models.image = ["image-01", "image-01-live"]     # image-01-live 支持画风（漫画/元气/中世纪/水彩）；均支持图生图（人物主体参考）
models.video = ["video-01"]
models.music = ["music-3.0", "music-2.6", "music-cover"]  # music-3.0 推荐；music-cover 翻唱（唯一参考音频翻唱能力）
# 另有独立接口：/v1/lyrics_generation（歌词生成）、/v1/voice_design（音色设计）、/v1/files/upload（声音克隆上传）

[providers.openrouter]
api_key_env = "OPENROUTER_API_KEY"
referer_url = "https://astro-agent.app"    # HTTP-Referer header（OpenRouter 要求）
site_title = "Astro Agent"                  # X-Title header

[routing]
default_text      = "claude-sonnet-4-5"
default_embedding = "text-embedding-004"                  # Google
default_tts       = "minimax/speech-2.8-hd"               # MiniMax 主力（最新 HD）
default_asr       = "google/latest_long"                  # Google Cloud STT 主力
default_image     = "google/imagen-3.0-generate-002"      # Google Imagen 3 主力
default_video     = "google/veo-3.1-generate-preview"     # Google Veo 3.1 主力（含原生音频）
default_music     = "google/lyria-3-pro-preview"          # Google Lyria 3 主力

[routing.fallback]
text  = ["claude-sonnet-4-5", "MiniMax-M3", "gpt-4o", "deepseek-chat"]
tts   = ["minimax/speech-2.8-hd", "google/en-US-Chirp3-HD-Aoede"]
asr   = ["google/latest_long", "minimax/asr-01", "openai/whisper-1"]
image = ["google/imagen-3.0-generate-002", "minimax/image-01", "openai/dall-e-3"]
video = ["google/veo-3.1-generate-preview", "minimax/video-01"]
music = ["google/lyria-3-pro-preview", "minimax/music-3.0"]
```

---

## 设计要点

- **MediaData::Path 延迟上传** — 本地文件调用 provider 时才读取上传，不提前 base64
- **TTS 流式输出** — `stream_synthesize` 返回 `BoxStream<Bytes>`，边生成边播放
- **视频/音乐异步轮询** — 不阻塞主 Agent 循环，Task 模式统一处理
- **Provider 降级** — Registry 配置 fallback 链，主 provider 失败自动切换
