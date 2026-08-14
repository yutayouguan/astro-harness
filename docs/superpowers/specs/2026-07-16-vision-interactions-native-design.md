# Vision 工具：Google Interactions 原生图片理解 + OpenAI 分离

日期：2026-07-16  
状态：已批准设计  
替代：[2026-07-15-vision-openai-compat-design.md](./2026-07-15-vision-openai-compat-design.md) 中「Google 统一走 OpenAI 兼容」路径  
参考：[Gemini 图片理解（Interactions）](https://ai.google.dev/gemini-api/docs/image-understanding?hl=zh-cn#rest_2)

## 背景

- `vision` 当前经 `openai_vision_completions`：Google / OpenAI 都走 `chat/completions`。
- Gemini 正式推荐 Interactions API 做多模态图片理解（说明、多图、检测、分割）。
- 仓库出图侧已有 / 在推进 Interactions；视觉应对齐：**Google 原生、OpenAI 独立**。

## 目标

1. Google：直接 `POST …/v1beta/interactions`，不再走 `…/v1beta/openai/chat/completions`。
2. OpenAI：继续 `chat/completions`，与 Google 请求体完全分开。
3. 单一 `vision` 工具：`mode` = `describe` | `detect` | `segment`；支持多图。
4. `detect` / `segment`：Google 用 `response_format` + JSON schema；OpenAI 尽力 JSON 兜底。

## 非目标

- 聊天附件改为真多模态 parts
- Files API 上传（本轮本地 → inline base64；过大失败上浮）
- 检测框 / 分割 mask 的前端可视化
- Anthropic 等其它供应商
- 改 ProvidersPanel 表单结构（`vision_model` 字段沿用）

## 已确认决策

| 项 | 选择 |
|----|------|
| Google API | Interactions API（非 generateContent） |
| 范围 | describe + 多图 + detect + segment |
| 工具形态 | 单一 `vision` + `mode` |
| OpenAI 高级 mode | 尽力兜底（prompt + JSON，不保证坐标精度） |
| 实现路径 | 双 HTTP helper + 工具分流（方案 1） |
| 凭证顺序 | Google → OpenAI（同现网） |

## 工具契约

### 参数

| 字段 | 类型 | 说明 |
|------|------|------|
| `image_urls` | `string[]` | 主字段：工作区相对路径或 `http(s)`；至少 1 张 |
| `image_url` | `string` | 兼容旧调用；有则并入 `image_urls` |
| `prompt` | `string?` | 缺省：describe→「请描述这张图片」；detect/segment→内置指令（可被覆盖） |
| `mode` | enum | `describe`（默认）\| `detect` \| `segment` |

### 模式语义

- **describe**：自然语言；多图一并理解。
- **detect**：结构化 JSON：`boxes[].{box_2d, label}`；`box_2d` = `[ymin,xmin,ymax,xmax]`，归一化 `[0,1000]`。
- **segment**：同上并含 `mask`（`[x,y]` 多边形，0–1000）；Google 侧设 `generation_config.thinking_level = "minimal"`。

### 成功输出

- describe：助手文本；末尾可附 `provider=` / `model=` / `mode=`。
- detect/segment：以 JSON 正文为主（`boxes`），再附元信息行；OpenAI 解析失败时返回原文并标 `fallback=openai`。

## 协议

### Google Interactions

```http
POST {google_native_base}/v1beta/interactions
x-goog-api-key: …
Content-Type: application/json
```

请求要点：

- `model`：`vision_model` 或默认 `gemini-3.5-flash`
- `input`：`[{type:"text",text}, {type:"image", data|uri, mime_type}, …]`
  - 本地文件 / `data:` URL → `data`（纯 base64，无 data-URL 前缀）+ `mime_type`
  - `http(s)` → `uri` + `mime_type`（扩展名推断；未知 `image/jpeg`）
- detect/segment：`response_format` 带 JSON schema（`boxes`）；segment 另加 `thinking_level: minimal`
- 解析：优先 `output_text`；否则从 `steps[]` 中 `model_output` 的 text parts 拼接
- Base：`google_native_base`（剥掉 `/v1beta/openai`），再拼 `/v1beta/interactions`

### OpenAI chat/completions

```http
POST {openai_compat_base}/chat/completions
Authorization: Bearer …
```

- `content`：一条 `text` + 多条 `image_url`（data URL 或远程 URL）
- describe：现逻辑
- detect/segment：JSON mode / `response_format`（若模型支持）+ 同坐标约定 prompt；解析 `boxes`，失败则原文 + `fallback=openai`

### 共享解析（工具层）

本地相对路径：`workspace_dir` 读字节 → mime + base64。  
mime 映射：png/jpeg/webp/heic/heif（及现有 jpg/gif/bmp）；未知 `image/jpeg`。

## 架构

```
resolve_image_gen_targets (+ vision_model)
  → vision.dispatch
       → google_interactions_vision   // providers::interactions_http（与 image_gen 共用模块）
       → openai_vision_completions    // media_http：扩展多图 + 结构化兜底
```

Google 视觉路径**禁止**再调用 `google_openai_base` / openai compat completions。  
与 [image-gen Interactions 设计](./2026-07-16-image-gen-interactions-api-design.md) 共用 `google_native_base`、`x-goog-api-key` 与（若已落地）`interactions_http` 模块；视觉请求/结果类型独立命名，不塞进出图结构体。

## 触及模块

| 区域 | 变更 |
|------|------|
| `providers/.../interactions_http.rs`（新建或复用） | `google_interactions_vision`：Interactions 看图 / 检测 / 分割 |
| `providers/.../media_http.rs` | 扩展 `openai_vision_completions`（多图 + detect/segment JSON）；Google 视觉不再经此 openai 路径 |
| `tools/.../vision.rs` | 新参数、mode 分流、多图解析 |
| `apps/desktop/.../messages.ts`（及对应 i18n） | 更新 `agentTools.vision.desc` |
| 测试 | mock HTTP：describe / 多图 / detect schema / Google 失败→OpenAI / 无凭证 |

## 错误与边界

| 情况 | 行为 |
|------|------|
| 无图（urls 空） | 参数错误 |
| 本地文件不存在 | 立即失败 |
| Google 失败 | 记入 errors，尝试 OpenAI |
| OpenAI 结构化解析失败 | 原文 + `fallback=openai` |
| 两边都失败 | 汇总错误，提示配置 Key |
| 超大 inline | 由 API 报错上浮（本轮不加硬上限） |

## 默认模型

| 供应商 | 默认 |
|--------|------|
| Google | `gemini-3.5-flash` |
| OpenAI | `gpt-4o` |

## 验收

- [ ] Google：单图 describe 可用
- [ ] Google：多图可用
- [ ] Google：detect / segment 返回合法 `boxes` JSON
- [ ] 仅 OpenAI：describe 可用；detect/segment 尽力 JSON
- [ ] Google 失败可落到 OpenAI
- [ ] 工具文案体现 Interactions 原生 + OpenAI 分离
- [ ] 无密钥时错误清晰，不 panic

## 风险

| 风险 | 缓解 |
|------|------|
| Interactions 响应字段形态差异（`output_text` vs `steps`） | 解析器兼容两条路径；单测覆盖 |
| OpenAI detect/segment 坐标不准 | 文档与工具描述标明「尽力」；输出带 `fallback=openai` |
| 本地大图撑爆请求体 | 与旧版一致先发送；后续可加体积上限或 Files API |
| 与 image-gen Interactions 实现重复 | 同模块复用 base/鉴权；视觉用独立 request/result 类型 |
