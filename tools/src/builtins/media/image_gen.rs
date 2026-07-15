//! 图片生成工具：按文本提示调用 Google / OpenAI 等图片 Provider。
//!
//! 凭据来自 [`ToolContext::image_gen_targets`]：先试 primary，失败再试 fallback。
//! 成功图片写入工作区 `generated/images/`。

use memory::{generated_dir, GeneratedKind};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use providers::trait_::ProviderConfig;

use crate::context::{ImageGenCreds, ToolContext};
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `image_gen` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ImageGenArgs {
    /// 图片的详细文字描述（不可为空）。
    pub prompt: String,
    /// 宽高比（可选，主要 Google）：如 `1:1` / `16:9` / `9:16`。
    #[serde(default)]
    pub aspect_ratio: Option<String>,
    /// 输出分辨率（Google Interactions）：`0.5K` / `1K` / `2K` / `4K`（K 须大写）。
    #[serde(default)]
    pub image_size: Option<String>,
    /// 参考图工作区相对路径（最多 14 张）。
    #[serde(default)]
    pub reference_images: Option<Vec<String>>,
    /// 上一轮 Interactions 会话 ID（多轮编辑）。
    #[serde(default)]
    pub previous_interaction_id: Option<String>,
    /// 启用 Google Search grounding。
    #[serde(default)]
    pub google_search: bool,
    /// 启用图片搜索（须同时 `google_search=true`）。
    #[serde(default)]
    pub image_search: bool,
    /// 思考深度：`minimal` / `high`。
    #[serde(default)]
    pub thinking_level: Option<String>,
    /// YouTube 等外部视频 URL（与 `video` 二选一）。
    #[serde(default)]
    pub video_uri: Option<String>,
    /// 工作区内已生成视频相对路径（与 `video_uri` 二选一）。
    #[serde(default)]
    pub video: Option<String>,
}

fn validate_image_gen_args(args: &ImageGenArgs) -> anyhow::Result<()> {
    if args.prompt.trim().is_empty() {
        anyhow::bail!("image_gen 需要 prompt 参数");
    }
    if let Some(sz) = args.image_size.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        match sz {
            "0.5K" | "1K" | "2K" | "4K" => {}
            _ => anyhow::bail!(
                "image_size 无效: {sz}（仅支持 0.5K / 1K / 2K / 4K，须大写 K）"
            ),
        }
    }
    if let Some(level) = args.thinking_level.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        match level {
            "minimal" | "high" => {}
            _ => anyhow::bail!("thinking_level 无效: {level}（仅支持 minimal / high）"),
        }
    }
    if args.image_search && !args.google_search {
        anyhow::bail!("image_search 需要同时设置 google_search=true");
    }
    let has_uri = args
        .video_uri
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    let has_file = args
        .video
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    if has_uri && has_file {
        anyhow::bail!("video 与 video_uri 不能同时使用");
    }
    if let Some(refs) = &args.reference_images {
        if refs.iter().filter(|p| !p.trim().is_empty()).count() > 14 {
            anyhow::bail!("reference_images 最多 14 张");
        }
    }
    Ok(())
}

/// 向注册表登记 `image_gen` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "image_gen".to_string(),
        toolset: "image_gen".to_string(),
        description: "Generate an image from a text prompt. Uses Gemini OpenAI-compatible images API when Google is enabled, otherwise OpenAI gpt-image-2. Optional aspect_ratio (e.g. 1:1, 16:9) for Google."
            .to_string(),
        schema: schema_for_args::<ImageGenArgs>(),
        check_fn: None,
        icon: "palette",
    });
}

/// 依次尝试 primary / fallback 凭据生成图片，返回本地路径与所用模型信息。
///
/// # 错误
/// 无可用 Provider、全部尝试失败，或缺少 `prompt`。
pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: ImageGenArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("image_gen 参数无效: {e}"))?;
    validate_image_gen_args(&parsed)?;
    let prompt = parsed.prompt.trim();
    let aspect_ratio = parsed
        .aspect_ratio
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    if ctx.image_gen_targets.is_empty() {
        anyhow::bail!(
            "未找到可用的图片生成提供商。请在「模型提供商」中开启 Google 或 OpenAI，并配置 API Key。"
        );
    }

    let mut errors = Vec::new();
    for creds in [
        ctx.image_gen_targets.primary.as_ref(),
        ctx.image_gen_targets.fallback.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        match generate_one(ctx, prompt, aspect_ratio, creds).await {
            Ok(path) => {
                return Ok(format!(
                    "图片已生成：{path}\nprovider={}\nmodel={}\nhint: 可用作 video_gen 的 image / last_frame / reference_image（工作区相对路径）",
                    creds.provider, creds.model
                ));
            }
            Err(err) => {
                errors.push(format!("{} ({}): {err}", creds.provider, creds.model));
            }
        }
    }

    anyhow::bail!("图片生成失败：{}", errors.join("；"))
}

