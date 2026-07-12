//! 智谱 AI（BigModel）大模型供应商薄封装。
//!
//! 通过 OpenAI 兼容协议调用智谱 GLM API。

use crate::http_stream::openai_compatible_chat_stream;
use crate::trait_::*;
use crate::verify;
use async_trait::async_trait;

/// 智谱 AI API 的 [`AiProvider`] 实现。
pub struct ZhipuProvider {
    /// 复用的 HTTP 客户端。
    client: reqwest::Client,
}

impl ZhipuProvider {
    /// 创建使用默认 `reqwest::Client` 的实例。
    pub fn new() -> Self {
        ZhipuProvider {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for ZhipuProvider {
    /// 等价于 [`ZhipuProvider::new`]。
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatProvider for ZhipuProvider {
    /// 调用智谱 OpenAI 兼容流式接口。
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<serde_json::Value>,
        config: &ProviderConfig,
    ) -> anyhow::Result<ChatStream> {
        openai_compatible_chat_stream(&self.client, "zhipu", messages, tools, config).await
    }
}

#[async_trait]
impl VerifyProvider for ZhipuProvider {
    /// 发送最小 `chat/completions` 请求探测连通性。
    async fn verify(&self, model: &str, config: &ProviderConfig) -> VerifyResult {
        verify::probe(&self.client, "zhipu", model, config).await
    }
}

#[async_trait]
impl AiProvider for ZhipuProvider {
    /// 供应商标识符：`zhipu`。
    fn name(&self) -> &str {
        "zhipu"
    }

    /// 默认模型：`glm-4.7-flash`。
    fn default_model(&self) -> &str {
        "glm-4.7-flash"
    }
}
