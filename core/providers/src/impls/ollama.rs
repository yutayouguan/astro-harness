//! Ollama — 本地 OpenAI 兼容，无需 API Key。

use reqwest::header::HeaderMap;
use crate::compat::{OpenAICompatible, OpenAICompletionModel};
use crate::traits::{Capable, Capabilities, Nothing, ProviderExt};

#[derive(Debug, Clone, Copy, Default)]
pub struct Ollama;

impl ProviderExt for Ollama {
    const NAME: &'static str = "ollama";
    const BASE_URL: &'static str = "http://localhost:11434/v1";
    fn auth_headers(&self, _key: &str) -> HeaderMap {
        HeaderMap::new()
    }
}

impl OpenAICompatible for Ollama {
    const STREAM_USAGE: bool = true;
}

impl Capabilities for Ollama {
    type Chat = Capable<OpenAICompletionModel<Self>>;
    type Embedding = Nothing;
    type ImageGen = Nothing;
    type VideoGen = Nothing;
    type TTS = Nothing;
    type MusicGen = Nothing;
}
