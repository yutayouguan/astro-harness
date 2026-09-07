//! 供应商能力分发 — 外部调用者的唯一入口。
//!
//! 所有 provider 操作（聊天、媒体、验证、元数据）通过本模块的公开函数访问。
//! 内部使用 trait 系统（`Registry` + `DynProvider`）路由到具体实现。

use serde_json::Value;

use crate::types::error::{ProviderError, ProviderResult};
use crate::types::media::{GeneratedAudio, GeneratedImage, GeneratedVideo};
use crate::types::request::{
    ChatCompletionRequest, PromptCacheConfig, ProviderConfig, ResponsesRequest, ThinkingConfig,
    ToolChoice,
};
use crate::types::request_content::{
    ChatCompletionMessage, FunctionToolDefinition, ToolDefinition,
};
use crate::types::stream::CompletionStream;

/// 非 Agent Chat/Anthropic/Gemini 兼容管线分发。
pub async fn chat_stream_direct(
    provider: &str,
    request: ChatCompletionRequest,
    config: &ProviderConfig,
) -> ProviderResult<CompletionStream> {
    let provider = normalize_provider_id(provider);
    let mut reg = crate::registry::Registry::new();
    register_provider(&mut reg, provider, config);

    let dyn_model = reg
        .chat_completion_model(provider)
        .ok_or_else(|| ProviderError::UnknownProvider(provider.to_string()))?;

    dyn_model
        .stream(request)
        .await
        .map_err(ProviderError::Other)
}

/// Agent 原生 Responses 管线分发。
pub async fn responses_stream_direct(
    provider: &str,
    prompt: ResponsesRequest,
    config: &ProviderConfig,
) -> ProviderResult<CompletionStream> {
    let provider = normalize_provider_id(provider);
    let mut reg = crate::registry::Registry::new();
    register_provider(&mut reg, provider, config);

    let dyn_model =
        reg.responses_model(provider)
            .ok_or_else(|| ProviderError::UnsupportedCapability {
                provider: provider.to_string(),
                capability: "Agent Responses API".to_string(),
            })?;

    dyn_model.stream(prompt).await.map_err(ProviderError::Other)
}

/// 聊天补全 — 接受旧签名（messages + tools JSON + config）。
///
/// 大多数调用者使用此函数。tools 为 OpenAI 格式 JSON。
pub async fn chat_stream(
    provider: &str,
    messages: Vec<ChatCompletionMessage>,
    tools: Vec<Value>,
    config: &ProviderConfig,
) -> ProviderResult<CompletionStream> {
    chat_stream_with_tool_policy(provider, messages, tools, config, None, None).await
}

/// Agent-only Responses API entry point.
///
/// Unlike [`chat_stream`], this accepts the canonical Responses item history
/// and refuses providers that do not advertise Responses support. Legacy chat,
/// Anthropic, Gemini, and Interactions protocols remain available only to
/// tool-owned callers through their dedicated APIs.
pub async fn agent_responses_stream(
    provider: &str,
    instructions: String,
    input: Vec<agent_protocol::ResponseItem>,
    tools: Vec<Value>,
    config: &ProviderConfig,
) -> ProviderResult<CompletionStream> {
    let provider = normalize_provider_id(provider);
    if !supports_agent_responses(provider) {
        return Err(ProviderError::UnsupportedCapability {
            provider: provider.to_string(),
            capability: "Agent Responses API".to_string(),
        });
    }
    let config = force_agent_responses_config(config);
    let mut additional_params = config.additional_params.clone();
    let prompt_cache = PromptCacheConfig::take_from_additional_params(&mut additional_params)
        .map_err(|detail| ProviderError::ModelError {
            provider: provider.to_string(),
            detail,
        })?;
    let has_prompt_cache_controls = prompt_cache.is_some()
        || additional_params
            .as_object()
            .is_some_and(|params| params.keys().any(|key| key.starts_with("prompt_cache_")));
    if has_prompt_cache_controls {
        if let Some(capability) = unsupported_prompt_cache_controls(provider, &config.model) {
            return Err(ProviderError::UnsupportedCapability {
                provider: provider.to_string(),
                capability,
            });
        }
    }
    let tool_definitions = parse_tool_definitions(provider, &tools)?;
    let request = ResponsesRequest {
        model: config.model.clone(),
        instructions,
        input,
        tools: tool_definitions,
        tool_choice: None,
        parallel_tool_calls: None,
        temperature: Some(config.temperature),
        max_tokens: Some(config.max_tokens),
        thinking: Some(ThinkingConfig {
            enabled: config.thinking_enabled,
            budget_tokens: None,
            effort: config.reasoning_effort.clone(),
        }),
        prompt_cache,
        additional_params,
    };
    responses_stream_direct(provider, request, &config).await
}

