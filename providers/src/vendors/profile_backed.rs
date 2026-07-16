//! 表驱动的统一 [`AiProvider`]：按 [`ProviderProfile`] 分发协议。

use crate::http_stream::chat_stream_for_provider;
use crate::image_http::openai_generate_image;
use crate::interactions_http::{google_interactions_image, InteractionImageRequest};
use crate::profile::{resolve, ProviderProfile};
use crate::trait_::*;
use crate::verify;
use async_trait::async_trait;

/// 由静态 [`ProviderProfile`] 驱动的供应商实现。
pub struct ProfileBackedProvider {
    client: reqwest::Client,
    profile: &'static ProviderProfile,
}

impl ProfileBackedProvider {
    /// 使用给定 profile 构造。
    pub fn new(profile: &'static ProviderProfile) -> Self {
        Self {
            client: reqwest::Client::new(),
            profile,
        }
    }

    /// 按 id 查找 profile；未知 id 返回 `None`。
    pub fn try_from_id(id: &str) -> Option<Self> {
        resolve(id).map(Self::new)
    }
}

#[async_trait]
impl ChatProvider for ProfileBackedProvider {
    async fn chat_stream(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<serde_json::Value>,
        config: &ProviderConfig,
    ) -> anyhow::Result<ChatStream> {
        chat_stream_for_provider(&self.client, self.profile.id, messages, tools, config).await
    }
}

#[async_trait]
impl VerifyProvider for ProfileBackedProvider {
    async fn verify(&self, model: &str, config: &ProviderConfig) -> VerifyResult {
        verify::probe(&self.client, self.profile.id, model, config).await
    }
}

#[async_trait]
impl AiProvider for ProfileBackedProvider {
    fn name(&self) -> &str {
        self.profile.id
    }

    fn default_model(&self) -> &str {
        self.profile.default_model
    }

    fn supports_image_gen(&self) -> bool {
        self.profile.supports_image_gen
    }

    fn supports_embedding(&self) -> bool {
        self.profile.supports_embedding
    }

    fn auth_kind(&self) -> AuthKind {
        self.profile.auth
    }

    async fn generate_image(
        &self,
        prompt: &str,
        config: &ProviderConfig,
    ) -> anyhow::Result<Vec<GeneratedImage>> {
        if !self.profile.supports_image_gen {
            anyhow::bail!("{} 不支持图片生成", self.profile.id);
        }
        let mut cfg = config.clone();
        if cfg.model.trim().is_empty() {
            cfg.model = crate::image_gen::default_image_model(self.profile.id).to_string();
        }
        match self.profile.id {
            "openai" => openai_generate_image(&self.client, prompt, &cfg).await,
            "google" => {
                let request = InteractionImageRequest {
                    prompt: prompt.to_string(),
                    ..Default::default()
                };
                let result = google_interactions_image(&self.client, &cfg, &request).await?;
                Ok(vec![result.image])
            }
            other => anyhow::bail!("{other} 不支持图片生成"),
        }
    }
}
