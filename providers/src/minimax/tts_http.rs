//! MiniMax TTS HTTP（`POST /v1/t2a_v2`）。

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde_json::{json, Value};

use super::defaults::DEFAULT_API_BASE;
use crate::trait_::ProviderConfig;

// ── 类型 ──────────────────────────────────────────────

/// 音色设置。
#[derive(Debug, Clone)]
pub struct VoiceSetting {
    /// 音色 ID。
    pub voice_id: String,
    /// 语速倍率 [0.5, 2]（默认 1.0）。
    pub speed: f32,
    /// 音量倍率 (0, 10]（默认 1.0）。
    pub vol: f32,
    /// 音高偏移 [-12, 12]（默认 0）。
    pub pitch: i32,
    /// 情绪控制（happy/sad/angry/fearful/disgusted/surprised/calm/fluent/whisper）。
    pub emotion: Option<String>,
    /// 是否启用文本规范化（数字阅读场景）。
    pub text_normalization: bool,
    /// 是否朗读 LaTeX 公式。
    pub latex_read: bool,
}

impl Default for VoiceSetting {
    fn default() -> Self {
        Self {
            voice_id: String::new(),
            speed: 1.0,
            vol: 1.0,
            pitch: 0,
            emotion: None,
            text_normalization: false,
            latex_read: false,
        }
    }
}

/// 混合音色权重。
#[derive(Debug, Clone)]
pub struct TimbreWeight {
    pub voice_id: String,
    pub weight: u32,
}

/// 声音效果器。
#[derive(Debug, Clone)]
pub struct VoiceModify {
    /// 音高调整 [-100, 100]。
    pub pitch: Option<i32>,
    /// 强度调整 [-100, 100]。
    pub intensity: Option<i32>,
    /// 音色调整 [-100, 100]。
    pub timbre: Option<i32>,
    /// 音效：spacious_echo / auditorium_echo / lofi_telephone / robotic。
    pub sound_effects: Option<String>,
}

/// 音频输出设置。
#[derive(Debug, Clone)]
pub struct AudioSetting {
    /// 采样率（默认 32000）。
    pub sample_rate: u32,
    /// 比特率（默认 128000）。
    pub bitrate: u32,
    /// 音频格式：mp3/pcm/flac/wav/pcmu_raw/pcmu_wav/opus（默认 `"mp3"`）。
    pub format: String,
    /// 声道数（默认 1）。
    pub channel: u8,
    /// 是否恒定比特率（仅流式 mp3）。
    pub force_cbr: bool,
}

impl Default for AudioSetting {
    fn default() -> Self {
        Self {
            sample_rate: 32000,
            bitrate: 128000,
            format: "mp3".to_string(),
            channel: 1,
            force_cbr: false,
        }
    }
}

/// MiniMax TTS 请求参数。
#[derive(Debug, Clone)]
pub struct MiniMaxTtsRequest {
    /// 模型名称。
    pub model: String,
    /// 待合成文本。
    pub text: String,
    /// 音色设置。
    pub voice_setting: VoiceSetting,
    /// 音频输出设置。
    pub audio_setting: AudioSetting,
    /// 输出格式：`"url"` 或 `"hex"`。
    pub output_format: String,
    /// 语言增强。
    pub language_boost: Option<String>,
    /// 发音词典（注音/替换规则）。
    pub pronunciation_dict: Option<Vec<String>>,
    /// 混合音色权重。
    pub timbre_weights: Option<Vec<TimbreWeight>>,
    /// 声音效果器。
    pub voice_modify: Option<VoiceModify>,
    /// 是否开启字幕。
    pub subtitle_enable: bool,
    /// 字幕粒度：sentence / word / word_streaming。
    pub subtitle_type: Option<String>,
    /// 是否添加 AIGC 水印（仅非流式）。
    pub aigc_watermark: bool,
}

