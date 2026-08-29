//! 视频生成：Google 原生 Veo `predictLongRunning`。

use std::path::{Path, PathBuf};

use home::{generated_dir, GeneratedKind};
use providers::media_http::{
    default_video_model, google_native_generate_video, VideoGenExtras, VideoImagePart,
};
use providers::ProviderConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `video_gen` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct VideoGenArgs {
    /// 详细镜头提示词（必填）：主体/动作、镜头、场景、光线、风格、氛围——不要简短概述。可直接传递思考草稿。
    pub prompt: String,
    /// 文件名短标题；默认 "Video"。
    #[serde(default)]
    pub title: Option<String>,
    /// 宽高比，如 `16:9` / `9:16` / `1:1` / `4:3`。
    #[serde(default)]
    pub aspect_ratio: Option<String>,
    /// 时长（秒）。MiniMax H3：4-15；旧版：6 或 10。（Google Veo 在 extend/refs/last_frame/1080p/4K 时强制为 8）。
    #[serde(default)]
    pub duration_seconds: Option<u32>,
    /// 输出分辨率：MiniMax H3：`768P` / `2K`；旧版：`720P` / `768P` / `1080P`；Google：`720p` / `1080p` / `4K`。
    #[serde(default)]
    pub resolution: Option<String>,
    /// 负面提示词（可选；原生 Veo 忽略）。
    #[serde(default)]
    pub negative_prompt: Option<String>,
    /// 视觉风格：`cinematic` / `creative`（原生 Veo 忽略）。
    #[serde(default)]
    pub style: Option<String>,
    /// 从工作区视频路径续拍（推荐用于后续镜头）。
    #[serde(default)]
    pub extend_video: Option<String>,
    #[serde(default)]
    pub extend_video_uri: Option<String>,
    /// 从先前操作 ID 续拍。
    #[serde(default)]
    pub extend_video_id: Option<String>,
    /// 参考图片路径（Google Veo：最多 3 张；MiniMax H3：最多 9 张）。
    #[serde(default)]
    pub reference_images: Option<Vec<String>>,
    /// 单张参考图片路径（合并到 reference_images）。
    #[serde(default)]
    pub reference_image: Option<String>,
    /// 首帧图片路径（图生视频/插值）。
    #[serde(default)]
    pub image: Option<String>,
    /// 尾帧图片路径（需同时提供 `image`）。
    #[serde(default)]
    pub last_frame: Option<String>,
    /// 人物生成：`allow_adult` / `allow_all` / `dont_allow`。
    #[serde(default)]
    pub person_generation: Option<String>,
    #[serde(default)]
    pub seed: Option<i64>,
    /// 主体参考图片路径（MiniMax 旧版 S2V：角色一致性）。
    #[serde(default)]
    pub subject_reference_image: Option<String>,
    /// 禁用提示词优化（MiniMax：更精确控制）。
    #[serde(default)]
    pub disable_prompt_optimizer: Option<bool>,
    /// 参考视频路径/URL（仅 MiniMax H3，最多 3 个，每个 2-15s）。
    #[serde(default)]
    pub reference_videos: Option<Vec<String>>,
    /// 参考音频路径/URL（仅 MiniMax H3，最多 3 个，每个 2-15s）。
    #[serde(default)]
    pub reference_audios: Option<Vec<String>>,
    /// 生成前启用 H3 Context-IR 提示词增强（仅 MiniMax H3）。
    #[serde(default)]
    pub enhance_prompt: Option<bool>,
}

/// 向注册表登记 `video_gen` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "video_gen".to_string(),
        toolset: "video_gen".to_string(),
        description: "Generate a short video via Google Veo. Clarify shot direction first. Prefer extend_video for next shots. Saves under generated/videos/."
            .to_string(),
        schema: schema_for_args::<VideoGenArgs>(),
        check_fn: None,
        icon: "clapperboard",
            ..ToolEntry::lifecycle_defaults().deferred()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["video_gen"],
    async_ctx: dispatch,
}

