# Video Understand (Interactions) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增独立工具 `video_understand`：Google Interactions 原生视频理解（本地 inline / Files API / YouTube、多视频≤10、可选 `media_resolution`），纯 Google，不走 OpenAI。

**Architecture:** `providers::files_http` 负责 resumable 上传与轮询；`providers::interactions_http` 追加视频理解 body/parse/HTTP；`tools::video_understand` 做参数校验、100MB 分流与结果文案；登记 toolset 与前端开关。

**Tech Stack:** Rust (`providers` / `tools` / `home`)、`reqwest`、`serde_json`、`base64`、schemars；前端 `useAgentTools` + i18n。

**参考:** `docs/superpowers/specs/2026-07-16-video-understand-interactions-design.md`；[视频理解](https://ai.google.dev/gemini-api/docs/video-understanding?hl=zh-cn)；[媒体分辨率](https://ai.google.dev/gemini-api/docs/media-resolution?hl=zh-cn)

## Global Constraints

- Google only：`POST {google_native_base}/v1beta/interactions`，Header `x-goog-api-key`；禁止 `google_openai_base` / `chat/completions` / OpenAI 回退。
- 模型：`creds.vision_model`，空则 `gemini-3.5-flash`（复用 `default_vision_model("google")`）。
- 本地分流：`INLINE_MAX_BYTES = 100 * 1024 * 1024`；≥ 阈值走 Files API；YouTube 永不 Files。
- `videos` + `youtube_urls` 至少一类非空，合计 ≤10。
- `media_resolution` ∈ `low`|`medium`|`high`（缺省不传）；映射为每个 video part 的 `"resolution"`。
- `input` 顺序：全部 video parts，再 text。
- Files 轮询间隔 5s，超时 10 分钟；不主动 `files.delete`。
- 若 `interactions_http.rs` 已存在（TTS 等）：只追加视频理解符号，不破坏现有 API。

## File Structure

| File | Responsibility |
|------|----------------|
| `providers/src/protocol/files_http.rs` | Files API：resumable 上传 + 轮询 ACTIVE |
| `providers/src/protocol/interactions_http.rs` | 追加：视频 part / body / text 解析 / `google_interactions_video` |
| `providers/src/protocol/mod.rs` / `lib.rs` | 登记 `files_http` |
| `tools/src/builtins/media/video_understand.rs` | 新工具 |
| `tools/src/builtins/media/mod.rs` 等 | 注册 / dispatch / re-export |
| `home/src/config/tools_enabled.rs` | `KNOWN_TOOLSET_IDS` + 名映射 |
| `frontend/src/hooks/useAgentTools.ts` | 工具开关定义 |
| `frontend/src/i18n/messages.ts` | 中英 title/desc |
| `tools/tests/tools_test.rs` | 注册表断言含 `video_understand` |

---

### Task 1: Files API helper

**Files:**
- Create: `providers/src/protocol/files_http.rs`
- Modify: `providers/src/protocol/mod.rs`
- Modify: `providers/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub const INLINE_MAX_BYTES: u64 = 100 * 1024 * 1024;`
  - `pub const FILES_POLL_INTERVAL: Duration = Duration::from_secs(5);`
  - `pub const FILES_POLL_TIMEOUT: Duration = Duration::from_secs(600);`
  - `pub struct UploadedFile { pub name: String, pub uri: String, pub mime_type: String }`
  - `pub async fn google_files_upload_and_wait(client: &Client, config: &ProviderConfig, bytes: &[u8], mime_type: &str, display_name: &str) -> Result<UploadedFile>`
  - `pub fn parse_file_status(v: &Value) -> Result<FileState>`（测轮询分支；`FileState::{Processing, Active, Failed}`）
- Consumes: `crate::media_http::google_native_base`；`ProviderConfig`

- [ ] **Step 1: 注册模块**

`providers/src/protocol/mod.rs` 增加：

```rust
pub mod files_http;
```

`providers/src/lib.rs` 的 `pub use protocol::{...}` 加入 `files_http`。

- [ ] **Step 2: 写失败测试（`files_http.rs` 底部）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn inline_max_is_100mb() {
        assert_eq!(INLINE_MAX_BYTES, 100 * 1024 * 1024);
    }

    #[test]
    fn parse_active_state() {
        let v = json!({ "state": "ACTIVE", "uri": "https://x", "name": "files/abc", "mimeType": "video/mp4" });
        match parse_file_status(&v).unwrap() {
            FileState::Active { uri, name, mime_type } => {
                assert_eq!(uri, "https://x");
                assert_eq!(name, "files/abc");
                assert_eq!(mime_type, "video/mp4");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_failed_state() {
        let v = json!({ "state": "FAILED" });
        assert!(matches!(parse_file_status(&v).unwrap(), FileState::Failed));
    }

    #[test]
    fn parse_processing_state() {
        let v = json!({ "state": "PROCESSING" });
        assert!(matches!(parse_file_status(&v).unwrap(), FileState::Processing));
    }
}
```

- [ ] **Step 3: 跑测确认失败**

Run: `cargo test -p providers parse_active_state -- --nocapture`  
Expected: FAIL（模块/符号不存在）

- [ ] **Step 4: 实现 `files_http.rs`**

要点（实现须完整可编译）：

```rust
//! Google Files API：可恢复上传 + 轮询至 ACTIVE。

use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use tokio::time::sleep;

use crate::media_http::google_native_base;
use crate::trait_::ProviderConfig;

pub const INLINE_MAX_BYTES: u64 = 100 * 1024 * 1024;
pub const FILES_POLL_INTERVAL: Duration = Duration::from_secs(5);
pub const FILES_POLL_TIMEOUT: Duration = Duration::from_secs(600);

pub struct UploadedFile {
    pub name: String,
    pub uri: String,
    pub mime_type: String,
}

#[derive(Debug)]
pub enum FileState {
    Processing,
    Active {
        name: String,
        uri: String,
        mime_type: String,
    },
    Failed,
}

fn trim_slash(s: &str) -> String {
    s.trim_end_matches('/').to_string()
}

pub fn parse_file_status(v: &Value) -> Result<FileState> {
    let state = v
        .get("state")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_ascii_uppercase();
    match state.as_str() {
        "ACTIVE" => {
            let name = v
                .get("name")
                .and_then(|n| n.as_str())
                .ok_or_else(|| anyhow!("Files ACTIVE 响应缺 name"))?
                .to_string();
            let uri = v
                .get("uri")
                .and_then(|u| u.as_str())
                .ok_or_else(|| anyhow!("Files ACTIVE 响应缺 uri"))?
                .to_string();
            let mime_type = v
                .get("mimeType")
                .or_else(|| v.get("mime_type"))
                .and_then(|m| m.as_str())
                .unwrap_or("video/mp4")
                .to_string();
            Ok(FileState::Active {
                name,
                uri,
                mime_type,
            })
        }
        "FAILED" => Ok(FileState::Failed),
        _ => Ok(FileState::Processing),
    }
}

pub async fn google_files_upload_and_wait(
    client: &Client,
    config: &ProviderConfig,
    bytes: &[u8],
    mime_type: &str,
    display_name: &str,
) -> Result<UploadedFile> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    let host = {
        let base = google_native_base(config);
        trim_slash(&base)
            .trim_end_matches("/v1beta")
            .to_string()
    };
    let start_url = format!("{host}/upload/v1beta/files");

    let start = client
        .post(&start_url)
        .header("x-goog-api-key", config.api_key.trim())
        .header("X-Goog-Upload-Protocol", "resumable")
        .header("X-Goog-Upload-Command", "start")
        .header("X-Goog-Upload-Header-Content-Length", bytes.len().to_string())
        .header("X-Goog-Upload-Header-Content-Type", mime_type)
        .header("content-type", "application/json")
        .json(&json!({ "file": { "display_name": display_name } }))
        .send()
        .await
        .with_context(|| format!("Files 启动上传失败: {start_url}"))?;
    let upload_url = start
        .headers()
        .get("x-goog-upload-url")
        .or_else(|| start.headers().get("X-Goog-Upload-URL"))
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| anyhow!("Files 启动响应缺 x-goog-upload-url"))?
        .to_string();

    let uploaded = client
        .post(&upload_url)
        .header("Content-Length", bytes.len().to_string())
        .header("X-Goog-Upload-Offset", "0")
        .header("X-Goog-Upload-Command", "upload, finalize")
        .body(bytes.to_vec())
        .send()
        .await
        .context("Files 上传数据失败")?;
    let status = uploaded.status();
    let info: Value = uploaded.json().await.context("解析 Files 上传响应失败")?;
    if !status.is_success() {
        anyhow::bail!("Files 上传 HTTP {status}: {info}");
    }
    let file = info.get("file").unwrap_or(&info);
    let mut name = file
        .get("name")
        .and_then(|n| n.as_str())
        .ok_or_else(|| anyhow!("Files 上传响应缺 file.name"))?
        .to_string();
    let mut uri = file
        .get("uri")
        .and_then(|u| u.as_str())
        .unwrap_or("")
        .to_string();
    let mut mime = file
        .get("mimeType")
        .or_else(|| file.get("mime_type"))
        .and_then(|m| m.as_str())
        .unwrap_or(mime_type)
        .to_string();

    let deadline = Instant::now() + FILES_POLL_TIMEOUT;
    loop {
        let get_url = format!("{host}/v1beta/{name}");
        let resp = client
            .get(&get_url)
            .header("x-goog-api-key", config.api_key.trim())
            .send()
            .await
            .with_context(|| format!("Files 查询失败: {get_url}"))?;
        let st = resp.status();
        let v: Value = resp.json().await.context("解析 Files 状态失败")?;
        if !st.is_success() {
            anyhow::bail!("Files 查询 HTTP {st}: {v}");
        }
        match parse_file_status(&v)? {
            FileState::Active {
                name: n,
                uri: u,
                mime_type: m,
            } => {
                name = n;
                uri = u;
                mime = m;
                break;
            }
            FileState::Failed => anyhow::bail!("Files 处理失败: {name}"),
            FileState::Processing => {
                if Instant::now() >= deadline {
                    anyhow::bail!("Files 处理超时（>10min）: {name}");
                }
                sleep(FILES_POLL_INTERVAL).await;
            }
        }
    }
    if uri.is_empty() {
        anyhow::bail!("Files ACTIVE 但仍无 uri: {name}");
    }
    Ok(UploadedFile {
        name,
        uri,
        mime_type: mime,
    })
}
```

- [ ] **Step 5: 跑测通过**

Run: `cargo test -p providers --lib files_http -- --nocapture`  
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add providers/src/protocol/files_http.rs providers/src/protocol/mod.rs providers/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(providers): add Google Files API upload-and-wait helper

EOF
)"
```

---

### Task 2: Interactions 视频理解 body / 解析 / HTTP

**Files:**
- Modify: `providers/src/protocol/interactions_http.rs`

**Interfaces:**
- Consumes: `interactions_url`；`google_native_base` 已在同模块；`ProviderConfig`
- Produces:
  - `pub enum InteractionVideoPart { Inline { mime_type: String, data_b64: String }, Uri { mime_type: Option<String>, uri: String } }`
  - `pub struct InteractionVideoUnderstandRequest { pub model: String, pub prompt: String, pub videos: Vec<InteractionVideoPart>, pub media_resolution: Option<String> }`
  - `pub fn build_interaction_video_body(req: &InteractionVideoUnderstandRequest) -> Value`
  - `pub fn parse_interaction_output_text(v: &Value) -> Result<String>`（优先 `output_text`，否则拼接 `steps`/`model_output` text）
  - `pub async fn google_interactions_video(client: &Client, config: &ProviderConfig, req: &InteractionVideoUnderstandRequest) -> Result<String>`

- [ ] **Step 1: 写失败测试**

在 `interactions_http.rs` 增加 `#[cfg(test)] mod video_tests`：

```rust
#[cfg(test)]
mod video_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn build_body_videos_before_text_with_resolution() {
        let req = InteractionVideoUnderstandRequest {
            model: "gemini-3.5-flash".into(),
            prompt: "Summarize".into(),
            videos: vec![
                InteractionVideoPart::Uri {
                    mime_type: Some("video/mp4".into()),
                    uri: "https://generativelanguage.googleapis.com/v1beta/files/x".into(),
                },
                InteractionVideoPart::Inline {
                    mime_type: "video/mp4".into(),
                    data_b64: "YWJj".into(),
                },
                InteractionVideoPart::Uri {
                    mime_type: None,
                    uri: "https://www.youtube.com/watch?v=9hE5-98ZeCg".into(),
                },
            ],
            media_resolution: Some("low".into()),
        };
        let body = build_interaction_video_body(&req);
        assert_eq!(body["model"], "gemini-3.5-flash");
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 4);
        assert_eq!(input[0]["type"], "video");
        assert_eq!(input[0]["uri"], "https://generativelanguage.googleapis.com/v1beta/files/x");
        assert_eq!(input[0]["mime_type"], "video/mp4");
        assert_eq!(input[0]["resolution"], "low");
        assert_eq!(input[1]["data"], "YWJj");
        assert_eq!(input[1]["resolution"], "low");
        assert_eq!(input[2]["uri"], "https://www.youtube.com/watch?v=9hE5-98ZeCg");
        assert!(input[2].get("mime_type").is_none());
        assert_eq!(input[2]["resolution"], "low");
        assert_eq!(input[3]["type"], "text");
        assert_eq!(input[3]["text"], "Summarize");
        assert!(body.get("response_format").is_none());
    }

    #[test]
    fn build_body_omits_resolution_when_unset() {
        let req = InteractionVideoUnderstandRequest {
            model: "m".into(),
            prompt: "q".into(),
            videos: vec![InteractionVideoPart::Uri {
                mime_type: Some("video/webm".into()),
                uri: "https://example/file".into(),
            }],
            media_resolution: None,
        };
        let body = build_interaction_video_body(&req);
        assert!(body["input"][0].get("resolution").is_none());
    }

    #[test]
    fn parse_output_text_field() {
        let v = json!({ "output_text": "hello video" });
        assert_eq!(parse_interaction_output_text(&v).unwrap(), "hello video");
    }

    #[test]
    fn parse_steps_model_output_text() {
        let v = json!({
            "steps": [
                { "type": "thought", "content": [{ "type": "text", "text": "ignore" }] },
                { "type": "model_output", "content": [
                    { "type": "text", "text": "partA" },
                    { "type": "text", "text": "partB" }
                ]}
            ]
        });
        assert_eq!(parse_interaction_output_text(&v).unwrap(), "partA\npartB");
    }
}
```

- [ ] **Step 2: 跑测确认失败**

Run: `cargo test -p providers build_body_videos_before_text_with_resolution -- --nocapture`  
Expected: FAIL

- [ ] **Step 3: 实现类型与函数**

追加到 `interactions_http.rs`（与 TTS 并存）：

```rust
#[derive(Debug, Clone)]
pub enum InteractionVideoPart {
    Inline {
        mime_type: String,
        data_b64: String,
    },
    Uri {
        mime_type: Option<String>,
        uri: String,
    },
}

#[derive(Debug, Clone)]
pub struct InteractionVideoUnderstandRequest {
    pub model: String,
    pub prompt: String,
    pub videos: Vec<InteractionVideoPart>,
    pub media_resolution: Option<String>,
}

pub fn build_interaction_video_body(req: &InteractionVideoUnderstandRequest) -> Value {
    let mut input = Vec::new();
    for v in &req.videos {
        let mut part = match v {
            InteractionVideoPart::Inline {
                mime_type,
                data_b64,
            } => json!({
                "type": "video",
                "data": data_b64,
                "mime_type": mime_type,
            }),
            InteractionVideoPart::Uri { mime_type, uri } => {
                let mut p = json!({ "type": "video", "uri": uri });
                if let Some(mt) = mime_type.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                    p["mime_type"] = json!(mt);
                }
                p
            }
        };
        if let Some(res) = req
            .media_resolution
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            part["resolution"] = json!(res);
        }
        input.push(part);
    }
    input.push(json!({ "type": "text", "text": req.prompt }));
    json!({
        "model": req.model,
        "input": input,
    })
}

pub fn parse_interaction_output_text(v: &Value) -> Result<String> {
    if let Some(t) = v.get("output_text").and_then(|t| t.as_str()) {
        let t = t.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    let mut parts = Vec::new();
    if let Some(steps) = v.get("steps").and_then(|s| s.as_array()) {
        for step in steps {
            let ty = step.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if ty != "model_output" {
                continue;
            }
            if let Some(content) = step.get("content").and_then(|c| c.as_array()) {
                for block in content {
                    if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                            let t = t.trim();
                            if !t.is_empty() {
                                parts.push(t.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    if parts.is_empty() {
        anyhow::bail!("Interactions 响应无文本输出");
    }
    Ok(parts.join("\n"))
}

pub async fn google_interactions_video(
    client: &Client,
    config: &ProviderConfig,
    req: &InteractionVideoUnderstandRequest,
) -> Result<String> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    if req.videos.is_empty() {
        anyhow::bail!("video understand 需要至少一条视频");
    }
    let url = interactions_url(config);
    let body = build_interaction_video_body(req);
    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .header("x-goog-api-key", config.api_key.trim())
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 Google interactions video 失败: {url}"))?;
    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析 Google interactions video 响应失败")?;
    if !status.is_success() {
        anyhow::bail!(
            "Google interactions HTTP {status}: {}",
            error_message(&v)
        );
    }
    parse_interaction_output_text(&v)
}
```

- [ ] **Step 4: 跑测通过**

Run: `cargo test -p providers video_tests -- --nocapture`  
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add providers/src/protocol/interactions_http.rs
git commit -m "$(cat <<'EOF'
feat(providers): add Interactions video understanding helper

EOF
)"
```

---

### Task 3: `video_understand` 工具

**Files:**
- Create: `tools/src/builtins/media/video_understand.rs`
- Modify: `tools/src/builtins/media/mod.rs`
- Modify: `tools/src/builtins/mod.rs`
- Modify: `tools/src/lib.rs`（`pub(crate) use` + `register_all`）
- Modify: `tools/src/core/dispatch.rs`

**Interfaces:**
- Consumes:
  - `providers::files_http::{google_files_upload_and_wait, INLINE_MAX_BYTES}`
  - `providers::interactions_http::{google_interactions_video, InteractionVideoPart, InteractionVideoUnderstandRequest}`
  - `providers::media_http::default_vision_model`
  - `ToolContext` / `ImageGenCreds`
- Produces: `register` / `dispatch`；参数类型 `VideoUnderstandArgs`

- [ ] **Step 1: 实现 `video_understand.rs`**

```rust
//! 视频理解：Google Interactions（本地 / Files API / YouTube）。
//!
//! 参考：<https://ai.google.dev/gemini-api/docs/video-understanding>

