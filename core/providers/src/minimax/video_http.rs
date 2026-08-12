//! MiniMax 视频生成 HTTP（异步任务模式）。
//!
//! 同时支持旧版 v1 API（Hailuo-2.3 等）和 H3 v2 API（MiniMax-H3）。
//! 流程：创建任务 → 轮询状态 → 下载视频。

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde_json::{json, Value};

use super::defaults::DEFAULT_API_BASE;
use crate::types::request::ProviderConfig;

// ---------------------------------------------------------------------------
// H3 模型检测
// ---------------------------------------------------------------------------

/// 判断是否为 H3 系列模型（使用 v2 API）。
pub fn is_h3_model(model: &str) -> bool {
    let m = model.trim();
    m.eq_ignore_ascii_case("MiniMax-H3") || m.to_ascii_lowercase().starts_with("minimax-h3-")
}

// ---------------------------------------------------------------------------
// 类型
// ---------------------------------------------------------------------------

/// 视频主体参考（旧版 v1 API）。
#[derive(Debug, Clone)]
pub struct VideoSubjectRef {
    /// 参考类型，如 `"character"`。
    pub ref_type: String,
    /// 参考图片 URL 列表。
    pub images: Vec<String>,
}

/// 视频生成请求参数。
///
/// 同时承载 v1（扁平字段）和 v2/H3（content 数组）所需的所有数据。
/// 构建请求体时按模型自动选择格式。
#[derive(Debug, Clone)]
pub struct MiniMaxVideoRequest {
    /// 模型名称。
    pub model: String,
    /// 视频描述。
    pub prompt: String,

    // ── 图片帧 ──
    /// 首帧图片（I2V）。data URI 或 URL。
    pub first_frame_image: Option<String>,
    /// 末帧图片。data URI 或 URL。
    pub last_frame_image: Option<String>,

    // ── H3 多模态参考（v2 r2va 模式）──
    /// 参考图片 URL（最多 9 张）。
    pub reference_images: Vec<String>,
    /// 参考视频 URL（最多 3 个，每个 2-15s，总时长 ≤15s）。
    pub reference_videos: Vec<String>,
    /// 参考音频 URL（最多 3 个，每个 2-15s，总时长 ≤15s）。
    pub reference_audios: Vec<String>,

    // ── 旧版主体参考（v1 S2V）──
    pub subject_reference: Option<Vec<VideoSubjectRef>>,

    // ── 生成参数 ──
    /// 视频时长（秒）：H3 支持 4-15，旧版 6 或 10。
    pub duration: u32,
    /// 分辨率：H3 支持 `"768P"` / `"2K"`，旧版 `"720P"` / `"768P"` / `"1080P"`。
    pub resolution: String,
    /// 画面比例（H3 文生视频必填）：`"16:9"` / `"9:16"` / `"1:1"` / `"4:3"` 等。
    /// 图生视频模式设为 `"adaptive"` 或 None（自动适配图片比例）。
    pub ratio: Option<String>,
    /// 是否启用提示词优化。
    pub prompt_optimizer: bool,
    /// 是否缩短 prompt_optimizer 优化耗时。
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
            reference_images: Vec::new(),
            reference_videos: Vec::new(),
            reference_audios: Vec::new(),
            subject_reference: None,
            duration: 6,
            resolution: "768P".to_string(),
            ratio: None,
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
    Cancelled,
}

