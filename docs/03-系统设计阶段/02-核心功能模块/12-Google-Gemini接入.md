# Google Gemini 接入文档

> 介绍 Astro 当前接入 Google Gemini 的 API 体系、功能覆盖与配置方式。
>
> 代码位置：`crates/agent-providers/src/google/`

---

## API 体系概览

Google Gemini 对外提供三套 API，Astro 全部接入：

| API | 端点 | 用途 | Astro 对应 Provider ID |
|-----|------|------|----------------------|
| **Interactions API**（推荐） | `v1beta/interactions` | Agent 对话 + 多模态生成 | `google` |
| **generateContent API** | `v1beta/models/{model}:streamGenerateContent` | 原生流式文本 + 工具调用 | `gemini-native` |
| **OpenAI 兼容层** | `v1beta/openai/chat/completions` | 与 OpenAI SDK 兼容 | 不推荐使用 |

**Base URL（统一）**：`https://generativelanguage.googleapis.com`

**API Revision**（Interactions API 请求头）：`Api-Revision: 2026-05-20`

---

## Provider 路由

### `google`（推荐，对应 Interactions API）

```toml
# ~/.astro/config.toml（或环境变量 GOOGLE_API_KEY / GEMINI_API_KEY）
# Google 为内置 Provider，通过 providers.json 或环境变量配置
# 默认模型：gemini-3.5-flash
```

- `ApiMode::Interactions`
- 聊天与多模态生成统一走 `interactions_http.rs`
- HTTP 请求头：`x-goog-api-key: {key}` + `Api-Revision: 2026-05-20`

### `gemini-native`（原生 generateContent）

```toml
# ~/.astro/config.toml（或环境变量 GOOGLE_API_KEY / GEMINI_API_KEY）
# gemini-native 为内置 Provider，通过 providers.json 或环境变量配置
# 默认模型：gemini-3.1-pro
```

- `ApiMode::GeminiNative`
- API key 作 query 参数：`?key={key}&alt=sse`
- 聊天走 `interactions_http.rs` 内的原生 generateContent 流
- 完整支持 `system_instruction`、`function_declarations`、thinking（思考链）

---

## 功能覆盖

### 1. 对话（Chat）

**代码**：`crates/agent-providers/src/google/interactions_http.rs`

| 功能 | 支持 |
|------|------|
| 多轮流式对话 | ✅ SSE 流式 |
| 工具调用（Function Calling） | ✅ `function_declarations` |
| 结构化输出 | ✅ JSON schema |
| 系统提示（System Instruction） | ✅ |
| 图片输入（Vision） | ✅ base64 / URL |
| 音频输入 | ✅ base64 |
| 视频输入 | ✅ base64 / Files API URI |
| 思考链（Thinking） | ✅ `thought` 部分提取 |
| 上下文缓存（Long Context） | ✅ 透传 token |

默认模型：`gemini-3.5-flash`

### 2. 语音合成（TTS）

**代码**：`crates/agent-providers/src/google/interactions_http.rs` → `google_interactions_tts`  
**Veo TTS 模型**：`crates/agent-providers/src/google/veo_http.rs` → `default_tts_model()`

| 功能 | 支持 |
|------|------|
| 单发言人 TTS | ✅ |
| 多发言人 TTS | ✅ 多角色 |
| 流式 PCM 输出 | ✅ PCM s16le |
| WAV 封装 | ✅ `pcm_to_wav()` |
| 音色选择（voice name） | ✅ |
| 语速 / 音调 | ✅ `speaking_rate`、`pitch` |

默认 TTS 模型：`gemini-3.1-flash-tts-preview`  
支持格式：`audio/pcm`（16kHz / 24kHz）

### 3. 图片生成（Image Generation）

**代码**：`crates/agent-providers/src/google/interactions_http.rs` → `google_interactions_image`

| 功能 | 支持 |
|------|------|
| 文本生成图片 | ✅ |
| 图片编辑（inpainting） | ✅ 附参考图 |
| 负面提示词 | ✅ `negative_prompt` |
| 尺寸 / 宽高比 | ✅ `aspect_ratio` |
| 图片数量 | ✅ `number_of_images` |

默认图像模型：`gemini-3.1-flash-image`

### 4. 视觉理解（Vision）

**代码**：`crates/agent-providers/src/google/interactions_http.rs` → `google_interactions_vision`

| 功能 | 支持 |
|------|------|
| 图片问答 | ✅ |
| 多图输入 | ✅ |
| base64 图片 | ✅ |
| URL 图片 | ✅ |
| 分析模式 | ✅ `describe`、`extract_text`、`caption` 等 |

