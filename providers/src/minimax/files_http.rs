//! MiniMax 文件管理（上传 / 查询 / 下载 URL）。

use std::fmt;

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde_json::Value;

use super::defaults::DEFAULT_API_BASE;
use crate::trait_::ProviderConfig;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// 文件上传用途。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileUploadPurpose {
    /// 声音克隆。
    VoiceClone,
    /// 提示音频。
    PromptAudio,
    /// 视频理解。
    VideoUnderstanding,
}

impl fmt::Display for FileUploadPurpose {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::VoiceClone => "voice_clone",
            Self::PromptAudio => "prompt_audio",
            Self::VideoUnderstanding => "video_understanding",
        };
        f.write_str(s)
    }
}

/// MiniMax 文件元信息。
#[derive(Debug, Clone)]
pub struct MiniMaxFileInfo {
    /// 文件 ID。
    pub file_id: u64,
    /// 文件名。
    pub filename: String,
    /// 下载 URL（查询时返回，上传时通常为 None）。
    pub download_url: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// 解析 MiniMax API 基址。
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

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// 上传文件到 MiniMax。
///
/// `POST {base}/files/upload`（multipart/form-data）。
pub async fn minimax_upload_file(
    client: &Client,
    config: &ProviderConfig,
    data: Vec<u8>,
    filename: &str,
    purpose: FileUploadPurpose,
) -> Result<MiniMaxFileInfo> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("MiniMax API Key 为空");
    }

    let base = minimax_base(config);
    let url = format!("{base}/files/upload");

    let file_part = reqwest::multipart::Part::bytes(data)
        .file_name(filename.to_string())
        .mime_str("application/octet-stream")
        .context("构造 multipart 文件部分失败")?;

    let form = reqwest::multipart::Form::new()
        .text("purpose", purpose.to_string())
        .part("file", file_part);

    let response = client
        .post(&url)
        .bearer_auth(config.api_key.trim())
        .multipart(form)
        .send()
        .await
        .with_context(|| format!("连接 MiniMax 文件上传 API 失败: {url}"))?;

    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析 MiniMax 文件上传响应 JSON 失败")?;

    if !status.is_success() {
        let msg = v
            .pointer("/base_resp/status_msg")
            .and_then(|m| m.as_str())
            .unwrap_or("文件上传失败");
        anyhow::bail!("MiniMax HTTP {status}: {msg}");
    }

    // base_resp.status_code != 0 表示业务错误
    let biz_code = v
        .pointer("/base_resp/status_code")
        .and_then(|c| c.as_i64())
        .unwrap_or(0);
    if biz_code != 0 {
        let msg = v
            .pointer("/base_resp/status_msg")
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误");
        anyhow::bail!("MiniMax 文件上传业务错误 ({biz_code}): {msg}");
    }

    let file_id = v
        .pointer("/file/file_id")
        .and_then(|id| id.as_u64())
        .ok_or_else(|| anyhow!("MiniMax 上传响应缺少 file.file_id"))?;
    let fname = v
        .pointer("/file/filename")
        .and_then(|n| n.as_str())
        .unwrap_or(filename)
        .to_string();

    Ok(MiniMaxFileInfo {
        file_id,
        filename: fname,
        download_url: None,
    })
}

/// 查询已上传文件的元信息。
///
/// `GET {base}/files/retrieve?file_id={file_id}`。
pub async fn minimax_retrieve_file(
    client: &Client,
    config: &ProviderConfig,
    file_id: u64,
) -> Result<MiniMaxFileInfo> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("MiniMax API Key 为空");
    }

    let base = minimax_base(config);
    let url = format!("{base}/files/retrieve?file_id={file_id}");

    let response = client
        .get(&url)
        .bearer_auth(config.api_key.trim())
        .send()
        .await
        .with_context(|| format!("连接 MiniMax 文件查询 API 失败: {url}"))?;

    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析 MiniMax 文件查询响应 JSON 失败")?;

    if !status.is_success() {
        let msg = v
            .pointer("/base_resp/status_msg")
            .and_then(|m| m.as_str())
            .unwrap_or("文件查询失败");
        anyhow::bail!("MiniMax HTTP {status}: {msg}");
    }

    let biz_code = v
        .pointer("/base_resp/status_code")
        .and_then(|c| c.as_i64())
        .unwrap_or(0);
    if biz_code != 0 {
        let msg = v
            .pointer("/base_resp/status_msg")
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误");
        anyhow::bail!("MiniMax 文件查询业务错误 ({biz_code}): {msg}");
    }

    let fid = v
        .pointer("/file/file_id")
        .and_then(|id| id.as_u64())
        .ok_or_else(|| anyhow!("MiniMax 查询响应缺少 file.file_id"))?;
    let filename = v
        .pointer("/file/filename")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let download_url = v
        .pointer("/file/download_url")
        .and_then(|u| u.as_str())
        .map(|s| s.to_string());

    Ok(MiniMaxFileInfo {
        file_id: fid,
        filename,
        download_url,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_upload_purpose_display() {
        assert_eq!(FileUploadPurpose::VoiceClone.to_string(), "voice_clone");
        assert_eq!(FileUploadPurpose::PromptAudio.to_string(), "prompt_audio");
        assert_eq!(
            FileUploadPurpose::VideoUnderstanding.to_string(),
            "video_understanding"
        );
    }
}
