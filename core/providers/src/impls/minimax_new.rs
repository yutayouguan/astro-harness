//! MiniMax — OpenAI 兼容聊天 + 多媒体能力。

use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::compat::{OpenAICompatible, OpenAICompletionModel};
use crate::traits::{Capable, Capabilities, Nothing, ProviderExt};

#[derive(Debug, Clone, Copy)]
pub struct MiniMaxNew;

impl ProviderExt for MiniMaxNew {
    const NAME: &'static str = "minimax";
    const BASE_URL: &'static str = "https://api.minimaxi.com/v1";
    fn auth_headers(&self, key: &str) -> HeaderMap {
        crate::impls::openai::bearer_headers(key)
    }
}

impl OpenAICompatible for MiniMaxNew {
    const STREAM_USAGE: bool = true;

    fn finalize_body(&self, body: &mut Value) {
        // reasoning_split=true → thinking 通过 reasoning_content 字段返回（而非 <think> 标签）
        body["reasoning_split"] = Value::Bool(true);

        if let Some(tc) = body.get("thinking_config").cloned() {
            body.as_object_mut().unwrap().remove("thinking_config");
            let enabled = tc.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
            body["thinking"] = serde_json::json!({
                "type": if enabled { "adaptive" } else { "disabled" }
            });
        }
    }
}

impl Capabilities for MiniMaxNew {
    type Chat = Capable<OpenAICompletionModel<Self>>;
    type Embedding = Nothing; // TODO: Capable<OpenAIEmbeddingModel<Self>>
    type ImageGen = Nothing;  // TODO: Capable<MiniMaxImageModel>
    type VideoGen = Nothing;  // TODO: Capable<MiniMaxVideoModel>
    type TTS = Nothing;       // TODO: Capable<MiniMaxTTSModel>
    type MusicGen = Nothing;  // TODO: Capable<MiniMaxMusicModel>
    type ASR = Nothing;
}
