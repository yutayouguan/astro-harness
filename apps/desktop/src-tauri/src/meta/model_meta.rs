//! 模型能力与上下文窗口：主要来自提供商 API 与 OpenRouter Models 表。
//!
//! 对目录漏标且运行时已支持的能力，可做有限的 known 补丁（不作泛化名称猜测）。

use serde::{Deserialize, Serialize};

/// 模型能力位（视觉 / 联网 / 推理 / 工具 / 文件·音频输入 / 生图·视频·音频·音乐）。
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
    /// 接受 file 输入（PDF 等）
    #[serde(default)]
    pub file: bool,
    /// 接受 audio 输入
    #[serde(default)]
    pub audio_in: bool,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistent_instructions: Option<String>,
}

impl ModelReasoningMeta {
    /// 是否携带任何有意义的推理元数据。
    pub fn is_empty(&self) -> bool {
        self.supported_efforts.is_empty()
            && self.default_effort.is_none()
            && self.default_enabled.is_none()
            && self.mandatory.is_none()
            && self.supports_max_tokens.is_none()
            && self.persistent_instructions.is_none()
    }
}

/// OpenRouter 单价（USD / 百万 tokens），便于列表展示与估费。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ModelPricingMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_per_million: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_per_million: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_per_million: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_per_million: Option<f64>,
}

/// OpenRouter `default_parameters`（采样默认值）。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ModelDefaultParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repetition_penalty: Option<f64>,
}

/// 前端展示用的模型元信息。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelInfo {
    pub id: String,
    /// 模型目录提供的运行时能力契约。
    #[serde(default)]
    pub profile: types::ModelProfile,
    /// 模型目录指定的工具模式；存在时覆盖全局 feature flag。
    #[serde(
        default,
        deserialize_with = "types::deserialize_optional_tool_mode",
        skip_serializing_if = "Option::is_none"
    )]
    pub tool_mode: Option<types::ToolMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knowledge_cutoff: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiration_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hugging_face_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_moderated: Option<bool>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pricing: Option<ModelPricingMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_parameters: Option<ModelDefaultParams>,
    /// 元数据来源：api / openrouter / deepseek_catalog / known（可组合）
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub meta_source: String,
}

impl ModelInfo {
    pub fn usable_context_window(&self) -> Option<u64> {
        self.context_window
            .map(|window| self.profile.usable_context_window(window))
    }
}

/// 兼容旧版 `models: ["id", ...]` 与新版对象数组
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModelEntryCompat {
    Id(String),
    Full(Box<ModelInfo>),
}

