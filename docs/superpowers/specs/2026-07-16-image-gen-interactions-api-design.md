# image_gen 升级至 Gemini Interactions API（Nano Banana）

> 日期：2026-07-16
>
> 状态：已实现；2026-09-01 补充 Azure Foundry v1 路径
>
> 参考：[Nano Banana 图片生成](https://ai.google.dev/gemini-api/docs/image-generation?hl=zh-cn)

Azure `gpt-image-2` 的独立契约见 [Azure AI Foundry `gpt-image-2` 接入设计](./2026-09-01-azure-gpt-image-2-design.md)。

## 背景

本设计提出时，`image_gen` 仅支持 `prompt` + 可选 `aspect_ratio`。Google 路径优先走 OpenAI 兼容 `POST …/images/generations`，失败再回退原生 `generateContent`（`responseModalities: IMAGE`）。

官方文档已将 **Interactions API**（`POST /v1beta/interactions`）作为 Nano Banana 图片生成的主推荐接口，并覆盖：

- 分辨率：`0.5K` / `1K` / `2K` / `4K`
- 宽高比：含 `21:9` 等完整集合
- 最多 14 张参考图（图生图 / 编辑 / 合成）
- 多轮编辑（`previous_interaction_id`）
- Google Search / Image Search 接地
- `thinking_level`（`minimal` / `high`）
- 视频转图片（YouTube URL 或本地视频）

## 目标

1. Google 出图主路径改为 Interactions API，对齐官方能力全集。
2. 扩展 `image_gen` 工具参数，风格与 `video_gen` 一致（路径相对工作区、结果含可复用 id）。
3. Agent 工具中 OpenAI / Azure / MiniMax 保持 prompt-only；通用 Provider 层和 Workflow 可传递尺寸、数量和输出格式。
4. 产物仍写入 `workspace/generated/images/`；成功返回 `interaction_id` 供多轮编辑。

## 非目标

- 独立的图片编辑 / mask UI
- Batch API 批量出图
- Imagen 专用模型路径（已弃用方向）
- 将 Thought 临时图落盘到工作区
- 改造通用 `Provider::generate_image` trait 承载全部 Interactions 能力

## 已确认决策

| 项 | 选择 |
|----|------|
| 范围 | 全做：基础出图 + 参考图 + 多轮 + Search + thinking + 视频转图 |
| 实现路径 | 工具层 Google 直连 Interactions；其他 Provider 经 `dispatch::generate_image` 按 profile 路由 |
| Interactions 失败 | 继续尝试已配置 fallback；高级参数不会在非 Google 路径上静默降级 |
| 默认模型 | 由 Provider 媒体配置和 profile 解析，不在工具内固化 |
| 交错多图 | 取 steps 中最后一张 image 落盘 |

## 架构

```
image_gen (tools)
  ├─ Google → providers::interactions_http::google_interactions_image
  │            POST {google_native_base}/v1beta/interactions
  │            Header: x-goog-api-key
  ├─ OpenAI → providers::dispatch::generate_image（仅 prompt/title）
  ├─ Azure → AzureOpenAiV1 → POST /openai/v1/images/generations
  └─ MiniMax → MiniMax T2I（仅 prompt/title）
```

最终类型位于 `crates/agent-providers/src/google/interactions_http.rs`：

- `InteractionImageRequest`：prompt、response_format 字段、参考图 bytes、video、tools、previous_id、thinking_level
- `InteractionImageResult`：`GeneratedImage`（或等价）+ `interaction_id` + 可选 `output_text` / `search_suggestions`

`image_gen` 的 Google 主路径直接使用 Interactions，不再先尝试 OpenAI-compatible 图片端点。

## 工具契约（`ImageGenArgs`）

| 参数 | 必填 | 说明 |
|------|------|------|
| `prompt` | 是 | 文本描述 |
| `aspect_ratio` | 否 | 如 `1:1` / `16:9` / `9:16` / `21:9` / `3:2`… |
| `image_size` | 否 | `0.5K` / `1K` / `2K` / `4K`（必须大写 K） |
| `reference_images` | 否 | 路径字符串数组，最多 14；工作区相对或绝对 |
| `previous_interaction_id` | 否 | 多轮编辑 |
| `google_search` | 否 | bool，启用 Search 接地 |
| `image_search` | 否 | bool，需同时 `google_search`；`search_types: ["web_search","image_search"]` |
| `thinking_level` | 否 | `minimal` / `high` |
| `video_uri` | 否 | 公开 YouTube URL |
| `video` | 否 | 本地视频工作区路径；与 `video_uri` 互斥 |

### 校验

- `image_size` 小写拒绝；`thinking_level` 非法值拒绝
- `image_search=true` 且未开 `google_search` → 错误
- `video` 与 `video_uri` 同时存在 → 错误
- 参考图 / 本地视频不可读或不在工作区可解析范围 → 错误
- 参考图超过 14 → 错误
- OpenAI / Azure / MiniMax 路径：高级字段在 HTTP 请求前拒绝，不静默忽略

### 成功返回（文本）

```
图片已生成：generated/images/img-….png
provider=google
model=gemini-3.1-flash-image
interaction_id=<id>
hint: 下次编辑可传 previous_interaction_id；可用作 video_gen 的 image/last_frame
```

若启用 Search 且响应含 `search_suggestions`，追加该 HTML 片段（满足展示要求）。

## HTTP 细节

**请求**

- URL：`{google_native_base}/v1beta/interactions`（复用 `google_native_base`，去掉 `/v1beta/openai`）
- Body：
  - `model`
  - `input`：text + 可选 image（base64 + mime）+ 可选 video（uri 或 data）
  - `response_format`：`{ type: "image", mime_type?, aspect_ratio?, image_size? }`
  - `previous_interaction_id?`
  - `tools?`：`[{ "type": "google_search", "search_types"? }]`
  - `generation_config?`：`{ "thinking_level": "…" }`

**响应**

1. 解析 `id` → `interaction_id`
2. 遍历 `steps`：仅处理 `model_output` 中的 image / text；忽略 `thought` 中的临时图
3. 落盘最后一张 image；MIME 决定扩展名
4. 可选提取 Search 相关展示字段

**错误**

- 非 2xx：`Google interactions HTTP {status}: {message}`
- 无图片块：明确可能被安全策略拦截

## 测试

- 单元：参数校验矩阵
- 单元：请求 body JSON 拼装快照/字段断言
- 单元：mock 响应解析（单图、交错多图取末张、带 id）
- 路径：`generated/images` 命名不变
- 不强制真实 API 集成测试

## 实现顺序（建议）

1. `interactions_http`：请求构建 + 响应解析 + 单测
2. 扩展 `image_gen` Args / 校验 / Google 直连接线
3. OpenAI / Azure / MiniMax 高级参数的请求前拒绝行为
4. 工具描述与注册文案更新
5. 手动冒烟：文生图 → 多轮 → 参考图 → Search（可选）

## 风险

- Interactions 响应 schema 可能有字段命名变体（snake / camel）：解析需兼容
- 本地大视频 base64 体积大：实现时限制文件大小并给出清晰错误
- Lite 模型不支持部分能力：错误原样返回，不在工具层静默降级
