# image_gen Interactions API Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Google `image_gen` 升级到 Gemini Interactions API（Nano Banana 全能力：分辨率、参考图、多轮、Search、thinking、视频转图），OpenAI 路径保持现有 prompt 出图。

**Architecture:** 新增 `providers::interactions_http`（请求拼装 + 响应解析 + HTTP 调用）；`tools` 层 `image_gen` 对 Google 直连该 helper（同 `video_gen` 模式），对 OpenAI 仍走 `Provider::generate_image` 并忽略高级参数。不改 `Provider::generate_image` trait。

**Tech Stack:** Rust (`providers` / `tools`)、`reqwest`、`serde_json`、`base64`、workspace 路径解析（对齐 `video_gen`）。

**参考:** `docs/superpowers/specs/2026-07-16-image-gen-interactions-api-design.md`；[图片生成文档](https://ai.google.dev/gemini-api/docs/image-generation?hl=zh-cn)

## Global Constraints

- Google 主路径：`POST {google_native_base}/v1beta/interactions`，Header `x-goog-api-key`。
- 默认模型：`gemini-3.1-flash-image`。
- Interactions 失败：**不**回退 OpenAI 兼容 `/images/generations`。
- `image_size` 仅 `0.5K`/`1K`/`2K`/`4K`（大写 K）；`thinking_level` 仅 `minimal`/`high`。
- 参考图最多 14；本地视频最大 **20 MiB**（超限清晰报错）。
- 落盘目录仍为 `generated/images/`；忽略 `thought` 临时图。
- 交错多图：取 `model_output` 中**最后一张** image。
- OpenAI：只使用 `prompt`；若传入高级参数则在结果追加 `note:` 一行。

## File Structure

| File | Responsibility |
|------|----------------|
| `crates/agent-providers/src/protocol/interactions_http.rs` | 新建：请求/响应类型、body 构建、解析、HTTP 调用 |
| `crates/agent-providers/src/protocol/mod.rs` | `pub mod interactions_http` |
| `crates/agent-providers/src/lib.rs` | re-export `interactions_http` |
| `crates/agent-tools/src/builtin/media/image_gen.rs` | Args 扩展、校验、Google/OpenAI 分支、落盘与返回文案 |

---

### Task 1: Interactions 纯函数 — body 构建与响应解析

**Files:**
- Create: `crates/agent-providers/src/protocol/interactions_http.rs`
- Modify: `crates/agent-providers/src/protocol/mod.rs`
- Modify: `crates/agent-providers/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub struct InteractionImagePart { pub data: Vec<u8>, pub mime_type: String }`
  - `pub struct InteractionVideoInput { /* Uri(String) | Bytes { data, mime_type } */ }`
  - `pub struct InteractionImageRequest { prompt, aspect_ratio, image_size, mime_type, reference_images, previous_interaction_id, google_search, image_search, thinking_level, video }`
  - `pub struct InteractionImageResult { pub image: GeneratedImage, pub interaction_id: String, pub output_text: Option<String>, pub search_suggestions: Option<String> }`
  - `pub fn build_interaction_image_body(model: &str, req: &InteractionImageRequest) -> serde_json::Value`
  - `pub fn parse_interaction_image_response(v: &serde_json::Value) -> anyhow::Result<InteractionImageResult>`

- [x] **Step 1: 写失败测试（同文件 `#[cfg(test)]`）**

在新建文件底部加入（此时函数尚未实现，或先 stub 再补测试）：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn build_body_includes_aspect_size_and_search() {
        let req = InteractionImageRequest {
            prompt: "a cat".into(),
            aspect_ratio: Some("16:9".into()),
            image_size: Some("2K".into()),
            mime_type: None,
            reference_images: vec![],
            previous_interaction_id: None,
            google_search: true,
            image_search: true,
            thinking_level: Some("high".into()),
            video: None,
        };
        let body = build_interaction_image_body("gemini-3.1-flash-image", &req);
        assert_eq!(body["model"], "gemini-3.1-flash-image");
        assert_eq!(body["response_format"]["type"], "image");
        assert_eq!(body["response_format"]["aspect_ratio"], "16:9");
        assert_eq!(body["response_format"]["image_size"], "2K");
        assert_eq!(body["generation_config"]["thinking_level"], "high");
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools[0]["type"], "google_search");
        let st = tools[0]["search_types"].as_array().unwrap();
        assert!(st.iter().any(|x| x == "web_search"));
        assert!(st.iter().any(|x| x == "image_search"));
        let input = body["input"].as_array().unwrap();
        assert_eq!(input[0]["type"], "text");
        assert_eq!(input[0]["text"], "a cat");
    }

    #[test]
    fn parse_takes_last_model_output_image_ignores_thought() {
        let v = json!({
            "id": "ix-123",
            "steps": [
                {
                    "type": "thought",
                    "summary": [{ "type": "image", "data": "AAAA", "mime_type": "image/png" }]
                },
                {
                    "type": "model_output",
                    "content": [
                        { "type": "text", "text": "hello" },
                        { "type": "image", "data": "Zmlyc3Q=", "mime_type": "image/png" },
                        { "type": "image", "data": "c2Vjb25k", "mime_type": "image/jpeg" }
                    ]
                },
                {
                    "type": "google_search_result",
                    "search_suggestions": "<div>suggest</div>"
                }
            ]
        });
        let r = parse_interaction_image_response(&v).unwrap();
        assert_eq!(r.interaction_id, "ix-123");
        assert_eq!(r.image.mime_type, "image/jpeg");
        assert_eq!(r.image.data, b"second");
        assert_eq!(r.output_text.as_deref(), Some("hello"));
        assert_eq!(r.search_suggestions.as_deref(), Some("<div>suggest</div>"));
    }

    #[test]
    fn parse_errors_when_no_image() {
        let v = json!({ "id": "ix", "steps": [{ "type": "model_output", "content": [{ "type": "text", "text": "x" }] }] });
        assert!(parse_interaction_image_response(&v).is_err());
    }
}
```

- [x] **Step 2: 注册模块**

`crates/agent-providers/src/protocol/mod.rs` 增加：

```rust
pub mod interactions_http;
```

`crates/agent-providers/src/lib.rs` 的 `pub use protocol::{...}` 加入 `interactions_http`。

- [x] **Step 3: 实现类型与函数**

```rust
//! Gemini Interactions API 出图（Nano Banana）。

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use serde_json::{json, Value};

