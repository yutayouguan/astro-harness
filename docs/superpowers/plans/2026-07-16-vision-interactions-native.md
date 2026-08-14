# Vision Interactions 原生图片理解 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `vision` 的 Google 路径改为 Gemini Interactions API 原生图片理解（describe / 多图 / detect / segment），OpenAI 仍走独立的 `chat/completions` 并尽力兜底结构化输出。

**Architecture:** 在 `providers::interactions_http` 增加视觉请求拼装/解析与 `google_interactions_vision` HTTP 调用；扩展 `media_http::openai_vision_completions` 支持多图与 mode；`tools` 层 `vision` 只做参数合并、路径解析与凭证分流。Google 不再经 `google_openai_base`。

**Tech Stack:** Rust (`providers` / `tools`)、`reqwest`、`serde_json`、`base64`、schemars（工具参数）。

**参考:** `docs/superpowers/specs/2026-07-16-vision-interactions-native-design.md`；[图片理解](https://ai.google.dev/gemini-api/docs/image-understanding?hl=zh-cn#rest_2)

## Global Constraints

- Google：`POST {google_native_base}/v1beta/interactions`，Header `x-goog-api-key`（不用 Bearer）。
- OpenAI：`POST {openai_base}/chat/completions`，Bearer；与 Google 请求体完全分离。
- 默认模型：Google `gemini-3.5-flash`；OpenAI `gpt-4o`；可读 `creds.vision_model`。
- 凭证顺序：Google → OpenAI（含聊天 OpenAI / `OPENAI_API_KEY`）。
- 工具：单一 `vision`；`mode` ∈ `describe`|`detect`|`segment`（默认 describe）；支持 `image_urls` + 兼容 `image_url`。
- detect/segment 坐标：`box_2d` = `[ymin,xmin,ymax,xmax]` 归一化 `[0,1000]`；segment 含 `mask`；Google segment 设 `thinking_level: minimal`。
- 本轮不接 Files API、不改聊天附件多模态、不改 ProvidersPanel 表单。
- 若 `interactions_http.rs` 已由 image_gen 计划落地：只追加视觉符号，不破坏出图 API。

## File Structure

| File | Responsibility |
|------|----------------|
| `crates/agent-providers/src/protocol/interactions_http.rs` | 新建或扩展：`VisionMode`、图片输入、body/parse、`google_interactions_vision` |
| `crates/agent-providers/src/protocol/mod.rs` | `pub mod interactions_http`（若尚未注册） |
| `crates/agent-providers/src/lib.rs` | re-export `interactions_http`（若尚未导出） |
| `crates/agent-providers/src/protocol/media_http.rs` | 扩展 OpenAI 视觉 helper（多图 + mode）；Google 视觉调用方停止使用 openai 路径 |
| `crates/agent-tools/src/builtin/media/vision.rs` | 新契约与分流 |
| `apps/desktop/src/i18n/messages.ts` | 中英 `agentTools.vision.desc` |

---

### Task 1: Interactions 视觉 — body 构建、响应解析、HTTP

**Files:**
- Create or Modify: `crates/agent-providers/src/protocol/interactions_http.rs`
- Modify: `crates/agent-providers/src/protocol/mod.rs`（若缺模块）
- Modify: `crates/agent-providers/src/lib.rs`（若缺 re-export）

**Interfaces:**
- Produces:
  - `pub enum VisionMode { Describe, Detect, Segment }` — `as_str()`, `FromStr` / `parse`
  - `pub enum VisionImagePart { Inline { mime_type: String, data_b64: String }, Uri { mime_type: String, uri: String } }`
  - `pub fn vision_boxes_json_schema(include_mask: bool) -> serde_json::Value`
  - `pub fn default_vision_prompt(mode: VisionMode) -> &'static str`
  - `pub fn build_interaction_vision_body(model: &str, prompt: &str, images: &[VisionImagePart], mode: VisionMode) -> serde_json::Value`
  - `pub fn parse_interaction_vision_text(v: &serde_json::Value) -> anyhow::Result<String>`
  - `pub async fn google_interactions_vision(client: &reqwest::Client, prompt: &str, images: &[VisionImagePart], mode: VisionMode, config: &ProviderConfig) -> anyhow::Result<String>`
- Consumes: `crate::media_http::google_native_base`；`ProviderConfig`

- [ ] **Step 1: 注册模块（若文件不存在）**

`crates/agent-providers/src/protocol/mod.rs` 确保有：

```rust
pub mod interactions_http;
```

`crates/agent-providers/src/lib.rs` 的 `pub use protocol::{...}` 加入 `interactions_http`。

若文件已存在（image_gen），跳过创建，只追加视觉部分。

- [ ] **Step 2: 写失败测试（`interactions_http.rs` 底部 `#[cfg(test)]`）**

```rust
#[cfg(test)]
mod vision_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn build_describe_body_inline_and_uri() {
        let images = vec![
            VisionImagePart::Inline {
                mime_type: "image/png".into(),
                data_b64: "YWJj".into(),
            },
            VisionImagePart::Uri {
                mime_type: "image/jpeg".into(),
                uri: "https://example.com/a.jpg".into(),
            },
        ];
        let body = build_interaction_vision_body(
            "gemini-3.5-flash",
            "compare",
            &images,
            VisionMode::Describe,
        );
        assert_eq!(body["model"], "gemini-3.5-flash");
        let input = body["input"].as_array().unwrap();
        assert_eq!(input[0]["type"], "text");
        assert_eq!(input[0]["text"], "compare");
        assert_eq!(input[1]["type"], "image");
        assert_eq!(input[1]["data"], "YWJj");
        assert_eq!(input[1]["mime_type"], "image/png");
        assert_eq!(input[2]["uri"], "https://example.com/a.jpg");
        assert!(body.get("response_format").is_none());
    }

    #[test]
    fn build_detect_body_has_schema_without_mask() {
        let body = build_interaction_vision_body(
            "gemini-3.5-flash",
            "detect",
            &[VisionImagePart::Uri {
                mime_type: "image/png".into(),
                uri: "https://x/y.png".into(),
            }],
            VisionMode::Detect,
        );
        let schema = &body["response_format"]["schema"];
        assert_eq!(body["response_format"]["mime_type"], "application/json");
        let props = &schema["properties"]["boxes"]["items"]["properties"];
        assert!(props.get("box_2d").is_some());
        assert!(props.get("label").is_some());
        assert!(props.get("mask").is_none());
        assert!(body.get("generation_config").is_none());
    }

    #[test]
    fn build_segment_body_has_mask_and_minimal_thinking() {
        let body = build_interaction_vision_body(
            "gemini-3.5-flash",
            "seg",
            &[VisionImagePart::Uri {
                mime_type: "image/png".into(),
                uri: "https://x/y.png".into(),
            }],
            VisionMode::Segment,
        );
        let props = &body["response_format"]["schema"]["properties"]["boxes"]["items"]["properties"];
        assert!(props.get("mask").is_some());
        assert_eq!(body["generation_config"]["thinking_level"], "minimal");
    }

    #[test]
    fn parse_prefers_output_text() {
        let v = json!({ "output_text": "caption", "steps": [] });
        assert_eq!(parse_interaction_vision_text(&v).unwrap(), "caption");
    }

    #[test]
    fn parse_falls_back_to_steps_model_output() {
        let v = json!({
            "steps": [
                {
                    "type": "model_output",
                    "content": [
                        { "type": "text", "text": "hello " },
                        { "type": "text", "text": "world" }
                    ]
                }
            ]
        });
        assert_eq!(parse_interaction_vision_text(&v).unwrap(), "hello world");
    }

    #[test]
    fn parse_errors_when_empty() {
        let v = json!({ "steps": [] });
        assert!(parse_interaction_vision_text(&v).is_err());
    }
}
```

- [ ] **Step 3: Run 测试确认失败**

Run: `cargo test -p providers vision_tests -- --nocapture`  
Expected: FAIL（模块/符号不存在，或测试未通过）

- [ ] **Step 4: 实现视觉类型与函数**

在 `interactions_http.rs` 追加（保留已有 image_gen 符号不动）：

```rust
use crate::media_http::google_native_base;
use crate::trait_::ProviderConfig;
use reqwest::Client;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisionMode {
    Describe,
    Detect,
    Segment,
}

impl VisionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Describe => "describe",
            Self::Detect => "detect",
            Self::Segment => "segment",
        }
    }

    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "describe" => Ok(Self::Describe),
            "detect" => Ok(Self::Detect),
            "segment" => Ok(Self::Segment),
            other => anyhow::bail!("无效 mode: {other}（期望 describe|detect|segment）"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum VisionImagePart {
    Inline { mime_type: String, data_b64: String },
    Uri { mime_type: String, uri: String },
}

pub fn default_vision_prompt(mode: VisionMode) -> &'static str {
    match mode {
        VisionMode::Describe => "请描述这张图片",
        VisionMode::Detect => {
            "Detect all prominent items in the image. The box_2d should be [ymin, xmin, ymax, xmax] normalized to 0-1000. Return JSON with boxes array of {box_2d, label}."
        }
        VisionMode::Segment => {
            "Give segmentation masks for the prominent items. Each entry: box_2d [ymin,xmin,ymax,xmax] 0-1000, mask as [x,y] polygon 0-1000, and label."
        }
    }
}

pub fn vision_boxes_json_schema(include_mask: bool) -> Value {
    let mut item_props = json!({
        "box_2d": { "type": "array", "items": { "type": "integer" } },
        "label": { "type": "string" }
    });
    let mut required = vec!["box_2d", "label"];
    if include_mask {
        item_props["mask"] = json!({
            "type": "array",
            "items": { "type": "array", "items": { "type": "integer" } }
        });
        required.push("mask");
    }
    json!({
        "type": "object",
        "properties": {
            "boxes": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": item_props,
                    "required": required
                }
            }
        },
        "required": ["boxes"]
    })
}

pub fn build_interaction_vision_body(
    model: &str,
    prompt: &str,
    images: &[VisionImagePart],
    mode: VisionMode,
) -> Value {
    let mut input = vec![json!({"type": "text", "text": prompt})];
    for img in images {
        match img {
            VisionImagePart::Inline { mime_type, data_b64 } => {
                input.push(json!({
                    "type": "image",
                    "data": data_b64,
                    "mime_type": mime_type
                }));
            }
            VisionImagePart::Uri { mime_type, uri } => {
                input.push(json!({
                    "type": "image",
                    "uri": uri,
                    "mime_type": mime_type
                }));
            }
        }
    }
    let mut body = json!({ "model": model, "input": input });
    match mode {
        VisionMode::Describe => {}
        VisionMode::Detect => {
            body["response_format"] = json!({
                "type": "text",
                "mime_type": "application/json",
                "schema": vision_boxes_json_schema(false)
            });
        }
        VisionMode::Segment => {
            body["response_format"] = json!({
                "type": "text",
                "mime_type": "application/json",
                "schema": vision_boxes_json_schema(true)
            });
            body["generation_config"] = json!({ "thinking_level": "minimal" });
        }
    }
    body
}

pub fn parse_interaction_vision_text(v: &Value) -> Result<String> {
    if let Some(s) = v.get("output_text").and_then(|t| t.as_str()) {
        let t = s.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    let mut parts = Vec::new();
    if let Some(steps) = v.get("steps").and_then(|s| s.as_array()) {
        for step in steps {
            if step.get("type").and_then(|t| t.as_str()) != Some("model_output") {
                continue;
            }
            if let Some(content) = step.get("content").and_then(|c| c.as_array()) {
                for item in content {
                    if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                            parts.push(t.to_string());
                        }
                    }
                }
            }
        }
    }
    let joined = parts.join("");
    if joined.trim().is_empty() {
        anyhow::bail!("Interactions 视觉响应无文本");
    }
    Ok(joined)
}

fn interactions_url(config: &ProviderConfig) -> String {
    let base = trim_slash_helper(&google_native_base(config));
    // 若本文件已有 trim_slash（image_gen），复用之；否则本地：
    // fn trim_slash(s: &str) -> String { s.trim_end_matches('/').to_string() }
    if base.contains("/v1beta") {
        format!("{base}/interactions")
    } else {
        format!("{base}/v1beta/interactions")
    }
}

pub async fn google_interactions_vision(
    client: &Client,
    prompt: &str,
    images: &[VisionImagePart],
    mode: VisionMode,
    config: &ProviderConfig,
) -> Result<String> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    if images.is_empty() {
        anyhow::bail!("vision 至少需要一张图片");
    }
    let model = if config.model.trim().is_empty() {
        crate::media_http::default_vision_model("google")
    } else {
        config.model.trim()
    };
    let url = interactions_url(config);
    let body = build_interaction_vision_body(model, prompt, images, mode);
    let response = client
        .post(&url)
        .header("x-goog-api-key", &config.api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 Google Interactions 视觉失败: {url}"))?;
    let status = response.status();
    let v: Value = response.json().await.context("解析 Interactions 视觉 JSON 失败")?;
    if !status.is_success() {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("Interactions 视觉请求失败");
        anyhow::bail!("Google Interactions HTTP {status}: {msg}");
    }
    parse_interaction_vision_text(&v)
}
```

注意：`trim_slash` / `interactions_url` 若 image_gen 已实现同类函数，合并复用，避免重复。

- [ ] **Step 5: Run 测试确认通过**

Run: `cargo test -p providers vision_tests -- --nocapture`  
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add providers/src/protocol/interactions_http.rs providers/src/protocol/mod.rs providers/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(providers): add Google Interactions vision helper

Native /v1beta/interactions path for describe/detect/segment,
separate from OpenAI-compatible chat/completions.

EOF
)"
```

---

### Task 2: 扩展 OpenAI 视觉 helper（多图 + mode）

**Files:**
- Modify: `crates/agent-providers/src/protocol/media_http.rs`

**Interfaces:**
- Consumes: `VisionMode` from `crate::interactions_http`（或本文件内镜像 enum；优先 import interactions_http）
- Produces（替换签名，更新唯一调用方 `vision.rs` 在 Task 3）：
  - `pub async fn openai_vision_completions(client: &Client, prompt: &str, image_urls: &[String], mode: VisionMode, config: &ProviderConfig) -> Result<String>`
- 行为变化：
  - **不再**因 `provider=="google"` 走 `google_openai_base`；本函数仅 OpenAI 兼容 base（默认 `https://api.openai.com/v1`）。
  - `image_urls` 全部写入 content 的 `image_url` parts。
  - `Detect`/`Segment`：body 增加 `response_format: { type: "json_object" }`（或模型支持的 json schema）；prompt 由调用方传入（已含坐标约定）。
  - 返回 content 字符串（调用方负责解析 boxes / 标记 fallback）。

