//! MiniMax 音乐生成 HTTP（`POST /v1/music_generation`）。

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde_json::{json, Value};

use super::defaults::DEFAULT_API_BASE;
use super::tts_http::hex_to_bytes;
use crate::trait_::ProviderConfig;

// ── 类型 ──────────────────────────────────────────────

/// 音乐音频输出设置。
#[derive(Debug, Clone)]
pub struct MusicAudioSetting {
    /// 采样率（默认 44100）。
    pub sample_rate: u32,
    /// 比特率（默认 256000）。
    pub bitrate: u32,
    /// 音频格式（默认 `"mp3"`）。
    pub format: String,
}

impl Default for MusicAudioSetting {
    fn default() -> Self {
        Self {
            sample_rate: 44100,
            bitrate: 256000,
            format: "mp3".to_string(),
        }
    }
}

/// MiniMax 音乐生成请求参数。
#[derive(Debug, Clone)]
pub struct MiniMaxMusicRequest {
    /// 模型名称。
    pub model: String,
    /// 音乐描述/风格提示。
    pub prompt: String,
    /// 歌词文本。
    pub lyrics: String,
    /// 输出格式：`"url"` 或 `"hex"`。
    pub output_format: String,
    /// 音频输出设置。
    pub audio_setting: MusicAudioSetting,
    /// 是否纯器乐（无人声）。
    pub is_instrumental: bool,
    /// 是否启用歌词优化。
    pub lyrics_optimizer: bool,
    /// 翻唱模式：参考音频 URL。
    pub audio_url: Option<String>,
}

impl Default for MiniMaxMusicRequest {
    fn default() -> Self {
        Self {
            model: super::defaults::DEFAULT_MUSIC_MODEL.to_string(),
            prompt: String::new(),
            lyrics: String::new(),
            output_format: "url".to_string(),
            audio_setting: MusicAudioSetting::default(),
            is_instrumental: false,
            lyrics_optimizer: false,
            audio_url: None,
        }
    }
}

/// 音乐生成结果。
#[derive(Debug, Clone)]
pub struct MiniMaxMusicResult {
    /// 原始音频字节。
    pub audio_bytes: Vec<u8>,
    /// MIME 类型。
    pub mime_type: String,
    /// 音乐时长（毫秒）。
    pub duration_ms: u64,
}

// ── 工具函数 ───────────────────────────────────────────

/// 根据音频格式返回 MIME 类型。
fn mime_for_format(fmt: &str) -> &'static str {
    match fmt {
        "mp3" => "audio/mpeg",
        "pcm" => "audio/L16",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        _ => "audio/mpeg",
    }
}

/// 构建 API 基址。
fn minimax_base(config: &ProviderConfig) -> String {
    config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_API_BASE)
        .trim_end_matches('/')
        .to_string()
}

// ── 主函数 ────────────────────────────────────────────

