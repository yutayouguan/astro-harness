//! OpenRouter — OpenAI 兼容网关。

use reqwest::header::HeaderMap;
use crate::compat::{OpenAICompatible, OpenAICompletionModel};
use crate::traits::{Capable, Capabilities, Nothing, ProviderExt};

#[derive(Debug, Clone, Copy, Default)]
pub struct OpenRouter;

impl ProviderExt for OpenRouter {
    const NAME: &'static str = "openrouter";
    const BASE_URL: &'static str = "https://openrouter.ai/api/v1";
    fn auth_headers(&self, key: &str) -> HeaderMap {
        crate::impls::openai::bearer_headers(key)
    }
}

impl OpenAICompatible for OpenRouter {
    const STREAM_USAGE: bool = true;
}

impl Capabilities for OpenRouter {
    type Chat = Capable<OpenAICompletionModel<Self>>;
    type Embedding = Nothing;
    type ImageGen = Nothing;
    type VideoGen = Nothing;
    type TTS = Nothing;
    type MusicGen = Nothing;
}
