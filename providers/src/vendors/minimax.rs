//! MiniMax 大模型供应商薄封装。
//!
//! 通过 OpenAI 兼容协议调用 MiniMax Chat API。

use crate::http_stream::openai_compatible_chat_stream;
use crate::trait_::*;
use crate::verify;
use async_trait::async_trait;

/// MiniMax API 的 [`AiProvider`] 实现。
pub struct MiniMaxProvider {
    /// 复用的 HTTP 客户端。
    client: reqwest::Client,
}

impl MiniMaxProvider {
    /// 创建使用默认 `reqwest::Client` 的实例。
    pub fn new() -> Self {
        MiniMaxProvider {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for MiniMaxProvider {
    /// 等价于 [`MiniMaxProvider::new`]。
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatProvider for MiniMaxProvider {
    /// 调用 MiniMax OpenAI 兼容流式接口。
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<serde_json::Value>,
        config: &ProviderConfig,
    ) -> anyhow::Result<ChatStream> {
        openai_compatible_chat_stream(&self.client, "minimax", messages, tools, config).await
    }
}

#[async_trait]
impl VerifyProvider for MiniMaxProvider {
    /// 发送最小 `chat/completions` 请求探测连通性。
    async fn verify(&self, model: &str, config: &ProviderConfig) -> VerifyResult {
        verify::probe(&self.client, "minimax", model, config).await
    }
}

#[async_trait]
impl AiProvider for MiniMaxProvider {
    /// 供应商标识符：`minimax`（别名 `minmax`）。
    fn name(&self) -> &str {
        "minimax"
    }

    /// 默认模型：`MiniMax-M2.5`。
    fn default_model(&self) -> &str {
        "MiniMax-M2.5"
    }
}
