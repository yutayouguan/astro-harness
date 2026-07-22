//! Anthropic Messages API 流式聊天入口。

use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde_json::{json, Value};

use super::defaults;
use super::messages::to_anthropic_messages;
use super::sse::extract_anthropic_delta;
use super::tools::openai_tools_to_anthropic;
use crate::http_stream::{merge_additional_params, resolve_base, sse_chat_stream, trim_slash};
use crate::trait_::{ChatMessage, ChatStream, ProviderConfig};

/// Anthropic Messages API 流式
pub async fn anthropic_chat_stream(
    client: &Client,
    messages: Vec<ChatMessage>,
    tools: Vec<Value>,
    config: &ProviderConfig,
) -> Result<ChatStream> {
    if config.api_key.is_empty() {
        return Err(anyhow!("缺少 Anthropic API Key"));
    }
    let base = trim_slash(&resolve_base(config, "claude"));
    let url = format!("{base}/v1/messages");

    let (system, api_messages) = to_anthropic_messages(&messages);

    let mut body = json!({
        "model": config.model,
        "max_tokens": config.max_tokens,
        "stream": true,
        "temperature": config.temperature,
        "messages": api_messages,
    });
    if !system.is_null() {
        body["system"] = system;
    }

    // Extended Thinking（budget_tokens 须 >= 1024 且 < max_tokens）
    if config.thinking_enabled {
        let raw_budget = match config.reasoning_effort.trim() {
            "max" => defaults::THINKING_BUDGET_MAX,
            "high" | "" => defaults::THINKING_BUDGET_HIGH,
            other => other
                .parse::<u32>()
                .unwrap_or(defaults::THINKING_BUDGET_HIGH),
        };
        let budget = raw_budget.clamp(1024, config.max_tokens.saturating_sub(1).max(1024));
        body["thinking"] = json!({
            "type": "enabled",
            "budget_tokens": budget,
        });
        // Anthropic 要求 thinking 启用时 temperature 必须为 1 或不传
        body.as_object_mut().unwrap().remove("temperature");
    }

    let anthropic_tools = openai_tools_to_anthropic(&tools);
    if !anthropic_tools.is_empty() {
        body["tools"] = Value::Array(anthropic_tools);
    }
    if let Some(tc) = config.additional_params.get("tool_choice") {
        body["tool_choice"] = tc.clone();
    }

    merge_additional_params(&mut body, &config.additional_params);

    let mut req = client
        .post(&url)
        .header("x-api-key", &config.api_key)
        .header("anthropic-version", defaults::ANTHROPIC_VERSION)
        .header("anthropic-beta", defaults::ANTHROPIC_BETA)
        .header("content-type", "application/json")
        .json(&body);

    // 兼容第三方 Anthropic 代理：部分代理不认 x-api-key，额外发 Bearer
    if config
        .base_url
        .as_deref()
        .is_some_and(|u| !u.is_empty() && !u.contains("anthropic.com"))
    {
        req = req.bearer_auth(&config.api_key);
    }

    let response = req
        .send()
        .await
        .with_context(|| format!("连接 Anthropic 失败: {url}"))?;

    sse_chat_stream(response, Arc::new(extract_anthropic_delta)).await
}
