# Vision OpenAI 兼容工具 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 stub `vision` 工具替换为 Google/OpenAI 兼容 `chat/completions` 多模态看图，并支持媒体 Tab 配置 `vision_model`。

**Architecture:** 在 `providers::media_http` 增加 `openai_vision_completions` helper；`vision` 工具读取本地/远程图，Google→OpenAI 凭证与 `tts` 相同；`vision_model` 沿现有 `image_gen_*` 透传链进入 `ImageGenCreds`。

**Tech Stack:** Rust (`providers`/`tools`/`proto`/`backend`/`astro-agent`)、Tauri `ProvidersPanel`、i18n。

**参考:** `docs/superpowers/specs/2026-07-15-vision-openai-compat-design.md`；[Gemini OpenAI 图片理解](https://ai.google.dev/gemini-api/docs/openai?hl=zh-cn#javascript_4)

## Global Constraints

- 凭证：启用 Google 优先，OpenAI 备用（同 `tts`）。
- 默认模型：Google `gemini-3.5-flash`；OpenAI `gpt-4o`。
- 图片输入：工作区相对路径 + `http(s)://`；不改聊天附件多模态。
- 本轮不接 Anthropic / 原生 `generateContent` 视觉路径。

## File Structure

| File | Responsibility |
|------|----------------|
| `providers/src/protocol/media_http.rs` | `default_vision_model` + `openai_vision_completions` |
| `providers/tests/provider_test.rs` | 默认模型断言 |
| `tools/src/builtins/vision.rs` | 真正看图 dispatch |
| `tools/src/core/context.rs` | `ImageGenCreds.vision_model` + `from_parts` |
| `proto/proto/astro.proto` | `image_gen_vision_model` / fallback |
| `backend/.../astro_service.rs` + `commands.rs` | 透传字段 |
| `providers_commands.rs` + DTO/config | persist `vision_model` |
| `ProvidersPanel.tsx` / `types.ts` / `messages.ts` | 媒体 Tab UI |

---

### Task 1: providers 视觉 HTTP helper + 默认模型

**Files:**
- Modify: `providers/src/protocol/media_http.rs`
- Modify: `providers/tests/provider_test.rs`

**Interfaces:**
- Produces:
  - `pub fn default_vision_model(provider: &str) -> &'static str`
  - `pub async fn openai_vision_completions(client: &Client, prompt: &str, image_url: &str, config: &ProviderConfig) -> Result<String>`

- [ ] **Step 1: 在 `media_http.rs` 增加默认与调用**

在 `default_tts_model` 附近：

```rust
pub fn default_vision_model(provider: &str) -> &'static str {
    match provider {
        "google" => "gemini-3.5-flash",
        "openai" => "gpt-4o",
        _ => "gpt-4o",
    }
}
```

Helper（Google 用 `google_openai_base`；其它用 `openai_compatible_base` 或默认 `https://api.openai.com/v1`）：

```rust
pub async fn openai_vision_completions(
    client: &Client,
    prompt: &str,
    image_url: &str,
    config: &ProviderConfig,
) -> Result<String> {
    // POST {base}/chat/completions
    // body: model, messages: [{ role: user, content: [text, image_url] }]
    // 返回 choices[0].message.content 字符串
}
```

Google endpoint：`{google_openai_base(config)}/chat/completions`。  
OpenAI：`{openai_compatible_base(base)}/chat/completions`。

- [ ] **Step 2: 测试默认模型**

```rust
assert_eq!(default_vision_model("google"), "gemini-3.5-flash");
assert_eq!(default_vision_model("openai"), "gpt-4o");
```

Run: `cargo test -p providers test_default -- --nocapture`（或对应测试名）  
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git commit -m "feat(providers): add OpenAI-compat vision completions helper"
```

---

### Task 2: `vision_model` 配置透传

**Files:**
- Modify: `tools/src/core/context.rs` — `ImageGenCreds.vision_model`；`from_parts` 增加 `vision_model` / `fb_vision_model`
- Modify: `proto/proto/astro.proto` — `image_gen_vision_model = 24`；`image_gen_fallback_vision_model = 25`
- Modify: `backend/src/grpc/astro_service.rs` — `from_parts` 传新字段
- Modify: `frontend/src-tauri/src/commands.rs` — ChatRequest 填字段
- Modify: `frontend/src-tauri/src/providers_commands.rs` — ProviderConfig/DTO/Input/`resolve_image_gen_targets`/`to_dto`/`save`
- Modify: `frontend/src/types.ts` — `vision_model?: string`

**Interfaces:**
- Produces: `ImageGenCreds { ..., vision_model: String }`；resolve 后 Google/OpenAI target 带默认或配置值

- [ ] **Step 1: 扩展 Creds + from_parts**

`from_parts` 签名追加 `vision_model: &str, fb_vision_model: &str`，primary/fallback 均写入 `vision_model`（fallback 用 `fb_vision_model`）。

- [ ] **Step 2: proto + 调用点对齐**

重建后修全部 `ChatRequest { ... }` / `from_parts(` 编译错误。

`resolve`：

```rust
vision_model: resolve_media_model(
    &p.vision_model,
    default_vision_model_for_kind(&p.kind), // google→gemini-3.5-flash, openai→gpt-4o
),
```

- [ ] **Step 3: `cargo check -p tools -p backend -p astro-agent`**  
Expected: 无错误

- [ ] **Step 4: Commit**

```bash
git commit -m "feat(providers): persist and wire vision_model through chat targets"
```

---

### Task 3: 实现 `vision` 工具

**Files:**
- Modify: `tools/src/builtins/vision.rs`
- Modify: `frontend/src/hooks/useAgentTools.ts`（若描述硬编码）
- Modify: `frontend/src/i18n/messages.ts` — `agentTools.vision.desc`

**Interfaces:**
- Consumes: `openai_vision_completions`；`ctx.image_gen_targets.google()/openai()`；`creds.vision_model`
- Produces: 文本结果 `…\nprovider=…\nmodel=…`

- [ ] **Step 1: 重写 `vision.rs`**

逻辑要点：

1. 解析 `image_url` / `prompt`（默认「请描述这张图片」）。
2. 若 `http(s):` → 直接用；否则 `workspace_dir.join`，`exists` 校验，读 bytes + base64，mime 由扩展名（`.png`→`image/png`，`.webp`→`image/webp`，`.gif`→`image/gif`，否则 `image/jpeg`）。
3. Google creds 存在则调 helper（`ProviderConfig` 填 key/base_url/model=`vision_model` 或 default）。
4. 失败再试 OpenAI（targets → 聊天 openai → `OPENAI_API_KEY`，与 `tts` 备用策略对齐时可简化为 targets.openai() + env）。
5. 更新 `ToolEntry.description`：说明真正看图，Google/OpenAI 兼容 completions。

示例成功返回：

```text
<assistant text>
provider=google
model=gemini-3.5-flash
```

- [ ] **Step 2: 无凭证单元路径**（可选小型测试或手动）

若有 `tools` 测试夹具：设空 targets，dispatch 期望错误含「API Key」。

- [ ] **Step 3: Commit**

```bash
git commit -m "feat(tools): implement vision via OpenAI-compat chat completions"
```

---

### Task 4: 媒体 Tab「视觉模型」UI + i18n

**Files:**
- Modify: `frontend/src/components/ProvidersPanel.tsx` — Draft/`MEDIA_MODEL_DEFAULTS`/媒体表单
- Modify: `frontend/src/i18n/messages.ts` — `providers.visionModel`
- Modify: `frontend/src/types.ts`（若 Task 2 未做完）

- [ ] **Step 1: Draft + defaults**

```ts
google: { ..., vision: "gemini-3.5-flash" },
openai: { ..., vision: "gpt-4o" },
```

字段 `vision_model`：Google 与 OpenAI 媒体 Tab 均显示（视频仍仅 Google）。

- [ ] **Step 2: 更新 `agentTools.vision.desc`**

中/英去掉「占位 / stub」，改为说明 OpenAI 兼容看图。

- [ ] **Step 3: `npx tsc -b`**  
Expected: 无错误

- [ ] **Step 4: Commit**

```bash
git commit -m "feat(frontend): add vision model field on provider Media tab"
```

---

### Task 5: 规格状态与验收核对

**Files:**
- Modify: `docs/superpowers/specs/2026-07-15-vision-openai-compat-design.md` — 状态改为已实现

- [ ] **Step 1: 勾验收清单**（有 key 时手动；无 key 时至少 `cargo check` + tsc）
- [ ] **Step 2: Commit**

```bash
git commit -m "docs: mark vision OpenAI-compat design as implemented"
```

---

## Spec coverage

| Spec 项 | Task |
|---------|------|
| OpenAI 兼容 completions | 1, 3 |
| Google→OpenAI 凭证 | 3 |
| 路径 + http(s) | 3 |
| `vision_model` 配置 | 2, 4 |
| 默认 gemini-3.5-flash / gpt-4o | 1, 2 |
| 不改聊天附件 | （刻意不做） |
| UI 文案非 stub | 4 |
