//! MiniMax 声音克隆 HTTP（`POST /v1/voice_clone`）。

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};

use super::defaults::DEFAULT_API_BASE;
use crate::trait_::ProviderConfig;

// ---------------------------------------------------------------------------
// 类型
// ---------------------------------------------------------------------------

/// 声音克隆请求参数。
#[derive(Debug, Clone, Default)]
pub struct VoiceCloneRequest {
    /// 克隆源音频文件 ID。
    pub file_id: u64,
    /// 自定义音色 ID。
    pub voice_id: String,
    /// 试听文本（可选）。
    pub text: Option<String>,
    /// 试听模型（有 text 时必填）。
    pub model: Option<String>,
    /// 是否降噪。
    pub need_noise_reduction: bool,
    /// 是否音量归一化。
    pub need_volume_normalization: bool,
    /// 语言增强。
    pub language_boost: Option<String>,
}


/// 声音克隆结果。
#[derive(Debug, Clone)]
pub struct VoiceCloneResult {
    /// 试听音频 URL（仅当请求包含 text 时返回）。
    pub demo_audio_url: Option<String>,
}

// ---------------------------------------------------------------------------
// 公开 API
// ---------------------------------------------------------------------------

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

/// 调用 MiniMax 声音克隆 API。
pub async fn minimax_voice_clone(
    client: &Client,
    config: &ProviderConfig,
    req: &VoiceCloneRequest,
) -> Result<VoiceCloneResult> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("MiniMax API Key 为空");
    }
    if req.voice_id.trim().is_empty() {
        anyhow::bail!("voice_id 为空");
    }
    if req.file_id == 0 {
        anyhow::bail!("file_id 无效");
    }

    let base = minimax_base(config);
    let url = format!("{base}/voice_clone");

    let mut body = json!({
        "file_id": req.file_id,
        "voice_id": req.voice_id,
        "need_noise_reduction": req.need_noise_reduction,
        "need_volume_normalization": req.need_volume_normalization,
    });

    if let Some(ref text) = req.text {
        body["text"] = json!(text);
        if let Some(ref model) = req.model {
            body["model"] = json!(model);
        }
    }
    if let Some(ref lang) = req.language_boost {
        body["language_boost"] = json!(lang);
    }

    let response = client
        .post(&url)
        .bearer_auth(config.api_key.trim())
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 MiniMax 声音克隆 API 失败: {url}"))?;

    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析声音克隆响应失败")?;

    if !status.is_success() {
        let msg = v
            .pointer("/base_resp/status_msg")
            .and_then(|m| m.as_str())
            .unwrap_or("声音克隆失败");
        anyhow::bail!("MiniMax 声音克隆 HTTP {status}: {msg}");
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
        anyhow::bail!("MiniMax 声音克隆业务错误 ({code}): {msg}");
    }

    let demo_audio_url = v
        .get("demo_audio")
        .and_then(|d| d.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    Ok(VoiceCloneResult { demo_audio_url })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_request_values() {
        let req = VoiceCloneRequest::default();
        assert_eq!(req.file_id, 0);
        assert!(req.voice_id.is_empty());
        assert!(req.text.is_none());
        assert!(req.model.is_none());
        assert!(!req.need_noise_reduction);
        assert!(!req.need_volume_normalization);
        assert!(req.language_boost.is_none());
    }

    #[test]
    fn body_with_preview_text() {
        let req = VoiceCloneRequest {
            file_id: 12345,
            voice_id: "my-voice".to_string(),
            text: Some("试听文本".to_string()),
            model: Some("speech-2.8-hd".to_string()),
            ..Default::default()
        };
        let mut body = json!({
            "file_id": req.file_id,
            "voice_id": req.voice_id,
        });
        if let Some(ref text) = req.text {
            body["text"] = json!(text);
            if let Some(ref model) = req.model {
                body["model"] = json!(model);
            }
        }
        assert_eq!(body["text"], "试听文本");
        assert_eq!(body["model"], "speech-2.8-hd");
    }

    #[test]
    fn body_without_preview_text() {
        let req = VoiceCloneRequest {
            file_id: 12345,
            voice_id: "my-voice".to_string(),
            ..Default::default()
        };
        let mut body = json!({
            "file_id": req.file_id,
            "voice_id": req.voice_id,
        });
        if let Some(ref text) = req.text {
            body["text"] = json!(text);
        }
        assert!(body.get("text").is_none());
        assert!(body.get("model").is_none());
    }
}