use crate::trait_::GeneratedImage;

pub struct InteractionImagePart {
    pub data: Vec<u8>,
    pub mime_type: String,
}

pub enum InteractionVideoInput {
    Uri { uri: String, mime_type: String },
    Bytes { data: Vec<u8>, mime_type: String },
}

#[derive(Default)]
pub struct InteractionImageRequest {
    pub prompt: String,
    pub aspect_ratio: Option<String>,
    pub image_size: Option<String>,
    pub mime_type: Option<String>,
    pub reference_images: Vec<InteractionImagePart>,
    pub previous_interaction_id: Option<String>,
    pub google_search: bool,
    pub image_search: bool,
    pub thinking_level: Option<String>,
    pub video: Option<InteractionVideoInput>,
}

pub struct InteractionImageResult {
    pub image: GeneratedImage,
    pub interaction_id: String,
    pub output_text: Option<String>,
    pub search_suggestions: Option<String>,
}

pub fn build_interaction_image_body(model: &str, req: &InteractionImageRequest) -> Value {
    let mut input = vec![json!({ "type": "text", "text": req.prompt })];
    for img in &req.reference_images {
        input.push(json!({
            "type": "image",
            "data": base64::engine::general_purpose::STANDARD.encode(&img.data),
            "mime_type": img.mime_type,
        }));
    }
    if let Some(v) = &req.video {
        match v {
            InteractionVideoInput::Uri { uri, mime_type } => {
                input.push(json!({ "type": "video", "uri": uri, "mime_type": mime_type }));
            }
            InteractionVideoInput::Bytes { data, mime_type } => {
                input.push(json!({
                    "type": "video",
                    "data": base64::engine::general_purpose::STANDARD.encode(data),
                    "mime_type": mime_type,
                }));
            }
        }
    }

    let mut response_format = json!({ "type": "image" });
    if let Some(ar) = req.aspect_ratio.as_deref().filter(|s| !s.is_empty()) {
        response_format["aspect_ratio"] = json!(ar);
    }
    if let Some(sz) = req.image_size.as_deref().filter(|s| !s.is_empty()) {
        response_format["image_size"] = json!(sz);
    }
    if let Some(mt) = req.mime_type.as_deref().filter(|s| !s.is_empty()) {
        response_format["mime_type"] = json!(mt);
    }

    let mut body = json!({
        "model": model,
        "input": input,
        "response_format": response_format,
    });
    if let Some(id) = req.previous_interaction_id.as_deref().filter(|s| !s.is_empty()) {
        body["previous_interaction_id"] = json!(id);
    }
    if req.google_search {
        let mut tool = json!({ "type": "google_search" });
        if req.image_search {
            tool["search_types"] = json!(["web_search", "image_search"]);
        }
        body["tools"] = json!([tool]);
    }
    if let Some(level) = req.thinking_level.as_deref().filter(|s| !s.is_empty()) {
        body["generation_config"] = json!({ "thinking_level": level });
    }
    body
}