/// 调用 Google 视频接口并落盘。
pub async fn dispatch(
    ctx: &ToolContext<'_>,
    args: &serde_json::Value,
) -> anyhow::Result<types::ToolOutput> {
    let parsed: VideoGenArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("video_gen 参数无效: {e}"))?;
    let prompt = parsed.prompt.trim();
    if prompt.is_empty() {
        anyhow::bail!("video_gen 需要 prompt");
    }

    if let Some(creds) = ctx.image_gen_targets.minimax() {
        return dispatch_minimax_video(ctx, &parsed, creds).await;
    }

    let mut refs: Vec<String> = parsed
        .reference_images
        .unwrap_or_default()
        .into_iter()
        .filter_map(|s| {
            let t = s.trim().to_string();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        })
        .collect();
    if let Some(one) = opt_path(parsed.reference_image.as_deref()) {
        refs.push(one.to_string());
    }
    if refs.len() > 3 {
        anyhow::bail!("video_gen: reference_images 最多 3 张");
    }

    let image_path = opt_path(parsed.image.as_deref());
    let last_frame_path = opt_path(parsed.last_frame.as_deref());
    let extend_path = opt_path(parsed.extend_video.as_deref());
    let extend_uri = opt_path(parsed.extend_video_uri.as_deref());
    let extend_id = opt_path(parsed.extend_video_id.as_deref());

    if last_frame_path.is_some() && image_path.is_none() {
        anyhow::bail!(
            "video_gen: last_frame 需要同时提供 image（首帧）。建议先用 image_gen 生成首尾帧。"
        );
    }
    if !refs.is_empty() && (image_path.is_some() || last_frame_path.is_some()) {
        anyhow::bail!("video_gen: reference_images 不能与 image/last_frame 同时使用");
    }
    let has_extend = extend_path.is_some() || extend_uri.is_some() || extend_id.is_some();
    if has_extend && (image_path.is_some() || last_frame_path.is_some() || !refs.is_empty()) {
        anyhow::bail!("video_gen: 续拍不能与 image/last_frame/reference_images 同时使用");
    }

    let res_lower = parsed
        .resolution
        .as_deref()
        .map(|s| s.trim().to_ascii_lowercase());
    let needs_eight = has_extend
        || !refs.is_empty()
        || last_frame_path.is_some()
        || matches!(res_lower.as_deref(), Some("1080p") | Some("4k"));
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
            "未找到可用的 Google 或 MiniMax 提供商。请在「模型提供商」中开启并配置 API Key。"
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

    let mut reference_parts = Vec::with_capacity(refs.len());
    for r in &refs {
        reference_parts.push(load_image_part(ctx, r)?);
    }

    let extend_video = extend_path.map(|p| load_video_part(ctx, p)).transpose()?;
    let (extend_video, extend_video_uri, extend_video_id) = if extend_video.is_some() {
        (extend_video, None, None)
    } else if extend_uri.is_some() {
        (None, extend_uri.map(|s| s.to_string()), None)
    } else {
        (None, None, extend_id.map(|s| s.to_string()))
    };

    let extras = VideoGenExtras {
        aspect_ratio: opt_owned(parsed.aspect_ratio.as_deref()),
        duration_seconds: duration,
        resolution: opt_owned(parsed.resolution.as_deref()),
        negative_prompt: opt_owned(parsed.negative_prompt.as_deref()),
        style: opt_owned(parsed.style.as_deref()),
        extend_video_id,
        extend_video_uri,
        extend_video,
        person_generation: opt_owned(parsed.person_generation.as_deref()),
        seed: parsed.seed,
        image: image_path.map(|p| load_image_part(ctx, p)).transpose()?,
        last_frame: last_frame_path
            .map(|p| load_image_part(ctx, p))
            .transpose()?,
        reference_images: reference_parts,
    };

    let has_native_ignored = parsed
        .negative_prompt
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty())
        || parsed
            .style
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty());

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
        google_native_generate_video(&client, prompt, &config, &extras, Some(&mut on_progress))
            .await
            .map_err(|e| anyhow::anyhow!("Google Veo 视频失败: {e}"))?;
    on_progress("api_path=native");
    let api_path = "native";

    let filename =
        super::media_out::generated_media_filename(parsed.title.as_deref(), "视频", "mp4");
    let path = dir.join(&filename);
    std::fs::write(&path, &video.data)?;
    let rel = rel_workspace(ctx, &path);
    let _ = std::fs::write(
        &progress_path,
        format!("status=saved path={rel}\n{}", progress_lines.join("\n")),
    );

    let op_label = "operation_name";
    let mut lines = vec![
        format!("视频已生成：{rel}"),
        "provider=google".to_string(),
        format!("model={model}"),
        format!("api_path={api_path}"),
        format!("{op_label}={}", video.operation_id),
    ];
    if let Some(uri) = &video.video_uri {
        lines.push(format!("video_uri={uri}"));
    }
    if has_native_ignored {
        lines.push("native_ignored=negative_prompt,style".to_string());
    }
    if !duration_note.is_empty() {
        lines.push(duration_note.trim_end().to_string());
    }
    lines.push(format!("progress:\n{}", progress_lines.join("\n")));
    lines.push(format!(
        "next_shot_hint: video_gen(prompt=\"…\", extend_video=\"{rel}\", duration_seconds=8, aspect_ratio=…)"
    ));
    Ok(super::media_out::media_output(
        lines.join("\n"),
        types::MediaKind::Video,
        &rel,
        if video.mime_type.trim().is_empty() {
            "video/mp4"
        } else {
            video.mime_type.as_str()
        },
        "视频已生成",
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

fn resolve_workspace_file(
    ctx: &ToolContext<'_>,
    input: &str,
    kind: &str,
) -> anyhow::Result<PathBuf> {
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
        .map_err(|_| anyhow::anyhow!("{kind}不存在: {}", path.display()))?;
    if !canon.starts_with(&canon_ws) {
        anyhow::bail!("{kind}必须位于工作区内: {}", path.display());
    }
    if !canon.is_file() {
        anyhow::bail!("{kind}不存在: {}", path.display());
    }
    Ok(canon)
}

fn load_image_part(ctx: &ToolContext<'_>, relative: &str) -> anyhow::Result<VideoImagePart> {
    let path = resolve_workspace_file(ctx, relative, "视频图片")?;
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

fn load_video_part(ctx: &ToolContext<'_>, relative: &str) -> anyhow::Result<VideoImagePart> {
    let path = resolve_workspace_file(ctx, relative, "续拍视频")?;
    let bytes = std::fs::read(&path)
        .map_err(|e| anyhow::anyhow!("读取视频失败 {}: {e}", path.display()))?;
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("extend.mp4")
        .to_string();
    let mime = mime_from_video_name(&filename);
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

fn mime_from_video_name(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".webm") {
        "video/webm"
    } else if lower.ends_with(".mov") {
        "video/quicktime"
    } else {
        "video/mp4"
    }
}

async fn dispatch_minimax_video(
    ctx: &ToolContext<'_>,
    parsed: &VideoGenArgs,
    creds: &crate::context::ImageGenCreds,
) -> anyhow::Result<types::ToolOutput> {
    use providers::minimax::video_http::{
        is_h3_model, minimax_create_video, minimax_download_video, minimax_download_video_url,
        minimax_enhance_prompt, minimax_query_video, MiniMaxVideoRequest, VideoSubjectRef,
        VideoTaskStatus,
    };

    let model = if creds.video_model.trim().is_empty() {
        providers::minimax::defaults::DEFAULT_VIDEO_MODEL.to_string()
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
    let encode_image = |p: &str| -> anyhow::Result<String> {
        let abs = if std::path::Path::new(p).is_absolute() {
            std::path::PathBuf::from(p)
        } else {
            ctx.workspace_dir.join(p)
        };
        let bytes = std::fs::read(&abs)
            .map_err(|e| anyhow::anyhow!("读取图片失败 {}: {e}", abs.display()))?;
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
        let mime = mime_from_name(p);
        Ok(format!("data:{mime};base64,{b64}"))
    };

    let h3 = is_h3_model(&model);

    let first_frame = parsed
        .image
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(encode_image)
        .transpose()?;

    let last_frame = parsed
        .last_frame
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(encode_image)
        .transpose()?;

    // H3 参考图片（从 reference_images 和 reference_image 合并）
    let mut ref_images: Vec<String> = Vec::new();
    if let Some(ref imgs) = parsed.reference_images {
        for img in imgs {
            let trimmed = img.trim();
            if !trimmed.is_empty() {
                ref_images.push(encode_image(trimmed)?);
            }
        }
    }
    if let Some(ref img) = parsed.reference_image {
        let trimmed = img.trim();
        if !trimmed.is_empty() {
            ref_images.push(encode_image(trimmed)?);
        }
    }

    // H3 参考视频
    let ref_videos: Vec<String> = parsed
        .reference_videos
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter_map(|v| {
            let t = v.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        })
        .collect();

    // H3 参考音频
    let ref_audios: Vec<String> = parsed
        .reference_audios
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter_map(|a| {
            let t = a.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        })
        .collect();

    // 旧版 S2V 主体参考（非 H3 时使用）
    let subject_ref = if !h3 {
        parsed
            .subject_reference_image
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|p| -> anyhow::Result<Vec<VideoSubjectRef>> {
                let encoded = encode_image(p)?;
                Ok(vec![VideoSubjectRef {
                    ref_type: "character".to_string(),
                    images: vec![encoded],
                }])
            })
            .transpose()?
    } else {
        None
    };

    let duration = if h3 {
        parsed.duration_seconds.unwrap_or(6).clamp(4, 15)
    } else {
        parsed.duration_seconds.unwrap_or(6).clamp(6, 10)
    };

    let resolution = parsed
        .resolution
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("768P")
        .to_string();

    let ratio = parsed.aspect_ratio.clone();

    let mut req = MiniMaxVideoRequest {
        model: model.clone(),
        prompt: parsed.prompt.clone(),
        first_frame_image: first_frame,
        last_frame_image: last_frame,
        reference_images: ref_images,
        reference_videos: ref_videos,
        reference_audios: ref_audios,
        subject_reference: subject_ref,
        duration,
        resolution,
        ratio,
        prompt_optimizer: !parsed.disable_prompt_optimizer.unwrap_or(false),
        ..MiniMaxVideoRequest::default()
    };

    // H3 Context-IR 提示词增强
    if h3 && parsed.enhance_prompt.unwrap_or(false) {
        eprintln!("[minimax video] H3 Context-IR 提示词增强中…");
        if let Ok(ir) = minimax_enhance_prompt(&reqwest::Client::new(), &config, &req).await {
            if !ir.enhanced_prompt.is_empty() {
                eprintln!("[minimax video] 提示词增强完成");
                req.prompt = ir.enhanced_prompt;
            }
        }
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()?;

    let task_id = minimax_create_video(&client, &config, &req).await?;
    eprintln!("[minimax video] 任务已创建: {task_id}，轮询中…");

    let mut poll_count = 0u32;
    let max_polls = 90u32; // H3 最长 15s 可能需要更久
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        poll_count += 1;
        let result = minimax_query_video(&client, &config, &task_id, &model).await?;
        match result.status {
            VideoTaskStatus::Success => {
                eprintln!("[minimax video] 视频生成完成，正在下载…");
                let video = if let Some(ref url) = result.download_url {
                    minimax_download_video_url(&client, url).await?
                } else if let Some(ref file_id) = result.file_id {
                    minimax_download_video(&client, &config, file_id).await?
                } else {
                    anyhow::bail!("视频生成成功但无下载途径");
                };

                let dir = generated_dir(&ctx.workspace_dir, GeneratedKind::Videos);
                std::fs::create_dir_all(&dir)?;
                let path = dir.join(super::media_out::generated_media_filename(
                    parsed.title.as_deref(),
                    "视频",
                    "mp4",
                ));
                std::fs::write(&path, &video.data)?;
                let rel = path
                    .strip_prefix(&ctx.workspace_dir)
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|_| path.display().to_string());
                return Ok(super::media_out::media_output(
                    format!(
                        "视频已生成：{rel}\nprovider=minimax\nmodel={model}\ntask_id={task_id}"
                    ),
                    types::MediaKind::Video,
                    &rel,
                    "video/mp4",
                    "视频已生成",
                ));
            }
            VideoTaskStatus::Fail | VideoTaskStatus::Cancelled => {
                anyhow::bail!(
                    "MiniMax 视频生成失败 (task_id={task_id}, status={:?})",
                    result.status
                );
            }
            _ => {
                if poll_count >= max_polls {
                    anyhow::bail!(
                        "MiniMax 视频生成超时 (task_id={task_id}, 已轮询 {poll_count} 次)"
                    );
                }
                eprintln!(
                    "[minimax video] 视频生成中… ({:?}, 第 {poll_count}/{max_polls} 次)",
                    result.status
                );
            }
        }
    }
}
