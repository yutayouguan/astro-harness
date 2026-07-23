//! OpenAI — 基础 OpenAI 兼容 + Bearer auth 辅助。

use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};

use crate::compat::{OpenAICompatible, OpenAICompletionModel};
use crate::traits::{Capable, Capabilities, Nothing, ProviderExt};

/// Bearer token 认证 header（OpenAI 及大多数兼容厂商共用）。
pub fn bearer_headers(api_key: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if !api_key.is_empty() {
        if let Ok(val) = HeaderValue::from_str(&format!("Bearer {api_key}")) {
            headers.insert(AUTHORIZATION, val);
        }
    }
    headers
}

#[derive(Debug, Clone, Copy)]
pub struct OpenAI;

impl ProviderExt for OpenAI {
    const NAME: &'static str = "openai";
    const BASE_URL: &'static str = "https://api.openai.com/v1";
    fn auth_headers(&self, key: &str) -> HeaderMap {
        bearer_headers(key)
    }
}

impl OpenAICompatible for OpenAI {
    const STREAM_USAGE: bool = true;
}

impl Capabilities for OpenAI {
    type Chat = Capable<OpenAICompletionModel<Self>>;
    type Embedding = Nothing; // TODO: Phase 3 — Capable<OpenAIEmbeddingModel>
    type ImageGen = Nothing;  // TODO: Phase 3 — Capable<OpenAIImageModel>
    type VideoGen = Nothing;
    type TTS = Nothing;       // TODO: Phase 3 — Capable<OpenAITTSModel>
    type MusicGen = Nothing;
    type ASR = Nothing;
}
