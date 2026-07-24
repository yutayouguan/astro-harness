//! 腾讯混元 — OpenAI 兼容。

use reqwest::header::HeaderMap;
use crate::compat::{OpenAICompatible, OpenAICompletionModel};
use crate::traits::{Capable, Capabilities, Nothing, ProviderExt};

#[derive(Debug, Clone, Copy)]
pub struct Hunyuan;

impl ProviderExt for Hunyuan {
    const NAME: &'static str = "hunyuan";
    const BASE_URL: &'static str = "https://api.hunyuan.cloud.tencent.com/v1";
    fn auth_headers(&self, key: &str) -> HeaderMap {
        crate::impls::openai::bearer_headers(key)
    }
}

impl OpenAICompatible for Hunyuan {
    const STREAM_USAGE: bool = false;
}

impl Capabilities for Hunyuan {
    type Chat = Capable<OpenAICompletionModel<Self>>;
    type Embedding = Nothing;
    type ImageGen = Nothing;
    type VideoGen = Nothing;
    type TTS = Nothing;
    type MusicGen = Nothing;
    type ASR = Nothing;
}
