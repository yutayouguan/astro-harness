//! Azure OpenAI 流式聊天（deployment URL + api-key 认证）。

use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde_json::{json, Value};

use super::chat::{extract_openai_delta, to_openai_messages};
use crate::http_stream::{merge_additional_params, resolve_base, sse_chat_stream};
use crate::trait_::{ChatMessage, ChatStream, ProviderConfig};

/// Azure OpenAI REST API 版本号。
pub const AZURE_API_VERSION: &str = "2024-06-01";

/// 从 Azure endpoint 提取资源根路径（去掉 `/openai`、`/v1` 后缀）。
pub fn azure_base(endpoint: &str) -> String {
    crate::http_stream::trim_slash(endpoint)
        .trim_end_matches("/openai")
        .trim_end_matches("/v1")
        .to_string()
}

/// Azure OpenAI 流式 chat completions（ChatCompletions + URL/Auth quirk）。
pub async fn azure_chat_stream(
    client: &Client,
    messages: Vec<ChatMessage>,
    tools: Vec<Value>,
    config: &ProviderConfig,
) -> Result<ChatStream> {
    if config.api_key.is_empty() {
        return Err(anyhow!("缺少 Azure OpenAI API Key"));
    }
    let base = azure_base(&resolve_base(config, "azure"));
    if base.is_empty() || base.contains("YOUR_RESOURCE") {
        return Err(anyhow!("请配置有效的 Azure OpenAI endpoint"));
    }
    let deployment = &config.model;
    let url = format!(
        "{base}/openai/deployments/{deployment}/chat/completions?api-version={AZURE_API_VERSION}"
    );

    let mut body = json!({
        "messages": to_openai_messages(&messages),
        "stream": true,
        "temperature": config.temperature,
        "max_tokens": config.max_tokens,
    });
    // Azure OpenAI 支持 stream_options.include_usage
    body["stream_options"] = json!({ "include_usage": true });
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools);
        body["tool_choice"] = json!("auto");
    }

    merge_additional_params(&mut body, &config.additional_params);
    let response = client
        .post(&url)
        .header("api-key", &config.api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .with_context(|| format!("连接 Azure OpenAI 失败: {url}"))?;

    sse_chat_stream(response, Arc::new(extract_openai_delta)).await
}
