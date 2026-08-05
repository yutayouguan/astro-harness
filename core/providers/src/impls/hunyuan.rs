//! 腾讯混元 — OpenAI 兼容。

use reqwest::header::HeaderMap;
use crate::compat::{OpenAICompatible, OpenAICompletionModel};
use crate::compat::media::{CompatEmbeddingModel, CompatImageGenModel, CompatTTSModel};
use crate::traits::{Capable, Capabilities, Nothing, ProviderExt};

#[derive(Debug, Clone, Copy, Default)]
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
    type Embedding = Capable<CompatEmbeddingModel>;
    type ImageGen = Capable<CompatImageGenModel>;
    type VideoGen = Nothing;
    type TTS = Capable<CompatTTSModel>;
    type MusicGen = Nothing;
}
