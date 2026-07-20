//! 模型能力与上下文窗口：主要来自提供商 API 与 OpenRouter Models 表。
//!
//! 对目录漏标且运行时已支持的能力，可做有限的 known 补丁（不作泛化名称猜测）。

use serde::{Deserialize, Serialize};

/// 模型能力位（视觉 / 联网 / 推理 / 工具 / 生图 / 生视频 / 生音频 / 生音乐）。
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
    #[serde(default)]
    pub image_gen: bool,
    #[serde(default)]
    pub video_gen: bool,
    #[serde(default)]
    pub audio_gen: bool,
    #[serde(default)]
    pub music_gen: bool,
}

/// OpenRouter `reasoning` 对象：档位、默认开关等。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ModelReasoningMeta {
    /// 如 `["xhigh","high"]` / `["high","medium","low"]`
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_efforts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mandatory: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_max_tokens: Option<bool>,
}

impl ModelReasoningMeta {
    /// 是否携带任何有意义的推理元数据。
    pub fn is_empty(&self) -> bool {
        self.supported_efforts.is_empty()
            && self.default_effort.is_none()
            && self.default_enabled.is_none()
            && self.mandatory.is_none()
            && self.supports_max_tokens.is_none()
    }
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
    /// OpenRouter 推理档位 / 默认开关（无推理模型为 null）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ModelReasoningMeta>,
    /// 元数据来源：api / openrouter / known（可组合）
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
        reasoning: None,
        meta_source: String::new(),
    };
    enrich_model_info(&mut info, kind, hints);
    info
}

/// `meta_source` 是否已包含指定来源标记（`api` / `openrouter` 等）。
fn prior_has_source(meta_source: &str, needle: &str) -> bool {
    meta_source.split('+').any(|s| s == needle)
}

/// 合并 API hints 与 OpenRouter 表，刷新能力位与上下文窗口。
pub fn enrich_model_info(info: &mut ModelInfo, kind: &str, hints: Option<ApiModelHints>) {
    let id_lower = info.id.to_lowercase();
    let kind = kind.to_lowercase();
    let prior_meta = info.meta_source.clone();

    // 只保留先前由 API 写入的上下文；能力一律按本次 API + OpenRouter 重算
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
    info.reasoning = None;
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

    if let Some(entry) = crate::openrouter_meta::lookup(&id_lower, &kind) {
        if info.display_name.is_none() {
            info.display_name = entry.display_name.clone();
        }
        if let Some(n) = entry.max_input_tokens {
            if !context_locked_by_api {
                info.context_window = Some(n);
            }
        }
        let api_has_output = hints.as_ref().and_then(|h| h.max_output_tokens).is_some()
            || retained_api_out.is_some();
        if let Some(n) = entry.max_output_tokens {
            if !api_has_output {
                info.max_output_tokens = Some(n);
            }
        }

        let media_only = (entry.supports_image_generation
            || entry.supports_video_generation
            || entry.supports_audio_output
            || entry.supports_music_generation)
            && !entry.supports_function_calling;

        if media_only {
            info.capabilities.vision = entry.supports_vision;
            info.capabilities.web = false;
            info.capabilities.reasoning = false;
            info.capabilities.tools = false;
            info.capabilities.image_gen |= entry.supports_image_generation;
            info.capabilities.video_gen |= entry.supports_video_generation;
            info.capabilities.music_gen |= entry.supports_music_generation;
            if !entry.supports_music_generation {
                info.capabilities.audio_gen |= entry.supports_audio_output;
            }
            info.reasoning = None;
        } else {
            info.capabilities.vision |= entry.supports_vision;
            info.capabilities.web |= entry.supports_web_search;
            info.capabilities.reasoning |= entry.supports_reasoning;
            info.capabilities.tools |= entry.supports_function_calling;
            info.capabilities.image_gen |= entry.supports_image_generation;
            info.capabilities.video_gen |= entry.supports_video_generation;
            info.capabilities.music_gen |= entry.supports_music_generation;
            if !entry.supports_music_generation {
                info.capabilities.audio_gen |= entry.supports_audio_output;
            }
            if entry.supports_reasoning {
                info.reasoning = Some(entry.reasoning.clone());
            }
        }
        sources.push("openrouter");
    }

    if apply_known_capability_overrides(&kind, &id_lower, info) {
        sources.push("known");
    }

    sources.sort();
    sources.dedup();
    info.meta_source = sources.join("+");
}