- [ ] **Step 1: 写单元测试（`media_http` tests 模块）**

```rust
#[test]
fn openai_vision_body_multi_image_and_json_mode() {
    use crate::interactions_http::VisionMode;
    let urls = vec![
        "https://a/1.jpg".to_string(),
        "data:image/png;base64,AAAA".to_string(),
    ];
    let body = build_openai_vision_body("gpt-4o", "detect please", &urls, VisionMode::Detect);
    assert_eq!(body["model"], "gpt-4o");
    assert_eq!(body["response_format"]["type"], "json_object");
    let content = body["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content.len(), 3); // text + 2 images
}

#[test]
fn openai_vision_body_describe_omits_response_format() {
    use crate::interactions_http::VisionMode;
    let body = build_openai_vision_body(
        "gpt-4o",
        "hi",
        &["https://a/1.jpg".into()],
        VisionMode::Describe,
    );
    assert!(body.get("response_format").is_none());
}
```

抽出纯函数：

```rust
pub(crate) fn build_openai_vision_body(
    model: &str,
    prompt: &str,
    image_urls: &[String],
    mode: crate::interactions_http::VisionMode,
) -> Value {
    let mut content = vec![json!({"type": "text", "text": prompt})];
    for u in image_urls {
        content.push(json!({
            "type": "image_url",
            "image_url": { "url": u }
        }));
    }
    let mut body = json!({
        "model": model,
        "messages": [{ "role": "user", "content": content }]
    });
    if matches!(mode, crate::interactions_http::VisionMode::Detect | crate::interactions_http::VisionMode::Segment) {
        body["response_format"] = json!({ "type": "json_object" });
    }
    body
}
```

