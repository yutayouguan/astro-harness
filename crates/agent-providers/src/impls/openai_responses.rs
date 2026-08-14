//! OpenAI Responses API — 原生 CompletionModel 实现。
//!
//! 直接从 `CompletionRequest` 构建 Responses API 请求体，
//! 复用 `openai/responses.rs` 中的消息转换和 SSE 解析。

use anyhow::{anyhow, Context, Result};
use reqwest::Client as HttpClient;
use serde_json::{json, Value};

use crate::traits::{
    Capabilities, Capable, CompletionModel, FromClient, Nothing, ProviderClient, ProviderExt,
};
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
        Self {
            http,
            base_url,
            api_key,
            model,
            provider_id,
        }
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
        if self.api_key.trim().is_empty() {
            return Err(anyhow!("Responses API Key 为空"));
        }

        let base = crate::compat::openai_compatible_base(&self.base_url);
        let url = format!("{base}/responses");
        let is_openai = self.provider_id.starts_with("openai");
        let model = if request.model.is_empty() {
            &self.model
        } else {
            &request.model
        };

        let input = crate::openai::responses::to_responses_input(&request.messages);

        let instructions = request.messages.iter().find_map(|m| match m {
            crate::types::message::Message::System { content } => Some(content.clone()),
            _ => None,
        });

        let mut body = json!({
            "model": model,
            "input": input,
            "stream": true,
        });
        if is_openai {
            body["store"] = json!(false);
        }
        if let Some(inst) = instructions {
            if !inst.is_empty() {
                body["instructions"] = json!(inst);
            }
        }
        let has_reasoning = request.thinking.as_ref().is_some_and(|tc| tc.enabled);
        if !has_reasoning {
            if let Some(temp) = request.temperature {
                body["temperature"] = json!(temp);
            }
        }
        if let Some(max) = request.max_tokens {
            if max > 0 {
                body["max_output_tokens"] = json!(max);
            }
        }

        // Tools — 直接从 ToolDefinition 构建 Responses 格式
        if !request.tools.is_empty() {
            let tools: Vec<Value> = request
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    })
                })
                .collect();
            body["tools"] = Value::Array(tools);
            body["tool_choice"] = json!("auto");
            if is_openai {
                body["parallel_tool_calls"] = json!(true);
            }
        }

        // Reasoning / Thinking
        if let Some(ref tc) = request.thinking {
            if tc.enabled {
                let effort = match tc.effort.trim() {
                    "" | "high" => "high",
                    other => other,
                };
                if is_openai {
                    body["reasoning"] = json!({"effort": effort, "summary": "auto"});
                } else {
                    body["reasoning"] = json!({"effort": effort});
                }
            }
        }

        // additional_params
        if let Some(extra) = request.additional_params.as_object() {
            if let Some(obj) = body.as_object_mut() {
                for (k, v) in extra {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }

        let response = self
            .http
            .post(&url)
            .bearer_auth(self.api_key.trim())
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .with_context(|| format!("连接 Responses API 失败: {url}"))?;

        crate::shared::sse::sse_stream(
            response,
            std::sync::Arc::new(crate::openai::responses::extract_responses_chunks),
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
