//! 供应商能力分发 — 外部调用者的唯一入口。
//!
//! 所有 provider 操作（聊天、媒体、验证、元数据）通过本模块的公开函数访问。
//! 内部使用 trait 系统（`Registry` + `DynProvider`）路由到具体实现。

use serde_json::Value;

use crate::types::error::{ProviderError, ProviderResult};
use crate::types::media::{GeneratedAudio, GeneratedImage, GeneratedVideo};
use crate::types::message::{Message, ToolDefinition};
use crate::types::request::{CompletionRequest, ProviderConfig, ThinkingConfig, ToolChoice};
use crate::types::stream::CompletionStream;

/// 协议管线分发。
///
/// 接受 `CompletionRequest`，返回 `CompletionStream`。
pub async fn chat_stream_direct(
    provider: &str,
    request: CompletionRequest,
    config: &ProviderConfig,
) -> ProviderResult<CompletionStream> {
    let provider = normalize_provider_id(provider);
    let mut reg = crate::registry::Registry::new();
    register_provider(&mut reg, provider, config);

    let dyn_model = reg
        .completion_model(provider)
        .ok_or_else(|| ProviderError::UnknownProvider(provider.to_string()))?;

    dyn_model
        .stream(request)
        .await
        .map_err(ProviderError::Other)
}

/// 聊天补全 — 接受旧签名（messages + tools JSON + config）。
///
/// 大多数调用者使用此函数。tools 为 OpenAI 格式 JSON。
pub async fn chat_stream(
    provider: &str,
    messages: Vec<Message>,
    tools: Vec<Value>,
    config: &ProviderConfig,
) -> ProviderResult<CompletionStream> {
    chat_stream_with_tool_policy(provider, messages, tools, config, None, None).await
}

pub(crate) async fn chat_stream_with_tool_policy(
    provider: &str,
    messages: Vec<Message>,
    tools: Vec<Value>,
    config: &ProviderConfig,
    tool_choice: Option<ToolChoice>,
    parallel_tool_calls: Option<bool>,
) -> ProviderResult<CompletionStream> {
    let tool_defs: Vec<ToolDefinition> = tools
        .iter()
        .filter_map(|t| {
            let f = t.get("function").unwrap_or(t);
            Some(ToolDefinition {
                name: f.get("name")?.as_str()?.to_string(),
                description: f
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string(),
                parameters: f
                    .get("parameters")
                    .cloned()
                    .unwrap_or(serde_json::json!({"type": "object", "properties": {}})),
            })
        })
        .collect();

    let request = CompletionRequest {
        model: config.model.clone(),
        messages,
        tools: tool_defs,
        tool_choice,
        parallel_tool_calls,
        temperature: Some(config.temperature),
        max_tokens: Some(config.max_tokens),
        thinking: Some(ThinkingConfig {
            enabled: config.thinking_enabled,
            budget_tokens: None,
            effort: config.reasoning_effort.clone(),
        }),
        additional_params: config.additional_params.clone(),
        previous_interaction_id: config.previous_interaction_id.clone(),
    };

    chat_stream_direct(provider, request, config).await
}

/// 图片生成。按 profile 的 `image_mode` 路由到对应 HTTP 模块。
pub async fn generate_image(
    provider: &str,
    prompt: &str,
    config: &ProviderConfig,
) -> ProviderResult<Vec<GeneratedImage>> {
    let provider = normalize_provider_id(provider);
    let profile = crate::profile::resolve_or_openai_compat(provider);
    let mode = profile
        .image_mode
        .ok_or_else(|| ProviderError::UnsupportedCapability {
            provider: provider.to_string(),
            capability: "图片生成".to_string(),
        })?;
    let mut cfg = config.clone();
    if cfg.model.trim().is_empty() && !profile.default_image_model.is_empty() {
        cfg.model = profile.default_image_model.to_string();
    }
    let client = shared_http_client();
    match mode {
        crate::profile::ImageGenMode::OpenAi => {
            Ok(crate::openai::image_http::openai_generate_image(&client, prompt, &cfg).await?)
        }
        crate::profile::ImageGenMode::GoogleInteractions => {
            let req = crate::google::interactions_http::InteractionImageRequest {
                prompt: prompt.to_string(),
                ..Default::default()
            };
            let result =
                crate::google::interactions_http::google_interactions_image(&client, &cfg, &req)
                    .await?;
            Ok(vec![result.image])
        }
        crate::profile::ImageGenMode::MiniMax => {
            let req = crate::minimax::image_http::MiniMaxImageRequest {
                prompt: prompt.to_string(),
                ..Default::default()
            };
            Ok(crate::minimax::image_http::minimax_generate_image(&client, &cfg, &req).await?)
        }
    }
}

