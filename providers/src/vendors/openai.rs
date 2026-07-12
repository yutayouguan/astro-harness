//! OpenAI 大模型供应商薄封装。
//!
//! 通过 OpenAI 兼容 `chat/completions` SSE 实现流式对话，
//! 并支持 Images API 出图与连通性探测。

use crate::http_stream::openai_compatible_chat_stream;
use crate::image_http::openai_generate_image;
use crate::trait_::*;
use crate::verify;
use async_trait::async_trait;

/// OpenAI API 的 [`AiProvider`] 实现。
pub struct OpenAiProvider {
    /// 复用的 HTTP 客户端。
    client: reqwest::Client,
}

impl OpenAiProvider {
    /// 创建使用默认 `reqwest::Client` 的实例。
    pub fn new() -> Self {
        OpenAiProvider {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for OpenAiProvider {
    /// 等价于 [`OpenAiProvider::new`]。
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatProvider for OpenAiProvider {
    /// 调用 OpenAI 兼容流式聊天接口。
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<serde_json::Value>,
        config: &ProviderConfig,
    ) -> anyhow::Result<ChatStream> {
        openai_compatible_chat_stream(&self.client, "openai", messages, tools, config).await
    }
}

#[async_trait]
impl ImageGenProvider for OpenAiProvider {
    /// 调用 OpenAI Images API 生成图片。
    async fn generate_image(
        &self,
        prompt: &str,
        config: &ProviderConfig,
    ) -> anyhow::Result<Vec<GeneratedImage>> {
        let mut cfg = config.clone();
        if cfg.model.trim().is_empty() {
            cfg.model = crate::image_gen::default_image_model("openai").to_string();
        }
        openai_generate_image(&self.client, prompt, &cfg).await
    }
}

#[async_trait]
impl VerifyProvider for OpenAiProvider {
    /// 发送最小 `chat/completions` 请求探测连通性。
    async fn verify(&self, model: &str, config: &ProviderConfig) -> VerifyResult {
        verify::probe(&self.client, "openai", model, config).await
    }
}

#[async_trait]
impl AiProvider for OpenAiProvider {
    /// 供应商标识符：`openai`。
    fn name(&self) -> &str {
        "openai"
    }

    /// 默认模型：`gpt-5.6`。
    fn default_model(&self) -> &str {
        "gpt-5.6"
    }

    /// 支持 embedding API。
    fn supports_embedding(&self) -> bool {
        true
    }

    /// 支持图片生成。
    fn supports_image_gen(&self) -> bool {
        true
    }

    /// 委托 [`ImageGenProvider::generate_image`] 实现出图。
    async fn generate_image(
        &self,
        prompt: &str,
        config: &ProviderConfig,
    ) -> anyhow::Result<Vec<GeneratedImage>> {
        ImageGenProvider::generate_image(self, prompt, config).await
    }
}
