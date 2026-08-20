# Audio Understand Interactions 原生 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增 `audio_understand` 工具：Google 走 Gemini Interactions 原生音频理解（describe + 结构化 transcribe，含 YouTube），OpenAI 分路径（describe→Chat `input_audio`，transcribe→Whisper）并尽力 JSON 兜底。

**Architecture:** 在已有 `providers::interactions_http`（TTS）上追加音频理解 body/parse/`google_interactions_audio`；在 `media_http` 增加仅 OpenAI 的 Whisper 与 audio-describe helpers；`tools` 新建 `audio_understand` 做参数解析、mime/YouTube 判定与凭证分流。Google 禁止经 `google_openai_base`。

**Tech Stack:** Rust (`providers` / `tools` / `home`)、`reqwest`（含 multipart）、`serde_json`、`base64`、schemars；前端 `useAgentTools` + i18n。

**参考:** `docs/superpowers/specs/2026-07-16-audio-understand-interactions-native-design.md`；[音频理解](https://ai.google.dev/gemini-api/docs/audio?hl=zh-cn)

## Global Constraints

- Google：`POST {google_native_base}/v1beta/interactions`，Header `x-goog-api-key`（不用 Bearer）。
- OpenAI：`describe` → `POST …/chat/completions`（Bearer + `input_audio`）；`transcribe` → `POST …/audio/transcriptions`（multipart Whisper）。两路径与 Google 请求体完全分离。
- 默认模型：Google `gemini-3.5-flash`（可读 `creds.vision_model`）；OpenAI describe `gpt-4o`；Whisper `whisper-1`。
- 凭证顺序：Google → OpenAI（含聊天 OpenAI / `OPENAI_API_KEY`）。
- 工具：单一 `audio_understand`；`mode` ∈ `describe`|`transcribe`（默认 describe）；单输入 `audio_url`；可选 `start`/`end`（`MM:SS`）。
- YouTube（主机含 `youtube.com` / `youtu.be`）仅 Google，Interactions 用 `type: video` + `mime_type: video/mp4`。
- Transcribe schema：`summary` + `segments[].{speaker,timestamp,content,emotion}` 必填；`language`/`translation` 建议有。
- 本轮不接 Files API、不改 ProvidersPanel、不改聊天附件多模态。
- 若 `interactions_http.rs` 已有 TTS：只追加音频理解符号，不破坏 TTS API。

## File Structure

| File | Responsibility |
|------|----------------|
| `crates/agent-providers/src/protocol/interactions_http.rs` | `AudioUnderstandMode`、媒体 part、body/parse、`google_interactions_audio` |
| `crates/agent-providers/src/protocol/media_http.rs` | `default_whisper_model`、OpenAI describe body/HTTP、Whisper multipart、JSON 尽力组装 |
| `crates/agent-tools/src/builtin/media/audio_understand.rs` | 新工具契约与分流 |
| `crates/agent-tools/src/builtin/media/mod.rs` | `pub mod audio_understand` |
| `crates/agent-tools/src/builtin/mod.rs` / `crates/agent-tools/src/lib.rs` | re-export + `register_all` |
| `crates/agent-tools/src/engine/dispatch.rs` | `match` 分支 |
| `home/src/config/tools_enabled.rs` | `KNOWN_TOOLSET_IDS` + `tool_name_to_toolset` |
| `apps/desktop/src/hooks/useAgentTools.ts` | 工具卡片 |
| `apps/desktop/src/components/ToolIcons.tsx` | `IconAudioUnderstand`（波形/耳机简图标） |
| `apps/desktop/src/i18n/messages.ts` | 中英 title/desc |

---

### Task 1: Interactions 音频理解 — body、schema、解析、HTTP

**Files:**
- Modify: `crates/agent-providers/src/protocol/interactions_http.rs`
- Verify: `crates/agent-providers/src/protocol/mod.rs` 已有 `pub mod interactions_http`
- Verify: `crates/agent-providers/src/lib.rs` 已 re-export `interactions_http`

**Interfaces:**
- Produces:
  - `pub enum AudioUnderstandMode { Describe, Transcribe }` — `as_str()`, `parse(s: &str) -> Result<Self>`（空/`describe`→Describe；`transcribe`→Transcribe；其它 bail）
  - `pub enum AudioMediaPart { Inline { media_type: AudioMediaKind, mime_type: String, data_b64: String }, Uri { media_type: AudioMediaKind, mime_type: String, uri: String } }`
  - `pub enum AudioMediaKind { Audio, Video }` — Interactions `type` 字段：`audio` / `video`
  - `pub fn audio_transcribe_json_schema() -> serde_json::Value`
  - `pub fn default_audio_understand_prompt(mode: AudioUnderstandMode) -> &'static str`
  - `pub fn build_interaction_audio_body(model: &str, prompt: &str, media: &AudioMediaPart, mode: AudioUnderstandMode) -> serde_json::Value`
  - `pub fn parse_interaction_audio_text(v: &serde_json::Value) -> anyhow::Result<String>`（优先 `output_text`，否则拼 `steps[].model_output` text；可与其它 Interactions text 解析 DRY，但对外符号用此名）
  - `pub async fn google_interactions_audio(client: &reqwest::Client, prompt: &str, media: &AudioMediaPart, mode: AudioUnderstandMode, config: &ProviderConfig) -> anyhow::Result<String>`
- Consumes: `interactions_url`、`google_native_base`（间接）、`ProviderConfig`、`error_message`（文件内已有可复用）

- [x] **Step 1: 写失败测试（`interactions_http.rs` 底部新 `mod audio_understand_tests`）**

```rust
#[cfg(test)]
mod audio_understand_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn build_describe_inline_audio() {
        let media = AudioMediaPart::Inline {
            media_type: AudioMediaKind::Audio,
            mime_type: "audio/mp3".into(),
            data_b64: "YWJj".into(),
        };
        let body = build_interaction_audio_body(
            "gemini-3.5-flash",
            "describe please",
            &media,
            AudioUnderstandMode::Describe,
        );
        assert_eq!(body["model"], "gemini-3.5-flash");
        let input = body["input"].as_array().unwrap();
        assert_eq!(input[0]["type"], "text");
        assert_eq!(input[0]["text"], "describe please");
        assert_eq!(input[1]["type"], "audio");
        assert_eq!(input[1]["data"], "YWJj");
        assert_eq!(input[1]["mime_type"], "audio/mp3");
        assert!(body.get("response_format").is_none());
    }

    #[test]
    fn build_youtube_as_video_uri() {
        let media = AudioMediaPart::Uri {
            media_type: AudioMediaKind::Video,
            mime_type: "video/mp4".into(),
            uri: "https://www.youtube.com/watch?v=ku-N-eS1lgM".into(),
        };
        let body = build_interaction_audio_body(
            "gemini-3.5-flash",
            "transcribe",
            &media,
            AudioUnderstandMode::Transcribe,
        );
        let input = body["input"].as_array().unwrap();
        assert_eq!(input[1]["type"], "video");
        assert_eq!(input[1]["uri"], "https://www.youtube.com/watch?v=ku-N-eS1lgM");
        assert_eq!(input[1]["mime_type"], "video/mp4");
        assert_eq!(body["response_format"]["mime_type"], "application/json");
        let props = &body["response_format"]["schema"]["properties"];
        assert!(props.get("summary").is_some());
        assert!(props["segments"]["items"]["properties"].get("emotion").is_some());
        assert!(props["segments"]["items"]["properties"].get("speaker").is_some());
    }

    #[test]
    fn parse_prefers_output_text() {
        let v = json!({ "output_text": "ok audio", "steps": [] });
        assert_eq!(parse_interaction_audio_text(&v).unwrap(), "ok audio");
    }

    #[test]
    fn parse_falls_back_to_steps() {
        let v = json!({
            "steps": [{
                "type": "model_output",
                "content": [
                    { "type": "text", "text": "hello " },
                    { "type": "text", "text": "audio" }
                ]
            }]
        });
        assert_eq!(parse_interaction_audio_text(&v).unwrap(), "hello audio");
    }

    #[test]
    fn mode_parse() {
        assert_eq!(
            AudioUnderstandMode::parse("").unwrap(),
            AudioUnderstandMode::Describe
        );
        assert_eq!(
            AudioUnderstandMode::parse("transcribe").unwrap(),
            AudioUnderstandMode::Transcribe
        );
        assert!(AudioUnderstandMode::parse("detect").is_err());
    }
}
```

- [x] **Step 2: Run 确认失败**

Run: `cargo test -p providers audio_understand_tests -- --nocapture`  
Expected: FAIL（符号不存在）

- [x] **Step 3: 实现类型与函数**

追加到 `interactions_http.rs`（保留 TTS 不动）：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioUnderstandMode {
    Describe,
    Transcribe,
}

impl AudioUnderstandMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Describe => "describe",
            Self::Transcribe => "transcribe",
        }
    }

    pub fn parse(s: &str) -> anyhow::Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "describe" => Ok(Self::Describe),
            "transcribe" => Ok(Self::Transcribe),
            other => anyhow::bail!("audio_understand mode 无效: {other}（支持 describe|transcribe）"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioMediaKind {
    Audio,
    Video,
}

impl AudioMediaKind {
    fn as_type_str(self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::Video => "video",
        }
    }
}