/// 语音合成（TTS）。
pub async fn text_to_speech(
    provider: &str,
    text: &str,
    config: &ProviderConfig,
) -> ProviderResult<GeneratedAudio> {
    let provider = normalize_provider_id(provider);
    let profile = crate::profile::resolve_or_openai_compat(provider);
    if !profile.supports_tts() {
        return Err(ProviderError::UnsupportedCapability {
            provider: provider.to_string(),
            capability: "语音合成 (TTS)".to_string(),
        });
    }
    let mut cfg = config.clone();
    if cfg.model.trim().is_empty() {
        cfg.model = profile.default_tts_model.to_string();
    }
    let client = shared_http_client();
    match provider {
        "minimax" => {
            let req = crate::minimax::tts_http::MiniMaxTtsRequest {
                text: text.to_string(),
                ..Default::default()
            };
            let r = crate::minimax::tts_http::minimax_tts(&client, &cfg, &req).await?;
            Ok(GeneratedAudio {
                data: r.audio_bytes,
                mime_type: r.mime_type,
                duration_ms: r.duration_ms,
            })
        }
        _ => {
            let req = crate::openai::tts_http::OpenAiTtsRequest {
                model: cfg.model.clone(),
                input: text.to_string(),
                voice: "alloy".to_string(),
                response_format: "mp3".to_string(),
                speed: 1.0,
            };
            let r = crate::openai::tts_http::openai_tts(&client, &cfg, &req).await?;
            Ok(GeneratedAudio {
                data: r.audio_bytes,
                mime_type: r.mime_type,
                duration_ms: 0,
            })
        }
    }
}

/// 视频生成扩展选项。
#[derive(Debug, Clone, Default)]
pub struct VideoGenOptions {
    pub prompt: String,
    /// 首帧图片（data URI 或 URL）。
    pub first_frame_image: Option<String>,
    /// 末帧图片（data URI 或 URL）。
    pub last_frame_image: Option<String>,
    /// H3 参考图片 URL（最多 9 张）。
    pub reference_images: Vec<String>,
    /// H3 参考视频 URL（最多 3 个）。
    pub reference_videos: Vec<String>,
    /// H3 参考音频 URL（最多 3 个）。
    pub reference_audios: Vec<String>,
    /// 时长（秒）。
    pub duration: Option<u32>,
    /// 分辨率：`"768P"` / `"2K"` / `"720P"` / `"1080P"`。
    pub resolution: Option<String>,
    /// 画面比例：`"16:9"` / `"9:16"` 等。
    pub ratio: Option<String>,
    /// 是否启用提示词优化（默认 true）。
    pub prompt_optimizer: Option<bool>,
    /// 是否先做 H3 Context-IR 提示词增强。
    pub enhance_prompt: bool,
}

/// 视频生成（简单接口，只传 prompt）。
pub async fn generate_video(
    provider: &str,
    prompt: &str,
    config: &ProviderConfig,
) -> ProviderResult<GeneratedVideo> {
    generate_video_with_options(
        provider,
        &VideoGenOptions {
            prompt: prompt.to_string(),
            ..Default::default()
        },
        config,
    )
    .await
}

