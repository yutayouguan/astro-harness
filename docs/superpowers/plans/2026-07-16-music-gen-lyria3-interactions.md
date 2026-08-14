# Music Gen Lyria 3 Interactions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增 `music_gen` 工具，经 Gemini Interactions 原生调用 Lyria 3（clip/pro），支持参考图与 Pro WAV，Google only，与 OpenAI 及本地 `music` 播放完全分开。

**Architecture:** 在已有 `providers::interactions_http` 追加独立的 Lyria 请求/解析/`google_interactions_music`；`tools` 新建 `music_gen.rs` 做参数校验、读图、落盘；前端工具开关与预览解析认「音乐已生成」。禁止 `…/v1beta/openai`。

**Tech Stack:** Rust (`providers` / `tools` / `home`)、`reqwest`、`serde_json`、`base64`、schemars；前端 `useAgentTools` + `messages.ts` + `parseGeneratedMedia`。

**参考:** `docs/superpowers/specs/2026-07-16-music-gen-lyria3-interactions-design.md`；[Lyria 3 音乐生成](https://ai.google.dev/gemini-api/docs/music-generation)

## Global Constraints

- Google：`POST {google_native_base}/v1beta/interactions`，Header `x-goog-api-key`（不用 Bearer / `?key=`）。
- 模型：`clip` → `lyria-3-clip-preview`（默认）；`pro` → `lyria-3-pro-preview`。
- Google only：无 Google 凭证直接失败；**不**回退 OpenAI。
- `format=wav` 仅 `pro`；`clip` + `wav` → 硬错误。
- 参考图最多 10；落盘 `generated/audio/music-*.{mp3|wav}`。
- 不做 PCM→WAV；不做 Lyria RealTime。
- 若 `interactions_http.rs` 已有 TTS/image/vision：只**追加** music 符号，不破坏现有 API。
- HTTP client 超时：`600s`（对齐 `video_gen` 量级，Pro 可能较久）。

## File Structure

| File | Responsibility |
|------|----------------|
| `providers/src/protocol/interactions_http.rs` | Lyria 类型、body、parse、`google_interactions_music` + 单测 |
| `tools/src/builtins/media/music_gen.rs` | 新建：Args、校验、读图、dispatch、落盘 |
| `tools/src/builtins/media/mod.rs` | `pub mod music_gen` |
| `tools/src/builtins/mod.rs` | re-export `music_gen` |
| `tools/src/lib.rs` | `register_all` + `pub use` |
| `tools/src/core/dispatch.rs` | `"music_gen" => …` |
| `home/src/config/tools_enabled.rs` | `KNOWN_TOOLSET_IDS` + `tool_name_to_toolset` |
| `apps/desktop/src/hooks/useAgentTools.ts` | AGENT_TOOLS 条目 |
| `apps/desktop/src/i18n/messages.ts` | zh/en 文案 |
| `apps/desktop/src/lib/parseGeneratedMedia.ts` | 认「音乐已生成」 |
| `apps/desktop/src/lib/parseGeneratedMedia.test.ts` | 若已有测文件则追加；否则本 Task 创建最小测 |

---

### Task 1: `interactions_http` Lyria helpers

**Files:**
- Modify: `providers/src/protocol/interactions_http.rs`
- Test: 同文件 `mod tests`

**Interfaces:**
- Consumes: 已有 `interactions_url`、`error_message`（文件内）、`google_native_base`（经 url）、`ProviderConfig`、`reqwest::Client`
- Produces:
  - `pub enum MusicAudioFormat { Mp3, Wav }`
  - `pub struct MusicImagePart { pub mime_type: String, pub data_base64: String }`
  - `pub struct InteractionMusicRequest { pub model: String, pub prompt: String, pub images: Vec<MusicImagePart>, pub format: MusicAudioFormat }`
  - `pub struct InteractionMusicResult { pub audio_bytes: Vec<u8>, pub mime_type: String, pub lyrics_text: Option<String>, pub interaction_id: String }`
  - `pub fn default_music_model_clip() -> &'static str` → `"lyria-3-clip-preview"`
  - `pub fn default_music_model_pro() -> &'static str` → `"lyria-3-pro-preview"`
  - `pub fn resolve_lyria_model_id(alias: &str) -> Result<String>` — `clip`/`pro`/完整 model id
  - `pub fn build_interaction_music_body(req: &InteractionMusicRequest) -> Value`
  - `pub fn parse_interaction_music_response(v: &Value) -> Result<InteractionMusicResult>`
  - `pub fn music_extension(mime: &str, format: MusicAudioFormat) -> &'static str`
  - `pub async fn google_interactions_music(client: &Client, config: &ProviderConfig, req: &InteractionMusicRequest) -> Result<InteractionMusicResult>`

- [ ] **Step 1: Write the failing tests**

在 `interactions_http.rs` 的 `mod tests` 末尾追加（若 `tests` 模块已有 `use super::*` 则复用）：

```rust
#[test]
fn build_music_body_text_only() {
    let req = InteractionMusicRequest {
        model: "lyria-3-clip-preview".into(),
        prompt: "minimal techno".into(),
        images: vec![],
        format: MusicAudioFormat::Mp3,
    };
    let body = build_interaction_music_body(&req);
    assert_eq!(body["model"], "lyria-3-clip-preview");
    assert_eq!(body["input"], "minimal techno");
    assert_eq!(body["response_format"]["type"], "audio");
    assert!(body.get("generation_config").is_none());
}

#[test]
fn build_music_body_with_images_and_wav() {
    let req = InteractionMusicRequest {
        model: "lyria-3-pro-preview".into(),
        prompt: "ambient from image".into(),
        images: vec![MusicImagePart {
            mime_type: "image/jpeg".into(),
            data_base64: "abc".into(),
        }],
        format: MusicAudioFormat::Wav,
    };
    let body = build_interaction_music_body(&req);
    let input = body["input"].as_array().expect("input array");
    assert_eq!(input[0]["type"], "text");
    assert_eq!(input[0]["text"], "ambient from image");
    assert_eq!(input[1]["type"], "image");
    assert_eq!(input[1]["mime_type"], "image/jpeg");
    assert_eq!(input[1]["data"], "abc");
    // Pro+Wav：在 response_format 上附 mime_type 提示（官方文档字段若变更，只改此处与本断言）
    assert_eq!(body["response_format"]["type"], "audio");
    assert_eq!(body["response_format"]["mime_type"], "audio/wav");
}

#[test]
fn parse_music_output_audio_and_text() {
    let v = serde_json::json!({
        "id": "ix-music-1",
        "output_audio": { "data": "Zm9v", "mime_type": "audio/mpeg" },
        "output_text": "[Verse]\nhello"
    });
    let r = parse_interaction_music_response(&v).unwrap();
    assert_eq!(r.interaction_id, "ix-music-1");
    assert_eq!(r.audio_bytes, b"foo");
    assert!(r.mime_type.contains("mpeg"));
    assert_eq!(r.lyrics_text.as_deref(), Some("[Verse]\nhello"));
}

#[test]
fn parse_music_from_steps_fallback() {
    let v = serde_json::json!({
        "id": "ix-2",
        "steps": [{
            "type": "model_output",
            "content": [
                { "type": "text", "text": "line1" },
                { "type": "audio", "data": "YmFy", "mime_type": "audio/wav" }
            ]
        }]
    });
    let r = parse_interaction_music_response(&v).unwrap();
    assert_eq!(r.audio_bytes, b"bar");
    assert!(r.mime_type.contains("wav"));
    assert_eq!(r.lyrics_text.as_deref(), Some("line1"));
}

#[test]
fn parse_music_missing_audio_errors() {
    let v = serde_json::json!({ "id": "ix", "output_text": "only text" });
    assert!(parse_interaction_music_response(&v).is_err());
}

#[test]
fn resolve_lyria_model_and_extension() {
    assert_eq!(resolve_lyria_model_id("clip").unwrap(), "lyria-3-clip-preview");
    assert_eq!(resolve_lyria_model_id("pro").unwrap(), "lyria-3-pro-preview");
    assert_eq!(
        resolve_lyria_model_id("lyria-3-pro-preview").unwrap(),
        "lyria-3-pro-preview"
    );
    assert!(resolve_lyria_model_id("nope").is_err());
    assert_eq!(music_extension("audio/mpeg", MusicAudioFormat::Mp3), "mp3");
    assert_eq!(music_extension("audio/wav", MusicAudioFormat::Wav), "wav");
    assert_eq!(music_extension("", MusicAudioFormat::Wav), "wav");
    assert_eq!(music_extension("", MusicAudioFormat::Mp3), "mp3");
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p providers --lib interactions_http::tests::build_music_body_text_only -- --nocapture
```

Expected: FAIL（类型/函数未定义）

- [ ] **Step 3: Implement types + body + parse + HTTP**

在 `interactions_http.rs` 末尾（vision 段之后、`mod tests` 之前）追加大致如下（保持与文件现有风格一致：`anyhow`、`json!`、`base64` STANDARD）：

```rust
// ── Music (Lyria 3) ──────────────────────────────────────────────────────────

pub fn default_music_model_clip() -> &'static str {
    "lyria-3-clip-preview"
}
pub fn default_music_model_pro() -> &'static str {
    "lyria-3-pro-preview"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicAudioFormat {
    Mp3,
    Wav,
}

#[derive(Debug, Clone)]
pub struct MusicImagePart {
    pub mime_type: String,
    pub data_base64: String,
}

#[derive(Debug, Clone)]
pub struct InteractionMusicRequest {
    pub model: String,
    pub prompt: String,
    pub images: Vec<MusicImagePart>,
    pub format: MusicAudioFormat,
}

#[derive(Debug, Clone)]
pub struct InteractionMusicResult {
    pub audio_bytes: Vec<u8>,
    pub mime_type: String,
    pub lyrics_text: Option<String>,
    pub interaction_id: String,
}

pub fn resolve_lyria_model_id(alias: &str) -> Result<String> {
    let a = alias.trim();
    Ok(match a {
        "" | "clip" => default_music_model_clip().to_string(),
        "pro" => default_music_model_pro().to_string(),
        "lyria-3-clip-preview" | "lyria-3-pro-preview" => a.to_string(),
        _ => anyhow::bail!("无效 music model: {a}（clip | pro | lyria-3-*-preview）"),
    })
}

pub fn music_extension(mime: &str, format: MusicAudioFormat) -> &'static str {
    let m = mime.to_ascii_lowercase();
    if m.contains("wav") {
        return "wav";
    }
    if m.contains("mpeg") || m.contains("mp3") {
        return "mp3";
    }
    match format {
        MusicAudioFormat::Wav => "wav",
        MusicAudioFormat::Mp3 => "mp3",
    }
}

pub fn build_interaction_music_body(req: &InteractionMusicRequest) -> Value {
    let input = if req.images.is_empty() {
        json!(req.prompt)
    } else {
        let mut parts = vec![json!({ "type": "text", "text": req.prompt })];
        for img in &req.images {
            parts.push(json!({
                "type": "image",
                "mime_type": img.mime_type,
                "data": img.data_base64,
            }));
        }
        json!(parts)
    };

    let mut response_format = json!({ "type": "audio" });
    if req.format == MusicAudioFormat::Wav {
        response_format["mime_type"] = json!("audio/wav");
    }

    json!({
        "model": req.model,
        "input": input,
        "response_format": response_format,
    })
}

pub fn parse_interaction_music_response(v: &Value) -> Result<InteractionMusicResult> {
    let interaction_id = v
        .get("id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();

    let mut mime_type = String::new();
    let mut lyrics_parts: Vec<String> = Vec::new();
    let mut audio_b64: Option<&str> = None;

    if let Some(oa) = v.get("output_audio").or_else(|| v.get("outputAudio")) {
        if let Some(d) = oa.get("data").and_then(|x| x.as_str()) {
            audio_b64 = Some(d);
        }
        if let Some(m) = oa
            .get("mime_type")
            .or_else(|| oa.get("mimeType"))
            .and_then(|x| x.as_str())
        {
            mime_type = m.to_string();
        }
    }
    if let Some(t) = v
        .get("output_text")
        .or_else(|| v.get("outputText"))
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        lyrics_parts.push(t.to_string());
    }

    if audio_b64.is_none() {
        if let Some(steps) = v.get("steps").and_then(|s| s.as_array()) {
            for step in steps {
                let st = step.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if st != "model_output" && !st.is_empty() {
                    continue;
                }
                let Some(content) = step.get("content").and_then(|c| c.as_array()) else {
                    continue;
                };
                for block in content {
                    let bt = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    match bt {
                        "audio" => {
                            if let Some(d) = block.get("data").and_then(|x| x.as_str()) {
                                audio_b64 = Some(d);
                            }
                            if let Some(m) = block
                                .get("mime_type")
                                .or_else(|| block.get("mimeType"))
                                .and_then(|x| x.as_str())
                            {
                                mime_type = m.to_string();
                            }
                        }
                        "text" => {
                            if let Some(t) = block.get("text").and_then(|x| x.as_str()) {
                                let t = t.trim();
                                if !t.is_empty() {
                                    lyrics_parts.push(t.to_string());
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    let b64 = audio_b64.ok_or_else(|| anyhow!("Google interactions music 响应无音频数据"))?;
    let audio_bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .context("解码 Interactions music 音频失败")?;
    if audio_bytes.is_empty() {
        anyhow::bail!("Google interactions music 返回空音频");
    }

    let lyrics_text = if lyrics_parts.is_empty() {
        None
    } else {
        Some(lyrics_parts.join("\n"))
    };

    // filtered_prompt：若有则拼进错误旁注不在此处；成功路径可忽略
    let _ = v.get("filtered_prompt");

    Ok(InteractionMusicResult {
        audio_bytes,
        mime_type,
        lyrics_text,
        interaction_id,
    })
}

pub async fn google_interactions_music(
    client: &Client,
    config: &ProviderConfig,
    req: &InteractionMusicRequest,
) -> Result<InteractionMusicResult> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    if req.prompt.trim().is_empty() {
        anyhow::bail!("music prompt 为空");
    }
    if req.images.len() > 10 {
        anyhow::bail!("music 参考图最多 10 张");
    }

    let url = interactions_url(config);
    let body = build_interaction_music_body(req);
    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .header("x-goog-api-key", config.api_key.trim())
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 Google interactions music 失败: {url}"))?;
    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析 Google interactions music 响应失败")?;
    if !status.is_success() {
        let mut msg = error_message(&v);
        if let Some(fp) = v
            .get("filtered_prompt")
            .and_then(|x| x.as_str())
            .or_else(|| v.pointer("/filtered_prompt/text").and_then(|x| x.as_str()))
        {
            msg = format!("{msg}; filtered_prompt={fp}");
        }
        anyhow::bail!("Google interactions music HTTP {status}: {msg}");
    }
    parse_interaction_music_response(&v)
}
```

注意：`error_message` 若为 `fn` 私有则直接调用；若不可见则复制同文件 TTS 用的指针逻辑。

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p providers --lib interactions_http::tests::build_music_body_ -- --nocapture
cargo test -p providers --lib interactions_http::tests::parse_music_ -- --nocapture
cargo test -p providers --lib interactions_http::tests::resolve_lyria_ -- --nocapture
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add providers/src/protocol/interactions_http.rs
git commit -m "$(cat <<'EOF'
feat(providers): add Lyria 3 Interactions music helpers

Google-native music generation body/parse/HTTP, separate from TTS
and OpenAI-compatible endpoints.
EOF
)"
```

---

### Task 2: `music_gen` 工具 + 注册

**Files:**
- Create: `tools/src/builtins/media/music_gen.rs`
- Modify: `tools/src/builtins/media/mod.rs`
- Modify: `tools/src/builtins/mod.rs`
- Modify: `tools/src/lib.rs`
- Modify: `tools/src/core/dispatch.rs`
- Modify: `home/src/config/tools_enabled.rs`

**Interfaces:**
- Consumes: Task 1 全部 Produces；`ToolContext::image_gen_targets.google()`；`home::{generated_dir, GeneratedKind}`；`providers::trait_::ProviderConfig`
- Produces: `music_gen::register` / `music_gen::dispatch`；toolset id `music_gen`

- [ ] **Step 1: Write failing tool-level unit tests（同文件 `#[cfg(test)]`）**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ImageGenTargets;
    use std::path::PathBuf;

    fn empty_ctx(ws: PathBuf) -> (ImageGenTargets, String, home::AgentMemory /* 按项目实际构造 */) {
        // 若 ToolContext 构造过重：只测纯函数 validate_music_gen_args / resolve_model_alias
        // 推荐把校验抽成:
        //   fn validate_music_gen_args(args: &MusicGenArgs) -> anyhow::Result<(String /*model_id*/, MusicAudioFormat)>
        unimplemented!("见 Step 3 抽出的 validate；此处测空 prompt / wav+clip / >10 images")
    }

    #[test]
    fn reject_empty_prompt() {
        let args = MusicGenArgs {
            prompt: "  ".into(),
            model: None,
            reference_images: None,
            format: None,
        };
        assert!(validate_music_gen_args(&args).is_err());
    }

    #[test]
    fn reject_wav_on_clip() {
        let args = MusicGenArgs {
            prompt: "lofi".into(),
            model: Some("clip".into()),
            reference_images: None,
            format: Some("wav".into()),
        };
        let err = validate_music_gen_args(&args).unwrap_err().to_string();
        assert!(err.contains("wav") || err.contains("pro"));
    }

    #[test]
    fn accept_pro_wav() {
        let args = MusicGenArgs {
            prompt: "piano".into(),
            model: Some("pro".into()),
            reference_images: None,
            format: Some("wav".into()),
        };
        let (model_id, fmt) = validate_music_gen_args(&args).unwrap();
        assert_eq!(model_id, "lyria-3-pro-preview");
        assert_eq!(fmt, MusicAudioFormat::Wav);
    }

    #[test]
    fn reject_too_many_images() {
        let args = MusicGenArgs {
            prompt: "x".into(),
            model: None,
            reference_images: Some((0..11).map(|i| format!("a{i}.jpg")).collect()),
            format: None,
        };
        assert!(validate_music_gen_args(&args).is_err());
    }
}
```

说明：若项目里 `AgentMemory` 难构造，**只测 `validate_music_gen_args`**，不要强行拼完整 `ToolContext`。

- [ ] **Step 2: Run tests — expect fail**

```bash
cargo test -p tools --lib music_gen::tests::reject_empty_prompt -- --nocapture
```

Expected: FAIL（模块不存在）

- [ ] **Step 3: Implement `music_gen.rs`**

```rust
//! 音乐生成：Google Lyria 3 via Gemini Interactions（Google only）。
//!
//! 与本地播放工具 `music` 分离；无 Google 凭证不回退 OpenAI。
//! 产物写入 `generated/audio/music-*.{mp3|wav}`。

