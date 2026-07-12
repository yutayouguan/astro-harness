//! NVIDIA NIM 大模型供应商薄封装。
//!
//! 通过 OpenAI 兼容协议调用 NVIDIA Integrate API。

use crate::http_stream::openai_compatible_chat_stream;
use crate::trait_::*;
use crate::verify;
use async_trait::async_trait;

/// NVIDIA Integrate API 的 [`AiProvider`] 实现。
pub struct NvidiaProvider {
    /// 复用的 HTTP 客户端。
    client: reqwest::Client,
}

impl NvidiaProvider {
    /// 创建使用默认 `reqwest::Client` 的实例。
    pub fn new() -> Self {
        NvidiaProvider {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for NvidiaProvider {
    /// 等价于 [`NvidiaProvider::new`]。
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatProvider for NvidiaProvider {
    /// 调用 NVIDIA OpenAI 兼容流式接口。
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<serde_json::Value>,
        config: &ProviderConfig,
    ) -> anyhow::Result<ChatStream> {
        openai_compatible_chat_stream(&self.client, "nvidia", messages, tools, config).await
    }
}

#[async_trait]
impl VerifyProvider for NvidiaProvider {
    /// 发送最小 `chat/completions` 请求探测连通性。
    async fn verify(&self, model: &str, config: &ProviderConfig) -> VerifyResult {
        verify::probe(&self.client, "nvidia", model, config).await
    }
}

#[async_trait]
impl AiProvider for NvidiaProvider {
    /// 供应商标识符：`nvidia`。
    fn name(&self) -> &str {
        "nvidia"
    }

    /// 默认模型：`meta/llama-3.3-70b-instruct`。
    fn default_model(&self) -> &str {
        "meta/llama-3.3-70b-instruct"
    }
}
