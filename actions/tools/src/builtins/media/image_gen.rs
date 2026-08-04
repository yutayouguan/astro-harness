//! 图片生成工具：按文本提示调用 Google Gemini Interactions / OpenAI 等图片 Provider。
//!
//! Google 路径走 Gemini Interactions API（参考图、多轮、search、video 等）；
//! OpenAI 路径仅 prompt-only 生图。凭据来自 [`ToolContext::image_gen_targets`]：
//! 先试 primary，失败再试 fallback。成功图片写入工作区 `generated/images/`。

use std::path::{Path, PathBuf};

use home::{generated_dir, GeneratedKind};
use providers::interactions_http::{
    google_interactions_image, InteractionImagePart, InteractionImageRequest, InteractionVideoInput,
};
use providers::ProviderConfig;
use providers::types::media::GeneratedImage;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::{ImageGenCreds, ToolContext};
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

const MAX_VIDEO_BYTES: usize = 20 * 1024 * 1024;

/// `image_gen` tool args.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ImageGenArgs {
    /// Detailed image prompt (required): subject, setting, composition, lighting, materials, style, mood—not a short summary. Pass thinking drafts verbatim.
    pub prompt: String,
    /// Short title for the filename; default "Image".
    #[serde(default)]
    pub title: Option<String>,
    /// Aspect ratio when the user asks (e.g. 16:9). Common: `1:1` / `16:9` / `9:16` / `4:3` / `3:4`.
    #[serde(default)]
    pub aspect_ratio: Option<String>,
    /// Resolution tier when the user asks: `0.5K` / `1K` / `2K` / `4K` (uppercase K).
    #[serde(default)]
    pub image_size: Option<String>,
    /// Reference image workspace paths (max 14).
    #[serde(default)]
    pub reference_images: Option<Vec<String>>,
    /// Previous Interactions session id (multi-turn edit).
    #[serde(default)]
    pub previous_interaction_id: Option<String>,
    /// Enable Google Search grounding.
    #[serde(default)]
    pub google_search: bool,
    /// Enable image search (requires `google_search=true`).
    #[serde(default)]
    pub image_search: bool,
    /// Thinking depth: `minimal` / `high`.
    #[serde(default)]
    pub thinking_level: Option<String>,
    /// External video URL (mutually exclusive with `video`).
    #[serde(default)]
    pub video_uri: Option<String>,
    /// Workspace-relative generated video path (mutually exclusive with `video_uri`).
    #[serde(default)]
    pub video: Option<String>,
}

fn validate_image_gen_args(args: &ImageGenArgs) -> anyhow::Result<()> {
    if args.prompt.trim().is_empty() {
        anyhow::bail!("image_gen 需要 prompt 参数");
    }
    if let Some(sz) = args
        .image_size
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        match sz {
            "0.5K" | "1K" | "2K" | "4K" => {}
            _ => anyhow::bail!("image_size 无效: {sz}（仅支持 0.5K / 1K / 2K / 4K，须大写 K）"),
        }
    }
    if let Some(level) = args
        .thinking_level
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
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

/// 仅规范化显式传入的 `image_size` 大小写（如 `2k` → `2K`）；缺省字段保持不填。
fn normalize_image_gen_args(args: &mut ImageGenArgs) {
    if let Some(sz) = args.image_size.as_deref() {
        if let Some(norm) = normalize_image_size_token(sz) {
            args.image_size = Some(norm);
        }
    }
}

fn normalize_image_size_token(s: &str) -> Option<String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "0.5k" => Some("0.5K".into()),
        "1k" => Some("1K".into()),
        "2k" => Some("2K".into()),
        "4k" => Some("4K".into()),
        _ => None,
    }
}

fn has_advanced_interactions_args(args: &ImageGenArgs) -> bool {
    args.image_size
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty())
        || args
            .reference_images
            .as_ref()
            .is_some_and(|refs| refs.iter().any(|p| !p.trim().is_empty()))
        || args
            .previous_interaction_id
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty())
        || args.google_search
        || args.image_search
        || args
            .thinking_level
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty())
        || args
            .video_uri
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty())
        || args
            .video
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty())
}

/// 向注册表登记 `image_gen` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "image_gen".to_string(),
        toolset: "image_gen".to_string(),
        description: "Generate or edit images (Gemini Interactions; OpenAI fallback is prompt-only). Clarify subject/style before generating. Use aspect_ratio/image_size fields—not only prompt. Saves under generated/images/."
            .to_string(),
        schema: schema_for_args::<ImageGenArgs>(),
        check_fn: None,
        icon: "palette",
            ..ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["image_gen"],
    async_ctx: dispatch,
}

