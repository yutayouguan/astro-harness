//! 模型能力与上下文窗口：仅来自提供商 API 字段与 LiteLLM 表，不做名称硬编码猜测。

use serde::{Deserialize, Serialize};

/// 模型能力位（视觉 / 联网 / 推理 / 工具）。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ModelCapabilities {
    #[serde(default)]
    pub vision: bool,
    #[serde(default)]
    pub web: bool,
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default)]
    pub tools: bool,
}

/// 前端展示用的模型元信息。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// 输入上下文窗口（token）；未知为 null
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    #[serde(default)]
    pub capabilities: ModelCapabilities,
    /// 元数据来源：api / litellm（可组合）
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub meta_source: String,
}

/// 兼容旧版 `models: ["id", ...]` 与新版对象数组
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModelEntryCompat {
    Id(String),
    Full(ModelInfo),
}

impl ModelEntryCompat {
    /// 转为完整 [`ModelInfo`]，并按供应商 kind 做元数据 enrich。
    pub fn into_info(self, kind: &str) -> ModelInfo {
        match self {
            Self::Full(mut info) => {
                enrich_model_info(&mut info, kind, None);
                info
            }
            Self::Id(id) => enrich_from_id(&id, kind, None),
        }
    }
}

/// API 侧已解析出的提示（如 Google inputTokenLimit）
#[derive(Debug, Clone, Default)]
pub struct ApiModelHints {
    pub display_name: Option<String>,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
    pub supported_methods: Vec<String>,
}

/// 仅用模型 id + kind（及可选 API hints）构造并 enrich [`ModelInfo`]。
pub fn enrich_from_id(id: &str, kind: &str, hints: Option<ApiModelHints>) -> ModelInfo {
    let mut info = ModelInfo {
        id: id.to_string(),
        display_name: None,
        context_window: None,
        max_output_tokens: None,
        capabilities: ModelCapabilities::default(),
        meta_source: String::new(),
    };
    enrich_model_info(&mut info, kind, hints);
    info
}

/// `meta_source` 是否已包含指定来源标记（`api` / `litellm` 等）。
fn prior_has_source(meta_source: &str, needle: &str) -> bool {
    meta_source.split('+').any(|s| s == needle)
}

