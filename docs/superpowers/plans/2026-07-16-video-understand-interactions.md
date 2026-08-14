# Video Understand (Interactions) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增独立工具 `video_understand`：Google Interactions 原生视频理解（本地/http → inline 或 Files API、YouTube、`mode`=qa|summarize|timeline），纯 Google，不走 OpenAI。

**Architecture:** `providers::files_http` 负责 resumable 上传、轮询 `ACTIVE`、可选 delete；`providers::interactions_http` 追加视频理解 body/parse/HTTP；`tools::video_understand` 做参数、`mode` 默认 prompt、输入分流与文案；登记 toolset 与前端开关。

**Tech Stack:** Rust (`providers` / `tools` / `home`)、`reqwest`、`serde_json`、`base64`、schemars；前端 `useAgentTools` + i18n。

**参考:** `docs/superpowers/specs/2026-07-16-video-understand-interactions-design.md`；[视频理解](https://ai.google.dev/gemini-api/docs/video-understanding?hl=zh-cn)

> **修订说明：** 本计划覆盖同日早先草稿（多视频 / `media_resolution`）。以修订后 spec 为准：单一 `video_url`、`mode`、timeline JSON。

## Global Constraints

- Google only：`POST {google_native_base}/v1beta/interactions`，Header `x-goog-api-key`；禁止 `google_openai_base` / `chat/completions` / OpenAI 回退。
- 模型：`creds.vision_model`，空则 `gemini-3.5-flash`（`default_vision_model("google")`）。
- 单一源：`video_url`（工作区路径 | http(s) | YouTube）。
- `mode` ∈ `qa`|`summarize`|`timeline`（默认 `qa`）。
- 本地/下载后分流：`INLINE_MAX_BYTES = 100 * 1024 * 1024`；≥ 阈值走 Files；YouTube 永不 Files。
- `input` 顺序：**先 video part，再 text**。
- Files 轮询间隔 5s，超时 10 分钟；用完后 best-effort `files.delete`。
- 若 `interactions_http.rs` 已存在（TTS / image）：只追加视频符号，不破坏现有 API。

## File Structure

| File | Responsibility |
|------|----------------|
| `crates/agent-providers/src/protocol/files_http.rs` | Files API：upload + ACTIVE 轮询 + 可选 delete |
| `crates/agent-providers/src/protocol/interactions_http.rs` | 追加：`VideoUnderstandMode`、video part/body/parse、`google_interactions_video` |
| `crates/agent-providers/src/protocol/mod.rs` / `lib.rs` | 登记 `files_http` |
| `crates/agent-tools/src/builtin/media/video_understand.rs` | 新工具 |
| `crates/agent-tools/src/builtin/media/mod.rs`、`builtin/mod.rs`、`lib.rs`、`dispatch.rs` | 注册 / dispatch / re-export |
| `home/src/config/tools_enabled.rs` | `KNOWN_TOOLSET_IDS` + 名映射 |
| `apps/desktop/src/hooks/useAgentTools.ts` | 工具开关 |
| `apps/desktop/src/i18n/messages.ts` | 中英 title/desc |
| `tools/tests/tools_test.rs` | 注册表含 `video_understand` |

---

### Task 1: Files API helper

**Files:**
- Create: `crates/agent-providers/src/protocol/files_http.rs`
- Modify: `crates/agent-providers/src/protocol/mod.rs`
- Modify: `crates/agent-providers/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub const INLINE_MAX_BYTES: u64 = 100 * 1024 * 1024;`
  - `pub const FILES_POLL_INTERVAL: Duration = Duration::from_secs(5);`
  - `pub const FILES_POLL_TIMEOUT: Duration = Duration::from_secs(600);`
  - `pub struct UploadedFile { pub name: String, pub uri: String, pub mime_type: String }`
  - `pub enum FileState { Processing, Active { name, uri, mime_type }, Failed }`
  - `pub fn parse_file_status(v: &Value) -> Result<FileState>`
  - `pub fn upload_url_from_start_headers(headers: &reqwest::header::HeaderMap) -> Result<String>`
  - `pub async fn google_files_upload_and_wait(client: &Client, config: &ProviderConfig, bytes: &[u8], mime_type: &str, display_name: &str) -> Result<UploadedFile>`
  - `pub async fn google_files_delete(client: &Client, config: &ProviderConfig, file_name: &str) -> Result<()>`（best-effort 调用方可忽略 Err）
- Consumes: `crate::media_http::google_native_base`；`ProviderConfig`

- [ ] **Step 1: 注册模块**

`crates/agent-providers/src/protocol/mod.rs` 增加：

```rust
pub mod files_http;
```

`crates/agent-providers/src/lib.rs` 的 `pub use protocol::{...}` 加入 `files_http`。

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
        let v = json!({
            "state": "ACTIVE",
            "uri": "https://generativelanguage.googleapis.com/v1beta/files/abc",
            "name": "files/abc",
            "mimeType": "video/mp4"
        });
        match parse_file_status(&v).unwrap() {
            FileState::Active { uri, name, mime_type } => {
                assert!(uri.contains("files/abc"));
                assert_eq!(name, "files/abc");
                assert_eq!(mime_type, "video/mp4");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_failed_and_processing() {
        assert!(matches!(
            parse_file_status(&json!({ "state": "FAILED" })).unwrap(),
            FileState::Failed
        ));
        assert!(matches!(
            parse_file_status(&json!({ "state": "PROCESSING" })).unwrap(),
            FileState::Processing
        ));
        // nested under "file"
        let v = json!({ "file": { "state": "ACTIVE", "name": "files/x", "uri": "u", "mime_type": "video/webm" } });
        match parse_file_status(&v).unwrap() {
            FileState::Active { mime_type, .. } => assert_eq!(mime_type, "video/webm"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn upload_url_from_headers() {
        let mut map = reqwest::header::HeaderMap::new();
        map.insert(
            "x-goog-upload-url",
            "https://example.com/upload?foo=1".parse().unwrap(),
        );
        assert_eq!(
            upload_url_from_start_headers(&map).unwrap(),
            "https://example.com/upload?foo=1"
        );
    }
}
```

- [ ] **Step 3: Run 确认失败**

```bash
cargo test -p providers files_http -- --nocapture
```

Expected: compile fail（模块/符号不存在）或 test fail。

- [ ] **Step 4: 最小实现**

```rust
//! Google Files API（resumable upload + ACTIVE 轮询）。
//! 与 OpenAI 无关；鉴权用 `x-goog-api-key`。

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

#[derive(Debug, Clone)]
pub struct UploadedFile {
    pub name: String,
    pub uri: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

fn native_root(config: &ProviderConfig) -> String {
    trim_slash(&google_native_base(config))
}

fn v1beta_root(config: &ProviderConfig) -> String {
    let n = native_root(config);
    if n.contains("/v1beta") {
        n
    } else {
        format!("{n}/v1beta")
    }
}

/// 从 start 响应头提取可恢复上传 URL。
pub fn upload_url_from_start_headers(headers: &reqwest::header::HeaderMap) -> Result<String> {
    headers
        .get("x-goog-upload-url")
        .or_else(|| headers.get("X-Goog-Upload-URL"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("Files API start 响应缺少 x-goog-upload-url"))
}

fn file_object<'a>(v: &'a Value) -> &'a Value {
    v.get("file").unwrap_or(v)
}

/// 解析 Files get / upload 完成响应当中的 state。
pub fn parse_file_status(v: &Value) -> Result<FileState> {
    let f = file_object(v);
    let state = f
        .get("state")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_ascii_uppercase();
    match state.as_str() {
        "ACTIVE" => {
            let name = f
                .get("name")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let uri = f
                .get("uri")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let mime_type = f
                .get("mimeType")
                .or_else(|| f.get("mime_type"))
                .and_then(|x| x.as_str())
                .unwrap_or("video/mp4")
                .to_string();
            if name.is_empty() || uri.is_empty() {
                anyhow::bail!("ACTIVE 文件缺少 name/uri");
            }
            Ok(FileState::Active {
                name,
                uri,
                mime_type,
            })
        }
        "FAILED" => Ok(FileState::Failed),
        "PROCESSING" | "" => Ok(FileState::Processing),
        other => Ok(FileState::Processing), // 未知当作处理中，避免误杀
        _ => {
            let _ = other;
            Ok(FileState::Processing)
        }
    }
}

/// resumable 上传并等到 ACTIVE。
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
    let key = config.api_key.trim();
    let root = native_root(config);
    let start_url = if root.contains("/upload/") {
        format!("{root}/v1beta/files")
    } else {
        // https://generativelanguage.googleapis.com/upload/v1beta/files
        let host = root
            .trim_end_matches("/v1beta")
            .to_string();
        format!("{host}/upload/v1beta/files")
    };

    let start = client
        .post(&start_url)
        .header("x-goog-api-key", key)
        .header("X-Goog-Upload-Protocol", "resumable")
        .header("X-Goog-Upload-Command", "start")
        .header(
            "X-Goog-Upload-Header-Content-Length",
            bytes.len().to_string(),
        )
        .header("X-Goog-Upload-Header-Content-Type", mime_type)
        .header("content-type", "application/json")
        .json(&json!({ "file": { "display_name": display_name } }))
        .send()
        .await
        .with_context(|| format!("Files API start 失败: {start_url}"))?;
    if !start.status().is_success() {
        let status = start.status();
        let body = start.text().await.unwrap_or_default();
        anyhow::bail!("Files API start HTTP {status}: {body}");
    }
    let upload_url = upload_url_from_start_headers(start.headers())?;

    let uploaded = client
        .post(&upload_url)
        .header("Content-Length", bytes.len().to_string())
        .header("X-Goog-Upload-Offset", "0")
        .header("X-Goog-Upload-Command", "upload, finalize")
        .body(bytes.to_vec())
        .send()
        .await
        .context("Files API upload finalize 失败")?;
    let status = uploaded.status();
    let v: Value = uploaded.json().await.context("解析 Files upload 响应失败")?;
    if !status.is_success() {
        anyhow::bail!(
            "Files API upload HTTP {status}: {}",
            v.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("upload failed")
        );
    }

    // 上传响应可能已是 ACTIVE
    match parse_file_status(&v)? {
        FileState::Active {
            name,
            uri,
            mime_type,
        } => {
            return Ok(UploadedFile {
                name,
                uri,
                mime_type,
            });
        }
        FileState::Failed => anyhow::bail!("Files API 处理失败"),
        FileState::Processing => {}
    }

    let name = file_object(&v)
        .get("name")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow!("upload 响应无 file.name"))?
        .to_string();

    let get_url = format!("{}/{}", v1beta_root(config), name);
    let deadline = Instant::now() + FILES_POLL_TIMEOUT;
    loop {
        if Instant::now() > deadline {
            anyhow::bail!("Files API 轮询超时（等待 ACTIVE）: {name}");
        }
        sleep(FILES_POLL_INTERVAL).await;
        let resp = client
            .get(&get_url)
            .header("x-goog-api-key", key)
            .send()
            .await
            .with_context(|| format!("Files API get 失败: {get_url}"))?;
        let st = resp.status();
        let body: Value = resp.json().await.context("解析 Files get 响应失败")?;
        if !st.is_success() {
            anyhow::bail!("Files API get HTTP {st}");
        }
        match parse_file_status(&body)? {
            FileState::Active {
                name,
                uri,
                mime_type,
            } => {
                return Ok(UploadedFile {
                    name,
                    uri,
                    mime_type,
                });
            }
            FileState::Failed => anyhow::bail!("Files API 处理失败: {name}"),
            FileState::Processing => continue,
        }
    }
}

/// 删除已上传文件（best-effort）。
pub async fn google_files_delete(
    client: &Client,
    config: &ProviderConfig,
    file_name: &str,
) -> Result<()> {
    if config.api_key.trim().is_empty() || file_name.trim().is_empty() {
        anyhow::bail!("delete 参数无效");
    }
    let url = format!("{}/{}", v1beta_root(config), file_name.trim());
    let resp = client
        .delete(&url)
        .header("x-goog-api-key", config.api_key.trim())
        .send()
        .await
        .with_context(|| format!("Files API delete 失败: {url}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Files API delete HTTP {status}: {text}");
    }
    Ok(())
}
```

修掉 `parse_file_status` 里多余的匹配臂（实现时写干净的 `match`，不要重复 `_`）。`other` 未知 state → `Processing`。

- [ ] **Step 5: 跑测试**

```bash
cargo test -p providers files_http -- --nocapture
```

Expected: PASS。

- [ ] **Step 6: Commit**

```bash
git add providers/src/protocol/files_http.rs providers/src/protocol/mod.rs providers/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(providers): add Google Files API upload helper

Resumable upload, ACTIVE polling, optional delete for video_understand.
EOF
)"
```

---

### Task 2: Interactions 视频理解 — body / 解析 / HTTP

**Files:**
- Modify: `crates/agent-providers/src/protocol/interactions_http.rs`

**Interfaces:**
- Produces:
  - `pub enum VideoUnderstandMode { Qa, Summarize, Timeline }` — `as_str()`, `parse`/`FromStr`
  - `pub enum VideoInputPart { Inline { mime_type: String, data_b64: String }, Uri { mime_type: Option<String>, uri: String } }`
  - `pub fn default_video_understand_prompt(mode: VideoUnderstandMode) -> &'static str`
  - `pub fn video_timeline_json_schema() -> Value`
  - `pub fn build_interaction_video_body(model: &str, prompt: &str, video: &VideoInputPart, mode: VideoUnderstandMode) -> Value`
  - `pub fn parse_interaction_video_text(v: &Value) -> Result<String>`（优先 `output_text`，否则 steps）
  - `pub fn try_parse_timeline_events(text: &str) -> Result<Value>`（得到含 `events` 的 JSON `Value`）
  - `pub async fn google_interactions_video(client: &Client, config: &ProviderConfig, model: &str, prompt: &str, video: &VideoInputPart, mode: VideoUnderstandMode) -> Result<String>`
- Consumes: 已有 `interactions_url`；`ProviderConfig`；内部复用与 image 相同的 `error_message`（若为 private，同文件直接用）

- [ ] **Step 1: 写失败测试**

在 `interactions_http.rs` 增加 `#[cfg(test)] mod video_understand_tests`：

```rust
#[cfg(test)]
mod video_understand_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn build_qa_body_video_before_text() {
        let body = build_interaction_video_body(
            "gemini-3.5-flash",
            "summarize please",
            &VideoInputPart::Uri {
                mime_type: Some("video/mp4".into()),
                uri: "https://www.youtube.com/watch?v=abc".into(),
            },
            VideoUnderstandMode::Qa,
        );
        assert_eq!(body["model"], "gemini-3.5-flash");
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 2);
        assert_eq!(input[0]["type"], "video");
        assert_eq!(input[0]["uri"], "https://www.youtube.com/watch?v=abc");
        assert_eq!(input[1]["type"], "text");
        assert_eq!(input[1]["text"], "summarize please");
        assert!(body.get("response_format").is_none());
    }

    #[test]
    fn build_inline_and_timeline_schema() {
        let body = build_interaction_video_body(
            "gemini-3.5-flash",
            "timeline",
            &VideoInputPart::Inline {
                mime_type: "video/mp4".into(),
                data_b64: "YWJj".into(),
            },
            VideoUnderstandMode::Timeline,
        );
        assert_eq!(body["input"][0]["data"], "YWJj");
        assert_eq!(body["input"][0]["mime_type"], "video/mp4");
        assert_eq!(body["response_format"]["type"], "json_schema");
        // 或 mime_type=application/json + schema——与仓库 vision/image 已有风格对齐；
        // 实现时二选一并固定测试断言为实际字段。
        let schema = body
            .pointer("/response_format/schema")
            .or_else(|| body.pointer("/response_format/json_schema/schema"))
            .expect("schema");
        assert!(schema.pointer("/properties/events").is_some());
    }

    #[test]
    fn parse_prefers_output_text() {
        let v = json!({ "output_text": "ok", "steps": [] });
        assert_eq!(parse_interaction_video_text(&v).unwrap(), "ok");
    }

    #[test]
    fn parse_falls_back_to_steps() {
        let v = json!({
            "steps": [{
                "type": "model_output",
                "content": [{ "type": "text", "text": "from-steps" }]
            }]
        });
        assert_eq!(parse_interaction_video_text(&v).unwrap(), "from-steps");
    }

    #[test]
    fn try_parse_timeline_events_ok() {
        let text = r#"{"events":[{"timestamp":"00:05","description":"intro","modality":"both"}]}"#;
        let v = try_parse_timeline_events(text).unwrap();
        assert_eq!(v["events"][0]["timestamp"], "00:05");
    }

    #[test]
    fn try_parse_timeline_events_rejects_missing() {
        assert!(try_parse_timeline_events(r#"{"foo":1}"#).is_err());
    }

    #[test]
    fn mode_parse() {
        assert_eq!(
            "timeline".parse::<VideoUnderstandMode>().unwrap(),
            VideoUnderstandMode::Timeline
        );
        assert!("nope".parse::<VideoUnderstandMode>().is_err());
    }
}
```

> 实现 `response_format` 时与现有 Interactions 文档一致。优先采用：
>
> ```json
> "response_format": {
>   "type": "json_schema",
>   "json_schema": {
>     "name": "video_timeline",
>     "schema": { ... }
>   }
> }
> ```
>
> 若线上 Gemini Interactions 实际要求 `mime_type` + `schema`（与 vision design 一致），以能通的那套为准，并改测试断言。

- [ ] **Step 2: Run 确认失败**

```bash
cargo test -p providers video_understand_tests -- --nocapture
```

Expected: FAIL（符号不存在）。

- [ ] **Step 3: 最小实现（追加到 `interactions_http.rs`）**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoUnderstandMode {
    Qa,
    Summarize,
    Timeline,
}

impl VideoUnderstandMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Qa => "qa",
            Self::Summarize => "summarize",
            Self::Timeline => "timeline",
        }
    }
}