use base64::Engine;
use providers::files_http::{google_files_upload_and_wait, INLINE_MAX_BYTES};
use providers::interactions_http::{
    google_interactions_video, InteractionVideoPart, InteractionVideoUnderstandRequest,
};
use providers::media_http::default_vision_model;
use providers::trait_::ProviderConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

const MAX_VIDEOS: usize = 10;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct VideoUnderstandArgs {
    /// 对视频的问题或指令。
    pub prompt: String,
    /// 工作区相对/绝对路径；可与 youtube_urls 混用，合计 ≤10。
    #[serde(default)]
    pub videos: Option<Vec<String>>,
    /// 公开 YouTube URL。
    #[serde(default)]
    pub youtube_urls: Option<Vec<String>>,
    /// `low` | `medium` | `high`；映射 Interactions 每 part 的 resolution。
    #[serde(default)]
    pub media_resolution: Option<String>,
}

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "video_understand".to_string(),
        toolset: "video_understand".to_string(),
        description: "Analyze one or more videos with Google Gemini Interactions (workspace paths and/or public YouTube URLs). Large local files (>100MB) upload via Files API. Optional media_resolution: low|medium|high. Timestamps in prompts use MM:SS. Google only."
            .to_string(),
        schema: schema_for_args::<VideoUnderstandArgs>(),
        check_fn: None,
        icon: "clapperboard",
    });
}

pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: VideoUnderstandArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("video_understand 参数无效: {e}"))?;
    let prompt = parsed.prompt.trim();
    if prompt.is_empty() {
        anyhow::bail!("video_understand 需要 prompt");
    }

    let videos: Vec<String> = parsed
        .videos
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let youtube_urls: Vec<String> = parsed
        .youtube_urls
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if videos.is_empty() && youtube_urls.is_empty() {
        anyhow::bail!("video_understand 需要 videos 或 youtube_urls");
    }
    let total = videos.len() + youtube_urls.len();
    if total > MAX_VIDEOS {
        anyhow::bail!("video_understand 视频合计不得超过 {MAX_VIDEOS}（当前 {total}）");
    }

    let media_resolution = match parsed.media_resolution.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => None,
        Some(r) if matches!(r, "low" | "medium" | "high") => Some(r.to_string()),
        Some(r) => anyhow::bail!("media_resolution 须为 low|medium|high，收到: {r}"),
    };

    let creds = ctx
        .image_gen_targets
        .google()
        .ok_or_else(|| anyhow::anyhow!("video_understand 需要 Google API Key"))?;

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
    let mut parts = Vec::new();
    let mut files_count = 0usize;

    for path_str in &videos {
        let path = resolve_video_path(ctx, path_str)?;
        let bytes = std::fs::read(&path)
            .map_err(|e| anyhow::anyhow!("读取视频失败 {}: {e}", path.display()))?;
        let mime = video_mime_from_path(&path);
        let display = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("video");
        if (bytes.len() as u64) >= INLINE_MAX_BYTES {
            let uploaded =
                google_files_upload_and_wait(&client, &config, &bytes, mime, display).await?;
            files_count += 1;
            parts.push(InteractionVideoPart::Uri {
                mime_type: Some(uploaded.mime_type),
                uri: uploaded.uri,
            });
        } else {
            let data_b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            parts.push(InteractionVideoPart::Inline {
                mime_type: mime.to_string(),
                data_b64,
            });
        }
    }

    for url in &youtube_urls {
        parts.push(InteractionVideoPart::Uri {
            mime_type: None,
            uri: url.clone(),
        });
    }

    let local_n = videos.len();
    let yt_n = youtube_urls.len();
    let req = InteractionVideoUnderstandRequest {
        model: model.clone(),
        prompt: prompt.to_string(),
        videos: parts,
        media_resolution: media_resolution.clone(),
    };
    let text = google_interactions_video(&client, &config, &req).await?;

    let mut out = format!(
        "{text}\n\nprovider=google\nmodel={model}\nvideos={local_n} files={files_count} youtube={yt_n}"
    );
    if let Some(r) = media_resolution {
        out.push_str(&format!("\nmedia_resolution={r}"));
    }
    out.push_str("\nhint: 引用时间点请用 MM:SS（如 01:15）");
    Ok(out)
}