- [ ] **Step 2: Run 测试确认失败后实现并改签名**

将 `openai_vision_completions` 改为：

```rust
pub async fn openai_vision_completions(
    client: &Client,
    prompt: &str,
    image_urls: &[String],
    mode: crate::interactions_http::VisionMode,
    config: &ProviderConfig,
) -> Result<String> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("API Key 为空");
    }
    if image_urls.is_empty() {
        anyhow::bail!("vision 至少需要一张图片");
    }
    let model = if config.model.trim().is_empty() {
        default_vision_model("openai")
    } else {
        config.model.trim()
    };
    let raw = config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("https://api.openai.com/v1");
    let base = openai_compatible_base(raw);
    let url = format!("{base}/chat/completions");
    let body = build_openai_vision_body(model, prompt, image_urls, mode);
    // … POST bearer_auth，解析 choices[0].message.content（保留现有字符串/数组逻辑）
}
```

文档注释更新：写明「仅 OpenAI 兼容路径；Google 请用 `google_interactions_vision`」。

若编译因 `vision.rs` 旧签名失败，本任务可先在 `vision.rs` 做最小适配（单 URL 包成 slice、mode=Describe），Task 3 再完整重写。

- [ ] **Step 3: Run 测试**

Run: `cargo test -p providers openai_vision_body -- --nocapture`  
Expected: PASS  
Run: `cargo test -p providers --lib`  
Expected: PASS（或仅 vision/openai 相关失败若 tools 未适配则下一步处理）

