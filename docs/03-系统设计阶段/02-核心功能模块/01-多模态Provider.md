# 多模态 Provider 系统

> **Harness 边界（2026-09-04）**：Provider 是 Model 网关和协议适配层，不拥有 Agent turn、工具权限或持久化生命周期。Agent 通过 `ResponsesRequest` 交付原生 Items/tool schemas；非 Agent 兼容调用使用 `ChatCompletionRequest`。两条请求类型不互相降级。契约详见 [Responses API 原生工具协议与 Astro 工具协议](../../04-详细设计阶段/04-工具与扩展生态/05-Responses-API原生工具协议与Astro工具协议详细设计.md)。

> 阶段：系统设计 | 状态：**实现定稿** | 更新：2026-09-04

## 架构概览

`crates/agent-providers`（package name `providers`）— 多厂商 LLM/图像/音频/视频 Provider 层。

```
dispatch (唯一入口)
  ├── register_provider() — 按 provider id + config.api_mode 路由
  │     ├── 内置厂商 → trait 系统 (OpenAICompatible / Anthropic / Google / ...)
  │     └── TOML 自定义 → ConfigDrivenResponsesModel (Responses API)
  ├── agent_responses_stream() — Agent Responses-only
  ├── chat_stream() / chat_stream_direct() — 非 Agent 兼容补全
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
ResponsesModel           — Agent Responses 请求
ChatCompletionModel      — 非 Agent 兼容补全
EmbeddingModel           — 向量嵌入
ImageGenModel            — 图像生成
VideoGenModel            — 视频生成
TTSModel                 — 语音合成
MusicGenModel            — 音乐生成

ProviderExt              — 厂商基础（NAME, BASE_URL, auth_headers）
OpenAIResponsesCompatible: ProviderExt — Responses 线路 hook
OpenAICompatible: ProviderExt — Chat Completions 兼容线路 hook
Capabilities<Chat, Embedding, ImageGen, ...> — 编译期能力声明
```

### OpenAICompatible 声明式常量

新增厂商只需设常量，不需要覆盖 `finalize_body()`：

```rust
impl OpenAICompatible for NewProvider {
    // 基础能力
    const STREAM_USAGE: bool = true;
    const SUPPORTS_TOOLS: bool = true;
    // Thinking 格式（4 种）
    const THINKING_FORMAT: ThinkingFormat = ThinkingFormat::DeepSeek;
    //   None             — 不处理 thinking
    //   ReasoningEffort  — OpenAI: reasoning_effort + max_completion_tokens
    //   DeepSeek         — thinking.type=enabled/disabled + reasoning_effort
    //   MiniMaxAdaptive  — reasoning_split=true + thinking.type=adaptive

    // Effort 映射表
    const EFFORT_MAP: &[(&str, &str)] = &[("max", "max"), ("xhigh", "max")];

}

impl OpenAIResponsesCompatible for NewProvider {
    const STORE_FALSE: bool = false;
    const PARALLEL_TOOLS: bool = false;
    const REASONING_SUMMARY: bool = false;
    const SUPPORTS_PERSISTENT_REASONING: bool = false;
}
```

### 泛型补全模型

| 泛型 | 路径 | 用途 |
|---|---|---|
| `OpenAICompletionModel<Ext>` | `compat/completion.rs` | Chat Completions 路径 |
| `OpenAIResponsesModel<Ext>` | `compat/responses.rs` | Responses API 路径 |
| `ConfigDrivenResponsesModel` | `custom.rs` | TOML 自定义 provider（仅 Responses） |

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
- 自定义 Provider 不获得 `persistent` capability；该能力仅由 OpenAI 模型目录的
  `persistent_instructions` 打开

---

## Provider Profile 表

### 接入门槛：只接入支持 Responses API 的提供商

Agent 对话与全部 Agent 能力只走 Responses 路径，因此**新装目录与“添加提供商”列表只提供
支持 Responses API 的提供商**：OpenAI、DeepSeek、Azure OpenAI、OpenRouter、百炼、
MiniMax，以及自定义 Provider（按 OpenAI 兼容处理）。

不支持的提供商（Claude、Google、智谱、Ollama、NVIDIA、月之暗面、火山引擎、混元、
MiniMax Anthropic 通道、Gemini 原生）不再进入目录，桌面端 `add_provider` 也会拒绝这些
kind；历史配置里的条目保持原样，不删除、不重写，仍可用于图片、语音、嵌入等媒体能力。
协议层实现（Chat Completions / Anthropic Messages / Gemini）因此保留，供历史配置使用；
若某厂商后续支持 Responses，只需把它的 `supports_agent_responses` 置为 true 即可重新接入。

判断依据是 [`ProviderProfile::supports_agent_responses`](../../../crates/agent-providers/src/profile.rs)，
桌面端的 `supports_responses_toggle(kind)` 与 DTO 的 `supports_responses_api` 都取自它。

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
| `image_mode` | 图片生成协议路由（OpenAi / AzureOpenAiV1 / GoogleInteractions / MiniMax） |
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

