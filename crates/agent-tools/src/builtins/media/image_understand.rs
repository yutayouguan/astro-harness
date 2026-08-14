//! 图像理解工具：Google Interactions API 优先，OpenAI `chat/completions` 备用。
//!
//! 支持 describe / detect / segment 模式与多图输入。

use base64::Engine;
use providers::compat::openai_compatible_base;
use providers::interactions_http::{
    default_vision_prompt, google_interactions_vision, VisionImagePart, VisionMode,
};
use providers::media_http::{default_vision_model, openai_vision_completions};
use providers::ProviderConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::context::{ImageGenCreds, ToolContext};
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// Arguments for the `image_understand` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ImageUnderstandArgs {
    /// Workspace-relative paths or http(s) URLs (primary field).
    #[serde(default)]
    pub image_urls: Option<Vec<String>>,
    /// Legacy single-image field; merged into image_urls when set.
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    /// describe | detect | segment; default describe.
    #[serde(default)]
    pub mode: Option<String>,
}

/// 向注册表登记 `image_understand` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "image_analyze".to_string(),
        toolset: "image_analyze".to_string(),
        description: "Analyze image(s). Modes: describe, detect, segment. Pass image_urls or legacy image_url. Google Interactions API; OpenAI fallback."
            .to_string(),
        schema: schema_for_args::<ImageUnderstandArgs>(),
        check_fn: None,
        icon: "eye",
            ..ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["image_analyze"],
    async_ctx: dispatch,
}

/// 读取图片并调用视觉模型，返回描述/问答文本。
pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: ImageUnderstandArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("image_understand 参数无效: {e}"))?;
    let mode = VisionMode::parse(parsed.mode.as_deref().unwrap_or(""))?;
    let mut urls = parsed.image_urls.unwrap_or_default();
    if let Some(one) = parsed.image_url {
        let t = one.trim();
        if !t.is_empty() {
            urls.push(t.to_string());
        }
    }
    urls.retain(|u| !u.trim().is_empty());
    if urls.is_empty() {
        anyhow::bail!("image_understand 需要 image_urls 或 image_url");
    }
    let prompt = parsed
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_vision_prompt(mode))
        .to_string();

    let google_parts = resolve_google_images(ctx, &urls)?;
    let openai_urls = resolve_openai_image_urls(ctx, &urls)?;

    let mut errors = Vec::new();
    if let Some(creds) = ctx.image_gen_targets.google() {
        match call_google_vision(creds, &prompt, &google_parts, mode).await {
            Ok(msg) => return Ok(msg),
            Err(e) => errors.push(format!("google: {e}")),
        }
    }

    match call_openai_vision(ctx, &prompt, &openai_urls, mode).await {
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

fn resolve_google_images(
    ctx: &ToolContext<'_>,
    urls: &[String],
) -> anyhow::Result<Vec<VisionImagePart>> {
    let mut out = Vec::new();
    for u in urls {
        let u = u.trim();
        if u.starts_with("http://") || u.starts_with("https://") {
            out.push(VisionImagePart::Uri {
                mime_type: mime_from_url_or_path(u).to_string(),
                uri: u.to_string(),
            });
        } else if let Some(rest) = u.strip_prefix("data:") {
            let (meta, b64) = rest
                .split_once(',')
                .ok_or_else(|| anyhow::anyhow!("无效 data URL"))?;
            let mime = meta.split(';').next().unwrap_or("image/jpeg");
            out.push(VisionImagePart::Inline {
                mime_type: mime.to_string(),
                data_b64: b64.to_string(),
            });
        } else {
            let path = resolve_local_path(ctx, u)?;
            if !path.exists() {
                anyhow::bail!("本地文件不存在: {}", path.display());
            }
            let bytes = std::fs::read(&path)
                .map_err(|e| anyhow::anyhow!("读取图片失败 {}: {e}", path.display()))?;
            let mime = mime_from_path(&path);
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            out.push(VisionImagePart::Inline {
                mime_type: mime.to_string(),
                data_b64: b64,
            });
        }
    }
    Ok(out)
}

fn resolve_openai_image_urls(
    ctx: &ToolContext<'_>,
    urls: &[String],
) -> anyhow::Result<Vec<String>> {
    let mut out = Vec::new();
    for u in urls {
        let u = u.trim();
        if u.starts_with("http://") || u.starts_with("https://") || u.starts_with("data:") {
            out.push(u.to_string());
        } else {
            let path = resolve_local_path(ctx, u)?;
            if !path.exists() {
                anyhow::bail!("本地文件不存在: {}", path.display());
            }
            let bytes = std::fs::read(&path)
                .map_err(|e| anyhow::anyhow!("读取图片失败 {}: {e}", path.display()))?;
            let mime = mime_from_path(&path);
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            out.push(format!("data:{mime};base64,{b64}"));
        }
    }
    Ok(out)
}

/// 将用户/AI 给定的图片路径解析为本地绝对路径。
/// - 绝对路径：拒绝含 `..` 的路径后直接使用（supports uploads dir outside workspace）。
/// - 相对路径：经 `path_safe::resolve_safe` 解析到 workspace，防止逃逸与 symlink 攻击。
fn resolve_local_path(ctx: &ToolContext<'_>, raw: &str) -> anyhow::Result<std::path::PathBuf> {
    let p = std::path::Path::new(raw);
    if p.components().any(|c| c == std::path::Component::ParentDir) {
        anyhow::bail!("路径不允许包含 ..");
    }
    if p.is_absolute() {
        return Ok(p.to_path_buf());
    }
    crate::path_safe::resolve_safe(&ctx.workspace_dir, raw)
}

fn mime_from_url_or_path(s: &str) -> &'static str {
    let path_part = s.split(['?', '#']).next().unwrap_or(s);
    mime_from_extension(
        std::path::Path::new(path_part)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or(""),
    )
}

