//! MiniMax 音乐生成 HTTP（`POST /v1/music_generation`）。

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde_json::{json, Value};

use super::defaults::DEFAULT_API_BASE;
use super::tts_http::hex_to_bytes;
use crate::types::request::ProviderConfig;

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
    /// 翻唱模式：参考音频 base64。
    pub audio_base64: Option<String>,
    /// 两步翻唱：前处理返回的特征 ID（与 audio_url/audio_base64 互斥）。
    pub cover_feature_id: Option<String>,
    /// 是否添加 AIGC 水印（仅非流式模式）。
    pub aigc_watermark: bool,
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
            audio_base64: None,
            cover_feature_id: None,
            aigc_watermark: false,
        }
    }
}

/// 歌词生成结果。
#[derive(Debug, Clone)]
pub struct MiniMaxLyricsResult {
    pub song_title: String,
    pub style_tags: String,
    pub lyrics: String,
}

/// 翻唱前处理结果。
#[derive(Debug, Clone)]
pub struct MiniMaxCoverPreprocessResult {
    pub cover_feature_id: String,
    pub formatted_lyrics: String,
    pub structure_result: String,
    pub audio_duration: f64,
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
    if let Some(ref audio_b64) = req.audio_base64 {
        body["audio_base64"] = json!(audio_b64);
    }
    if let Some(ref cfi) = req.cover_feature_id {
        body["cover_feature_id"] = json!(cfi);
    }
    if req.aigc_watermark {
        body["aigc_watermark"] = json!(true);
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

// ── 歌词生成 ────────────────────────────────────────────

/// 调用 MiniMax 歌词生成 API。
///
/// `mode`：`"write_full_song"` 或 `"edit"`。
pub async fn minimax_generate_lyrics(
    client: &Client,
    config: &ProviderConfig,
    mode: &str,
    prompt: &str,
    lyrics: Option<&str>,
    title: Option<&str>,
) -> Result<MiniMaxLyricsResult> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("MiniMax API Key 为空");
    }

    let base = minimax_base(config);
    let url = format!("{base}/lyrics_generation");

    let mut body = json!({ "mode": mode });
    if !prompt.is_empty() {
        body["prompt"] = json!(prompt);
    }
    if let Some(l) = lyrics.filter(|s| !s.is_empty()) {
        body["lyrics"] = json!(l);
    }
    if let Some(t) = title.filter(|s| !s.is_empty()) {
        body["title"] = json!(t);
    }

    let resp = client
        .post(&url)
        .bearer_auth(config.api_key.trim())
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .context("连接 MiniMax 歌词生成 API 失败")?;

    let status = resp.status();
    let v: Value = resp.json().await.context("解析歌词生成响应失败")?;
    if !status.is_success() {
        let msg = v
            .pointer("/base_resp/status_msg")
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误");
        anyhow::bail!("MiniMax 歌词生成 HTTP {status}: {msg}");
    }
    let code = v
        .pointer("/base_resp/status_code")
        .and_then(|c| c.as_i64())
        .unwrap_or(0);
    if code != 0 {
        let msg = v
            .pointer("/base_resp/status_msg")
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误");
        anyhow::bail!("MiniMax 歌词生成业务错误 ({code}): {msg}");
    }

    Ok(MiniMaxLyricsResult {
        song_title: v
            .get("song_title")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        style_tags: v
            .get("style_tags")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        lyrics: v
            .get("lyrics")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

// ── 翻唱前处理 ──────────────────────────────────────────

/// 调用 MiniMax 翻唱前处理 API，提取音频特征和歌词。
pub async fn minimax_cover_preprocess(
    client: &Client,
    config: &ProviderConfig,
    audio_url: Option<&str>,
    audio_base64: Option<&str>,
) -> Result<MiniMaxCoverPreprocessResult> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("MiniMax API Key 为空");
    }
    if audio_url.is_none() && audio_base64.is_none() {
        anyhow::bail!("翻唱前处理需要 audio_url 或 audio_base64");
    }

    let base = minimax_base(config);
    let url = format!("{base}/music_cover_preprocess");

    let mut body = json!({ "model": "music-cover" });
    if let Some(u) = audio_url {
        body["audio_url"] = json!(u);
    }
    if let Some(b) = audio_base64 {
        body["audio_base64"] = json!(b);
    }

    let resp = client
        .post(&url)
        .bearer_auth(config.api_key.trim())
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .context("连接 MiniMax 翻唱前处理 API 失败")?;

    let status = resp.status();
    let v: Value = resp.json().await.context("解析翻唱前处理响应失败")?;
    if !status.is_success() {
        let msg = v
            .pointer("/base_resp/status_msg")
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误");
        anyhow::bail!("MiniMax 翻唱前处理 HTTP {status}: {msg}");
    }
    let code = v
        .pointer("/base_resp/status_code")
        .and_then(|c| c.as_i64())
        .unwrap_or(0);
    if code != 0 {
        let msg = v
            .pointer("/base_resp/status_msg")
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误");
        anyhow::bail!("MiniMax 翻唱前处理业务错误 ({code}): {msg}");
    }

    Ok(MiniMaxCoverPreprocessResult {
        cover_feature_id: v
            .get("cover_feature_id")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        formatted_lyrics: v
            .get("formatted_lyrics")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        structure_result: v
            .get("structure_result")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string(),
        audio_duration: v
            .get("audio_duration")
            .and_then(|n| n.as_f64())
            .unwrap_or(0.0),
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
