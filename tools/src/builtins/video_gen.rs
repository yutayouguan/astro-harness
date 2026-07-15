//! 视频生成：Google OpenAI 兼容 `…/videos`（Veo），写入工作区 `generated/`。

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
    /// 时长秒数（可选；使用参考图/首尾帧/续拍时建议 8）。
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
    /// 角色/风格参考图（工作区相对路径，可选，最多 1 张）。
    #[serde(default)]
    pub reference_image: Option<String>,
    /// 首帧图路径（图生视频 / 插值，可选）。
    #[serde(default)]
    pub image: Option<String>,
    /// 尾帧图路径（插值；必须同时提供 `image`）。
    #[serde(default)]
    pub last_frame: Option<String>,
}

/// 向注册表登记 `video_gen` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "video_gen".to_string(),
        toolset: "video_gen".to_string(),
        description: "Generate a short video via Google Veo (OpenAI-compatible videos API). Optional: aspect_ratio, duration_seconds, resolution, negative_prompt, style, extend_video_id, reference_image, image, last_frame (last_frame requires image). Advanced modes usually need duration_seconds=8. May take several minutes."
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
        anyhow::bail!("video_gen: last_frame 需要同时提供 image（首帧）");
    }

    let needs_eight = last_frame_path.is_some()
        || reference_path.is_some()
        || extend_id.is_some();
    let mut duration = parsed.duration_seconds.filter(|s| *s > 0);
    if needs_eight && duration.is_none() {
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

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;
    let video = google_openai_generate_video(&client, prompt, &config, &extras).await?;

    let dir = ctx.workspace_dir.join("generated");
    std::fs::create_dir_all(&dir)?;
    let filename = format!(
        "vid-{}-{}.mp4",
        chrono::Local::now().format("%Y%m%d-%H%M%S"),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let path = dir.join(&filename);
    std::fs::write(&path, &video.data)?;
    Ok(format!(
        "视频已生成：{}\nprovider=google\nmodel={model}\noperation_id={}",
        path.display(),
        video.operation_id
    ))
}

fn opt_path(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

fn opt_owned(s: Option<&str>) -> Option<String> {
    opt_path(s).map(|s| s.to_string())
}

fn load_image_part(ctx: &ToolContext<'_>, relative: &str) -> anyhow::Result<VideoImagePart> {
    let path = ctx.workspace_dir.join(relative);
    if !path.is_file() {
        anyhow::bail!("视频图片不存在: {}", path.display());
    }
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
