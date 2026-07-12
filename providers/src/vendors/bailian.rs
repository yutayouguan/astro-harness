//! 阿里云百炼（DashScope 兼容模式）供应商薄封装。
//!
//! 通过 OpenAI 兼容协议调用百炼 / 通义千问 API。

use crate::http_stream::openai_compatible_chat_stream;
use crate::trait_::*;
use crate::verify;
use async_trait::async_trait;

/// 百炼（DashScope 兼容模式）的 [`AiProvider`] 实现。
pub struct BailianProvider {
    /// 复用的 HTTP 客户端。
    client: reqwest::Client,
}

impl BailianProvider {
    /// 创建使用默认 `reqwest::Client` 的实例。
    pub fn new() -> Self {
        BailianProvider {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for BailianProvider {
    /// 等价于 [`BailianProvider::new`]。
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatProvider for BailianProvider {
    /// 调用百炼 OpenAI 兼容流式接口。
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<serde_json::Value>,
        config: &ProviderConfig,
    ) -> anyhow::Result<ChatStream> {
        openai_compatible_chat_stream(&self.client, "bailian", messages, tools, config).await
    }
}

#[async_trait]
impl VerifyProvider for BailianProvider {
    /// 发送最小 `chat/completions` 请求探测连通性。
    async fn verify(&self, model: &str, config: &ProviderConfig) -> VerifyResult {
        verify::probe(&self.client, "bailian", model, config).await
    }
}

#[async_trait]
impl AiProvider for BailianProvider {
    /// 供应商标识符：`bailian`。
    fn name(&self) -> &str {
        "bailian"
    }

    /// 默认模型：`qwen3.6-plus`。
    fn default_model(&self) -> &str {
        "qwen3.6-plus"
    }
}