/// 使用单一凭据调用 Provider，并将首张图片落盘。
///
/// # 返回
/// 生成文件的绝对/显示路径字符串。
async fn generate_one(
    ctx: &ToolContext<'_>,
    prompt: &str,
    aspect_ratio: Option<&str>,
    creds: &ImageGenCreds,
) -> anyhow::Result<String> {
    let provider = ctx
        .providers
        .get(&creds.provider)
        .ok_or_else(|| anyhow::anyhow!("未知 Provider: {}", creds.provider))?;

    let additional_params = if let Some(ar) = aspect_ratio.filter(|_| creds.provider == "google") {
        serde_json::json!({ "aspect_ratio": ar })
    } else {
        serde_json::Value::Null
    };

    let config = ProviderConfig {
        api_key: creds.api_key.clone(),
        base_url: if creds.base_url.trim().is_empty() {
            None
        } else {
            Some(creds.base_url.clone())
        },
        model: creds.model.clone(),
        additional_params,
        ..ProviderConfig::default()
    };

    let images = provider.generate_image(prompt, &config).await?;
    let img = images
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("未返回图片数据"))?;

    let dir = generated_dir(&ctx.workspace_dir, GeneratedKind::Images);
    std::fs::create_dir_all(&dir)?;
    let ext = if img.mime_type.contains("jpeg") || img.mime_type.contains("jpg") {
        "jpg"
    } else if img.mime_type.contains("webp") {
        "webp"
    } else {
        "png"
    };
    let filename = format!(
        "img-{}-{}.{}",
        chrono::Local::now().format("%Y%m%d-%H%M%S"),
        &uuid::Uuid::new_v4().simple().to_string()[..8],
        ext
    );
    let path = dir.join(filename);
    std::fs::write(&path, &img.data)?;
    let rel = path
        .strip_prefix(&ctx.workspace_dir)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string());
    Ok(rel)
}

#[cfg(test)]
mod arg_tests {
    use super::*;

    #[test]
    fn rejects_lowercase_image_size() {
        let a = ImageGenArgs {
            prompt: "x".into(),
            aspect_ratio: None,
            image_size: Some("1k".into()),
            reference_images: None,
            previous_interaction_id: None,
            google_search: false,
            image_search: false,
            thinking_level: None,
            video_uri: None,
            video: None,
        };
        assert!(validate_image_gen_args(&a).is_err());
    }

    #[test]
    fn rejects_image_search_without_google_search() {
        let a = ImageGenArgs {
            prompt: "x".into(),
            aspect_ratio: None,
            image_size: None,
            reference_images: None,
            previous_interaction_id: None,
            google_search: false,
            image_search: true,
            thinking_level: None,
            video_uri: None,
            video: None,
        };
        assert!(validate_image_gen_args(&a).is_err());
    }

    #[test]
    fn rejects_both_video_inputs() {
        let a = ImageGenArgs {
            prompt: "x".into(),
            aspect_ratio: None,
            image_size: None,
            reference_images: None,
            previous_interaction_id: None,
            google_search: false,
            image_search: false,
            thinking_level: None,
            video_uri: Some("https://www.youtube.com/watch?v=x".into()),
            video: Some("generated/videos/a.mp4".into()),
        };
        assert!(validate_image_gen_args(&a).is_err());
    }

    #[test]
    fn accepts_valid_size_and_thinking() {
        let a = ImageGenArgs {
            prompt: "x".into(),
            aspect_ratio: Some("1:1".into()),
            image_size: Some("1K".into()),
            reference_images: None,
            previous_interaction_id: None,
            google_search: true,
            image_search: true,
            thinking_level: Some("minimal".into()),
            video_uri: None,
            video: None,
        };
        assert!(validate_image_gen_args(&a).is_ok());
    }
}

#[cfg(test)]
mod path_tests {
    use memory::{generated_dir, GeneratedKind};
    use std::path::Path;

    #[test]
    fn image_gen_target_dir_is_images() {
        let d = generated_dir(Path::new("/ws"), GeneratedKind::Images);
        assert!(d.ends_with("generated/images"));
    }
}
