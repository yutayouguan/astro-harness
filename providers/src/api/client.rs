//! 持有凭证与 HTTP 客户端的 ProviderClient（借鉴 Rig Client 分层）。

use anyhow::{anyhow, Result};

use crate::profile::{self, normalize_provider_id};
use crate::trait_::AuthKind;

/// 已解析凭证的客户端；对话/探测时再配 model 等运行参数。
#[derive(Debug, Clone)]
pub struct ProviderClient {
    /// 复用的 HTTP 客户端。
    pub http: reqwest::Client,
    /// 供应商标识符（已规范化别名）。
    pub provider_id: String,
    /// API 密钥。
    pub api_key: String,
    /// 自定义 API 基址。
    pub base_url: Option<String>,
    /// 推断的认证方式。
    pub auth: AuthKind,
}

impl ProviderClient {
    /// 从 provider id、密钥与基址显式构造客户端。
    pub fn from_config(provider_id: &str, api_key: String, base_url: Option<String>) -> Self {
        let id = normalize_provider_id(provider_id).to_string();
        Self {
            http: reqwest::Client::new(),
            provider_id: id.clone(),
            api_key,
            base_url,
            auth: AuthKind::for_provider(&id),
        }
    }

    /// 从进程环境变量构造；Ollama 允许无 key。
    pub fn from_env(provider_id: &str) -> Result<Self> {
        let id = normalize_provider_id(provider_id).to_string();
        let auth = AuthKind::for_provider(&id);
        let api_key = if auth == AuthKind::None {
            String::new()
        } else {
            read_env_api_key(&id).ok_or_else(|| {
                anyhow!(
                    "未找到 {} 的 API Key 环境变量（尝试过: {}）",
                    id,
                    env_api_key_names(&id).join(", ")
                )
            })?
        };
        let base_url = Some(profile::default_base_for(&id).to_string()).filter(|s| !s.is_empty());
        Ok(Self::from_config(&id, api_key, base_url))
    }

    /// 替换底层 HTTP 客户端（用于注入自定义超时、代理等）。
    pub fn with_http(mut self, http: reqwest::Client) -> Self {
        self.http = http;
        self
    }

    /// 构造结构化抽取器（Rig `client.extractor::<T>(model)` 风格）。
    pub fn extractor<T>(&self, model: impl Into<String>) -> crate::extractor::ExtractorBuilder<T>
    where
        T: serde::de::DeserializeOwned + serde::Serialize + schemars::JsonSchema + Send + Sync,
    {
        let config = crate::trait_::ProviderConfig {
            api_key: self.api_key.clone(),
            base_url: self.base_url.clone(),
            model: String::new(),
            temperature: 0.2,
            max_tokens: 4096,
            thinking_enabled: false,
            reasoning_effort: "high".into(),
            additional_params: serde_json::Value::Null,
            previous_interaction_id: None,
        };
        crate::extractor::ExtractorBuilder::new(self.provider_id.clone(), model, config)
    }
}

/// 返回某供应商依次尝试的环境变量名列表。
pub fn env_api_key_names(provider_id: &str) -> &'static [&'static str] {
    profile::env_api_key_names(provider_id)
}

/// 从环境变量读取第一个非空的 API Key。
pub fn read_env_api_key(provider_id: &str) -> Option<String> {
    for name in env_api_key_names(provider_id) {
        if let Ok(value) = std::env::var(name) {
            let trimmed = value.trim().to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}