#[derive(Debug, Clone)]
pub enum AudioMediaPart {
    Inline {
        media_type: AudioMediaKind,
        mime_type: String,
        data_b64: String,
    },
    Uri {
        media_type: AudioMediaKind,
        mime_type: String,
        uri: String,
    },
}

pub fn default_audio_understand_prompt(mode: AudioUnderstandMode) -> &'static str {
    match mode {
        AudioUnderstandMode::Describe => "请描述这段音频",
        AudioUnderstandMode::Transcribe => {
            "Process the audio and generate a detailed transcription. \
Identify distinct speakers (Speaker 1, Speaker 2, …). \
Provide timestamps MM:SS. Detect language; if not English provide English translation in translation. \
emotion must be one of happy, sad, angry, neutral. Include a brief summary."
        }
    }
}

pub fn audio_transcribe_json_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "summary": { "type": "string" },
            "segments": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "speaker": { "type": "string" },
                        "timestamp": { "type": "string" },
                        "content": { "type": "string" },
                        "language": { "type": "string" },
                        "translation": { "type": "string" },
                        "emotion": {
                            "type": "string",
                            "enum": ["happy", "sad", "angry", "neutral"]
                        }
                    },
                    "required": ["speaker", "timestamp", "content", "emotion"]
                }
            }
        },
        "required": ["summary", "segments"]
    })
}

