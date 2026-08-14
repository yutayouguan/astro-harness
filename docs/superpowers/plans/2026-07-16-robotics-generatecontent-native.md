# Robotics generateContent 原生 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增独立工具 `robotics`，经 Google 原生 `generateContent` 调用 `gemini-robotics-er-1.6-preview`，支持 point / detect / trajectory / plan。

**Architecture:** `providers::robotics_http` 负责 URL/body/解析与 HTTP；`tools::robotics` 负责参数、读图、凭证与输出格式化。仅 Google，无 OpenAI 回退；与 `vision`（Interactions）和多 Agent `orchestration` 职责分离。

**Tech Stack:** Rust (`providers` / `tools` / `home`)、`reqwest`、`serde_json`、`base64`、schemars；前端 i18n + `useAgentTools`。

**参考:** `docs/superpowers/specs/2026-07-16-robotics-generatecontent-native-design.md`；[Gemini Robotics-ER 1.6](https://ai.google.dev/gemini-api/docs/robotics-overview?hl=zh-cn)

## Global Constraints

- Google：`POST {google_native_base}/v1beta/models/{model}:generateContent`，Header `x-goog-api-key`（不用 Bearer；不用 Interactions / OpenAI 兼容）。
- 默认模型：`gemini-robotics-er-1.6-preview`；工具参数 `model` 可覆盖。
- 凭证：仅 `image_gen_targets.google()`；无 Google Key 立即失败。
- `mode` ∈ `point`|`detect`|`trajectory`|`plan`（默认 `point`）。
- 坐标：point/trajectory 为 `[y,x]`；detect 为 `box_2d`=`[ymin,xmin,ymax,xmax]`；均归一化整数 `[0,1000]`。
- `thinking_budget` 未传时默认 `0`（所有 mode）。
- 本轮不做：真硬件执行、code_execution、视频跟踪、ProvidersPanel `robotics_model`、前端可视化叠加。

## File Structure

| File | Responsibility |
|------|----------------|
| `providers/src/protocol/robotics_http.rs` | Mode、prompt、body、解析、`google_robotics_generate` |
| `providers/src/protocol/mod.rs` | `pub mod robotics_http` |
| `providers/src/lib.rs` | re-export `robotics_http` |
| `tools/src/builtins/media/robotics.rs` | 工具参数、读图、dispatch |
| `tools/src/builtins/media/mod.rs` | `pub mod robotics` |
| `tools/src/builtins/mod.rs` | re-export `robotics` |
| `tools/src/lib.rs` | `register_all` + `pub use` |
| `tools/src/core/dispatch.rs` | `"robotics" => …` |
| `home/src/config/tools_enabled.rs` | toolset 映射 + `KNOWN_TOOLSET_IDS` |
| `tools/tests/tools_test.rs` | 注册表含 `robotics` |
| `apps/desktop/src/components/ToolIcons.tsx` | `IconRobotics` |
| `apps/desktop/src/hooks/useAgentTools.ts` | 目录项 |
| `apps/desktop/src/i18n/messages.ts` | 中英 title/desc |

---

### Task 1: providers `robotics_http` — body / 解析 / HTTP

**Files:**
- Create: `providers/src/protocol/robotics_http.rs`
- Modify: `providers/src/protocol/mod.rs`
- Modify: `providers/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub enum RoboticsMode { Point, Detect, Trajectory, Plan }` — `as_str()`, `parse`
  - `pub struct RoboticsImage { pub mime_type: String, pub data_b64: String }`
  - `pub fn default_robotics_model() -> &'static str`
  - `pub fn default_robotics_prompt(mode: RoboticsMode, queries: Option<&[String]>, robot_api: Option<&str>, user_prompt: Option<&str>) -> String`
  - `pub fn build_robotics_generate_body(prompt: &str, images: &[RoboticsImage], thinking_budget: i32) -> serde_json::Value`
  - `pub fn parse_generate_content_text(v: &serde_json::Value) -> anyhow::Result<String>`
  - `pub fn strip_json_fence(s: &str) -> &str`
  - `pub fn robotics_generate_content_url(config: &ProviderConfig, model: &str) -> String`
  - `pub async fn google_robotics_generate(client: &reqwest::Client, model: &str, prompt: &str, images: &[RoboticsImage], thinking_budget: i32, config: &ProviderConfig) -> anyhow::Result<String>`
- Consumes: `crate::media_http::google_native_base`；`ProviderConfig`

- [ ] **Step 1: 注册模块**

在 `providers/src/protocol/mod.rs` 增加：

```rust
pub mod robotics_http;
```

在 `providers/src/lib.rs` 的 `pub use protocol::{...}` 加入 `robotics_http`。

- [ ] **Step 2: 写失败测试（文件底部 `#[cfg(test)]`）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_model_is_robotics_er_16() {
        assert_eq!(
            default_robotics_model(),
            "gemini-robotics-er-1.6-preview"
        );
    }

    #[test]
    fn parse_mode() {
        assert_eq!(RoboticsMode::parse("").unwrap(), RoboticsMode::Point);
        assert_eq!(RoboticsMode::parse("detect").unwrap(), RoboticsMode::Detect);
        assert!(RoboticsMode::parse("segment").is_err());
    }

    #[test]
    fn build_body_inline_and_thinking_budget() {
        let images = [RoboticsImage {
            mime_type: "image/png".into(),
            data_b64: "YWJj".into(),
        }];
        let body = build_robotics_generate_body("Point to items", &images, 0);
        let parts = body["contents"][0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["inlineData"]["mimeType"], "image/png");
        assert_eq!(parts[0]["inlineData"]["data"], "YWJj");
        assert_eq!(parts[1]["text"], "Point to items");
        assert_eq!(body["generationConfig"]["temperature"], 1.0);
        assert_eq!(
            body["generationConfig"]["thinkingConfig"]["thinkingBudget"],
            0
        );
        assert!(body.get("model").is_none());
    }

    #[test]
    fn url_uses_v1beta_generate_content() {
        let cfg = ProviderConfig {
            api_key: "k".into(),
            base_url: Some(
                "https://generativelanguage.googleapis.com/v1beta/openai".into(),
            ),
            ..ProviderConfig::default()
        };
        let url = robotics_generate_content_url(&cfg, "gemini-robotics-er-1.6-preview");
        assert_eq!(
            url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-robotics-er-1.6-preview:generateContent"
        );
    }

    #[test]
    fn parse_candidates_text() {
        let v = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        {"text": "[{\"point\":[1,2],\"label\":\"a\"}]"}
                    ]
                }
            }]
        });
        assert!(parse_generate_content_text(&v).unwrap().contains("point"));
    }

    #[test]
    fn strip_fence() {
        let s = "```json\n[{\"point\":[1,2],\"label\":\"a\"}]\n```";
        assert!(strip_json_fence(s).starts_with('['));
    }

    #[test]
    fn point_prompt_mentions_normalized_coords() {
        let p = default_robotics_prompt(RoboticsMode::Point, None, None, None);
        assert!(p.contains("0-1000") || p.contains("0–1000"));
        assert!(p.contains("point"));
    }

    #[test]
    fn plan_prompt_includes_robot_api_when_provided() {
        let api = "def move(x,y,high): ...";
        let p = default_robotics_prompt(RoboticsMode::Plan, None, Some(api), Some("pick blue block"));
        assert!(p.contains("move"));
        assert!(p.contains("function"));
        assert!(p.contains("pick blue block"));
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p providers robotics_http -- --nocapture`

Expected: FAIL（模块/符号不存在）

- [ ] **Step 4: 最小实现**

创建 `providers/src/protocol/robotics_http.rs`：

```rust
//! Gemini Robotics-ER：原生 `generateContent`（点 / 框 / 轨迹 / 规划）。

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};

