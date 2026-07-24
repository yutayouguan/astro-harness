//! 阿里百炼 (DashScope) — OpenAI 兼容。

use reqwest::header::HeaderMap;
use crate::compat::{OpenAICompatible, OpenAICompletionModel};
use crate::traits::{Capable, Capabilities, Nothing, ProviderExt};

#[derive(Debug, Clone, Copy)]
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
    type Embedding = Nothing;
    type ImageGen = Nothing;
    type VideoGen = Nothing;
    type TTS = Nothing;
    type MusicGen = Nothing;
    type ASR = Nothing;
}
