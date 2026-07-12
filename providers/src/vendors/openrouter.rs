//! OpenRouter 聚合路由供应商薄封装。
//!
//! 通过 OpenAI 兼容协议访问 OpenRouter 上的多模型路由。

use crate::http_stream::openai_compatible_chat_stream;
use crate::trait_::*;
use crate::verify;
use async_trait::async_trait;

/// OpenRouter API 的 [`AiProvider`] 实现。
pub struct OpenRouterProvider {
    /// 复用的 HTTP 客户端。
    client: reqwest::Client,
}

impl OpenRouterProvider {
    /// 创建使用默认 `reqwest::Client` 的实例。
    pub fn new() -> Self {
        OpenRouterProvider {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for OpenRouterProvider {
    /// 等价于 [`OpenRouterProvider::new`]。
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatProvider for OpenRouterProvider {
    /// 调用 OpenRouter 流式 `chat/completions` 接口。
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<serde_json::Value>,
        config: &ProviderConfig,
    ) -> anyhow::Result<ChatStream> {
        openai_compatible_chat_stream(&self.client, "openrouter", messages, tools, config).await
    }
}

#[async_trait]
impl VerifyProvider for OpenRouterProvider {
    /// 发送最小 `chat/completions` 请求探测连通性。
    async fn verify(&self, model: &str, config: &ProviderConfig) -> VerifyResult {
        verify::probe(&self.client, "openrouter", model, config).await
    }
}

#[async_trait]
impl AiProvider for OpenRouterProvider {
    /// 供应商标识符：`openrouter`。
    fn name(&self) -> &str {
        "openrouter"
    }

    /// 默认模型：`openai/gpt-5.6`。
    fn default_model(&self) -> &str {
        "openai/gpt-5.6"
    }
}