/// 依次尝试 primary / fallback 凭据生成图片，返回本地路径与所用模型信息。
///
/// # 错误
/// 无可用 Provider、全部尝试失败，或缺少 `prompt`。
pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<common::ToolOutput> {
    let mut parsed: ImageGenArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("image_gen 参数无效: {e}"))?;
    normalize_image_gen_args(&mut parsed);
    validate_image_gen_args(&parsed)?;

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
        match generate_one(ctx, &parsed, creds).await {
            Ok(output) => return Ok(output),
            Err(err) => {
                errors.push(format!("{} ({}): {err}", creds.provider, creds.model));
            }
        }
    }

    anyhow::bail!("图片生成失败：{}", errors.join("；"))
}

/// 使用单一凭据生成图片并落盘。
async fn generate_one(
    ctx: &ToolContext<'_>,
    args: &ImageGenArgs,
    creds: &ImageGenCreds,
) -> anyhow::Result<common::ToolOutput> {
    if creds.provider == "google" {
        return generate_one_google(ctx, args, creds).await;
    }
    generate_one_openai_compat(ctx, args, creds).await
}

async fn generate_one_google(
    ctx: &ToolContext<'_>,
    args: &ImageGenArgs,
    creds: &ImageGenCreds,
) -> anyhow::Result<common::ToolOutput> {
    let prompt = args.prompt.trim();
    let config = ProviderConfig {
        api_key: creds.api_key.clone(),
        base_url: if creds.base_url.trim().is_empty() {
            None
        } else {
            Some(creds.base_url.clone())
        },
        model: creds.model.clone(),
        ..ProviderConfig::default()
    };

    let reference_images = if let Some(refs) = &args.reference_images {
        refs.iter()
            .filter(|p| !p.trim().is_empty())
            .map(|p| load_reference_image(ctx, p))
            .collect::<anyhow::Result<Vec<_>>>()?
    } else {
        Vec::new()
    };

    let video = if let Some(uri) = opt_path(args.video_uri.as_deref()) {
        Some(InteractionVideoInput::Uri {
            uri: uri.to_string(),
            mime_type: "video/mp4".to_string(),
        })
    } else if let Some(path) = opt_path(args.video.as_deref()) {
        Some(load_video_input(ctx, path)?)
    } else {
        None
    };

    let req = InteractionImageRequest {
        prompt: prompt.to_string(),
        aspect_ratio: opt_owned(args.aspect_ratio.as_deref()),
        image_size: opt_owned(args.image_size.as_deref()),
        mime_type: None,
        reference_images,
        previous_interaction_id: opt_owned(args.previous_interaction_id.as_deref()),
        google_search: args.google_search,
        image_search: args.image_search,
        thinking_level: opt_owned(args.thinking_level.as_deref()),
        video,
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()?;

    let result = google_interactions_image(&client, &config, &req).await?;
    let rel = save_generated_image(ctx, &result.image, args.title.as_deref())?;

    let mut out = format!(
        "图片已生成：{rel}\nprovider={}\nmodel={}\ninteraction_id={}",
        creds.provider, creds.model, result.interaction_id
    );
    if let Some(text) = &result.output_text {
        out.push_str(&format!("\noutput_text: {text}"));
    }
    if let Some(sug) = &result.search_suggestions {
        out.push_str(&format!("\nsearch_suggestions: {sug}"));
    }
    out.push_str(&format!(
        "\nhint: 可用作 video_gen 的 image / last_frame / reference_images（单路径可放进数组，工作区相对路径）；多轮编辑可传 previous_interaction_id=\"{}\"",
        result.interaction_id
    ));
    Ok(super::media_out::media_output(
        out,
        common::MediaKind::Image,
        &rel,
        &result.image.mime_type,
        "图片已生成",
    ))
}

async fn generate_one_openai_compat(
    ctx: &ToolContext<'_>,
    args: &ImageGenArgs,
    creds: &ImageGenCreds,
) -> anyhow::Result<common::ToolOutput> {
    let provider = ctx
        .providers
        .get(&creds.provider)
        .ok_or_else(|| anyhow::anyhow!("未知 Provider: {}", creds.provider))?;

    let prompt = args.prompt.trim();
    let config = ProviderConfig {
        api_key: creds.api_key.clone(),
        base_url: if creds.base_url.trim().is_empty() {
            None
        } else {
            Some(creds.base_url.clone())
        },
        model: creds.model.clone(),
        ..ProviderConfig::default()
    };

    let images = provider.generate_image(prompt, &config).await?;
    let img = images
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("未返回图片数据"))?;

    let rel = save_generated_image(ctx, &img, args.title.as_deref())?;
    let mut out = format!(
        "图片已生成：{rel}\nprovider={}\nmodel={}\nhint: 可用作 video_gen 的 image / last_frame / reference_images（单路径可放进数组，工作区相对路径）",
        creds.provider, creds.model
    );
    if has_advanced_interactions_args(args) {
        out.push_str(
            "\nnote: OpenAI 路径忽略 Interactions 高级参数（image_size/reference_images/…）",
        );
    }
    Ok(super::media_out::media_output(
        out,
        common::MediaKind::Image,
        &rel,
        &img.mime_type,
        "图片已生成",
    ))
}

