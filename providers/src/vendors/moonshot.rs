//! Moonshot（Kimi）大模型供应商薄封装。
//!
//! 通过 OpenAI 兼容协议调用 Moonshot API。

use crate::http_stream::openai_compatible_chat_stream;
use crate::trait_::*;
use crate::verify;
use async_trait::async_trait;

/// Moonshot（Kimi）API 的 [`AiProvider`] 实现。
pub struct MoonshotProvider {
    /// 复用的 HTTP 客户端。
    client: reqwest::Client,
}

impl MoonshotProvider {
    /// 创建使用默认 `reqwest::Client` 的实例。
    pub fn new() -> Self {
        MoonshotProvider {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for MoonshotProvider {
    /// 等价于 [`MoonshotProvider::new`]。
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatProvider for MoonshotProvider {
    /// 调用 Moonshot OpenAI 兼容流式接口。
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<serde_json::Value>,
        config: &ProviderConfig,
    ) -> anyhow::Result<ChatStream> {
        openai_compatible_chat_stream(&self.client, "moonshot", messages, tools, config).await
    }
}

#[async_trait]
impl VerifyProvider for MoonshotProvider {
    /// 发送最小 `chat/completions` 请求探测连通性。
    async fn verify(&self, model: &str, config: &ProviderConfig) -> VerifyResult {
        verify::probe(&self.client, "moonshot", model, config).await
    }
}

#[async_trait]
impl AiProvider for MoonshotProvider {
    /// 供应商标识符：`moonshot`。
    fn name(&self) -> &str {
        "moonshot"
    }

    /// 默认模型：`kimi-k2.5`。
    fn default_model(&self) -> &str {
        "kimi-k2.5"
    }
}
