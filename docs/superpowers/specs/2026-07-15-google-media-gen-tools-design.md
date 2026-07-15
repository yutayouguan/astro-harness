# Google 媒体生成内置工具（图 / 视频 / 音频）

日期：2026-07-15  
状态：已实现（核心工具路径）  
参考：[Gemini OpenAI 兼容性](https://ai.google.dev/gemini-api/docs/openai?hl=zh-cn#rest)（图 / 视频）；音频为 Gemini TTS 原生 `generateContent`（不在该兼容页）

## 背景

Astro 已有：

- `image_gen`：Google 原生 `generateContent` + OpenAI images；凭证经 `ImageGenTargets`（Google → OpenAI）
- `tts`：仅 OpenAI `/audio/speech`，凭聊天 OpenAI key 或 `OPENAI_API_KEY`
- **无**视频生成 builtin

Google 官方 OpenAI 兼容层已提供：

- `POST …/v1beta/openai/images/generations`
- `POST …/v1beta/openai/videos` + `GET …/videos/{id}`（异步）

语音生成需走 Gemini TTS 原生 API（`responseModalities: AUDIO` + `speechConfig`）。

## 目标

1. Agent 可通过内置工具生成图片、视频、音频，并写入 `workspace/generated/`。
2. 图片 / 视频优先走 Google OpenAI 兼容 REST；音频优先 Google TTS；图与音可回落 OpenAI。
3. 凭证：已启用且有 key 的供应商扫描（Google 优先；图/音备用 OpenAI；视频仅 Google）。

## 非目标

- 多说话人播客流水线、图生视频精调 UI
- 独立媒体设置页 / 非 Agent 的一键按钮（本轮）
- OpenAI Sora 视频备用
- Live API 实时语音

## 已确认决策

| 项 | 选择 |
|----|------|
| 范围 | 图 + 视频 + 音频（B） |
| 工具面 | 扩 `image_gen` + 新 `video_gen` + 扩 `tts`（A） |
| 凭证 | 扫描启用供应商；图/音 Google→OpenAI；视频仅 Google（A） |
| 实现路径 | providers HTTP + 三工具接线（方案 1） |

## 工具契约

### `image_gen`（已有，改 Google 路径）

- 参数：保持现有 `prompt`（及已有可选字段）
- Google：`POST {openai_compat_base}/images/generations`，Bearer，`response_format=b64_json`，默认模型 `gemini-2.5-flash-image`
- 失败时可回退现有原生 `generateContent` IMAGE，再试 OpenAI
- 输出：`generated/img-*.{png|jpg|webp}`

### `video_gen`（新）

- toolset：`video_gen`
- 参数：`prompt`（必填）；可选 `aspect_ratio`、`duration_seconds`
- Google：`POST {openai_compat_base}/videos`（form / JSON 以官方 REST 为准）→ 轮询 `GET …/videos/{id}`（约 10s 间隔，总超时 5–10 分钟）→ 下载 `url`
- 默认模型：`veo-3.1-generate-preview`
- 无 Google 凭证 → 明确错误；无 OpenAI 备用
- 输出：`generated/vid-*.mp4`（或响应 mime 对应后缀）

### `tts`（扩）

- 参数：`text`；`voice`（可选：Google 预置如 `Kore`；OpenAI 路径保留 alloy 等）
- Google：原生 `…/v1beta/models/{tts_model}:generateContent`，`responseModalities: ["AUDIO"]` + `speechConfig.voiceConfig`；默认 `gemini-2.5-flash-preview-tts`（若上线稳定可换 `gemini-3.1-flash-tts-preview`）
- PCM → wav 落盘（与官方示例一致，24kHz / 16-bit / mono）
- 回落：现有 OpenAI `/audio/speech` + `gpt-4o-mini-tts`
- 工具描述改为表明 Google 或 OpenAI

## 架构

```
resolve_media_targets / 扩展 ImageGenTargets
  → gRPC / ToolContext
      → image_gen | video_gen | tts
          → providers HTTP helpers
              → workspace/generated/
```

### providers

- `google_openai_images_generate`（兼容出图）
- `google_openai_videos_create` / `retrieve` / 下载
- `google_tts_generate`（原生 AUDIO）
- base URL：与 chat 相同的 `openai_compatible_base` 归一化（`…/v1beta/openai`）；原生 TTS 继续用去掉 `/openai` 的 `google_base`

### 凭证注入

- 扩展现有 `resolve_image_gen_targets` 模式：至少保证 Google（图/音/视频模型字段）+ 可选 OpenAI（图/音）
- `ToolContext`：复用或轻度扩展 `ImageGenTargets`（或 `MediaGenTargets`）；`tts` 不再仅依赖 `chat_provider == openai`
- chat gRPC 字段：可延续 `image_gen_*` 并增加 video/tts 模型字段，或共用 Google slot + 按工具选默认 model

### 注册面

- `tools`：`register` / `dispatch_tool` + 新 `video_gen.rs`
- `KNOWN_TOOLSET_IDS` / `tools-enabled` / `useAgentTools`：加入 `video_gen`；更新 `tts` / `image_gen` 文案

## 错误与超时

| 情况 | 行为 |
|------|------|
| 无 key | 中文/英文明确错误 |
| 视频 `failed` | 立即失败并带 API 错误信息 |
| 视频轮询超时 | 失败，附带 operation id 便于排查 |
| Google 图失败 | 原生回退 → OpenAI |
| Google TTS 失败 | OpenAI TTS |

## 测试

- providers：mock HTTP — 兼容出图 b64、视频 create→completed、TTS inline PCM
- tools：无凭证 / 参数校验 / 落盘后缀
- 手动：配置 Google key；Agent 分别调用三工具；无 Google 时图/音仍可走 OpenAI

## 验收

- [ ] `image_gen` 在有 Google 时走兼容 `images/generations`（可观测于请求或日志）
- [ ] `video_gen` 可生成并落盘；超时/失败可理解
- [ ] `tts` Google 优先，OpenAI 备用仍可用
- [ ] 工具开关 UI 出现 `video_gen`
- [ ] 无密钥时错误清晰，不 panic

## 风险

| 风险 | 缓解 |
|------|------|
| Veo 耗时长、占工具线程 | 明确超时；间隔休眠；后续可改异步任务 |
| TTS 模型 id 变更 | 常量集中 + 单一默认 |
| 兼容层参数静默忽略 | 只传文档列出的字段 |
| 原生出图与兼容出图行为差异 | 兼容优先，原生作 fallback |