use crate::media_http::google_native_base;
use crate::trait_::ProviderConfig;

fn trim_slash(endpoint: &str) -> String {
    endpoint.trim_end_matches('/').to_string()
}

pub fn default_robotics_model() -> &'static str {
    "gemini-robotics-er-1.6-preview"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoboticsMode {
    Point,
    Detect,
    Trajectory,
    Plan,
}

impl RoboticsMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Point => "point",
            Self::Detect => "detect",
            Self::Trajectory => "trajectory",
            Self::Plan => "plan",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "point" => Ok(Self::Point),
            "detect" => Ok(Self::Detect),
            "trajectory" => Ok(Self::Trajectory),
            "plan" => Ok(Self::Plan),
            other => anyhow::bail!("无效 mode: {other}（期望 point|detect|trajectory|plan）"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RoboticsImage {
    pub mime_type: String,
    pub data_b64: String,
}

pub fn default_robotics_prompt(
    mode: RoboticsMode,
    queries: Option<&[String]>,
    robot_api: Option<&str>,
    user_prompt: Option<&str>,
) -> String {
    if let Some(p) = user_prompt.map(str::trim).filter(|s| !s.is_empty()) {
        // 调用方自定义 prompt 时仍可附带 queries / robot_api 上下文
        let mut out = p.to_string();
        if let Some(q) = queries.filter(|q| !q.is_empty()) {
            out.push_str("\nObjects: ");
            out.push_str(&q.join(", "));
        }
        if let Some(api) = robot_api.map(str::trim).filter(|s| !s.is_empty()) {
            out.push_str("\n\nRobot API:\n");
            out.push_str(api);
            out.push_str(
                "\nProvide the sequence of function calls as a JSON list of objects with \"function\" and \"args\" keys.",
            );
        }
        return out;
    }
    match mode {
        RoboticsMode::Point => {
            if let Some(q) = queries.filter(|q| !q.is_empty()) {
                format!(
                    "Get all points matching the following objects: {}.\n\
                     The label returned should be an identifying name for the object detected.\n\
                     The answer should follow the json format:\n\
                     [{{\"point\": [y, x], \"label\": <label>}}, ...].\n\
                     The points are in [y, x] format normalized to 0-1000.",
                    q.join(", ")
                )
            } else {
                "Point to no more than 10 items in the image. The label returned \
                 should be an identifying name for the object detected.\n\
                 The answer should follow the json format: [{\"point\": [y, x], \"label\": <label1>}, ...]. \
                 The points are in [y, x] format normalized to 0-1000."
                    .to_string()
            }
        }
        RoboticsMode::Detect => "Return bounding boxes as a JSON array with labels. Never return masks \
             or code fencing. Limit to 25 objects. Include as many objects as you \
             can identify.\n\
             If an object is present multiple times, name them according to their \
             unique characteristic (colors, size, position, etc.).\n\
             The format should be as follows: [{\"box_2d\": [ymin, xmin, ymax, xmax], \
             \"label\": <label for the object>}] normalized to 0-1000. The values in \
             box_2d must only be integers."
            .to_string(),
        RoboticsMode::Trajectory => "Place a point on the primary object to move, then up to 15 points for the \
             trajectory to the target location described by the scene or task.\n\
             The points should be labeled by order of the trajectory, from '0' \
             (start) to <n> (final point).\n\
             The answer should follow the json format:\n\
             [{\"point\": [y, x], \"label\": <label>}, ...].\n\
             The points are in [y, x] format normalized to 0-1000."
            .to_string(),
        RoboticsMode::Plan => {
            let mut s = "Explain how to complete the task visible in the image step by step. \
                 Point to each object that you refer to. Each point should be in the format:\n\
                 [{\"point\": [y, x], \"label\": <label>}], where the coordinates are \
                 normalized between 0-1000."
                .to_string();
            if let Some(api) = robot_api.map(str::trim).filter(|a| !a.is_empty()) {
                s.push_str("\n\nYou have the following robot functions available:\n");
                s.push_str(api);
                s.push_str(
                    "\n\nProvide reasoning, then the sequence of function calls as a JSON list of objects, \
                     where each object has a \"function\" key and an \"args\" key (a list of arguments).",
                );
            }
            s
        }
    }
}

pub fn build_robotics_generate_body(
    prompt: &str,
    images: &[RoboticsImage],
    thinking_budget: i32,
) -> Value {
    let mut parts = Vec::new();
    for img in images {
        parts.push(json!({
            "inlineData": {
                "mimeType": img.mime_type,
                "data": img.data_b64
            }
        }));
    }
    parts.push(json!({ "text": prompt }));
    // model 只出现在 URL path，body 不含 model（对齐官方 REST）
    json!({
        "contents": [{ "role": "user", "parts": parts }],
        "generationConfig": {
            "temperature": 1.0,
            "thinkingConfig": {
                "thinkingBudget": thinking_budget
            }
        }
    })
}

pub fn robotics_generate_content_url(config: &ProviderConfig, model: &str) -> String {
    let base = trim_slash(&google_native_base(config));
    if base.contains("/v1beta") {
        format!("{base}/models/{model}:generateContent")
    } else {
        format!("{base}/v1beta/models/{model}:generateContent")
    }
}

pub fn parse_generate_content_text(v: &Value) -> Result<String> {
    let mut parts = Vec::new();
    if let Some(cands) = v.get("candidates").and_then(|c| c.as_array()) {
        for cand in cands {
            if let Some(ps) = cand
                .pointer("/content/parts")
                .and_then(|p| p.as_array())
            {
                for p in ps {
                    if let Some(t) = p.get("text").and_then(|t| t.as_str()) {
                        parts.push(t.to_string());
                    }
                }
            }
        }
    }
    let joined = parts.join("");
    if joined.trim().is_empty() {
        anyhow::bail!("generateContent 响应无文本");
    }
    Ok(joined)
}

pub fn strip_json_fence(s: &str) -> &str {
    let t = s.trim();
    let t = t
        .strip_prefix("```json")
        .or_else(|| t.strip_prefix("```JSON"))
        .or_else(|| t.strip_prefix("```"))
        .unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t);
    t.trim()
}

