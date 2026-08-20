//! Azure OpenAI — deployment URL + `api-key` header 认证 + 连通性探测。

use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Client;
use serde_json::{json, Value};

use crate::compat::{apply_thinking_compat, OpenAICompatible, OpenAICompletionModel, ThinkingFormat};
use crate::traits::{Capabilities, Capable, Nothing, ProviderExt};

const API_VERSION: &str = "2024-06-01";

#[derive(Debug, Clone, Copy, Default)]
pub struct Azure;

impl ProviderExt for Azure {
    const NAME: &'static str = "azure";
    const BASE_URL: &'static str = "";

    fn auth_headers(&self, key: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Ok(val) = HeaderValue::from_str(key) {
            headers.insert("api-key", val);
        }
        headers
    }
}

impl OpenAICompatible for Azure {
    const STREAM_USAGE: bool = true;
    const THINKING_FORMAT: ThinkingFormat = ThinkingFormat::ReasoningEffort;
    const EFFORT_MAP: &'static [(&'static str, &'static str)] =
        &[("max", "high"), ("xhigh", "high")];

    fn finalize_body(&self, body: &mut Value) {
        apply_thinking_compat(Self::THINKING_FORMAT, Self::EFFORT_MAP, body);
        body.as_object_mut().map(|obj| obj.remove("model"));
    }
}

impl Capabilities for Azure {
    type Chat = Capable<OpenAICompletionModel<Self>>;
    type Embedding = Nothing;
    type ImageGen = Nothing;
    type VideoGen = Nothing;
    type TTS = Nothing;
    type MusicGen = Nothing;
}

/// 从 Azure endpoint 提取资源根路径。
pub fn azure_base(endpoint: &str) -> String {
    endpoint
        .trim_end_matches('/')
        .trim_end_matches("/openai")
        .trim_end_matches("/v1")
        .to_string()
}

/// 构造 Azure 部署 URL。
pub fn azure_deployment_url(endpoint: &str, deployment: &str) -> String {
    let base = azure_base(endpoint);
    format!("{base}/openai/deployments/{deployment}/chat/completions?api-version={API_VERSION}")
}

/// Azure OpenAI 连通性探测（deployment URL + api-key 认证）。
pub async fn probe_azure(
    client: &Client,
    model: &str,
    config: &crate::types::request::ProviderConfig,
) -> Result<String, String> {
    let endpoint = config
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("");
    let url = azure_deployment_url(endpoint, model);
    let body = json!({
        "max_tokens": 1,
        "messages": [{"role": "user", "content": "ping"}]
    });
    let resp = client
        .post(&url)
        .header("api-key", &config.api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("连接 Azure OpenAI 失败: {e}"))?;
    let status = resp.status();
    let json: Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "失败 ({status}): {}",
            super::openai::extract_error_message(&json)
        ));
    }
    Ok("调用成功".to_string())
}