impl std::str::FromStr for VideoUnderstandMode {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "qa" | "" => Ok(Self::Qa),
            "summarize" => Ok(Self::Summarize),
            "timeline" => Ok(Self::Timeline),
            other => anyhow::bail!("未知 video_understand mode: {other}"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum VideoInputPart {
    Inline {
        mime_type: String,
        data_b64: String,
    },
    Uri {
        mime_type: Option<String>,
        uri: String,
    },
}

pub fn default_video_understand_prompt(mode: VideoUnderstandMode) -> &'static str {
    match mode {
        VideoUnderstandMode::Qa => {
            "请概括该视频，并用要点回答关于其内容的问题。引用时刻请用 MM:SS。"
        }
        VideoUnderstandMode::Summarize => {
            "请用 3–5 句话总结该视频，并分别说明关键的视觉与音频要点。引用时刻请用 MM:SS。"
        }
        VideoUnderstandMode::Timeline => {
            "提取该视频的关键事件时间线。每个事件包含 timestamp(MM:SS)、description、modality(visual|audio|both)。"
        }
    }
}

pub fn video_timeline_json_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "events": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "timestamp": { "type": "string" },
                        "description": { "type": "string" },
                        "modality": { "type": "string", "enum": ["visual", "audio", "both"] }
                    },
                    "required": ["timestamp", "description", "modality"]
                }
            }
        },
        "required": ["events"]
    })
}