/// 合并 API hints 与 LiteLLM 表，刷新能力位与上下文窗口。
pub fn enrich_model_info(info: &mut ModelInfo, kind: &str, hints: Option<ApiModelHints>) {
    let id_lower = info.id.to_lowercase();
    let kind = kind.to_lowercase();
    let prior_meta = info.meta_source.clone();

    // 只保留先前由 API 写入的上下文；能力一律按本次 API + LiteLLM 重算（清掉 heuristic 等）
    let retained_api_ctx = if prior_has_source(&prior_meta, "api") {
        info.context_window
    } else {
        None
    };
    let retained_api_out = if prior_has_source(&prior_meta, "api") {
        info.max_output_tokens
    } else {
        None
    };
    let retained_display = info.display_name.clone();

    info.context_window = retained_api_ctx;
    info.max_output_tokens = retained_api_out;
    info.capabilities = ModelCapabilities::default();
    info.display_name = retained_display;
    info.meta_source.clear();

    let mut sources = Vec::new();
    let mut context_locked_by_api = info.context_window.is_some();
    if context_locked_by_api {
        sources.push("api");
    }

    if let Some(ref h) = hints {
        if info.display_name.is_none() {
            info.display_name = h.display_name.clone();
        }
        if let Some(n) = h.context_window {
            info.context_window = Some(n);
            context_locked_by_api = true;
            if !sources.iter().any(|s| *s == "api") {
                sources.push("api");
            }
        }
        if let Some(n) = h.max_output_tokens {
            info.max_output_tokens = Some(n);
        }
        let methods = h
            .supported_methods
            .iter()
            .map(|s| s.to_lowercase())
            .collect::<Vec<_>>();
        if !methods.is_empty() {
            let can_gen = methods.iter().any(|m| {
                m.contains("generatecontent") || m.contains("createmessage") || m == "chat"
            });
            let is_embed = methods.iter().any(|m| m.contains("embed"));
            if can_gen && !is_embed {
                info.capabilities.tools = true;
                if !sources.iter().any(|s| *s == "api") {
                    sources.push("api");
                }
            }
        }
    }

    if let Some(entry) = crate::litellm_meta::lookup(&id_lower, &kind) {
        if let Some(n) = entry.max_input_tokens {
            if !context_locked_by_api {
                info.context_window = Some(n);
            }
        }
        let api_has_output = hints
            .as_ref()
            .and_then(|h| h.max_output_tokens)
            .is_some()
            || retained_api_out.is_some();
        if let Some(n) = entry.max_output_tokens {
            if !api_has_output {
                info.max_output_tokens = Some(n);
            }
        }

        let non_chat = entry
            .mode
            .as_deref()
            .map(|m| {
                let m = m.to_lowercase();
                m.contains("embed")
                    || m.contains("image")
                    || m.contains("audio")
                    || m.contains("moderation")
            })
            .unwrap_or(false);

        if non_chat {
            info.capabilities.vision = false;
            info.capabilities.web = false;
            info.capabilities.reasoning = false;
            info.capabilities.tools = false;
        } else {
            info.capabilities.vision |= entry.supports_vision;
            info.capabilities.web |= entry.supports_web_search;
            info.capabilities.reasoning |= entry.supports_reasoning;
            info.capabilities.tools |= entry.supports_function_calling;
        }
        sources.push("litellm");
    }

    sources.sort();
    sources.dedup();
    info.meta_source = sources.join("+");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_api_only_without_litellm() {
        crate::litellm_meta::with_fixture("{}", || {
            let info = enrich_from_id(
                "gemini-2.5-flash",
                "google",
                Some(ApiModelHints {
                    display_name: Some("Gemini 2.5 Flash".into()),
                    context_window: Some(1_048_576),
                    max_output_tokens: Some(65_536),
                    supported_methods: vec!["generateContent".into()],
                }),
            );
            assert_eq!(info.context_window, Some(1_048_576));
            assert!(info.capabilities.tools);
            assert_eq!(info.meta_source, "api");
            assert!(!info.capabilities.vision);
            assert!(!info.capabilities.web);
        });
    }

    #[test]
    fn litellm_enriches_deepseek() {
        crate::litellm_meta::with_fixture(
            r#"{
              "deepseek-chat": {
                "max_input_tokens": 131072,
                "max_output_tokens": 8192,
                "supports_function_calling": true,
                "mode": "chat",
                "litellm_provider": "deepseek"
              }
            }"#,
            || {
                let info = enrich_from_id("deepseek-chat", "deepseek", None);
                assert_eq!(info.context_window, Some(131072));
                assert_eq!(info.max_output_tokens, Some(8192));
                assert!(info.capabilities.tools);
                assert_eq!(info.meta_source, "litellm");
            },
        );
    }

    #[test]
    fn clears_stale_heuristic_when_litellm_hits() {
        crate::litellm_meta::with_fixture(
            r#"{
              "deepseek-v4-flash": {
                "max_input_tokens": 1000000,
                "supports_function_calling": true,
                "mode": "chat",
                "litellm_provider": "deepseek"
              }
            }"#,
            || {
                let mut stale = ModelInfo {
                    id: "deepseek-v4-flash".into(),
                    display_name: None,
                    context_window: Some(128_000),
                    max_output_tokens: None,
                    capabilities: ModelCapabilities {
                        vision: true,
                        ..Default::default()
                    },
                    meta_source: "heuristic+table".into(),
                };
                enrich_model_info(&mut stale, "deepseek", None);
                assert_eq!(stale.context_window, Some(1_000_000));
                assert!(!stale.capabilities.vision);
                assert!(stale.capabilities.tools);
                assert_eq!(stale.meta_source, "litellm");
            },
        );
    }

    #[test]
    fn no_hardcode_when_both_miss() {
        crate::litellm_meta::with_fixture("{}", || {
            let info = enrich_from_id("gpt-4o-2024-08-06", "openai", None);
            assert_eq!(info.context_window, None);
            assert!(!info.capabilities.vision);
            assert!(!info.capabilities.tools);
            assert!(info.meta_source.is_empty());
        });
    }

    #[test]
    fn api_without_litellm_keeps_api_only() {
        crate::litellm_meta::with_fixture("{}", || {
            let info = enrich_from_id(
                "some-unknown-model",
                "openai",
                Some(ApiModelHints {
                    display_name: None,
                    context_window: Some(32_000),
                    max_output_tokens: None,
                    supported_methods: Vec::new(),
                }),
            );
            assert_eq!(info.context_window, Some(32_000));
            assert_eq!(info.meta_source, "api");
            assert!(!info.capabilities.tools);
            assert!(!info.capabilities.vision);
        });
    }
}
