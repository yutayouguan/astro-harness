//! Anthropic Claude 大模型供应商薄封装。
//!
//! 通过 Anthropic Messages API SSE 实现流式对话与工具调用。

use crate::http_stream::anthropic_chat_stream;
use crate::trait_::*;
use crate::verify;
use async_trait::async_trait;

/// Claude（Anthropic）API 的 [`AiProvider`] 实现。
pub struct ClaudeProvider {
    /// 复用的 HTTP 客户端。
    client: reqwest::Client,
}

impl ClaudeProvider {
    /// 创建使用默认 `reqwest::Client` 的实例。
    pub fn new() -> Self {
        ClaudeProvider {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for ClaudeProvider {
    /// 等价于 [`ClaudeProvider::new`]。
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatProvider for ClaudeProvider {
    /// 调用 Anthropic Messages 流式接口。
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<serde_json::Value>,
        config: &ProviderConfig,
    ) -> anyhow::Result<ChatStream> {
        anthropic_chat_stream(&self.client, messages, tools, config).await
    }
}

#[async_trait]
impl VerifyProvider for ClaudeProvider {
    /// 发送最小 Messages 请求探测连通性。
    async fn verify(&self, model: &str, config: &ProviderConfig) -> VerifyResult {
        verify::probe(&self.client, "claude", model, config).await
    }
}

#[async_trait]
impl AiProvider for ClaudeProvider {
    /// 供应商标识符：`claude`（别名 `anthropic`）。
    fn name(&self) -> &str {
        "claude"
    }

    /// 默认模型：`claude-opus-4-8`。
    fn default_model(&self) -> &str {
        "claude-opus-4-8"
    }

    /// 不支持图片生成。
    fn supports_image_gen(&self) -> bool {
        false
    }
}