impl VideoTaskStatus {
    fn from_str(s: &str) -> Self {
        match s {
            "Preparing" | "preparing" => Self::Preparing,
            "Queueing" | "queueing" => Self::Queueing,
            "Processing" | "processing" => Self::Processing,
            "Success" | "succeeded" => Self::Success,
            "cancelled" => Self::Cancelled,
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
    /// v1 API 返回的 file_id。
    pub file_id: Option<String>,
    /// v2/H3 API 返回的直接下载 URL。
    pub download_url: Option<String>,
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

/// H3 Context-IR 提示词增强结果。
#[derive(Debug, Clone)]
pub struct ContextIrResult {
    pub task_id: String,
    pub enhanced_prompt: String,
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

/// 根据模型返回视频 API base（v1 或 v2）。
fn video_api_base(config: &ProviderConfig, model: &str) -> String {
    if is_h3_model(model) {
        let raw = config
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        match raw {
            Some(b) if b.contains("/v1") => {
                b.replace("/v1", "/v2").trim_end_matches('/').to_string()
            }
            Some(b) => b.trim_end_matches('/').to_string(),
            None => "https://api.minimaxi.com/v2".to_string(),
        }
    } else {
        minimax_base(config)
    }
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
// H3 content 数组构建
// ---------------------------------------------------------------------------

/// 将请求参数转换为 H3 v2 API 的 `content` 数组。
fn build_h3_content(req: &MiniMaxVideoRequest) -> Vec<Value> {
    let mut content = vec![];

    // 文本 prompt（必需）
    if !req.prompt.trim().is_empty() {
        content.push(json!({
            "type": "text",
            "text": req.prompt,
        }));
    }

    // 首帧图片
    if let Some(ref img) = req.first_frame_image {
        content.push(json!({
            "type": "image_url",
            "image_url": { "url": img },
            "role": "first_frame",
        }));
    }

    // 末帧图片
    if let Some(ref img) = req.last_frame_image {
        content.push(json!({
            "type": "image_url",
            "image_url": { "url": img },
            "role": "last_frame",
        }));
    }

    // 参考图片
    for img in &req.reference_images {
        content.push(json!({
            "type": "image_url",
            "image_url": { "url": img },
            "role": "reference_image",
        }));
    }

    // 参考视频
    for vid in &req.reference_videos {
        content.push(json!({
            "type": "video_url",
            "video_url": { "url": vid },
            "role": "reference_video",
        }));
    }

    // 参考音频
    for aud in &req.reference_audios {
        content.push(json!({
            "type": "audio_url",
            "audio_url": { "url": aud },
            "role": "reference_audio",
        }));
    }

    content
}

// ---------------------------------------------------------------------------
// 公开 API
// ---------------------------------------------------------------------------

/// 创建视频生成任务，返回 task_id。
///
/// 自动根据模型名选择 v1 或 v2 API 格式。
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

    let model = if req.model.trim().is_empty() {
        super::defaults::DEFAULT_VIDEO_MODEL
    } else {
        req.model.trim()
    };

    let base = video_api_base(config, model);
    let url = format!("{base}/video_generation");

    let body = if is_h3_model(model) {
        build_h3_request_body(req, model)
    } else {
        build_v1_request_body(req, model)
    };

    let response = client
        .post(&url)
        .bearer_auth(config.api_key.trim())
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 MiniMax 视频生成 API 失败: {url}"))?;

    let status = response.status();
    let v: Value = response.json().await.context("解析视频生成响应失败")?;

    if !status.is_success() {
        let msg = v
            .pointer("/base_resp/status_msg")
            .and_then(|m| m.as_str())
            .or_else(|| v.get("error").and_then(|e| e.as_str()))
            .unwrap_or("视频生成请求失败");
        anyhow::bail!("MiniMax 视频生成 HTTP {status}: {msg}");
    }
    if !is_h3_model(model) {
        check_base_resp(&v, "视频生成")?;
    }

    v.get("task_id")
        .and_then(|t| {
            t.as_str()
                .map(str::to_string)
                .or_else(|| Some(t.to_string()))
        })
        .ok_or_else(|| anyhow!("视频生成响应缺少 task_id"))
}

/// 查询视频生成任务状态。
///
/// 自动根据模型名选择 v1 或 v2 查询接口与响应格式。
pub async fn minimax_query_video(
    client: &Client,
    config: &ProviderConfig,
    task_id: &str,
    model: &str,
) -> Result<VideoTaskResult> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("MiniMax API Key 为空");
    }

    let base = video_api_base(config, model);
    let url = if is_h3_model(model) {
        format!("{base}/query/video_generation/{task_id}")
    } else {
        format!("{base}/query/video_generation?task_id={task_id}")
    };

    let response = client
        .get(&url)
        .bearer_auth(config.api_key.trim())
        .send()
        .await
        .with_context(|| format!("连接 MiniMax 视频查询 API 失败: {url}"))?;

    let status = response.status();
    let v: Value = response.json().await.context("解析视频查询响应失败")?;

    if !status.is_success() {
        let msg = v
            .pointer("/base_resp/status_msg")
            .and_then(|m| m.as_str())
            .or_else(|| v.get("error").and_then(|e| e.as_str()))
            .unwrap_or("查询失败");
        anyhow::bail!("MiniMax 视频查询 HTTP {status}: {msg}");
    }

    if is_h3_model(model) {
        parse_h3_query_result(&v, task_id)
    } else {
        check_base_resp(&v, "视频查询")?;
        parse_v1_query_result(&v, task_id)
    }
}

/// 兼容旧签名（不传 model 时按旧版 v1 处理）。
pub async fn minimax_query_video_v1(
    client: &Client,
    config: &ProviderConfig,
    task_id: &str,
) -> Result<VideoTaskResult> {
    minimax_query_video(client, config, task_id, "").await
}

/// 下载已完成的视频（v1：通过 file_id；v2：直接 URL）。
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
    let v: Value = response.json().await.context("解析文件查询响应失败")?;

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

    download_video_from_url(client, download_url).await
}

/// 通过直接 URL 下载视频（H3 v2 API）。
pub async fn minimax_download_video_url(client: &Client, url: &str) -> Result<MiniMaxVideoResult> {
    download_video_from_url(client, url).await
}

/// H3 Context-IR 提示词增强：深度理解多模态上下文，返回增强后的 prompt。
pub async fn minimax_enhance_prompt(
    client: &Client,
    config: &ProviderConfig,
    req: &MiniMaxVideoRequest,
) -> Result<ContextIrResult> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("MiniMax API Key 为空");
    }

    let model = if req.model.trim().is_empty() {
        super::defaults::DEFAULT_VIDEO_MODEL
    } else {
        req.model.trim()
    };

    if !is_h3_model(model) {
        anyhow::bail!("Context-IR 提示词增强仅 MiniMax-H3 系列支持");
    }

    let base = video_api_base(config, model);
    let url = format!("{base}/video_generation");

    let content = build_h3_content(req);
    let mut body = json!({
        "model": model,
        "content": content,
        "task_type": "h3_context_ir",
    });
    if let Some(ref ratio) = req.ratio {
        body["ratio"] = json!(ratio);
    }

    let response = client
        .post(&url)
        .bearer_auth(config.api_key.trim())
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| "连接 MiniMax Context-IR API 失败")?;

    let status = response.status();
    let v: Value = response.json().await.context("解析 Context-IR 响应失败")?;

    if !status.is_success() {
        let msg = v
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("Context-IR 请求失败");
        anyhow::bail!("MiniMax Context-IR HTTP {status}: {msg}");
    }

    let task_id = v
        .get("task_id")
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow!("Context-IR 响应缺少 task_id"))?
        .to_string();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5 * 60);
    loop {
        if std::time::Instant::now() > deadline {
            anyhow::bail!("Context-IR 超时（task_id={task_id}）");
        }
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        let qurl = format!("{base}/query/video_generation/{task_id}");
        let qresp = client
            .get(&qurl)
            .bearer_auth(config.api_key.trim())
            .send()
            .await?;
        let qv: Value = qresp.json().await?;

        let task_status = qv
            .pointer("/task/status")
            .or_else(|| qv.get("status"))
            .and_then(|s| s.as_str())
            .unwrap_or("processing");

        match task_status {
            "succeeded" | "Success" => {
                let prompt = qv
                    .pointer("/task/content/prompt")
                    .or_else(|| qv.pointer("/content/prompt"))
                    .and_then(|p| p.as_str())
                    .unwrap_or("")
                    .to_string();
                return Ok(ContextIrResult {
                    task_id,
                    enhanced_prompt: prompt,
                });
            }
            "failed" | "Fail" | "cancelled" => {
                let err = qv
                    .pointer("/task/error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("未知错误");
                anyhow::bail!("Context-IR 失败: {err}");
            }
            _ => continue,
        }
    }
}

