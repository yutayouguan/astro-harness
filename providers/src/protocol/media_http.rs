//! Google OpenAI 兼容出图/视频/视觉，以及旧版 Gemini `generateContent` TTS。
//!
//! TTS 主路径请用 [`crate::interactions_http::google_interactions_tts`]。

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::time::sleep;

use crate::http_stream::openai_compatible_base;
use crate::trait_::{GeneratedImage, ProviderConfig};

/// 去掉 endpoint 末尾斜杠。
fn trim_slash(endpoint: &str) -> String {
    endpoint.trim_end_matches('/').to_string()
}

/// Google 原生 API 根（去掉 `/v1beta/openai`）。
pub fn google_native_base(config: &ProviderConfig) -> String {
    let raw = config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("https://generativelanguage.googleapis.com");
    let base = trim_slash(raw);
    if base.ends_with("/openai") {
        return base
            .trim_end_matches("/openai")
            .trim_end_matches("/v1beta")
            .to_string();
    }
    base
}

/// Google OpenAI 兼容基址（`…/v1beta/openai`）。
pub fn google_openai_base(config: &ProviderConfig) -> String {
    let raw = config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("https://generativelanguage.googleapis.com/v1beta/openai");
    let base = trim_slash(raw);
    if base.contains("/v1beta/openai") || base.ends_with("/openai") {
        return openai_compatible_base(&base);
    }
    let native = google_native_base(config);
    if native.contains("/v1beta") {
        format!("{native}/openai")
    } else {
        format!("{native}/v1beta/openai")
    }
}

/// 默认视频模型。
pub fn default_video_model() -> &'static str {
    "veo-3.1-generate-preview"
}

/// 默认 Gemini TTS 模型。
pub fn default_tts_model() -> &'static str {
    "gemini-3.1-flash-tts-preview"
}

/// 默认视觉（图片理解）模型。
pub fn default_vision_model(provider: &str) -> &'static str {
    match provider {
        "google" => "gemini-3.5-flash",
        "openai" => "gpt-4o",
        _ => "gpt-4o",
    }
}

/// 默认 Whisper 转写模型。
pub fn default_whisper_model() -> &'static str {
    "whisper-1"
}

/// 从 MIME 或格式字符串映射 OpenAI `input_audio.format`。
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

/// 构建 OpenAI Chat `input_audio` 描述请求体。
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

/// Whisper 纯文本转尽力而为的转写 JSON（单 segment）。
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

fn parse_chat_completion_content(content: &Value) -> Result<String> {
    if let Some(s) = content.as_str() {
        return Ok(s.to_string());
    }
    if let Some(arr) = content.as_array() {
        let mut parts = Vec::new();
        for item in arr {
            if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                parts.push(t.to_string());
            } else if let Some(t) = item.as_str() {
                parts.push(t.to_string());
            }
        }
        if !parts.is_empty() {
            return Ok(parts.join("\n"));
        }
    }
    anyhow::bail!("响应 content 格式无法解析")
}

/// OpenAI Chat `input_audio` 音频描述；仅走 OpenAI 兼容 base，不经 Google。
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

    let response = client
        .post(&url)
        .bearer_auth(&config.api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 OpenAI 音频描述失败: {url}"))?;
    let status = response.status();
    let v: Value = response.json().await.context("解析音频描述响应失败")?;
    if !status.is_success() {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .or_else(|| v.get("error").and_then(|e| e.as_str()))
            .unwrap_or("音频描述请求失败");
        anyhow::bail!("音频描述 HTTP {status}: {msg}");
    }

    let content = v
        .pointer("/choices/0/message/content")
        .ok_or_else(|| anyhow!("音频描述响应无 choices[0].message.content"))?;
    parse_chat_completion_content(content)
}

/// OpenAI Whisper 转写；`multipart/form-data` 上传 `file` + `model`，返回纯文本。
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

