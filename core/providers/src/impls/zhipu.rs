//! 智谱 GLM — OpenAI 兼容。

use reqwest::header::HeaderMap;
use crate::compat::{OpenAICompatible, OpenAICompletionModel};
use crate::traits::{Capable, Capabilities, Nothing, ProviderExt};

#[derive(Debug, Clone, Copy, Default)]
pub struct Zhipu;

impl ProviderExt for Zhipu {
    const NAME: &'static str = "zhipu";
    const BASE_URL: &'static str = "https://open.bigmodel.cn/api/paas/v4";
    fn auth_headers(&self, key: &str) -> HeaderMap {
        crate::impls::openai::bearer_headers(key)
    }
}

impl OpenAICompatible for Zhipu {
    const STREAM_USAGE: bool = false;
}

impl Capabilities for Zhipu {
    type Chat = Capable<OpenAICompletionModel<Self>>;
    type Embedding = Nothing;
    type ImageGen = Nothing;
    type VideoGen = Nothing;
    type TTS = Nothing;
    type MusicGen = Nothing;
    type ASR = Nothing;
}
