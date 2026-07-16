//! Google 原生 Veo 与 OpenAI 媒体 HTTP 辅助。
//!
//! Google 出图/TTS/视觉请用 [`crate::interactions_http`]。

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use reqwest::Client;
use serde_json::{json, Value};
use tokio::time::sleep;

use crate::http_stream::openai_compatible_base;
use crate::interactions_http::VisionMode;
use crate::trait_::ProviderConfig;

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
/// 仅 OpenAI 兼容 base（默认 `https://api.openai.com/v1`）；Google 请用
/// [`crate::interactions_http::google_interactions_vision`]。
/// `image_url` 可为 `data:image/...;base64,...` 或 `http(s)://`。

pub(crate) fn build_openai_vision_body(
    model: &str,
    prompt: &str,
    image_urls: &[String],
    mode: VisionMode,
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
    if matches!(mode, VisionMode::Detect | VisionMode::Segment) {
        body["response_format"] = json!({ "type": "json_object" });
    }
    body
}

/// OpenAI 兼容视觉：`POST …/chat/completions`，content 含 text + image_url。
///
/// 仅 OpenAI 兼容 base（默认 `https://api.openai.com/v1`）；Google 请用
/// [`crate::interactions_http::google_interactions_vision`]。
/// `image_urls` 可为 `data:image/...;base64,...` 或 `http(s)://`。
pub async fn openai_vision_completions(
    client: &Client,
    prompt: &str,
    image_urls: &[String],
    mode: VisionMode,
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
    if let Some(s) = content.as_str() {
        return Ok(s.to_string());
    }
    // 部分兼容层可能返回 content 数组
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
    anyhow::bail!("视觉响应 content 格式无法解析")
}

/// 视频生成结果。
#[derive(Debug, Clone)]
pub struct GeneratedVideo {
    pub data: Vec<u8>,
    pub mime_type: String,
    pub operation_id: String,
    pub video_uri: Option<String>,
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
    pub extend_video_uri: Option<String>,
    /// 续拍视频字节（本地文件或已下载 URI）；mime 通常 `video/mp4`
    pub extend_video: Option<VideoImagePart>,
    /// `allow_adult` / `allow_all` / `dont_allow`
    pub person_generation: Option<String>,
    pub seed: Option<i64>,
    pub image: Option<VideoImagePart>,
    pub last_frame: Option<VideoImagePart>,
    pub reference_images: Vec<VideoImagePart>,
}

/// Google 原生 API 根，始终以 `…/v1beta` 结尾。
pub fn google_v1beta_root(config: &ProviderConfig) -> String {
    let native = trim_slash(&google_native_base(config));
    if native.ends_with("/v1beta") {
        native
    } else if native.contains("/v1beta/") {
        let idx = native.find("/v1beta").unwrap();
        format!("{}{}", &native[..idx], "/v1beta")
    } else {
        format!("{native}/v1beta")
    }
}

pub fn google_veo_predict_url(config: &ProviderConfig, model: &str) -> String {
    format!(
        "{}/models/{}:predictLongRunning",
        google_v1beta_root(config),
        model.trim()
    )
}

pub fn google_operation_url(config: &ProviderConfig, operation_name: &str) -> String {
    let name = operation_name.trim().trim_start_matches('/');
    format!("{}/{}", google_v1beta_root(config), name)
}

fn inline_data_value(part: &VideoImagePart) -> Value {
    json!({
        "inlineData": {
            "mimeType": part.mime,
            "data": base64::engine::general_purpose::STANDARD.encode(&part.bytes),
        }
    })
}

pub fn build_veo_predict_body(prompt: &str, extras: &VideoGenExtras) -> Value {
    let mut instance = json!({ "prompt": prompt });
    if let Some(img) = &extras.image {
        instance["image"] = inline_data_value(img);
    }
    if let Some(img) = &extras.last_frame {
        instance["lastFrame"] = inline_data_value(img);
    }
    if !extras.reference_images.is_empty() {
        instance["referenceImages"] = Value::Array(
            extras
                .reference_images
                .iter()
                .map(|img| {
                    json!({
                        "image": inline_data_value(img),
                        "referenceType": "asset",
                    })
                })
                .collect(),
        );
    }
    if let Some(vid) = &extras.extend_video {
        instance["video"] = inline_data_value(vid);
    }

    let mut parameters = serde_json::Map::new();
    if let Some(ar) = extras
        .aspect_ratio
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        parameters.insert("aspectRatio".into(), json!(ar));
    }
    if let Some(sec) = extras.duration_seconds.filter(|s| *s > 0) {
        parameters.insert("durationSeconds".into(), json!(sec));
    }
    if let Some(res) = extras
        .resolution
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let normalized = if res.eq_ignore_ascii_case("4k") {
            "4k".to_string()
        } else {
            res.to_ascii_lowercase()
        };
        parameters.insert("resolution".into(), json!(normalized));
    }
    if let Some(pg) = extras
        .person_generation
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        parameters.insert("personGeneration".into(), json!(pg));
    }
    if let Some(seed) = extras.seed {
        parameters.insert("seed".into(), json!(seed));
    }

    let mut body = json!({ "instances": [instance] });
    if !parameters.is_empty() {
        body["parameters"] = Value::Object(parameters);
    }
    body
}