/// 调用 MiniMax 音乐生成 API，返回音频字节。
pub async fn minimax_generate_music(
    client: &Client,
    config: &ProviderConfig,
    req: &MiniMaxMusicRequest,
) -> Result<MiniMaxMusicResult> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("MiniMax API Key 为空");
    }
    if req.prompt.trim().is_empty() && req.lyrics.trim().is_empty() {
        anyhow::bail!("音乐生成需要 prompt 或 lyrics 至少一项非空");
    }

    let base = minimax_base(config);
    let url = format!("{base}/music_generation");

    let model = if req.model.trim().is_empty() {
        super::defaults::DEFAULT_MUSIC_MODEL
    } else {
        req.model.trim()
    };

    let mut body = json!({
        "model": model,
        "prompt": req.prompt,
        "output_format": req.output_format,
        "audio_setting": {
            "sample_rate": req.audio_setting.sample_rate,
            "bitrate": req.audio_setting.bitrate,
            "format": req.audio_setting.format,
        },
        "is_instrumental": req.is_instrumental,
        "lyrics_optimizer": req.lyrics_optimizer,
    });

    if !req.lyrics.trim().is_empty() {
        body["lyrics"] = json!(req.lyrics);
    }
    if let Some(ref audio_url) = req.audio_url {
        body["audio_url"] = json!(audio_url);
    }

    let response = client
        .post(&url)
        .bearer_auth(config.api_key.trim())
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 MiniMax 音乐生成 API 失败: {url}"))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("MiniMax Music HTTP {status}: {text}");
    }

    let json: Value = response
        .json()
        .await
        .context("解析 MiniMax 音乐生成 JSON 失败")?;

    // 检查 base_resp
    let base_resp_code = json
        .pointer("/base_resp/status_code")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if base_resp_code != 0 {
        let msg = json
            .pointer("/base_resp/status_msg")
            .and_then(|v| v.as_str())
            .unwrap_or("未知错误");
        anyhow::bail!("MiniMax 音乐生成业务错误 ({base_resp_code}): {msg}");
    }

    let audio_raw = json
        .pointer("/data/audio")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("MiniMax 音乐生成响应缺少 data.audio 字段"))?;

    let audio_bytes = if req.output_format == "hex" {
        hex_to_bytes(audio_raw)?
    } else {
        // output_format = "url"：audio_raw 是下载链接
        let dl_resp = client
            .get(audio_raw)
            .send()
            .await
            .with_context(|| format!("下载 MiniMax 音乐音频失败: {audio_raw}"))?;
        if !dl_resp.status().is_success() {
            anyhow::bail!(
                "下载 MiniMax 音乐音频 HTTP {}: {}",
                dl_resp.status(),
                audio_raw
            );
        }
        dl_resp
            .bytes()
            .await
            .context("读取 MiniMax 音乐音频字节失败")?
            .to_vec()
    };

    if audio_bytes.is_empty() {
        return Err(anyhow!("MiniMax 音乐生成返回空音频"));
    }

    let duration_ms = json
        .pointer("/extra_info/music_duration")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let fmt = if req.audio_setting.format.is_empty() {
        "mp3"
    } else {
        &req.audio_setting.format
    };

    Ok(MiniMaxMusicResult {
        audio_bytes,
        mime_type: mime_for_format(fmt).to_string(),
        duration_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_request_values() {
        let req = MiniMaxMusicRequest::default();
        assert_eq!(req.model, "music-3.0");
        assert_eq!(req.output_format, "url");
        assert!(!req.is_instrumental);
        assert!(!req.lyrics_optimizer);
        assert!(req.audio_url.is_none());
        assert!(req.prompt.is_empty());
        assert!(req.lyrics.is_empty());
    }

    #[test]
    fn default_music_audio_setting() {
        let a = MusicAudioSetting::default();
        assert_eq!(a.sample_rate, 44100);
        assert_eq!(a.bitrate, 256000);
        assert_eq!(a.format, "mp3");
    }

    #[test]
    fn mime_for_known_formats() {
        assert_eq!(mime_for_format("mp3"), "audio/mpeg");
        assert_eq!(mime_for_format("pcm"), "audio/L16");
        assert_eq!(mime_for_format("flac"), "audio/flac");
        assert_eq!(mime_for_format("wav"), "audio/wav");
        assert_eq!(mime_for_format("unknown"), "audio/mpeg");
    }

    #[test]
    fn body_includes_lyrics_when_present() {
        let req = MiniMaxMusicRequest {
            lyrics: "Hello world".to_string(),
            ..Default::default()
        };
        let mut body = json!({
            "model": req.model,
            "prompt": req.prompt,
            "output_format": req.output_format,
            "audio_setting": {
                "sample_rate": req.audio_setting.sample_rate,
                "bitrate": req.audio_setting.bitrate,
                "format": req.audio_setting.format,
            },
            "is_instrumental": req.is_instrumental,
            "lyrics_optimizer": req.lyrics_optimizer,
        });
        if !req.lyrics.trim().is_empty() {
            body["lyrics"] = json!(req.lyrics);
        }
        assert_eq!(body["lyrics"], "Hello world");
    }

    #[test]
    fn body_omits_lyrics_when_empty() {
        let req = MiniMaxMusicRequest::default();
        let mut body = json!({
            "model": req.model,
            "prompt": req.prompt,
        });
        if !req.lyrics.trim().is_empty() {
            body["lyrics"] = json!(req.lyrics);
        }
        assert!(body.get("lyrics").is_none());
    }

    #[test]
    fn body_includes_audio_url_when_present() {
        let req = MiniMaxMusicRequest {
            audio_url: Some("https://example.com/audio.mp3".to_string()),
            ..Default::default()
        };
        let mut body = json!({"model": req.model});
        if let Some(ref audio_url) = req.audio_url {
            body["audio_url"] = json!(audio_url);
        }
        assert_eq!(body["audio_url"], "https://example.com/audio.mp3");
    }
}