pub async fn google_robotics_generate(
    client: &Client,
    model: &str,
    prompt: &str,
    images: &[RoboticsImage],
    thinking_budget: i32,
    config: &ProviderConfig,
) -> Result<String> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    if images.is_empty() {
        anyhow::bail!("robotics 至少需要一张图片");
    }
    let model = if model.trim().is_empty() {
        default_robotics_model()
    } else {
        model.trim()
    };
    let url = robotics_generate_content_url(config, model);
    let body = build_robotics_generate_body(prompt, images, thinking_budget);
    let response = client
        .post(&url)
        .header("x-goog-api-key", &config.api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 Google Robotics generateContent 失败: {url}"))?;
    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析 Robotics generateContent JSON 失败")?;
    if !status.is_success() {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("Robotics generateContent 失败");
        anyhow::bail!("Google Robotics HTTP {status}: {msg}");
    }
    parse_generate_content_text(&v)
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p providers robotics_http -- --nocapture`

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add providers/src/protocol/robotics_http.rs providers/src/protocol/mod.rs providers/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(providers): add Gemini Robotics-ER generateContent helper

Native generateContent client for point/detect/trajectory/plan prompts.
EOF
)"
```

---

### Task 2: tools `robotics` — 注册与 dispatch

**Files:**
- Create: `tools/src/builtins/media/robotics.rs`
- Modify: `tools/src/builtins/media/mod.rs`
- Modify: `tools/src/builtins/mod.rs`

**Interfaces:**
- Consumes: Task 1 全部公开符号；`ToolContext::image_gen_targets.google()`；`ProviderConfig`
- Produces:
  - `pub fn register(registry: &mut ToolRegistry)`
  - `pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String>`
  - `pub struct RoboticsArgs { … }`

- [ ] **Step 1: 挂模块**

`tools/src/builtins/media/mod.rs`：

```rust
pub mod robotics;
```

`tools/src/builtins/mod.rs`：

```rust
pub use media::{image_gen, music, robotics, tts, video_gen, vision};
```

- [ ] **Step 2: 写失败测试（`robotics.rs` 内 `#[cfg(test)]`）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use providers::robotics_http::RoboticsMode;

    #[test]
    fn args_default_mode_point() {
        let args: RoboticsArgs = serde_json::from_value(serde_json::json!({
            "image_url": "a.png"
        }))
        .unwrap();
        assert_eq!(args.image_url.as_deref(), Some("a.png"));
        assert_eq!(RoboticsMode::parse(args.mode.as_deref().unwrap_or("")).unwrap(), RoboticsMode::Point);
    }

    #[test]
    fn format_output_marks_parse_raw_on_invalid_json() {
        let out = format_robotics_output("not-json", "m", RoboticsMode::Point, false);
        assert!(out.contains("parse=raw"));
        assert!(out.contains("provider=google"));
    }

    #[test]
    fn format_output_pretty_json_array() {
        let raw = r#"[{"point":[1,2],"label":"a"}]"#;
        let out = format_robotics_output(raw, "m", RoboticsMode::Point, true);
        assert!(out.contains("\"point\""));
        assert!(!out.contains("parse=raw"));
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p tools robotics -- --nocapture`

Expected: FAIL

- [ ] **Step 4: 实现 `robotics.rs`**

要点（完整文件由实现者按此骨架编写）：

```rust
//! Robotics：Google Gemini Robotics-ER 原生 generateContent。

use base64::Engine;
use providers::robotics_http::{
    default_robotics_model, default_robotics_prompt, google_robotics_generate, strip_json_fence,
    RoboticsImage, RoboticsMode,
};
use providers::trait_::ProviderConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::context::{ImageGenCreds, ToolContext};
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct RoboticsArgs {
    #[serde(default)]
    pub image_urls: Option<Vec<String>>,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub queries: Option<Vec<String>>,
    #[serde(default)]
    pub robot_api: Option<String>,
    #[serde(default)]
    pub thinking_budget: Option<i32>,
    #[serde(default)]
    pub model: Option<String>,
}

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "robotics".to_string(),
        toolset: "robotics".to_string(),
        description: "Spatial robotics perception and task planning via Google Gemini Robotics-ER (generateContent). Modes: point (default), detect, trajectory, plan. Pass image_urls or image_url. Optional queries, robot_api (plan), thinking_budget, model. Google-only; not vision and not multi-agent orchestration."
            .to_string(),
        schema: schema_for_args::<RoboticsArgs>(),
        check_fn: None,
        icon: "bot",
    });
}