/// OpenAI 兼容视觉：`POST …/chat/completions`，content 含 text + image_url。
///
/// `image_url` 可为 `data:image/...;base64,...` 或 `http(s)://`。
/// `provider` 为 `"google"` 时走 [`google_openai_base`]，否则走 OpenAI 兼容 base。
pub async fn openai_vision_completions(
    client: &Client,
    provider: &str,
    prompt: &str,
    image_url: &str,
    config: &ProviderConfig,
) -> Result<String> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("API Key 为空");
    }
    let model = if config.model.trim().is_empty() {
        default_vision_model(provider)
    } else {
        config.model.trim()
    };

    let base = if provider == "google"
        || config
            .base_url
            .as_deref()
            .map(|u| u.contains("generativelanguage.googleapis.com"))
            .unwrap_or(false)
    {
        google_openai_base(config)
    } else {
        let raw = config
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("https://api.openai.com/v1");
        openai_compatible_base(raw)
    };
    let url = format!("{base}/chat/completions");
    let body = json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": prompt },
                {
                    "type": "image_url",
                    "image_url": { "url": image_url }
                }
            ]
        }]
    });

    let response = client
        .post(&url)
        .bearer_auth(&config.api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接视觉接口失败: {url}"))?;
    let status = response.status();
    let v: Value = response.json().await.context("解析视觉响应失败")?;
    if !status.is_success() {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .or_else(|| v.get("error").and_then(|e| e.as_str()))
            .unwrap_or("视觉请求失败");
        anyhow::bail!("视觉 HTTP {status}: {msg}");
    }

    let content = v
        .pointer("/choices/0/message/content")
        .ok_or_else(|| anyhow!("视觉响应无 choices[0].message.content"))?;
    parse_chat_completion_content(content)
}

/// 默认 Google 图片兼容模型。
pub fn default_google_compat_image_model() -> &'static str {
    "gemini-3.1-flash-image"
}

/// Google OpenAI 兼容出图：`POST …/images/generations`。
pub async fn google_openai_generate_image(
    client: &Client,
    prompt: &str,
    config: &ProviderConfig,
) -> Result<Vec<GeneratedImage>> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    let model = if config.model.trim().is_empty() {
        default_google_compat_image_model()
    } else {
        config.model.trim()
    };
    let base = google_openai_base(config);
    let url = format!("{base}/images/generations");
    let mut body = json!({
        "model": model,
        "prompt": prompt,
        "response_format": "b64_json",
        "n": 1,
    });
    // Gemini 专有：宽高比（工具可经 additional_params.aspect_ratio 注入）
    if let Some(ar) = config
        .additional_params
        .get("aspect_ratio")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        body["aspect_ratio"] = json!(ar);
    }

    let response = client
        .post(&url)
        .bearer_auth(&config.api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 Google 兼容出图 API 失败: {url}"))?;

    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析 Google 兼容出图响应失败")?;
    if !status.is_success() {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("Google 兼容出图失败");
        anyhow::bail!("Google images HTTP {status}: {msg}");
    }

    let data_arr = v
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| anyhow!("Google 兼容出图响应无 data"))?;
    let mut images = Vec::new();
    for item in data_arr {
        let Some(b64) = item.get("b64_json").and_then(|b| b.as_str()) else {
            continue;
        };
        let data = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .context("解码 Google 兼容出图 base64 失败")?;
        images.push(GeneratedImage {
            data,
            mime_type: "image/png".to_string(),
        });
    }
    if images.is_empty() {
        anyhow::bail!("Google 兼容出图未返回图片");
    }
    Ok(images)
}

/// 视频生成结果。
#[derive(Debug, Clone)]
pub struct GeneratedVideo {
    pub data: Vec<u8>,
    pub mime_type: String,
    pub operation_id: String,
}

/// multipart 图片字段（首帧 / 尾帧 / 参考图）。
#[derive(Debug, Clone)]
pub struct VideoImagePart {
    pub bytes: Vec<u8>,
    pub filename: String,
    pub mime: String,
}

/// Google 视频创建的可选参数（文本 + 图片）。
#[derive(Debug, Clone, Default)]
pub struct VideoGenExtras {
    pub aspect_ratio: Option<String>,
    pub duration_seconds: Option<u32>,
    pub resolution: Option<String>,
    pub negative_prompt: Option<String>,
    pub style: Option<String>,
    pub extend_video_id: Option<String>,
    /// `allow_adult` / `allow_all` / `dont_allow`
    pub person_generation: Option<String>,
    pub seed: Option<i64>,
    pub image: Option<VideoImagePart>,
    pub last_frame: Option<VideoImagePart>,
    pub reference_image: Option<VideoImagePart>,
}

fn multipart_image(field: &str, img: VideoImagePart) -> Result<(String, reqwest::multipart::Part)> {
    let part = reqwest::multipart::Part::bytes(img.bytes)
        .file_name(img.filename)
        .mime_str(&img.mime)
        .map_err(|e| anyhow!("视频图片字段 {field} mime 无效: {e}"))?;
    Ok((field.to_string(), part))
}

