//! 视觉工具：OpenAI 兼容 `chat/completions` 图片理解（Google / OpenAI）。
//!
//! 参考：<https://ai.google.dev/gemini-api/docs/openai#javascript_4>

use base64::Engine;
use providers::http_stream::openai_compatible_base;
use providers::interactions_http::VisionMode;
use providers::media_http::{default_vision_model, openai_vision_completions};
use providers::trait_::ProviderConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::{ImageGenCreds, ToolContext};
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `vision` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct VisionArgs {
    /// 工作区内相对路径，或 `http(s)://` URL。
    pub image_url: String,
    /// 对图片的问题或指令；缺省为「请描述这张图片」。
    #[serde(default)]
    pub prompt: Option<String>,
}

/// 向注册表登记 `vision` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "vision".to_string(),
        toolset: "vision".to_string(),
        description: "Analyze an image (workspace relative path or http(s) URL) with a vision-capable model via OpenAI-compatible chat/completions. Prefer Google (Gemini) when enabled, else OpenAI."
            .to_string(),
        schema: schema_for_args::<VisionArgs>(),
        check_fn: None,
        icon: "eye",
    });
}

/// 读取图片并调用视觉模型，返回描述/问答文本。
pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: VisionArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("vision 参数无效: {e}"))?;
    let image_url = parsed.image_url.trim();
    if image_url.is_empty() {
        anyhow::bail!("vision 需要 image_url");
    }
    let prompt = parsed
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("请描述这张图片");

    let resolved_url = resolve_image_url(ctx, image_url)?;

    let mut errors = Vec::new();
    if let Some(creds) = ctx.image_gen_targets.google() {
        match call_vision(creds, "google", prompt, &resolved_url).await {
            Ok(msg) => return Ok(msg),
            Err(e) => errors.push(format!("google: {e}")),
        }
    }

    match call_openai_vision(ctx, prompt, &resolved_url).await {
        Ok(msg) => Ok(msg),
        Err(e) => {
            errors.push(format!("openai: {e}"));
            anyhow::bail!(
                "vision 失败：{}。请配置 Google 或 OpenAI API Key。",
                errors.join("；")
            )
        }
    }
}

fn resolve_image_url(ctx: &ToolContext<'_>, image_url: &str) -> anyhow::Result<String> {
    if image_url.starts_with("http://") || image_url.starts_with("https://") {
        return Ok(image_url.to_string());
    }
    let path = ctx.workspace_dir.join(image_url);
    if !path.exists() {
        anyhow::bail!("本地文件不存在: {}", path.display());
    }
    let bytes = std::fs::read(&path)
        .map_err(|e| anyhow::anyhow!("读取图片失败 {}: {e}", path.display()))?;
    let mime = mime_from_path(&path);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{mime};base64,{b64}"))
}

fn mime_from_path(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "jpg" | "jpeg" => "image/jpeg",
        "bmp" => "image/bmp",
        _ => "image/jpeg",
    }
}

async fn call_vision(
    creds: &ImageGenCreds,
    provider: &str,
    prompt: &str,
    image_url: &str,
) -> anyhow::Result<String> {
    let model = if creds.vision_model.trim().is_empty() {
        default_vision_model(provider).to_string()
    } else {
        creds.vision_model.trim().to_string()
    };
    let config = ProviderConfig {
        api_key: creds.api_key.clone(),
        base_url: if creds.base_url.trim().is_empty() {
            None
        } else {
            Some(creds.base_url.clone())
        },
        model: model.clone(),
        ..ProviderConfig::default()
    };
    let client = reqwest::Client::new();
    let text = openai_vision_completions(
        &client,
        prompt,
        &[image_url.to_string()],
        VisionMode::Describe,
        &config,
    )
    .await?;
    Ok(format!(
        "{text}\nprovider={provider}\nmodel={model}"
    ))
}

async fn call_openai_vision(
    ctx: &ToolContext<'_>,
    prompt: &str,
    image_url: &str,
) -> anyhow::Result<String> {
    if let Some(creds) = ctx.image_gen_targets.openai() {
        return call_vision(creds, "openai", prompt, image_url).await;
    }
    if !ctx.chat_api_key.is_empty() && ctx.chat_provider == "openai" {
        let model = if ctx.chat_model.trim().is_empty() {
            default_vision_model("openai").to_string()
        } else {
            ctx.chat_model.trim().to_string()
        };
        let base = if ctx.chat_base_url.trim().is_empty() {
            None
        } else {
            Some(openai_compatible_base(&ctx.chat_base_url))
        };
        let config = ProviderConfig {
            api_key: ctx.chat_api_key.clone(),
            base_url: base,
            model: model.clone(),
            ..ProviderConfig::default()
        };
        let client = reqwest::Client::new();
        let text = openai_vision_completions(
            &client,
            prompt,
            &[image_url.to_string()],
            VisionMode::Describe,
            &config,
        )
        .await?;
        return Ok(format!("{text}\nprovider=openai\nmodel={model}"));
    }
    let env_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    if env_key.is_empty() {
        anyhow::bail!("无可用 OpenAI API Key");
    }
    let model = default_vision_model("openai").to_string();
    let config = ProviderConfig {
        api_key: env_key,
        base_url: Some("https://api.openai.com/v1".into()),
        model: model.clone(),
        ..ProviderConfig::default()
    };
    let client = reqwest::Client::new();
    let text = openai_vision_completions(
        &client,
        prompt,
        &[image_url.to_string()],
        VisionMode::Describe,
        &config,
    )
    .await?;
    Ok(format!("{text}\nprovider=openai\nmodel={model}"))
}
