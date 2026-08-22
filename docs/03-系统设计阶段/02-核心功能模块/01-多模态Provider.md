# 多模态 Provider 系统

> 阶段：系统设计 | 状态：**实现定稿** | 更新：2026-08-22

## 架构概览

`crates/agent-providers`（package name `providers`）— 多厂商 LLM/图像/音频/视频 Provider 层。

```
dispatch (唯一入口)
  ├── register_provider() — 按 provider id + config.api_mode 路由
  │     ├── 内置厂商 → trait 系统 (OpenAICompatible / Anthropic / Google / ...)
  │     └── TOML 自定义 → ConfigDrivenCompletionModel (Responses API)
  ├── chat_stream() / chat_stream_direct() — 聊天补全
  ├── generate_image() / text_to_speech() / generate_video() — 媒体
  └── verify() — 连通性探测
```

---

## 协议管线（5 种 ApiMode）

| ApiMode | 协议 | 厂商 |
|---|---|---|
| `ChatCompletions` | OpenAI Chat Completions (`/chat/completions`) | OpenAI、DeepSeek、MiniMax、Azure、智谱、月之暗面、百炼、火山、NVIDIA、OpenRouter、Ollama、混元、Mimo |
| `Responses` | OpenAI Responses API (`/responses`) | OpenAI、DeepSeek、MiniMax + TOML 自定义 |
| `AnthropicMessages` | Anthropic Messages API (`/v1/messages`) | Claude |
| `Interactions` | Google Gemini Interactions (`/v1beta/interactions`) | Google |
| `GeminiNative` | Gemini streamGenerateContent | Google（备用） |

协议选择：`ProviderProfile.api_mode`（静态默认）+ `ProviderConfig.api_mode`（运行时覆盖）。

---

## Trait 系统

### 核心 Trait 层次

```text
CompletionModel          — 聊天补全（唯一异步 trait）
EmbeddingModel           — 向量嵌入
ImageGenModel            — 图像生成
VideoGenModel            — 视频生成
TTSModel                 — 语音合成
MusicGenModel            — 音乐生成

ProviderExt              — 厂商基础（NAME, BASE_URL, auth_headers）
OpenAICompatible: ProviderExt — OpenAI 兼容厂商 hook（声明式常量 + finalize hook）
Capabilities<Chat, Embedding, ImageGen, ...> — 编译期能力声明
```

### OpenAICompatible 声明式常量

新增厂商只需设常量，不需要覆盖 `finalize_body()`：

```rust
impl OpenAICompatible for NewProvider {
    // 基础能力
    const STREAM_USAGE: bool = true;
    const SUPPORTS_TOOLS: bool = true;
    const SUPPORTS_RESPONSES: bool = true;

    // Thinking 格式（4 种）
    const THINKING_FORMAT: ThinkingFormat = ThinkingFormat::DeepSeek;
    //   None             — 不处理 thinking
    //   ReasoningEffort  — OpenAI: reasoning_effort + max_completion_tokens
    //   DeepSeek         — thinking.type=enabled/disabled + reasoning_effort
    //   MiniMaxAdaptive  — reasoning_split=true + thinking.type=adaptive

    // Effort 映射表
    const EFFORT_MAP: &[(&str, &str)] = &[("max", "max"), ("xhigh", "max")];

    // Responses API 行为
    const RESPONSES_STORE_FALSE: bool = false;
    const RESPONSES_PARALLEL_TOOLS: bool = false;
    const RESPONSES_REASONING_SUMMARY: bool = false;
}
```

### 泛型补全模型

| 泛型 | 路径 | 用途 |
|---|---|---|
| `OpenAICompletionModel<Ext>` | `compat/completion.rs` | Chat Completions 路径 |
| `OpenAIResponsesModel<Ext>` | `compat/responses.rs` | Responses API 路径 |
| `ConfigDrivenCompletionModel` | `custom.rs` | TOML 自定义 provider（仅 Responses） |

共享 thinking 转换：`apply_thinking_compat(ThinkingFormat, &[effort_map], body)` — 4 种格式统一处理。

---

## TOML 自定义 Provider

用户在 `~/.astro/config.toml` 声明即可接入任何 OpenAI 兼容 API：

```toml
[custom_providers.my-corp]
name = "Corp LLM"
base_url = "https://llm.corp.internal/v1"
env_keys = ["CORP_API_KEY"]
default_model = "corp-v3"

# 可选：模型声明（不在 OpenRouter 上的私有模型）
[[custom_providers.my-corp.models]]
id = "corp-v3"
display_name = "Corp V3"
context_window = 128000
max_output_tokens = 32000
reasoning = true
supported_efforts = ["high", "max"]
default_effort = "high"
```