pub fn build_interaction_audio_body(
    model: &str,
    prompt: &str,
    media: &AudioMediaPart,
    mode: AudioUnderstandMode,
) -> Value {
    let media_json = match media {
        AudioMediaPart::Inline {
            media_type,
            mime_type,
            data_b64,
        } => json!({
            "type": media_type.as_type_str(),
            "data": data_b64,
            "mime_type": mime_type,
        }),
        AudioMediaPart::Uri {
            media_type,
            mime_type,
            uri,
        } => json!({
            "type": media_type.as_type_str(),
            "uri": uri,
            "mime_type": mime_type,
        }),
    };
    let mut body = json!({
        "model": model,
        "input": [
            { "type": "text", "text": prompt },
            media_json
        ]
    });
    if mode == AudioUnderstandMode::Transcribe {
        body["response_format"] = json!({
            "mime_type": "application/json",
            "schema": audio_transcribe_json_schema()
        });
        // 若官方 Interactions 使用 type+schema 形态，兼容并列：
        // 实现时以当前 Gemini 文档/已落地 vision body 为准，选一种并让测试锁定。
    }
    body
}

pub fn parse_interaction_audio_text(v: &Value) -> Result<String> {
    if let Some(t) = v.get("output_text").and_then(|x| x.as_str()) {
        let t = t.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    let mut out = String::new();
    if let Some(steps) = v.get("steps").and_then(|s| s.as_array()) {
        for step in steps {
            let ty = step
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("");
            if ty != "model_output" && ty != "content" {
                // 仍尝试读 content
            }
            if let Some(content) = step.get("content").and_then(|c| c.as_array()) {
                for part in content {
                    if part.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(t) = part.get("text").and_then(|x| x.as_str()) {
                            out.push_str(t);
                        }
                    }
                }
            }
        }
    }
    let out = out.trim().to_string();
    if out.is_empty() {
        anyhow::bail!("Google interactions 音频响应无文本");
    }
    Ok(out)
}

pub async fn google_interactions_audio(
    client: &Client,
    prompt: &str,
    media: &AudioMediaPart,
    mode: AudioUnderstandMode,
    config: &ProviderConfig,
) -> Result<String> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    let model = if config.model.trim().is_empty() {
        "gemini-3.5-flash".to_string()
    } else {
        config.model.trim().to_string()
    };
    let url = interactions_url(config);
    let body = build_interaction_audio_body(&model, prompt, media, mode);
    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .header("x-goog-api-key", config.api_key.trim())
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 Google interactions 音频理解失败: {url}"))?;
    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析 Google interactions 音频理解响应失败")?;
    if !status.is_success() {
        anyhow::bail!(
            "Google interactions HTTP {status}: {}",
            error_message(&v)
        );
    }
    parse_interaction_audio_text(&v)
}
```

注意：`response_format` 形状若与 vision 计划已落地版本不一致，**以同文件里 vision（若已存在）或 TTS 旁注释的官方 Interactions 形态为准**，修改测试断言使之一致，禁止 Google 走 openai 兼容。

- [x] **Step 4: Run 测试通过**

Run: `cargo test -p providers audio_understand_tests -- --nocapture`  
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add providers/src/protocol/interactions_http.rs
git commit -m "$(cat <<'EOF'
feat(providers): Google Interactions audio understand helper

Add describe/transcribe body builders and HTTP for audio/video inputs.

EOF
)"
```

---

### Task 2: OpenAI Whisper + Chat audio describe（独立路径）

**Files:**
- Modify: `crates/agent-providers/src/protocol/media_http.rs`

