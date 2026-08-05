//! OpenAI Responses API — CompletionModel 封装。
//!
//! 复用 `openai/responses.rs` 中的消息转换、工具转换和 SSE 解析，
//! 将 `CompletionRequest` 适配为 `responses_chat_stream` 调用。

use anyhow::Result;
use reqwest::Client as HttpClient;
use serde_json::json;

use crate::traits::{Capable, Capabilities, CompletionModel, FromClient, Nothing, ProviderClient, ProviderExt};
use crate::types::{CompletionRequest, CompletionStream};

// ─── Provider Extension ─────────────────────────────────

#[derive(Debug, Clone, Copy, Default)]
pub struct OpenAIResponses;

impl ProviderExt for OpenAIResponses {
    const NAME: &'static str = "openai-responses";
    const BASE_URL: &'static str = "https://api.openai.com/v1";

    fn auth_headers(&self, key: &str) -> reqwest::header::HeaderMap {
        crate::impls::openai::bearer_headers(key)
    }
}

impl Capabilities for OpenAIResponses {
    type Chat = Capable<ResponsesCompletionModel>;
    type Embedding = Nothing;
    type ImageGen = Nothing;
    type VideoGen = Nothing;
    type TTS = Nothing;
    type MusicGen = Nothing;
}

// ─── Completion Model ────────────────────────────────────

pub struct ResponsesCompletionModel {
    http: HttpClient,
    base_url: String,
    api_key: String,
    model: String,
    provider_id: &'static str,
}

impl Clone for ResponsesCompletionModel {
    fn clone(&self) -> Self {
        Self {
            http: self.http.clone(),
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
            model: self.model.clone(),
            provider_id: self.provider_id,
        }
    }
}

impl ResponsesCompletionModel {
    pub fn new(
        http: HttpClient,
        base_url: String,
        api_key: String,
        model: String,
        provider_id: &'static str,
    ) -> Self {
        Self { http, base_url, api_key, model, provider_id }
    }
}

impl FromClient<OpenAIResponses> for ResponsesCompletionModel {
    fn from_client(client: &ProviderClient<OpenAIResponses>, model: &str) -> Self {
        Self {
            http: client.http.clone(),
            base_url: client.base_url.clone(),
            api_key: client.api_key.clone(),
            model: model.to_string(),
            provider_id: "openai-responses",
        }
    }
}

#[async_trait::async_trait]
impl CompletionModel for ResponsesCompletionModel {
    async fn stream(&self, request: CompletionRequest) -> Result<CompletionStream> {
        let tools_json: Vec<serde_json::Value> = request
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                })
            })
            .collect();

        let thinking = request.thinking.as_ref();
        let config = crate::types::request::ProviderConfig {
            api_key: self.api_key.clone(),
            base_url: Some(self.base_url.clone()),
            model: if request.model.is_empty() {
                self.model.clone()
            } else {
                request.model.clone()
            },
            temperature: request.temperature.unwrap_or(-1.0),
            max_tokens: request.max_tokens.unwrap_or(0),
            thinking_enabled: thinking.map_or(false, |t| t.enabled),
            reasoning_effort: thinking.map_or_else(String::new, |t| t.effort.clone()),
            additional_params: request.additional_params.clone(),
            previous_interaction_id: None,
        };

        crate::openai::responses::responses_chat_stream(
            &self.http,
            self.provider_id,
            request.messages,
            tools_json,
            &config,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::client::ChatClient;

    #[test]
    fn openai_responses_has_chat() {
        let client = ProviderClient::new("test-key", OpenAIResponses);
        let _model = client.completion_model("gpt-5.6-sol");
    }
}
