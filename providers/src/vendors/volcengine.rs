//! 火山引擎（豆包 Ark）大模型供应商薄封装。
//!
//! 通过 OpenAI 兼容协议调用火山引擎 Ark API。

use crate::http_stream::openai_compatible_chat_stream;
use crate::trait_::*;
use crate::verify;
use async_trait::async_trait;

/// 火山引擎 Ark API 的 [`AiProvider`] 实现。
pub struct VolcengineProvider {
    /// 复用的 HTTP 客户端。
    client: reqwest::Client,
}

impl VolcengineProvider {
    /// 创建使用默认 `reqwest::Client` 的实例。
    pub fn new() -> Self {
        VolcengineProvider {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for VolcengineProvider {
    /// 等价于 [`VolcengineProvider::new`]。
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatProvider for VolcengineProvider {
    /// 调用火山引擎 OpenAI 兼容流式接口。
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<serde_json::Value>,
        config: &ProviderConfig,
    ) -> anyhow::Result<ChatStream> {
        openai_compatible_chat_stream(&self.client, "volcengine", messages, tools, config).await
    }
}

#[async_trait]
impl VerifyProvider for VolcengineProvider {
    /// 发送最小 `chat/completions` 请求探测连通性。
    async fn verify(&self, model: &str, config: &ProviderConfig) -> VerifyResult {
        verify::probe(&self.client, "volcengine", model, config).await
    }
}

#[async_trait]
impl AiProvider for VolcengineProvider {
    /// 供应商标识符：`volcengine`。
    fn name(&self) -> &str {
        "volcengine"
    }

    /// 默认模型：火山 endpoint 前缀 `ep-`（需用户填写完整 endpoint id）。
    fn default_model(&self) -> &str {
        "ep-"
    }
}
