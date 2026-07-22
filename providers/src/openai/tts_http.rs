//! OpenAI TTS HTTP（`POST /audio/speech`）。

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde_json::{json, Value};

use super::defaults::DEFAULT_API_BASE;
use crate::http_stream::openai_compatible_base;
use crate::trait_::ProviderConfig;

/// 默认 TTS 模型。
pub fn default_tts_model() -> &'static str {
    super::defaults::DEFAULT_TTS_MODEL
}

/// TTS 请求参数。
#[derive(Debug, Clone)]
pub struct OpenAiTtsRequest {
    /// 模型，如 `"tts-1"` / `"tts-1-hd"` / `"gpt-4o-mini-tts"`。
    pub model: String,
    /// 待合成文本（最长 4096 字符）。
    pub input: String,
    /// 音色：`alloy` / `echo` / `fable` / `onyx` / `nova` / `shimmer`。
    pub voice: String,
    /// 输出格式：`mp3` / `opus` / `aac` / `flac` / `wav` / `pcm`。
    pub response_format: String,
    /// 语速倍率（0.25 – 4.0）。
    pub speed: f32,
}

impl Default for OpenAiTtsRequest {
    fn default() -> Self {
        Self {
            model: default_tts_model().to_string(),
            input: String::new(),
            voice: "alloy".to_string(),
            response_format: "mp3".to_string(),
            speed: 1.0,
        }
    }
}

/// TTS 结果。
#[derive(Debug, Clone)]
pub struct OpenAiTtsResult {
    /// 原始音频字节。
    pub audio_bytes: Vec<u8>,
    /// MIME 类型。
    pub mime_type: String,
}

fn mime_for_format(fmt: &str) -> &'static str {
    match fmt {
        "mp3" => "audio/mpeg",
        "opus" => "audio/opus",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "pcm" => "audio/L16",
        _ => "audio/mpeg",
    }
}

fn openai_base(config: &ProviderConfig) -> String {
    let raw = config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_API_BASE);
    openai_compatible_base(raw)
}

/// 调用 OpenAI TTS API，返回原始音频字节。
pub async fn openai_tts(
    client: &Client,
    config: &ProviderConfig,
    req: &OpenAiTtsRequest,
) -> Result<OpenAiTtsResult> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("OpenAI API Key 为空");
    }
    if req.input.trim().is_empty() {
        anyhow::bail!("TTS input 为空");
    }

    let model = if req.model.trim().is_empty() {
        default_tts_model()
    } else {
        req.model.trim()
    };

    let base = openai_base(config);
    let url = format!("{base}/audio/speech");

    let mut body = json!({
        "model": model,
        "input": req.input,
        "voice": req.voice,
    });
    if !req.response_format.is_empty() {
        body["response_format"] = json!(req.response_format);
    }
    if (req.speed - 1.0).abs() > f32::EPSILON {
        body["speed"] = json!(req.speed);
    }

    let response = client
        .post(&url)
        .bearer_auth(config.api_key.trim())
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 OpenAI TTS API 失败: {url}"))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| {
                v.pointer("/error/message")
                    .and_then(|m| m.as_str())
                    .map(str::to_string)
            })
            .unwrap_or(text);
        anyhow::bail!("OpenAI TTS HTTP {status}: {msg}");
    }

    let bytes = response
        .bytes()
        .await
        .context("读取 OpenAI TTS 音频字节失败")?;

    if bytes.is_empty() {
        return Err(anyhow!("OpenAI TTS 返回空音频"));
    }

    let fmt = if req.response_format.is_empty() {
        "mp3"
    } else {
        &req.response_format
    };

    Ok(OpenAiTtsResult {
        audio_bytes: bytes.to_vec(),
        mime_type: mime_for_format(fmt).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_model_is_tts1() {
        assert_eq!(default_tts_model(), "tts-1");
    }

    #[test]
    fn default_request_values() {
        let req = OpenAiTtsRequest::default();
        assert_eq!(req.model, "tts-1");
        assert_eq!(req.voice, "alloy");
        assert_eq!(req.response_format, "mp3");
        assert!((req.speed - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn mime_for_known_formats() {
        assert_eq!(mime_for_format("mp3"), "audio/mpeg");
        assert_eq!(mime_for_format("opus"), "audio/opus");
        assert_eq!(mime_for_format("aac"), "audio/aac");
        assert_eq!(mime_for_format("flac"), "audio/flac");
        assert_eq!(mime_for_format("wav"), "audio/wav");
        assert_eq!(mime_for_format("pcm"), "audio/L16");
        assert_eq!(mime_for_format("unknown"), "audio/mpeg");
    }
}
