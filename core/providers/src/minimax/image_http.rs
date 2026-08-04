//! MiniMax 图像生成 HTTP（`POST /v1/image_generation`）。

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use reqwest::Client;
use serde_json::{json, Value};

use super::defaults::DEFAULT_API_BASE;
use crate::types::media::GeneratedImage;
use crate::types::ProviderConfig;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// 主体参考（角色一致性）。
#[derive(Debug, Clone)]
pub struct SubjectRef {
    /// 参考类型，如 `"character"`。
    pub ref_type: String,
    /// 图片文件 URL 或 base64。
    pub image_file: String,
}

/// 画风设置（仅 `image-01-live` 生效）。
#[derive(Debug, Clone)]
pub struct ImageStyle {
    /// 风格类型：`漫画` / `元气` / `中世纪` / `水彩`。
    pub style_type: String,
    /// 风格权重 (0, 1]，默认 0.8。
    pub style_weight: f32,
}

/// MiniMax 图像生成请求参数。
#[derive(Debug, Clone)]
pub struct MiniMaxImageRequest {
    /// 模型名称：`image-01` / `image-01-live`。
    pub model: String,
    /// 提示词。
    pub prompt: String,
    /// 宽高比，如 `"1:1"`、`"16:9"`。
    pub aspect_ratio: String,
    /// 自定义宽度（像素，[512, 2048]，8 的倍数，仅 image-01）。
    pub width: Option<u32>,
    /// 自定义高度（像素，[512, 2048]，8 的倍数，仅 image-01）。
    pub height: Option<u32>,
    /// 响应格式：`"url"` 或 `"base64"`。
    pub response_format: String,
    /// 生成数量 [1, 9]。
    pub n: u32,
    /// 是否启用提示词优化。
    pub prompt_optimizer: bool,
    /// 主体参考列表（角色一致性）。
    pub subject_reference: Option<Vec<SubjectRef>>,
    /// 画风设置（仅 image-01-live）。
    pub style: Option<ImageStyle>,
    /// 随机种子。
    pub seed: Option<i64>,
    /// 是否添加 AIGC 水印。
    pub aigc_watermark: bool,
}

impl Default for MiniMaxImageRequest {
    fn default() -> Self {
        Self {
            model: "image-01".to_string(),
            prompt: String::new(),
            aspect_ratio: "1:1".to_string(),
            width: None,
            height: None,
            response_format: "url".to_string(),
            n: 1,
            prompt_optimizer: false,
            subject_reference: None,
            style: None,
            seed: None,
            aigc_watermark: false,
        }
    }
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

/// 构造请求体 JSON（提取为独立函数便于测试）。
fn build_image_body(req: &MiniMaxImageRequest) -> Value {
    let mut body = json!({
        "model": req.model,
        "prompt": req.prompt,
        "aspect_ratio": req.aspect_ratio,
        "response_format": req.response_format,
        "n": req.n,
        "prompt_optimizer": req.prompt_optimizer,
    });

    if let Some(w) = req.width {
        body["width"] = json!(w);
    }
    if let Some(h) = req.height {
        body["height"] = json!(h);
    }
    if let Some(refs) = &req.subject_reference {
        let arr: Vec<Value> = refs
            .iter()
            .map(|r| {
                json!({
                    "type": r.ref_type,
                    "image_file": r.image_file,
                })
            })
            .collect();
        body["subject_reference"] = Value::Array(arr);
    }
    if let Some(ref style) = req.style {
        body["style"] = json!({
            "style_type": style.style_type,
            "style_weight": style.style_weight,
        });
    }
    if let Some(seed) = req.seed {
        body["seed"] = json!(seed);
    }
    if req.aigc_watermark {
        body["aigc_watermark"] = json!(true);
    }

    body
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// MiniMax 图像生成。
///
/// `POST {base}/image_generation`，返回生成的图片列表。
pub async fn minimax_generate_image(
    client: &Client,
    config: &ProviderConfig,
    req: &MiniMaxImageRequest,
) -> Result<Vec<GeneratedImage>> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("MiniMax API Key 为空");
    }

