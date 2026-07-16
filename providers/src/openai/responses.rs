//! OpenAI Responses API 适配器骨架（Hermes 第三协议）。
//!
//! 当前无默认 [`crate::profile::ProviderProfile`] 绑定；调用返回明确错误。

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::Value;

use crate::trait_::{ChatMessage, ChatStream, ProviderConfig};

/// Responses 模式入口；尚未接线真实 HTTP。
pub async fn responses_chat_stream(
    _client: &Client,
    _provider: &str,
    _messages: Vec<ChatMessage>,
    _tools: Vec<Value>,
    _config: &ProviderConfig,
) -> Result<ChatStream> {
    Err(anyhow!(
        "ApiMode::Responses 尚未接线：请使用 ChatCompletions 或 AnthropicMessages profile"
    ))
}