/// 视频生成（完整接口，支持 H3 全部特性）。
pub async fn generate_video_with_options(
    provider: &str,
    options: &VideoGenOptions,
    config: &ProviderConfig,
) -> ProviderResult<GeneratedVideo> {
    let provider = normalize_provider_id(provider);
    let profile = crate::profile::resolve_or_openai_compat(provider);
    if !profile.supports_video() {
        return Err(ProviderError::UnsupportedCapability {
            provider: provider.to_string(),
            capability: "视频生成".to_string(),
        });
    }
    let mut cfg = config.clone();
    if cfg.model.trim().is_empty() {
        cfg.model = profile.default_video_model.to_string();
    }
    let client = shared_http_client();
    match provider {
        "minimax" => {
            let model_name = cfg.model.clone();
            let duration = options.duration.unwrap_or(6);
            let resolution = options
                .resolution
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("768P")
                .to_string();

            let mut req = crate::minimax::video_http::MiniMaxVideoRequest {
                prompt: options.prompt.clone(),
                model: model_name.clone(),
                first_frame_image: options.first_frame_image.clone(),
                last_frame_image: options.last_frame_image.clone(),
                reference_images: options.reference_images.clone(),
                reference_videos: options.reference_videos.clone(),
                reference_audios: options.reference_audios.clone(),
                duration,
                resolution,
                ratio: options.ratio.clone(),
                prompt_optimizer: options.prompt_optimizer.unwrap_or(true),
                ..Default::default()
            };

            // H3 Context-IR 提示词增强
            if options.enhance_prompt && crate::minimax::video_http::is_h3_model(&model_name) {
                if let Ok(ir) =
                    crate::minimax::video_http::minimax_enhance_prompt(&client, &cfg, &req).await
                {
                    if !ir.enhanced_prompt.is_empty() {
                        req.prompt = ir.enhanced_prompt;
                    }
                }
            }

            let task_id =
                crate::minimax::video_http::minimax_create_video(&client, &cfg, &req).await?;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10 * 60);
            loop {
                if std::time::Instant::now() > deadline {
                    return Err(ProviderError::Timeout {
                        operation: "MiniMax 视频生成".to_string(),
                        detail: format!("task_id={task_id}"),
                    });
                }
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                let status = crate::minimax::video_http::minimax_query_video(
                    &client,
                    &cfg,
                    &task_id,
                    &model_name,
                )
                .await?;
                if status.status == crate::minimax::video_http::VideoTaskStatus::Success {
                    if let Some(ref url) = status.download_url {
                        let result =
                            crate::minimax::video_http::minimax_download_video_url(&client, url)
                                .await?;
                        break Ok(GeneratedVideo {
                            data: result.data,
                            mime_type: result.mime_type,
                            width: result.width,
                            height: result.height,
                        });
                    }
                    if let Some(ref file_id) = status.file_id {
                        let result = crate::minimax::video_http::minimax_download_video(
                            &client, &cfg, file_id,
                        )
                        .await?;
                        break Ok(GeneratedVideo {
                            data: result.data,
                            mime_type: result.mime_type,
                            width: status.video_width.unwrap_or(0),
                            height: status.video_height.unwrap_or(0),
                        });
                    }
                    return Err(ProviderError::ModelError {
                        provider: "minimax".to_string(),
                        detail: "视频生成成功但无下载途径".to_string(),
                    });
                }
                if !status.status.is_pending() {
                    return Err(ProviderError::ModelError {
                        provider: "minimax".to_string(),
                        detail: format!("视频生成失败: {:?}", status.status),
                    });
                }
            }
        }
        _ => Err(ProviderError::UnsupportedCapability {
            provider: provider.to_string(),
            capability: "视频生成".to_string(),
        }),
    }
}

/// 音乐生成。
pub async fn generate_music(
    provider: &str,
    prompt: &str,
    config: &ProviderConfig,
) -> ProviderResult<GeneratedAudio> {
    let provider = normalize_provider_id(provider);
    let profile = crate::profile::resolve_or_openai_compat(provider);
    if !profile.supports_music() {
        return Err(ProviderError::UnsupportedCapability {
            provider: provider.to_string(),
            capability: "音乐生成".to_string(),
        });
    }
    let mut cfg = config.clone();
    if cfg.model.trim().is_empty() {
        cfg.model = profile.default_music_model.to_string();
    }
    let client = shared_http_client();
    match provider {
        "minimax" => {
            let req = crate::minimax::music_http::MiniMaxMusicRequest {
                prompt: prompt.to_string(),
                ..Default::default()
            };
            let r = crate::minimax::music_http::minimax_generate_music(&client, &cfg, &req).await?;
            Ok(GeneratedAudio {
                data: r.audio_bytes,
                mime_type: r.mime_type,
                duration_ms: r.duration_ms,
            })
        }
        _ => Err(ProviderError::UnsupportedCapability {
            provider: provider.to_string(),
            capability: "音乐生成".to_string(),
        }),
    }
}

