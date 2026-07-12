//! DeepSeek 大模型供应商薄封装。
//!
//! 通过 OpenAI 兼容协议调用 DeepSeek API，支持 thinking 模式与 reasoning 流式回传。

use crate::http_stream::openai_compatible_chat_stream;
use crate::trait_::*;
use crate::verify;
use async_trait::async_trait;

/// DeepSeek API 的 [`AiProvider`] 实现。
pub struct DeepSeekProvider {
    /// 复用的 HTTP 客户端。
    client: reqwest::Client,
}

impl DeepSeekProvider {
    /// 创建使用默认 `reqwest::Client` 的实例。
    pub fn new() -> Self {
        DeepSeekProvider {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for DeepSeekProvider {
    /// 等价于 [`DeepSeekProvider::new`]。
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatProvider for DeepSeekProvider {
    /// 调用 DeepSeek OpenAI 兼容流式接口（含 thinking 参数）。
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<serde_json::Value>,
        config: &ProviderConfig,
    ) -> anyhow::Result<ChatStream> {
        openai_compatible_chat_stream(&self.client, "deepseek", messages, tools, config).await
    }
}

#[async_trait]
impl VerifyProvider for DeepSeekProvider {
    /// 发送最小 `chat/completions` 请求探测连通性。
    async fn verify(&self, model: &str, config: &ProviderConfig) -> VerifyResult {
        verify::probe(&self.client, "deepseek", model, config).await
    }
}

#[async_trait]
impl AiProvider for DeepSeekProvider {
    /// 供应商标识符：`deepseek`。
    fn name(&self) -> &str {
        "deepseek"
    }

    /// 默认模型：`deepseek-chat`。
    fn default_model(&self) -> &str {
        "deepseek-chat"
    }
}
