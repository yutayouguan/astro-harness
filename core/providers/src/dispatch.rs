//! 供应商能力分发 — 外部调用者的唯一入口。
//!
//! 所有 provider 操作（聊天、媒体、验证、元数据）通过本模块的公开函数访问。
//! 内部使用 trait 系统（`Registry` + `DynProvider`）路由到具体实现。

use anyhow::Result;
use serde_json::Value;

use crate::types::message::{Message, ToolDefinition};
use crate::types::media::{GeneratedAudio, GeneratedImage, GeneratedVideo};
use crate::types::request::{CompletionRequest, ProviderConfig, ThinkingConfig};
use crate::types::stream::CompletionStream;

/// 协议管线分发。
///
/// 接受 `CompletionRequest`，返回 `CompletionStream`。
pub async fn chat_stream_direct(
    provider: &str,
    request: CompletionRequest,
    config: &ProviderConfig,
) -> Result<CompletionStream> {
    let provider = normalize_provider_id(provider);
    let mut reg = crate::registry::Registry::new();
    register_provider(&mut reg, provider, config);

    let dyn_model = reg
        .completion_model(provider)
        .ok_or_else(|| anyhow::anyhow!("未知 provider: {provider}"))?;

    dyn_model.stream(request).await
}

/// 聊天补全 — 接受旧签名（messages + tools JSON + config）。
///
/// 大多数调用者使用此函数。tools 为 OpenAI 格式 JSON。
pub async fn chat_stream(
    provider: &str,
    messages: Vec<Message>,
    tools: Vec<Value>,
    config: &ProviderConfig,
) -> Result<CompletionStream> {
    let tool_defs: Vec<ToolDefinition> = tools
        .iter()
        .filter_map(|t| {
            let f = t.get("function").unwrap_or(t);
            Some(ToolDefinition {
                name: f.get("name")?.as_str()?.to_string(),
                description: f.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string(),
                parameters: f.get("parameters").cloned().unwrap_or(serde_json::json!({"type": "object", "properties": {}})),
            })
        })
        .collect();

    let request = CompletionRequest {
        model: config.model.clone(),
        messages,
        tools: tool_defs,
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
) -> Result<Vec<GeneratedImage>> {
    let provider = normalize_provider_id(provider);
    let profile = crate::profile::resolve_or_openai_compat(provider);
    let mode = profile.image_mode
        .ok_or_else(|| anyhow::anyhow!("{provider} 不支持图片生成"))?;
    let mut cfg = config.clone();
    if cfg.model.trim().is_empty() && !profile.default_image_model.is_empty() {
        cfg.model = profile.default_image_model.to_string();
    }
    let client = shared_http_client();
    match mode {
        crate::profile::ImageGenMode::OpenAi => {
            crate::openai::image_http::openai_generate_image(&client, prompt, &cfg).await
        }
        crate::profile::ImageGenMode::GoogleInteractions => {
            let req = crate::google::interactions_http::InteractionImageRequest {
                prompt: prompt.to_string(),
                ..Default::default()
            };
            let result = crate::google::interactions_http::google_interactions_image(&client, &cfg, &req).await?;
            Ok(vec![result.image])
        }
        crate::profile::ImageGenMode::MiniMax => {
            let req = crate::minimax::image_http::MiniMaxImageRequest {
                prompt: prompt.to_string(),
                ..Default::default()
            };
            crate::minimax::image_http::minimax_generate_image(&client, &cfg, &req).await
        }
    }
}

/// 语音合成（TTS）。
pub async fn text_to_speech(
    provider: &str,
    text: &str,
    config: &ProviderConfig,
) -> Result<GeneratedAudio> {
    let provider = normalize_provider_id(provider);
    let profile = crate::profile::resolve_or_openai_compat(provider);
    if !profile.supports_tts() {
        anyhow::bail!("{provider} 不支持语音合成 (TTS)");
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
            Ok(GeneratedAudio { data: r.audio_bytes, mime_type: r.mime_type, duration_ms: r.duration_ms })
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
            Ok(GeneratedAudio { data: r.audio_bytes, mime_type: r.mime_type, duration_ms: 0 })
        }
    }
}

/// 视频生成。
pub async fn generate_video(
    provider: &str,
    prompt: &str,
    config: &ProviderConfig,
) -> Result<GeneratedVideo> {
    let provider = normalize_provider_id(provider);
    let profile = crate::profile::resolve_or_openai_compat(provider);
    if !profile.supports_video() {
        anyhow::bail!("{provider} 不支持视频生成");
    }
    let mut cfg = config.clone();
    if cfg.model.trim().is_empty() {
        cfg.model = profile.default_video_model.to_string();
    }
    let client = shared_http_client();
    match provider {
        "minimax" => {
            let req = crate::minimax::video_http::MiniMaxVideoRequest {
                prompt: prompt.to_string(),
                model: cfg.model.clone(),
                ..Default::default()
            };
            let task_id = crate::minimax::video_http::minimax_create_video(&client, &cfg, &req).await?;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10 * 60);
            loop {
                if std::time::Instant::now() > deadline {
                    anyhow::bail!("MiniMax 视频生成超时（task_id={task_id}）");
                }
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                let status = crate::minimax::video_http::minimax_query_video(&client, &cfg, &task_id).await?;
                if status.status == crate::minimax::video_http::VideoTaskStatus::Success {
                    if let Some(file_id) = status.file_id {
                        let result = crate::minimax::video_http::minimax_download_video(&client, &cfg, &file_id).await?;
                        break Ok(GeneratedVideo {
                            data: result.data,
                            mime_type: result.mime_type,
                            width: status.video_width.unwrap_or(0),
                            height: status.video_height.unwrap_or(0),
                        });
                    }
                    anyhow::bail!("MiniMax 视频生成成功但无 file_id");
                }
                if !status.status.is_pending() {
                    anyhow::bail!("MiniMax 视频生成失败: {:?}", status.status);
                }
            }
        }
        _ => anyhow::bail!("{provider} 的视频生成暂未对接"),
    }
}