pub fn extract_veo_operation_name(create_body: &Value) -> Result<String> {
    create_body
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("Veo 响应缺少 operation name"))
}

pub fn extract_veo_video_uri(status_body: &Value) -> Result<String> {
    if status_body.get("done") != Some(&json!(true))
        && status_body.pointer("/done").and_then(|d| d.as_bool()) != Some(true)
    {
        anyhow::bail!("Veo operation 尚未完成");
    }
    if let Some(msg) = status_body
        .pointer("/error/message")
        .and_then(|m| m.as_str())
        .or_else(|| status_body.get("error").and_then(|e| e.as_str()))
    {
        anyhow::bail!("Veo 生成失败: {msg}");
    }
    status_body
        .pointer("/response/generateVideoResponse/generatedSamples/0/video/uri")
        .and_then(|u| u.as_str())
        .or_else(|| {
            status_body
                .pointer("/response/generate_video_response/generated_samples/0/video/uri")
                .and_then(|u| u.as_str())
        })
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("Veo 完成但缺少 video.uri"))
}

async fn google_api_get_bytes(client: &Client, url: &str, api_key: &str) -> Result<Vec<u8>> {
    let response = client
        .get(url)
        .header("x-goog-api-key", api_key)
        .send()
        .await
        .with_context(|| format!("GET 失败: {url}"))?;
    let status = response.status();
    if status.is_success() {
        return response
            .bytes()
            .await
            .context("读取响应字节失败")
            .map(|b| b.to_vec());
    }
    let v: Value = response.json().await.unwrap_or(json!({}));
    let msg = v
        .pointer("/error/message")
        .and_then(|m| m.as_str())
        .unwrap_or("GET 请求失败");
    anyhow::bail!("GET {status}: {msg}");
}

