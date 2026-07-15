//! 视频生成：Google OpenAI 兼容 `…/videos`（Veo），写入工作区 `generated/videos/`。

use std::path::{Path, PathBuf};

use memory::{generated_dir, GeneratedKind};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use providers::media_http::{
    default_video_model, google_openai_generate_video, VideoGenExtras, VideoImagePart,
};
use providers::trait_::ProviderConfig;

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `video_gen` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct VideoGenArgs {
    /// 视频画面的文字描述（不可为空）。
    pub prompt: String,
    /// 宽高比，如 `16:9` / `9:16`（可选）。
    #[serde(default)]
    pub aspect_ratio: Option<String>,
    /// 时长秒数（可选；参考图/首尾帧/续拍时强制为 8）。
    #[serde(default)]
    pub duration_seconds: Option<u32>,
    /// 输出分辨率（可选）：`720p` / `1080p` / `4K`。
    #[serde(default)]
    pub resolution: Option<String>,
    /// 负面提示：希望排除的内容（可选）。
    #[serde(default)]
    pub negative_prompt: Option<String>,
    /// 视觉风格（可选）：`cinematic` / `creative`。
    #[serde(default)]
    pub style: Option<String>,
    /// 续拍：已有视频的 operation id（可选）。
    #[serde(default)]
    pub extend_video_id: Option<String>,
    /// 角色/风格参考图（工作区相对或绝对路径，可选；不可与 image/last_frame 同用）。
    #[serde(default)]
    pub reference_image: Option<String>,
    /// 首帧图路径（建议先 image_gen；图生视频 / 插值）。
    #[serde(default)]
    pub image: Option<String>,
    /// 尾帧图路径（插值；必须同时提供 `image`；不可与 reference_image 同用）。
    #[serde(default)]
    pub last_frame: Option<String>,
    /// 人物生成策略（可选）：`allow_adult` / `allow_all` / `dont_allow`。
    #[serde(default)]
    pub person_generation: Option<String>,
    /// 随机种子（可选，整数）。
    #[serde(default)]
    pub seed: Option<i64>,
}

/// 向注册表登记 `video_gen` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "video_gen".to_string(),
        toolset: "video_gen".to_string(),
        description: "Generate a short video via Google Veo. Prefer image_gen for first/last frames, then pass image/last_frame. Result includes operation_id — use it as extend_video_id for the next shot. reference_image cannot combine with image/last_frame. Advanced modes force duration_seconds=8. Writes to generated/videos/."
            .to_string(),
        schema: schema_for_args::<VideoGenArgs>(),
        check_fn: None,
        icon: "clapperboard",
    });
}

/// 调用 Google 视频接口并落盘。
pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: VideoGenArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("video_gen 参数无效: {e}"))?;
    let prompt = parsed.prompt.trim();
    if prompt.is_empty() {
        anyhow::bail!("video_gen 需要 prompt");
    }

    let last_frame_path = opt_path(parsed.last_frame.as_deref());
    let image_path = opt_path(parsed.image.as_deref());
    let reference_path = opt_path(parsed.reference_image.as_deref());
    let extend_id = opt_path(parsed.extend_video_id.as_deref());

    if last_frame_path.is_some() && image_path.is_none() {
        anyhow::bail!("video_gen: last_frame 需要同时提供 image（首帧）。建议先用 image_gen 生成首尾帧。");
    }
    if reference_path.is_some() && (image_path.is_some() || last_frame_path.is_some()) {
        anyhow::bail!(
            "video_gen: reference_image 不能与 image/last_frame 同时使用；请二选一（参考图 或 首尾帧插值）。"
        );
    }

    let needs_eight = last_frame_path.is_some()
        || reference_path.is_some()
        || extend_id.is_some();
    let mut duration = parsed.duration_seconds.filter(|s| *s > 0);
    let mut duration_note = String::new();
    if needs_eight {
        if let Some(d) = duration {
            if d != 8 {
                duration_note =
                    format!("duration_note: duration_seconds={d}→8 (advanced mode enforced)\n");
            }
        }
        duration = Some(8);
    }

    let creds = ctx.image_gen_targets.google().ok_or_else(|| {
        anyhow::anyhow!(
            "未找到可用的 Google 提供商。请在「模型提供商」中开启 Google 并配置 API Key。"
        )
    })?;

    let model = if creds.video_model.trim().is_empty() {
        default_video_model().to_string()
    } else {
        creds.video_model.trim().to_string()
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

    let extras = VideoGenExtras {
        aspect_ratio: opt_owned(parsed.aspect_ratio.as_deref()),
        duration_seconds: duration,
        resolution: opt_owned(parsed.resolution.as_deref()),
        negative_prompt: opt_owned(parsed.negative_prompt.as_deref()),
        style: opt_owned(parsed.style.as_deref()),
        extend_video_id: extend_id.map(|s| s.to_string()),
        person_generation: opt_owned(parsed.person_generation.as_deref()),
        seed: parsed.seed,
        image: image_path
            .map(|p| load_image_part(ctx, p))
            .transpose()?,
        last_frame: last_frame_path
            .map(|p| load_image_part(ctx, p))
            .transpose()?,
        reference_image: reference_path
            .map(|p| load_image_part(ctx, p))
            .transpose()?,
    };

    let dir = generated_dir(&ctx.workspace_dir, GeneratedKind::Videos);
    std::fs::create_dir_all(&dir)?;
    let progress_path = dir.join("progress-latest.txt");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10 * 60))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()?;

    let mut progress_lines: Vec<String> = Vec::new();
    let mut on_progress = |msg: &str| {
        progress_lines.push(msg.to_string());
        let body = progress_lines.join("\n");
        let _ = std::fs::write(&progress_path, &body);
    };

    let video =
        google_openai_generate_video(&client, prompt, &config, &extras, Some(&mut on_progress))
            .await?;

    let filename = format!(
        "vid-{}-{}.mp4",
        chrono::Local::now().format("%Y%m%d-%H%M%S"),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let path = dir.join(&filename);
    std::fs::write(&path, &video.data)?;
    let rel = rel_workspace(ctx, &path);
    let _ = std::fs::write(
        &progress_path,
        format!("status=saved path={rel}\n{}", progress_lines.join("\n")),
    );

    Ok(format!(
        "视频已生成：{rel}\n\
         provider=google\n\
         model={model}\n\
         operation_id={}\n\
         {duration_note}\
         progress:\n{}\n\
         next_shot_hint: video_gen(prompt=\"…\", extend_video_id=\"{}\", duration_seconds=8, aspect_ratio=…)",
        video.operation_id,
        progress_lines.join("\n"),
        video.operation_id,
    ))
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
        .map_err(|_| anyhow::anyhow!("视频图片不存在: {}", path.display()))?;
    if !canon.starts_with(&canon_ws) {
        anyhow::bail!("视频图片必须位于工作区内: {}", path.display());
    }
    if !canon.is_file() {
        anyhow::bail!("视频图片不存在: {}", path.display());
    }
    Ok(canon)
}

fn load_image_part(ctx: &ToolContext<'_>, relative: &str) -> anyhow::Result<VideoImagePart> {
    let path = resolve_workspace_file(ctx, relative)?;
    let bytes = std::fs::read(&path)
        .map_err(|e| anyhow::anyhow!("读取图片失败 {}: {e}", path.display()))?;
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("image.jpg")
        .to_string();
    let mime = mime_from_name(&filename);
    Ok(VideoImagePart {
        bytes,
        filename,
        mime: mime.to_string(),
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
