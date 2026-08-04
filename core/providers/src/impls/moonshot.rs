//! 月之暗面 Kimi — OpenAI 兼容。

use reqwest::header::HeaderMap;
use crate::compat::{OpenAICompatible, OpenAICompletionModel};
use crate::traits::{Capable, Capabilities, Nothing, ProviderExt};

#[derive(Debug, Clone, Copy, Default)]
pub struct Moonshot;

impl ProviderExt for Moonshot {
    const NAME: &'static str = "moonshot";
    const BASE_URL: &'static str = "https://api.moonshot.cn/v1";
    fn auth_headers(&self, key: &str) -> HeaderMap {
        crate::impls::openai::bearer_headers(key)
    }
}

impl OpenAICompatible for Moonshot {
    const STREAM_USAGE: bool = true;
}

impl Capabilities for Moonshot {
    type Chat = Capable<OpenAICompletionModel<Self>>;
    type Embedding = Nothing;
    type ImageGen = Nothing;
    type VideoGen = Nothing;
    type TTS = Nothing;
    type MusicGen = Nothing;
    type ASR = Nothing;
}
