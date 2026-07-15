# 视频理解：Google Interactions 原生 + 独立工具

日期：2026-07-16  
状态：已批准设计，待实现  
参考：[Gemini 视频理解](https://ai.google.dev/gemini-api/docs/video-understanding?hl=zh-cn)

相关但不合并：[Vision Interactions 图片理解](./2026-07-16-vision-interactions-native-design.md)、[TTS Interactions](./2026-07-16-tts-interactions-api-design.md)、[audio_understand](./2026-07-16-audio-understand-interactions-native-design.md)

> **修订说明**：本文件覆盖同日早先草稿。定稿以对话决策为准：单一 `video_url`、`mode`（含 `timeline` JSON）、仅 Google、不做多视频 / `media_resolution`。

## 背景

- 仓库已有 `vision`（图片）与 `video_gen`（Veo 生成），**没有**视频理解工具。
- Gemini 推荐 Interactions API 做视频理解；输入支持 File API、内嵌（&lt;100MB）、YouTube URL。
- 策略对齐：本能力 **仅 Google 原生**，与 OpenAI 接口完全分开，不实现伪回退。

## 目标

1. 新增独立工具 / toolset：`video_understand`（与 `vision` 并存）。
2. Google：`POST …/v1beta/interactions`；本地大文件经 Files API 上传并轮询 `ACTIVE`。
3. 支持：工作区路径、http(s) 直链（先下载再分流）、公开 YouTube。
4. `mode`：`qa` | `summarize` | `timeline`（后者结构化 JSON）。
5. 时间戳引用：靠 prompt 中 `MM:SS`（不单开参数）。

## 非目标

- OpenAI / 其它供应商视频理解
- 多视频合计 ≤10、`media_resolution`
- Cloud Storage 注册
- 向 Agent 暴露可复用 file-id / 独立 `files.delete` 工具
- 前端时间轴 / 抽帧可视化
- 聊天附件真多模态视频 parts
- 把本能力塞进 `vision` 工具

## 已确认决策

| 项 | 选择 |
|----|------|
| 产品形态 | 独立工具 `video_understand`（方案 A） |
| 实现路径 | 双模块 HTTP + 工具分流（方案 1）：`files_http` + `interactions_http` |
| 输入 | 全套：inline（&lt;100MB）+ Files API（≥100MB）+ YouTube |
| 供应商 | 纯 Google（无 OpenAI 回退） |
| 能力范围 | `qa` / `summarize` 自然语言 + `timeline` 结构化 events |
| 模型 | 复用 Google `vision_model`，默认 `gemini-3.5-flash`；不新开配置字段 |
| 用完后 delete | 可选尝试 `files.delete`；失败忽略，不影响主结果 |

## 工具契约

### 登记

- `name` / `toolset`：`video_understand`
- 加入 `KNOWN_TOOLSET_IDS`、dispatch、前端 `useAgentTools` + i18n
- 凭证：仅 `ctx.image_gen_targets.google()`；无 Key → 明确错误

### 参数

| 字段 | 类型 | 说明 |
|------|------|------|
| `video_url` | `string` | 必填。工作区相对路径、`http(s)` 直链，或公开 YouTube URL |
| `prompt` | `string?` | 自定义指令；缺省用 `mode` 内置文案（可被覆盖） |
| `mode` | enum | `qa`（默认）\| `summarize` \| `timeline` |

规则：

- 只接受 **一个** 视频源（官方最佳实践）
- YouTube：识别 `youtube.com/watch`、`youtu.be` 等公开链接
- 工具描述写明：提示中可用 `MM:SS` 时间戳提问

### mode 语义

| mode | 默认 prompt 意图 | 输出 |
|------|------------------|------|
| `qa` | 按 `prompt` 回答；无 prompt 则引导概括并用要点回答 | 自然语言 |
| `summarize` | 3–5 句总结 + 音频/视觉要点 | 自然语言 |
| `timeline` | 关键事件时间线 | **结构化 JSON**（见下） |

### `timeline` JSON schema

```json
{
  "events": [
    { "timestamp": "MM:SS", "description": "…", "modality": "visual|audio|both" }
  ]
}
```

- Google：`response_format` + JSON schema
- 解析失败：返回原文并标 `parse_error=true`（不回退 OpenAI）

### 成功输出尾部元信息

```
…

provider=google
model=gemini-3.5-flash
mode=timeline
input=file_api|inline|youtube
```

可选附加：`hint: 引用时间点请用 MM:SS（如 01:15）`

## 输入路由

```
video_url
  ├─ YouTube 公开链接 ──► Interactions: { type: video, uri }
  ├─ http(s) 非 YouTube ──► 下载到缓冲再按本地大小规则
  └─ 工作区相对路径 ──► 读字节 + 推断 mime
        ├─ size < 100MB ──► inline: { type: video, data: base64, mime_type }
        └─ size ≥ 100MB ──► File API → ACTIVE → { type: video, uri, mime_type }
```

| 项 | 约定 |
|----|------|
| inline 阈值 | `INLINE_MAX_BYTES = 100 * 1024 * 1024`；≥ 强制 File API |
| mime | `mp4/mpeg/mov/avi/webm/wmv/3gpp/mpg/flv` → 对应 `video/*`；未知默认 `video/mp4` |
| File 轮询 | 间隔 ~5s；超时 **10 分钟**；`FAILED` 立即失败 |
| YouTube | 仅公开视频；限制在工具描述中注明 |
| 清理 | 用完后可选 `files.delete`；失败不影响主结果 |

## HTTP 协议

### Files API — `providers::files_http`（新建）

Base：`google_native_base(config)`。鉴权：`x-goog-api-key`。

1. `POST {native}/upload/v1beta/files` — resumable `start`（Content-Length / Content-Type / display_name）→ 响应头 `x-goog-upload-url`
2. 对 upload URL：`upload, finalize`，body = 原始字节 → `file.uri` / `file.name` / `mime_type`
3. `GET {native}/v1beta/{file.name}` 每 5s 轮询：`ACTIVE` 成功，`FAILED` 报错；超时 10 分钟
4. （可选）`DELETE {native}/v1beta/{file.name}` — best-effort

YouTube 永不走 Files。不向 Agent 暴露可复用 file name。

### Interactions — `google_interactions_video`

```http
POST {google_native_base}/v1beta/interactions
x-goog-api-key: …
Content-Type: application/json
```

请求要点：

- `model`：`vision_model` 或默认 `gemini-3.5-flash`
- `input`：**先 video part，再 text**（官方最佳实践）
  - Files / YouTube：`{ "type": "video", "uri": "…", "mime_type": "…" }`（YouTube 可省略 mime 或用 `video/mp4`）
  - inline：`{ "type": "video", "data": "<base64>", "mime_type": "…" }`
- `timeline`：附加 `response_format`（events schema）
- `qa` / `summarize`：不设 `response_format`

响应解析：优先 `output_text`；否则拼接 `steps[]` 中 `model_output` 的 text parts（与 vision / TTS Interactions 同策略）。

**禁止**：本工具调用 `google_openai_base`、`chat/completions` 或任何 OpenAI 路径。

## 架构

```
resolve_image_gen_targets (+ vision_model)
  → video_understand.dispatch
       → 分类 video_url（youtube | http 下载 | 本地）
       → inline(<100MB) | files_http.upload_and_wait(≥100MB) | youtube uri
       → interactions_http::google_interactions_video
         （video 在前、text 在后；timeline 带 response_format）
```

与 TTS / vision Interactions **共用** `google_native_base`、`interactions_url`、`x-goog-api-key`；视频请求/结果类型独立命名，不塞进 TTS 或出图结构体。  
`media_http` **不承载**视频理解路径。

## 触及模块

| 区域 | 变更 |
|------|------|
| `providers/.../files_http.rs` | 新建：可恢复上传 + 轮询 + 可选 delete |
| `providers/.../interactions_http.rs` | 扩展：`google_interactions_video` + body/解析 |
| `providers` `mod` / `lib` 导出 | 登记 `files_http` |
| `tools/.../video_understand.rs` | 新工具 |
| `tools` register / dispatch / media `mod` | 接入 |
| `home/.../tools_enabled.rs` | `KNOWN_TOOLSET_IDS` + 名映射 |
| `frontend` `useAgentTools` + i18n | 开关与文案 |
| 单测 | 分流阈值、YouTube 分类、timeline schema、无 Key、Files FAILED、从不碰 OpenAI |

## 错误与边界

| 情况 | 行为 |
|------|------|
| 无 Google Key | 立即失败，提示配置；绝不请求 OpenAI |
| `video_url` 空 | 参数错误 |
| 本地文件不存在 | 立即失败 |
| 远程非 YouTube 下载失败 | 上浮错误 |
| Files `FAILED` / 轮询超时 | 上浮错误 |
| Interactions HTTP 失败 | 上浮（不回退 OpenAI） |
| `timeline` JSON 解析失败 | 原文 + `parse_error=true` |
| 上传后 delete 失败 | 忽略，不影响主结果 |

## 默认模型

| 供应商 | 默认 |
|--------|------|
| Google | `gemini-3.5-flash`（经 `vision_model` 可覆盖） |

## 验收

- [ ] `<100MB` 本地 → inline → 自然语言理解
- [ ] `≥100MB`（或单测模拟阈值）→ File API → `ACTIVE` → 理解
- [ ] 公开 YouTube URL 理解成功
- [ ] `mode=qa|summarize` 自然语言；`timeline` 产出合法 `events` JSON
- [ ] 元信息含 `provider=google`、`model=`、`mode=`、`input=file_api|inline|youtube`
- [ ] 无 Google Key 失败清晰；从不请求 OpenAI
- [ ] Agent 工具面板出现独立 `video_understand` 开关

## 风险

| 风险 | 缓解 |
|------|------|
| Files 轮询久、长视频延迟 | 10min 超时 + 清晰错误；5s 间隔 |
| 大文件误走 inline 撑爆内存 | ≥100MB 强制 Files |
| Interactions 响应字段形态差异 | 解析器兼容 `output_text` 与 `steps`；单测覆盖 |
| `timeline` schema 不被模型严格遵守 | `parse_error=true` + 原文上浮 |
| 与 TTS / vision Interactions 重复代码 | 同模块复用 URL/鉴权；视频独立类型 |