    let base = minimax_base(config);
    let url = format!("{base}/image_generation");
    let body = build_image_body(req);

    let response = client
        .post(&url)
        .bearer_auth(config.api_key.trim())
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 MiniMax 图像生成 API 失败: {url}"))?;

    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析 MiniMax 图像生成响应 JSON 失败")?;

    if !status.is_success() {
        let msg = v
            .pointer("/base_resp/status_msg")
            .and_then(|m| m.as_str())
            .unwrap_or("图像生成失败");
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
        anyhow::bail!("MiniMax 图像生成业务错误 ({biz_code}): {msg}");
    }

    let mut images = Vec::new();

    if req.response_format == "base64" {
        // base64 模式：data.image_base64[]
        let arr = v
            .pointer("/data/image_base64")
            .and_then(|a| a.as_array())
            .ok_or_else(|| anyhow!("MiniMax 响应缺少 data.image_base64"))?;
        for item in arr {
            let b64 = item
                .as_str()
                .ok_or_else(|| anyhow!("image_base64 元素非字符串"))?;
            let data = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .context("解码 MiniMax 图片 base64 失败")?;
            images.push(GeneratedImage {
                data,
                mime_type: "image/png".to_string(),
            });
        }
    } else {
        // url 模式：data.image_urls[]
        let arr = v
            .pointer("/data/image_urls")
            .and_then(|a| a.as_array())
            .ok_or_else(|| anyhow!("MiniMax 响应缺少 data.image_urls"))?;
        for (i, item) in arr.iter().enumerate() {
            let img_url = item
                .as_str()
                .ok_or_else(|| anyhow!("image_urls[{i}] 非字符串"))?;
            match client.get(img_url).send().await {
                Ok(resp) => match resp.bytes().await {
                    Ok(bytes) => {
                        images.push(GeneratedImage {
                            data: bytes.to_vec(),
                            mime_type: "image/png".to_string(),
                        });
                    }
                    Err(e) => {
                        eprintln!("下载 MiniMax 图片[{i}] 字节失败: {e}");
                    }
                },
                Err(e) => {
                    eprintln!("下载 MiniMax 图片[{i}] URL 失败: {e}");
                }
            }
        }
    }

    if images.is_empty() {
        anyhow::bail!("MiniMax 未返回图片数据");
    }
    Ok(images)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_request_values() {
        let req = MiniMaxImageRequest::default();
        assert_eq!(req.model, "image-01");
        assert_eq!(req.prompt, "");
        assert_eq!(req.aspect_ratio, "1:1");
        assert_eq!(req.response_format, "url");
        assert_eq!(req.n, 1);
        assert!(!req.prompt_optimizer);
        assert!(req.subject_reference.is_none());
    }

    #[test]
    fn build_body_without_subject_reference() {
        let req = MiniMaxImageRequest {
            prompt: "a cat".to_string(),
            ..Default::default()
        };
        let body = build_image_body(&req);
        assert_eq!(body["model"], "image-01");
        assert_eq!(body["prompt"], "a cat");
        assert_eq!(body["aspect_ratio"], "1:1");
        assert_eq!(body["response_format"], "url");
        assert_eq!(body["n"], 1);
        assert_eq!(body["prompt_optimizer"], false);
        assert!(body.get("subject_reference").is_none());
    }

    #[test]
    fn build_body_with_subject_reference() {
        let req = MiniMaxImageRequest {
            prompt: "a portrait".to_string(),
            subject_reference: Some(vec![SubjectRef {
                ref_type: "character".to_string(),
                image_file: "https://example.com/ref.png".to_string(),
            }]),
            ..Default::default()
        };
        let body = build_image_body(&req);
        let refs = body["subject_reference"].as_array().unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0]["type"], "character");
        assert_eq!(refs[0]["image_file"], "https://example.com/ref.png");
    }
}