/// 音乐生成。
pub async fn generate_music(
    provider: &str,
    prompt: &str,
    config: &ProviderConfig,
) -> Result<GeneratedAudio> {
    let provider = normalize_provider_id(provider);
    let profile = crate::profile::resolve_or_openai_compat(provider);
    if !profile.supports_music() {
        anyhow::bail!("{provider} 不支持音乐生成");
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
            Ok(GeneratedAudio { data: r.audio_bytes, mime_type: r.mime_type, duration_ms: r.duration_ms })
        }
        _ => anyhow::bail!("{provider} 的音乐生成暂未对接"),
    }
}

/// 文本嵌入。
pub async fn embed(
    provider: &str,
    texts: &[String],
    config: &ProviderConfig,
) -> Result<Vec<Vec<f32>>> {
    let provider = normalize_provider_id(provider);
    let profile = crate::profile::resolve_or_openai_compat(provider);
    if !profile.supports_embedding {
        anyhow::bail!("{provider} 不支持文本嵌入");
    }
    let mut cfg = config.clone();
    if cfg.model.trim().is_empty() && !profile.default_embedding_model.is_empty() {
        cfg.model = profile.default_embedding_model.to_string();
    }
    let client = shared_http_client();
    crate::openai::embeddings_http::openai_batch_embed(&client, texts, &cfg.model, &cfg).await
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
    crate::profile::resolve(provider).map_or(false, |p| p.supports_image_gen)
}

fn shared_http_client() -> reqwest::Client {
    crate::registry::shared_http_client()
}

fn normalize_provider_id(id: &str) -> &str {
    crate::profile::normalize_provider_id(id)
}

/// 根据 provider id 注册到注册表。
fn register_provider(
    reg: &mut crate::registry::Registry,
    provider: &str,
    config: &ProviderConfig,
) {
    let key = &config.api_key;
    let base = config.base_url.as_deref().filter(|s| !s.trim().is_empty());
    let model = &config.model;

    match provider {
        "anthropic" | "claude" => reg.register_anthropic(key, base, model),
        "google" => reg.register_google(key, base, model),
        "openai" => reg.register_openai(key, base, model),
        "deepseek" => register_compat::<crate::impls::deepseek::DeepSeek>(reg, key, base, model),
        "azure" => register_compat::<crate::impls::azure::Azure>(reg, key, base, model),
        "zhipu" => register_media::<crate::impls::zhipu::Zhipu>(reg, key, base, model),
        "moonshot" => register_compat::<crate::impls::moonshot::Moonshot>(reg, key, base, model),
        "ollama" => register_compat::<crate::impls::ollama::Ollama>(reg, key, base, model),
        "nvidia" => register_compat::<crate::impls::nvidia::Nvidia>(reg, key, base, model),
        "bailian" => register_media::<crate::impls::bailian::Bailian>(reg, key, base, model),
        "volcengine" => register_media::<crate::impls::volcengine::Volcengine>(reg, key, base, model),
        "openrouter" => register_compat::<crate::impls::openrouter::OpenRouter>(reg, key, base, model),
        "minimax" | "minmax" => reg.register_minimax(key, base, model),
        "hunyuan" => register_media::<crate::impls::hunyuan::Hunyuan>(reg, key, base, model),
        "mimo" => register_compat::<crate::impls::mimo::Mimo>(reg, key, base, model),
        "gemini-native" => reg.register_gemini_native(key, base, model),
        "openai-responses" => reg.register_responses("openai-responses", key, base, model),
        "minimax-responses" => reg.register_responses("minimax-responses", key, base, model),
        _ => register_compat::<crate::impls::openai::OpenAI>(reg, key, base, model),
    }
}

fn register_compat<Ext>(
    reg: &mut crate::registry::Registry,
    api_key: &str,
    base_url: Option<&str>,
    model: &str,
)
where
    Ext: crate::compat::OpenAICompatible
        + crate::traits::ProviderExt
        + crate::traits::Capabilities<Chat = crate::traits::Capable<crate::compat::OpenAICompletionModel<Ext>>>
        + Default
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
)
where
    Ext: crate::compat::OpenAICompatible
        + crate::traits::ProviderExt
        + crate::traits::Capabilities<
            Chat = crate::traits::Capable<crate::compat::OpenAICompletionModel<Ext>>,
            Embedding = crate::traits::Capable<crate::compat::media::CompatEmbeddingModel>,
            ImageGen = crate::traits::Capable<crate::compat::media::CompatImageGenModel>,
            TTS = crate::traits::Capable<crate::compat::media::CompatTTSModel>,
        >
        + Default
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
            "openai", "anthropic", "claude", "deepseek", "google",
            "azure", "zhipu", "moonshot", "ollama", "nvidia",
            "bailian", "volcengine", "openrouter", "minimax", "hunyuan",
            "mimo", "gemini-native", "openai-responses", "minimax-responses",
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
    fn responses_providers_resolve() {
        let config = ProviderConfig::default();
        for id in ["openai-responses", "minimax-responses"] {
            let mut reg = crate::registry::Registry::new();
            register_provider(&mut reg, id, &config);
            assert!(
                reg.completion_model(id).is_some(),
                "responses provider {id} should resolve"
            );
        }
    }
}