- [ ] **Step 4: Commit**

```bash
git add providers/src/protocol/media_http.rs
git commit -m "$(cat <<'EOF'
feat(providers): OpenAI vision multi-image and JSON mode

Keep OpenAI chat/completions separate from Google Interactions;
support detect/segment json_object fallback body.

EOF
)"
```

---

### Task 3: 重写 `vision` 工具分流

**Files:**
- Modify: `crates/agent-tools/src/builtin/media/vision.rs`

**Interfaces:**
- Consumes:
  - `google_interactions_vision`, `VisionMode`, `VisionImagePart`, `default_vision_prompt`
  - `openai_vision_completions`, `default_vision_model`
- Produces: 更新后的 `VisionArgs` / `dispatch`

- [ ] **Step 1: 替换参数与 register description**

```rust
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct VisionArgs {
    /// 工作区相对路径或 http(s) URL 列表（主字段）。
    #[serde(default)]
    pub image_urls: Option<Vec<String>>,
    /// 兼容单图旧参数；有则并入 image_urls。
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    /// describe | detect | segment；缺省 describe。
    #[serde(default)]
    pub mode: Option<String>,
}
```

description（工具英文描述，供模型）：

```text
Analyze image(s). Modes: describe (default), detect (boxes JSON), segment (boxes+mask JSON). Pass image_urls (workspace paths or http(s)) or legacy image_url. Google uses Interactions API; OpenAI uses chat/completions fallback.
```

