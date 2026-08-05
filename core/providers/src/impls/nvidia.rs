//! NVIDIA — OpenAI 兼容。

use reqwest::header::HeaderMap;
use crate::compat::{OpenAICompatible, OpenAICompletionModel};
use crate::traits::{Capable, Capabilities, Nothing, ProviderExt};

#[derive(Debug, Clone, Copy, Default)]
pub struct Nvidia;

impl ProviderExt for Nvidia {
    const NAME: &'static str = "nvidia";
    const BASE_URL: &'static str = "https://integrate.api.nvidia.com/v1";
    fn auth_headers(&self, key: &str) -> HeaderMap {
        crate::impls::openai::bearer_headers(key)
    }
}

impl OpenAICompatible for Nvidia {
    const STREAM_USAGE: bool = true;
}

impl Capabilities for Nvidia {
    type Chat = Capable<OpenAICompletionModel<Self>>;
    type Embedding = Nothing;
    type ImageGen = Nothing;
    type VideoGen = Nothing;
    type TTS = Nothing;
    type MusicGen = Nothing;
}
