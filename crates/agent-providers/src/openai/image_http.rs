//! OpenAI 图片生成 HTTP

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use futures::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};

use super::defaults::DEFAULT_API_BASE;
use crate::compat::openai_compatible_base;
use crate::types::media::{GeneratedImage, ImageGenConfig};
use crate::types::ProviderConfig;

const DEFAULT_IMAGE_MODEL: &str = "gpt-image-2";
const DEFAULT_IMAGE_SIZE: &str = "1024x1024";
const DEFAULT_OUTPUT_FORMAT: &str = "png";
const DEFAULT_OUTPUT_COMPRESSION: u8 = 100;
const MAX_ERROR_BODY_CHARS: usize = 4096;
const MAX_IMAGE_BYTES: usize = 32 * 1024 * 1024;
const MAX_IMAGE_BASE64_BYTES: usize = MAX_IMAGE_BYTES.div_ceil(3) * 4;
const MAX_IMAGE_RESPONSE_BYTES: usize = 128 * 1024 * 1024;

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
    openai_generate_image_with_config(client, prompt, config, &ImageGenConfig::default()).await
}

pub async fn openai_generate_image_with_config(
    client: &Client,
    prompt: &str,
    config: &ProviderConfig,
    image_config: &ImageGenConfig,
) -> Result<Vec<GeneratedImage>> {
    generate_image(
        client,
        prompt,
        config,
        image_config,
        ImageApiFlavor::OpenAiCompatible,
    )
    .await
}

/// Azure AI Foundry OpenAI v1 图片生成。
///
/// 该端点使用 Bearer 认证和 OpenAI v1 路径，不是传统 Azure deployment URL。
pub async fn azure_foundry_generate_image(
    client: &Client,
    prompt: &str,
    config: &ProviderConfig,
) -> Result<Vec<GeneratedImage>> {
    azure_foundry_generate_image_with_config(client, prompt, config, &ImageGenConfig::default())
        .await
}

pub async fn azure_foundry_generate_image_with_config(
    client: &Client,
    prompt: &str,
    config: &ProviderConfig,
    image_config: &ImageGenConfig,
) -> Result<Vec<GeneratedImage>> {
    generate_image(
        client,
        prompt,
        config,
        image_config,
        ImageApiFlavor::AzureFoundryV1,
    )
    .await
}

async fn generate_image(
    client: &Client,
    prompt: &str,
    config: &ProviderConfig,
    image_config: &ImageGenConfig,
    flavor: ImageApiFlavor,
) -> Result<Vec<GeneratedImage>> {
    let provider_label = match flavor {
        ImageApiFlavor::OpenAiCompatible => "OpenAI",
        ImageApiFlavor::AzureFoundryV1 => "Azure Foundry",
    };
    if config.api_key.trim().is_empty() {
        anyhow::bail!("{provider_label} API Key 为空");
    }
    let model = if !image_config.model.trim().is_empty() {
        image_config.model.trim()
    } else if config.model.trim().is_empty() {
        DEFAULT_IMAGE_MODEL
    } else {
        config.model.trim()
    };
    let base = match flavor {
        ImageApiFlavor::OpenAiCompatible => openai_base(config),
        ImageApiFlavor::AzureFoundryV1 => {
            let endpoint = config.base_url.as_deref().unwrap_or_default();
            crate::impls::azure::azure_openai_v1_base(endpoint)
        }
    };
    let url = format!("{base}/images/generations");
    let safe_url = sanitized_url(&url);

    let body = image_generation_request_body(model, prompt, image_config, flavor)?;
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
    if response
        .content_length()
        .is_some_and(|length| length > MAX_IMAGE_RESPONSE_BYTES as u64)
    {
        anyhow::bail!("{provider_label} 图片响应超过 128 MiB 限制");
    }
    let response_bytes = read_limited_body(
        response,
        MAX_IMAGE_RESPONSE_BYTES,
        &format!("读取 {provider_label} 图片响应"),
    )
    .await?;

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
            if b64.len() > MAX_IMAGE_BASE64_BYTES {
                anyhow::bail!("{provider_label} 图片超过 32 MiB 限制");
            }
            let data = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .with_context(|| format!("解码 {provider_label} 图片 base64 失败"))?;
            if data.len() > MAX_IMAGE_BYTES {
                anyhow::bail!("{provider_label} 图片超过 32 MiB 限制");
            }
            images.push(GeneratedImage {
                data,
                mime_type: output_mime_type(image_config),
            });
            continue;
        }
        // 部分模型返回 url：下载
        if let Some(url) = item.get("url").and_then(|u| u.as_str()) {
            validate_remote_image_url(url)?;
            let response = client
                .get(url)
                .send()
                .await
                .with_context(|| format!("下载 {provider_label} 图片 URL 失败"))?
                .error_for_status()
                .with_context(|| format!("下载 {provider_label} 图片 URL 返回错误状态"))?;
            if response
                .content_length()
                .is_some_and(|length| length > MAX_IMAGE_BYTES as u64)
            {
                anyhow::bail!("{provider_label} 图片超过 32 MiB 限制");
            }
            let bytes = read_limited_body(
                response,
                MAX_IMAGE_BYTES,
                &format!("读取 {provider_label} 图片字节"),
            )
            .await?;
            if bytes.len() > MAX_IMAGE_BYTES {
                anyhow::bail!("{provider_label} 图片超过 32 MiB 限制");
            }
            images.push(GeneratedImage {
                data: bytes.to_vec(),
                mime_type: output_mime_type(image_config),
            });
        }
    }

    if images.is_empty() {
        anyhow::bail!("{provider_label} 未返回图片数据");
    }
    Ok(images)
}

