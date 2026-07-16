# 音乐生成：Lyria 3 via Gemini Interactions（Google 原生）

日期：2026-07-16  
状态：已批准，待实现  
参考：

- [使用 Lyria 3 生成音乐](https://ai.google.dev/gemini-api/docs/music-generation)
- [Lyria RealTime](https://ai.google.dev/gemini-api/docs/realtime-music-generation?hl=zh-cn)（本轮非目标，仅对照）

## 背景

Astro 现有：

- `music`：本地播放（mpv / Spotify / Apple Music / 汽水），**不是** AI 作曲
- `tts` / `image_gen` / vision：Google 路径已走或正在走 **Interactions**（`POST …/v1beta/interactions` + `x-goog-api-key`），与 OpenAI 兼容端点分开
- **无** `music_gen` 内置工具

官方 Lyria 3 通过 Interactions API 提供：

| 模型 | Model ID | 用途 | 时长 | 默认输出 |
|------|----------|------|------|----------|
| Lyria 3 Clip | `lyria-3-clip-preview` | 短 clip / 试听 | 固定约 30s | MP3 |
| Lyria 3 Pro | `lyria-3-pro-preview` | 完整歌曲 | 约数分钟（可用 prompt 引导） | MP3；可请求 WAV |

支持文本提示、最多 10 张参考图、响应中的歌词/结构文本。与 OpenAI 兼容页无关。

## 目标

1. 新建 Agent 工具 `music_gen`：Google Lyria 3 Interactions 原生生成音乐并落盘。
2. 与 OpenAI / `…/v1beta/openai` **完全分路径**；无 Google 凭证则失败，不回退。
3. 第一版能力：`prompt` + `model`（clip/pro）+ 最多 10 张 `reference_images` + Pro 可选 `format`（mp3/wav）。
4. 产物写入 `workspace/generated/audio/`；返回路径、`interaction_id`、可选歌词。

## 非目标

- Lyria RealTime（WebSocket 实时控曲）
- 多轮编辑已生成音乐
- OpenAI 或其它厂商音乐 API
- 改造本地 `music` 播放工具
- 前端内嵌实时播放器（聊天预览可后续接 `parseGeneratedMedia`）

## 已确认决策

| 项 | 选择 |
|----|------|
| 产品 | Lyria 3（非 RealTime） |
| 工具面 | 新建 `music_gen`，与本地 `music` 分离 |
| 能力范围 | prompt + clip/pro + ≤10 参考图 + Pro `format` |
| 实现路径 | `providers::interactions_http` 增 Lyria 助手 + 新工具（方案 1） |
| 凭证 | `image_gen_targets.google()`；Google only |
| `format=wav` + clip | **硬错误**（不静默忽略） |
| 默认 model | `clip` → `lyria-3-clip-preview` |

## 架构

```
music_gen (tools)
  └─ 仅 Google key（image_gen_targets.google）
        → providers::interactions_http::google_interactions_music
              POST {google_native_base}/v1beta/interactions
              Header: x-goog-api-key
              → workspace/generated/audio/music-*.{mp3|wav}
```

要点：

- 复用现有 `interactions_url` / `google_native_base`；禁止经 `google_openai_base`。
- 请求/结果类型独立命名（`InteractionMusicRequest` / `InteractionMusicResult`），不塞进 TTS 结构体。
- 本地 `music` 不动；生成结果可提示用文件浏览器或后续用播放工具打开。
- 第一版模型 ID 用常量 + 工具参数 `model=clip|pro` 映射；暂不强制扩展 `ImageGenCreds.music_model`（可选后续）。

## 工具契约（`MusicGenArgs`）

| 参数 | 必填 | 说明 |
|------|------|------|
| `prompt` | 是 | 音乐描述（流派/乐器/BPM/情绪/结构标签等写入文案） |
| `model` | 否 | `clip`（默认）\| `pro` → `lyria-3-clip-preview` / `lyria-3-pro-preview` |
| `reference_images` | 否 | 工作区相对路径，最多 **10** 张 |
| `format` | 否 | `mp3`（默认）\| `wav`；**仅 `pro` 允许 `wav`** |

### 校验

- `prompt` 非空
- `model` 仅 `clip` / `pro` 或省略
- `reference_images` 非空路径计数 ≤ 10；文件须可读
- `format=wav` 且非 `pro` → 错误：「wav 仅 lyria-3-pro 支持」
- 无 Google 凭证 → 明确错误，不尝试 OpenAI

### 成功返回

```
音乐已生成：generated/audio/music-….mp3
provider=google
model=lyria-3-clip-preview
interaction_id=<id>
lyrics:
...
```

无歌词/结构文本则省略 `lyrics:` 段。

### 注册面

- `tools`：`music_gen.rs` + `register` / `dispatch`
- `KNOWN_TOOLSET_IDS` / `useAgentTools` / 文案 i18n（若有）
- icon：与现有图标表对齐（优先 `music`）

## HTTP 细节

### 请求

- URL：`{google_native_base}/v1beta/interactions`
- Header：`x-goog-api-key`
- Body：
  - `model`：`lyria-3-clip-preview` | `lyria-3-pro-preview`
  - `input`：无图时为 string；有图时为数组  
    `[{ "type": "text", "text": "…" }, { "type": "image", "mime_type": "…", "data": "<b64>" }, …]`
  - `response_format`：`{ "type": "audio" }`
  - `format=wav`（仅 Pro）：按官方 REST 附加字段；若文档仅有 modality，则依赖响应 `mime_type` / 文件魔数决定后缀

### 响应解析

1. 优先 `output_audio.data`（base64）
2. 否则遍历 `steps[]` 中 `type==model_output` 的 `content[]`，取 `type==audio` 最后一块
3. 歌词：`output_text`；否则拼接所有 `type==text`
4. 后缀：响应 `mime_type`（`audio/mpeg`→`.mp3`，`audio/wav`→`.wav`）→ 请求 `format` → 默认 `.mp3`
5. **不做** PCM→WAV（Lyria 输出已是封装音频，与 TTS raw PCM 不同）

### providers API

```rust
InteractionMusicRequest {
    model: String,
    prompt: String,
    images: Vec<MusicImagePart>, // mime + base64
    format: MusicAudioFormat,    // Mp3 | Wav
}
InteractionMusicResult {
    audio_bytes: Vec<u8>,
    mime_type: String,
    lyrics_text: Option<String>,
    interaction_id: String,
}
google_interactions_music(client, config, req) -> Result<InteractionMusicResult>
```

### 超时与错误

- Pro 可能较久：HTTP 超时建议 ≥ 5–10 分钟（可与视频生成同量级或独立常量）
- 非 2xx：`Google interactions music HTTP {status}: {message}`
- 无音频数据：明确报错
- 安全过滤：透传 API 错误；若响应含 `filtered_prompt` 则附带说明

## 落盘

- 目录：`generated/audio/`（`GeneratedKind::Audio`）
- 文件名：`music-{utc}-{短 id}.{mp3|wav}`

## 测试

### providers

- body：纯文本 `input` string；带图时数组（text + ≤10 image）
- model / format 映射
- 解析：`output_audio` + `output_text`；无 convenience 时从 `steps` 取
- 错误：非 2xx、无音频

### tools

- 无 Google 凭证 → 错误
- 空 prompt、`format=wav`+clip、参考图 >10 → 校验失败
- 落盘后缀与 mime/format 一致

### 手动验收

- [ ] Google key 下 `music_gen(prompt=…)` 生成 clip 并可播
- [ ] `model=pro`；`format=wav` 仅 Pro 成功
- [ ] 带 `reference_images` 可调用
- [ ] 返回含 `interaction_id`；有歌词则出现在结果中
- [ ] 工具开关出现 `music_gen`；无 key 错误清晰
- [ ] 请求不经 `…/v1beta/openai`

## 风险

| 风险 | 缓解 |
|------|------|
| Pro 耗时长 | 加长超时；错误信息可读 |
| wav 的 `response_format` 字段文档模糊 | 实现前核对官方 REST；以 mime/魔数定后缀 |
| 安全过滤拦截提示 | 透传 API 错误 / `filtered_prompt` |
| 与本地 `music` 工具名混淆 | 描述写明 generate vs play；toolset 分离 |

## 与相关设计的关系

- 复用 [TTS Interactions](./2026-07-16-tts-interactions-api-design.md) / [image-gen Interactions](./2026-07-16-image-gen-interactions-api-design.md) 的 `google_native_base`、`interactions_url`、`x-goog-api-key` 约定
- 不扩展 OpenAI TTS / 出图回退链
- RealTime 若后续需要，另开设计（WebSocket 会话生命周期）