pub async fn dispatch(ctx: &ToolContext<'_>, args: &Value) -> anyhow::Result<String> {
    let parsed: RoboticsArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("robotics 参数无效: {e}"))?;
    let mode = RoboticsMode::parse(parsed.mode.as_deref().unwrap_or(""))?;
    let mut urls = parsed.image_urls.unwrap_or_default();
    if let Some(one) = parsed.image_url {
        let t = one.trim();
        if !t.is_empty() {
            urls.push(t.to_string());
        }
    }
    urls.retain(|u| !u.trim().is_empty());
    if urls.is_empty() {
        anyhow::bail!("robotics 需要 image_urls 或 image_url");
    }

    let queries = parsed.queries.filter(|q| !q.is_empty());
    let robot_api = parsed
        .robot_api
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let user_prompt = parsed
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let prompt = default_robotics_prompt(
        mode,
        queries.as_deref(),
        robot_api,
        user_prompt,
    );
    let thinking_budget = parsed.thinking_budget.unwrap_or(0);
    let model = parsed
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_robotics_model())
        .to_string();

    let Some(creds) = ctx.image_gen_targets.google() else {
        anyhow::bail!("robotics 需要配置 Google API Key（Providers 面板）");
    };
    let images = resolve_images(ctx, &urls).await?;
    let text = call_google(creds, &model, &prompt, &images, thinking_budget).await?;
    let looks_json = looks_like_json_payload(&text);
    Ok(format_robotics_output(&text, &model, mode, looks_json))
}