- [ ] **Step 2: 实现解析与分流**

核心逻辑（保持错误汇总风格）：

```rust
pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: VisionArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("vision 参数无效: {e}"))?;
    let mode = VisionMode::parse(parsed.mode.as_deref().unwrap_or(""))?;
    let mut urls = parsed.image_urls.unwrap_or_default();
    if let Some(one) = parsed.image_url {
        let t = one.trim();
        if !t.is_empty() {
            urls.push(t.to_string());
        }
    }
    urls.retain(|u| !u.trim().is_empty());
    if urls.is_empty() {
        anyhow::bail!("vision 需要 image_urls 或 image_url");
    }
    let prompt = parsed
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_vision_prompt(mode))
        .to_string();

    let google_parts = resolve_google_images(ctx, &urls)?;
    let openai_urls = resolve_openai_image_urls(ctx, &urls)?;

    let mut errors = Vec::new();
    if let Some(creds) = ctx.image_gen_targets.google() {
        match call_google_vision(creds, &prompt, &google_parts, mode).await {
            Ok(msg) => return Ok(msg),
            Err(e) => errors.push(format!("google: {e}")),
        }
    }

    match call_openai_vision(ctx, &prompt, &openai_urls, mode).await {
        Ok(msg) => Ok(msg),
        Err(e) => {
            errors.push(format!("openai: {e}"));
            anyhow::bail!(
                "vision 失败：{}。请配置 Google 或 OpenAI API Key。",
                errors.join("；")
            )
        }
    }
}

fn resolve_google_images(ctx: &ToolContext<'_>, urls: &[String]) -> anyhow::Result<Vec<VisionImagePart>> {
    let mut out = Vec::new();
    for u in urls {
        let u = u.trim();
        if u.starts_with("http://") || u.starts_with("https://") {
            out.push(VisionImagePart::Uri {
                mime_type: mime_from_url_or_path(u).to_string(),
                uri: u.to_string(),
            });
        } else if let Some(rest) = u.strip_prefix("data:") {
            // data:mime;base64,XXXX
            let (meta, b64) = rest.split_once(',').ok_or_else(|| anyhow::anyhow!("无效 data URL"))?;
            let mime = meta.split(';').next().unwrap_or("image/jpeg");
            out.push(VisionImagePart::Inline {
                mime_type: mime.to_string(),
                data_b64: b64.to_string(),
            });
        } else {
            let path = ctx.workspace_dir.join(u);
            if !path.exists() {
                anyhow::bail!("本地文件不存在: {}", path.display());
            }
            let bytes = std::fs::read(&path)
                .map_err(|e| anyhow::anyhow!("读取图片失败 {}: {e}", path.display()))?;
            let mime = mime_from_path(&path);
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            out.push(VisionImagePart::Inline {
                mime_type: mime.to_string(),
                data_b64: b64,
            });
        }
    }
    Ok(out)
}

fn resolve_openai_image_urls(ctx: &ToolContext<'_>, urls: &[String]) -> anyhow::Result<Vec<String>> {
    // 远程 URL 原样；本地 → data:mime;base64,...（沿用旧 mime_from_path）
    ...
}

async fn call_google_vision(
    creds: &ImageGenCreds,
    prompt: &str,
    images: &[VisionImagePart],
    mode: VisionMode,
) -> anyhow::Result<String> {
    let model = if creds.vision_model.trim().is_empty() {
        default_vision_model("google").to_string()
    } else {
        creds.vision_model.trim().to_string()
    };
    let config = ProviderConfig {
        api_key: creds.api_key.clone(),
        base_url: if creds.base_url.trim().is_empty() {
            None
        } else {
            Some(creds.base_url.clone())
        },
        model: model.clone(),
        ..ProviderConfig::default()
    };
    let client = reqwest::Client::new();
    let text = google_interactions_vision(&client, prompt, images, mode, &config).await?;
    Ok(format!(
        "{text}\nprovider=google\nmodel={model}\nmode={}",
        mode.as_str()
    ))
}

async fn call_openai_vision(
    ctx: &ToolContext<'_>,
    prompt: &str,
    image_urls: &[String],
    mode: VisionMode,
) -> anyhow::Result<String> {
    // 凭证解析同现逻辑（openai targets → chat openai → OPENAI_API_KEY）
    // 调用 openai_vision_completions(&client, prompt, image_urls, mode, &config)
    // describe：直接附加 meta
    // detect/segment：若 content 可 serde 解析为含 boxes 的 JSON，pretty 打印；否则原文 + fallback=openai
    ...
}
```

