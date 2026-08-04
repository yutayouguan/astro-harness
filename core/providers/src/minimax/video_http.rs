//! MiniMax 视频生成 HTTP（异步任务模式）。
//!
//! 流程：创建任务 → 轮询状态 → 下载视频。

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde_json::{json, Value};

use super::defaults::DEFAULT_API_BASE;
use crate::types::request::ProviderConfig;

// ---------------------------------------------------------------------------
// 类型
// ---------------------------------------------------------------------------

/// 视频主体参考。
#[derive(Debug, Clone)]
pub struct VideoSubjectRef {
    /// 参考类型，如 `"character"`。
    pub ref_type: String,
    /// 参考图片 URL 列表。
    pub images: Vec<String>,
}

/// 视频生成请求参数。
#[derive(Debug, Clone)]
pub struct MiniMaxVideoRequest {
    /// 模型名称。
    pub model: String,
    /// 视频描述。
    pub prompt: String,
    /// 首帧图片（I2V）。
    pub first_frame_image: Option<String>,
    /// 末帧图片。
    pub last_frame_image: Option<String>,
    /// 主体参考。
    pub subject_reference: Option<Vec<VideoSubjectRef>>,
    /// 视频时长（秒）：6 或 10。
    pub duration: u32,
    /// 分辨率：`"720P"` / `"768P"` / `"1080P"`。
    pub resolution: String,
    /// 是否启用提示词优化。
    pub prompt_optimizer: bool,
    /// 是否缩短 prompt_optimizer 优化耗时（Hailuo-2.3 / Hailuo-02）。
    pub fast_pretreatment: bool,
    /// 是否添加 AIGC 水印。
    pub aigc_watermark: bool,
}

impl Default for MiniMaxVideoRequest {
    fn default() -> Self {
        Self {
            model: super::defaults::DEFAULT_VIDEO_MODEL.to_string(),
            prompt: String::new(),
            first_frame_image: None,
            last_frame_image: None,
            subject_reference: None,
            duration: 6,
            resolution: "768P".to_string(),
            prompt_optimizer: true,
            fast_pretreatment: false,
            aigc_watermark: false,
        }
    }
}

/// 视频任务状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VideoTaskStatus {
    Preparing,
    Queueing,
    Processing,
    Success,
    Fail,
}

impl VideoTaskStatus {
    fn from_str(s: &str) -> Self {
        match s {
            "Preparing" => Self::Preparing,
            "Queueing" => Self::Queueing,
            "Processing" => Self::Processing,
            "Success" => Self::Success,
            _ => Self::Fail,
        }
    }

    /// 任务是否仍在进行中。
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Preparing | Self::Queueing | Self::Processing)
    }
}

/// 视频任务查询结果。
#[derive(Debug, Clone)]
pub struct VideoTaskResult {
    pub task_id: String,
    pub status: VideoTaskStatus,
    pub file_id: Option<String>,
    pub video_width: Option<u32>,
    pub video_height: Option<u32>,
}

/// 视频最终结果。
#[derive(Debug, Clone)]
pub struct MiniMaxVideoResult {
    pub data: Vec<u8>,
    pub mime_type: String,
    pub width: u32,
    pub height: u32,
}

// ---------------------------------------------------------------------------
// 内部工具
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

fn check_base_resp(v: &Value, ctx: &str) -> Result<()> {
    let code = v
        .pointer("/base_resp/status_code")
        .and_then(|c| c.as_i64())
        .unwrap_or(0);
    if code != 0 {
        let msg = v
            .pointer("/base_resp/status_msg")
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误");
        anyhow::bail!("MiniMax {ctx} 业务错误 ({code}): {msg}");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 公开 API
// ---------------------------------------------------------------------------

/// 创建视频生成任务，返回 task_id。
pub async fn minimax_create_video(
    client: &Client,
    config: &ProviderConfig,
    req: &MiniMaxVideoRequest,
) -> Result<String> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("MiniMax API Key 为空");
    }
    if req.prompt.trim().is_empty() && req.first_frame_image.is_none() {
        anyhow::bail!("视频生成需要 prompt 或 first_frame_image");
    }

    let base = minimax_base(config);
    let url = format!("{base}/video_generation");

    let model = if req.model.trim().is_empty() {
        super::defaults::DEFAULT_VIDEO_MODEL
    } else {
        req.model.trim()
    };

    let mut body = json!({
        "model": model,
        "prompt": req.prompt,
        "duration": req.duration,
        "resolution": req.resolution,
        "prompt_optimizer": req.prompt_optimizer,
        "fast_pretreatment": req.fast_pretreatment,
        "aigc_watermark": req.aigc_watermark,
    });

    if let Some(ref img) = req.first_frame_image {
        body["first_frame_image"] = json!(img);
    }
    if let Some(ref img) = req.last_frame_image {
        body["last_frame_image"] = json!(img);
    }
    if let Some(ref refs) = req.subject_reference {
        let arr: Vec<Value> = refs
            .iter()
            .map(|r| {
                json!({
                    "type": r.ref_type,
                    "image": r.images,
                })
            })
            .collect();
        body["subject_reference"] = Value::Array(arr);
    }

    let response = client
        .post(&url)
        .bearer_auth(config.api_key.trim())
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 MiniMax 视频生成 API 失败: {url}"))?;

    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析视频生成响应失败")?;

    if !status.is_success() {
        let msg = v
            .pointer("/base_resp/status_msg")
            .and_then(|m| m.as_str())
            .unwrap_or("视频生成请求失败");
        anyhow::bail!("MiniMax 视频生成 HTTP {status}: {msg}");
    }
    check_base_resp(&v, "视频生成")?;

    v.get("task_id")
        .and_then(|t| t.as_str().map(str::to_string).or_else(|| Some(t.to_string())))
        .ok_or_else(|| anyhow!("视频生成响应缺少 task_id"))
}