use std::path::{Path, PathBuf};

use base64::Engine;
use home::{generated_dir, GeneratedKind};
use providers::interactions_http::{
    google_interactions_music, music_extension, resolve_lyria_model_id, InteractionMusicRequest,
    MusicAudioFormat, MusicImagePart,
};
use providers::trait_::ProviderConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MusicGenArgs {
    /// 音乐描述（流派/乐器/BPM/结构等）。
    pub prompt: String,
    /// `clip`（默认，约 30s）或 `pro`（完整歌曲）。
    #[serde(default)]
    pub model: Option<String>,
    /// 参考图工作区路径，最多 10。
    #[serde(default)]
    pub reference_images: Option<Vec<String>>,
    /// `mp3`（默认）或 `wav`（仅 pro）。
    #[serde(default)]
    pub format: Option<String>,
}

pub fn validate_music_gen_args(
    args: &MusicGenArgs,
) -> anyhow::Result<(String, MusicAudioFormat)> {
    if args.prompt.trim().is_empty() {
        anyhow::bail!("music_gen 需要 prompt");
    }
    let alias = args
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("clip");
    let model_id = resolve_lyria_model_id(alias)?;
    let is_pro = model_id.contains("pro");

    let fmt_raw = args
        .format
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("mp3")
        .to_ascii_lowercase();
    let format = match fmt_raw.as_str() {
        "mp3" => MusicAudioFormat::Mp3,
        "wav" => MusicAudioFormat::Wav,
        other => anyhow::bail!("format 无效: {other}（mp3 | wav）"),
    };
    if format == MusicAudioFormat::Wav && !is_pro {
        anyhow::bail!("wav 仅 lyria-3-pro 支持（请设 model=pro）");
    }

    if let Some(refs) = &args.reference_images {
        let n = refs.iter().filter(|p| !p.trim().is_empty()).count();
        if n > 10 {
            anyhow::bail!("reference_images 最多 10 张");
        }
    }
    Ok((model_id, format))
}

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "music_gen".to_string(),
        toolset: "music_gen".to_string(),
        description: "Generate music with Google Lyria 3 (Interactions). model=clip|pro; optional reference_images (≤10); format=mp3|wav (wav requires pro). Google only — not local music playback. Writes generated/audio/.".to_string(),
        schema: schema_for_args::<MusicGenArgs>(),
        check_fn: None,
        icon: "music",
    });
}

pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: MusicGenArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("music_gen 参数无效: {e}"))?;
    let (model_id, format) = validate_music_gen_args(&parsed)?;

    let creds = ctx.image_gen_targets.google().ok_or_else(|| {
        anyhow::anyhow!("music_gen 需要 Google API Key（未配置 Google，且不回退 OpenAI）")
    })?;

    let mut images = Vec::new();
    if let Some(refs) = &parsed.reference_images {
        for rel in refs.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            images.push(load_music_image(ctx, rel)?);
        }
    }

    let config = ProviderConfig {
        api_key: creds.api_key.clone(),
        base_url: if creds.base_url.trim().is_empty() {
            None
        } else {
            Some(creds.base_url.clone())
        },
        model: model_id.clone(),
        ..ProviderConfig::default()
    };
    let req = InteractionMusicRequest {
        model: model_id.clone(),
        prompt: parsed.prompt.trim().to_string(),
        images,
        format,
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()?;
    let result = google_interactions_music(&client, &config, &req).await?;

    let ext = music_extension(&result.mime_type, format);
    let dir = generated_dir(&ctx.workspace_dir, GeneratedKind::Audio);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!(
        "music-{}-{}.{}",
        chrono::Local::now().format("%Y%m%d-%H%M%S"),
        &uuid::Uuid::new_v4().simple().to_string()[..8],
        ext
    ));
    std::fs::write(&path, &result.audio_bytes)?;
    let rel = path
        .strip_prefix(&ctx.workspace_dir)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string());

    let mut out = format!(
        "音乐已生成：{rel}\nprovider=google\nmodel={model_id}\ninteraction_id={}",
        result.interaction_id
    );
    if let Some(lyrics) = result.lyrics_text.filter(|s| !s.trim().is_empty()) {
        out.push_str("\nlyrics:\n");
        out.push_str(&lyrics);
    }
    Ok(out)
}