fn looks_like_json_payload(text: &str) -> bool {
    let t = strip_json_fence(text);
    (t.starts_with('[') || t.starts_with('{')) && serde_json::from_str::<Value>(t).is_ok()
}

fn format_robotics_output(text: &str, model: &str, mode: RoboticsMode, ok_json: bool) -> String {
    let body = if ok_json {
        let t = strip_json_fence(text);
        serde_json::from_str::<Value>(t)
            .ok()
            .and_then(|v| serde_json::to_string_pretty(&v).ok())
            .unwrap_or_else(|| text.to_string())
    } else if mode == RoboticsMode::Plan {
        // plan 常含推理 + JSON；保留原文，不加 parse=raw
        text.to_string()
    } else {
        format!("{text}\nparse=raw")
    };
    format!(
        "{body}\nprovider=google\nmodel={model}\nmode={}",
        mode.as_str()
    )
}

async fn call_google(
    creds: &ImageGenCreds,
    model: &str,
    prompt: &str,
    images: &[RoboticsImage],
    thinking_budget: i32,
) -> anyhow::Result<String> {
    let config = ProviderConfig {
        api_key: creds.api_key.clone(),
        base_url: if creds.base_url.trim().is_empty() {
            None
        } else {
            Some(creds.base_url.clone())
        },
        model: model.to_string(),
        ..ProviderConfig::default()
    };
    let client = reqwest::Client::new();
    google_robotics_generate(&client, model, prompt, images, thinking_budget, &config).await
}

