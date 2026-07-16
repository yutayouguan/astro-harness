//! 音乐生成：Google Lyria 3 via Gemini Interactions（Google only）。
//!
//! 与本地播放工具 `music` 分离；无 Google 凭证不回退 OpenAI。
//! 产物写入 `generated/audio/music-*.{mp3|wav}`。

use std::path::PathBuf;

use base64::Engine;
use home::{generated_dir, GeneratedKind};
use providers::interactions_http::{
    google_interactions_music, music_extension, resolve_lyria_model_id, InteractionMusicRequest,
    MusicAudioFormat, MusicImagePart,
};
use providers::trait_::ProviderConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MusicGenArgs {
    /// 音乐描述（流派/乐器/BPM/结构等）。
    pub prompt: String,
    /// `clip`（默认，约 30s）或 `pro`（完整歌曲）。
    #[serde(default)]
    pub model: Option<String>,
    /// 参考图工作区路径，最多 10。
    #[serde(default)]
    pub reference_images: Option<Vec<String>>,
    /// `mp3`（默认）或 `wav`（仅 pro）。
    #[serde(default)]
    pub format: Option<String>,
}

pub fn validate_music_gen_args(
    args: &MusicGenArgs,
    configured_music_model: &str,
) -> anyhow::Result<(String, MusicAudioFormat)> {
    if args.prompt.trim().is_empty() {
        anyhow::bail!("music_gen 需要 prompt");
    }
    let requested = args
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let configured = configured_music_model.trim();
    let model_id = if let Some(requested) = requested {
        resolve_lyria_model_id(requested)?
    } else if !configured.is_empty() {
        configured.to_string()
    } else {
        resolve_lyria_model_id("clip")?
    };
    let is_pro = model_id == "lyria-3-pro-preview";

    let fmt_raw = args
        .format
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("mp3")
        .to_ascii_lowercase();
    let format = match fmt_raw.as_str() {
        "mp3" => MusicAudioFormat::Mp3,
        "wav" => MusicAudioFormat::Wav,
        other => anyhow::bail!("format 无效: {other}（mp3 | wav）"),
    };
    if format == MusicAudioFormat::Wav && !is_pro {
        anyhow::bail!("wav 仅 lyria-3-pro 支持（请设 model=pro）");
    }

    if let Some(refs) = &args.reference_images {
        let n = refs.iter().filter(|p| !p.trim().is_empty()).count();
        if n > 10 {
            anyhow::bail!("reference_images 最多 10 张");
        }
    }
    Ok((model_id, format))
}

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "music_gen".to_string(),
        toolset: "music_gen".to_string(),
        description: "Generate music with Google Lyria 3 (Interactions). model=clip|pro; optional reference_images (≤10); format=mp3|wav (wav requires pro). Google only — not local music playback. Writes generated/audio/.".to_string(),
        schema: schema_for_args::<MusicGenArgs>(),
        check_fn: None,
        icon: "music",
            ..ToolEntry::lifecycle_defaults()
    });
}

pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: MusicGenArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("music_gen 参数无效: {e}"))?;
    let creds = ctx.image_gen_targets.google().ok_or_else(|| {
        anyhow::anyhow!("music_gen 需要 Google API Key（未配置 Google，且不回退 OpenAI）")
    })?;
    let (model_id, format) = validate_music_gen_args(&parsed, &creds.music_model)?;

    let mut images = Vec::new();
    if let Some(refs) = &parsed.reference_images {
        for rel in refs.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            images.push(load_music_image(ctx, rel)?);
        }
    }

    let config = ProviderConfig {
        api_key: creds.api_key.clone(),
        base_url: if creds.base_url.trim().is_empty() {
            None
        } else {
            Some(creds.base_url.clone())
        },
        model: model_id.clone(),
        ..ProviderConfig::default()
};
    let req = InteractionMusicRequest {
        model: model_id.clone(),
        prompt: parsed.prompt.trim().to_string(),
        images,
        format,
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()?;
    let result = google_interactions_music(&client, &config, &req).await?;

    let ext = music_extension(&result.mime_type, format);
    let dir = generated_dir(&ctx.workspace_dir, GeneratedKind::Audio);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!(
        "music-{}-{}.{}",
        chrono::Local::now().format("%Y%m%d-%H%M%S"),
        &uuid::Uuid::new_v4().simple().to_string()[..8],
        ext
    ));
    std::fs::write(&path, &result.audio_bytes)?;
    let rel = path
        .strip_prefix(&ctx.workspace_dir)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string());

    let mime = result.mime_type.clone();
    let mut out = format!(
        "音乐已生成：{rel}\nprovider=google\nmodel={model_id}\ninteraction_id={}",
        result.interaction_id
    );
    if let Some(lyrics) = result.lyrics_text.filter(|s| !s.trim().is_empty()) {
        out.push_str("\nlyrics:\n");
        out.push_str(&lyrics);
    }
    Ok(super::media_out::with_generated_media(
        out,
        common::MediaKind::Audio,
        &rel,
        &mime,
        "音乐已生成",
    ))
}

