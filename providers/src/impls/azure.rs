//! Azure OpenAI — deployment URL + `api-key` header 认证。

use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;

use crate::compat::{OpenAICompatible, OpenAICompletionModel};
use crate::traits::{Capable, Capabilities, Nothing, ProviderExt};

const API_VERSION: &str = "2024-06-01";

#[derive(Debug, Clone, Copy)]
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

    fn finalize_body(&self, body: &mut Value) {
        // Azure 不在 body 里传 model — 在 URL deployment 路径中
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
    type ASR = Nothing;
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