fn save_generated_image(
    ctx: &ToolContext<'_>,
    img: &GeneratedImage,
    title: Option<&str>,
) -> anyhow::Result<String> {
    let dir = generated_dir(&ctx.workspace_dir, GeneratedKind::Images);
    std::fs::create_dir_all(&dir)?;
    let ext = if img.mime_type.contains("jpeg") || img.mime_type.contains("jpg") {
        "jpg"
    } else if img.mime_type.contains("webp") {
        "webp"
    } else {
        "png"
    };
    let filename = super::media_out::generated_media_filename(title, "图片", ext);
    let path = dir.join(filename);
    std::fs::write(&path, &img.data)?;
    Ok(rel_workspace(ctx, &path))
}

fn opt_path(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

fn opt_owned(s: Option<&str>) -> Option<String> {
    opt_path(s).map(|s| s.to_string())
}

fn rel_workspace(ctx: &ToolContext<'_>, path: &Path) -> String {
    path.strip_prefix(&ctx.workspace_dir)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string())
}

fn resolve_workspace_file(ctx: &ToolContext<'_>, input: &str) -> anyhow::Result<PathBuf> {
    let p = PathBuf::from(input);
    let path = if p.is_absolute() {
        p
    } else {
        ctx.workspace_dir.join(input)
    };
    let canon_ws = ctx
        .workspace_dir
        .canonicalize()
        .unwrap_or_else(|_| ctx.workspace_dir.clone());
    let canon = path
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("文件不存在: {}", path.display()))?;
    if !canon.starts_with(&canon_ws) {
        anyhow::bail!("文件必须位于工作区内: {}", path.display());
    }
    if !canon.is_file() {
        anyhow::bail!("文件不存在: {}", path.display());
    }
    Ok(canon)
}

fn load_reference_image(
    ctx: &ToolContext<'_>,
    relative: &str,
) -> anyhow::Result<InteractionImagePart> {
    let path = resolve_workspace_file(ctx, relative)?;
    let data = std::fs::read(&path)
        .map_err(|e| anyhow::anyhow!("读取参考图失败 {}: {e}", path.display()))?;
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("image.jpg");
    Ok(InteractionImagePart {
        data,
        mime_type: mime_from_name(filename).to_string(),
    })
}

fn check_video_byte_len(len: usize) -> anyhow::Result<()> {
    if len > MAX_VIDEO_BYTES {
        anyhow::bail!(
            "视频文件过大（{} 字节，最大 {} MiB）",
            len,
            MAX_VIDEO_BYTES / (1024 * 1024)
        );
    }
    Ok(())
}

fn load_video_input(
    ctx: &ToolContext<'_>,
    relative: &str,
) -> anyhow::Result<InteractionVideoInput> {
    let path = resolve_workspace_file(ctx, relative)?;
    let meta = std::fs::metadata(&path)?;
    check_video_byte_len(meta.len() as usize)?;
    let data = std::fs::read(&path)
        .map_err(|e| anyhow::anyhow!("读取视频失败 {}: {e}", path.display()))?;
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("video.mp4");
    Ok(InteractionVideoInput::Bytes {
        data,
        mime_type: video_mime_from_name(filename).to_string(),
    })
}

fn mime_from_name(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else {
        "image/jpeg"
    }
}

fn video_mime_from_name(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".webm") {
        "video/webm"
    } else if lower.ends_with(".mov") {
        "video/quicktime"
    } else {
        "video/mp4"
    }
}

#[cfg(test)]
mod arg_tests {
    use super::*;