fn image_generation_request_body(
    model: &str,
    prompt: &str,
    config: &ImageGenConfig,
    flavor: ImageApiFlavor,
) -> Result<Value> {
    let n = if config.n == 0 { 1 } else { config.n };
    if !(1..=10).contains(&n) {
        anyhow::bail!("图片生成张数 n 必须在 1..=10 之间");
    }
    let size = match (config.width, config.height) {
        (Some(width), Some(height)) if width > 0 && height > 0 => format!("{width}x{height}"),
        (None, None) => DEFAULT_IMAGE_SIZE.to_string(),
        _ => anyhow::bail!("图片尺寸必须同时提供 width 和 height"),
    };
    let output_format = normalized_output_format(config.output_format.as_deref())?;
    if config.output_compression.is_some_and(|value| value > 100) {
        anyhow::bail!("output_compression 必须在 0..=100 之间");
    }
    let mut body = json!({
        "model": model,
        "prompt": prompt,
        "n": n,
        "size": size,
    });
    if flavor == ImageApiFlavor::AzureFoundryV1 {
        body["output_format"] = json!(output_format.as_deref().unwrap_or(DEFAULT_OUTPUT_FORMAT));
        body["output_compression"] = json!(config
            .output_compression
            .unwrap_or(DEFAULT_OUTPUT_COMPRESSION));
    } else if let Some(format) = output_format {
        body["output_format"] = json!(format);
        if let Some(compression) = config.output_compression {
            body["output_compression"] = json!(compression);
        }
    }
    if let Some(quality) = non_empty(config.quality.as_deref()) {
        body["quality"] = json!(quality);
    }
    if let Some(background) = non_empty(config.background.as_deref()) {
        body["background"] = json!(background);
    }
    if let Some(extra) = config.additional_params.as_object() {
        let object = body
            .as_object_mut()
            .expect("image request body is an object");
        for (key, value) in extra {
            if !matches!(
                key.as_str(),
                "model"
                    | "prompt"
                    | "n"
                    | "size"
                    | "output_format"
                    | "output_compression"
                    | "quality"
                    | "background"
            ) {
                object.insert(key.clone(), value.clone());
            }
        }
    }
    Ok(body)
}

fn normalized_output_format(value: Option<&str>) -> Result<Option<String>> {
    let Some(value) = non_empty(value) else {
        return Ok(None);
    };
    let normalized = match value.to_ascii_lowercase().as_str() {
        "png" => "png",
        "jpg" | "jpeg" => "jpeg",
        "webp" => "webp",
        _ => anyhow::bail!("不支持的图片输出格式: {value}"),
    };
    Ok(Some(normalized.to_string()))
}

fn output_mime_type(config: &ImageGenConfig) -> String {
    match config
        .output_format
        .as_deref()
        .unwrap_or(DEFAULT_OUTPUT_FORMAT)
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    }
    .to_string()
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn validate_remote_image_url(raw: &str) -> Result<()> {
    let url = reqwest::Url::parse(raw).context("图片返回 URL 无效")?;
    if !matches!(url.scheme(), "http" | "https") {
        anyhow::bail!("图片返回 URL 仅支持 http/https");
    }
    if !url.username().is_empty() || url.password().is_some() {
        anyhow::bail!("图片返回 URL 不得包含用户信息");
    }
    let host = url.host_str().unwrap_or_default();
    let ip_literal = host.trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost")
        || host.ends_with(".localhost")
        || ip_literal
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| !is_public_ip(ip))
    {
        anyhow::bail!("图片返回 URL 不得指向本机或私有地址");
    }
    Ok(())
}

fn is_public_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ip) => {
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_broadcast()
                || ip.octets()[0] == 0)
        }
        std::net::IpAddr::V6(ip) => {
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_unique_local()
                || ip.is_unicast_link_local())
        }
    }
}

async fn read_limited_body(
    response: reqwest::Response,
    limit: usize,
    context: &str,
) -> Result<Vec<u8>> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| format!("{context}失败"))?;
        if body.len().saturating_add(chunk.len()) > limit {
            anyhow::bail!("{context}超过 {} MiB 限制", limit / 1024 / 1024);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
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
