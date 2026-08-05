//! DeepSeek — OpenAI 兼容 + thinking 参数。

use reqwest::header::HeaderMap;
use serde_json::Value;

use crate::compat::OpenAICompatible;
use crate::traits::{Capable, Capabilities, Nothing, ProviderExt};
use crate::compat::OpenAICompletionModel;

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

    fn finalize_body(&self, body: &mut Value) {
        if let Some(tc) = body.get("thinking_config").cloned() {
            body.as_object_mut().unwrap().remove("thinking_config");
            let enabled = tc.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
            body["thinking"] = serde_json::json!({
                "type": if enabled { "enabled" } else { "disabled" }
            });
            if enabled {
                let effort = tc
                    .get("effort")
                    .and_then(|v| v.as_str())
                    .unwrap_or("high");
                // DeepSeek API 有效值：high / max（低于 high 的级别会被服务端映射为 high）
                let mapped = match effort {
                    "max" | "xhigh" => "max",
                    _ => "high",
                };
                body["reasoning_effort"] = serde_json::json!(mapped);
            }
        }
    }
}

impl Capabilities for DeepSeek {
    type Chat = Capable<OpenAICompletionModel<Self>>;
    type Embedding = Nothing;
    type ImageGen = Nothing;
    type VideoGen = Nothing;
    type TTS = Nothing;
    type MusicGen = Nothing;
}