`ModelInfo` 字段：id、display_name、description、context_window、max_output_tokens、capabilities（tools/vision/web/reasoning/file/audio/image_gen/video_gen/music_gen）、reasoning（supported_efforts/default_effort/persistent_instructions）、pricing、default_parameters、meta_source。

### Persistent reasoning

`persistent` 是本地 reasoning effort，不是直接透传给所有 Provider 的 wire 值：

1. 只有 `backend_id = openai` 且模型目录带非空 `persistent_instructions` 时，Desktop 才显示该档位；
2. 目录 enrich 只为 OpenAI 保留并规范化该字段，Azure、OpenRouter 与自定义 Provider 不获得隐式能力；
3. Desktop 经 `ChatRequest.persistent_instructions` 传给 Server，Server 在初始配置与 active-turn 切换时再次校验；
4. Provider adapter 消费内部 `astro_persistent_instructions`，将其合并进 instructions，并从最终 JSON 删除该内部键；
5. OpenAI Responses wire 使用 `reasoning.effort = "disabled"`；其他 adapter 收到 `persistent` 明确失败；
6. 切回普通 effort 后指令可留在 Session 中供再次启用，但不会注入普通请求。

---

## Fallback 链

`model_targets: Vec<ModelTarget>` — primary + 最多 `MAX_MODEL_FALLBACKS`（3）个备用。首个 chunk 前失败自动切换到下一目标。

```rust
pub struct ModelTarget {
    pub provider_id: String,
    pub backend_id: String,      // provider kind（如 "deepseek"）
    pub model: String,
    pub api_key: String,
    pub base_url: String,
}
```

`ModelTarget.api_mode` 随链路传播到 `ProviderConfig`，确保探测和聊天走同一协议。辅助任务各有独立目标链，缺省回退到 primary。

---

## 能力矩阵（实际实现）

| 能力 | Anthropic | OpenAI | Google | Azure | DeepSeek | MiniMax | 混元 | 智谱 | 百炼 | 火山 |
|---|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|
| 聊天 | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Responses API | — | ✓ | — | ✓ | ✓ | ✓ | — | — | — | — |
| Persistent reasoning | — | 目录门控 | — | — | — | — | — | — | — | — |
| 嵌入 | — | ✓ | ✓ | — | — | ✓ | ✓ | ✓ | ✓ | ✓ |
| 图像生成 | — | ✓ | ✓ | ✓ | — | ✓ | ✓ | ✓ | ✓ | ✓ |
| TTS | — | ✓ | ✓ | — | — | ✓ | ✓ | ✓ | ✓ | ✓ |
| 视频生成 | — | — | ✓ | — | — | ✓ | ✓ | ✓ | ✓ | ✓ |
| 音乐生成 | — | — | ✓ | — | — | ✓ | ✓ | ✓ | — | ✓ |
| ASR | — | ✓ | ✓ | — | — | ✓ | ✓ | ✓ | ✓ | ✓ |

### Azure Foundry `gpt-image-2`

Azure 的 Responses 和图片生成共用 OpenAI v1 base URL 与 Bearer 认证。新配置默认使用 `https://<resource>.services.ai.azure.com/openai/v1`，同时兼容 `https://<resource>.openai.azure.com/openai/v1`。聊天部署名与 `gpt-image-2` 媒体部署名独立配置，生图路由仍以 `ImageGenMode::AzureOpenAiV1` 与其他协议隔离。

`ImageGenConfig` 传递尺寸、数量、PNG/JPEG/WebP、compression、quality 和 background。Provider 设置另存 `active_image_provider_id`，因此 Azure 可作为默认生图 Provider，而不改变默认聊天 Provider。Workflow 运行时从 keyring/环境变量注入凭据，不将 Key 写入节点 JSON。

详细配置见 [Azure AI Foundry `gpt-image-2` 使用说明](../../azure-gpt-image-2.md)，分层和安全契约见 [接入设计](../../superpowers/specs/2026-09-01-azure-gpt-image-2-design.md)。

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
    .with_responses(model)       // ResponsesModel
    .with_chat_completion(model) // ChatCompletionModel
    .with_embedding(model)     // EmbeddingModel
    .with_image_gen(model)     // ImageGenModel
    .with_tts(model)           // TTSModel
    .with_video_gen(model)     // VideoGenModel
    .with_music_gen(model)     // MusicGenModel

// dispatch::register_provider — Chat/media 与 Responses 能力独立挂载
fn register_provider(reg, provider, config) {
    let responses = config.api_mode == "responses";
    match provider {
        "openai" => reg.register_openai(...),
        "azure" => reg.register_azure(...),
        "deepseek" => register_compat::<DeepSeek>(...),
        // ...
        other => lookup_custom_provider(other) 或 fallback OpenAI compat
    }
    if responses {
        reg.attach_responses::<Provider>(...);
    }
}
```