fn step_type(step: &Value) -> &str {
    step.get("type").and_then(|t| t.as_str()).unwrap_or("")
}

fn block_type(block: &Value) -> &str {
    block.get("type").and_then(|t| t.as_str()).unwrap_or("")
}

pub fn parse_interaction_image_response(v: &Value) -> Result<InteractionImageResult> {
    let interaction_id = v
        .get("id")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("Interactions 响应缺少 id"))?
        .to_string();

    let steps = v.get("steps").and_then(|s| s.as_array()).map(|a| a.as_slice()).unwrap_or(&[]);

    let mut texts: Vec<String> = Vec::new();
    let mut last_image: Option<GeneratedImage> = None;
    let mut search_suggestions: Option<String> = None;

    for step in steps {
        let ty = step_type(step);
        if ty == "google_search_result" {
            if let Some(s) = step
                .get("search_suggestions")
                .and_then(|x| x.as_str())
                .filter(|s| !s.is_empty())
            {
                search_suggestions = Some(s.to_string());
            }
            continue;
        }
        if ty != "model_output" {
            continue; // 跳过 thought 等
        }
        let content = step
            .get("content")
            .and_then(|c| c.as_array())
            .map(|a| a.as_slice())
            .unwrap_or(&[]);
        for block in content {
            match block_type(block) {
                "text" => {
                    if let Some(t) = block.get("text").and_then(|x| x.as_str()) {
                        if !t.is_empty() {
                            texts.push(t.to_string());
                        }
                    }
                }
                "image" => {
                    let b64 = block
                        .get("data")
                        .and_then(|d| d.as_str())
                        .ok_or_else(|| anyhow!("image block 缺少 data"))?;
                    let mime = block
                        .get("mime_type")
                        .or_else(|| block.get("mimeType"))
                        .and_then(|m| m.as_str())
                        .unwrap_or("image/png")
                        .to_string();
                    let data = base64::engine::general_purpose::STANDARD
                        .decode(b64)
                        .context("解码 Interactions 图片 base64 失败")?;
                    last_image = Some(GeneratedImage {
                        data,
                        mime_type: mime,
                    });
                }
                _ => {}
            }
        }
    }

    let image = last_image.ok_or_else(|| {
        anyhow!("未返回图片数据（可能被安全策略拦截）")
    })?;
    Ok(InteractionImageResult {
        image,
        interaction_id,
        output_text: if texts.is_empty() {
            None
        } else {
            Some(texts.join("\n"))
        },
        search_suggestions,
    })
}
```

- [x] **Step 4: 跑测试**

Run:

```bash
cargo test -p providers interactions_http -- --nocapture
```

Expected: PASS（3 tests）

- [x] **Step 5: Commit**

```bash
git add providers/src/protocol/interactions_http.rs providers/src/protocol/mod.rs providers/src/lib.rs
git commit -m "feat(providers): add Gemini Interactions image body/parse helpers"
```

---

### Task 2: HTTP 调用 `google_interactions_image`

**Files:**
- Modify: `crates/agent-providers/src/protocol/interactions_http.rs`

**Interfaces:**
- Consumes: `build_interaction_image_body`, `parse_interaction_image_response`, `media_http::google_native_base`
- Produces:
  - `pub async fn google_interactions_image(client: &reqwest::Client, config: &ProviderConfig, req: &InteractionImageRequest) -> Result<InteractionImageResult>`

- [x] **Step 1: 实现 HTTP 函数**

```rust
use reqwest::Client;
use crate::media_http::google_native_base;
use crate::trait_::ProviderConfig;