/// H3 视频重生成（768P → 2K 升级）。
///
/// `base_video_url` 为原始 768P 视频 URL，`req` 需包含原始生成参数。
pub async fn minimax_regenerate_video(
    client: &Client,
    config: &ProviderConfig,
    req: &MiniMaxVideoRequest,
    base_video_url: &str,
) -> Result<String> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("MiniMax API Key 为空");
    }

    let model = if req.model.trim().is_empty() {
        super::defaults::DEFAULT_VIDEO_MODEL
    } else {
        req.model.trim()
    };

    if !is_h3_model(model) {
        anyhow::bail!("视频重生成（2K 升级）仅 MiniMax-H3 系列支持");
    }

    let base = video_api_base(config, model);
    let url = format!("{base}/video_generation");

    let mut content = build_h3_content(req);
    content.push(json!({
        "type": "video_url",
        "video_url": { "url": base_video_url },
        "role": "base_video",
    }));

    let body = json!({
        "model": model,
        "content": content,
        "duration": req.duration,
        "resolution": "2K",
        "task_type": "regeneration",
    });

    let response = client
        .post(&url)
        .bearer_auth(config.api_key.trim())
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| "连接 MiniMax 视频重生成 API 失败")?;

    let status = response.status();
    let v: Value = response.json().await.context("解析重生成响应失败")?;

    if !status.is_success() {
        let msg = v
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("重生成请求失败");
        anyhow::bail!("MiniMax 视频重生成 HTTP {status}: {msg}");
    }

    v.get("task_id")
        .and_then(|t| {
            t.as_str()
                .map(str::to_string)
                .or_else(|| Some(t.to_string()))
        })
        .ok_or_else(|| anyhow!("重生成响应缺少 task_id"))
}

