//! 模型能力与上下文窗口：主要来自提供商 API 与 LiteLLM 表。
//!
//! 对目录已知漏标（如 DeepSeek V4 的 `supports_reasoning`）可做有限的 known 补丁，
//! 不做泛化名称猜测。

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
        let api_has_output = hints.as_ref().and_then(|h| h.max_output_tokens).is_some()
            || retained_api_out.is_some();
        if let Some(n) = entry.max_output_tokens {
            if !api_has_output {
                info.max_output_tokens = Some(n);
            }
        }

        let mode = entry.mode.as_deref().unwrap_or("").to_lowercase();
        let mode_image = mode.contains("image");
        let mode_video = mode.contains("video");
        let mode_music = mode.contains("music") || mode == "audio_generation";
        let audio_only_chat = mode == "chat"
            && entry.supports_audio_output
            && !entry.supports_function_calling
            && entry.supported_output_modalities.len() == 1
            && entry.supported_output_modalities[0].eq_ignore_ascii_case("audio");
        let is_music = mode_music || audio_only_chat;
        let mode_audio = mode.contains("audio") && !is_music;
        let non_chat = mode.contains("embed")
            || mode_image
            || mode_audio
            || mode_video
            || is_music
            || mode.contains("moderation");

        if non_chat {
            info.capabilities.vision = false;
            info.capabilities.web = false;
            info.capabilities.reasoning = false;
            info.capabilities.tools = false;
            info.capabilities.image_gen |= mode_image || entry.supports_image_generation;
            info.capabilities.video_gen |= mode_video || entry.supports_video_generation;
            info.capabilities.music_gen |= is_music;
            if !is_music {
                info.capabilities.audio_gen |= mode_audio || entry.supports_audio_output;
            }
        } else {
            info.capabilities.vision |= entry.supports_vision;
            info.capabilities.web |= entry.supports_web_search;
            info.capabilities.reasoning |= entry.supports_reasoning;
            info.capabilities.tools |= entry.supports_function_calling;
            info.capabilities.image_gen |= entry.supports_image_generation;
            info.capabilities.video_gen |= entry.supports_video_generation;
            info.capabilities.music_gen |= is_music;
            if !is_music {
                info.capabilities.audio_gen |= entry.supports_audio_output;
            }
        }
        sources.push("litellm");
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
/// DeepSeek V4 的 thinking 是请求级参数（`thinking` / `reasoning_effort`），官方
/// LiteLLM `deepseek-v4-*` 条目的 `supports_reasoning` 仍为 null；Azure 等镜像已标 true。
fn apply_known_capability_overrides(kind: &str, id_lower: &str, info: &mut ModelInfo) -> bool {
    let is_deepseek_family = kind == "deepseek" || id_lower.contains("deepseek");
    if !is_deepseek_family {
        return false;
    }
    // 非 chat（embed 等）不打补丁
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
        return true;
    }
    false
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
                // LiteLLM 未标 supports_reasoning 时，known 补丁补上 V4 thinking
                assert!(stale.capabilities.reasoning);
                assert_eq!(stale.meta_source, "known+litellm");
            },
        );
    }

    #[test]
    fn deepseek_v4_known_reasoning_without_litellm_flag() {
        crate::litellm_meta::with_fixture(
            r#"{
              "deepseek-v4-pro": {
                "max_input_tokens": 1000000,
                "supports_function_calling": true,
                "mode": "chat",
                "litellm_provider": "deepseek"
              }
            }"#,
            || {
                let info = enrich_from_id("deepseek-v4-pro", "deepseek", None);
                assert!(info.capabilities.tools);
                assert!(info.capabilities.reasoning);
                assert!(info.meta_source.contains("known"));
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

    #[test]
    fn litellm_image_mode_sets_image_gen() {
        crate::litellm_meta::with_fixture(
            r#"{
              "dall-e-3": {
                "mode": "image_generation",
                "litellm_provider": "openai"
              }
            }"#,
            || {
                let info = enrich_from_id("dall-e-3", "openai", None);
                assert!(info.capabilities.image_gen);
                assert!(!info.capabilities.tools);
                assert!(!info.capabilities.vision);
                assert_eq!(info.meta_source, "litellm");
            },
        );
    }

    #[test]
    fn litellm_audio_output_sets_audio_gen() {
        crate::litellm_meta::with_fixture(
            r#"{
              "gpt-4o-mini-tts": {
                "mode": "audio_speech",
                "supports_audio_output": true,
                "litellm_provider": "openai"
              }
            }"#,
            || {
                let info = enrich_from_id("gpt-4o-mini-tts", "openai", None);
                assert!(info.capabilities.audio_gen);
                assert!(!info.capabilities.tools);
            },
        );
    }

    #[test]
    fn litellm_chat_explicit_media_flags() {
        crate::litellm_meta::with_fixture(
            r#"{
              "gemini-2.0-flash": {
                "mode": "chat",
                "supports_function_calling": true,
                "supports_vision": true,
                "supports_image_generation": true,
                "litellm_provider": "gemini"
              }
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
    fn litellm_video_mode_sets_video_gen() {
        crate::litellm_meta::with_fixture(
            r#"{
              "sora-2": {
                "mode": "video_generation",
                "litellm_provider": "openai"
              }
            }"#,
            || {
                let info = enrich_from_id("sora-2", "openai", None);
                assert!(info.capabilities.video_gen);
                assert!(!info.capabilities.tools);
            },
        );
    }

    #[test]
    fn litellm_music_mode_sets_music_gen_only() {
        crate::litellm_meta::with_fixture(
            r#"{"lyria-test":{"mode":"music_generation","litellm_provider":"gemini"}}"#,
            || {
                let info = enrich_from_id("lyria-test", "google", None);
                assert!(info.capabilities.music_gen);
                assert!(!info.capabilities.audio_gen);
                assert!(!info.capabilities.tools);
            },
        );
    }

    #[test]
    fn litellm_audio_only_chat_sets_music_gen() {
        crate::litellm_meta::with_fixture(
            r#"{"gemini/lyria-test":{"mode":"chat","supports_audio_output":true,"supports_function_calling":false,"supported_output_modalities":["audio"],"litellm_provider":"gemini"}}"#,
            || {
                let info = enrich_from_id("lyria-test", "google", None);
                assert!(info.capabilities.music_gen);
                assert!(!info.capabilities.audio_gen);
            },
        );
    }

    #[test]
    fn litellm_tts_mode_does_not_set_music_gen() {
        crate::litellm_meta::with_fixture(
            r#"{"tts-test":{"mode":"audio_speech","supports_audio_output":true,"litellm_provider":"gemini"}}"#,
            || {
                let info = enrich_from_id("tts-test", "google", None);
                assert!(info.capabilities.audio_gen);
                assert!(!info.capabilities.music_gen);
            },
        );
    }
}