pub async fn google_interactions_image(
    client: &Client,
    config: &ProviderConfig,
    req: &InteractionImageRequest,
) -> Result<InteractionImageResult> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    let model = if config.model.trim().is_empty() {
        "gemini-3.1-flash-image"
    } else {
        config.model.trim()
    };
    let base = google_native_base(config);
    let url = if base.contains("/v1beta") {
        format!("{base}/interactions")
    } else {
        format!("{base}/v1beta/interactions")
    };
    let body = build_interaction_image_body(model, req);

    let response = client
        .post(&url)
        .header("x-goog-api-key", &config.api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 Google Interactions API 失败: {url}"))?;

    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析 Google Interactions 响应失败")?;
    if !status.is_success() {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("Google Interactions 出图失败");
        anyhow::bail!("Google interactions HTTP {status}: {msg}");
    }
    parse_interaction_image_response(&v)
}
```

注意：`google_native_base` 在 `media_http` 已是 `pub`；若循环依赖（`media_http` 不引用 `interactions_http`，则 OK）。不要在 `media_http` import `interactions_http`。

- [x] **Step 2: 编译检查**

Run:

```bash
cargo check -p providers
```

Expected: 成功

- [x] **Step 3: Commit**

```bash
git add providers/src/protocol/interactions_http.rs
git commit -m "feat(providers): call Gemini /v1beta/interactions for image gen"
```

---

### Task 3: 扩展 `image_gen` 参数校验（可测纯函数）

**Files:**
- Modify: `crates/agent-tools/src/builtin/media/image_gen.rs`

**Interfaces:**
- Produces:
  - 扩展后的 `ImageGenArgs`
  - `fn validate_image_gen_args(args: &ImageGenArgs) -> anyhow::Result<()>`（或内联于 dispatch 前的清晰校验块）
  - 单元测试：非法 `image_size` / `thinking_level` / `image_search` 缺 `google_search` / `video`+`video_uri`

- [x] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod arg_tests {
    use super::*;

    #[test]
    fn rejects_lowercase_image_size() {
        let a = ImageGenArgs {
            prompt: "x".into(),
            aspect_ratio: None,
            image_size: Some("1k".into()),
            reference_images: None,
            previous_interaction_id: None,
            google_search: false,
            image_search: false,
            thinking_level: None,
            video_uri: None,
            video: None,
        };
        assert!(validate_image_gen_args(&a).is_err());
    }

    #[test]
    fn rejects_image_search_without_google_search() {
        let a = ImageGenArgs {
            prompt: "x".into(),
            aspect_ratio: None,
            image_size: None,
            reference_images: None,
            previous_interaction_id: None,
            google_search: false,
            image_search: true,
            thinking_level: None,
            video_uri: None,
            video: None,
        };
        assert!(validate_image_gen_args(&a).is_err());
    }

    #[test]
    fn rejects_both_video_inputs() {
        let a = ImageGenArgs {
            prompt: "x".into(),
            aspect_ratio: None,
            image_size: None,
            reference_images: None,
            previous_interaction_id: None,
            google_search: false,
            image_search: false,
            thinking_level: None,
            video_uri: Some("https://www.youtube.com/watch?v=x".into()),
            video: Some("generated/videos/a.mp4".into()),
        };
        assert!(validate_image_gen_args(&a).is_err());
    }

    #[test]
    fn accepts_valid_size_and_thinking() {
        let a = ImageGenArgs {
            prompt: "x".into(),
            aspect_ratio: Some("1:1".into()),
            image_size: Some("1K".into()),
            reference_images: None,
            previous_interaction_id: None,
            google_search: true,
            image_search: true,
            thinking_level: Some("minimal".into()),
            video_uri: None,
            video: None,
        };
        assert!(validate_image_gen_args(&a).is_ok());
    }
}
```