pub fn build_interaction_video_body(
    model: &str,
    prompt: &str,
    video: &VideoInputPart,
    mode: VideoUnderstandMode,
) -> Value {
    let video_part = match video {
        VideoInputPart::Inline { mime_type, data_b64 } => json!({
            "type": "video",
            "data": data_b64,
            "mime_type": mime_type,
        }),
        VideoInputPart::Uri { mime_type, uri } => {
            let mut p = json!({ "type": "video", "uri": uri });
            if let Some(m) = mime_type.as_ref().filter(|s| !s.is_empty()) {
                p["mime_type"] = json!(m);
            }
            p
        }
    };
    let mut body = json!({
        "model": model,
        "input": [video_part, { "type": "text", "text": prompt }],
    });
    if mode == VideoUnderstandMode::Timeline {
        body["response_format"] = json!({
            "type": "json_schema",
            "json_schema": {
                "name": "video_timeline",
                "schema": video_timeline_json_schema()
            }
        });
    }
    body
}

pub fn parse_interaction_video_text(v: &Value) -> Result<String> {
    if let Some(t) = v.get("output_text").and_then(|x| x.as_str()) {
        let t = t.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    let mut parts = Vec::new();
    if let Some(steps) = v.get("steps").and_then(|s| s.as_array()) {
        for step in steps {
            let content = step
                .get("content")
                .or_else(|| step.pointer("/model_output/content"));
            if let Some(arr) = content.and_then(|c| c.as_array()) {
                for item in arr {
                    if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                        parts.push(t.to_string());
                    }
                }
            } else if let Some(t) = step
                .pointer("/content/0/text")
                .and_then(|t| t.as_str())
            {
                parts.push(t.to_string());
            }
        }
    }
    let joined = parts.join("\n").trim().to_string();
    if joined.is_empty() {
        anyhow::bail!("Interactions 视频理解响应无文本");
    }
    Ok(joined)
}

