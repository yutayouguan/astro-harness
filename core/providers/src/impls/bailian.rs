//! 阿里百炼 (DashScope) — OpenAI 兼容。

use reqwest::header::HeaderMap;
use crate::compat::{OpenAICompatible, OpenAICompletionModel};
use crate::compat::media::{CompatEmbeddingModel, CompatImageGenModel, CompatTTSModel};
use crate::traits::{Capable, Capabilities, Nothing, ProviderExt};

#[derive(Debug, Clone, Copy, Default)]
pub struct Bailian;

impl ProviderExt for Bailian {
    const NAME: &'static str = "bailian";
    const BASE_URL: &'static str = "https://dashscope.aliyuncs.com/compatible-mode/v1";
    fn auth_headers(&self, key: &str) -> HeaderMap {
        crate::impls::openai::bearer_headers(key)
    }
}

impl OpenAICompatible for Bailian {
    const STREAM_USAGE: bool = false;
}

impl Capabilities for Bailian {
    type Chat = Capable<OpenAICompletionModel<Self>>;
    type Embedding = Capable<CompatEmbeddingModel>;
    type ImageGen = Capable<CompatImageGenModel>;
    type VideoGen = Nothing;
    type TTS = Capable<CompatTTSModel>;
    type MusicGen = Nothing;
}