    #[test]
    fn rejects_lowercase_image_size() {
        let a = ImageGenArgs {
            prompt: "x".into(),
            title: None,
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
            title: None,
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
            title: None,
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
            title: None,
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

    #[test]
    fn normalize_lowercase_image_size() {
        let mut a = ImageGenArgs {
            prompt: "cat".into(),
            title: None,
            aspect_ratio: None,
            image_size: Some("2k".into()),
            reference_images: None,
            previous_interaction_id: None,
            google_search: false,
            image_search: false,
            thinking_level: None,
            video_uri: None,
            video: None,
        };
        normalize_image_gen_args(&mut a);
        assert_eq!(a.image_size.as_deref(), Some("2K"));
        assert!(validate_image_gen_args(&a).is_ok());
    }

    fn base_args() -> ImageGenArgs {
        ImageGenArgs {
            prompt: "x".into(),
            title: None,
            aspect_ratio: None,
            image_size: None,
            reference_images: None,
            previous_interaction_id: None,
            google_search: false,
            image_search: false,
            thinking_level: None,
            video_uri: None,
            video: None,
        }
    }

    #[test]
    fn detects_advanced_interactions_args() {
        let base = base_args();
        assert!(!has_advanced_interactions_args(&base));

        let cases: Vec<(&str, ImageGenArgs)> = vec![
            (
                "image_size",
                ImageGenArgs {
                    image_size: Some("1K".into()),
                    ..base.clone()
                },
            ),
            (
                "reference_images",
                ImageGenArgs {
                    reference_images: Some(vec!["generated/images/ref.png".into()]),
                    ..base.clone()
                },
            ),
            (
                "previous_interaction_id",
                ImageGenArgs {
                    previous_interaction_id: Some("int-abc".into()),
                    ..base.clone()
                },
            ),
            (
                "google_search",
                ImageGenArgs {
                    google_search: true,
                    ..base.clone()
                },
            ),
            (
                "image_search",
                ImageGenArgs {
                    google_search: true,
                    image_search: true,
                    ..base.clone()
                },
            ),
            (
                "thinking_level",
                ImageGenArgs {
                    thinking_level: Some("high".into()),
                    ..base.clone()
                },
            ),
            (
                "video_uri",
                ImageGenArgs {
                    video_uri: Some("https://www.youtube.com/watch?v=x".into()),
                    ..base.clone()
                },
            ),
            (
                "video",
                ImageGenArgs {
                    video: Some("generated/videos/a.mp4".into()),
                    ..base.clone()
                },
            ),
        ];
        for (name, args) in cases {
            assert!(
                has_advanced_interactions_args(&args),
                "expected advanced for {name}"
            );
        }
    }
}

#[cfg(test)]
mod path_tests {
    use super::*;
    use crate::context::{ImageGenTargets, ToolContext};
    use home::{generated_dir, GeneratedKind};
    use memory::MemoryManager;
    use providers::registry::ProviderRegistry;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn image_gen_target_dir_is_images() {
        let d = generated_dir(Path::new("/ws"), GeneratedKind::Images);
        assert!(d.ends_with("generated/images"));
    }

    #[test]
    fn resolve_workspace_file_rejects_escape() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().join("ws");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), b"x").unwrap();

        let mut memory = MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions =
            session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
        let targets = ImageGenTargets::default();
        let providers = ProviderRegistry::new();
        let ctx = ToolContext {
            memory: &mut memory,
            sessions: &sessions,
            memory_dir: dir.path().to_path_buf(),
            workspace_dir: ws,
            project_root: None,
            image_gen_targets: &targets,
            providers: &providers,
            session_id: "test".into(),
            turn_id: None,
            chat_api_key: String::new(),
            chat_base_url: String::new(),
            chat_provider: String::new(),
            chat_model: String::new(),
            chat_targets: vec![],
            execution: None,
            hook_bus: None,
        };

        let err = resolve_workspace_file(&ctx, "../outside/secret.txt").unwrap_err();
        assert!(err.to_string().contains("工作区内"), "unexpected: {err}");
    }

    #[test]
    fn resolve_workspace_file_accepts_in_workspace() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("ok.txt"), b"ok").unwrap();

        let mut memory = MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions =
            session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
        let targets = ImageGenTargets::default();
        let providers = ProviderRegistry::new();
        let ctx = ToolContext {
            memory: &mut memory,
            sessions: &sessions,
            memory_dir: dir.path().to_path_buf(),
            workspace_dir: ws.clone(),
            project_root: None,
            image_gen_targets: &targets,
            providers: &providers,
            session_id: "test".into(),
            turn_id: None,
            chat_api_key: String::new(),
            chat_base_url: String::new(),
            chat_provider: String::new(),
            chat_model: String::new(),
            chat_targets: vec![],
            execution: None,
            hook_bus: None,
        };

        let path = resolve_workspace_file(&ctx, "ok.txt").unwrap();
        assert!(path.ends_with("ok.txt"));
    }

    #[test]
    fn check_video_byte_len_rejects_over_limit() {
        let err = check_video_byte_len(MAX_VIDEO_BYTES + 1).unwrap_err();
        assert!(err.to_string().contains("视频文件过大"));
    }

    #[test]
    fn check_video_byte_len_accepts_at_limit() {
        assert!(check_video_byte_len(MAX_VIDEO_BYTES).is_ok());
    }
}