pub fn try_parse_timeline_events(text: &str) -> Result<Value> {
    let trimmed = text.trim();
    // 允许 markdown fence
    let json_str = if let Some(rest) = trimmed.strip_prefix("```") {
        let rest = rest
            .trim_start_matches("json")
            .trim_start_matches('\n');
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else {
        trimmed
    };
    let v: Value = serde_json::from_str(json_str).context("timeline JSON 解析失败")?;
    if !v.get("events").map(|e| e.is_array()).unwrap_or(false) {
        anyhow::bail!("timeline JSON 缺少 events 数组");
    }
    Ok(v)
}

pub async fn google_interactions_video(
    client: &Client,
    config: &ProviderConfig,
    model: &str,
    prompt: &str,
    video: &VideoInputPart,
    mode: VideoUnderstandMode,
) -> Result<String> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    let url = interactions_url(config);
    let body = build_interaction_video_body(model, prompt, video, mode);
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
    parse_interaction_video_text(&v)
}
```

- [ ] **Step 4: 跑测试**

```bash
cargo test -p providers video_understand_tests -- --nocapture
```

Expected: PASS。顺带：

```bash
cargo test -p providers interactions_http -- --nocapture
```

确保 TTS/image 未回归。

- [ ] **Step 5: Commit**

```bash
git add providers/src/protocol/interactions_http.rs
git commit -m "$(cat <<'EOF'
feat(providers): Google Interactions video understanding helper