fn load_music_image(ctx: &ToolContext<'_>, relative: &str) -> anyhow::Result<MusicImagePart> {
    let path = resolve_workspace_file(ctx, relative)?;
    let bytes = std::fs::read(&path)
        .map_err(|e| anyhow::anyhow!("读取参考图失败 {}: {e}", path.display()))?;
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("image.jpg");
    let mime = mime_from_name(filename);
    Ok(MusicImagePart {
        mime_type: mime.to_string(),
        data_base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
    })
}

fn resolve_workspace_file(ctx: &ToolContext<'_>, input: &str) -> anyhow::Result<PathBuf> {
    let p = PathBuf::from(input);
    let path = if p.is_absolute() {
        p
    } else {
        ctx.workspace_dir.join(input)
    };
    let canon_ws = ctx
        .workspace_dir
        .canonicalize()
        .unwrap_or_else(|_| ctx.workspace_dir.clone());
    let canon = path
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("文件不存在: {}", path.display()))?;
    if !canon.starts_with(&canon_ws) {
        anyhow::bail!("文件必须位于工作区内: {}", path.display());
    }
    if !canon.is_file() {
        anyhow::bail!("文件不存在: {}", path.display());
    }
    Ok(canon)
}

fn mime_from_name(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else {
        "image/jpeg"
    }
}
```

依赖：若 `tools/Cargo.toml` 尚无 `base64`，添加与 `providers` 同版本；已有则跳过。检查：

```bash
rg 'base64' tools/Cargo.toml
```

- [ ] **Step 4: Wire modules / dispatch / toolset**

`tools/src/builtins/media/mod.rs`：

```rust
pub mod music_gen;
```

`tools/src/builtins/mod.rs`：

```rust
pub use media::{image_gen, music, music_gen, tts, video_gen, vision};
```

`tools/src/lib.rs`：在 `pub(crate) use builtins::{…}` 与 `register_all` 中加入 `music_gen`（`music` 旁）：

```rust
music_gen::register(registry);
```

`tools/src/core/dispatch.rs`：

```rust
"music_gen" => crate::music_gen::dispatch(ctx, args).await,
```

（确保 `crate::music_gen` 经 builtins re-export 可达；若不行用 `crate::builtins::music_gen`。）

`home/src/config/tools_enabled.rs`：

- `KNOWN_TOOLSET_IDS` 在 `"tts"` 后插入 `"music_gen"`
- `tool_name_to_toolset`：`"music_gen" => "music_gen"`

- [ ] **Step 5: Run tests**

```bash
cargo test -p tools --lib music_gen -- --nocapture
cargo test -p home --lib tools_enabled -- --nocapture
cargo check -p tools -p home
```

Expected: PASS / 无错误

- [ ] **Step 6: Commit**

```bash
git add tools/src/builtins/media/music_gen.rs tools/src/builtins/media/mod.rs tools/src/builtins/mod.rs tools/src/lib.rs tools/src/core/dispatch.rs home/src/config/tools_enabled.rs tools/Cargo.toml
git commit -m "$(cat <<'EOF'
feat(tools): add music_gen Lyria 3 Google-native tool