fn mime_from_path(path: &std::path::Path) -> &'static str {
    mime_from_extension(path.extension().and_then(|e| e.to_str()).unwrap_or(""))
}

fn mime_from_extension(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "jpg" | "jpeg" => "image/jpeg",
        "bmp" => "image/bmp",
        "heic" => "image/heic",
        "heif" => "image/heif",
        _ => "image/jpeg",
    }
}

async fn call_google_vision(
    creds: &ImageGenCreds,
    prompt: &str,
    images: &[VisionImagePart],
    mode: VisionMode,
) -> anyhow::Result<String> {
    let model = if creds.vision_model.trim().is_empty() {
        default_vision_model("google").to_string()
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
    let text = google_interactions_vision(&client, prompt, images, mode, &config).await?;
    Ok(format!(
        "{text}\nprovider=google\nmodel={model}\nmode={}",
        mode.as_str()
    ))
}

async fn call_openai_vision(
    ctx: &ToolContext<'_>,
    prompt: &str,
    image_urls: &[String],
    mode: VisionMode,
) -> anyhow::Result<String> {
    let client = reqwest::Client::new();

    if let Some(creds) = ctx.image_gen_targets.openai() {
        let model = if creds.vision_model.trim().is_empty() {
            default_vision_model("openai").to_string()
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
        let text = openai_vision_completions(&client, prompt, image_urls, mode, &config).await?;
        return Ok(format_openai_vision_output(&text, &model, mode));
    }

    if !ctx.credentials.api_key.is_empty() && ctx.credentials.provider == "openai" {
        let model = if ctx.credentials.model.trim().is_empty() {
            default_vision_model("openai").to_string()
        } else {
            ctx.credentials.model.trim().to_string()
        };
        let base = if ctx.credentials.base_url.trim().is_empty() {
            None
        } else {
            Some(openai_compatible_base(&ctx.credentials.base_url))
        };
        let config = ProviderConfig {
            api_key: ctx.credentials.api_key.clone(),
            base_url: base,
            model: model.clone(),
            ..ProviderConfig::default()
        };
        let text = openai_vision_completions(&client, prompt, image_urls, mode, &config).await?;
        return Ok(format_openai_vision_output(&text, &model, mode));
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
    let text = openai_vision_completions(&client, prompt, image_urls, mode, &config).await?;
    Ok(format_openai_vision_output(&text, &model, mode))
}

fn format_openai_vision_output(text: &str, model: &str, mode: VisionMode) -> String {
    let body = match mode {
        VisionMode::Describe => text.to_string(),
        VisionMode::Detect | VisionMode::Segment => {
            if let Ok(v) = serde_json::from_str::<Value>(text) {
                if v.get("boxes").is_some() {
                    serde_json::to_string_pretty(&v).unwrap_or_else(|_| text.to_string())
                } else {
                    format!("{text}\nfallback=openai")
                }
            } else {
                format!("{text}\nfallback=openai")
            }
        }
    };
    format!(
        "{body}\nprovider=openai\nmodel={model}\nmode={}",
        mode.as_str()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_image_url_into_list() {
        let args = serde_json::json!({"image_url": "a.png", "mode": "detect"});
        let parsed: ImageUnderstandArgs = serde_json::from_value(args).unwrap();
        assert!(parsed.image_url.as_deref() == Some("a.png"));
        assert_eq!(VisionMode::parse("detect").unwrap(), VisionMode::Detect);
    }

    #[test]
    fn mime_from_extension_heic() {
        assert_eq!(mime_from_extension("heic"), "image/heic");
        assert_eq!(mime_from_extension("HEIF"), "image/heif");
    }

    #[test]
    fn format_detect_pretty_prints_boxes_json() {
        let raw = r#"{"boxes":[{"box_2d":[0,1,2,3],"label":"cat"}]}"#;
        let out = format_openai_vision_output(raw, "gpt-4o", VisionMode::Detect);
        assert!(out.contains("\"boxes\""));
        assert!(!out.contains("fallback=openai"));
        assert!(out.contains("provider=openai"));
    }

    #[test]
    fn format_detect_appends_fallback_when_no_boxes() {
        let out = format_openai_vision_output("not json", "gpt-4o", VisionMode::Detect);
        assert!(out.contains("fallback=openai"));
    }
}
