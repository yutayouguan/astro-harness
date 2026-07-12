//! Google / OpenAI 图片生成 HTTP

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use reqwest::Client;
use serde_json::{json, Value};

use crate::http_stream::openai_compatible_base;
use crate::trait_::{GeneratedImage, ProviderConfig};

/// 去掉 endpoint 末尾斜杠。
fn trim_slash(endpoint: &str) -> String {
    endpoint.trim_end_matches('/').to_string()
}

/// 解析 Google API 基址（配置优先，否则官方默认）。
fn google_base(config: &ProviderConfig) -> String {
    config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("https://generativelanguage.googleapis.com")
        .to_string()
}

/// 解析 OpenAI 兼容 API 基址。
fn openai_base(config: &ProviderConfig) -> String {
    let raw = config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("https://api.openai.com/v1");
    openai_compatible_base(raw)
}

/// Google Gemini 原生出图（generateContent + responseModalities IMAGE）
pub async fn google_generate_image(
    client: &Client,
    prompt: &str,
    config: &ProviderConfig,
) -> Result<Vec<GeneratedImage>> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    let model = if config.model.trim().is_empty() {
        "gemini-3-pro-image-preview"
    } else {
        config.model.trim()
    };
    let base = trim_slash(&google_base(config));
    let url = if base.contains("/v1beta") {
        format!("{base}/models/{model}:generateContent?key={}", config.api_key)
    } else {
        format!(
            "{base}/v1beta/models/{model}:generateContent?key={}",
            config.api_key
        )
    };

    let body = json!({
        "contents": [{
            "role": "user",
            "parts": [{"text": prompt}]
        }],
        "generationConfig": {
            "responseModalities": ["TEXT", "IMAGE"]
        }
    });

    let response = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 Google 图片 API 失败: {url}"))?;

    let status = response.status();
    let v: Value = response
        .json()
        .await
        .context("解析 Google 图片响应 JSON 失败")?;

    if !status.is_success() {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("Google 图片生成失败");
        anyhow::bail!("Google HTTP {status}: {msg}");
    }

    let mut images = Vec::new();
    let parts = v
        .pointer("/candidates/0/content/parts")
        .and_then(|p| p.as_array())
        .ok_or_else(|| anyhow!("Google 响应中无图片 parts"))?;

    for part in parts {
        let inline = part.get("inlineData").or_else(|| part.get("inline_data"));
        let Some(inline) = inline else { continue };
        let b64 = inline
            .get("data")
            .and_then(|d| d.as_str())
            .ok_or_else(|| anyhow!("inlineData 缺少 data"))?;
        let mime = inline
            .get("mimeType")
            .or_else(|| inline.get("mime_type"))
            .and_then(|m| m.as_str())
            .unwrap_or("image/png")
            .to_string();
        let data = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .context("解码 Google 图片 base64 失败")?;
        images.push(GeneratedImage {
            data,
            mime_type: mime,
        });
    }

    if images.is_empty() {
        anyhow::bail!("Google 未返回图片数据");
    }
    Ok(images)
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