**Interfaces:**
- Produces:
  - `pub fn default_whisper_model() -> &'static str` → `"whisper-1"`
  - `pub(crate) fn build_openai_audio_describe_body(model: &str, prompt: &str, audio_b64: &str, format: &str) -> Value`
  - `pub async fn openai_audio_describe(client: &Client, prompt: &str, audio_bytes: &[u8], mime_or_format: &str, config: &ProviderConfig) -> Result<String>`
  - `pub async fn openai_audio_transcriptions(client: &Client, audio_bytes: &[u8], filename: &str, config: &ProviderConfig) -> Result<String>`（返回 Whisper 纯文本）
  - `pub fn whisper_text_to_transcribe_json(text: &str) -> Value`（尽力：`summary`=截断前 200 字或全文；单 segment：`speaker:"Speaker 1"`, `timestamp:"00:00"`, `content`:text, `emotion:"neutral"`；整体带标记由工具层加 `fallback=openai`）
- 行为：
  - **绝不**因 Google base 改走 `google_openai_base`；仅 `openai_compatible_base`（默认 `https://api.openai.com/v1`）。
  - describe：`messages[0].content` = `[{type:text},{type:input_audio,input_audio:{data,format}}]`；`format` 从 mime 映射：`wav|mp3|mp4|mpeg|m4a|webm`（未知用 `mp3`）。
  - Whisper：`multipart/form-data`：`file` + `model`。

- [x] **Step 1: 写失败测试**

```rust
#[test]
fn openai_audio_describe_body_has_input_audio() {
    let body = build_openai_audio_describe_body("gpt-4o", "hi", "AAAA", "mp3");
    assert_eq!(body["model"], "gpt-4o");
    let content = body["messages"][0]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[1]["type"], "input_audio");
    assert_eq!(content[1]["input_audio"]["data"], "AAAA");
    assert_eq!(content[1]["input_audio"]["format"], "mp3");
}

#[test]
fn whisper_text_to_json_single_segment() {
    let v = whisper_text_to_transcribe_json("hello world");
    assert_eq!(v["segments"][0]["content"], "hello world");
    assert_eq!(v["segments"][0]["emotion"], "neutral");
    assert!(v["summary"].as_str().unwrap().contains("hello"));
}

#[test]
fn default_whisper_is_whisper1() {
    assert_eq!(default_whisper_model(), "whisper-1");
}
```

- [x] **Step 2: Run 确认失败后实现**

```rust
pub fn default_whisper_model() -> &'static str {
    "whisper-1"
}

pub(crate) fn audio_format_from_mime(mime: &str) -> &'static str {
    let m = mime.to_ascii_lowercase();
    if m.contains("wav") {
        "wav"
    } else if m.contains("mp4") || m.contains("m4a") {
        "mp4"
    } else if m.contains("webm") {
        "webm"
    } else if m.contains("mpeg") {
        "mpeg"
    } else {
        "mp3"
    }
}

pub(crate) fn build_openai_audio_describe_body(
    model: &str,
    prompt: &str,
    audio_b64: &str,
    format: &str,
) -> Value {
    json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": prompt },
                {
                    "type": "input_audio",
                    "input_audio": {
                        "data": audio_b64,
                        "format": format
                    }
                }
            ]
        }]
    })
}

pub fn whisper_text_to_transcribe_json(text: &str) -> Value {
    let t = text.trim();
    let summary = if t.chars().count() > 200 {
        t.chars().take(200).collect::<String>()
    } else {
        t.to_string()
    };
    json!({
        "summary": summary,
        "segments": [{
            "speaker": "Speaker 1",
            "timestamp": "00:00",
            "content": t,
            "emotion": "neutral"
        }]
    })
}

pub async fn openai_audio_describe(
    client: &Client,
    prompt: &str,
    audio_bytes: &[u8],
    mime_or_format: &str,
    config: &ProviderConfig,
) -> Result<String> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("API Key 为空");
    }
    if audio_bytes.is_empty() {
        anyhow::bail!("音频为空");
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
    let b64 = base64::engine::general_purpose::STANDARD.encode(audio_bytes);
    let format = audio_format_from_mime(mime_or_format);
    let body = build_openai_audio_describe_body(model, prompt, &b64, format);
    // POST bearer_auth；解析 choices[0].message.content（复用现有 vision content 提取逻辑时可抽小函数）
    // …
}

pub async fn openai_audio_transcriptions(
    client: &Client,
    audio_bytes: &[u8],
    filename: &str,
    config: &ProviderConfig,
) -> Result<String> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("API Key 为空");
    }
    if audio_bytes.is_empty() {
        anyhow::bail!("音频为空");
    }
    let model = if config.model.trim().is_empty() {
        default_whisper_model()
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
    let url = format!("{base}/audio/transcriptions");
    let part = reqwest::multipart::Part::bytes(audio_bytes.to_vec())
        .file_name(filename.to_string())
        .mime_str("application/octet-stream")
        .unwrap_or_else(|_| reqwest::multipart::Part::bytes(audio_bytes.to_vec()));
    let form = reqwest::multipart::Form::new()
        .text("model", model.to_string())
        .part("file", part);
    let response = client
        .post(&url)
        .bearer_auth(&config.api_key)
        .multipart(form)
        .send()
        .await
        .with_context(|| format!("连接 OpenAI Whisper 失败: {url}"))?;
    let status = response.status();
    let v: Value = response.json().await.context("解析 Whisper 响应失败")?;
    if !status.is_success() {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("Whisper 请求失败");
        anyhow::bail!("Whisper HTTP {status}: {msg}");
    }
    v.get("text")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("Whisper 响应无 text"))
}
```