impl Default for MiniMaxTtsRequest {
    fn default() -> Self {
        Self {
            model: super::defaults::DEFAULT_TTS_MODEL.to_string(),
            text: String::new(),
            voice_setting: VoiceSetting::default(),
            audio_setting: AudioSetting::default(),
            output_format: "url".to_string(),
            language_boost: None,
            pronunciation_dict: None,
            timbre_weights: None,
            voice_modify: None,
            subtitle_enable: false,
            subtitle_type: None,
            aigc_watermark: false,
        }
    }
}

/// TTS 合成结果。
#[derive(Debug, Clone)]
pub struct MiniMaxTtsResult {
    /// 原始音频字节。
    pub audio_bytes: Vec<u8>,
    /// MIME 类型。
    pub mime_type: String,
    /// 音频时长（毫秒）。
    pub duration_ms: u64,
}

// ── 工具函数 ───────────────────────────────────────────

/// 将 hex 字符串解码为字节。`music_http` 复用此函数。
pub(crate) fn hex_to_bytes(hex: &str) -> Result<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        anyhow::bail!("hex 字符串长度非偶数");
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).context("hex 解码失败"))
        .collect()
}

/// 根据音频格式返回 MIME 类型。
fn mime_for_format(fmt: &str) -> &'static str {
    match fmt {
        "mp3" => "audio/mpeg",
        "pcm" | "pcmu_raw" => "audio/L16",
        "flac" => "audio/flac",
        "wav" | "pcmu_wav" => "audio/wav",
        "opus" => "audio/ogg",
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

/// 调用 MiniMax TTS API，返回音频字节。
pub async fn minimax_tts(
    client: &Client,
    config: &ProviderConfig,
    req: &MiniMaxTtsRequest,
) -> Result<MiniMaxTtsResult> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("MiniMax API Key 为空");
    }
    if req.text.trim().is_empty() {
        anyhow::bail!("TTS text 为空");
    }

    let base = minimax_base(config);
    let url = format!("{base}/t2a_v2");

    let model = if req.model.trim().is_empty() {
        super::defaults::DEFAULT_TTS_MODEL
    } else {
        req.model.trim()
    };

    let mut vs = json!({
        "voice_id": req.voice_setting.voice_id,
        "speed": req.voice_setting.speed,
        "vol": req.voice_setting.vol,
        "pitch": req.voice_setting.pitch,
    });
    if let Some(ref emotion) = req.voice_setting.emotion {
        vs["emotion"] = json!(emotion);
    }
    if req.voice_setting.text_normalization {
        vs["text_normalization"] = json!(true);
    }
    if req.voice_setting.latex_read {
        vs["latex_read"] = json!(true);
    }

    let mut aus = json!({
        "sample_rate": req.audio_setting.sample_rate,
        "bitrate": req.audio_setting.bitrate,
        "format": req.audio_setting.format,
        "channel": req.audio_setting.channel,
    });
    if req.audio_setting.force_cbr {
        aus["force_cbr"] = json!(true);
    }

    let mut body = json!({
        "model": model,
        "text": req.text,
        "stream": false,
        "voice_setting": vs,
        "audio_setting": aus,
        "output_format": req.output_format,
    });

    if let Some(ref lang) = req.language_boost {
        body["language_boost"] = json!(lang);
    }
    if let Some(ref dict) = req.pronunciation_dict {
        body["pronunciation_dict"] = json!({"tone": dict});
    }
    if let Some(ref tw) = req.timbre_weights {
        let arr: Vec<Value> = tw.iter().map(|t| json!({"voice_id": t.voice_id, "weight": t.weight})).collect();
        body["timbre_weights"] = Value::Array(arr);
    }
    if let Some(ref vm) = req.voice_modify {
        let mut obj = serde_json::Map::new();
        if let Some(p) = vm.pitch { obj.insert("pitch".into(), json!(p)); }
        if let Some(i) = vm.intensity { obj.insert("intensity".into(), json!(i)); }
        if let Some(t) = vm.timbre { obj.insert("timbre".into(), json!(t)); }
        if let Some(ref se) = vm.sound_effects { obj.insert("sound_effects".into(), json!(se)); }
        if !obj.is_empty() {
            body["voice_modify"] = Value::Object(obj);
        }
    }
    if req.subtitle_enable {
        body["subtitle_enable"] = json!(true);
        if let Some(ref st) = req.subtitle_type {
            body["subtitle_type"] = json!(st);
        }
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
        .with_context(|| format!("连接 MiniMax TTS API 失败: {url}"))?;

    let status = response.status();
    if !status.is_success() {
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("MiniMax TTS HTTP {status}: {text}");
    }

    let json: Value = response
        .json()
        .await
        .context("解析 MiniMax TTS JSON 失败")?;

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
        anyhow::bail!("MiniMax TTS 业务错误 ({base_resp_code}): {msg}");
    }

    let audio_raw = json
        .pointer("/data/audio")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("MiniMax TTS 响应缺少 data.audio 字段"))?;

    let audio_bytes = if req.output_format == "hex" {
        hex_to_bytes(audio_raw)?
    } else {
        // output_format = "url"：audio_raw 是下载链接
        let dl_resp = client
            .get(audio_raw)
            .send()
            .await
            .with_context(|| format!("下载 MiniMax TTS 音频失败: {audio_raw}"))?;
        if !dl_resp.status().is_success() {
            anyhow::bail!(
                "下载 MiniMax TTS 音频 HTTP {}: {}",
                dl_resp.status(),
                audio_raw
            );
        }
        dl_resp
            .bytes()
            .await
            .context("读取 MiniMax TTS 音频字节失败")?
            .to_vec()
    };

    if audio_bytes.is_empty() {
        return Err(anyhow!("MiniMax TTS 返回空音频"));
    }

    let duration_ms = json
        .pointer("/extra_info/audio_length")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let fmt = if req.audio_setting.format.is_empty() {
        "mp3"
    } else {
        &req.audio_setting.format
    };

    Ok(MiniMaxTtsResult {
        audio_bytes,
        mime_type: mime_for_format(fmt).to_string(),
        duration_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_to_bytes_known_values() {
        assert_eq!(hex_to_bytes("48656c6c6f").unwrap(), b"Hello");
        assert_eq!(hex_to_bytes("00ff").unwrap(), vec![0x00, 0xff]);
        assert_eq!(hex_to_bytes("").unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn hex_to_bytes_odd_length() {
        assert!(hex_to_bytes("abc").is_err());
    }

    #[test]
    fn hex_to_bytes_invalid_chars() {
        assert!(hex_to_bytes("zzzz").is_err());
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
    fn default_request_values() {
        let req = MiniMaxTtsRequest::default();
        assert_eq!(req.model, "speech-2.8-hd");
        assert_eq!(req.output_format, "url");
        assert!(req.language_boost.is_none());
        assert!((req.voice_setting.speed - 1.0).abs() < f32::EPSILON);
        assert!((req.voice_setting.vol - 1.0).abs() < f32::EPSILON);
        assert_eq!(req.voice_setting.pitch, 0);
        assert_eq!(req.audio_setting.sample_rate, 32000);
        assert_eq!(req.audio_setting.bitrate, 128000);
        assert_eq!(req.audio_setting.format, "mp3");
        assert_eq!(req.audio_setting.channel, 1);
    }

    #[test]
    fn default_voice_setting() {
        let vs = VoiceSetting::default();
        assert!(vs.voice_id.is_empty());
        assert!((vs.speed - 1.0).abs() < f32::EPSILON);
        assert!((vs.vol - 1.0).abs() < f32::EPSILON);
        assert_eq!(vs.pitch, 0);
    }

    #[test]
    fn default_audio_setting() {
        let a = AudioSetting::default();
        assert_eq!(a.sample_rate, 32000);
        assert_eq!(a.bitrate, 128000);
        assert_eq!(a.format, "mp3");
        assert_eq!(a.channel, 1);
    }
}