/// 文本嵌入。
pub async fn embed(
    provider: &str,
    texts: &[String],
    config: &ProviderConfig,
) -> ProviderResult<Vec<Vec<f32>>> {
    let provider = normalize_provider_id(provider);
    let profile = crate::profile::resolve_or_openai_compat(provider);
    if !profile.supports_embedding {
        return Err(ProviderError::UnsupportedCapability {
            provider: provider.to_string(),
            capability: "文本嵌入".to_string(),
        });
    }
    let mut cfg = config.clone();
    if cfg.model.trim().is_empty() && !profile.default_embedding_model.is_empty() {
        cfg.model = profile.default_embedding_model.to_string();
    }
    let client = shared_http_client();
    Ok(
        crate::openai::embeddings_http::openai_batch_embed(&client, texts, &cfg.model, &cfg)
            .await?,
    )
}

/// 连通性验证。
pub async fn verify(
    provider: &str,
    model: &str,
    config: &ProviderConfig,
) -> crate::shared::verify::VerifyResult {
    let client = shared_http_client();
    crate::verify::probe(&client, provider, model, config).await
}

/// 查询 provider 默认模型名。
pub fn default_model(provider: &str) -> String {
    crate::profile::default_chat_model(provider).to_string()
}

/// 查询 provider 是否支持图片生成。
pub fn supports_image_gen(provider: &str) -> bool {
    crate::profile::resolve(provider).is_some_and(|p| p.supports_image_gen)
}

fn shared_http_client() -> reqwest::Client {
    crate::registry::shared_http_client()
}

fn normalize_provider_id(id: &str) -> &str {
    crate::profile::normalize_provider_id(id)
}

/// 是否应使用 Responses API 协议。
///
/// 优先级：`config.api_mode` 显式指定 > profile `supports_responses` 默认。
/// - `api_mode == "chat"` → 强制 ChatCompletions
/// - `api_mode == "responses"` → 强制 Responses
/// - `api_mode` 为空 → profile.supports_responses 决定
fn use_responses(provider: &str, config: &ProviderConfig) -> bool {
    match config.api_mode.as_str() {
        "chat" => false,
        "responses" => true,
        _ => crate::profile::resolve(provider).is_some_and(|p| p.supports_responses),
    }
}

/// 根据 provider id 注册到注册表。
fn register_provider(reg: &mut crate::registry::Registry, provider: &str, config: &ProviderConfig) {
    let key = &config.api_key;
    let base = config.base_url.as_deref().filter(|s| !s.trim().is_empty());
    let model = &config.model;
    let responses = use_responses(provider, config);

    match provider {
        "anthropic" | "claude" => reg.register_anthropic(key, base, model),
        "google" => reg.register_google(key, base, model),
        "openai" if responses => reg.register_openai_responses(key, base, model),
        "openai" => reg.register_openai(key, base, model),
        "deepseek" if responses => reg
            .register_openai_compat_responses::<crate::impls::deepseek::DeepSeek>(
                "deepseek", key, base, model,
            ),
        "deepseek" => register_compat::<crate::impls::deepseek::DeepSeek>(reg, key, base, model),
        "azure" => register_compat::<crate::impls::azure::Azure>(reg, key, base, model),
        "zhipu" => register_media::<crate::impls::zhipu::Zhipu>(reg, key, base, model),
        "moonshot" => register_compat::<crate::impls::moonshot::Moonshot>(reg, key, base, model),
        "ollama" => register_compat::<crate::impls::ollama::Ollama>(reg, key, base, model),
        "nvidia" => register_compat::<crate::impls::nvidia::Nvidia>(reg, key, base, model),
        "bailian" => register_media::<crate::impls::bailian::Bailian>(reg, key, base, model),
        "volcengine" => {
            register_media::<crate::impls::volcengine::Volcengine>(reg, key, base, model)
        }
        "openrouter" => {
            register_compat::<crate::impls::openrouter::OpenRouter>(reg, key, base, model)
        }
        "minimax" | "minmax" if responses => reg.register_minimax_responses(key, base, model),
        "minimax" | "minmax" => reg.register_minimax(key, base, model),
        "minimax-anthropic" => {
            reg.register_anthropic(key, base, model);
            reg.register_alias("minimax-anthropic", "anthropic");
        }
        "hunyuan" => register_media::<crate::impls::hunyuan::Hunyuan>(reg, key, base, model),
        "mimo" => register_compat::<crate::impls::mimo::Mimo>(reg, key, base, model),
        "gemini-native" => reg.register_gemini_native(key, base, model),
        other => {
            if let Some(custom_cfg) = lookup_custom_provider(other) {
                reg.register_custom(other, &custom_cfg, key, model);
            } else {
                register_compat::<crate::impls::openai::OpenAI>(reg, key, base, model);
            }
        }
    }
}