Agent tool for clip/pro music generation with optional images;
Google only, separate from local music playback.
EOF
)"
```

---

### Task 3: 前端开关 + 预览解析

**Files:**
- Modify: `apps/desktop/src/hooks/useAgentTools.ts`
- Modify: `apps/desktop/src/i18n/messages.ts`
- Modify: `apps/desktop/src/lib/parseGeneratedMedia.ts`
- Modify or Create: `apps/desktop/src/lib/parseGeneratedMedia.test.ts`（若仓库用 vitest；无测文件则只改解析并跳过测，或加最小测）

**Interfaces:**
- Consumes: 工具返回文案前缀 `音乐已生成：`
- Produces: AGENT_TOOLS 条目 `music_gen`；i18n keys；parse 识别 audio

- [ ] **Step 1: 更新 `parseGeneratedMedia`**

```ts
const LABELED =
  /(?:图片|视频|语音|音乐)已生成[：:]\s*(\S+)/g;

function kindFromLabel(line: string): GeneratedMediaKind | null {
  if (line.includes("图片已生成")) return "image";
  if (line.includes("视频已生成")) return "video";
  if (line.includes("语音已生成") || line.includes("音乐已生成")) return "audio";
  return null;
}
```

注释也改为含 `music_gen`。

若存在测试文件，追加：

```ts
expect(parseGeneratedMedia("音乐已生成：generated/audio/music-1.mp3")).toEqual([
  { kind: "audio", path: "generated/audio/music-1.mp3" },
]);
```

- [ ] **Step 2: `useAgentTools.ts`**

在 `tts` 条目后、`skills` 前插入（复用 `IconMusic`，与本地 `music` 区分靠 title/desc）：

```ts
{
  id: "music_gen",
  titleKey: "agentTools.musicGen.title",
  descKey: "agentTools.musicGen.desc",
  Icon: IconMusic,
  tone: "violet",
  params: [
    { name: "prompt", type: "string" },
    { name: "model", type: "string", optional: true },
    { name: "reference_images", type: "string", optional: true },
    { name: "format", type: "string", optional: true },
  ],
},
```

同时把 `AgentToolId` 联合类型（若有显式 union）加入 `"music_gen"`。

- [ ] **Step 3: i18n `messages.ts`**

中文（靠近 `agentTools.tts`）：

```ts
"agentTools.musicGen.title": "音乐生成",
"agentTools.musicGen.desc": "Google Lyria 3 原生生成（Interactions）；clip/pro，可选参考图；写入 generated/audio",
```

英文：

```ts
"agentTools.musicGen.title": "Music Generation",
"agentTools.musicGen.desc": "Google Lyria 3 native (Interactions); clip/pro, optional reference images; writes generated/audio",
```

- [ ] **Step 4: 跑前端测（若有）**

```bash
cd frontend && npm test -- --run src/lib/parseGeneratedMedia.test.ts
```

Expected: PASS（无该文件则跳过）

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/hooks/useAgentTools.ts apps/desktop/src/i18n/messages.ts apps/desktop/src/lib/parseGeneratedMedia.ts apps/desktop/src/lib/parseGeneratedMedia.test.ts
git commit -m "$(cat <<'EOF'
feat(frontend): expose music_gen tool and parse audio preview

Add agent tool toggle/i18n and recognize 音乐已生成 paths.
EOF
)"
```

