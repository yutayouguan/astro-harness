# video_gen：Google Veo 原生 predictLongRunning + OpenAI 兼容回退

日期：2026-07-16  
状态：已批准设计  
升级：[2026-07-15-google-media-gen-tools-design.md](./2026-07-15-google-media-gen-tools-design.md) 中 `video_gen` 路径  
参考：[Gemini API — Veo 视频生成（REST）](https://ai.google.dev/gemini-api/docs/veo?hl=zh-cn#rest)

## 背景

- `video_gen` 当前经 `google_openai_generate_video`：`POST …/v1beta/openai/videos`（multipart）→ 轮询 `GET …/videos/{id}` → Bearer 下载。
- 官方 Veo 文档主推原生长时运行接口：`models/{model}:predictLongRunning` + 轮询 operation + `x-goog-api-key` 下载 `video.uri`。
- 仓库内 TTS 已走 Google 原生；图片有兼容/原生双路径。视频应对齐：**原生优先、兼容回退、与 OpenAI 请求体完全分开**。

## 目标

1. Google 首选：`POST …/v1beta/models/{model}:predictLongRunning`（JSON instances/parameters）。
2. 失败时回退现有 OpenAI 兼容 `/videos`（不删除兼容实现）。
3. 能力完整对齐文档：文生视频、图生视频、首尾帧插值、最多 3 张参考图、视频扩展；以及 `aspectRatio` / `resolution` / `durationSeconds` / `personGeneration` / `seed`。
4. 续拍：`extend_video`（本地）→ `extend_video_uri` → `extend_video_id`（兼容回退）。
5. 产物仍写入 `generated/videos/`，进度写 `progress-latest.txt`。

## 非目标

- 独立视频 UI / 非 Agent 一键生成
- OpenAI Sora 或其它厂商视频
- 批量多视频输出（Veo 3.1 每请求 1 条）
- Files API 云端上传（本轮本地 → `inlineData` base64）
- 改 ProvidersPanel 表单结构（沿用既有 Google video_model / API key）

## 已确认决策

| 项 | 选择 |
|----|------|
| API 策略 | 原生优先，失败回退 OpenAI 兼容 |
| 功能范围 | 完整对齐 Veo 文档（含最多 3 参考图 + 续拍） |
| 续拍输入 | 本地路径 + URI + `extend_video_id`；优先本地 |
| 实现结构 | `media_http.rs` 新增原生函数 + 工具层 try/fallback（方案 A） |
| 默认模型 | `veo-3.1-generate-preview` |
| `negative_prompt` / `style` | 仅兼容回退转发；原生忽略并在结果注明 |

## 架构

```
video_gen (tools)
    │ 校验参数 / 读工作区图片与视频 / 落盘
    ▼
providers::media_http
    ├─ google_native_generate_video     ← 首选
    │     POST …/models/{model}:predictLongRunning
    │     GET  …/{operation.name}
    │     GET  video.uri  (x-goog-api-key)
    └─ google_openai_generate_video     ← 回退
          POST …/openai/videos (multipart)
          GET  …/videos/{id}
```

| 层 | 职责 |
|----|------|
| `tools/.../video_gen.rs` | schema、互斥校验、路径解析、写盘、进度与 `next_shot_hint`、native→compat 回退 |
| `providers/.../media_http.rs` | HTTP 协议、轮询、下载；`VideoGenExtras` / `GeneratedVideo` |
| Base URL | 原生用 `google_native_base`；兼容用 `google_openai_base`（与现有一致） |

与图片路径差异：图片是兼容优先再原生；**视频为原生优先再兼容**。

## 工具契约

### 参数（`VideoGenArgs`）

| 字段 | 类型 | 说明 |
|------|------|------|
| `prompt` | string | 必填 |
| `aspect_ratio` | string? | `16:9` / `9:16` |
| `duration_seconds` | u32? | `4` / `6` / `8`；扩展 / 参考图 / 1080p·4k 等高级模式强制 `8` |
| `resolution` | string? | `720p` / `1080p` / `4k`（大小写归一，如 `4K`→`4k`） |
| `person_generation` | string? | `allow_adult` / `allow_all` / `dont_allow` |
| `seed` | i64? | 可选 |
| `image` | string? | 首帧工作区路径 |
| `last_frame` | string? | 尾帧路径；必须同时有 `image` |
| `reference_images` | string[]? | 最多 3 条工作区图片路径 |
| `reference_image` | string? | 旧单字段；并入 `reference_images` |
| `extend_video` | string? | 本地已生成 mp4 路径（原生续拍首选） |
| `extend_video_uri` | string? | 上一代远端 URI |
| `extend_video_id` | string? | OpenAI 兼容 operation id |
| `negative_prompt` | string? | 仅兼容路径 |
| `style` | string? | 仅兼容路径 |

### 互斥与优先级

- `last_frame` 必须同时有 `image`。
- `reference_images`（含旧字段合并后）不可与 `image` / `last_frame` 同用。
- 续拍不可与首尾帧 / 参考图同用。
- 续拍优先级：`extend_video` → `extend_video_uri` → `extend_video_id`。
- `reference_images.len() > 3` → 直接失败。
- 高级模式（有续拍 / 参考图 / 尾帧 / 或 resolution 为 1080p·4k）：`duration_seconds` 强制为 `8`，若用户传了其它值则在结果中 `duration_note`。

### 成功输出

- 本地相对路径、`provider=google`、`model=…`
- `api_path=native|compat`
- 原生：`operation_name=…`；若有则 `video_uri=…`
- 兼容：`operation_id=…`
- 若原生忽略了 `negative_prompt`/`style`，附一行 `native_ignored=…`
- `next_shot_hint`：优先 `extend_video="<rel path>"`；并可选保留 operation / uri 提示

## 协议：Google 原生 Veo

### 创建

```http
POST {google_native_base}/v1beta/models/{model}:predictLongRunning
x-goog-api-key: {api_key}
Content-Type: application/json
```

（若 `google_native_base` 已含 `/v1beta`，则拼 `/models/...`；与 TTS 现有 URL 拼装规则一致。）

请求体要点：

```json
{
  "instances": [{
    "prompt": "...",
    "image": { "inlineData": { "mimeType": "image/png", "data": "<b64>" } },
    "lastFrame": { "inlineData": { ... } },
    "referenceImages": [
      { "image": { "inlineData": { ... } }, "referenceType": "asset" }
    ],
    "video": { "inlineData": { "mimeType": "video/mp4", "data": "<b64>" } }
  }],
  "parameters": {
    "aspectRatio": "9:16",
    "resolution": "720p",
    "durationSeconds": 8,
    "personGeneration": "allow_adult",
    "seed": 123
  }
}
```

- 首帧：`instances[0].image`；尾帧：`instances[0].lastFrame`（与官方 REST 示例一致）。
- 续拍本地文件：读 mp4 → base64 → `instances[0].video.inlineData`。
- 续拍 URI（无本地文件时）：带 API key 下载字节 → 再按同上 `inlineData` 提交（统一一条路径，不依赖 URI 直传形态）。
- `referenceType` 固定 `"asset"`（文档示例）。

### 轮询

```http
GET {google_native_base}/v1beta/{operation.name}
x-goog-api-key: {api_key}
```

- 间隔 10s；总超时 10 分钟（与现有兼容路径一致）。
- `done == true` 且无顶层 `error` 时，取  
  `response.generateVideoResponse.generatedSamples[0].video.uri`  
  （同时容忍 camelCase / 偶发 snake_case 字段名）。
- `done` 且带 `error` 或无 sample → 失败（可触发回退）。

### 下载

```http
GET {video.uri}
x-goog-api-key: {api_key}
```

跟随重定向；成功后返回 `GeneratedVideo { data, mime_type: "video/mp4", operation_id: operation.name }`。  
建议 `GeneratedVideo` 增加可选 `video_uri`，供工具层写入 hint。

## 协议：OpenAI 兼容回退

保留现有 `google_openai_generate_video` 行为：

- multipart：`model` / `prompt` / 可选参数 / `image` / `last_frame` / `reference_images` / `extend_video_id`
- 续拍仅认 `extend_video_id`；若用户只有本地 `extend_video` / URI 而无 id，兼容路径报明确错误（期望原生已承接；若原生也失败则合并进双失败消息）。
- 多参考图：兼容路径对每张图追加一个名为 `reference_images` 的 multipart part（最多 3）；与现有单图字段同名兼容。

## 错误处理

| 场景 | 行为 |
|------|------|
| 参数互斥 / 缺 prompt / 路径非法 | 直接失败，不请求 |
| 原生任一步失败 | 记 `native_err` + progress；尝试兼容 |
| 兼容续拍缺 id | 明确「回退需要 extend_video_id」 |
| 两者都失败 | `Google 原生视频失败: …; 兼容回退失败: …` |
| 超时 | 原生超时后回退；回退再超时则终止 |
| 安全过滤无视频 | 透出 API message；不落盘空文件 |

进度文件增加：`api_path=native|compat`；回退时 `fallback=openai_compat reason=…`。

## 测试

单测为主，不打真网：

1. 原生 create URL：从兼容 base 正确 strip 到原生根并拼 `predictLongRunning`。
2. 请求体：文生 / 首尾帧 / 多参考图 / 本地续拍的 JSON 形状。
3. 高级模式 `duration→8`；互斥组合。
4. `reference_image` → `reference_images` 合并。
5. 回退语义：原生 Err → 调兼容；原生 Ok → 不调兼容（可测薄封装或 mock）。

## 文件触点

- `providers/src/protocol/media_http.rs`：新增 `google_native_generate_video`；扩展 `VideoGenExtras` / `GeneratedVideo`；保留兼容函数。
- `tools/src/builtins/media/video_gen.rs`：参数升级、校验、native→compat、落盘与 hint。
- 工具描述字符串：改为标明 Veo 原生 + 兼容回退。

## 验收

1. 仅配置 Google key：文生视频可原生成功并落盘。
2. 原生故意不可用时（如错误 model）可回退兼容并仍落盘（若兼容可用）。
3. 首尾帧 / 最多 3 参考图 / 本地续拍在原生路径参数校验与请求体正确。
4. 现有单测通过；新增上述单测绿。