async fn resolve_images(
    ctx: &ToolContext<'_>,
    urls: &[String],
) -> anyhow::Result<Vec<RoboticsImage>> {
    let client = reqwest::Client::new();
    let mut out = Vec::new();
    for u in urls {
        let u = u.trim();
        if u.starts_with("http://") || u.starts_with("https://") {
            let resp = client
                .get(u)
                .send()
                .await
                .map_err(|e| anyhow::anyhow!("下载图片失败 {u}: {e}"))?;
            if !resp.status().is_success() {
                anyhow::bail!("下载图片 HTTP {}: {u}", resp.status());
            }
            let bytes = resp
                .bytes()
                .await
                .map_err(|e| anyhow::anyhow!("读取远程图片失败 {u}: {e}"))?;
            let mime = mime_from_url_or_path(u).to_string();
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            out.push(RoboticsImage {
                mime_type: mime,
                data_b64: b64,
            });
        } else if let Some(rest) = u.strip_prefix("data:") {
            let (meta, b64) = rest
                .split_once(',')
                .ok_or_else(|| anyhow::anyhow!("无效 data URL"))?;
            let mime = meta.split(';').next().unwrap_or("image/jpeg");
            out.push(RoboticsImage {
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
            let mime = mime_from_path(&path).to_string();
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            out.push(RoboticsImage {
                mime_type: mime,
                data_b64: b64,
            });
        }
    }
    Ok(out)
}

fn mime_from_url_or_path(s: &str) -> &'static str {
    let path_part = s.split(['?', '#']).next().unwrap_or(s);
    mime_from_extension(
        std::path::Path::new(path_part)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or(""),
    )
}

fn mime_from_path(path: &std::path::Path) -> &'static str {
    mime_from_extension(path.extension().and_then(|e| e.to_str()).unwrap_or(""))
}

