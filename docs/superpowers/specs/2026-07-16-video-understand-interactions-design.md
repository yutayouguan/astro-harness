# 视频理解：Google Interactions 原生 + 独立工具

日期：2026-07-16  
状态：已批准设计，待实现  
参考：[Gemini 视频理解](https://ai.google.dev/gemini-api/docs/video-understanding?hl=zh-cn)、[媒体分辨率](https://ai.google.dev/gemini-api/docs/media-resolution?hl=zh-cn)

相关但不合并：[Vision Interactions 图片理解](./2026-07-16-vision-interactions-native-design.md)、[image_gen Interactions](./2026-07-16-image-gen-interactions-api-design.md)

## 背景

- 仓库已有 `vision`（图片，OpenAI 兼容 `chat/completions`）与 `video_gen`（Veo 生成），**没有**视频理解工具。
- Gemini 推荐 Interactions API 做视频理解，输入支持：File API、内嵌（&lt;100MB）、YouTube URL。
- 与「Google 原生 / OpenAI 分离」策略对齐：本能力 **仅 Google**，不实现 OpenAI 伪兜底。

## 目标

1. 新增独立工具 / toolset：`video_understand`（与 `vision` 并存）。
2. Google：`POST …/v1beta/interactions`；大本地文件经 Files API 上传并轮询 `ACTIVE`。
3. 支持：本地路径、公开 YouTube、多视频合计 ≤10、可选每 part `resolution`（`media_resolution` 参数映射）。
4. 时间戳引用：靠 prompt 中 `MM:SS`（不单开参数）。

## 非目标

- OpenAI / 其它供应商视频理解
- Cloud Storage 注册
- Agent 可复用的独立 `files.delete` / 持久 file-id 工具
- 前端时间轴 / 抽帧可视化
- 聊天附件真多模态视频 parts
- 把本能力塞进 `vision` 工具

## 已确认决策

| 项 | 选择 |
|----|------|
| 产品形态 | 独立工具 `video_understand`（方案 B） |
| 输入 | 内嵌 (&lt;100MB) + Files API（≥100MB）+ YouTube |
| 供应商 | 纯 Google（无 OpenAI 回退） |
| 能力范围 | 核心问答/摘要 + `media_resolution` + 多视频 ≤10 |
| 实现路径 | 工具层 + `files_http` + `interactions_http::google_interactions_video` |
| 模型 | 复用 Google `vision_model`，默认 `gemini-3.5-flash`；不新开配置字段 |
| 分辨率字段 | Interactions 每 video part 的 `"resolution"`（`low`/`medium`/`high`）；非法值拒绝 |

## 工具契约

### 登记

- `name` / `toolset`：`video_understand`
- 加入 `KNOWN_TOOLSET_IDS`、dispatch、前端 `useAgentTools` + i18n
- 凭证：仅 `ctx.image_gen_targets.google()`；无 Key → 明确错误

### 参数

| 字段 | 必填 | 说明 |
|------|------|------|
| `prompt` | 是 | 问答 / 摘要；可含 `MM:SS` |
| `videos` | 条件 | `string[]`，工作区相对或绝对路径；0–10 |
| `youtube_urls` | 条件 | 公开 YouTube URL；0–10 |
| `media_resolution` | 否 | `low` \| `medium` \| `high`；缺省不传（API 默认） |

### 校验

- `videos` 与 `youtube_urls` 至少一类非空
- 两者可混用，**合计条数 ≤ 10**
- 本地不存在 / 不可读 → 立即失败
- `media_resolution` 非空且非三档之一 → 参数错误
- mime：按扩展名映射官方视频表；未知默认 `video/mp4`

### 成功输出（文本）

```
<模型回答>

provider=google
model=gemini-3.5-flash
videos=N files=M youtube=K
media_resolution=low
hint: 引用时间点请用 MM:SS（如 01:15）
```

仅当调用方传入 `media_resolution` 时附该行。`files=M` 为经 Files API 上传的本地文件数。

## HTTP 协议

### Files API — `providers::files_http`（新建）

Base：`google_native_base(config)`。鉴权：`x-goog-api-key`。

1. `POST {native}/upload/v1beta/files` — resumable `start`（Content-Length / Content-Type / display_name）→ 响应头 `x-goog-upload-url`
2. 对 upload URL：`upload, finalize`，body = 原始字节 → `file.uri` / `file.name` / `mime_type`
3. `GET {native}/v1beta/{file.name}` 每 5s 轮询：`ACTIVE` 成功，`FAILED` 报错；超时常量 **10 分钟**

本轮不主动 `files.delete`；不向 Agent 暴露可复用 file name。

本地分流常量：`INLINE_MAX_BYTES = 100 * 1024 * 1024`。YouTube 永不走 Files。

### Interactions — `google_interactions_video`

```http
POST {native}/v1beta/interactions
x-goog-api-key: …
Content-Type: application/json
```

请求要点：

- `model`：`vision_model` 或默认 `gemini-3.5-flash`
- `input`：**先全部 video parts，再 text**（官方最佳实践）
  - Files / YouTube：`{ "type": "video", "uri": "…", "mime_type": "…" }`（YouTube 可省略 mime 或用 `video/mp4`）
  - inline：`{ "type": "video", "data": "<base64>", "mime_type": "…" }`
  - 若设置了 `media_resolution`：每个 video part 增加 `"resolution": "low"|"medium"|"high"`
- 不要 `response_format`（输出文本）

响应解析：优先 `output_text`；否则拼接 `steps[]` 中 `model_output` 的 text parts（与 vision / image-gen Interactions 同策略）。

**禁止**：本工具调用 `google_openai_base`、`chat/completions` 或任何 OpenAI 路径。

## 架构

```
resolve_image_gen_targets (+ vision_model)
  → video_understand.dispatch
       → 解析 videos / youtube_urls（合计 ≤10）
       → 本地：<100MB inline | ≥100MB files_http.upload_and_wait
       → interactions_http::google_interactions_video
```

与 image-gen / vision Interactions **共用** `google_native_base`、鉴权与（若已落地）`interactions_http` 模块；请求/结果类型独立命名，不塞进出图结构体。

## 触及模块

| 区域 | 变更 |
|------|------|
| `providers/.../files_http.rs` | 新建：可恢复上传 + 轮询 |
| `providers/.../interactions_http.rs` | 新建或扩展：`google_interactions_video` |
| `providers` `mod` / `lib` 导出 | 登记新模块 |
| `tools/.../video_understand.rs` | 新工具 |
| `tools` register / dispatch / media `mod` | 接入 |
| `home/.../tools_enabled.rs` | `KNOWN_TOOLSET_IDS` + 名映射 |
| `frontend` `useAgentTools` + i18n | 开关与文案 |
| 单测 | body 拼装、100MB 分流、合计>10、无 Key、Files FAILED、resolution 非法 |

## 错误与边界

| 情况 | 行为 |
|------|------|
| 无 Google Key | 立即失败，提示配置 |
| 视频源皆空 | 参数错误 |
| 合计 >10 | 参数错误 |
| 本地文件缺失 | 立即失败 |
| `media_resolution` 非法 | 参数错误 |
| Files `FAILED` / 超时 | 上浮错误 |
| Interactions HTTP 失败 | 上浮（不回退 OpenAI） |

## 验收

- [ ] 小本地视频 inline 理解成功
- [ ] ≥100MB（或单测模拟阈值）走 Files → ACTIVE → 理解
- [ ] YouTube URL 理解成功
- [ ] 多视频混用（文件 + YouTube）合计 ≤10
- [ ] `media_resolution` 写入各 video part 的 `resolution`
- [ ] 无 Google Key 失败清晰；从不请求 OpenAI
- [ ] Agent 工具面板出现独立 `video_understand` 开关

## 风险

| 风险 | 缓解 |
|------|------|
| Files 轮询久、长视频延迟 | 超时常量 + 清晰错误；5s 间隔 |
| 大文件 inline 撑爆内存 | ≥100MB 强制 Files |
| Interactions `resolution` / 响应字段随模型变化 | 解析双路径；错误原文上浮 |
| 与 image-gen Interactions 重复代码 | 同模块复用 base/鉴权；视频理解独立类型 |