Add video body builders, timeline schema, and google_interactions_video.
EOF
)"
```

---

### Task 3: `video_understand` 工具

**Files:**
- Create: `crates/agent-tools/src/builtin/media/video_understand.rs`
- Modify: `crates/agent-tools/src/builtin/media/mod.rs`
- Modify: `crates/agent-tools/src/builtin/mod.rs`
- Modify: `crates/agent-tools/src/lib.rs`（`pub use` + `register_all`）
- Modify: `crates/agent-tools/src/engine/dispatch.rs`
- Modify: `tools/tests/tools_test.rs`

**Interfaces:**
- Produces: `register` / `dispatch`；内部 `classify_video_source`、`mime_from_path`
- Consumes:
  - `files_http::{INLINE_MAX_BYTES, google_files_upload_and_wait, google_files_delete}`
  - `interactions_http::{VideoUnderstandMode, VideoInputPart, default_video_understand_prompt, google_interactions_video, try_parse_timeline_events}`
  - `media_http::default_vision_model`
  - `ToolContext` / `ImageGenCreds`

- [ ] **Step 1: 写工具层纯函数测试（同文件 `#[cfg(test)]`）**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_youtube() {
        assert!(matches!(
            classify_video_source("https://www.youtube.com/watch?v=9hE5-98ZeCg"),
            VideoSourceKind::Youtube
        ));
        assert!(matches!(
            classify_video_source("https://youtu.be/9hE5-98ZeCg"),
            VideoSourceKind::Youtube
        ));
    }

    #[test]
    fn classify_http_and_local() {
        assert!(matches!(
            classify_video_source("https://cdn.example.com/a.mp4"),
            VideoSourceKind::RemoteHttp
        ));
        assert!(matches!(
            classify_video_source("generated/videos/x.mp4"),
            VideoSourceKind::LocalPath
        ));
    }

    #[test]
    fn mime_mp4() {
        assert_eq!(
            mime_from_path(std::path::Path::new("a.MP4")),
            "video/mp4"
        );
        assert_eq!(
            mime_from_path(std::path::Path::new("a.webm")),
            "video/webm"
        );
    }

    #[test]
    fn chooses_inline_under_threshold() {
        assert!(!should_use_files_api(INLINE_MAX_BYTES - 1));
        assert!(should_use_files_api(INLINE_MAX_BYTES));
        assert!(should_use_files_api(INLINE_MAX_BYTES + 1));
    }
}
```

先实现测试引用的分类辅助函数，再写 dispatch。

- [ ] **Step 2: Run 失败**

```bash
cargo test -p tools video_understand -- --nocapture
```

Expected: 模块不存在 → FAIL。

- [ ] **Step 3: 实现工具**

`video_understand.rs` 关键结构（完整实现须覆盖下列逻辑）：

```rust
//! 视频理解：Google Interactions 原生（Files / inline / YouTube）。
//! 不走 OpenAI。

