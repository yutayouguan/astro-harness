//! 火山引擎 (Doubao) — OpenAI 兼容。

use reqwest::header::HeaderMap;
use crate::compat::{OpenAICompatible, OpenAICompletionModel};
use crate::compat::media::{CompatEmbeddingModel, CompatImageGenModel, CompatTTSModel};
use crate::traits::{Capable, Capabilities, Nothing, ProviderExt};

#[derive(Debug, Clone, Copy, Default)]
pub struct Volcengine;

impl ProviderExt for Volcengine {
    const NAME: &'static str = "volcengine";
    const BASE_URL: &'static str = "https://ark.cn-beijing.volces.com/api/v3";
    fn auth_headers(&self, key: &str) -> HeaderMap {
        crate::impls::openai::bearer_headers(key)
    }
}

impl OpenAICompatible for Volcengine {
    const STREAM_USAGE: bool = false;
}

impl Capabilities for Volcengine {
    type Chat = Capable<OpenAICompletionModel<Self>>;
    type Embedding = Capable<CompatEmbeddingModel>;
    type ImageGen = Capable<CompatImageGenModel>;
    type VideoGen = Nothing;
    type TTS = Capable<CompatTTSModel>;
    type MusicGen = Nothing;
}
