//! OpenAI 兼容媒体：Whisper 转写、Chat 音频描述、视觉 completions。
//!
//! 视觉模式见 [`crate::protocol::vision::VisionMode`]；
//! Google 原生视觉请用 [`crate::google::interactions_http`]。

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use reqwest::Client;
use serde_json::{json, Value};

use super::defaults::{DEFAULT_API_BASE, DEFAULT_VISION_MODEL};
use crate::protocol::http_stream::openai_compatible_base;
use crate::protocol::vision::VisionMode;
use crate::trait_::ProviderConfig;

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
        DEFAULT_VISION_MODEL
    } else {
        config.model.trim()
    };
    let raw = config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_API_BASE);
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
        .unwrap_or(DEFAULT_API_BASE);
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
        DEFAULT_VISION_MODEL
    } else {
        config.model.trim()
    };

    let raw = config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_API_BASE);
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let urls = vec![
            "https://a/1.jpg".to_string(),
            "data:image/png;base64,AAAA".to_string(),
        ];
        let body = build_openai_vision_body("gpt-4o", "detect please", &urls, VisionMode::Detect);
        assert_eq!(body["model"], "gpt-4o");
        assert_eq!(body["response_format"]["type"], "json_object");
        let content = body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content.len(), 3);
    }

    #[test]
    fn openai_vision_body_describe_omits_response_format() {
        let body = build_openai_vision_body(
            "gpt-4o",
            "hi",
            &["https://a/1.jpg".into()],
            VisionMode::Describe,
        );
        assert!(body.get("response_format").is_none());
    }
}