impl ModelEntryCompat {
    /// 转为完整 [`ModelInfo`]，并按供应商 kind 做元数据 enrich。
    pub fn into_info(self, kind: &str) -> ModelInfo {
        match self {
            Self::Full(mut info) => {
                enrich_model_info(&mut info, kind, None);
                *info
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
        profile: types::ModelProfile::default(),
        tool_mode: None,
        display_name: None,
        description: None,
        canonical_slug: None,
        knowledge_cutoff: None,
        expiration_date: None,
        created: None,
        hugging_face_id: None,
        is_moderated: None,
        context_window: None,
        max_output_tokens: None,
        capabilities: ModelCapabilities::default(),
        reasoning: None,
        pricing: None,
        default_parameters: None,
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
    let retained_description = info.description.clone();
    let retained_canonical = info.canonical_slug.clone();
    let retained_persistent_instructions = if kind == "openai" {
        info.reasoning
            .as_ref()
            .and_then(|reasoning| reasoning.persistent_instructions.as_deref())
            .map(str::trim)
            .filter(|instructions| !instructions.is_empty())
            .map(str::to_string)
    } else {
        None
    };

    info.context_window = retained_api_ctx;
    info.max_output_tokens = retained_api_out;
    info.capabilities = ModelCapabilities::default();
    info.reasoning = None;
    info.pricing = None;
    info.default_parameters = None;
    info.knowledge_cutoff = None;
    info.expiration_date = None;
    info.created = None;
    info.hugging_face_id = None;
    info.is_moderated = None;
    info.description = retained_description;
    info.canonical_slug = retained_canonical;
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
            if !sources.contains(&"api") {
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
                if !sources.contains(&"api") {
                    sources.push("api");
                }
            }
        }
    }

    if let Some(entry) = super::openrouter_meta::lookup(&id_lower, &kind) {
        if info.display_name.is_none() {
            info.display_name = entry.display_name.clone();
        }
        if info.description.is_none() {
            info.description = entry.description.clone();
        }
        if info.canonical_slug.is_none() {
            info.canonical_slug = entry.canonical_slug.clone();
        }
        info.knowledge_cutoff = entry.knowledge_cutoff.clone();
        info.expiration_date = entry.expiration_date.clone();
        info.created = entry.created;
        info.hugging_face_id = entry.hugging_face_id.clone();
        info.is_moderated = entry.is_moderated;
        info.pricing = entry.pricing.clone();
        info.default_parameters = entry.default_parameters.clone();
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
            info.capabilities.file = entry.supports_file_input;
            info.capabilities.audio_in = entry.supports_audio_input;
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
            info.capabilities.file |= entry.supports_file_input;
            info.capabilities.audio_in |= entry.supports_audio_input;
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

    if apply_deepseek_official_profile(&kind, &id_lower, info) {
        sources.push("deepseek_catalog");
    }

    if apply_known_capability_overrides(&kind, &id_lower, info) {
        sources.push("known");
    }
    if let Some(instructions) = retained_persistent_instructions {
        info.capabilities.reasoning = true;
        let reasoning = info
            .reasoning
            .get_or_insert_with(ModelReasoningMeta::default);
        if !reasoning
            .supported_efforts
            .iter()
            .any(|effort| effort == "persistent")
        {
            reasoning.supported_efforts.push("persistent".into());
        }
        reasoning.persistent_instructions = Some(instructions);
        sources.push("api");
    }

    sources.sort();
    sources.dedup();
    info.meta_source = sources.join("+");
}

/// DeepSeek 官方 Codex models.json 中已声明的模型能力。
fn apply_deepseek_official_profile(kind: &str, id_lower: &str, info: &mut ModelInfo) -> bool {
    if kind != "deepseek" {
        return false;
    }
    let (display_name, description, vision) = match id_lower {
        "deepseek-v4-flash" => (
            "DeepSeek-V4-Flash",
            "Latest frontier agentic coding model.",
            false,
        ),
        "deepseek-v4-pro" => (
            "DeepSeek-V4-Pro",
            "Most capable frontier agentic coding model.",
            false,
        ),
        "deepseek-v4-flash-vision-exp" => (
            "DeepSeek-V4-Flash-Vision",
            "Latest frontier agentic coding model with image input.",
            true,
        ),
        _ => return false,
    };

    info.display_name = Some(display_name.into());
    info.description = Some(description.into());
    info.context_window = Some(1_048_576);
    info.capabilities.tools = true;
    info.capabilities.reasoning = true;
    info.capabilities.web = true;
    info.capabilities.vision = vision;
    info.capabilities.file = false;
    info.capabilities.audio_in = false;
    info.reasoning = Some(ModelReasoningMeta {
        supported_efforts: vec!["low".into(), "high".into(), "max".into()],
        default_effort: Some("high".into()),
        default_enabled: Some(true),
        mandatory: Some(false),
        supports_max_tokens: None,
        persistent_instructions: None,
    });
    info.profile = types::ModelProfile {
        supports_search_tool: true,
        supports_parallel_tool_calls: true,
        support_verbosity: true,
        default_verbosity: Some(types::ModelVerbosity::Low),
        apply_patch_tool_type: Some(types::ApplyPatchToolType::Freeform),
        web_search_tool_type: types::WebSearchToolType::Text,
        input_modalities: if vision {
            vec![
                types::ModelInputModality::Text,
                types::ModelInputModality::Image,
            ]
        } else {
            vec![types::ModelInputModality::Text]
        },
        effective_context_window_percent: 95,
        auto_compact_token_limit: None,
        supports_reasoning_summaries: true,
        multi_agent_version: Some(types::ModelMultiAgentVersion::V2),
    };
    true
}

/// 目录漏标补丁：仅覆盖已核实、且运行时协议已支持的能力。
///
/// - DeepSeek V4：官方 API 支持 thinking；若 OpenRouter 未命中仍可补上。
/// - Google Gemini/Gemma 聊天模型：支持 Search grounding，OpenRouter 常不标 web。
fn apply_known_capability_overrides(kind: &str, id_lower: &str, info: &mut ModelInfo) -> bool {
    let mut changed = false;

    // Azure /models can list these before OpenRouter publishes modality metadata.
    // Reference-image editing is not chat vision/tool support. Do not infer pricing.
    let image_id = id_lower.strip_prefix("openai/").unwrap_or(id_lower);
    if matches!(kind, "azure" | "openai" | "openrouter")
        && matches!(
            image_id,
            "gpt-image-2"
                | "gpt-image-2-2026-04-21"
                | "gpt-image-2.5-flare"
                | "gpt-image-2.5-flare-2026-09-08"
                | "gpt-image-2.5-sunburst"
                | "gpt-image-2.5-sunburst-2026-09-08"
        )
    {
        info.capabilities = ModelCapabilities {
            image_gen: true,
            ..ModelCapabilities::default()
        };
        info.reasoning = None;
        return true;
    }

    let is_deepseek_family = kind == "deepseek" || id_lower.contains("deepseek");
    if is_deepseek_family
        && !info.capabilities.image_gen
        && !info.capabilities.video_gen
        && !info.capabilities.audio_gen
        && !info.capabilities.music_gen
        && !id_lower.contains("embed")
        && !id_lower.contains("tts")
        && !id_lower.contains("whisper")
    {
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
                    persistent_instructions: None,
                });
            }
            changed = true;
        }
    }

    if !info.capabilities.web && looks_like_native_web_model(kind, id_lower, info) {
        info.capabilities.web = true;
        changed = true;
    }

    changed
}