---

### Task 4: 验证与自检

**Files:** 无新增；跑命令

- [ ] **Step 1: providers + tools 测试**

```bash
cargo test -p providers --lib interactions_http -- --nocapture
cargo test -p tools --lib music_gen -- --nocapture
cargo check -p tools -p providers -p home
```

Expected: 全部 PASS / 无错误

- [ ] **Step 2: Spec 对照清单（人工勾）**

- [ ] Google Interactions URL + `x-goog-api-key`
- [ ] clip/pro 映射
- [ ] ≤10 参考图
- [ ] wav 仅 pro 硬错误
- [ ] 无 Google 不回退 OpenAI
- [ ] 落盘 `music-*.{mp3|wav}` + `音乐已生成` + `interaction_id`
- [ ] 本地 `music` 未改行为
- [ ] 工具开关 `music_gen`

- [ ] **Step 3: 若有未提交改动则提交**

```bash
git status
```

干净则完成。

---

## Spec coverage（自检）

| Spec 项 | Task |
|---------|------|
| Lyria 3 Interactions 原生 | 1 |
| clip/pro + format + images | 1 body + 2 validate |
| Google only / 无 OpenAI | 2 dispatch |
| 落盘 + 返回文案 | 2 |
| 注册 toolset / 前端 | 2–3 |
| parse 预览 | 3 |
| 非 RealTime / 不改本地 music | 全局约束 |

## Placeholder scan

无 TBD/TODO；`response_format.mime_type` 为 Pro+Wav 的约定字段（文档模糊时的明确选择）；若线上 API 拒识该字段，只改 `build_interaction_music_body` 与对应单测，不改工具契约。