`mime_from_path` 扩展：增加 `heic`/`heif` → 对应 MIME；保留 jpeg 默认。

- [ ] **Step 3: 编译与单元级校验**

Run: `cargo test -p tools --test tools_test register_all_includes_panel_tools -- --nocapture`  
Expected: PASS  

Run: `cargo check -p tools`  
Expected: 无错误

可选：在 `vision.rs` 底部加 `#[cfg(test)]`：

```rust
#[test]
fn merges_image_url_into_list() {
    let args = serde_json::json!({"image_url": "a.png", "mode": "detect"});
    let parsed: VisionArgs = serde_json::from_value(args).unwrap();
    assert!(parsed.image_url.as_deref() == Some("a.png"));
    assert_eq!(VisionMode::parse("detect").unwrap(), VisionMode::Detect);
}
```

- [ ] **Step 4: Commit**

```bash
git add tools/src/builtin/media/vision.rs
git commit -m "$(cat <<'EOF'
feat(tools): route vision through Google Interactions API

Support mode/image_urls; keep OpenAI chat/completions as fallback
for describe/detect/segment.

EOF
)"
```

---

### Task 4: i18n 文案

**Files:**
- Modify: `apps/desktop/src/i18n/messages.ts`

- [ ] **Step 1: 更新中英描述**

