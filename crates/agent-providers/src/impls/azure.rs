//! Azure OpenAI v1 — Responses、Chat Completions 和 Images 共用 OpenAI v1 协议与 Bearer 认证。

use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::compat::{
    apply_thinking_compat, OpenAICompatible, OpenAICompletionModel, OpenAIResponsesCompatible,
    ThinkingFormat,
};
use crate::traits::{
    Capabilities, Capable, EmbeddingModel, FromClient, ImageGenModel, ModelBase, Nothing,
    ProviderClient, ProviderExt,
};
use crate::types::media::{Embedding, GeneratedImage, ImageGenConfig};

#[derive(Debug, Clone, Copy, Default)]
pub struct Azure;

impl ProviderExt for Azure {
    const NAME: &'static str = "azure";
    const BASE_URL: &'static str = "";

    fn auth_headers(&self, _key: &str) -> HeaderMap {
        // Azure OpenAI v1 follows the OpenAI SDK contract: an empty provider-specific
        // header map makes the shared clients add `Authorization: Bearer ...`.
        HeaderMap::new()
    }
}

impl OpenAICompatible for Azure {
    const STREAM_USAGE: bool = true;
    const THINKING_FORMAT: ThinkingFormat = ThinkingFormat::ReasoningEffort;
    const EFFORT_MAP: &'static [(&'static str, &'static str)] =
        &[("max", "high"), ("xhigh", "high")];

    fn finalize_body(&self, body: &mut Value) {
        apply_thinking_compat(
            Self::THINKING_FORMAT,
            <Self as OpenAICompatible>::EFFORT_MAP,
            body,
        );
    }
}

impl OpenAIResponsesCompatible for Azure {
    const STORE_FALSE: bool = true;
    const INCLUDE_ENCRYPTED_REASONING: bool = true;
    const EFFORT_MAP: &'static [(&'static str, &'static str)] =
        &[("max", "high"), ("xhigh", "high")];

    fn responses_base_url<'a>(&self, base: &'a str) -> std::borrow::Cow<'a, str> {
        std::borrow::Cow::Owned(azure_openai_v1_base(base))
    }
}

impl Capabilities for Azure {
    type Chat = Capable<OpenAICompletionModel<Self>>;
    type Embedding = Capable<AzureEmbeddingModel>;
    type ImageGen = Capable<AzureImageModel>;
    type VideoGen = Nothing;
    type TTS = Nothing;
    type MusicGen = Nothing;
}

/// Azure AI Foundry OpenAI v1 embedding model.
#[derive(Clone)]
pub struct AzureEmbeddingModel(ModelBase);

impl FromClient<Azure> for AzureEmbeddingModel {
    fn from_client(client: &ProviderClient<Azure>, model: &str) -> Self {
        Self(ModelBase::from_client(client, model))
    }
}

#[async_trait::async_trait]
impl EmbeddingModel for AzureEmbeddingModel {
    async fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Embedding>> {
        let mut cfg = self.0.to_provider_config();
        let endpoint = cfg
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| crate::profile::default_base_for("azure"));
        cfg.base_url = Some(azure_openai_v1_base(endpoint));
        let vectors = crate::openai::embeddings_http::openai_batch_embed(
            self.0.http(),
            texts,
            self.0.model(),
            &cfg,
        )
        .await?;
        Ok(vectors
            .into_iter()
            .map(|values| Embedding { values })
            .collect())
    }
}

/// Azure AI Foundry OpenAI v1 image generation model.
#[derive(Clone)]
pub struct AzureImageModel(ModelBase);

impl FromClient<Azure> for AzureImageModel {
    fn from_client(client: &ProviderClient<Azure>, model: &str) -> Self {
        Self(ModelBase::from_client(client, model))
    }
}

#[async_trait::async_trait]
impl ImageGenModel for AzureImageModel {
    async fn generate(
        &self,
        prompt: &str,
        config: &ImageGenConfig,
    ) -> anyhow::Result<Vec<GeneratedImage>> {
        let cfg = self.0.to_provider_config();
        crate::openai::image_http::azure_foundry_generate_image_with_config(
            self.0.http(),
            prompt,
            &cfg,
            config,
        )
        .await
    }
}

/// 将 Azure 资源根地址或 OpenAI v1 地址规范为统一基址。
///
/// 同时支持新的 `services.ai.azure.com` 和传统 `openai.azure.com`
/// 主机名；新配置应优先使用前者。
pub fn azure_openai_v1_base(endpoint: &str) -> String {
    let base = endpoint.trim().trim_end_matches('/');
    if base.ends_with("/openai/v1") || base.ends_with("/v1") {
        base.to_string()
    } else if base.ends_with("/openai") {
        format!("{base}/v1")
    } else {
        format!("{base}/openai/v1")
    }
}

#[cfg(test)]
mod tests {
    use super::azure_openai_v1_base;

    #[test]
    fn normalizes_both_azure_host_styles_to_openai_v1() {
        assert_eq!(
            azure_openai_v1_base("https://example.services.ai.azure.com/openai/v1/"),
            "https://example.services.ai.azure.com/openai/v1"
        );
        assert_eq!(
            azure_openai_v1_base("https://example.openai.azure.com"),
            "https://example.openai.azure.com/openai/v1"
        );
        assert_eq!(
            azure_openai_v1_base("https://example.openai.azure.com/openai"),
            "https://example.openai.azure.com/openai/v1"
        );
    }
}