/// 创建并轮询 Google OpenAI 兼容视频，完成后下载字节。
///
/// `on_progress` 在创建成功与每次轮询时回调（如 `status=processing`）。
pub async fn google_openai_generate_video(
    client: &Client,
    prompt: &str,
    config: &ProviderConfig,
    extras: &VideoGenExtras,
    mut on_progress: Option<&mut dyn FnMut(&str)>,
) -> Result<GeneratedVideo> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    let model = if config.model.trim().is_empty() {
        default_video_model()
    } else {
        config.model.trim()
    };
    let base = google_openai_base(config);
    let create_url = format!("{base}/videos");

    let mut form = reqwest::multipart::Form::new()
        .text("model", model.to_string())
        .text("prompt", prompt.to_string());
    if let Some(ar) = extras
        .aspect_ratio
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        form = form.text("aspect_ratio", ar.to_string());
    }
    if let Some(sec) = extras.duration_seconds.filter(|s| *s > 0) {
        form = form.text("duration_seconds", sec.to_string());
    }
    if let Some(res) = extras
        .resolution
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        form = form.text("resolution", res.to_string());
    }
    if let Some(neg) = extras
        .negative_prompt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        form = form.text("negative_prompt", neg.to_string());
    }
    if let Some(style) = extras
        .style
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        form = form.text("style", style.to_string());
    }
    if let Some(id) = extras
        .extend_video_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        form = form.text("extend_video_id", id.to_string());
    }
    if let Some(pg) = extras
        .person_generation
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        form = form.text("person_generation", pg.to_string());
    }
    if let Some(seed) = extras.seed {
        form = form.text("seed", seed.to_string());
    }
    if let Some(img) = extras.image.clone() {
        let (name, part) = multipart_image("image", img)?;
        form = form.part(name, part);
    }
    if let Some(img) = extras.last_frame.clone() {
        let (name, part) = multipart_image("last_frame", img)?;
        form = form.part(name, part);
    }
    if let Some(img) = extras.reference_image.clone() {
        let (name, part) = multipart_image("reference_images", img)?;
        form = form.part(name, part);
    }

    let emit = |cb: &mut Option<&mut dyn FnMut(&str)>, msg: &str| {
        if let Some(f) = cb.as_mut() {
            f(msg);
        }
    };

    emit(&mut on_progress, "status=creating");
    let create_resp = client
        .post(&create_url)
        .bearer_auth(&config.api_key)
        .multipart(form)
        .send()
        .await
        .with_context(|| format!("创建 Google 视频失败: {create_url}"))?;
    let create_status = create_resp.status();
    let create_body: Value = create_resp
        .json()
        .await
        .context("解析 Google 视频创建响应失败")?;
    if !create_status.is_success() {
        let msg = create_body
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("创建视频失败");
        anyhow::bail!("Google videos HTTP {create_status}: {msg}");
    }

    let op_id = create_body
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("视频响应缺少 id"))?
        .to_string();
    emit(
        &mut on_progress,
        &format!("status=queued operation_id={op_id}"),
    );

    let poll_url = format!("{base}/videos/{op_id}");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10 * 60);
    loop {
        if tokio::time::Instant::now() > deadline {
            anyhow::bail!("视频生成超时（operation id={op_id}）");
        }
        let poll = client
            .get(&poll_url)
            .bearer_auth(&config.api_key)
            .send()
            .await
            .with_context(|| format!("轮询 Google 视频失败: {poll_url}"))?;
        let status = poll.status();
        let body: Value = poll.json().await.context("解析视频轮询响应失败")?;
        if !status.is_success() {
            let msg = body
                .pointer("/error/message")
                .and_then(|m| m.as_str())
                .unwrap_or("轮询视频失败");
            anyhow::bail!("Google videos poll HTTP {status}: {msg}");
        }
        let state = body
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("processing");
        emit(
            &mut on_progress,
            &format!("status={state} operation_id={op_id}"),
        );
        match state {
            "completed" => {
                let url = body
                    .get("url")
                    .and_then(|u| u.as_str())
                    .ok_or_else(|| anyhow!("视频已完成但缺少 url（id={op_id}）"))?;
                emit(&mut on_progress, "status=downloading");
                let bytes = client
                    .get(url)
                    .bearer_auth(&config.api_key)
                    .send()
                    .await
                    .context("下载生成视频失败")?
                    .error_for_status()
                    .context("下载视频 HTTP 失败")?
                    .bytes()
                    .await
                    .context("读取视频字节失败")?;
                emit(&mut on_progress, "status=completed");
                return Ok(GeneratedVideo {
                    data: bytes.to_vec(),
                    mime_type: "video/mp4".to_string(),
                    operation_id: op_id,
                });
            }
            "failed" => {
                let err = body
                    .pointer("/error/message")
                    .and_then(|m| m.as_str())
                    .or_else(|| body.get("error").and_then(|e| e.as_str()))
                    .unwrap_or("unknown");
                anyhow::bail!("视频生成失败（id={op_id}）: {err}");
            }
            _ => sleep(Duration::from_secs(10)).await,
        }
    }
}

