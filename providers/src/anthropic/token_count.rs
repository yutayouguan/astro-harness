//! Anthropic Token Counting API。
//!
//! 端点：`POST /v1/messages/count_tokens`

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};

use super::defaults;
use super::messages::to_anthropic_messages;
use super::tools::openai_tools_to_anthropic;
use crate::http_stream::{resolve_base, trim_slash};
use crate::trait_::ProviderConfig;
use crate::trait_::ChatMessage;

/// 统计消息和工具的 token 数。
pub async fn anthropic_count_tokens(
    client: &Client,
    messages: &[ChatMessage],
    tools: &[Value],
    config: &ProviderConfig,
) -> Result<u32> {
    let base = trim_slash(&resolve_base(config, "claude"));
    let url = format!("{base}/v1/messages/count_tokens");

    let (system, api_messages) = to_anthropic_messages(messages);

    let mut body = json!({
        "model": config.model,
        "messages": api_messages,
    });
    if !system.is_null() {
        body["system"] = system;
    }
    let anthropic_tools = openai_tools_to_anthropic(tools);
    if !anthropic_tools.is_empty() {
        body["tools"] = Value::Array(anthropic_tools);
    }

    let resp = client
        .post(&url)
        .header("x-api-key", &config.api_key)
        .header("anthropic-version", defaults::ANTHROPIC_VERSION)
        .header("anthropic-beta", defaults::ANTHROPIC_BETA)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .context("连接 Anthropic Count Tokens 失败")?;
    let status = resp.status();
    let v: Value = resp
        .json()
        .await
        .context("解析 Count Tokens 响应失败")?;
    if !status.is_success() {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误");
        anyhow::bail!("Anthropic Count Tokens HTTP {status}: {msg}");
    }

    v.get("input_tokens")
        .and_then(|t| t.as_u64())
        .map(|t| t as u32)
        .context("Count Tokens 响应缺少 input_tokens")
}