- [x] **Step 3: Run**

Run: `cargo test -p providers openai_audio_describe_body -- --nocapture`  
Run: `cargo test -p providers whisper_text_to_json -- --nocapture`  
Expected: PASS  
Run: `cargo test -p providers --lib`  
Expected: PASS

- [x] **Step 4: Commit**

```bash
git add providers/src/protocol/media_http.rs
git commit -m "$(cat <<'EOF'
feat(providers): OpenAI audio describe and Whisper helpers

Separate Chat input_audio describe from Whisper transcriptions;
keep Google out of openai-compatible audio path.

EOF
)"
```

---

### Task 3: 工具 `audio_understand` + 注册 / 分发 / tools-enabled

**Files:**
- Create: `crates/agent-tools/src/builtin/media/audio_understand.rs`
- Modify: `crates/agent-tools/src/builtin/media/mod.rs`
- Modify: `crates/agent-tools/src/builtin/mod.rs`
- Modify: `crates/agent-tools/src/lib.rs`
- Modify: `crates/agent-tools/src/engine/dispatch.rs`
- Modify: `home/src/config/tools_enabled.rs`

**Interfaces:**
- Consumes: Task 1/2 全部公开符号；`ToolContext` / `ImageGenCreds`；`default_vision_model`
- Produces: `register` / `dispatch`；toolset id `audio_understand`

- [x] **Step 1: 模块挂钩**

`media/mod.rs`：

```rust
pub mod audio_understand;
pub mod image_gen;
// …其余不变
```

`builtin/mod.rs`：

```rust
pub use media::{audio_understand, image_gen, music, tts, video_gen, vision};
```

`lib.rs` `pub(crate) use builtins::{…}` 加入 `audio_understand`；`register_all` 在 `vision::register` 旁调用 `audio_understand::register(registry)`。

`dispatch.rs` match 增加：

```rust
"audio_understand" => crate::audio_understand::dispatch(ctx, args).await,
```

`tools_enabled.rs`：

```rust
// KNOWN_TOOLSET_IDS 在 "vision" 后加入：
"audio_understand",

// tool_name_to_toolset：
"audio_understand" => "audio_understand",
```

- [x] **Step 2: 实现 `audio_understand.rs`**