// ---------------------------------------------------------------------------
// 请求体构建
// ---------------------------------------------------------------------------

fn build_h3_request_body(req: &MiniMaxVideoRequest, model: &str) -> Value {
    let content = build_h3_content(req);
    let has_image_input = req.first_frame_image.is_some();

    let mut body = json!({
        "model": model,
        "content": content,
        "duration": req.duration,
        "resolution": req.resolution,
    });

    // 文生视频必须指定 ratio 且不能为 adaptive；图生视频自动 adaptive
    if has_image_input {
        body["ratio"] = json!("adaptive");
    } else if let Some(ref ratio) = req.ratio {
        body["ratio"] = json!(ratio);
    } else {
        body["ratio"] = json!("16:9");
    }

    body
}

fn build_v1_request_body(req: &MiniMaxVideoRequest, model: &str) -> Value {
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

    body
}

// ---------------------------------------------------------------------------
// 查询结果解析
// ---------------------------------------------------------------------------

fn parse_h3_query_result(v: &Value, task_id: &str) -> Result<VideoTaskResult> {
    let task = v.get("task").unwrap_or(v);
    let task_status = task
        .get("status")
        .and_then(|s| s.as_str())
        .unwrap_or("processing");

    let download_url = task
        .pointer("/content/url")
        .and_then(|u| u.as_str())
        .map(str::to_string);

    Ok(VideoTaskResult {
        task_id: v
            .get("task_id")
            .and_then(|t| t.as_str())
            .unwrap_or(task_id)
            .to_string(),
        status: VideoTaskStatus::from_str(task_status),
        file_id: None,
        download_url,
        video_width: None,
        video_height: None,
    })
}

