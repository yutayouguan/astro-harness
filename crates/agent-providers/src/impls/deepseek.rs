//! DeepSeek — OpenAI 兼容 + thinking 参数。

use reqwest::header::HeaderMap;

use crate::compat::{
    OpenAICompatible, OpenAICompletionModel, OpenAIResponsesCompatible, ThinkingFormat,
};
use crate::traits::{Capabilities, Capable, Nothing, ProviderExt};

#[derive(Debug, Clone, Copy, Default)]
pub struct DeepSeek;

impl ProviderExt for DeepSeek {
    const NAME: &'static str = "deepseek";
    const BASE_URL: &'static str = "https://api.deepseek.com/v1";
    fn auth_headers(&self, key: &str) -> HeaderMap {
        crate::impls::openai::bearer_headers(key)
    }
}

impl OpenAICompatible for DeepSeek {
    const STREAM_USAGE: bool = true;
    const THINKING_FORMAT: ThinkingFormat = ThinkingFormat::DeepSeek;
    const EFFORT_MAP: &'static [(&'static str, &'static str)] = &[("max", "max"), ("xhigh", "max")];
}

impl OpenAIResponsesCompatible for DeepSeek {
    const EFFORT_MAP: &'static [(&'static str, &'static str)] = &[("max", "max"), ("xhigh", "max")];
}

impl Capabilities for DeepSeek {
    type Chat = Capable<OpenAICompletionModel<Self>>;
    type Embedding = Nothing;
    type ImageGen = Nothing;
    type VideoGen = Nothing;
    type TTS = Nothing;
    type MusicGen = Nothing;
}
