//! Azure OpenAI 大模型供应商薄封装。
//!
//! 通过 Azure 部署专用 URL 调用 OpenAI 兼容 chat completions SSE。

use crate::http_stream::azure_chat_stream;
use crate::trait_::*;
use crate::verify;
use async_trait::async_trait;

/// Azure OpenAI 服务的 [`AiProvider`] 实现。
pub struct AzureProvider {
    /// 复用的 HTTP 客户端。
    client: reqwest::Client,
}

impl AzureProvider {
    /// 创建使用默认 `reqwest::Client` 的实例。
    pub fn new() -> Self {
        AzureProvider {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for AzureProvider {
    /// 等价于 [`AzureProvider::new`]。
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatProvider for AzureProvider {
    /// 调用 Azure 部署流式 chat completions 接口。
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<serde_json::Value>,
        config: &ProviderConfig,
    ) -> anyhow::Result<ChatStream> {
        azure_chat_stream(&self.client, messages, tools, config).await
    }
}

#[async_trait]
impl VerifyProvider for AzureProvider {
    /// 发送最小 chat completions 请求探测连通性。
    async fn verify(&self, model: &str, config: &ProviderConfig) -> VerifyResult {
        verify::probe(&self.client, "azure", model, config).await
    }
}

#[async_trait]
impl AiProvider for AzureProvider {
    /// 供应商标识符：`azure`。
    fn name(&self) -> &str {
        "azure"
    }

    /// 默认部署名：`gpt-5.6`（实际为 Azure deployment id，建议与 OpenAI 对齐）。
    fn default_model(&self) -> &str {
        "gpt-5.6"
    }
}