fn parse_v1_query_result(v: &Value, task_id: &str) -> Result<VideoTaskResult> {
    let task_status = v.get("status").and_then(|s| s.as_str()).unwrap_or("Fail");

    Ok(VideoTaskResult {
        task_id: v
            .get("task_id")
            .and_then(|t| t.as_str())
            .unwrap_or(task_id)
            .to_string(),
        status: VideoTaskStatus::from_str(task_status),
        file_id: v.get("file_id").and_then(|f| {
            f.as_str()
                .map(str::to_string)
                .or_else(|| Some(f.to_string()))
        }),
        download_url: None,
        video_width: v
            .get("video_width")
            .and_then(|w| w.as_u64())
            .map(|w| w as u32),
        video_height: v
            .get("video_height")
            .and_then(|h| h.as_u64())
            .map(|h| h as u32),
    })
}

// ---------------------------------------------------------------------------
// 下载
// ---------------------------------------------------------------------------

async fn download_video_from_url(client: &Client, url: &str) -> Result<MiniMaxVideoResult> {
    let dl_resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("下载 MiniMax 视频失败: {url}"))?;

    if !dl_resp.status().is_success() {
        anyhow::bail!("下载视频 HTTP {}: {url}", dl_resp.status());
    }

    let data = dl_resp.bytes().await.context("读取视频字节失败")?.to_vec();

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
        assert_eq!(req.model, super::super::defaults::DEFAULT_VIDEO_MODEL);
        assert_eq!(req.duration, 6);
        assert_eq!(req.resolution, "768P");
        assert!(req.prompt_optimizer);
        assert!(req.first_frame_image.is_none());
        assert!(req.last_frame_image.is_none());
        assert!(req.subject_reference.is_none());
        assert!(req.reference_images.is_empty());
        assert!(req.reference_videos.is_empty());
        assert!(req.reference_audios.is_empty());
        assert!(req.ratio.is_none());
    }

    #[test]
    fn status_parsing() {
        assert_eq!(
            VideoTaskStatus::from_str("Preparing"),
            VideoTaskStatus::Preparing
        );
        assert_eq!(
            VideoTaskStatus::from_str("Queueing"),
            VideoTaskStatus::Queueing
        );
        assert_eq!(
            VideoTaskStatus::from_str("Processing"),
            VideoTaskStatus::Processing
        );
        assert_eq!(
            VideoTaskStatus::from_str("Success"),
            VideoTaskStatus::Success
        );
        assert_eq!(
            VideoTaskStatus::from_str("succeeded"),
            VideoTaskStatus::Success
        );
        assert_eq!(VideoTaskStatus::from_str("Fail"), VideoTaskStatus::Fail);
        assert_eq!(VideoTaskStatus::from_str("failed"), VideoTaskStatus::Fail);
        assert_eq!(
            VideoTaskStatus::from_str("cancelled"),
            VideoTaskStatus::Cancelled
        );
        assert_eq!(VideoTaskStatus::from_str("unknown"), VideoTaskStatus::Fail);
    }

    #[test]
    fn status_is_pending() {
        assert!(VideoTaskStatus::Preparing.is_pending());
        assert!(VideoTaskStatus::Queueing.is_pending());
        assert!(VideoTaskStatus::Processing.is_pending());
        assert!(!VideoTaskStatus::Success.is_pending());
        assert!(!VideoTaskStatus::Fail.is_pending());
        assert!(!VideoTaskStatus::Cancelled.is_pending());
    }

    #[test]
    fn h3_model_detection() {
        assert!(is_h3_model("MiniMax-H3"));
        assert!(is_h3_model("minimax-h3"));
        assert!(is_h3_model("MiniMax-H3-preview"));
        assert!(!is_h3_model("MiniMax-Hailuo-2.3"));
        assert!(!is_h3_model("video-01"));
        assert!(!is_h3_model(""));
    }

    #[test]
    fn video_api_base_routing() {
        let v1_cfg = ProviderConfig {
            base_url: None,
            ..ProviderConfig::default()
        };
        assert_eq!(
            video_api_base(&v1_cfg, "MiniMax-Hailuo-2.3"),
            DEFAULT_API_BASE
        );
        assert_eq!(
            video_api_base(&v1_cfg, "MiniMax-H3"),
            "https://api.minimaxi.com/v2"
        );

        let custom_v1 = ProviderConfig {
            base_url: Some("https://api.minimaxi.com/v1".to_string()),
            ..ProviderConfig::default()
        };
        assert_eq!(
            video_api_base(&custom_v1, "MiniMax-H3"),
            "https://api.minimaxi.com/v2"
        );
    }

    #[test]
    fn h3_content_array() {
        let req = MiniMaxVideoRequest {
            prompt: "test prompt".to_string(),
            first_frame_image: Some("https://img.jpg".to_string()),
            reference_images: vec!["https://ref1.jpg".to_string()],
            reference_videos: vec!["https://ref.mp4".to_string()],
            ..MiniMaxVideoRequest::default()
        };
        let content = build_h3_content(&req);
        assert_eq!(content.len(), 4); // text + first_frame + ref_image + ref_video
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["role"], "first_frame");
        assert_eq!(content[2]["role"], "reference_image");
        assert_eq!(content[3]["role"], "reference_video");
    }

    #[test]
    fn h3_request_body_text_to_video() {
        let req = MiniMaxVideoRequest {
            model: "MiniMax-H3".to_string(),
            prompt: "a cat".to_string(),
            ratio: Some("16:9".to_string()),
            duration: 10,
            ..MiniMaxVideoRequest::default()
        };
        let body = build_h3_request_body(&req, "MiniMax-H3");
        assert_eq!(body["model"], "MiniMax-H3");
        assert_eq!(body["ratio"], "16:9");
        assert_eq!(body["duration"], 10);
        assert!(body.get("content").unwrap().is_array());
    }

    #[test]
    fn h3_request_body_image_to_video() {
        let req = MiniMaxVideoRequest {
            model: "MiniMax-H3".to_string(),
            prompt: "animate".to_string(),
            first_frame_image: Some("https://img.jpg".to_string()),
            ..MiniMaxVideoRequest::default()
        };
        let body = build_h3_request_body(&req, "MiniMax-H3");
        assert_eq!(body["ratio"], "adaptive");
    }

    #[test]
    fn v1_request_body() {
        let req = MiniMaxVideoRequest {
            model: "MiniMax-Hailuo-2.3".to_string(),
            prompt: "a dog".to_string(),
            first_frame_image: Some("data:image/jpeg;base64,abc".to_string()),
            subject_reference: Some(vec![VideoSubjectRef {
                ref_type: "character".to_string(),
                images: vec!["data:image/jpeg;base64,xyz".to_string()],
            }]),
            ..MiniMaxVideoRequest::default()
        };
        let body = build_v1_request_body(&req, "MiniMax-Hailuo-2.3");
        assert_eq!(body["prompt"], "a dog");
        assert!(body.get("first_frame_image").is_some());
        assert!(body.get("subject_reference").unwrap().is_array());
        assert!(body.get("content").is_none());
    }

    #[test]
    fn parse_h3_query_success() {
        let v = json!({
            "task_id": "abc123",
            "task": {
                "status": "succeeded",
                "content": {
                    "url": "https://video.mp4"
                }
            }
        });
        let result = parse_h3_query_result(&v, "abc123").unwrap();
        assert_eq!(result.status, VideoTaskStatus::Success);
        assert_eq!(result.download_url, Some("https://video.mp4".to_string()));
        assert!(result.file_id.is_none());
    }

    #[test]
    fn parse_v1_query_success() {
        let v = json!({
            "task_id": "xyz789",
            "status": "Success",
            "file_id": "file_001",
            "video_width": 1280,
            "video_height": 720,
            "base_resp": { "status_code": 0, "status_msg": "success" }
        });
        let result = parse_v1_query_result(&v, "xyz789").unwrap();
        assert_eq!(result.status, VideoTaskStatus::Success);
        assert_eq!(result.file_id, Some("file_001".to_string()));
        assert_eq!(result.video_width, Some(1280));
        assert!(result.download_url.is_none());
    }
}