```rust
//! 音频理解：Google Interactions 原生；OpenAI Chat describe / Whisper transcribe。
//!
//! 参考：https://ai.google.dev/gemini-api/docs/audio?hl=zh-cn

use base64::Engine;
use providers::http_stream::openai_compatible_base;
use providers::interactions_http::{
    default_audio_understand_prompt, google_interactions_audio, AudioMediaKind, AudioMediaPart,
    AudioUnderstandMode,
};
use providers::media_http::{
    default_vision_model, default_whisper_model, openai_audio_describe,
    openai_audio_transcriptions, whisper_text_to_transcribe_json,
};
use providers::trait_::ProviderConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::{ImageGenCreds, ToolContext};
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AudioUnderstandArgs {
    /// 工作区相对路径、http(s) 音频 URL、或 YouTube URL。
    pub audio_url: String,
    #[serde(default)]
    pub prompt: Option<String>,
    /// describe | transcribe；缺省 describe。
    #[serde(default)]
    pub mode: Option<String>,
    /// 可选时间窗起点 MM:SS。
    #[serde(default)]
    pub start: Option<String>,
    /// 可选时间窗终点 MM:SS。
    #[serde(default)]
    pub end: Option<String>,
}

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "audio_understand".to_string(),
        toolset: "audio_understand".to_string(),
        description: "Analyze audio. Modes: describe (default), transcribe (structured JSON with speakers/timestamps/emotion). Pass audio_url (workspace path, http(s), or YouTube). Google uses Interactions API; OpenAI uses chat input_audio for describe and Whisper for transcribe."
            .to_string(),
        schema: schema_for_args::<AudioUnderstandArgs>(),
        check_fn: None,
        icon: "audio-lines", // 若 icon 体系仅字符串且未知则用 "ear" / "music"；与 ToolIcons 映射对齐
    });
}

pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: AudioUnderstandArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("audio_understand 参数无效: {e}"))?;
    let audio_url = parsed.audio_url.trim();
    if audio_url.is_empty() {
        anyhow::bail!("audio_understand 需要 audio_url");
    }
    let mode = AudioUnderstandMode::parse(parsed.mode.as_deref().unwrap_or(""))?;
    validate_mmss(parsed.start.as_deref())?;
    validate_mmss(parsed.end.as_deref())?;

    let mut prompt = parsed
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_audio_understand_prompt(mode))
        .to_string();
    if let (Some(s), Some(e)) = (parsed.start.as_deref(), parsed.end.as_deref()) {
        prompt.push_str(&format!("\nProvide content from {s} to {e}."));
    } else if let Some(s) = parsed.start.as_deref() {
        prompt.push_str(&format!("\nStart from {s}."));
    } else if let Some(e) = parsed.end.as_deref() {
        prompt.push_str(&format!("\nEnd at {e}."));
    }

    let is_yt = is_youtube_url(audio_url);
    let mut errors = Vec::new();

    if let Some(creds) = ctx.image_gen_targets.google() {
        match call_google(ctx, creds, &prompt, audio_url, mode, is_yt).await {
            Ok(msg) => return Ok(msg),
            Err(e) => errors.push(format!("google: {e}")),
        }
    }

    if is_yt {
        errors.push("openai: YouTube 仅支持 Google Interactions".into());
        anyhow::bail!(
            "audio_understand 失败：{}。请配置 Google API Key（YouTube 需 Interactions）。",
            errors.join("；")
        );
    }

    match call_openai(ctx, &prompt, audio_url, mode).await {
        Ok(msg) => Ok(msg),
        Err(e) => {
            errors.push(format!("openai: {e}"));
            anyhow::bail!(
                "audio_understand 失败：{}。请配置 Google 或 OpenAI API Key。",
                errors.join("；")
            )
        }
    }
}

fn validate_mmss(v: Option<&str>) -> anyhow::Result<()> {
    let Some(s) = v.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let re_ok = s.len() <= 5
        && s.contains(':')
        && s.split_once(':')
            .map(|(a, b)| {
                a.chars().all(|c| c.is_ascii_digit())
                    && b.chars().all(|c| c.is_ascii_digit())
                    && b.len() == 2
                    && !a.is_empty()
                    && a.len() <= 2
            })
            .unwrap_or(false);
    if !re_ok {
        anyhow::bail!("时间格式无效（需要 MM:SS）: {s}");
    }
    Ok(())
}

fn is_youtube_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.contains("youtube.com") || lower.contains("youtu.be")
}

fn mime_from_path(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.as_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "wav" => "audio/wav",
        "mp3" => "audio/mp3",
        "aiff" | "aif" => "audio/aiff",
        "aac" => "audio/aac",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        _ => "audio/mp3",
    }
}

async fn call_google(
    ctx: &ToolContext<'_>,
    creds: &ImageGenCreds,
    prompt: &str,
    audio_url: &str,
    mode: AudioUnderstandMode,
    is_yt: bool,
) -> anyhow::Result<String> {
    let model = if creds.vision_model.trim().is_empty() {
        "gemini-3.5-flash".to_string()
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
    let media = resolve_google_media(ctx, audio_url, is_yt)?;
    let client = reqwest::Client::new();
    let text = google_interactions_audio(&client, prompt, &media, mode, &config).await?;
    Ok(format!(
        "{text}\nprovider=google\nmodel={model}\nmode={}",
        mode.as_str()
    ))
}

fn resolve_google_media(
    ctx: &ToolContext<'_>,
    audio_url: &str,
    is_yt: bool,
) -> anyhow::Result<AudioMediaPart> {
    if is_yt {
        return Ok(AudioMediaPart::Uri {
            media_type: AudioMediaKind::Video,
            mime_type: "video/mp4".into(),
            uri: audio_url.to_string(),
        });
    }
    if audio_url.starts_with("http://") || audio_url.starts_with("https://") {
        return Ok(AudioMediaPart::Uri {
            media_type: AudioMediaKind::Audio,
            mime_type: mime_from_path(std::path::Path::new(audio_url)).to_string(),
            uri: audio_url.to_string(),
        });
    }
    let path = ctx.workspace_dir.join(audio_url);
    if !path.exists() {
        anyhow::bail!("本地文件不存在: {}", path.display());
    }
    let bytes = std::fs::read(&path)
        .map_err(|e| anyhow::anyhow!("读取音频失败 {}: {e}", path.display()))?;
    let mime = mime_from_path(&path);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(AudioMediaPart::Inline {
        media_type: AudioMediaKind::Audio,
        mime_type: mime.to_string(),
        data_b64: b64,
    })
}

async fn call_openai(
    ctx: &ToolContext<'_>,
    prompt: &str,
    audio_url: &str,
    mode: AudioUnderstandMode,
) -> anyhow::Result<String> {
    let (bytes, mime, filename) = load_audio_bytes(ctx, audio_url).await?;
    match mode {
        AudioUnderstandMode::Describe => {
            let (config, model) = resolve_openai_describe_config(ctx)?;
            let client = reqwest::Client::new();
            let text =
                openai_audio_describe(&client, prompt, &bytes, &mime, &config).await?;
            Ok(format!(
                "{text}\nprovider=openai\nmodel={model}\nmode=describe"
            ))
        }
        AudioUnderstandMode::Transcribe => {
            let (config, model) = resolve_openai_whisper_config(ctx)?;
            let client = reqwest::Client::new();
            let text =
                openai_audio_transcriptions(&client, &bytes, &filename, &config).await?;
            let json = whisper_text_to_transcribe_json(&text);
            Ok(format!(
                "{}\nprovider=openai\nmodel={model}\nmode=transcribe\nfallback=openai",
                serde_json::to_string_pretty(&json)?
            ))
        }
    }
}

async fn load_audio_bytes(
    ctx: &ToolContext<'_>,
    audio_url: &str,
) -> anyhow::Result<(Vec<u8>, String, String)> {
    if audio_url.starts_with("http://") || audio_url.starts_with("https://") {
        let client = reqwest::Client::new();
        let resp = client
            .get(audio_url)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("下载音频失败: {e}"))?;
        if !resp.status().is_success() {
            anyhow::bail!("下载音频 HTTP {}", resp.status());
        }
        let mime = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("audio/mp3")
            .to_string();
        let bytes = resp.bytes().await?.to_vec();
        let filename = audio_url
            .rsplit('/')
            .next()
            .unwrap_or("audio.mp3")
            .to_string();
        return Ok((bytes, mime, filename));
    }
    let path = ctx.workspace_dir.join(audio_url);
    if !path.exists() {
        anyhow::bail!("本地文件不存在: {}", path.display());
    }
    let bytes = std::fs::read(&path)?;
    let mime = mime_from_path(&path).to_string();
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("audio.mp3")
        .to_string();
    Ok((bytes, mime, filename))
}

fn resolve_openai_describe_config(ctx: &ToolContext<'_>) -> anyhow::Result<(ProviderConfig, String)> {
    if let Some(creds) = ctx.image_gen_targets.openai() {
        let model = if creds.vision_model.trim().is_empty() {
            default_vision_model("openai").to_string()
        } else {
            creds.vision_model.trim().to_string()
        };
        let base = if creds.base_url.trim().is_empty() {
            Some("https://api.openai.com/v1".into())
        } else {
            Some(openai_compatible_base(&creds.base_url))
        };
        return Ok((
            ProviderConfig {
                api_key: creds.api_key.clone(),
                base_url: base,
                model: model.clone(),
                ..ProviderConfig::default()
            },
            model,
        ));
    }
    if !ctx.chat_api_key.is_empty() && ctx.chat_provider == "openai" {
        let model = if ctx.chat_model.trim().is_empty() {
            default_vision_model("openai").to_string()
        } else {
            ctx.chat_model.trim().to_string()
        };
        let base = if ctx.chat_base_url.trim().is_empty() {
            Some("https://api.openai.com/v1".into())
        } else {
            Some(openai_compatible_base(&ctx.chat_base_url))
        };
        return Ok((
            ProviderConfig {
                api_key: ctx.chat_api_key.clone(),
                base_url: base,
                model: model.clone(),
                ..ProviderConfig::default()
            },
            model,
        ));
    }
    let env_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    if env_key.is_empty() {
        anyhow::bail!("无可用 OpenAI API Key");
    }
    let model = default_vision_model("openai").to_string();
    Ok((
        ProviderConfig {
            api_key: env_key,
            base_url: Some("https://api.openai.com/v1".into()),
            model: model.clone(),
            ..ProviderConfig::default()
        },
        model,
    ))
}

fn resolve_openai_whisper_config(ctx: &ToolContext<'_>) -> anyhow::Result<(ProviderConfig, String)> {
    let model = default_whisper_model().to_string();
    if let Some(creds) = ctx.image_gen_targets.openai() {
        let base = if creds.base_url.trim().is_empty() {
            Some("https://api.openai.com/v1".into())
        } else {
            Some(openai_compatible_base(&creds.base_url))
        };
        return Ok((
            ProviderConfig {
                api_key: creds.api_key.clone(),
                base_url: base,
                model: model.clone(),
                ..ProviderConfig::default()
            },
            model,
        ));
    }
    if !ctx.chat_api_key.is_empty() && ctx.chat_provider == "openai" {
        let base = if ctx.chat_base_url.trim().is_empty() {
            Some("https://api.openai.com/v1".into())
        } else {
            Some(openai_compatible_base(&ctx.chat_base_url))
        };
        return Ok((
            ProviderConfig {
                api_key: ctx.chat_api_key.clone(),
                base_url: base,
                model: model.clone(),
                ..ProviderConfig::default()
            },
            model,
        ));
    }
    let env_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    if env_key.is_empty() {
        anyhow::bail!("无可用 OpenAI API Key");
    }
    Ok((
        ProviderConfig {
            api_key: env_key,
            base_url: Some("https://api.openai.com/v1".into()),
            model: model.clone(),
            ..ProviderConfig::default()
        },
        model,
    ))
}
```