use base64::Engine;
use providers::files_http::{
    google_files_delete, google_files_upload_and_wait, INLINE_MAX_BYTES,
};
use providers::interactions_http::{
    default_video_understand_prompt, google_interactions_video, try_parse_timeline_events,
    VideoInputPart, VideoUnderstandMode,
};
use providers::media_http::default_vision_model;
use providers::trait_::ProviderConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct VideoUnderstandArgs {
    /// 工作区相对路径、http(s) 直链，或公开 YouTube URL。
    pub video_url: String,
    #[serde(default)]
    pub prompt: Option<String>,
    /// `qa` | `summarize` | `timeline`；默认 `qa`。
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VideoSourceKind {
    Youtube,
    RemoteHttp,
    LocalPath,
}

fn classify_video_source(url: &str) -> VideoSourceKind {
    let lower = url.trim().to_ascii_lowercase();
    if lower.contains("youtube.com/") || lower.contains("youtu.be/") {
        return VideoSourceKind::Youtube;
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return VideoSourceKind::RemoteHttp;
    }
    VideoSourceKind::LocalPath
}

fn should_use_files_api(len: u64) -> bool {
    len >= INLINE_MAX_BYTES
}

fn mime_from_path(path: &std::path::Path) -> &'static str {
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

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "video_understand".into(),
        toolset: "video_understand".into(),
        description: "Analyze a video with Google Gemini Interactions (workspace path, http(s), or public YouTube). Modes: qa, summarize, timeline (JSON events). Supports MM:SS timestamps in the prompt. Google only.".into(),
        schema: schema_for_args::<VideoUnderstandArgs>(),
        check_fn: None,
        icon: "film",
    });
}

pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: VideoUnderstandArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("video_understand 参数无效: {e}"))?;
    let video_url = parsed.video_url.trim();
    if video_url.is_empty() {
        anyhow::bail!("video_understand 需要 video_url");
    }
    let mode: VideoUnderstandMode = parsed
        .mode
        .as_deref()
        .unwrap_or("qa")
        .parse()?;
    let prompt = parsed
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_video_understand_prompt(mode))
        .to_string();

    let Some(creds) = ctx.image_gen_targets.google() else {
        anyhow::bail!("video_understand 需要 Google API Key（不支持 OpenAI）");
    };
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

    let (video_part, input_kind, uploaded_name) =
        resolve_video_input(&client, &config, ctx, video_url).await?;

    let text = match google_interactions_video(
        &client, &config, &model, &prompt, &video_part, mode,
    )
    .await
    {
        Ok(t) => t,
        Err(e) => {
            if let Some(name) = uploaded_name.as_deref() {
                let _ = google_files_delete(&client, &config, name).await;
            }
            return Err(e);
        }
    };

    if let Some(name) = uploaded_name.as_deref() {
        let _ = google_files_delete(&client, &config, name).await;
    }

    let body = if mode == VideoUnderstandMode::Timeline {
        match try_parse_timeline_events(&text) {
            Ok(v) => format!(
                "{}\n\nprovider=google\nmodel={model}\nmode=timeline\ninput={input_kind}",
                serde_json::to_string_pretty(&v).unwrap_or(text)
            ),
            Err(_) => format!(
                "{text}\n\nprovider=google\nmodel={model}\nmode=timeline\ninput={input_kind}\nparse_error=true"
            ),
        }
    } else {
        format!(
            "{text}\n\nprovider=google\nmodel={model}\nmode={}\ninput={input_kind}\nhint: 引用时间点请用 MM:SS（如 01:15）",
            mode.as_str()
        )
    };
    Ok(body)
}

async fn resolve_video_input(
    client: &reqwest::Client,
    config: &ProviderConfig,
    ctx: &ToolContext<'_>,
    video_url: &str,
) -> anyhow::Result<(VideoInputPart, &'static str, Option<String>)> {
    match classify_video_source(video_url) {
        VideoSourceKind::Youtube => Ok((
            VideoInputPart::Uri {
                mime_type: None,
                uri: video_url.to_string(),
            },
            "youtube",
            None,
        )),
        VideoSourceKind::RemoteHttp => {
            let bytes = download_bytes(client, video_url).await?;
            bytes_to_part(client, config, &bytes, mime_from_url(video_url), "remote").await
        }
        VideoSourceKind::LocalPath => {
            let path = ctx.workspace_dir.join(video_url);
            if !path.exists() {
                anyhow::bail!("本地文件不存在: {}", path.display());
            }
            let bytes = std::fs::read(&path)
                .map_err(|e| anyhow::anyhow!("读取视频失败 {}: {e}", path.display()))?;
            let mime = mime_from_path(&path);
            let display = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("video");
            bytes_to_part(client, config, &bytes, mime, display).await
        }
    }
}

async fn bytes_to_part(
    client: &reqwest::Client,
    config: &ProviderConfig,
    bytes: &[u8],
    mime: &str,
    display_name: &str,
) -> anyhow::Result<(VideoInputPart, &'static str, Option<String>)> {
    if should_use_files_api(bytes.len() as u64) {
        let uploaded =
            google_files_upload_and_wait(client, config, bytes, mime, display_name).await?;
        Ok((
            VideoInputPart::Uri {
                mime_type: Some(uploaded.mime_type),
                uri: uploaded.uri,
            },
            "file_api",
            Some(uploaded.name),
        ))
    } else {
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        Ok((
            VideoInputPart::Inline {
                mime_type: mime.to_string(),
                data_b64: b64,
            },
            "inline",
            None,
        ))
    }
}

async fn download_bytes(client: &reqwest::Client, url: &str) -> anyhow::Result<Vec<u8>> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("下载视频失败: {e}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("下载视频 HTTP {}", resp.status());
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| anyhow::anyhow!("读取视频字节失败: {e}"))?;
    Ok(bytes.to_vec())
}

fn mime_from_url(url: &str) -> &'static str {
    let path = url.split('?').next().unwrap_or(url);
    mime_from_path(std::path::Path::new(path))
}
```

接线：

`media/mod.rs`:

```rust
pub mod video_understand;
```

`builtin/mod.rs`:

```rust
pub use media::{image_gen, music, tts, video_gen, video_understand, vision};
```

`lib.rs` `pub(crate) use` 加入 `video_understand`；`register_all` 在 `video_gen` 后调用 `video_understand::register`。

`dispatch.rs`:

```rust
"video_understand" => crate::video_understand::dispatch(ctx, args).await,
```

`tools_test.rs` 期望名字列表加入 `"video_understand"`。

- [ ] **Step 4: 测试**

```bash
cargo test -p tools video_understand -- --nocapture
cargo test -p tools --test tools_test -- --nocapture
```

Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add tools/src/builtin/media/video_understand.rs tools/src/builtin/media/mod.rs tools/src/builtin/mod.rs tools/src/lib.rs tools/src/engine/dispatch.rs tools/tests/tools_test.rs
git commit -m "$(cat <<'EOF'
feat(tools): add video_understand via Gemini Interactions

Google-only path with File API / inline / YouTube and timeline mode.
EOF
)"
```

---

### Task 4: 工具开关 + 前端 + i18n