默认模型：`gemini-3.5-flash`

### 5. 视频理解（Video Understanding）

**代码**：`crates/agent-providers/src/google/interactions_http.rs` → `google_interactions_video`

| 功能 | 支持 |
|------|------|
| 视频内容分析 | ✅ |
| base64 视频 | ✅ |
| Files API URI 输入 | ✅ |
| 字幕提取 | ✅ |
| 片段时间戳 | ✅ |
| 分析模式 | ✅ `summarize`、`transcribe`、`describe`、`qa` |

### 6. 音频理解（Audio Understanding）

**代码**：`crates/agent-providers/src/google/interactions_http.rs` → `google_interactions_audio`

| 功能 | 支持 |
|------|------|
| 音频转录（ASR） | ✅ |
| 音频问答 | ✅ |
| base64 音频 | ✅ |
| Files API URI 输入 | ✅ |
| 分析模式 | ✅ `transcribe`、`summarize`、`qa`、`translate` |

默认模型：`gemini-3.5-flash`

### 7. 音乐生成（Music Generation）

**代码**：`crates/agent-providers/src/google/interactions_http.rs` → `google_interactions_music`

| 功能 | 支持 |
|------|------|
| 文本生成音乐 | ✅ |
| 参考图片引导风格 | ✅ |
| 音频格式选择 | ✅ `MP3`、`WAV` |
| 时长控制 | ✅ `duration_secs` |
| 质量档位 | ✅ `clip`（快速）/ `pro`（高质量） |

默认模型：
- clip 档：`lyria-3-clip-preview`
- pro 档：`lyria-3-pro-preview`

### 8. 视频生成（Veo）

**代码**：`crates/agent-providers/src/google/veo_http.rs` → `google_native_generate_video`

| 功能 | 支持 |
|------|------|
| 文本生成视频 | ✅ |
| 图片生成视频（首帧） | ✅ `start_image` |
| 视频续拍（extend） | ✅ `extend_video` |
| 安全级别 | ✅ `allow_adult`、`allow_all`、`dont_allow` |
| 异步轮询 | ✅ `predictLongRunning` → `fetchPredictOperation` |

默认 Veo 模型：`veo-3.1-generate-preview`  
端点：`/v1beta/models/{model}:predictLongRunning`（非 Interactions，走原生生成端点）

### 9. 文件上传（Files API）

**代码**：`crates/agent-providers/src/google/files_http.rs` → `google_files_upload_and_wait`

| 功能 | 支持 |
|------|------|
| multipart 上传 | ✅ |
| 上传等待激活（轮询 ACTIVE） | ✅ |
| 文件删除 | ✅ `google_files_delete` |
| 返回 URI 供后续调用使用 | ✅ `file_uri` |

大文件（视频、音频）建议先上传 Files API，再将返回 URI 传给视频/音频理解接口。

---

## 代码地图

```
providers/src/google/
├── mod.rs                  # 模块入口，GoogleProvider 类型别名
├── defaults.rs             # 默认模型 / BASE URL 常量
├── interactions_http.rs    # 对话流 + TTS / 图片 / 视觉 / 视频理解 / 音频 / 音乐
├── veo_http.rs             # Veo 视频生成 + TTS 模型默认值 + google_native_base
├── files_http.rs           # Files API 上传/删除
├── tools.rs                # 工具声明转换
└── robotics_http.rs        # Robotics-ER generateContent（特殊场景）
```

---

## 认证

| 方式 | 用途 |
|------|------|
| `x-goog-api-key` 请求头 | Interactions API + Files API |
| `?key=` query 参数 | generateContent / streamGenerateContent / Veo |

环境变量（任一可用）：`GOOGLE_API_KEY`、`GEMINI_API_KEY`、`GOOGLE_AI_API_KEY`

---

## 参考链接

- [Interactions API 概览](https://ai.google.dev/gemini-api/docs/interactions-overview)
- [思考链（Thinking）](https://ai.google.dev/gemini-api/docs/thinking)
- [结构化输出](https://ai.google.dev/gemini-api/docs/structured-output)
- [Function Calling](https://ai.google.dev/gemini-api/docs/function-calling)
- [长上下文](https://ai.google.dev/gemini-api/docs/long-context)
- [语音生成（TTS）](https://ai.google.dev/gemini-api/docs/speech-generation)
- [视频理解](https://ai.google.dev/gemini-api/docs/video-understanding)
- [OpenAI 兼容层](https://ai.google.dev/gemini-api/docs/openai)