/// 从 ~/.astro/config.toml 查找自定义 provider 配置。
///
/// 每次调用都重新读取文件，支持运行时修改 config.toml 后无需重启。
/// 文件读取开销极低（< 1ms），且仅在 dispatch fallback 分支触发。
fn lookup_custom_provider(id: &str) -> Option<crate::custom::CustomProviderConfig> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    let path = std::path::PathBuf::from(home)
        .join(".astro")
        .join("config.toml");
    let map = crate::custom::load_custom_providers(&path);
    map.get(id).cloned()
}

fn register_compat<Ext>(
    reg: &mut crate::registry::Registry,
    api_key: &str,
    base_url: Option<&str>,
    model: &str,
) where
    Ext: crate::compat::OpenAICompatible
        + crate::traits::ProviderExt
        + crate::traits::Capabilities<
            Chat = crate::traits::Capable<crate::compat::OpenAICompletionModel<Ext>>,
        > + Default
        + Copy
        + 'static,
{
    reg.register_openai_compat::<Ext>(api_key, base_url, model);
}

fn register_media<Ext>(
    reg: &mut crate::registry::Registry,
    api_key: &str,
    base_url: Option<&str>,
    model: &str,
) where
    Ext: crate::compat::OpenAICompatible
        + crate::traits::ProviderExt
        + crate::traits::Capabilities<
            Chat = crate::traits::Capable<crate::compat::OpenAICompletionModel<Ext>>,
            Embedding = crate::traits::Capable<crate::compat::media::CompatEmbeddingModel>,
            ImageGen = crate::traits::Capable<crate::compat::media::CompatImageGenModel>,
            TTS = crate::traits::Capable<crate::compat::media::CompatTTSModel>,
        > + Default
        + Copy
        + 'static,
{
    reg.register_compat_with_media::<Ext>(api_key, base_url, model);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_all_providers() {
        let config = ProviderConfig::default();
        let providers = [
            "openai",
            "anthropic",
            "claude",
            "deepseek",
            "google",
            "azure",
            "zhipu",
            "moonshot",
            "ollama",
            "nvidia",
            "bailian",
            "volcengine",
            "openrouter",
            "minimax",
            "hunyuan",
            "mimo",
            "gemini-native",
        ];
        for id in providers {
            let mut reg = crate::registry::Registry::new();
            register_provider(&mut reg, id, &config);
            assert!(
                reg.completion_model(id).is_some(),
                "provider {id} should have completion model"
            );
        }
    }

    #[test]
    fn minimax_variant_ids_resolve() {
        let config = ProviderConfig::default();
        for id in ["minimax-anthropic", "minmax", "minmax-anthropic"] {
            let normalized = normalize_provider_id(id);
            let mut reg = crate::registry::Registry::new();
            register_provider(&mut reg, normalized, &config);
            assert!(
                reg.completion_model(normalized).is_some(),
                "provider alias {id} (normalized to {normalized}) should resolve"
            );
        }
    }

    #[test]
    fn responses_default_for_supported_providers() {
        let config = ProviderConfig::default();
        for id in ["openai", "deepseek", "minimax"] {
            assert!(
                use_responses(id, &config),
                "{id} should default to Responses API"
            );
        }
        for id in ["anthropic", "google", "ollama", "azure", "zhipu"] {
            assert!(
                !use_responses(id, &config),
                "{id} should NOT default to Responses API"
            );
        }
    }

    #[test]
    fn api_mode_override() {
        let mut chat_config = ProviderConfig::default();
        chat_config.api_mode = "chat".to_string();
        assert!(
            !use_responses("openai", &chat_config),
            "api_mode=chat forces ChatCompletions"
        );

        let mut resp_config = ProviderConfig::default();
        resp_config.api_mode = "responses".to_string();
        assert!(
            use_responses("ollama", &resp_config),
            "api_mode=responses forces Responses"
        );
    }

    #[test]
    fn responses_providers_register_correctly() {
        let config = ProviderConfig::default();
        for id in ["openai", "deepseek", "minimax"] {
            let mut reg = crate::registry::Registry::new();
            register_provider(&mut reg, id, &config);
            assert!(
                reg.completion_model(id).is_some(),
                "provider {id} (default Responses) should have completion model"
            );
        }
    }
}
