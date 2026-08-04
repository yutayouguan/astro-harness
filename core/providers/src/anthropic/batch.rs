//! Anthropic Message Batches API。
//!
//! 端点：`/v1/messages/batches`

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};

use super::defaults;
use crate::http_stream::{resolve_base, trim_slash};
use crate::types::request::ProviderConfig;

fn batch_url(config: &ProviderConfig) -> String {
    let base = trim_slash(&resolve_base(config, "claude"));
    format!("{base}/v1/messages/batches")
}

fn auth_headers(client: &Client, url: &str, config: &ProviderConfig) -> reqwest::RequestBuilder {
    client
        .post(url)
        .header("x-api-key", &config.api_key)
        .header("anthropic-version", defaults::ANTHROPIC_VERSION)
        .header("anthropic-beta", defaults::ANTHROPIC_BETA)
        .header("content-type", "application/json")
}

/// 创建批处理任务。
///
/// `requests` 中每个元素为 `{"custom_id": "...", "params": {Messages API 请求体}}`。
pub async fn anthropic_create_batch(
    client: &Client,
    requests: &[Value],
    config: &ProviderConfig,
) -> Result<Value> {
    let url = batch_url(config);
    let body = json!({ "requests": requests });
    let resp = auth_headers(client, &url, config)
        .json(&body)
        .send()
        .await
        .context("连接 Anthropic Batches 失败")?;
    let status = resp.status();
    let v: Value = resp.json().await.context("解析 Batches 响应失败")?;
    if !status.is_success() {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误");
        anyhow::bail!("Anthropic Batches HTTP {status}: {msg}");
    }
    Ok(v)
}

/// 查询批处理任务状态。
pub async fn anthropic_get_batch(
    client: &Client,
    batch_id: &str,
    config: &ProviderConfig,
) -> Result<Value> {
    let url = format!("{}/{batch_id}", batch_url(config));
    let resp = client
        .get(&url)
        .header("x-api-key", &config.api_key)
        .header("anthropic-version", defaults::ANTHROPIC_VERSION)
        .header("anthropic-beta", defaults::ANTHROPIC_BETA)
        .send()
        .await
        .context("连接 Anthropic Batches 失败")?;
    let status = resp.status();
    let v: Value = resp.json().await.context("解析 Batches 响应失败")?;
    if !status.is_success() {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误");
        anyhow::bail!("Anthropic Batches GET HTTP {status}: {msg}");
    }
    Ok(v)
}

/// 列出批处理任务。
pub async fn anthropic_list_batches(
    client: &Client,
    config: &ProviderConfig,
) -> Result<Value> {
    let url = batch_url(config);
    let resp = client
        .get(&url)
        .header("x-api-key", &config.api_key)
        .header("anthropic-version", defaults::ANTHROPIC_VERSION)
        .header("anthropic-beta", defaults::ANTHROPIC_BETA)
        .send()
        .await
        .context("连接 Anthropic Batches 失败")?;
    let status = resp.status();
    let v: Value = resp.json().await.context("解析 Batches 响应失败")?;
    if !status.is_success() {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误");
        anyhow::bail!("Anthropic Batches LIST HTTP {status}: {msg}");
    }
    Ok(v)
}

/// 获取批处理结果（JSONL 流）。
pub async fn anthropic_batch_results(
    client: &Client,
    batch_id: &str,
    config: &ProviderConfig,
) -> Result<String> {
    let url = format!("{}/{batch_id}/results", batch_url(config));
    let resp = client
        .get(&url)
        .header("x-api-key", &config.api_key)
        .header("anthropic-version", defaults::ANTHROPIC_VERSION)
        .header("anthropic-beta", defaults::ANTHROPIC_BETA)
        .send()
        .await
        .context("连接 Anthropic Batches Results 失败")?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Anthropic Batches Results HTTP {status}: {body}");
    }
    resp.text().await.context("读取 Batches Results 失败")
}
