//! 供应商门面 trait — 动态分发接口。

use async_trait::async_trait;

use crate::profile::AuthKind;
use crate::shared::verify::VerifyResult;
use crate::types::media::{GeneratedAudio, GeneratedImage, GeneratedVideo};
use crate::types::message::Message;
use crate::types::request::ProviderConfig;
use crate::types::stream::CompletionStream;

/// 流式聊天能力。
#[async_trait]
pub trait ChatProvider: Send + Sync {
    async fn chat_stream(
        &self,
        messages: Vec<Message>,
        tools: Vec<serde_json::Value>,
        config: &ProviderConfig,
    ) -> anyhow::Result<CompletionStream>;
}

/// 连通性探测能力。
#[async_trait]
pub trait VerifyProvider: Send + Sync {
    async fn verify(&self, model: &str, config: &ProviderConfig) -> VerifyResult;
}

/// 门面 trait：Chat + Verify + 媒体能力。
#[async_trait]
pub trait AiProvider: ChatProvider + VerifyProvider + Send + Sync {
    fn name(&self) -> &str;

    fn supports_tools(&self) -> bool {
        true
    }

    fn supports_image_gen(&self) -> bool {
        false
    }

    fn supports_embedding(&self) -> bool {
        false
    }

    fn default_model(&self) -> &str;

    fn auth_kind(&self) -> AuthKind {
        AuthKind::for_provider(self.name())
    }

    async fn generate_image(
        &self,
        _prompt: &str,
        _config: &ProviderConfig,
    ) -> anyhow::Result<Vec<GeneratedImage>> {
        anyhow::bail!("{} 不支持图片生成", self.name())
    }

    async fn text_to_speech(
        &self,
        text: &str,
        _config: &ProviderConfig,
    ) -> anyhow::Result<GeneratedAudio> {
        let _ = text;
        anyhow::bail!("{} 不支持语音合成 (TTS)", self.name())
    }

    async fn generate_video(
        &self,
        prompt: &str,
        _config: &ProviderConfig,
    ) -> anyhow::Result<GeneratedVideo> {
        let _ = prompt;
        anyhow::bail!("{} 不支持视频生成", self.name())
    }

    async fn generate_music(
        &self,
        prompt: &str,
        _config: &ProviderConfig,
    ) -> anyhow::Result<GeneratedAudio> {
        let _ = prompt;
        anyhow::bail!("{} 不支持音乐生成", self.name())
    }

    async fn embed(
        &self,
        texts: &[String],
        _config: &ProviderConfig,
    ) -> anyhow::Result<Vec<Vec<f32>>> {
        let _ = texts;
        anyhow::bail!("{} 不支持文本嵌入", self.name())
    }
}
