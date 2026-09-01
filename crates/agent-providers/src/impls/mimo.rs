//! 小米 Mimo — OpenAI 兼容 + Responses API + reasoning。

use reqwest::header::HeaderMap;

use crate::compat::{
    OpenAICompatible, OpenAICompletionModel, OpenAIResponsesCompatible, ThinkingFormat,
};
use crate::traits::{Capabilities, Capable, Nothing, ProviderExt};

#[derive(Debug, Clone, Copy, Default)]
pub struct Mimo;

impl ProviderExt for Mimo {
    const NAME: &'static str = "mimo";
    const BASE_URL: &'static str = "https://api.xiaomimimo.com/v1";
    fn auth_headers(&self, key: &str) -> HeaderMap {
        crate::impls::openai::bearer_headers(key)
    }
}

impl OpenAICompatible for Mimo {
    const STREAM_USAGE: bool = true;
    const THINKING_FORMAT: ThinkingFormat = ThinkingFormat::ReasoningEffort;
    const EFFORT_MAP: &'static [(&'static str, &'static str)] =
        &[("max", "high"), ("xhigh", "high")];
}

impl OpenAIResponsesCompatible for Mimo {
    const EFFORT_MAP: &'static [(&'static str, &'static str)] =
        &[("max", "high"), ("xhigh", "high")];
}

impl Capabilities for Mimo {
    type Chat = Capable<OpenAICompletionModel<Self>>;
    type Embedding = Nothing;
    type ImageGen = Nothing;
    type VideoGen = Nothing;
    type TTS = Nothing;
    type MusicGen = Nothing;
}