- [x] **Step 2: 扩展 Args + 实现 validate**

```rust
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ImageGenArgs {
    pub prompt: String,
    #[serde(default)]
    pub aspect_ratio: Option<String>,
    #[serde(default)]
    pub image_size: Option<String>,
    #[serde(default)]
    pub reference_images: Option<Vec<String>>,
    #[serde(default)]
    pub previous_interaction_id: Option<String>,
    #[serde(default)]
    pub google_search: bool,
    #[serde(default)]
    pub image_search: bool,
    #[serde(default)]
    pub thinking_level: Option<String>,
    #[serde(default)]
    pub video_uri: Option<String>,
    #[serde(default)]
    pub video: Option<String>,
}

fn validate_image_gen_args(args: &ImageGenArgs) -> anyhow::Result<()> {
    if args.prompt.trim().is_empty() {
        anyhow::bail!("image_gen 需要 prompt 参数");
    }
    if let Some(sz) = args.image_size.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        match sz {
            "0.5K" | "1K" | "2K" | "4K" => {}
            _ => anyhow::bail!(
                "image_size 无效: {sz}（仅支持 0.5K / 1K / 2K / 4K，须大写 K）"
            ),
        }
    }
    if let Some(level) = args.thinking_level.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        match level {
            "minimal" | "high" => {}
            _ => anyhow::bail!("thinking_level 无效: {level}（仅支持 minimal / high）"),
        }
    }
    if args.image_search && !args.google_search {
        anyhow::bail!("image_search 需要同时设置 google_search=true");
    }
    let has_uri = args.video_uri.as_deref().map(str::trim).is_some_and(|s| !s.is_empty());
    let has_file = args.video.as_deref().map(str::trim).is_some_and(|s| !s.is_empty());
    if has_uri && has_file {
        anyhow::bail!("video 与 video_uri 不能同时使用");
    }
    if let Some(refs) = &args.reference_images {
        if refs.iter().filter(|p| !p.trim().is_empty()).count() > 14 {
            anyhow::bail!("reference_images 最多 14 张");
        }
    }
    Ok(())
}
```

- [x] **Step 3: 跑测试**

Run:

```bash
cargo test -p tools arg_tests -- --nocapture
```

Expected: PASS

- [x] **Step 4: Commit**

```bash
git add tools/src/builtin/media/image_gen.rs
git commit -m "feat(tools): expand image_gen args and validation for Interactions"
```

---

### Task 4: Google / OpenAI dispatch 接线

**Files:**
- Modify: `crates/agent-tools/src/builtin/media/image_gen.rs`

**Interfaces:**
- Consumes: `providers::interactions_http::{google_interactions_image, InteractionImageRequest, InteractionImagePart, InteractionVideoInput}`
- 复用 `video_gen` 同类路径解析逻辑（可内联复制 `resolve_workspace_file` / mime helper，或抽私有 fn；本任务允许在 `image_gen.rs` 内复制精简版以避免跨模块大重构）