/// Google 原生 TTS：`generateContent` + AUDIO modality；返回 wav 字节。
///
/// **已弃用**：请改用 [`crate::interactions_http::google_interactions_tts`]
///（Interactions API）。本函数保留供兼容，工具层不再调用。
#[deprecated(
    note = "use interactions_http::google_interactions_tts (Gemini Interactions API)"
)]
pub async fn google_tts_generate(
    client: &Client,
    text: &str,
    voice: &str,
    config: &ProviderConfig,
) -> Result<Vec<u8>> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    let model = if config.model.trim().is_empty() {
        default_tts_model()
    } else {
        config.model.trim()
    };
    let voice_name = if voice.trim().is_empty()
        || matches!(
            voice.trim().to_ascii_lowercase().as_str(),
            "alloy" | "echo" | "fable" | "onyx" | "nova" | "shimmer"
        ) {
        "Kore"
    } else {
        voice.trim()
    };

    let base = trim_slash(&google_native_base(config));
    let url = if base.contains("/v1beta") {
        format!("{base}/models/{model}:generateContent?key={}", config.api_key)
    } else {
        format!(
            "{base}/v1beta/models/{model}:generateContent?key={}",
            config.api_key
        )
    };

    let body = json!({
        "contents": [{
            "role": "user",
            "parts": [{"text": text}]
        }],
        "generationConfig": {
            "responseModalities": ["AUDIO"],
            "speechConfig": {
                "voiceConfig": {
                    "prebuiltVoiceConfig": {
                        "voiceName": voice_name
                    }
                }
            }
        }
    });

    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 Google TTS 失败: {url}"))?;
    let status = response.status();
    let v: Value = response.json().await.context("解析 Google TTS 响应失败")?;
    if !status.is_success() {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("Google TTS 失败");
        anyhow::bail!("Google TTS HTTP {status}: {msg}");
    }

    let parts = v
        .pointer("/candidates/0/content/parts")
        .and_then(|p| p.as_array())
        .ok_or_else(|| anyhow!("Google TTS 响应无 parts"))?;
    for part in parts {
        let inline = part.get("inlineData").or_else(|| part.get("inline_data"));
        let Some(inline) = inline else { continue };
        let b64 = inline
            .get("data")
            .and_then(|d| d.as_str())
            .ok_or_else(|| anyhow!("TTS inlineData 缺少 data"))?;
        let pcm = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .context("解码 Google TTS PCM 失败")?;
        let rate = mime_sample_rate(
            inline
                .get("mimeType")
                .or_else(|| inline.get("mime_type"))
                .and_then(|m| m.as_str())
                .unwrap_or(""),
        )
        .unwrap_or(24_000);
        return Ok(pcm_to_wav(&pcm, rate, 1, 16));
    }
    anyhow::bail!("Google TTS 未返回音频数据")
}

fn mime_sample_rate(mime: &str) -> Option<u32> {
    // e.g. audio/L16;codec=pcm;rate=24000
    for part in mime.split(';') {
        let p = part.trim();
        if let Some(rest) = p.strip_prefix("rate=") {
            if let Ok(n) = rest.parse::<u32>() {
                return Some(n);
            }
        }
    }
    None
}

/// 将 PCM s16le 封装为 WAV。
pub fn pcm_to_wav(pcm: &[u8], sample_rate: u32, channels: u16, bits_per_sample: u16) -> Vec<u8> {
    let byte_rate = sample_rate * u32::from(channels) * u32::from(bits_per_sample) / 8;
    let block_align = channels * bits_per_sample / 8;
    let data_len = pcm.len() as u32;
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_openai_base_from_compat_endpoint() {
        let cfg = ProviderConfig {
            api_key: "k".into(),
            base_url: Some(
                "https://generativelanguage.googleapis.com/v1beta/openai".into(),
            ),
            model: String::new(),
            ..ProviderConfig::default()
        };
        assert_eq!(
            google_openai_base(&cfg),
            "https://generativelanguage.googleapis.com/v1beta/openai"
        );
    }

    #[test]
    fn pcm_wav_header_size() {
        let wav = pcm_to_wav(&[0u8; 4], 24_000, 1, 16);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 48);
    }

    #[test]
    fn default_vision_models() {
        assert_eq!(default_vision_model("google"), "gemini-3.5-flash");
        assert_eq!(default_vision_model("openai"), "gpt-4o");
    }

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
}
