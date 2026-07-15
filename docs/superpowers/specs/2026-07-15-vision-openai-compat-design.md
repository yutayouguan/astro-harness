# Vision 工具：OpenAI 兼容图片理解

日期：2026-07-15  
状态：已批准（待实现）  
参考：[Gemini OpenAI 兼容性 — 图片理解](https://ai.google.dev/gemini-api/docs/openai?hl=zh-cn#javascript_4)

## 背景

- `vision` 工具当前为 stub：仅校验路径/URL，不调用模型。
- Gemini / OpenAI 均支持 OpenAI 兼容 `chat/completions` 多模态：`content` 数组含 `text` + `image_url`（data URL 或远程 URL）。
- 聊天附件仍把图片塞进纯文本（本轮不改）。
- 媒体凭证已有 `ImageGenTargets`（Google → OpenAI）及媒体 Tab 的图/视频/TTS 模型配置。

## 目标

1. `vision` 真正看图：本地路径或 `http(s)` → OpenAI 兼容 completions → 返回描述/问答文本。
2. 凭证与 `tts` 一致：启用的 Google 优先，OpenAI 备用。
3. 媒体 Tab 可配置「视觉模型」；空 = 内置默认。

## 非目标

- 聊天附件改为真正的多模态 `content` parts（后续）
- Anthropic / 其它非 OpenAI 兼容供应商
- Google 原生 `generateContent` 视觉路径（本轮统一 OpenAI 兼容）
- 图片落盘、裁剪、批量多图（可后续扩展）

## 已确认决策

| 项 | 选择 |
|----|------|
| 凭证 | Google → OpenAI（同 `tts`） |
| 范围 | 仅 `vision` 工具 + 配置透传；不改聊天附件 |
| 图片输入 | 工作区相对路径 + `http(s)://` |
| 实现路径 | providers HTTP helper + 工具接线（方案 1） |

## 工具契约

### `vision`（替换 stub）

- 参数：`image_url`（必填）；`prompt`（可选，默认「请描述这张图片」）
- 本地路径：相对 `workspace_dir` 读文件 → `data:{mime};base64,...`（mime 由扩展名推断，缺省 `image/jpeg`）
- 远程 URL：直接作为 `image_url.url`
- 请求：

```http
POST {openai_compat_base}/chat/completions
Authorization: Bearer …
Content-Type: application/json

{
  "model": "<vision_model>",
  "messages": [{
    "role": "user",
    "content": [
      { "type": "text", "text": "<prompt>" },
      { "type": "image_url", "image_url": { "url": "<data-or-http-url>" } }
    ]
  }]
}
```

- 成功：返回助手文本（可附带 `provider` / `model` 一行元信息）
- 失败：汇总 Google / OpenAI 错误，提示配置 Key

## 默认模型

| 供应商 | 默认视觉模型 |
|--------|----------------|
| Google | `gemini-3.5-flash` |
| OpenAI | `gpt-4o` |

## 配置透传

- `ProviderConfig` / DTO / `save_provider` 增加 `vision_model`（string，空=默认）
- UI：Google / OpenAI 媒体 Tab 增加「视觉模型」输入（placeholder=默认 id）
- `ImageGenTarget` / `ImageGenCreds` 增加 `vision_model`
- `resolve_image_gen_targets` 填充默认；ChatRequest / `from_parts` 增加 primary（及必要时 fallback）透传字段
- 工具读 `creds.vision_model`，空则 `default_vision_model(provider)`

## 架构

```
resolve_image_gen_targets (+ vision_model)
  → ChatRequest / ImageGenCreds
      → vision.dispatch
          → providers::openai_vision_completions (或等价 helper)
              → 文本结果
```

## 触及模块（预期）

| 区域 | 变更 |
|------|------|
| `providers` | OpenAI 兼容看图 helper + 默认模型常量 |
| `tools/builtins/vision.rs` | 真正调用；更新 description |
| `tools/core/context.rs` + proto / backend / tauri commands | `vision_model` 透传 |
| `ProvidersPanel` + i18n | 媒体 Tab 视觉模型字段 |
| 测试 | mock HTTP：本地图 / 远程 URL / 无凭证 |

## 错误与边界

| 情况 | 行为 |
|------|------|
| 无 Google/OpenAI key | 明确错误 |
| 本地文件不存在 | 立即失败 |
| 远程 URL 不可用 | 由 API 返回错误并上浮 |
| Google 失败 | 尝试 OpenAI |
| 超大本地文件 | 允许先发送；若后续需上限再加（非本轮硬门槛） |

## 验收

- [ ] 有 Google key：对工作区图片提问得到描述
- [ ] 有 Google key：对 `https://` 图提问可用
- [ ] 仅 OpenAI：备用路径可用
- [ ] 媒体 Tab 配置视觉模型后生效；空用默认
- [ ] UI / 工具描述不再写「stub / 占位」
- [ ] 无密钥时错误清晰，不 panic

## 风险

| 风险 | 缓解 |
|------|------|
| 本地大图撑爆请求体 | 后续可加体积上限；本轮与聊天附件策略分离 |
| 默认模型 id 变更 | 常量集中；媒体 Tab 可覆盖 |
| mime 推断不准 | 扩展名映射；未知用 `image/jpeg` |