/// 目录漏标补丁：仅覆盖已核实、且运行时协议已支持的能力。
///
/// DeepSeek V4：官方 API 支持 thinking；若 OpenRouter 未命中仍可补上。
fn apply_known_capability_overrides(kind: &str, id_lower: &str, info: &mut ModelInfo) -> bool {
    let is_deepseek_family = kind == "deepseek" || id_lower.contains("deepseek");
    if !is_deepseek_family {
        return false;
    }
    if info.capabilities.image_gen
        || info.capabilities.video_gen
        || info.capabilities.audio_gen
        || info.capabilities.music_gen
    {
        return false;
    }
    if id_lower.contains("embed") || id_lower.contains("tts") || id_lower.contains("whisper") {
        return false;
    }

    let supports_thinking = id_lower.contains("deepseek-v4")
        || id_lower.contains("deepseek-reasoner")
        || id_lower.contains("deepseek-r1")
        || id_lower.contains("deepseek-r");
    if supports_thinking && !info.capabilities.reasoning {
        info.capabilities.reasoning = true;
        if info.reasoning.is_none() {
            info.reasoning = Some(ModelReasoningMeta {
                supported_efforts: vec!["high".into(), "xhigh".into()],
                default_effort: Some("high".into()),
                default_enabled: Some(true),
                mandatory: Some(false),
                supports_max_tokens: None,
            });
        }
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_api_only_without_openrouter() {
        crate::openrouter_meta::with_fixture(r#"{"data":[]}"#, || {
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
    fn openrouter_enriches_deepseek() {
        crate::openrouter_meta::with_fixture(
            r#"{
              "data": [{
                "id": "deepseek/deepseek-chat",
                "name": "DeepSeek Chat",
                "context_length": 131072,
                "architecture": {
                  "input_modalities": ["text"],
                  "output_modalities": ["text"]
                },
                "supported_parameters": ["tools", "tool_choice"],
                "top_provider": { "max_completion_tokens": 8192 },
                "reasoning": null
              }]
            }"#,
            || {
                let info = enrich_from_id("deepseek-chat", "deepseek", None);
                assert_eq!(info.context_window, Some(131072));
                assert_eq!(info.max_output_tokens, Some(8192));
                assert!(info.capabilities.tools);
                assert!(!info.capabilities.reasoning);
                assert_eq!(info.meta_source, "openrouter");
            },
        );
    }

    #[test]
    fn clears_stale_caps_when_openrouter_hits() {
        crate::openrouter_meta::with_fixture(
            r#"{
              "data": [{
                "id": "deepseek/deepseek-v4-flash",
                "name": "DeepSeek V4 Flash",
                "context_length": 1000000,
                "architecture": {
                  "input_modalities": ["text"],
                  "output_modalities": ["text"]
                },
                "supported_parameters": ["tools", "reasoning", "reasoning_effort"],
                "reasoning": { "mandatory": false, "default_effort": "high", "supported_efforts": ["xhigh", "high"] }
              }]
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
                    reasoning: None,
                    meta_source: "heuristic+table".into(),
                };
                enrich_model_info(&mut stale, "deepseek", None);
                assert_eq!(stale.context_window, Some(1_000_000));
                assert!(!stale.capabilities.vision);
                assert!(stale.capabilities.tools);
                assert!(stale.capabilities.reasoning);
                assert_eq!(stale.meta_source, "openrouter");
                let r = stale.reasoning.expect("reasoning meta");
                assert_eq!(r.default_effort.as_deref(), Some("high"));
                assert!(r.supported_efforts.iter().any(|e| e == "xhigh"));
            },
        );
    }

    #[test]
    fn deepseek_v4_known_reasoning_when_openrouter_misses() {
        crate::openrouter_meta::with_fixture(r#"{"data":[]}"#, || {
            let info = enrich_from_id("deepseek-v4-pro", "deepseek", None);
            assert!(info.capabilities.reasoning);
            assert_eq!(info.meta_source, "known");
        });
    }

    #[test]
    fn no_hardcode_when_both_miss() {
        crate::openrouter_meta::with_fixture(r#"{"data":[]}"#, || {
            let info = enrich_from_id("gpt-4o-2024-08-06", "openai", None);
            assert_eq!(info.context_window, None);
            assert!(!info.capabilities.vision);
            assert!(!info.capabilities.tools);
            assert!(info.meta_source.is_empty());
        });
    }

    #[test]
    fn api_without_openrouter_keeps_api_only() {
        crate::openrouter_meta::with_fixture(r#"{"data":[]}"#, || {
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

    #[test]
    fn openrouter_image_output_sets_image_gen() {
        crate::openrouter_meta::with_fixture(
            r#"{
              "data": [{
                "id": "openai/dall-e-3",
                "name": "DALL-E 3",
                "context_length": 4000,
                "architecture": {
                  "input_modalities": ["text"],
                  "output_modalities": ["image"]
                },
                "supported_parameters": [],
                "reasoning": null
              }]
            }"#,
            || {
                let info = enrich_from_id("dall-e-3", "openai", None);
                assert!(info.capabilities.image_gen);
                assert!(!info.capabilities.tools);
                assert!(!info.capabilities.vision);
                assert_eq!(info.meta_source, "openrouter");
            },
        );
    }

    #[test]
    fn openrouter_audio_output_sets_audio_gen() {
        crate::openrouter_meta::with_fixture(
            r#"{
              "data": [{
                "id": "openai/gpt-4o-mini-tts",
                "name": "GPT-4o mini TTS",
                "context_length": 8000,
                "architecture": {
                  "input_modalities": ["text"],
                  "output_modalities": ["audio"]
                },
                "supported_parameters": [],
                "reasoning": null
              }]
            }"#,
            || {
                let info = enrich_from_id("gpt-4o-mini-tts", "openai", None);
                assert!(info.capabilities.audio_gen);
                assert!(!info.capabilities.tools);
                assert!(!info.capabilities.music_gen);
            },
        );
    }

    #[test]
    fn openrouter_chat_multimodal_flags() {
        crate::openrouter_meta::with_fixture(
            r#"{
              "data": [{
                "id": "google/gemini-2.0-flash",
                "name": "Gemini 2.0 Flash",
                "context_length": 1048576,
                "architecture": {
                  "input_modalities": ["text", "image"],
                  "output_modalities": ["text", "image"]
                },
                "supported_parameters": ["tools", "tool_choice"],
                "reasoning": null
              }]
            }"#,
            || {
                let info = enrich_from_id("gemini-2.0-flash", "google", None);
                assert!(info.capabilities.tools);
                assert!(info.capabilities.vision);
                assert!(info.capabilities.image_gen);
                assert!(!info.capabilities.video_gen);
            },
        );
    }

    #[test]
    fn openrouter_video_output_sets_video_gen() {
        crate::openrouter_meta::with_fixture(
            r#"{
              "data": [{
                "id": "openai/sora-2",
                "name": "Sora 2",
                "context_length": 8000,
                "architecture": {
                  "input_modalities": ["text"],
                  "output_modalities": ["video"]
                },
                "supported_parameters": [],
                "reasoning": null
              }]
            }"#,
            || {
                let info = enrich_from_id("sora-2", "openai", None);
                assert!(info.capabilities.video_gen);
                assert!(!info.capabilities.tools);
            },
        );
    }

    #[test]
    fn openrouter_lyria_sets_music_gen_only() {
        crate::openrouter_meta::with_fixture(
            r#"{
              "data": [{
                "id": "google/lyria-3-pro-preview",
                "name": "Lyria 3 Pro",
                "context_length": 1048576,
                "architecture": {
                  "input_modalities": ["text", "image"],
                  "output_modalities": ["text", "audio"]
                },
                "supported_parameters": [],
                "reasoning": null
              }]
            }"#,
            || {
                let info = enrich_from_id("lyria-3-pro-preview", "google", None);
                assert!(info.capabilities.music_gen);
                assert!(!info.capabilities.audio_gen);
                assert!(!info.capabilities.tools);
                assert!(info.capabilities.vision);
            },
        );
    }
}