/// 查询视频生成任务状态。
pub async fn minimax_query_video(
    client: &Client,
    config: &ProviderConfig,
    task_id: &str,
) -> Result<VideoTaskResult> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("MiniMax API Key 为空");
    }

    let base = minimax_base(config);
    let url = format!("{base}/query/video_generation?task_id={task_id}");

    let response = client
        .get(&url)
        .bearer_auth(config.api_key.trim())
        .send()
        .await
        .with_context(|| format!("连接 MiniMax 视频查询 API 失败: {url}"))?;

    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析视频查询响应失败")?;

    if !status.is_success() {
        let msg = v
            .pointer("/base_resp/status_msg")
            .and_then(|m| m.as_str())
            .unwrap_or("查询失败");
        anyhow::bail!("MiniMax 视频查询 HTTP {status}: {msg}");
    }
    check_base_resp(&v, "视频查询")?;

    let task_status = v
        .get("status")
        .and_then(|s| s.as_str())
        .unwrap_or("Fail");

    Ok(VideoTaskResult {
        task_id: v
            .get("task_id")
            .and_then(|t| t.as_str())
            .unwrap_or(task_id)
            .to_string(),
        status: VideoTaskStatus::from_str(task_status),
        file_id: v
            .get("file_id")
            .and_then(|f| f.as_str().map(str::to_string).or_else(|| Some(f.to_string()))),
        video_width: v.get("video_width").and_then(|w| w.as_u64()).map(|w| w as u32),
        video_height: v.get("video_height").and_then(|h| h.as_u64()).map(|h| h as u32),
    })
}

/// 下载已完成的视频。
///
/// 先查询文件信息获取 download_url，再下载视频字节。
pub async fn minimax_download_video(
    client: &Client,
    config: &ProviderConfig,
    file_id: &str,
) -> Result<MiniMaxVideoResult> {
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
        .context("解析文件查询响应失败")?;

    if !status.is_success() {
        let msg = v
            .pointer("/base_resp/status_msg")
            .and_then(|m| m.as_str())
            .unwrap_or("文件查询失败");
        anyhow::bail!("MiniMax 文件查询 HTTP {status}: {msg}");
    }
    check_base_resp(&v, "文件查询")?;

    let download_url = v
        .pointer("/file/download_url")
        .and_then(|u| u.as_str())
        .ok_or_else(|| anyhow!("文件查询响应缺少 download_url"))?;

    let dl_resp = client
        .get(download_url)
        .send()
        .await
        .with_context(|| format!("下载 MiniMax 视频失败: {download_url}"))?;

    if !dl_resp.status().is_success() {
        anyhow::bail!("下载视频 HTTP {}: {download_url}", dl_resp.status());
    }

    let data = dl_resp
        .bytes()
        .await
        .context("读取视频字节失败")?
        .to_vec();

    if data.is_empty() {
        return Err(anyhow!("MiniMax 视频下载返回空数据"));
    }

    Ok(MiniMaxVideoResult {
        data,
        mime_type: "video/mp4".to_string(),
        width: 0,
        height: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_request_values() {
        let req = MiniMaxVideoRequest::default();
        assert_eq!(req.model, "MiniMax-Hailuo-2.3");
        assert_eq!(req.duration, 6);
        assert_eq!(req.resolution, "768P");
        assert!(req.prompt_optimizer);
        assert!(req.first_frame_image.is_none());
        assert!(req.last_frame_image.is_none());
        assert!(req.subject_reference.is_none());
    }

    #[test]
    fn status_parsing() {
        assert_eq!(VideoTaskStatus::from_str("Preparing"), VideoTaskStatus::Preparing);
        assert_eq!(VideoTaskStatus::from_str("Queueing"), VideoTaskStatus::Queueing);
        assert_eq!(VideoTaskStatus::from_str("Processing"), VideoTaskStatus::Processing);
        assert_eq!(VideoTaskStatus::from_str("Success"), VideoTaskStatus::Success);
        assert_eq!(VideoTaskStatus::from_str("Fail"), VideoTaskStatus::Fail);
        assert_eq!(VideoTaskStatus::from_str("unknown"), VideoTaskStatus::Fail);
    }

    #[test]
    fn status_is_pending() {
        assert!(VideoTaskStatus::Preparing.is_pending());
        assert!(VideoTaskStatus::Queueing.is_pending());
        assert!(VideoTaskStatus::Processing.is_pending());
        assert!(!VideoTaskStatus::Success.is_pending());
        assert!(!VideoTaskStatus::Fail.is_pending());
    }
}
