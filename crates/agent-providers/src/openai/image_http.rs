//! OpenAI 图片生成 HTTP

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use reqwest::Client;
use serde_json::{json, Value};

use super::defaults::DEFAULT_API_BASE;
use crate::compat::openai_compatible_base;
use crate::types::media::GeneratedImage;
use crate::types::ProviderConfig;

const DEFAULT_IMAGE_MODEL: &str = "gpt-image-2";
const DEFAULT_IMAGE_SIZE: &str = "1024x1024";
const DEFAULT_OUTPUT_FORMAT: &str = "png";
const DEFAULT_OUTPUT_COMPRESSION: u8 = 100;
const MAX_ERROR_BODY_CHARS: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageApiFlavor {
    OpenAiCompatible,
    AzureFoundryV1,
}

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
    generate_image(client, prompt, config, ImageApiFlavor::OpenAiCompatible).await
}

/// Azure AI Foundry OpenAI v1 图片生成。
///
/// 该端点使用 Bearer 认证和 OpenAI v1 路径，不是传统 Azure deployment URL。
pub async fn azure_foundry_generate_image(
    client: &Client,
    prompt: &str,
    config: &ProviderConfig,
) -> Result<Vec<GeneratedImage>> {
    generate_image(client, prompt, config, ImageApiFlavor::AzureFoundryV1).await
}

async fn generate_image(
    client: &Client,
    prompt: &str,
    config: &ProviderConfig,
    flavor: ImageApiFlavor,
) -> Result<Vec<GeneratedImage>> {
    let provider_label = match flavor {
        ImageApiFlavor::OpenAiCompatible => "OpenAI",
        ImageApiFlavor::AzureFoundryV1 => "Azure Foundry",
    };
    if config.api_key.trim().is_empty() {
        anyhow::bail!("{provider_label} API Key 为空");
    }
    let model = if config.model.trim().is_empty() {
        DEFAULT_IMAGE_MODEL
    } else {
        config.model.trim()
    };
    let base = openai_base(config);
    if flavor == ImageApiFlavor::AzureFoundryV1 && !base.ends_with("/openai/v1") {
        anyhow::bail!(
            "Azure 图片生成需要 Foundry OpenAI v1 endpoint（形如 https://<resource>.services.ai.azure.com/openai/v1），当前为 {}",
            sanitized_url(&base)
        );
    }
    let url = format!("{base}/images/generations");
    let safe_url = sanitized_url(&url);

    let body = image_generation_request_body(model, prompt, flavor);
    let response = build_image_generation_request(client, &url, config.api_key.trim(), &body)
        .send()
        .await
        .with_context(|| format!("连接 {provider_label} 图片 API 失败: {safe_url}"))?;

    let status = response.status();
    let response_url = response_url_without_query(response.url());
    let request_id = response
        .headers()
        .get("x-request-id")
        .or_else(|| response.headers().get("apim-request-id"))
        .or_else(|| response.headers().get("x-ms-request-id"))
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let response_bytes = response
        .bytes()
        .await
        .with_context(|| format!("读取 {provider_label} 图片响应失败"))?;

    if !status.is_success() {
        let msg = image_error_message(&response_bytes);
        let request_id = request_id
            .as_deref()
            .map(|id| format!(", request_id={id}"))
            .unwrap_or_default();
        anyhow::bail!(
            "{provider_label} 图片生成 HTTP {status} ({response_url}{request_id}): {msg}"
        );
    }

    let v: Value = serde_json::from_slice(&response_bytes).with_context(|| {
        let request_id = request_id
            .as_deref()
            .map(|id| format!(", request_id={id}"))
            .unwrap_or_default();
        format!("解析 {provider_label} 图片响应 JSON 失败 ({response_url}{request_id})")
    })?;

    let data_arr = v
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| anyhow!("{provider_label} 响应中无 data"))?;

    let mut images = Vec::new();
    for item in data_arr {
        if let Some(b64) = item.get("b64_json").and_then(|b| b.as_str()) {
            let data = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .with_context(|| format!("解码 {provider_label} 图片 base64 失败"))?;
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
                .with_context(|| format!("下载 {provider_label} 图片 URL 失败"))?
                .bytes()
                .await
                .with_context(|| format!("读取 {provider_label} 图片字节失败"))?;
            images.push(GeneratedImage {
                data: bytes.to_vec(),
                mime_type: "image/png".to_string(),
            });
        }
    }

    if images.is_empty() {
        anyhow::bail!("{provider_label} 未返回图片数据");
    }
    Ok(images)
}

fn image_generation_request_body(model: &str, prompt: &str, flavor: ImageApiFlavor) -> Value {
    let mut body = json!({
        "model": model,
        "prompt": prompt,
        "n": 1,
        "size": DEFAULT_IMAGE_SIZE,
    });
    if flavor == ImageApiFlavor::AzureFoundryV1 {
        body["output_format"] = json!(DEFAULT_OUTPUT_FORMAT);
        body["output_compression"] = json!(DEFAULT_OUTPUT_COMPRESSION);
    }
    body
}

fn build_image_generation_request(
    client: &Client,
    url: &str,
    api_key: &str,
    body: &Value,
) -> reqwest::RequestBuilder {
    client
        .post(url)
        .bearer_auth(api_key)
        .header("content-type", "application/json")
        .json(body)
}

fn response_url_without_query(url: &reqwest::Url) -> String {
    let mut sanitized = url.clone();
    sanitized.set_query(None);
    sanitized.set_fragment(None);
    sanitized.to_string()
}

fn sanitized_url(raw: &str) -> String {
    reqwest::Url::parse(raw)
        .map(|url| response_url_without_query(&url))
        .unwrap_or_else(|_| raw.split('?').next().unwrap_or_default().to_string())
}

fn image_error_message(body: &[u8]) -> String {
    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        if let Some(message) = value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .or_else(|| value.get("error").and_then(Value::as_str))
        {
            return message.to_string();
        }
    }

    let text = String::from_utf8_lossy(body);
    let mut chars = text.chars();
    let truncated = chars
        .by_ref()
        .take(MAX_ERROR_BODY_CHARS)
        .collect::<String>();
    if truncated.is_empty() {
        "响应体为空".to_string()
    } else if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

#[cfg(test)]
#[path = "image_http_tests.rs"]
mod tests;