**Files:**
- Modify: `home/src/config/tools_enabled.rs`
- Modify: `apps/desktop/src/hooks/useAgentTools.ts`
- Modify: `apps/desktop/src/i18n/messages.ts`

- [ ] **Step 1: `KNOWN_TOOLSET_IDS` 与映射**

在 `"video_gen"` 后加入 `"video_understand"`。

`tool_name_to_toolset`：

```rust
"video_understand" => "video_understand",
```

- [ ] **Step 2: AGENT_TOOLS**

在 `video_gen` 条目后插入（可复用 `IconVideoGen` 或现有 film 类图标；若无合适 icon 则临时复用 `IconEye`/`IconVideoGen`）：

```ts
{
  id: "video_understand",
  titleKey: "agentTools.videoUnderstand.title",
  descKey: "agentTools.videoUnderstand.desc",
  Icon: IconVideoGen,
  tone: "rose",
  params: [
    { name: "video_url", type: "string" },
    { name: "prompt", type: "string", optional: true },
    { name: "mode", type: "string", optional: true },
  ],
},
```

同步扩展 `AgentToolId` union 类型加入 `"video_understand"`。

- [ ] **Step 3: i18n（中英）**

中文：

```ts
"agentTools.videoUnderstand.title": "视频理解",
"agentTools.videoUnderstand.desc": "Google Gemini 原生：本地/链接/YouTube 视频问答与时间线（Interactions；支持 MM:SS）",
```

英文：

```ts
"agentTools.videoUnderstand.title": "Video Understanding",
"agentTools.videoUnderstand.desc": "Google Gemini native: ask about workspace/URL/YouTube videos (Interactions; MM:SS timestamps; modes qa/summarize/timeline)",
```

- [ ] **Step 4: 编译检查**

```bash
cargo check -p home -p tools -p providers
```

Expected: 无错误。前端若有 `tsc`：

```bash
cd frontend && npx tsc --noEmit
```

（若项目习惯不加严 tsc，至少保证 messages key 与 hooks 一致。）

- [ ] **Step 5: Commit**

```bash
git add home/src/config/tools_enabled.rs apps/desktop/src/hooks/useAgentTools.ts apps/desktop/src/i18n/messages.ts
git commit -m "$(cat <<'EOF'
feat: enable video_understand toolset in UI and config

Register KNOWN_TOOLSET_IDS, AgentTools toggle, and i18n copy.
EOF
)"
```

---

### Task 5: 端到端验收对照

**Files:** 无新文件（手测 / 单测汇总）

- [ ] **Step 1: 跑全量相关测试**

```bash
cargo test -p providers files_http video_understand_tests -- --nocapture
cargo test -p tools video_understand -- --nocapture
cargo test -p tools --test tools_test -- --nocapture
```

Expected: 全 PASS。

- [ ] **Step 2: 对照验收清单（spec）**

| 验收项 | 验证方式 |
|--------|----------|
| `<100MB` inline | 单测 `should_use_files_api` + body inline |
| `≥100MB` File API | 阈值单测 + `files_http` parse ACTIVE |
| YouTube | `classify_youtube` + body uri |
| modes | body schema 仅 timeline；工具文案含 mode |
| 元信息 | dispatch 输出含 `provider=google` / `input=` |
| 无 Key | dispatch 立即错误文案含 Google |
| 面板开关 | `AGENT_TOOLS` id 存在 |

- [ ] **Step 3:（可选）真机冒烟** — 有 Key 时对短 mp4 / YouTube 各跑一次 `video_understand`；不强制写入 CI。

- [ ] **Step 4: 若有文档缺口，更新 spec 勾选状态**（可选小 commit）

---

## Spec coverage (self-review)

| Spec 要求 | Task |
|-----------|------|
| 独立 `video_understand` | 3, 4 |
| Interactions + `x-goog-api-key` | 2 |
| File API + ACTIVE 轮询 | 1 |
| inline &lt;100MB | 1 常量 + 3 分流 |
| YouTube | 3 |
| http(s) 下载再分流 | 3 |
| mode qa/summarize/timeline | 2, 3 |
| timeline JSON + parse_error | 2, 3 |
| 仅 Google / 禁 OpenAI | Global + 3 |
| best-effort delete | 1, 3 |
| 复用 vision_model | 3 |
| 工具面板 / i18n | 4 |
| 不做多视频 / media_resolution | 刻意省略 |
| 不做聊天多模态附件 | 刻意省略 |

## Placeholder scan

无 TBD；`response_format` 字段名允许与线上 API 对齐后微调测试断言（Task 2 已注明）。

## Type consistency

- `VideoUnderstandMode` / `VideoInputPart`：Task 2 产出，Task 3 消费。
- `UploadedFile.name` → `google_files_delete`。
- `INLINE_MAX_BYTES`：Task 1 定义，Task 3 `should_use_files_api` 使用。