fn mime_from_extension(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "jpg" | "jpeg" => "image/jpeg",
        "bmp" => "image/bmp",
        "heic" => "image/heic",
        "heif" => "image/heif",
        _ => "image/jpeg",
    }
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p tools robotics -- --nocapture`

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add tools/src/builtins/media/robotics.rs tools/src/builtins/media/mod.rs tools/src/builtins/mod.rs
git commit -m "$(cat <<'EOF'
feat(tools): add robotics tool dispatch for Robotics-ER

Wire image resolve and Google-only generateContent modes.
EOF
)"
```

---

### Task 3: 路由、toolset、注册表测试

**Files:**
- Modify: `tools/src/lib.rs`
- Modify: `tools/src/core/dispatch.rs`
- Modify: `home/src/config/tools_enabled.rs`
- Modify: `tools/tests/tools_test.rs`

**Interfaces:**
- Consumes: `robotics::register` / `robotics::dispatch`
- Produces: 工具名 `robotics` 可注册、可分发、可开关

- [ ] **Step 1: 写失败断言（扩展现有测试）**

在 `tools/tests/tools_test.rs` 的 `register_all_includes_panel_tools` 期望列表中加入 `"robotics"`。

- [ ] **Step 2: Run to verify fail**

Run: `cargo test -p tools register_all_includes_panel_tools -- --nocapture`

Expected: FAIL `missing robotics`

- [ ] **Step 3: 接线**

`tools/src/lib.rs`：

```rust
pub(crate) use builtins::{
    browser, clarify, code_exec, confirm, create_agent, delegate, file_ops, image_gen, memory_tools,
    multi_agent, music, orchestration, present_callout, present_metrics, present_result, present_ui,
    request_user_location, robotics, scheduled, skills_tool, task_plan, terminal, tts, video_gen,
    vision, web_search,
};
```

`register_all` 在 `vision::register` 旁：

```rust
vision::register(registry);
robotics::register(registry);
```

`tools/src/core/dispatch.rs` match 臂：

```rust
"vision" => crate::vision::dispatch(ctx, args).await,
"robotics" => crate::robotics::dispatch(ctx, args).await,
```

`home/src/config/tools_enabled.rs`：

- `KNOWN_TOOLSET_IDS` 在 `"vision"` 后加 `"robotics"`
- `tool_name_to_toolset`：

```rust
"vision" => "vision",
"robotics" => "robotics",
```

- [ ] **Step 4: Run tests**

Run:

```bash
cargo test -p tools register_all_includes_panel_tools -- --nocapture
cargo test -p home tools_enabled -- --nocapture
```

Expected: PASS（若 home 无对应测试名，至少 `cargo test -p home --lib`）

- [ ] **Step 5: Commit**

```bash
git add tools/src/lib.rs tools/src/core/dispatch.rs home/src/config/tools_enabled.rs tools/tests/tools_test.rs
git commit -m "$(cat <<'EOF'
feat: register robotics toolset and dispatch route

Expose robotics in register_all, dispatch, and tools-enabled.
EOF
)"
```

---

### Task 4: 前端目录与 i18n

**Files:**
- Modify: `apps/desktop/src/components/ToolIcons.tsx`
- Modify: `apps/desktop/src/hooks/useAgentTools.ts`
- Modify: `apps/desktop/src/i18n/messages.ts`

**Interfaces:**
- Produces: UI 工具列表出现 `robotics`，中英文案说明与 `vision` / 多 Agent orchestration 的区别

- [ ] **Step 1: 增加图标**

在 `ToolIcons.tsx` 新增（机械臂风格简笔，复用现有 stroke 风格）：

