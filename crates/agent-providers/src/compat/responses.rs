//! OpenAI Responses API 兼容补全模型。
//!
//! `OpenAIResponsesModel<Ext>` 直接消费 Agent 的原生 Responses prompt，
//! 通过独立的 `OpenAIResponsesCompatible` trait 处理线路差异。

use std::marker::PhantomData;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use reqwest::Client as HttpClient;
use serde_json::{json, Value};

use crate::traits::{FromClient, ProviderClient, ProviderExt, ResponsesModel};
use crate::types::{CompletionStream, ResponsesRequest};

/// OpenAI 风格 Responses 线路的厂商差异。
///
/// 该契约与 Chat Completions 兼容 trait 分离，Agent 模型不依赖旧消息协议。
pub trait OpenAIResponsesCompatible: ProviderExt {
    const STORE_FALSE: bool = false;
    const PARALLEL_TOOLS: bool = false;
    const REASONING_SUMMARY: bool = false;
    const EFFORT_MAP: &'static [(&'static str, &'static str)] = &[];

    fn finalize_responses_body(&self, _body: &mut Value) {}

    fn responses_base_url<'a>(&self, base: &'a str) -> std::borrow::Cow<'a, str> {
        std::borrow::Cow::Borrowed(base)
    }
}

/// 泛型 OpenAI Responses API 补全模型。
///
/// `Ext` 为厂商扩展类型，通过独立 Responses trait 的 hook 处理厂商差异。
pub struct OpenAIResponsesModel<Ext> {
    http: HttpClient,
    base_url: String,
    api_key: String,
    model: String,
    _ext: PhantomData<Ext>,
    ext_instance: Ext,
}

impl<Ext: Clone> Clone for OpenAIResponsesModel<Ext> {
    fn clone(&self) -> Self {
        Self {
            http: self.http.clone(),
            base_url: self.base_url.clone(),
            api_key: self.api_key.clone(),
            model: self.model.clone(),
            _ext: PhantomData,
            ext_instance: self.ext_instance.clone(),
        }
    }
}

impl<Ext: ProviderExt> FromClient<Ext> for OpenAIResponsesModel<Ext> {
    fn from_client(client: &ProviderClient<Ext>, model: &str) -> Self {
        Self {
            http: client.http.clone(),
            base_url: client.base_url.clone(),
            api_key: client.api_key.clone(),
            model: model.to_string(),
            _ext: PhantomData,
            ext_instance: client.ext.clone(),
        }
    }
}

#[async_trait::async_trait]
impl<Ext> ResponsesModel for OpenAIResponsesModel<Ext>
where
    Ext: OpenAIResponsesCompatible + Clone + Send + Sync + 'static,
{
    async fn stream(&self, request: ResponsesRequest) -> Result<CompletionStream> {
        if self.api_key.trim().is_empty() {
            return Err(anyhow!("Responses API Key 为空"));
        }

        let base = crate::compat::openai_compatible_base(&self.base_url);
        let url = format!("{base}/responses");
        let model = if request.model.is_empty() {
            &self.model
        } else {
            &request.model
        };

        let input = crate::openai::responses::to_native_responses_input(&request.input)?;

        let mut body = json!({
            "model": model,
            "input": input,
            "stream": true,
        });
        if Ext::STORE_FALSE {
            body["store"] = json!(false);
        }
        if !request.instructions.is_empty() {
            body["instructions"] = json!(&request.instructions);
        }
        if let Some(temp) = request.temperature {
            let has_reasoning = request.thinking.as_ref().is_some_and(|tc| tc.enabled);
            let is_default = (temp - 0.7).abs() < f32::EPSILON || temp == 1.0;
            if !has_reasoning && !is_default {
                body["temperature"] = json!(temp);
            }
        }
        if let Some(max) = request.max_tokens {
            if max > 0 {
                body["max_output_tokens"] = json!(max);
            }
        }

        // 工具定义
        if !request.tools.is_empty() {
            let tools: Vec<Value> = request
                .tools
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<_, _>>()?;
            body["tools"] = Value::Array(tools);
            body["tool_choice"] = json!("auto");
            if Ext::PARALLEL_TOOLS {
                body["parallel_tool_calls"] = json!(true);
            }
        }

        // Reasoning / Thinking — 经过 EFFORT_MAP 映射
        if let Some(ref tc) = request.thinking {
            if tc.enabled {
                let raw = tc.effort.trim();
                let effort = Ext::EFFORT_MAP
                    .iter()
                    .find(|(k, _)| *k == raw)
                    .map(|(_, v)| *v)
                    .unwrap_or(if raw.is_empty() { "high" } else { raw });
                if Ext::REASONING_SUMMARY {
                    body["reasoning"] = json!({"effort": effort, "summary": "auto"});
                } else {
                    body["reasoning"] = json!({"effort": effort});
                }
            }
        }

        // 厂商 hook
        self.ext_instance.finalize_responses_body(&mut body);

        // 额外参数合并
        if let Some(extra) = request.additional_params.as_object() {
            if let Some(obj) = body.as_object_mut() {
                for (k, v) in extra {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }
        crate::shared::tool_policy::apply_openai_responses(
            &mut body,
            request.tool_choice.as_ref(),
            request.parallel_tool_calls,
        );

        let auth = self.ext_instance.auth_headers(&self.api_key);
        let has_auth = !auth.is_empty();
        let mut req_builder = self
            .http
            .post(&url)
            .headers(auth)
            .header("content-type", "application/json")
            .json(&body);

        if !self.api_key.is_empty() && !has_auth {
            req_builder = req_builder.bearer_auth(self.api_key.trim());
        }

        let response = req_builder
            .send()
            .await
            .with_context(|| format!("连接 {} Responses API 失败: {url}", Ext::NAME))?;

        crate::shared::sse::sse_stream_with_terminal(
            response,
            Arc::new(crate::openai::responses::extract_responses_chunks),
            true,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Debug, Clone, Copy, Default)]
    struct FakeResponses;

    impl ProviderExt for FakeResponses {
        const NAME: &'static str = "fake-responses";
        const BASE_URL: &'static str = "http://localhost:9999";
        fn auth_headers(&self, _key: &str) -> reqwest::header::HeaderMap {
            reqwest::header::HeaderMap::new()
        }
    }

    impl OpenAIResponsesCompatible for FakeResponses {}

    #[test]
    fn responses_model_from_client() {
        use crate::traits::FromClient;
        let client = ProviderClient::new("test-key", FakeResponses);
        let _model = OpenAIResponsesModel::from_client(&client, "gpt-5.6");
    }
}