- 统一走 Responses API
- 运行时读取，修改后无需重启
- TOML 声明的模型启动时注入 `~/.astro/cache/models.json`

---

## Provider Profile 表

`ProviderProfile` 静态表（`profile.rs::PROFILES`），每厂商一个条目：

| 字段 | 说明 |
|---|---|
| `id` | provider 标识符 |
| `api_mode` | 默认协议 |
| `default_base_url` | 默认 API 基址 |
| `auth` | 认证方式（Bearer / AnthropicKey / GoogleApiKey / AzureHeader / None） |
| `env_keys` | 环境变量名列表 |
| `supports_responses` | 是否支持 Responses API 模式切换（前端 UI 标志） |
| `supports_stream_usage` | 是否支持 stream_options.include_usage |
| `image_mode` | 图片生成协议路由（OpenAi / GoogleInteractions / MiniMax） |
| `default_*_model` | 各模态默认模型名 |

---

## 模型元数据（三层合并）

```
API 厂商端点 (/models)  →  OpenRouter 模型表  →  已知能力补丁
         ↓                        ↓                      ↓
     context_window         pricing / capabilities    DeepSeek thinking
     display_name           reasoning meta            Google web search
     max_output_tokens      input modalities          ...
                    ↓
            enrich_model_info() 合并
                    ↓
         ~/.astro/cache/models.json（单一事实源）
                    ↓
     ┌──────────────┴──────────────┐
     前端模型选择器          运行时 context_window 查询
```

`ModelInfo` 字段：id、display_name、description、context_window、max_output_tokens、capabilities（tools/vision/web/reasoning/file/audio/image_gen/video_gen/music_gen）、reasoning（supported_efforts/default_effort）、pricing、default_parameters、meta_source。

---

## Fallback 链

`ChatTarget` — primary + 最多 3 个备用。首个 chunk 前失败自动切换。

```rust
pub struct ChatTarget {
    pub provider_id: String,
    pub backend_id: String,      // provider kind（如 "deepseek"）
    pub model: String,
    pub api_key: String,
    pub base_url: String,
    pub api_mode: String,        // 随链路传播（如 "responses"）
}
```

`api_mode` 从 Tauri UI → ChatTarget → ProviderConfig → dispatch，确保探测和聊天走同一协议。

---

## 能力矩阵（实际实现）

| 能力 | Anthropic | OpenAI | Google | DeepSeek | MiniMax | 混元 | 智谱 | 百炼 | 火山 |
|---|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|
| 聊天 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Responses API | — | ✓ | — | ✓ | ✓ | — | — | — | — |
| 嵌入 | — | ✓ | ✓ | — | ✓ | ✓ | ✓ | ✓ | ✓ |
| 图像生成 | — | ✓ | ✓ | — | ✓ | ✓ | ✓ | ✓ | ✓ |
| TTS | — | ✓ | ✓ | — | ✓ | ✓ | ✓ | ✓ | ✓ |
| 视频生成 | — | — | ✓ | — | ✓ | ✓ | ✓ | ✓ | ✓ |
| 音乐生成 | — | — | ✓ | — | ✓ | ✓ | ✓ | — | ✓ |
| ASR | — | ✓ | ✓ | — | ✓ | ✓ | ✓ | ✓ | ✓ |

---

## Registry + Dispatch

```rust
// Registry — trait-based，内部持有共享 reqwest::Client
pub struct Registry {
    http: reqwest::Client,
    providers: HashMap<String, DynProvider>,
}

// DynProvider — 按能力组合
DynProvider::new(id, name)
    .with_completion(model)    // CompletionModel
    .with_embedding(model)     // EmbeddingModel
    .with_image_gen(model)     // ImageGenModel
    .with_tts(model)           // TTSModel
    .with_video_gen(model)     // VideoGenModel
    .with_music_gen(model)     // MusicGenModel

// dispatch::register_provider — 按 id + api_mode 路由
fn register_provider(reg, provider, config) {
    let responses = config.api_mode == "responses";
    match provider {
        "openai" if responses => reg.register_openai_responses(...),
        "openai" => reg.register_openai(...),
        "deepseek" if responses => reg.register_openai_compat_responses::<DeepSeek>(...),
        "deepseek" => register_compat::<DeepSeek>(...),
        // ...
        other => lookup_custom_provider(other) 或 fallback OpenAI compat
    }
}
```
