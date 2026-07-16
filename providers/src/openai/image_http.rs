//! OpenAI 图片生成 HTTP

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use reqwest::Client;
use serde_json::{json, Value};

use super::defaults::DEFAULT_API_BASE;
use crate::http_stream::openai_compatible_base;
use crate::trait_::{GeneratedImage, ProviderConfig};

/// 解析 OpenAI 兼容 API 基址。
fn openai_base(config: &ProviderConfig) -> String {
    let raw = config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_API_BASE);
    openai_compatible_base(raw)
}

/// OpenAI Images API（gpt-image-2 等）
pub async fn openai_generate_image(
    client: &Client,
    prompt: &str,
    config: &ProviderConfig,
) -> Result<Vec<GeneratedImage>> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("OpenAI API Key 为空");
    }
    let model = if config.model.trim().is_empty() {
        "gpt-image-2"
    } else {
        config.model.trim()
    };
    let base = openai_base(config);
    let url = format!("{base}/images/generations");

    let body = json!({
        "model": model,
        "prompt": prompt,
        "n": 1,
        "size": "1024x1024",
    });

    let response = client
        .post(&url)
        .bearer_auth(&config.api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 OpenAI 图片 API 失败: {url}"))?;

    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析 OpenAI 图片响应 JSON 失败")?;

    if !status.is_success() {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .or_else(|| v.get("error").and_then(|e| e.as_str()))
            .unwrap_or("OpenAI 图片生成失败");
        anyhow::bail!("OpenAI HTTP {status}: {msg}");
    }

    let data_arr = v
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| anyhow!("OpenAI 响应中无 data"))?;

    let mut images = Vec::new();
    for item in data_arr {
        if let Some(b64) = item.get("b64_json").and_then(|b| b.as_str()) {
            let data = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .context("解码 OpenAI 图片 base64 失败")?;
            images.push(GeneratedImage {
                data,
                mime_type: "image/png".to_string(),
            });
            continue;
        }
        // 部分模型返回 url：下载
        if let Some(url) = item.get("url").and_then(|u| u.as_str()) {
            let bytes = client
                .get(url)
                .send()
                .await
                .context("下载 OpenAI 图片 URL 失败")?
                .bytes()
                .await
                .context("读取 OpenAI 图片字节失败")?;
            images.push(GeneratedImage {
                data: bytes.to_vec(),
                mime_type: "image/png".to_string(),
            });
        }
    }

    if images.is_empty() {
        anyhow::bail!("OpenAI 未返回图片数据");
    }
    Ok(images)
}