```ts
"agentTools.vision.desc": "用 Google Interactions 原生接口看图（描述/检测/分割，支持多图）；OpenAI chat/completions 兜底",
// en:
"agentTools.vision.desc": "Analyze images via Google Interactions API (describe/detect/segment, multi-image); OpenAI chat/completions fallback",
```

- [ ] **Step 2: Commit**

```bash
git add apps/desktop/src/i18n/messages.ts
git commit -m "docs(i18n): update vision tool description for Interactions API"
```

---

### Task 5: 验收核对

- [ ] **Step 1: 跑相关测试**

```bash
cargo test -p providers vision_tests openai_vision_body default_vision -- --nocapture
cargo test -p tools --test tools_test -- --nocapture
cargo check -p tools -p providers -p astro-agent
```

Expected: 全部 PASS / check OK

- [ ] **Step 2: 对照 spec 验收清单（人工，有 Key 时）**

对照 `docs/superpowers/specs/2026-07-16-vision-interactions-native-design.md` 验收节：

- Google describe / 多图 / detect / segment
- 仅 OpenAI describe + detect/segment 尽力
- Google 失败落到 OpenAI
- 无密钥错误清晰

- [ ] **Step 3: 若有遗漏小修，单独 commit**（勿 amend 已推送提交）

---

## Spec coverage（自检）

| Spec 要求 | Task |
|-----------|------|
| Google Interactions 原生 | Task 1 |
| OpenAI 分离 + 多图 + JSON 兜底 | Task 2 |
| mode / image_urls / 兼容 image_url | Task 3 |
| detect/segment schema + thinking minimal | Task 1 |
| 凭证 Google→OpenAI | Task 3 |
| i18n 文案 | Task 4 |
| 不改聊天附件 / Files API / ProvidersPanel | 无任务（刻意跳过） |

## 执行提示

- 与 `2026-07-16-image-gen-interactions-api` 并行时：先落地任一方的 `interactions_http.rs` 骨架，另一方只追加符号。
- `openai_vision_completions` 改签名后务必全局搜调用点（目前仅 `vision.rs`）。