fn resolve_video_path(ctx: &ToolContext<'_>, raw: &str) -> anyhow::Result<std::path::PathBuf> {
    let p = std::path::PathBuf::from(raw);
    let path = if p.is_absolute() {
        p
    } else {
        ctx.workspace_dir.join(raw)
    };
    if !path.exists() {
        anyhow::bail!("本地视频不存在: {}", path.display());
    }
    Ok(path)
}

fn video_mime_from_path(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "mp4" => "video/mp4",
        "mpeg" | "mpg" => "video/mpeg",
        "mov" => "video/mov",
        "avi" => "video/avi",
        "flv" => "video/x-flv",
        "webm" => "video/webm",
        "wmv" => "video/wmv",
        "3gp" | "3gpp" => "video/3gpp",
        _ => "video/mp4",
    }
}
```

- [ ] **Step 2: 接线 register / dispatch / mod**

`tools/src/builtins/media/mod.rs`：

```rust
pub mod video_understand;
```

`tools/src/builtins/mod.rs`：在 `pub use media::{...}` 加入 `video_understand`。

`tools/src/lib.rs`：`pub(crate) use` 加入 `video_understand`；`register_all` 在 `vision::register` 附近调用 `video_understand::register(registry);`。

`tools/src/core/dispatch.rs`：

```rust
"video_understand" => crate::video_understand::dispatch(ctx, args).await,
```

- [ ] **Step 3: 编译检查**

Run: `cargo check -p tools`  
Expected: 成功

- [ ] **Step 4: Commit**

```bash
git add tools/src/builtins/media/video_understand.rs tools/src/builtins/media/mod.rs tools/src/builtins/mod.rs tools/src/lib.rs tools/src/core/dispatch.rs
git commit -m "$(cat <<'EOF'
feat(tools): add Google-only video_understand tool

