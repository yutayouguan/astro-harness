# 音频理解工具：Google Interactions 原生 + OpenAI 分离

日期：2026-07-16  
状态：已批准设计  
参考：[Gemini 音频理解（Interactions）](https://ai.google.dev/gemini-api/docs/audio?hl=zh-cn)  
对齐：[Vision Interactions 原生设计](./2026-07-16-vision-interactions-native-design.md)

## 背景

- 仓库已有 `vision`（图片理解）、`tts`（生语音），**尚无**独立音频理解工具。
- Gemini 正式推荐 Interactions API 做音频描述、转写、说话人分离与情绪等。
- 出图 / 视觉升级方向一致：**Google 原生 Interactions，OpenAI 独立接口**，不再经 Google 的 OpenAI 兼容层。

## 目标

1. 新增工具 `audio_understand`：`mode` = `describe` | `transcribe`。
2. Google：直接 `POST …/v1beta/interactions`；本地 / http 音频用 `type: audio`；YouTube 用 `type: video`。
3. OpenAI：`describe` → `chat/completions`；`transcribe` → `audio/transcriptions`（Whisper）+ 同形 JSON 尽力兜底。
4. `transcribe`：Google 用 `response_format` + JSON schema（summary / segments / speaker / timestamp / emotion 等）。

## 非目标

- Files API（>20MB 本地上传）
- 聊天附件改为真多模态 audio parts
- 前端波形 / 时间轴可视化
- Anthropic 等其它供应商
- 改 ProvidersPanel 表单结构（沿用 `vision_model`；Whisper 用内置默认）
- 多音频合并输入

## 已确认决策

| 项 | 选择 |
|----|------|
| 范围 | C：describe + 全量结构化 transcribe |
| 工具形态 | 新工具 `audio_understand` + `mode` |
| Google API | Interactions（非 generateContent / 非 openai 兼容） |
| OpenAI | describe→Chat；transcribe→Whisper + JSON 尽力 |
| 输入 | inline base64 / http uri + YouTube；无 Files API |
| 实现路径 | 双 HTTP helper + 工具分流（方案 1） |
| 凭证顺序 | Google → OpenAI（同 vision） |
| 模型字段 | Google 复用 `vision_model`；不新增 panel 字段 |

## 工具契约

### 参数

| 字段 | 类型 | 说明 |
|------|------|------|
| `audio_url` | `string` | 工作区相对路径、`http(s)` 音频 URL、或 YouTube URL |
| `prompt` | `string?` | 缺省：describe→「请描述这段音频」；transcribe→内置转写指令（可覆盖） |
| `mode` | enum | `describe`（默认）\| `transcribe` |
| `start` / `end` | `string?` | 可选时间窗 `MM:SS`；写入 prompt |

单文件输入；不做 `audio_urls` 数组。

### 模式语义

- **describe**：自然语言描述 / 总结 / 问答（含非语音声）。
- **transcribe**：结构化 JSON（见下）；Google 强制 `response_format`；OpenAI Whisper 出文本后尽力组装同形 JSON，缺字段可省略并标 `fallback=openai`。

### Transcribe JSON schema

```json
{
  "summary": "string",
  "segments": [
    {
      "speaker": "string",
      "timestamp": "MM:SS",
      "content": "string",
      "language": "string",
      "translation": "string",
      "emotion": "happy|sad|angry|neutral"
    }
  ]
}
```

- `required`：`summary`；`segments[]` 的 `speaker` / `timestamp` / `content` / `emotion`（对齐官方示例）。
- `language` / `translation`：建议输出，不强制。

内置 transcribe prompt 要求：说话人分离、`MM:SS` 时间戳、语言检测、非英译英（若适用）、情绪四选一、文首 summary。若调用方传入 `prompt` 则覆盖默认指令，但仍附带 schema（Google）。

### 成功输出

- describe：助手文本；末尾 `provider=` / `model=` / `mode=`。
- transcribe：JSON 正文为主，再附元信息行；OpenAI 不完整时原文或尽力 JSON + `fallback=openai`。

## 协议

### Google Interactions

```http
POST {google_native_base}/v1beta/interactions
x-goog-api-key: …
Content-Type: application/json
```

请求要点：

- `model`：`vision_model` 或默认 `gemini-3.5-flash`
- `input`：`[{type:"text",text}, {type:"audio"|"video", data|uri, mime_type}]`
  - 本地文件 → `type: audio` + `data`（纯 base64）+ `mime_type`
  - `http(s)` 非 YouTube → `type: audio` + `uri` + `mime_type`
  - YouTube（主机含 `youtube.com` / `youtu.be`）→ `type: video` + `uri` + `mime_type: "video/mp4"`
- `transcribe`：`response_format` 带上述 JSON schema
- 解析：优先 `output_text`；否则从 `steps[]` 中 `model_output` 的 text parts 拼接
- Base：`google_native_base`（剥掉 `/v1beta/openai`），再拼 `/v1beta/interactions`

Google 路径**禁止**再调用 `google_openai_base` / openai 兼容 completions。

### OpenAI（分开）

| mode | 接口 |
|------|------|
| `describe` | `POST {openai_compat_base}/chat/completions`：携带文本 + 音频输入（本地 → data URL / `input_audio` 字段，按模型支持拼装）；默认模型 `gpt-4o`（或配置中的 vision/chat 模型） |
| `transcribe` | `POST {openai_compat_base}/audio/transcriptions` multipart，模型默认 `whisper-1`；将纯文本尽力映射为 schema JSON（缺 speaker/emotion 等则省略或占位），并标 `fallback=openai` |

YouTube：**仅 Google**。OpenAI 路径遇到 YouTube → 明确错误（若 Google 已失败则汇总进最终错误）。

### 共享解析（工具层）

- 本地相对路径：`workspace_dir` 读字节 → mime + base64。
- mime：`wav→audio/wav`，`mp3→audio/mp3`，`aiff→audio/aiff`，`aac→audio/aac`，`ogg→audio/ogg`，`flac→audio/flac`，`m4a→audio/mp4`；未知默认 `audio/mp3`。
- `start` / `end`：须匹配 `^\d{1,2}:\d{2}$`，非法立即参数错误；有效时追加到 prompt（「Provide content from start to end」或中文等价）。

## 架构

```
resolve_image_gen_targets (+ vision_model)
  → audio_understand.dispatch
       → google_interactions_audio      // providers::interactions_http
       → openai_audio_describe          // chat/completions
       → openai_audio_transcriptions    // Whisper multipart
```

与 image-gen / vision Interactions 设计共用 `google_native_base`、`x-goog-api-key` 与（若已落地）`interactions_http` 模块；音频请求 / 结果类型独立命名，不塞进视觉或出图结构体。

## 触及模块

| 区域 | 变更 |
|------|------|
| `providers/.../interactions_http.rs`（新建或复用） | `google_interactions_audio` |
| `providers/.../media_http.rs` | Whisper + OpenAI describe；`default_whisper_model` |
| `tools/.../audio_understand.rs` + `media/mod.rs` | 新工具 |
| 工具 dispatch / registry | 挂载 `audio_understand` |
| `frontend/.../useAgentTools.ts` + i18n | 卡片与文案 |
| 测试 | mock：describe / transcribe schema / YouTube→video / Whisper / Google→OpenAI / 无密钥 |

## 错误与边界

| 情况 | 行为 |
|------|------|
| `audio_url` 空 | 参数错误 |
| 本地文件不存在 | 立即失败 |
| `start`/`end` 格式非法 | 参数错误 |
| YouTube + 仅 OpenAI | 明确「YouTube 仅 Google Interactions」 |
| Google 失败 | 记入 errors，尝试 OpenAI（非 YouTube） |
| Whisper→JSON 不完整 | 尽力 JSON / 原文 + `fallback=openai` |
| 两边都失败 | 汇总错误，提示配置 Key |
| 超大 inline | API 报错上浮（本轮不加 Files API） |

## 默认模型

| 供应商 / 用途 | 默认 |
|---------------|------|
| Google | `gemini-3.5-flash`（或 `vision_model`） |
| OpenAI describe | `gpt-4o` |
| OpenAI transcribe | `whisper-1` |

## 验收

- [ ] Google：本地音频 describe 可用
- [ ] Google：transcribe 返回合法 `summary` + `segments`
- [ ] Google：YouTube URL 走 `type: video`
- [ ] 仅 OpenAI：describe（Chat）+ transcribe（Whisper）可用
- [ ] YouTube 在仅 OpenAI 时错误清晰
- [ ] Google 失败可落到 OpenAI（非 YouTube）
- [ ] 工具文案体现 Interactions 原生 + OpenAI 分离
- [ ] 无密钥时错误清晰，不 panic

## 风险

| 风险 | 缓解 |
|------|------|
| Interactions 响应字段形态差异（`output_text` vs `steps`） | 解析器兼容两条路径；单测覆盖 |
| OpenAI Chat 音频字段因模型而异 | describe 失败信息清晰；转写优先 Whisper |
| OpenAI 结构化能力弱于 Gemini | 文档与输出标 `fallback=openai` |
| 本地大音频撑爆请求体 | 与 vision 一致先发送；后续 Files API |
| 与 image-gen / vision Interactions 重复 | 同模块复用 base/鉴权；音频独立类型 |
