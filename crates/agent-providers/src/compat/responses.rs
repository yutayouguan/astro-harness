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
use crate::types::{CompletionStream, PromptCacheMode, ResponsesRequest};

/// OpenAI 风格 Responses 线路的厂商差异。
///
/// 该契约与 Chat Completions 兼容 trait 分离，Agent 模型不依赖旧消息协议。
pub trait OpenAIResponsesCompatible: ProviderExt {
    const STORE_FALSE: bool = false;
    const INCLUDE_ENCRYPTED_REASONING: bool = false;
    const PARALLEL_TOOLS: bool = false;
    const REASONING_SUMMARY: bool = false;
    const EFFORT_MAP: &'static [(&'static str, &'static str)] = &[];
    const SUPPORTS_PERSISTENT_REASONING: bool = false;

    fn finalize_responses_body(&self, _body: &mut Value) {}

    fn responses_base_url<'a>(&self, base: &'a str) -> std::borrow::Cow<'a, str> {
        std::borrow::Cow::Borrowed(base)
    }
}

fn build_responses_body<Ext: OpenAIResponsesCompatible>(
    ext: &Ext,
    request: &ResponsesRequest,
    model: &str,
) -> Result<Value> {
    let input = crate::openai::responses::to_native_responses_input(&request.input)?;

    let mut additional_params = request.additional_params.clone();
    let persistent_instructions = additional_params
        .as_object_mut()
        .and_then(|params| params.remove("astro_persistent_instructions"))
        .and_then(|value| value.as_str().map(str::trim).map(str::to_string))
        .filter(|value| !value.is_empty());
    let persistent_requested = request
        .thinking
        .as_ref()
        .is_some_and(|thinking| thinking.enabled && thinking.effort.trim() == "persistent");
    if persistent_requested && !Ext::SUPPORTS_PERSISTENT_REASONING {
        return Err(anyhow!(
            "{} does not support persistent reasoning",
            Ext::NAME
        ));
    }
    let instructions = if persistent_requested {
        let persistent = persistent_instructions
            .ok_or_else(|| anyhow!("persistent reasoning requires persistent instructions"))?;
        if request.instructions.trim().is_empty() {
            persistent
        } else {
            format!("{}\n\n{}", request.instructions.trim(), persistent)
        }
    } else {
        request.instructions.clone()
    };

    let mut body = json!({
        "model": model,
        "input": input,
        "stream": true,
    });
    if !instructions.is_empty() {
        body["instructions"] = json!(instructions);
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

    if let Some(ref tc) = request.thinking {
        if tc.enabled {
            let raw = tc.effort.trim();
            let effort = if raw == "persistent" {
                "disabled"
            } else {
                Ext::EFFORT_MAP
                    .iter()
                    .find(|(k, _)| *k == raw)
                    .map(|(_, v)| *v)
                    .unwrap_or(if raw.is_empty() { "high" } else { raw })
            };
            if Ext::REASONING_SUMMARY {
                body["reasoning"] = json!({"effort": effort, "summary": "auto"});
            } else {
                body["reasoning"] = json!({"effort": effort});
            }
        }
    }

    ext.finalize_responses_body(&mut body);
    crate::shared::http::merge_additional_params(&mut body, &additional_params);

    if let Some(cache) = &request.prompt_cache {
        if let Some(key) = cache
            .key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
        {
            body["prompt_cache_key"] = json!(key);
        }
        if cache.mode.is_some() || cache.ttl_30m {
            let mut options = serde_json::Map::new();
            if let Some(mode) = cache.mode {
                options.insert(
                    "mode".into(),
                    json!(match mode {
                        PromptCacheMode::Implicit => "implicit",
                        PromptCacheMode::Explicit => "explicit",
                    }),
                );
            }
            if cache.ttl_30m {
                options.insert("ttl".into(), json!("30m"));
            }
            body["prompt_cache_options"] = Value::Object(options);
        }
    }

    crate::shared::tool_policy::apply_openai_responses(
        &mut body,
        request.tool_choice.as_ref(),
        request.parallel_tool_calls,
    );

    // Provider-mandated state policy is applied last so passthrough parameters cannot
    // accidentally re-enable server-side retention.
    if Ext::STORE_FALSE {
        body["store"] = json!(false);
    }
    if Ext::INCLUDE_ENCRYPTED_REASONING
        && request
            .thinking
            .as_ref()
            .is_some_and(|thinking| thinking.enabled)
    {
        let include = body
            .as_object_mut()
            .expect("Responses request body is always an object")
            .entry("include")
            .or_insert_with(|| Value::Array(Vec::new()));
        if !include.is_array() {
            *include = Value::Array(Vec::new());
        }
        let values = include.as_array_mut().expect("include normalized to array");
        if !values
            .iter()
            .any(|value| value.as_str() == Some("reasoning.encrypted_content"))
        {
            values.push(json!("reasoning.encrypted_content"));
        }
    }

    Ok(body)
}

fn build_responses_http_request<Ext: OpenAIResponsesCompatible>(
    http: &HttpClient,
    ext: &Ext,
    base_url: &str,
    api_key: &str,
    request: &ResponsesRequest,
    default_model: &str,
) -> Result<reqwest::Request> {
    let base = crate::compat::openai_compatible_base(base_url);
    let url = format!("{base}/responses");
    let model = if request.model.is_empty() {
        default_model
    } else {
        &request.model
    };
    let body = build_responses_body::<Ext>(ext, request, model)?;
    let auth = ext.auth_headers(api_key);
    let has_auth = !auth.is_empty();
    let mut builder = http
        .post(url)
        .headers(auth)
        .header("content-type", "application/json")
        .json(&body);
    if !api_key.is_empty() && !has_auth {
        builder = builder.bearer_auth(api_key.trim());
    }
    builder.build().context("build Responses HTTP request")
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

        let request = build_responses_http_request::<Ext>(
            &self.http,
            &self.ext_instance,
            &self.base_url,
            &self.api_key,
            &request,
            &self.model,
        )?;
        let url = request.url().to_string();
        let response = self
            .http
            .execute(request)
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
    use crate::types::{PromptCacheConfig, PromptCacheMode, ThinkingConfig};
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

    #[derive(Debug, Clone, Copy, Default)]
    struct PersistentResponses;

    impl ProviderExt for PersistentResponses {
        const NAME: &'static str = "openai";
        const BASE_URL: &'static str = "http://localhost:9999";
        fn auth_headers(&self, _key: &str) -> reqwest::header::HeaderMap {
            reqwest::header::HeaderMap::new()
        }
    }

    impl OpenAIResponsesCompatible for PersistentResponses {
        const SUPPORTS_PERSISTENT_REASONING: bool = true;
    }

    #[test]
    fn persistent_reasoning_maps_wire_effort_and_consumes_internal_instructions() {
        let request = ResponsesRequest {
            instructions: "base".into(),
            thinking: Some(ThinkingConfig {
                enabled: true,
                budget_tokens: None,
                effort: "persistent".into(),
            }),
            additional_params: json!({
                "astro_persistent_instructions": "keep working across turns",
                "top_p": 0.8
            }),
            ..ResponsesRequest::default()
        };

        let body = build_responses_body(&PersistentResponses, &request, "gpt-test")
            .expect("persistent request body");

        assert_eq!(body["reasoning"]["effort"], "disabled");
        assert_eq!(body["instructions"], "base\n\nkeep working across turns");
        assert_eq!(body["top_p"], 0.8);
        assert!(body.get("astro_persistent_instructions").is_none());
    }

    #[test]
    fn persistent_reasoning_requires_provider_capability_and_instructions() {
        let request = ResponsesRequest {
            thinking: Some(ThinkingConfig {
                enabled: true,
                budget_tokens: None,
                effort: "persistent".into(),
            }),
            additional_params: json!({"astro_persistent_instructions": "persist"}),
            ..ResponsesRequest::default()
        };
        assert!(build_responses_body(&FakeResponses, &request, "gpt-test").is_err());

        let missing = ResponsesRequest {
            thinking: request.thinking.clone(),
            ..ResponsesRequest::default()
        };
        assert!(build_responses_body(&PersistentResponses, &missing, "gpt-test").is_err());
    }

    #[test]
    fn inactive_persistent_reasoning_instructions_do_not_leak_to_the_wire() {
        let request = ResponsesRequest {
            instructions: "base".into(),
            thinking: Some(ThinkingConfig {
                enabled: true,
                budget_tokens: None,
                effort: "high".into(),
            }),
            additional_params: json!({
                "astro_persistent_instructions": "keep working across turns"
            }),
            ..ResponsesRequest::default()
        };

        let body = build_responses_body(&PersistentResponses, &request, "gpt-test")
            .expect("normal request body");

        assert_eq!(body["reasoning"]["effort"], "high");
        assert_eq!(body["instructions"], "base");
        assert!(body.get("astro_persistent_instructions").is_none());
    }

    #[test]
    fn responses_model_from_client() {
        use crate::traits::FromClient;
        let client = ProviderClient::new("test-key", FakeResponses);
        let _model = OpenAIResponsesModel::from_client(&client, "gpt-5.6");
    }

    #[test]
    fn azure_request_contract_uses_bearer_and_stateless_reasoning() {
        let request = ResponsesRequest {
            model: "gpt-5.6-deployment".into(),
            instructions: "be concise".into(),
            input: vec![agent_protocol::ResponseItem::Message {
                id: None,
                role: "user".into(),
                content: vec![agent_protocol::ContentItem::InputText {
                    text: "hello".into(),
                }],
                phase: None,
                internal_chat_message_metadata_passthrough: None,
            }],
            thinking: Some(ThinkingConfig {
                enabled: true,
                budget_tokens: None,
                effort: "high".into(),
            }),
            prompt_cache: Some(PromptCacheConfig {
                key: Some("agent:workspace:v1".into()),
                mode: Some(PromptCacheMode::Implicit),
                ttl_30m: true,
            }),
            additional_params: json!({"store": true}),
            ..ResponsesRequest::default()
        };
        let http = HttpClient::new();
        let built = build_responses_http_request::<crate::impls::azure::Azure>(
            &http,
            &crate::impls::azure::Azure,
            "https://example.services.ai.azure.com/openai/v1",
            "azure-secret",
            &request,
            "unused-model",
        )
        .expect("request should build");

        assert_eq!(
            built.url().as_str(),
            "https://example.services.ai.azure.com/openai/v1/responses"
        );
        assert_eq!(
            built
                .headers()
                .get("authorization")
                .expect("Authorization header"),
            "Bearer azure-secret"
        );
        assert!(built.headers().get("api-key").is_none());
        let body: Value = serde_json::from_slice(
            built
                .body()
                .and_then(reqwest::Body::as_bytes)
                .expect("JSON request body"),
        )
        .expect("valid JSON body");
        assert_eq!(body["store"], false);
        assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
        assert_eq!(body["prompt_cache_key"], "agent:workspace:v1");
        assert_eq!(
            body["prompt_cache_options"],
            json!({"mode": "implicit", "ttl": "30m"})
        );
    }
}
