//! Google Gemini 大模型供应商薄封装。
//!
//! 通过 Gemini `streamGenerateContent` SSE 实现流式对话，
//! 并支持 `generateContent` 原生出图。

use crate::http_stream::google_chat_stream;
use crate::image_http::google_generate_image;
use crate::trait_::*;
use crate::verify;
use async_trait::async_trait;

/// Google Gemini API 的 [`AiProvider`] 实现。
pub struct GoogleProvider {
    /// 复用的 HTTP 客户端。
    client: reqwest::Client,
}

impl GoogleProvider {
    /// 创建使用默认 `reqwest::Client` 的实例。
    pub fn new() -> Self {
        GoogleProvider {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for GoogleProvider {
    /// 等价于 [`GoogleProvider::new`]。
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatProvider for GoogleProvider {
    /// 调用 Gemini 流式 `generateContent` 接口。
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<serde_json::Value>,
        config: &ProviderConfig,
    ) -> anyhow::Result<ChatStream> {
        google_chat_stream(&self.client, messages, tools, config).await
    }
}

#[async_trait]
impl ImageGenProvider for GoogleProvider {
    /// 调用 Gemini 多模态出图接口。
    async fn generate_image(
        &self,
        prompt: &str,
        config: &ProviderConfig,
    ) -> anyhow::Result<Vec<GeneratedImage>> {
        let mut cfg = config.clone();
        if cfg.model.trim().is_empty() {
            cfg.model = crate::image_gen::default_image_model("google").to_string();
        }
        google_generate_image(&self.client, prompt, &cfg).await
    }
}

#[async_trait]
impl VerifyProvider for GoogleProvider {
    /// 发送最小 `generateContent` 请求探测连通性。
    async fn verify(&self, model: &str, config: &ProviderConfig) -> VerifyResult {
        verify::probe(&self.client, "google", model, config).await
    }
}

#[async_trait]
impl AiProvider for GoogleProvider {
    /// 供应商标识符：`google`。
    fn name(&self) -> &str {
        "google"
    }

    /// 默认模型：`gemini-3.5-flash`。
    fn default_model(&self) -> &str {
        "gemini-3.5-flash"
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