- [x] **Step 1: 更新 `register` 描述**

```rust
description: "Generate or edit images via Gemini Interactions (Nano Banana): text-to-image, up to 14 reference_images, previous_interaction_id for multi-turn, optional google_search/image_search, thinking_level, video_uri/video. Params: aspect_ratio, image_size (0.5K|1K|2K|4K). Falls back to OpenAI gpt-image-2 for prompt-only. Writes generated/images/."
```

- [x] **Step 2: 重写 `dispatch` / `generate_one`**

逻辑要点：

1. `validate_image_gen_args`
2. 遍历 primary/fallback creds
3. 若 `creds.provider == "google"`：
   - 读取 `reference_images` → `InteractionImagePart`（workspace 内、可读）
   - `video`：读文件，限制 `20 * 1024 * 1024` bytes；`video_uri` → `InteractionVideoInput::Uri { mime_type: "video/mp4" }`
   - 构建 `InteractionImageRequest`
   - `reqwest::Client`（超时建议 180s）
   - `google_interactions_image`
   - 落盘；返回含 `interaction_id`；若有 `search_suggestions` 追加
4. 若 OpenAI：
   - 检测是否传入高级参数（非空 `image_size`/`reference_images`/`previous_interaction_id`/`google_search`/`image_search`/`thinking_level`/`video`/`video_uri`）
   - 仅 `prompt` 走 `provider.generate_image`
   - 若有高级参数，结果末尾加 `note: OpenAI 路径忽略 Interactions 高级参数（image_size/reference_images/…）`

落盘逻辑保持现有 `generated/images/img-{ts}-{uuid8}.{ext}`。

Google `ProviderConfig`：

```rust
let config = ProviderConfig {
    api_key: creds.api_key.clone(),
    base_url: if creds.base_url.trim().is_empty() { None } else { Some(creds.base_url.clone()) },
    model: creds.model.clone(),
    ..ProviderConfig::default()
};
```

- [x] **Step 3: 编译 + 相关测试**

Run:

```bash
cargo test -p tools image_gen -- --nocapture
cargo check -p tools
```

Expected: PASS / 成功

- [x] **Step 4: Commit**

```bash
git add tools/src/builtin/media/image_gen.rs
git commit -m "feat(tools): route Google image_gen through Interactions API"
```

---

### Task 5: 冒烟核对清单（手工，不强制 API）

**Files:** 无代码变更（除非发现缺陷）

- [x] **Step 1: 确认导出与描述**

Run:

```bash
cargo test -p providers interactions_http
cargo test -p tools --lib
```

Expected: PASS

- [x] **Step 2: （有 Google Key 时）手工冒烟**

1. `prompt` only → 出图 + `interaction_id`
2. 用该 id 调 `previous_interaction_id` 改图
3. `aspect_ratio=16:9` + `image_size=1K`
4. 可选：`google_search=true`

- [x] **Step 3: 若有手工修复则单独 commit；否则跳过**

```bash
git commit -m "fix(image_gen): address Interactions smoke findings"
```

---

## Spec coverage（自检）

| Spec 要求 | Task |
|-----------|------|
| Interactions HTTP + body/parse | 1–2 |
| Args 全字段 + 校验 | 3 |
| Google 直连、不回退兼容出图 | 4 |
| OpenAI prompt-only + note | 4 |
| search_suggestions 展示 | 4 |
| 视频 uri/本地 20MiB | 4 |
| 忽略 thought 图 / 取末张 | 1 |
| 单元测试 | 1、3 |
| 工具描述更新 | 4 |

## 执行交接

Plan 已保存到 `docs/superpowers/plans/2026-07-16-image-gen-interactions-api.md`。

**两种执行方式：**

1. **Subagent-Driven（推荐）** — 每任务新开子代理，任务间审查  
2. **Inline Execution** — 本会话按 `executing-plans` 批量执行并设检查点  

选哪种？