单元测试（同文件 `#[cfg(test)]`）：

```rust
#[test]
fn rejects_bad_timestamp() {
    assert!(validate_mmss(Some("3")).is_err());
    assert!(validate_mmss(Some("02:30")).is_ok());
}

#[test]
fn youtube_detect() {
    assert!(is_youtube_url("https://youtu.be/abc"));
    assert!(!is_youtube_url("https://example.com/a.mp3"));
}
```

（若 `validate_mmss` / `is_youtube_url` 为私有，测 `dispatch` 参数错误，或 `pub(crate)` 之以便测。）

- [x] **Step 3: Run**

Run: `cargo test -p providers audio_understand_tests -- --nocapture`  
Run: `cargo test -p tools audio_understand -- --nocapture`  
Run: `cargo test -p home tools_enabled -- --nocapture`（若有相关测）  
Expected: PASS；`cargo check -p tools` PASS

- [x] **Step 4: Commit**

```bash
git add tools/src/builtin/media/audio_understand.rs \
  tools/src/builtin/media/mod.rs \
  tools/src/builtin/mod.rs \
  tools/src/lib.rs \
  tools/src/engine/dispatch.rs \
  home/src/config/tools_enabled.rs
git commit -m "$(cat <<'EOF'
feat(tools): add audio_understand with Google/OpenAI split

Wire Interactions audio understand and Whisper/Chat fallbacks
into registry, dispatch, and tools-enabled.

EOF
)"
```

