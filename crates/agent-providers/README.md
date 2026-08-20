# providers

多供应商 AI 能力统一封装层：通过 trait 系统与 OpenAI 兼容宏实现 16+ 厂商的流式聊天、图像/音频/视频生成、嵌入向量等能力一键接入。

## 核心职责

- 定义统一消息模型（`Message`）与流式 chunk 类型（`StreamChunk` / `Usage`）
- 提供编译期能力检查的 trait 系统（`Capabilities` / `Capable<M>` / `Nothing`）
- 实现 OpenAI 兼容层：`OpenAICompatible` trait + `openai_compat!` 宏，一行接厂商
- 覆盖 16 个厂商实现（5 种协议管线）：原生 + OpenAI 兼容 + Responses API
- 统一分发入口：`chat_stream` / `generate_image` / `text_to_speech` / `generate_video` / `embed`
- 流式补全控制：`CompletionStream` / `PauseControl`（暂停/恢复流）
- 厂商配置表（`ProviderProfile` / `ApiMode`）与环境变量 API Key 探测
- 跨厂商共享基础设施：HTTP/SSE 流解析、API Key 验证、视觉输入处理、媒体格式转换

## 模块结构

| 文件/目录 | 职责 |
|-----------|------|
| **types/** | |
| `types/message.rs` | `Message` — Provider 统一消息类型 |
| `types/stream.rs` | `CompletionStream` / `StreamChunk` / `Usage` / `PauseControl` |
| `types/error.rs` | `ProviderError` / `ProviderResult` |
| `types/media.rs` | `GeneratedImage` / `GeneratedAudio` / `GeneratedVideo` |
| `types/image_gen.rs` | 图像生成请求与参数 |
| `types/request.rs` | 请求参数封装 |
| `types/mod.rs` | `ProviderConfig` 与类型聚合 |
| **traits/** | |
| `traits/capability.rs` | `Capabilities` trait / `Capable<M>` / `Nothing` — 编译期能力检查 |
| `traits/client.rs` | `ProviderExt` trait — 厂商元信息（名称/base_url/auth） |
| `traits/models.rs` | `CompletionModel` / `EmbeddingModel` 等能力 trait |
| `traits/dyn_provider.rs` | 动态分发 provider 抽象 |
| **compat/** | |
| `compat/completion.rs` | `OpenAICompletionModel` — OpenAI 兼容聊天实现 |
| `compat/messages.rs` | 消息格式转换（统一 → OpenAI） |
| `compat/sse.rs` | SSE 流解析 |
| `compat/responses.rs` | OpenAI Responses API 支持 |
| `compat/media.rs` | 兼容层媒体模型（Embedding/ImageGen/TTS） |
| `compat/think_tag.rs` | 思考标签解析（`<think>` tag 提取） |
| **impls/** | |
| `impls/openai.rs` | OpenAI 原生实现 |
| `impls/openai_responses.rs` | OpenAI Responses API 实现 |
| `impls/anthropic.rs` | Claude/Anthropic 原生实现 |
| `impls/google.rs` | Google Gemini OpenAI 兼容实现 |
| `impls/gemini_native.rs` | Google Gemini 原生实现 |
| `impls/deepseek.rs` | DeepSeek 实现 |
| `impls/azure.rs` | Azure OpenAI 实现 |
| `impls/minimax_chat.rs` | MiniMax 聊天实现 |
| `impls/ollama.rs` | Ollama 本地模型实现 |
| `impls/openrouter.rs` | OpenRouter 多模型路由实现 |
| `impls/zhipu.rs` | 智谱 AI 实现 |
| `impls/moonshot.rs` | Moonshot AI 实现 |
| `impls/volcengine.rs` | 火山引擎实现 |
| `impls/bailian.rs` | 阿里百炼实现 |
| `impls/hunyuan.rs` | 腾讯混元实现 |
| `impls/mimo.rs` | Mimo 实现 |
| `impls/nvidia.rs` | NVIDIA NIM 实现 |
| **厂商专属模块** | |
| `anthropic/` | Anthropic 消息转换、工具格式、batch、token 计数 |
| `google/` | Gemini 原生工具格式、Files API、Interactions API、VEO 视频、Robotics |
| `openai/` | OpenAI 图像生成、TTS、嵌入向量、Responses API、默认配置 |
| `minimax/` | MiniMax 音乐/TTS/图像/视频生成、文件上传、voice clone |
| **其他** | |
| `dispatch.rs` | 统一分发入口：`chat_stream` / `generate_image` / `text_to_speech` 等 |
| `registry.rs` | 协议管线注册表 |
| `profile.rs` | `ProviderProfile` / `ApiMode` / `PROFILES` 静态配置表 |
| `shared/http.rs` | HTTP 流式请求与 SSE 解析 |
| `shared/verify.rs` | API Key 验证与连通性检查 |
| `shared/vision.rs` | 视觉输入处理（图像 base64/URL 转 content part） |
| `shared/media.rs` | 媒体格式转换 |
| `shared/extractor.rs` | `Extractor` — 结构化输出提取器 |

## 核心类型与 API

- `chat_stream(config, messages, tools)` — 统一聊天流式入口，返回 `CompletionStream`
- `generate_image(config, prompt, params)` — 图像生成
- `text_to_speech(config, text, voice)` — TTS 语音合成
- `generate_video(config, prompt)` — 视频生成
- `embed(config, texts)` — 嵌入向量生成
- `CompletionStream` — 流式补全流，可 `PauseControl` 暂停/恢复
- `StreamChunk` — 流式 chunk：delta text / tool_call_delta / usage / finish
- `Message` — Provider 消息类型（与 `types::Message` 互转）
- `ProviderConfig` — Provider 配置：base_url / api_key / model / 参数
- `ProviderProfile` — 静态厂商配置表条目
- `Capabilities` trait — 编译期声明厂商支持的能力组合
- `OpenAICompatible` trait — OpenAI 兼容层标记
- `openai_compat!` 宏 — 一行声明一个 OpenAI 兼容 provider
- `Extractor` / `ExtractorBuilder` — 结构化输出提取器
- `VerifyResult` — API Key 验证结果

## Crate 关系

| 方向 | crate | 说明 |
|------|-------|------|
| 被依赖 | `agent`（agent-core） | 流式调用、fallback 切换、PauseControl |
| 被依赖 | `tools`（agent-tools） | 媒体生成工具调用 provider 层 |
| 被依赖 | `agent-server` | gRPC 服务暴露 provider 能力 |
| 外部依赖 | `reqwest` | HTTP 客户端（rustls-tls） |
| 外部依赖 | `serde` / `serde_json` | 序列化 |

## 关键不变量

1. **单一分发入口**：外部只通过 `dispatch` 模块的函数调用 provider，不直接实例化厂商实现
2. **编译期能力检查**：`Capable<M>` / `Nothing` 类型级标记，调用未声明能力时编译失败
3. **首包前 fallback**：流式调用在收到第一个 chunk 前失败时自动切换到 fallback 目标
4. **OpenAI 兼容宏**：`openai_compat!` 确保兼容厂商实现统一，减少样板代码
5. **SSE 流解析**：共享 SSE 解析器处理所有兼容厂商的 `text/event-stream` 响应

## 测试

```bash
cargo test -p providers
```