/// Google 原生 Veo：`predictLongRunning` 创建、轮询 operation、下载视频字节。
///
/// 若 `extend_video` 为空且 `extend_video_uri` 有值，会先 GET 该 URI（带 API key）
/// 再写入请求体 `video.inlineData`。
pub async fn google_native_generate_video(
    client: &Client,
    prompt: &str,
    config: &ProviderConfig,
    extras: &VideoGenExtras,
    mut on_progress: Option<&mut dyn FnMut(&str)>,
) -> Result<GeneratedVideo> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    let api_key = config.api_key.trim();
    let model = if config.model.trim().is_empty() {
        default_video_model()
    } else {
        config.model.trim()
    };

    let mut body_extras = extras.clone();
    if body_extras.extend_video.is_none() {
        if let Some(uri) = extras
            .extend_video_uri
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let bytes = google_api_get_bytes(client, uri, api_key)
                .await
                .with_context(|| format!("下载 extend_video_uri 失败: {uri}"))?;
            body_extras.extend_video = Some(VideoImagePart {
                mime: "video/mp4".into(),
                filename: "extend.mp4".into(),
                bytes,
            });
        }
    }

    let predict_url = google_veo_predict_url(config, model);
    let body = build_veo_predict_body(prompt, &body_extras);

    let emit = |cb: &mut Option<&mut dyn FnMut(&str)>, msg: &str| {
        if let Some(f) = cb.as_mut() {
            f(msg);
        }
    };

    let create_resp = client
        .post(&predict_url)
        .header("x-goog-api-key", api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("Veo predictLongRunning 失败: {predict_url}"))?;
    let create_status = create_resp.status();
    let create_body: Value = create_resp
        .json()
        .await
        .context("解析 Veo 创建响应失败")?;
    if !create_status.is_success() {
        let msg = create_body
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("Veo predictLongRunning 失败");
        anyhow::bail!("Veo predict HTTP {create_status}: {msg}");
    }

    let op_name = extract_veo_operation_name(&create_body)?;
    emit(
        &mut on_progress,
        &format!("status=queued operation_name={op_name}"),
    );

    let poll_url = google_operation_url(config, &op_name);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10 * 60);
    let video_uri = loop {
        if tokio::time::Instant::now() > deadline {
            anyhow::bail!("Veo 生成超时（operation name={op_name}）");
        }
        let poll = client
            .get(&poll_url)
            .header("x-goog-api-key", api_key)
            .send()
            .await
            .with_context(|| format!("轮询 Veo operation 失败: {poll_url}"))?;
        let status = poll.status();
        let poll_body: Value = poll.json().await.context("解析 Veo 轮询响应失败")?;
        if !status.is_success() {
            let msg = poll_body
                .pointer("/error/message")
                .and_then(|m| m.as_str())
                .unwrap_or("轮询 Veo operation 失败");
            anyhow::bail!("Veo operation poll HTTP {status}: {msg}");
        }
        let done = poll_body.get("done") == Some(&json!(true))
            || poll_body.pointer("/done").and_then(|d| d.as_bool()) == Some(true);
        if done {
            break extract_veo_video_uri(&poll_body)?;
        }
        emit(
            &mut on_progress,
            &format!("status=processing operation_name={op_name}"),
        );
        sleep(Duration::from_secs(10)).await;
    };

    emit(&mut on_progress, "status=downloading");
    let bytes = google_api_get_bytes(client, &video_uri, api_key)
        .await
        .with_context(|| format!("下载 Veo 视频失败: {video_uri}"))?;
    emit(&mut on_progress, "status=completed");

    Ok(GeneratedVideo {
        data: bytes,
        mime_type: "video/mp4".into(),
        operation_id: op_name,
        video_uri: Some(video_uri),
    })
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

    #[test]
    fn google_v1beta_root_strips_openai_suffix() {
        let cfg = ProviderConfig {
            api_key: "k".into(),
            base_url: Some(
                "https://generativelanguage.googleapis.com/v1beta/openai".into(),
            ),
            model: String::new(),
            ..ProviderConfig::default()
        };
        assert_eq!(
            google_v1beta_root(&cfg),
            "https://generativelanguage.googleapis.com/v1beta"
        );
        assert_eq!(
            google_veo_predict_url(&cfg, "veo-3.1-generate-preview"),
            "https://generativelanguage.googleapis.com/v1beta/models/veo-3.1-generate-preview:predictLongRunning"
        );
        assert_eq!(
            google_operation_url(&cfg, "operations/abc123"),
            "https://generativelanguage.googleapis.com/v1beta/operations/abc123"
        );
    }

    #[test]
    fn build_veo_predict_body_text_and_params() {
        let extras = VideoGenExtras {
            aspect_ratio: Some("9:16".into()),
            resolution: Some("4K".into()),
            duration_seconds: Some(8),
            person_generation: Some("allow_adult".into()),
            seed: Some(7),
            ..Default::default()
        };
        let body = build_veo_predict_body("a lion", &extras);
        assert_eq!(body["instances"][0]["prompt"], "a lion");
        assert_eq!(body["parameters"]["aspectRatio"], "9:16");
        assert_eq!(body["parameters"]["resolution"], "4k");
        assert_eq!(body["parameters"]["durationSeconds"], 8);
        assert_eq!(body["parameters"]["personGeneration"], "allow_adult");
        assert_eq!(body["parameters"]["seed"], 7);
        assert!(body["instances"][0].get("image").is_none());
    }

    #[test]
    fn build_veo_predict_body_frames_refs_and_extend() {
        let img = VideoImagePart {
            bytes: b"PNG".to_vec(),
            filename: "a.png".into(),
            mime: "image/png".into(),
        };
        let vid = VideoImagePart {
            bytes: b"MP4".to_vec(),
            filename: "v.mp4".into(),
            mime: "video/mp4".into(),
        };
        let extras = VideoGenExtras {
            image: Some(img.clone()),
            last_frame: Some(img.clone()),
            reference_images: vec![img.clone(), img],
            extend_video: Some(vid),
            ..Default::default()
        };
        let body = build_veo_predict_body("interp", &extras);
        let inst = &body["instances"][0];
        assert!(inst["image"]["inlineData"]["data"].as_str().unwrap().len() > 0);
        assert!(inst["lastFrame"]["inlineData"]["data"].is_string());
        assert_eq!(inst["referenceImages"].as_array().unwrap().len(), 2);
        assert_eq!(inst["referenceImages"][0]["referenceType"], "asset");
        assert!(inst["video"]["inlineData"]["data"].is_string());
        assert_eq!(inst["video"]["inlineData"]["mimeType"], "video/mp4");
    }

    #[test]
    fn extract_veo_video_uri_success_and_error() {
        let ok = serde_json::json!({
            "done": true,
            "response": {
                "generateVideoResponse": {
                    "generatedSamples": [{
                        "video": { "uri": "https://example.com/v.mp4" }
                    }]
                }
            }
        });
        assert_eq!(
            extract_veo_video_uri(&ok).unwrap(),
            "https://example.com/v.mp4"
        );
        let err = serde_json::json!({
            "done": true,
            "error": { "message": "blocked" }
        });
        assert!(extract_veo_video_uri(&err)
            .unwrap_err()
            .to_string()
            .contains("blocked"));
    }
}