EOF
)"
```

---

### Task 4: Toolset 开关、前端、注册测试

**Files:**
- Modify: `home/src/config/tools_enabled.rs`
- Modify: `frontend/src/hooks/useAgentTools.ts`
- Modify: `frontend/src/i18n/messages.ts`
- Modify: `tools/tests/tools_test.rs`

**Interfaces:**
- Consumes: Task 3 的工具名 `video_understand`
- Produces: 面板可开关；`tool_name_to_toolset("video_understand") == "video_understand"`

- [ ] **Step 1: `KNOWN_TOOLSET_IDS` + 映射**

在 `vision` 后插入 `"video_understand"`；`tool_name_to_toolset`：

```rust
"video_understand" => "video_understand",
```

- [ ] **Step 2: 前端 `useAgentTools.ts`**

- `AgentToolId` 联合类型加 `"video_understand"`
- import 复用 `IconVideoGen`（或 `IconEye`）；tone 用 `"violet"` 与 `video_gen` 的 `"rose"` 区分
- 在 `vision` 条目后插入：

```ts
{
  id: "video_understand",
  titleKey: "agentTools.videoUnderstand.title",
  descKey: "agentTools.videoUnderstand.desc",
  Icon: IconVideoGen,
  tone: "violet",
  params: [
    { name: "prompt", type: "string" },
    { name: "videos", type: "string[]", optional: true },
    { name: "youtube_urls", type: "string[]", optional: true },
    { name: "media_resolution", type: "string", optional: true },
  ],
},
```

- [ ] **Step 3: i18n**

中文：

```ts
"agentTools.videoUnderstand.title": "视频理解",
"agentTools.videoUnderstand.desc": "Google Gemini Interactions：本地视频 / YouTube 问答与摘要；大文件自动 Files API；可选 media_resolution",
```

英文：

```ts
"agentTools.videoUnderstand.title": "Video Understanding",
"agentTools.videoUnderstand.desc": "Google Gemini Interactions: summarize/Q&A on workspace videos and YouTube; large files via Files API; optional media_resolution",
```

确保 `MessageKey` 能覆盖新 key（若为字面量索引类型，按仓库既有模式追加）。

- [ ] **Step 4: `tools_test.rs`**

在 `register_all_includes_panel_tools` 的 expected 列表加入 `"video_understand"`。

- [ ] **Step 5: 跑测**

Run: `cargo test -p tools register_all_includes_panel_tools -- --nocapture`  
Expected: PASS

Run: `cargo test -p providers --lib files_http video_tests -- --nocapture`  
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add home/src/config/tools_enabled.rs frontend/src/hooks/useAgentTools.ts frontend/src/i18n/messages.ts tools/tests/tools_test.rs
git commit -m "$(cat <<'EOF'
feat: wire video_understand toolset and UI toggle

EOF
)"
```