/// Google Gemini/Gemma 聊天、Perplexity / OpenAI search 等原生联网模型。
fn looks_like_native_web_model(kind: &str, id_lower: &str, info: &ModelInfo) -> bool {
    if info.capabilities.image_gen
        && !info.capabilities.tools
        && !info.capabilities.vision
        && !info.capabilities.reasoning
    {
        // 纯媒体输出模型不标联网
        return false;
    }
    if id_lower.contains("embed")
        || id_lower.contains("tts")
        || id_lower.contains("whisper")
        || id_lower.contains("veo")
        || id_lower.contains("lyria")
        || id_lower.contains("imagen")
        || id_lower.contains("robotics")
        || id_lower.contains("dall-e")
        || id_lower.contains("sora")
    {
        return false;
    }
    // 生图专用（如 gemini-*-flash-image）
    if id_lower.contains("image")
        && !id_lower.contains("vision")
        && (id_lower.contains("-image") || id_lower.ends_with("image"))
    {
        return false;
    }

    if id_lower.contains("sonar")
        || id_lower.contains("search-preview")
        || id_lower.contains(":online")
        || id_lower.contains("perplexity")
        || kind == "perplexity"
    {
        return true;
    }

    let googleish = kind == "google"
        || id_lower.contains("gemini")
        || id_lower.contains("gemma")
        || id_lower.starts_with("google/");
    if googleish {
        return id_lower.contains("gemini") || id_lower.contains("gemma");
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_catalog_accepts_tool_mode() {
        let mut info: ModelInfo = serde_json::from_value(serde_json::json!({
            "id": "gpt-test",
            "tool_mode": "code_mode_only"
        }))
        .unwrap();
        enrich_model_info(&mut info, "openai", None);
        assert_eq!(info.tool_mode, Some(types::ToolMode::CodeModeOnly));
    }

    #[test]
    fn model_catalog_ignores_unknown_tool_mode() {
        let info: ModelInfo = serde_json::from_value(serde_json::json!({
            "id": "gpt-future",
            "tool_mode": "future_tool_mode"
        }))
        .unwrap();
        assert_eq!(info.tool_mode, None);
    }

    #[test]
    fn model_catalog_keeps_persistent_reasoning_only_for_openai() {
        let value = serde_json::json!({
            "id": "gpt-test",
            "reasoning": {
                "supported_efforts": ["high"],
                "persistent_instructions": "  keep working  "
            }
        });
        let mut openai: ModelInfo =
            serde_json::from_value(value.clone()).expect("OpenAI model metadata");
        enrich_model_info(&mut openai, "openai", None);
        let reasoning = openai.reasoning.expect("OpenAI persistent metadata");
        assert_eq!(
            reasoning.persistent_instructions.as_deref(),
            Some("keep working")
        );
        assert!(reasoning.supported_efforts.contains(&"persistent".into()));

        let mut azure: ModelInfo = serde_json::from_value(value).expect("Azure model metadata");
        enrich_model_info(&mut azure, "azure", None);
        assert!(azure
            .reasoning
            .as_ref()
            .is_none_or(|reasoning| reasoning.persistent_instructions.is_none()));
    }
    use crate::meta::openrouter_meta;

    #[test]
    fn google_api_only_without_openrouter() {
        openrouter_meta::with_fixture(r#"{"data":[]}"#, || {
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
            assert!(info.meta_source.contains("api"));
            assert!(info.meta_source.contains("known"));
            assert!(!info.capabilities.vision);
            assert!(info.capabilities.web);
        });
    }

    #[test]
    fn openrouter_enriches_deepseek() {
        openrouter_meta::with_fixture(
            r#"{
              "data": [{
                "id": "deepseek/deepseek-chat",
                "name": "DeepSeek Chat",
                "description": "A chat model",
                "canonical_slug": "deepseek/deepseek-chat",
                "knowledge_cutoff": "2024-07-01",
                "context_length": 131072,
                "architecture": {
                  "input_modalities": ["text", "file"],
                  "output_modalities": ["text"]
                },
                "pricing": {
                  "prompt": "0.0000002",
                  "completion": "0.0000008",
                  "input_cache_read": "0.00000004"
                },
                "default_parameters": { "temperature": 1.0, "top_p": 1.0 },
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
                assert!(info.capabilities.file);
                assert!(!info.capabilities.reasoning);
                assert_eq!(info.description.as_deref(), Some("A chat model"));
                assert_eq!(
                    info.canonical_slug.as_deref(),
                    Some("deepseek/deepseek-chat")
                );
                assert_eq!(info.knowledge_cutoff.as_deref(), Some("2024-07-01"));
                let p = info.pricing.expect("pricing");
                assert!((p.prompt_per_million.unwrap() - 0.2).abs() < 1e-9);
                assert!((p.completion_per_million.unwrap() - 0.8).abs() < 1e-9);
                assert_eq!(info.meta_source, "openrouter");
            },
        );
    }

    #[test]
    fn official_deepseek_catalog_overrides_stale_and_openrouter_caps() {
        openrouter_meta::with_fixture(
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
                    profile: types::ModelProfile::default(),
                    tool_mode: None,
                    display_name: None,
                    description: None,
                    canonical_slug: None,
                    knowledge_cutoff: None,
                    expiration_date: None,
                    created: None,
                    hugging_face_id: None,
                    is_moderated: None,
                    context_window: Some(128_000),
                    max_output_tokens: None,
                    capabilities: ModelCapabilities {
                        vision: true,
                        ..Default::default()
                    },
                    reasoning: None,
                    pricing: None,
                    default_parameters: None,
                    meta_source: "heuristic+table".into(),
                };
                enrich_model_info(&mut stale, "deepseek", None);
                assert_eq!(stale.context_window, Some(1_048_576));
                assert!(!stale.capabilities.vision);
                assert!(stale.capabilities.tools);
                assert!(stale.capabilities.reasoning);
                assert_eq!(stale.meta_source, "deepseek_catalog+openrouter");
                let r = stale.reasoning.expect("reasoning meta");
                assert_eq!(r.default_effort.as_deref(), Some("high"));
                assert_eq!(r.supported_efforts, ["low", "high", "max"]);
            },
        );
    }

    #[test]
    fn deepseek_v4_known_reasoning_when_openrouter_misses() {
        openrouter_meta::with_fixture(r#"{"data":[]}"#, || {
            let info = enrich_from_id("deepseek-v4-pro", "deepseek", None);
            assert!(info.capabilities.reasoning);
            assert_eq!(info.context_window, Some(1_048_576));
            assert_eq!(info.usable_context_window(), Some(996_147));
            assert_eq!(info.meta_source, "deepseek_catalog");
            assert!(info.profile.supports_search_tool);
            assert!(info.profile.supports_parallel_tool_calls);
            assert!(info.profile.support_verbosity);
            assert_eq!(
                info.profile.default_verbosity,
                Some(types::ModelVerbosity::Low)
            );
            assert_eq!(
                info.profile.apply_patch_tool_type,
                Some(types::ApplyPatchToolType::Freeform)
            );
            assert!(info.profile.supports_reasoning_summaries);
            assert_eq!(
                info.profile.multi_agent_version,
                Some(types::ModelMultiAgentVersion::V2)
            );
            assert_eq!(info.profile.effective_context_window_percent, 95);
            assert_eq!(
                info.reasoning.unwrap().supported_efforts,
                ["low", "high", "max"]
            );
        });
    }

    #[test]
    fn deepseek_official_catalog_distinguishes_vision_model() {
        openrouter_meta::with_fixture(r#"{"data":[]}"#, || {
            let text = enrich_from_id("deepseek-v4-flash", "deepseek", None);
            let vision = enrich_from_id("deepseek-v4-flash-vision-exp", "deepseek", None);
            assert!(!text.capabilities.vision);
            assert_eq!(
                text.profile.input_modalities,
                [types::ModelInputModality::Text]
            );
            assert!(vision.capabilities.vision);
            assert_eq!(
                vision.profile.input_modalities,
                [
                    types::ModelInputModality::Text,
                    types::ModelInputModality::Image,
                ]
            );
        });
    }

    #[test]
    fn no_hardcode_when_both_miss() {
        openrouter_meta::with_fixture(r#"{"data":[]}"#, || {
            let info = enrich_from_id("gpt-4o-2024-08-06", "openai", None);
            assert_eq!(info.context_window, None);
            assert!(!info.capabilities.vision);
            assert!(!info.capabilities.tools);
            assert!(info.meta_source.is_empty());
        });
    }

    #[test]
    fn gpt_image_25_is_selectable_without_openrouter_metadata() {
        openrouter_meta::with_fixture(r#"{"data":[]}"#, || {
            for kind in ["azure", "openai", "openrouter"] {
                for id in [
                    "gpt-image-2.5-flare",
                    "gpt-image-2.5-sunburst",
                    "gpt-image-2.5-flare-2026-09-08",
                    "openai/gpt-image-2.5-sunburst-2026-09-08",
                    "gpt-image-2",
                ] {
                    let mut info = enrich_from_id(id, kind, None);
                    assert!(info.capabilities.image_gen, "{kind}/{id}");
                    assert!(!info.capabilities.tools);
                    assert!(!info.capabilities.vision);
                    assert!(!info.capabilities.reasoning);
                    assert_eq!(info.pricing, None);
                    assert_eq!(info.meta_source, "known");
                    enrich_model_info(&mut info, kind, None);
                    assert!(info.capabilities.image_gen, "cache refresh: {id}");
                }
            }
            for id in ["gpt-image-2.5-unknown", "not-gpt-image-2.5-flare"] {
                assert!(!enrich_from_id(id, "azure", None).capabilities.image_gen);
            }
        });
    }

    #[test]
    fn api_without_openrouter_keeps_api_only() {
        openrouter_meta::with_fixture(r#"{"data":[]}"#, || {
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
        openrouter_meta::with_fixture(
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
        openrouter_meta::with_fixture(
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
        openrouter_meta::with_fixture(
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
                assert!(info.capabilities.web);
                assert!(!info.capabilities.video_gen);
            },
        );
    }

    #[test]
    fn google_image_model_does_not_get_web() {
        openrouter_meta::with_fixture(r#"{"data":[]}"#, || {
            let info = enrich_from_id("gemini-2.5-flash-image", "google", None);
            assert!(!info.capabilities.web);
        });
    }

    #[test]
    fn openrouter_web_search_options_sets_web() {
        openrouter_meta::with_fixture(
            r#"{
              "data": [{
                "id": "openai/gpt-4o",
                "name": "GPT-4o",
                "context_length": 128000,
                "architecture": {
                  "input_modalities": ["text", "image"],
                  "output_modalities": ["text"]
                },
                "supported_parameters": ["tools", "web_search_options"],
                "reasoning": null
              }]
            }"#,
            || {
                let info = enrich_from_id("gpt-4o", "openai", None);
                assert!(info.capabilities.web);
                assert!(info.capabilities.vision);
            },
        );
    }

    #[test]
    fn openrouter_video_output_sets_video_gen() {
        openrouter_meta::with_fixture(
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
        openrouter_meta::with_fixture(
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