/// Convenience entry point for Agent-owned one-shot tasks such as title,
/// compaction, memory review, and smart approval.
pub async fn agent_responses_prompt(
    provider: &str,
    instructions: impl Into<String>,
    prompt: impl Into<String>,
    config: &ProviderConfig,
) -> ProviderResult<CompletionStream> {
    agent_responses_stream(
        provider,
        instructions.into(),
        vec![agent_protocol::ResponseItem::Message {
            id: None,
            role: "user".into(),
            content: vec![agent_protocol::ContentItem::InputText {
                text: prompt.into(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }],
        Vec::new(),
        config,
    )
    .await
}

/// Whether a provider may participate in the Agent target/fallback chain.
pub fn supports_agent_responses(provider: &str) -> bool {
    let provider = normalize_provider_id(provider);
    crate::profile::resolve(provider)
        .is_some_and(crate::profile::ProviderProfile::supports_agent_responses)
        || lookup_custom_provider(provider).is_some()
}

fn force_agent_responses_config(config: &ProviderConfig) -> ProviderConfig {
    let mut config = config.clone();
    config.api_mode = "responses".to_string();
    config
}

fn parsed_gpt_version(model: &str) -> Option<(u16, u16)> {
    let lower = model.to_ascii_lowercase();
    let marker = lower.find("gpt-")?;
    let version = &lower[marker + 4..];
    let mut parts = version.split(|ch: char| !ch.is_ascii_digit() && ch != '.');
    let numeric = parts.find(|part| !part.is_empty())?;
    let mut numbers = numeric.split('.');
    let major = numbers.next()?.parse().ok()?;
    let minor = numbers.next().unwrap_or("0").parse().ok()?;
    Some((major, minor))
}

fn unsupported_prompt_cache_controls(provider: &str, model: &str) -> Option<String> {
    match normalize_provider_id(provider) {
        "deepseek" => {
            Some("显式 prompt cache 控制（DeepSeek 上下文缓存由服务端自动管理）".to_string())
        }
        "azure" if parsed_gpt_version(model).is_some_and(|version| version < (5, 6)) => Some(
            format!("GPT-5.6+ prompt cache controls for model `{model}`"),
        ),
        _ => None,
    }
}

pub(crate) async fn chat_stream_with_tool_policy(
    provider: &str,
    messages: Vec<ChatCompletionMessage>,
    tools: Vec<Value>,
    config: &ProviderConfig,
    tool_choice: Option<ToolChoice>,
    parallel_tool_calls: Option<bool>,
) -> ProviderResult<CompletionStream> {
    let mut instructions = String::new();
    let mut input = Vec::with_capacity(messages.len());
    for message in messages {
        match message {
            ChatCompletionMessage::System { content } if instructions.is_empty() => {
                instructions = content;
            }
            other => input.push(other),
        }
    }
    let tool_defs = parse_tool_definitions(provider, &tools)?;

    let request = ChatCompletionRequest {
        model: config.model.clone(),
        instructions,
        input,
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

fn parse_tool_definition(value: &Value) -> Option<ToolDefinition> {
    if let Some(function) = value.get("function") {
        return parse_function_tool(function);
    }

    match value.get("type").and_then(Value::as_str) {
        Some("function") => parse_function_tool(value),
        Some("custom" | "namespace" | "tool_search" | "web_search") => {
            serde_json::from_value(value.clone()).ok()
        }
        Some(_) => None,
        None => parse_function_tool(value),
    }
}

fn parse_tool_definitions(provider: &str, values: &[Value]) -> ProviderResult<Vec<ToolDefinition>> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            parse_tool_definition(value).ok_or_else(|| ProviderError::ModelError {
                provider: provider.to_string(),
                detail: format!(
                    "invalid tool definition at index {index}: type={:?} name={:?}",
                    value.get("type").and_then(Value::as_str),
                    value.get("name").and_then(Value::as_str)
                ),
            })
        })
        .collect()
}

fn parse_function_tool(value: &Value) -> Option<ToolDefinition> {
    Some(ToolDefinition::Function(FunctionToolDefinition {
        name: value.get("name")?.as_str()?.to_string(),
        description: value
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        parameters: value
            .get("parameters")
            .cloned()
            .unwrap_or(serde_json::json!({"type": "object", "properties": {}})),
        strict: value
            .get("strict")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        defer_loading: value.get("defer_loading").and_then(Value::as_bool),
    }))
}

/// 图片生成。按 profile 的 `image_mode` 路由到对应 HTTP 模块。
pub async fn generate_image(
    provider: &str,
    prompt: &str,
    config: &ProviderConfig,
) -> ProviderResult<Vec<GeneratedImage>> {
    generate_image_with_options(
        provider,
        prompt,
        config,
        &crate::types::ImageGenConfig::default(),
    )
    .await
}

/// 图片生成，传递统一尺寸、数量和输出选项。
pub async fn generate_image_with_options(
    provider: &str,
    prompt: &str,
    config: &ProviderConfig,
    options: &crate::types::ImageGenConfig,
) -> ProviderResult<Vec<GeneratedImage>> {
    let provider = normalize_provider_id(provider);
    let profile = crate::profile::resolve_or_openai_compat(provider);
    let mode = profile
        .image_mode
        .ok_or_else(|| ProviderError::UnsupportedCapability {
            provider: provider.to_string(),
            capability: "图片生成".to_string(),
        })?;
    let count = if options.n == 0 { 1 } else { options.n };
    if count > 10 {
        return Err(ProviderError::Other(anyhow::anyhow!(
            "图片生成张数 n 必须在 1..=10 之间"
        )));
    }
    if matches!(
        (options.width, options.height),
        (Some(_), None) | (None, Some(_))
    ) {
        return Err(ProviderError::Other(anyhow::anyhow!(
            "图片尺寸必须同时提供 width 和 height"
        )));
    }
    if let Some(format) = options.output_format.as_deref() {
        if !matches!(
            format.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "webp"
        ) {
            return Err(ProviderError::Other(anyhow::anyhow!(
                "不支持的图片输出格式: {format}"
            )));
        }
    }
    if options.output_compression.is_some_and(|value| value > 100) {
        return Err(ProviderError::Other(anyhow::anyhow!(
            "output_compression 必须在 0..=100 之间"
        )));
    }
    let mut cfg = config.clone();
    if !options.model.trim().is_empty() {
        cfg.model = options.model.trim().to_string();
    } else if cfg.model.trim().is_empty() && !profile.default_image_model.is_empty() {
        cfg.model = profile.default_image_model.to_string();
    }
    let client = shared_http_client();
    match mode {
        crate::profile::ImageGenMode::OpenAi => Ok(
            crate::openai::image_http::openai_generate_image_with_config(
                &client, prompt, &cfg, options,
            )
            .await?,
        ),
        crate::profile::ImageGenMode::AzureOpenAiV1 => Ok(
            crate::openai::image_http::azure_foundry_generate_image_with_config(
                &client, prompt, &cfg, options,
            )
            .await?,
        ),
        crate::profile::ImageGenMode::GoogleInteractions => {
            let aspect_ratio =
                options
                    .aspect_ratio
                    .clone()
                    .or_else(|| match (options.width, options.height) {
                        (Some(width), Some(height)) if width > 0 && height > 0 => {
                            let divisor = gcd(width, height);
                            Some(format!("{}:{}", width / divisor, height / divisor))
                        }
                        _ => None,
                    });
            let mime_type = options
                .output_format
                .as_deref()
                .map(|format| match format.to_ascii_lowercase().as_str() {
                    "png" => "image/png",
                    "webp" => "image/webp",
                    _ => "image/jpeg",
                })
                .map(str::to_string);
            let mut images = Vec::with_capacity(count as usize);
            for _ in 0..count {
                let req = crate::google::interactions_http::InteractionImageRequest {
                    prompt: prompt.to_string(),
                    aspect_ratio: aspect_ratio.clone(),
                    mime_type: mime_type.clone(),
                    ..Default::default()
                };
                let result = crate::google::interactions_http::google_interactions_image(
                    &client, &cfg, &req,
                )
                .await?;
                images.push(result.image);
            }
            Ok(images)
        }
        crate::profile::ImageGenMode::MiniMax => {
            let req = crate::minimax::image_http::MiniMaxImageRequest {
                model: cfg.model.clone(),
                prompt: prompt.to_string(),
                aspect_ratio: options
                    .aspect_ratio
                    .clone()
                    .unwrap_or_else(|| "1:1".to_string()),
                width: options.width,
                height: options.height,
                response_format: "url".to_string(),
                n: count,
                ..Default::default()
            };
            Ok(crate::minimax::image_http::minimax_generate_image(&client, &cfg, &req).await?)
        }
    }
}

fn gcd(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
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
    if !profile.supports_embedding() {
        return Err(ProviderError::UnsupportedCapability {
            provider: provider.to_string(),
            capability: "文本嵌入".to_string(),
        });
    }
    let mut cfg = config.clone();
    if provider == "azure" {
        let endpoint = cfg
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(profile.default_base_url);
        cfg.base_url = Some(crate::impls::azure::azure_openai_v1_base(endpoint));
    }
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
    crate::profile::resolve(provider)
        .is_some_and(crate::profile::ProviderProfile::supports_image_gen)
}

fn shared_http_client() -> reqwest::Client {
    crate::registry::shared_http_client()
}

fn normalize_provider_id(id: &str) -> &str {
    crate::profile::normalize_provider_id(id)
}

/// 是否应使用 Responses API 协议。
///
/// 优先级：`config.api_mode` 显式指定 > profile `api_mode` 默认。
/// - `api_mode == "chat" | "chat_completions"` → 强制 Chat Completions
/// - `api_mode == "responses"` → 强制 Responses
/// - `api_mode` 为空 → profile.api_mode 决定
fn use_responses(provider: &str, config: &ProviderConfig) -> bool {
    crate::profile::effective_api_mode(provider, &config.api_mode)
        == crate::profile::ApiMode::Responses
}

/// 根据 provider id 注册到注册表。
fn register_provider(reg: &mut crate::registry::Registry, provider: &str, config: &ProviderConfig) {
    let key = &config.api_key;
    let base = config.base_url.as_deref().filter(|s| !s.trim().is_empty());
    let model = &config.model;
    let responses = use_responses(provider, config);

    let Some(profile) = crate::profile::resolve(provider) else {
        if let Some(custom_cfg) = lookup_custom_provider(provider) {
            reg.register_custom(provider, &custom_cfg, key, model);
        } else {
            register_compat::<crate::impls::openai::OpenAI>(reg, key, base, model);
        }
        return;
    };

    use crate::profile::ProviderKind;
    // Chat/media registration and Responses attachment are both selected from
    // the same profile descriptor. This prevents provider-id tables drifting.
    match profile.kind {
        ProviderKind::Anthropic => reg.register_anthropic(key, base, model),
        ProviderKind::Google => reg.register_google(key, base, model),
        ProviderKind::OpenAi => reg.register_openai(key, base, model),
        ProviderKind::DeepSeek => {
            register_compat::<crate::impls::deepseek::DeepSeek>(reg, key, base, model)
        }
        ProviderKind::Azure => reg.register_azure(key, base, model),
        ProviderKind::Zhipu => register_media::<crate::impls::zhipu::Zhipu>(reg, key, base, model),
        ProviderKind::Moonshot => {
            register_compat::<crate::impls::moonshot::Moonshot>(reg, key, base, model)
        }
        ProviderKind::Ollama => {
            register_compat::<crate::impls::ollama::Ollama>(reg, key, base, model)
        }
        ProviderKind::Nvidia => {
            register_compat::<crate::impls::nvidia::Nvidia>(reg, key, base, model)
        }
        ProviderKind::Bailian => {
            register_media::<crate::impls::bailian::Bailian>(reg, key, base, model)
        }
        ProviderKind::Volcengine => {
            register_media::<crate::impls::volcengine::Volcengine>(reg, key, base, model)
        }
        ProviderKind::OpenRouter => {
            register_embedding::<crate::impls::openrouter::OpenRouter>(reg, key, base, model)
        }
        ProviderKind::MiniMax => reg.register_minimax(key, base, model),
        ProviderKind::MiniMaxAnthropic => {
            reg.register_anthropic(key, base, model);
            reg.register_alias("minimax-anthropic", "anthropic");
        }
        ProviderKind::Hunyuan => {
            register_media::<crate::impls::hunyuan::Hunyuan>(reg, key, base, model)
        }
        ProviderKind::Mimo => register_compat::<crate::impls::mimo::Mimo>(reg, key, base, model),
        ProviderKind::GeminiNative => reg.register_gemini_native(key, base, model),
    }

    // 2) Responses 模式：独立挂载 Agent Responses 模型，保留 Chat/media 能力。
    //    默认由 profile.api_mode 决定；用户可显式覆盖协议模式。
    if responses {
        match profile.kind {
            ProviderKind::OpenAi => {
                reg.attach_responses::<crate::impls::openai::OpenAI>(provider, key, base, model)
            }
            ProviderKind::DeepSeek => {
                reg.attach_responses::<crate::impls::deepseek::DeepSeek>(provider, key, base, model)
            }
            ProviderKind::Azure => {
                reg.attach_responses::<crate::impls::azure::Azure>(provider, key, base, model)
            }
            ProviderKind::Bailian => {
                reg.attach_responses::<crate::impls::bailian::Bailian>(provider, key, base, model)
            }
            ProviderKind::MiniMax => reg.attach_responses::<crate::impls::minimax_chat::MiniMax>(
                "minimax", key, base, model,
            ),
            ProviderKind::Mimo => {
                reg.attach_responses::<crate::impls::mimo::Mimo>(provider, key, base, model)
            }
            ProviderKind::Ollama => {
                reg.attach_responses::<crate::impls::ollama::Ollama>(provider, key, base, model)
            }
            ProviderKind::OpenRouter => reg
                .attach_responses::<crate::impls::openrouter::OpenRouter>(
                    provider, key, base, model,
                ),
            ProviderKind::Zhipu => {
                reg.attach_responses::<crate::impls::zhipu::Zhipu>(provider, key, base, model)
            }
            ProviderKind::Moonshot => {
                reg.attach_responses::<crate::impls::moonshot::Moonshot>(provider, key, base, model)
            }
            ProviderKind::Nvidia => {
                reg.attach_responses::<crate::impls::nvidia::Nvidia>(provider, key, base, model)
            }
            ProviderKind::Volcengine => reg
                .attach_responses::<crate::impls::volcengine::Volcengine>(
                    provider, key, base, model,
                ),
            ProviderKind::Hunyuan => {
                reg.attach_responses::<crate::impls::hunyuan::Hunyuan>(provider, key, base, model)
            }
            ProviderKind::Anthropic
            | ProviderKind::Google
            | ProviderKind::MiniMaxAnthropic
            | ProviderKind::GeminiNative => {}
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

fn register_embedding<Ext>(
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
        > + Default
        + Copy
        + 'static,
{
    reg.register_compat_with_embedding::<Ext>(api_key, base_url, model);
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
            let registered = reg.get(id).expect("provider should be registered");
            assert!(
                registered.responses_model().is_some()
                    || registered.chat_completion_model().is_some(),
                "provider {id} should have a Responses or compatibility model"
            );
        }
    }

    #[test]
    fn profile_descriptor_and_runtime_registration_do_not_drift() {
        let config = ProviderConfig::default();
        for profile in crate::profile::PROFILES {
            let mut registry = crate::registry::Registry::new();
            register_provider(&mut registry, profile.id, &config);
            let registered = registry
                .get(profile.id)
                .unwrap_or_else(|| panic!("provider {} was not registered", profile.id));
            assert!(
                registered.chat_completion_model().is_some()
                    || registered.responses_model().is_some(),
                "provider {} has no language model runtime",
                profile.id
            );
            assert_eq!(
                registered.responses_model().is_some(),
                profile.supports_agent_responses(),
                "provider {} Responses capability disagrees with its runtime",
                profile.id
            );
        }
    }

    #[test]
    fn openrouter_registers_embedding_and_responses_capabilities() {
        let mut registry = crate::registry::Registry::new();
        register_provider(&mut registry, "openrouter", &ProviderConfig::default());
        let provider = registry.get("openrouter").unwrap();
        assert!(provider.embedding_model().is_some());
        assert!(provider.responses_model().is_some());
        assert!(crate::profile::resolve("openrouter")
            .is_some_and(crate::profile::ProviderProfile::supports_embedding));
    }

    #[test]
    fn minimax_anthropic_id_resolves() {
        let config = ProviderConfig::default();
        let id = "minimax-anthropic";
        let mut reg = crate::registry::Registry::new();
        register_provider(&mut reg, id, &config);
        let registered = reg
            .get(id)
            .expect("MiniMax Anthropic provider should resolve");
        assert!(
            registered.responses_model().is_some() || registered.chat_completion_model().is_some(),
            "provider {id} should expose a language model"
        );
    }

    #[test]
    fn responses_default_for_supported_providers() {
        let config = ProviderConfig::default();
        for id in ["openai", "deepseek", "minimax", "azure", "bailian", "mimo"] {
            assert!(
                use_responses(id, &config),
                "{id} should default to Responses API"
            );
        }
        for id in ["anthropic", "google", "ollama", "zhipu"] {
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

        chat_config.api_mode = "chat_completions".to_string();
        assert!(
            !use_responses("deepseek", &chat_config),
            "persisted api_mode=chat_completions forces ChatCompletions"
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
        for id in ["openai", "deepseek", "minimax", "mimo"] {
            let mut reg = crate::registry::Registry::new();
            register_provider(&mut reg, id, &config);
            assert!(
                reg.responses_model(id).is_some(),
                "provider {id} (default Responses) should have a Responses model"
            );
        }
    }

    #[test]
    fn parses_responses_native_tool_variants_without_flattening() {
        let custom = parse_tool_definition(&serde_json::json!({
            "type": "custom",
            "name": "apply_patch",
            "description": "Apply a patch",
            "format": {
                "type": "grammar",
                "syntax": "lark",
                "definition": "start: /.+/"
            }
        }))
        .unwrap();
        assert!(matches!(custom, ToolDefinition::Freeform(_)));

        let namespace = parse_tool_definition(&serde_json::json!({
            "type": "namespace",
            "name": "clock",
            "description": "Clock tools",
            "tools": [{
                "type": "function",
                "name": "now",
                "description": "Current time",
                "parameters": {"type": "object"},
                "strict": true
            }]
        }))
        .unwrap();
        assert!(matches!(namespace, ToolDefinition::Namespace(_)));
    }

    #[test]
    fn malformed_tool_definitions_fail_closed() {
        let error = parse_tool_definitions(
            "openai",
            &[
                serde_json::json!({
                    "type": "function",
                    "name": "valid",
                    "parameters": {"type": "object"}
                }),
                serde_json::json!({
                    "type": "function",
                    "description": "missing name"
                }),
            ],
        )
        .unwrap_err();

        assert!(matches!(error, ProviderError::ModelError { .. }));
        assert!(error.to_string().contains("index 1"));
    }

    #[test]
    fn agent_entry_forces_responses_even_with_chat_compat_override() {
        let config = ProviderConfig {
            api_mode: "chat_completions".into(),
            ..ProviderConfig::default()
        };
        let config = force_agent_responses_config(&config);
        assert_eq!(config.api_mode, "responses");

        let mut registry = crate::registry::Registry::new();
        register_provider(&mut registry, "deepseek", &config);
        assert!(registry.responses_model("deepseek").is_some());
    }

    #[tokio::test]
    async fn agent_entry_rejects_chat_only_provider_without_fallback() {
        let error = match agent_responses_stream(
            "anthropic",
            String::new(),
            Vec::new(),
            Vec::new(),
            &ProviderConfig::default(),
        )
        .await
        {
            Ok(_) => panic!("chat-only provider must not enter the Agent Responses path"),
            Err(error) => error,
        };
        assert!(matches!(error, ProviderError::UnsupportedCapability { .. }));
    }

    #[tokio::test]
    async fn azure_rejects_known_unsupported_prompt_cache_controls_before_http() {
        let config = ProviderConfig {
            api_key: "test-key".into(),
            base_url: Some("https://example.invalid/openai/v1".into()),
            model: "gpt-5.5".into(),
            additional_params: serde_json::json!({
                "prompt_cache_key": "agent:v1",
                "prompt_cache_options": {"mode": "implicit", "ttl": "30m"}
            }),
            ..ProviderConfig::default()
        };
        let error =
            match agent_responses_stream("azure", String::new(), Vec::new(), Vec::new(), &config)
                .await
            {
                Ok(_) => panic!("known pre-5.6 Azure model must be rejected"),
                Err(error) => error,
            };
        assert!(matches!(error, ProviderError::UnsupportedCapability { .. }));
    }

    #[tokio::test]
    async fn deepseek_rejects_prompt_cache_controls_before_http() {
        for additional_params in [
            serde_json::json!({
                "prompt_cache_key": "agent:v1",
                "prompt_cache_options": {"mode": "implicit"}
            }),
            serde_json::json!({"prompt_cache_retention": "24h"}),
        ] {
            let config = ProviderConfig {
                api_key: "test-key".into(),
                base_url: Some("https://example.invalid/v1".into()),
                model: "deepseek-v4-flash".into(),
                additional_params,
                ..ProviderConfig::default()
            };
            let error = match agent_responses_stream(
                "deepseek",
                String::new(),
                Vec::new(),
                Vec::new(),
                &config,
            )
            .await
            {
                Ok(_) => panic!("DeepSeek prompt cache controls must be rejected before HTTP"),
                Err(error) => error,
            };
            assert!(matches!(error, ProviderError::UnsupportedCapability { .. }));
            assert!(error.to_string().contains("服务端自动管理"));
        }
    }
}