- [ ] **Step 7: 更新 spec 状态（可选同 commit 或下一条）**

将 `docs/superpowers/specs/2026-07-16-video-understand-interactions-design.md` 状态改为 `已实现`（实现全部通过后）。

```bash
git add docs/superpowers/specs/2026-07-16-video-understand-interactions-design.md
git commit -m "$(cat <<'EOF'
docs: mark video_understand Interactions design as implemented

EOF
)"
```

---

## Spec coverage (self-review)

| Spec 要求 | Task |
|-----------|------|
| 独立 `video_understand` | 3, 4 |
| Interactions + x-goog-api-key | 2 |
| inline &lt;100MB / Files ≥100MB / YouTube | 1, 3 |
| 多视频 ≤10 + 混用 | 3 |
| `media_resolution` → part `resolution` | 2, 3 |
| 纯 Google、无 OpenAI | 3 |
| 复用 `vision_model` | 3 |
| 前端开关 + i18n | 4 |
| Files 轮询 / 超时 | 1 |
| 时间戳靠 prompt MM:SS | 3 文案 hint |
| Cloud Storage / delete / OpenAI | 明确非目标，无任务 |

**Placeholder scan:** 无 TBD。  
**类型一致性:** `InteractionVideoPart` / `InteractionVideoUnderstandRequest` / `INLINE_MAX_BYTES` 在 Task 2–3 一致。