```tsx
export function IconRobotics(props: IconProps) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" {...props}>
      <rect x="7" y="9" width="10" height="8" rx="1.5" />
      <circle cx="10" cy="13" r="1" />
      <circle cx="14" cy="13" r="1" />
      <path d="M12 5v4M9 17v2M15 17v2M5 12H3M21 12h-2" />
    </svg>
  );
}
```

- [ ] **Step 2: useAgentTools**

- `AgentToolId` 联合类型增加 `"robotics"`
- import `IconRobotics`
- 在 `vision` 条目后插入：

```ts
{
  id: "robotics",
  titleKey: "agentTools.robotics.title",
  descKey: "agentTools.robotics.desc",
  Icon: IconRobotics,
  tone: "pink",
  params: [
    { name: "image_url", type: "string" },
    { name: "mode", type: "string", optional: true },
    { name: "prompt", type: "string", optional: true },
    { name: "queries", type: "string", optional: true },
    { name: "robot_api", type: "string", optional: true },
  ],
},
```

- [ ] **Step 3: i18n**

中文（与 `agentTools.vision` 相邻）：

```ts
"agentTools.robotics.title": "机器人",
"agentTools.robotics.desc": "用 Google Robotics-ER 原生 generateContent 做空间指点/检测/轨迹与任务规划（非通用视觉，非多 Agent 编排）",
```

英文：

```ts
"agentTools.robotics.title": "Robotics",
"agentTools.robotics.desc": "Spatial pointing, boxes, trajectories, and task plans via Google Robotics-ER generateContent (not general vision; not multi-agent orchestration)",
```

- [ ] **Step 4: 类型检查（若项目有）**

Run: `cd frontend && npm test -- --run autoModelSelect.test.ts 2>/dev/null || true`

若 `KNOWN_TOOLSET` / 工具 id 列表测试需要更新，同步加入 `"robotics"`（检查 `apps/desktop/src/lib/autoModelSelect.test.ts` 是否枚举全部 tool id；**不要**把 robotics 并入 TaskKind `vision` 自动选模）。

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/components/ToolIcons.tsx apps/desktop/src/hooks/useAgentTools.ts apps/desktop/src/i18n/messages.ts
git commit -m "$(cat <<'EOF'
feat(ui): expose robotics tool in agent tools panel

Add icon, catalog entry, and zh/en copy for Robotics-ER.
EOF
)"
```

---

### Task 5: 端到端冒烟（可选，有 Key 时）

**Files:** 无代码变更（手工）

- [ ] **Step 1:** 配置 Google API Key，工作区放一张桌面物体图 `scene.png`

- [ ] **Step 2:** Agent 调用：

```json
{"image_url":"scene.png","mode":"point"}
```

预期：JSON 点列表 + `provider=google` + `model=gemini-robotics-er-1.6-preview`

- [ ] **Step 3:** 再试 `mode=plan` + 简短 `robot_api` 文本，预期含 `function`/`args` 或步骤说明

无 Key 时可跳过；自动化仍以 Task 1–4 单测为准。

---

## Spec coverage（自检）

| Spec 要求 | Task |
|-----------|------|
| 独立工具 + 四 mode | 2, 3 |
| 原生 generateContent | 1 |
| 默认 Robotics-ER 1.6 | 1 |
| 无 OpenAI 回退 | 2 |
| point/detect/trajectory/plan 输出约定 | 1 prompts + 2 format |
| robot_api 可选 | 1, 2 |
| thinking_budget 默认 0 | 1, 2 |
| 凭证 google() | 2 |
| tools-enabled / UI | 3, 4 |
| 与 vision / orchestration 边界文案 | 2 description, 4 i18n |
| 非目标：硬件 / code_exec / 视频 / Providers 字段 | 未实现（刻意） |

## Placeholder scan

- 无 TBD / 「implement later」；`build_robotics_generate_body` 签名与测试已对齐（body 不含 model）。
- 无「similar to Task N」未展开步骤。
