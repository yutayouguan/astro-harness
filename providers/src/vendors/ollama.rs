//! Ollama 本地大模型供应商薄封装。
//!
//! 通过 Ollama 原生 `/api/chat` NDJSON 或 OpenAI 兼容端点实现流式对话。

use crate::http_stream::ollama_chat_stream;
use crate::trait_::*;
use crate::verify;
use async_trait::async_trait;

/// 本地 Ollama 服务的 [`AiProvider`] 实现。
pub struct OllamaProvider {
    /// 复用的 HTTP 客户端。
    client: reqwest::Client,
}

impl OllamaProvider {
    /// 创建使用默认 `reqwest::Client` 的实例。
    pub fn new() -> Self {
        OllamaProvider {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for OllamaProvider {
    /// 等价于 [`OllamaProvider::new`]。
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatProvider for OllamaProvider {
    /// 调用 Ollama 流式聊天接口（自动选择原生或兼容协议）。
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<serde_json::Value>,
        config: &ProviderConfig,
    ) -> anyhow::Result<ChatStream> {
        ollama_chat_stream(&self.client, messages, tools, config).await
    }
}

#[async_trait]
impl VerifyProvider for OllamaProvider {
    /// 发送最小 `/api/chat` 请求探测连通性。
    async fn verify(&self, model: &str, config: &ProviderConfig) -> VerifyResult {
        verify::probe(&self.client, "ollama", model, config).await
    }
}

#[async_trait]
impl AiProvider for OllamaProvider {
    /// 供应商标识符：`ollama`。
    fn name(&self) -> &str {
        "ollama"
    }

    /// 默认模型：`llama3.3`。
    fn default_model(&self) -> &str {
        "llama3.3"
    }
}