---

### Task 4: 前端工具卡片与文案

**Files:**
- Modify: `apps/desktop/src/components/ToolIcons.tsx`
- Modify: `apps/desktop/src/hooks/useAgentTools.ts`
- Modify: `apps/desktop/src/i18n/messages.ts`

**Interfaces:**
- Produces: `AGENT_TOOLS` 条目 `id: "audio_understand"`；中英 i18n；图标组件

- [x] **Step 1: 图标**

在 `ToolIcons.tsx` 增加（简洁波形，非 emoji）：

```tsx
export function IconAudioUnderstand(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M4 10v4" />
      <path d="M8 7v10" />
      <path d="M12 4v16" />
      <path d="M16 7v10" />
      <path d="M20 10v4" />
    </IconBase>
  );
}
```

- [x] **Step 2: `useAgentTools.ts`**

在 `vision` 条目后插入：

```ts
{
  id: "audio_understand",
  titleKey: "agentTools.audioUnderstand.title",
  descKey: "agentTools.audioUnderstand.desc",
  Icon: IconAudioUnderstand,
  tone: "cyan",
  params: [
    { name: "audio_url", type: "string" },
    { name: "prompt", type: "string", optional: true },
    { name: "mode", type: "string", optional: true },
    { name: "start", type: "string", optional: true },
    { name: "end", type: "string", optional: true },
  ],
},
```

并 import `IconAudioUnderstand`。

- [x] **Step 3: i18n**

中文：

```ts
"agentTools.audioUnderstand.title": "音频理解",
"agentTools.audioUnderstand.desc": "Google Interactions 原生听音/转写（含 YouTube）；OpenAI 分路径 Chat 描述与 Whisper 转写",
```

英文：

```ts
"agentTools.audioUnderstand.title": "Audio understand",
"agentTools.audioUnderstand.desc": "Google Interactions for describe/transcribe (YouTube OK); OpenAI Chat describe + Whisper transcribe",
```

- [x] **Step 4: 确认前端无类型错误**

Run: `cd frontend && npx tsc --noEmit`（或项目惯用 `npm run typecheck`）  
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add apps/desktop/src/components/ToolIcons.tsx \
  apps/desktop/src/hooks/useAgentTools.ts \
  apps/desktop/src/i18n/messages.ts
git commit -m "$(cat <<'EOF'
feat(frontend): expose audio_understand tool in agent tools UI

EOF
)"
```

---

## Spec Coverage Checklist（自审）

| Spec 要求 | Task |
|-----------|------|
| Google Interactions 原生 audio | Task 1 |
| YouTube → `type: video` | Task 1 + Task 3 |
| transcribe `response_format` schema | Task 1 |
| OpenAI describe Chat `input_audio` | Task 2 |
| OpenAI Whisper + JSON 尽力 / `fallback=openai` | Task 2 + Task 3 |
| 工具 + mode + start/end | Task 3 |
| Google → OpenAI；YouTube 仅 Google | Task 3 |
| tools-enabled / 前端文案 | Task 3 + Task 4 |
| 无 Files API / 不改 ProvidersPanel | 全任务遵守 |

## Placeholder Scan

- 无 `unimplemented!` / TBD；`resolve_openai_*` 已给出完整凭证瀑布。
- `response_format` 若与已落地 vision Interactions 字段名不一致，以同文件既有形态为准并改测试。
