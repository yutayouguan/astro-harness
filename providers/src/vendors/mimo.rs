//! 小米 Mimo 大模型供应商薄封装。
//!
//! 通过 OpenAI 兼容协议调用小米 Mimo API。

use crate::http_stream::openai_compatible_chat_stream;
use crate::trait_::*;
use crate::verify;
use async_trait::async_trait;

/// 小米 Mimo API 的 [`AiProvider`] 实现。
pub struct MimoProvider {
    /// 复用的 HTTP 客户端。
    client: reqwest::Client,
}

impl MimoProvider {
    /// 创建使用默认 `reqwest::Client` 的实例。
    pub fn new() -> Self {
        MimoProvider {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for MimoProvider {
    /// 等价于 [`MimoProvider::new`]。
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatProvider for MimoProvider {
    /// 调用 Mimo OpenAI 兼容流式接口。
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<serde_json::Value>,
        config: &ProviderConfig,
    ) -> anyhow::Result<ChatStream> {
        openai_compatible_chat_stream(&self.client, "mimo", messages, tools, config).await
    }
}

#[async_trait]
impl VerifyProvider for MimoProvider {
    /// 发送最小 `chat/completions` 请求探测连通性。
    async fn verify(&self, model: &str, config: &ProviderConfig) -> VerifyResult {
        verify::probe(&self.client, "mimo", model, config).await
    }
}

#[async_trait]
impl AiProvider for MimoProvider {
    /// 供应商标识符：`mimo`。
    fn name(&self) -> &str {
        "mimo"
    }

    /// 默认模型：`mimo-7b`。
    fn default_model(&self) -> &str {
        "mimo-7b"
    }
}
