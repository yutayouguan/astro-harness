//! TOML 驱动的自定义 OpenAI 兼容 provider — 用户在 config.toml 中声明即可接入。
//!
//! 只支持 Responses API 路径（与 Codex 对齐），不需要 thinking format / effort map 等兼容参数。

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::traits::CompletionModel;
use crate::types::{CompletionRequest, CompletionStream};

/// 单个自定义 provider 的 TOML 配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomProviderConfig {
    /// 显示名称。
    #[serde(default)]
    pub name: String,
    /// API 基址。
    pub base_url: String,
    /// 环境变量名列表（按顺序尝试读取 API Key）。
    #[serde(default)]
    pub env_keys: Vec<String>,
    /// 默认模型名。
    #[serde(default)]
    pub default_model: String,
}

/// 所有自定义 provider 的顶层配置。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CustomProvidersTable {
    #[serde(default)]
    pub custom_providers: HashMap<String, CustomProviderConfig>,
}

/// 从 TOML 文件加载自定义 provider 表。
pub fn load_custom_providers(config_path: &Path) -> HashMap<String, CustomProviderConfig> {
    let content = match std::fs::read_to_string(config_path) {
        Ok(c) => c,
        Err(_) => return HashMap::new(),
    };
    let table: CustomProvidersTable = match toml::from_str(&content) {
        Ok(t) => t,
        Err(_) => return HashMap::new(),
    };
    table.custom_providers
}

/// 从环境变量读取第一个非空的 API Key。
pub fn read_env_key(env_keys: &[String]) -> Option<String> {
    for name in env_keys {
        if let Ok(value) = std::env::var(name) {
            let trimmed = value.trim().to_string();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

/// 配置驱动的 Responses API 补全模型。
///
/// 所有行为由运行时配置参数化，不依赖编译期 trait 常量。
/// 统一走 Responses API，与 Codex 自定义 provider 行为一致。
#[derive(Clone)]
pub struct ConfigDrivenCompletionModel {
    http: HttpClient,
    base_url: String,
    api_key: String,
    model: String,
    provider_id: String,
}

impl ConfigDrivenCompletionModel {
    pub fn new(
        http: HttpClient,
        config: &CustomProviderConfig,
        api_key: String,
        model: String,
        provider_id: String,
    ) -> Self {
        Self {
            http,
            base_url: config.base_url.clone(),
            api_key,
            model,
            provider_id,
        }
    }
}

#[async_trait::async_trait]
impl CompletionModel for ConfigDrivenCompletionModel {
    async fn stream(&self, request: CompletionRequest) -> Result<CompletionStream> {
        if self.api_key.trim().is_empty() {
            anyhow::bail!("{} API Key 为空", self.provider_id);
        }

        let base = crate::compat::openai_compatible_base(&self.base_url);
        let url = format!("{base}/responses");
        let model = if request.model.is_empty() {
            &self.model
        } else {
            &request.model
        };

        let input = crate::openai::responses::to_responses_input(&request.messages);

        let instructions = request.messages.iter().find_map(|m| {
            if let crate::types::message::Message::System { content } = m {
                Some(content.clone())
            } else {
                None
            }
        });

        let mut body = json!({
            "model": model,
            "input": input,
            "stream": true,
        });
        if let Some(inst) = instructions {
            if !inst.is_empty() {
                body["instructions"] = json!(inst);
            }
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
        }

        if let Some(ref tc) = request.thinking {
            if tc.enabled {
                let effort = match tc.effort.trim() {
                    "" | "high" => "high",
                    other => other,
                };
                body["reasoning"] = json!({"effort": effort});
            }
        }

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
            .with_context(|| format!("连接 {} Responses API 失败: {url}", self.provider_id))?;

        crate::shared::sse::sse_stream(
            response,
            Arc::new(crate::openai::responses::extract_responses_chunks),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_custom_providers_toml() {
        let toml = r#"
[custom_providers.my-corp]
name = "Corp LLM"
base_url = "https://llm.corp.internal/v1"
env_keys = ["CORP_API_KEY"]
default_model = "corp-v3"

[custom_providers.local-ollama]
base_url = "http://localhost:11434/v1"
default_model = "llama3.3"
"#;
        let table: CustomProvidersTable = toml::from_str(toml).unwrap();
        assert_eq!(table.custom_providers.len(), 2);

        let corp = &table.custom_providers["my-corp"];
        assert_eq!(corp.name, "Corp LLM");
        assert_eq!(corp.base_url, "https://llm.corp.internal/v1");
        assert_eq!(corp.env_keys, vec!["CORP_API_KEY"]);
        assert_eq!(corp.default_model, "corp-v3");

        let local = &table.custom_providers["local-ollama"];
        assert!(local.env_keys.is_empty());
    }

    #[test]
    fn empty_toml_returns_empty() {
        let table: CustomProvidersTable = toml::from_str("").unwrap();
        assert!(table.custom_providers.is_empty());
    }

    #[test]
    fn missing_optional_fields_default() {
        let toml = r#"
[custom_providers.minimal]
base_url = "https://api.example.com"
"#;
        let table: CustomProvidersTable = toml::from_str(toml).unwrap();
        let p = &table.custom_providers["minimal"];
        assert!(p.name.is_empty());
        assert!(p.env_keys.is_empty());
        assert!(p.default_model.is_empty());
    }
}