fn load_music_image(ctx: &ToolContext<'_>, relative: &str) -> anyhow::Result<MusicImagePart> {
    let path = resolve_workspace_file(ctx, relative)?;
    let bytes = std::fs::read(&path)
        .map_err(|e| anyhow::anyhow!("读取参考图失败 {}: {e}", path.display()))?;
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("image.jpg");
    let mime = mime_from_name(filename);
    Ok(MusicImagePart {
        mime_type: mime.to_string(),
        data_base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_empty_prompt() {
        let args = MusicGenArgs {
            prompt: "  ".into(),
            model: None,
            reference_images: None,
            format: None,
        };
        assert!(validate_music_gen_args(&args, "").is_err());
    }

    #[test]
    fn reject_wav_on_clip() {
        let args = MusicGenArgs {
            prompt: "lofi".into(),
            model: Some("clip".into()),
            reference_images: None,
            format: Some("wav".into()),
        };
        let err = validate_music_gen_args(&args, "").unwrap_err().to_string();
        assert!(err.contains("wav") || err.contains("pro"));
    }

    #[test]
    fn accept_pro_wav() {
        let args = MusicGenArgs {
            prompt: "piano".into(),
            model: Some("pro".into()),
            reference_images: None,
            format: Some("wav".into()),
        };
        let (model_id, fmt) = validate_music_gen_args(&args, "").unwrap();
        assert_eq!(model_id, "lyria-3-pro-preview");
        assert_eq!(fmt, MusicAudioFormat::Wav);
    }

    #[test]
    fn reject_wav_for_configured_model_that_only_contains_pro() {
        let args = MusicGenArgs {
            prompt: "piano".into(),
            model: None,
            reference_images: None,
            format: Some("wav".into()),
        };
        let err = validate_music_gen_args(&args, "music-production-v1")
            .unwrap_err()
            .to_string();
        assert!(err.contains("wav") || err.contains("pro"));
    }

    #[test]
    fn reject_too_many_images() {
        let args = MusicGenArgs {
            prompt: "x".into(),
            model: None,
            reference_images: Some((0..11).map(|i| format!("a{i}.jpg")).collect()),
            format: None,
        };
        assert!(validate_music_gen_args(&args, "").is_err());
    }

    #[test]
    fn configured_model_is_used_when_tool_arg_is_missing() {
        let args = MusicGenArgs {
            prompt: "piano".into(),
            model: None,
            reference_images: None,
            format: None,
        };
        let (model, _) =
            validate_music_gen_args(&args, "lyria-3-pro-preview").unwrap();
        assert_eq!(model, "lyria-3-pro-preview");
    }

    #[test]
    fn arbitrary_configured_model_is_passed_through() {
        let args = MusicGenArgs {
            prompt: "piano".into(),
            model: None,
            reference_images: None,
            format: None,
        };
        let (model, _) =
            validate_music_gen_args(&args, "custom-music-model-v7").unwrap();
        assert_eq!(model, "custom-music-model-v7");
    }

    #[test]
    fn explicit_invalid_model_is_rejected() {
        let args = MusicGenArgs {
            prompt: "piano".into(),
            model: Some("custom-music-model-v7".into()),
            reference_images: None,
            format: None,
        };
        assert!(validate_music_gen_args(&args, "").is_err());
    }

    #[test]
    fn explicit_alias_overrides_configured_model() {
        let args = MusicGenArgs {
            prompt: "piano".into(),
            model: Some("clip".into()),
            reference_images: None,
            format: None,
        };
        let (model, _) =
            validate_music_gen_args(&args, "lyria-3-pro-preview").unwrap();
        assert_eq!(model, "lyria-3-clip-preview");
    }

    #[test]
    fn empty_configuration_falls_back_to_clip() {
        let args = MusicGenArgs {
            prompt: "piano".into(),
            model: None,
            reference_images: None,
            format: None,
        };
        let (model, _) = validate_music_gen_args(&args, "").unwrap();
        assert_eq!(model, "lyria-3-clip-preview");
    }
}
