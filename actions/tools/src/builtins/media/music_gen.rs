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
use providers::minimax::music_http::{minimax_generate_music, MiniMaxMusicRequest};
use providers::ProviderConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MusicGenArgs {
    /// Music description (genre/instruments/BPM/mood/structure). Theme only if no lyrics body.
    pub prompt: String,
    /// Short title for the filename; default "Music".
    #[serde(default)]
    pub title: Option<String>,
    /// Optional lyrics. If already written, pass them here (else Lyria invents). Prefer `[Verse]`/`[Chorus]` tags.
    #[serde(default)]
    pub lyrics: Option<String>,
    /// `clip` (default, ~30s) or `pro` (full song).
    #[serde(default)]
    pub model: Option<String>,
    /// Reference image workspace paths (max 10).
    #[serde(default)]
    pub reference_images: Option<Vec<String>>,
    /// `mp3` (default) or `wav` (pro only).
    #[serde(default)]
    pub format: Option<String>,
    /// Cover reference audio URL (MiniMax music-cover model).
    #[serde(default)]
    pub cover_audio_url: Option<String>,
    /// Auto-generate lyrics from prompt (MiniMax lyrics_optimizer).
    #[serde(default)]
    pub auto_lyrics: Option<bool>,
}

/// 将风格 prompt 与可选歌词合成 Lyria 输入。
/// 有歌词时追加 `Lyrics:` 段（官方推荐前缀），要求按所给歌词演唱。
pub fn compose_music_prompt(prompt: &str, lyrics: Option<&str>) -> String {
    let base = prompt.trim();
    let Some(lyrics) = lyrics.map(str::trim).filter(|s| !s.is_empty()) else {
        return base.to_string();
    };
    // 已含 Lyrics: 前缀则直接拼接，避免重复标签。
    let lyrics_block = if lyrics.to_ascii_lowercase().starts_with("lyrics:") {
        lyrics.to_string()
    } else {
        format!("Lyrics:\n{lyrics}")
    };
    format!("{base}\n\nSing the following lyrics exactly (do not rewrite):\n{lyrics_block}")
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
        description: "Generate music via Google Lyria 3. model=clip (short) or pro (full song). Pass lyrics if already written or Lyria invents its own. Google only. Writes generated/audio/.".to_string(),
        schema: schema_for_args::<MusicGenArgs>(),
        check_fn: None,
        icon: "music",
            ..ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["music_gen"],
    async_ctx: dispatch,
}

pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<common::ToolOutput> {
    let parsed: MusicGenArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("music_gen 参数无效: {e}"))?;
    if let Some(creds) = ctx.image_gen_targets.minimax() {
        return dispatch_minimax_music(ctx, &parsed, creds).await;
    }

    let creds = ctx.image_gen_targets.google().ok_or_else(|| {
        anyhow::anyhow!("music_gen 需要 Google 或 MiniMax API Key")
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
    let prompt = compose_music_prompt(parsed.prompt.as_str(), parsed.lyrics.as_deref());
    let req = InteractionMusicRequest {
        model: model_id.clone(),
        prompt,
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
    let path = dir.join(super::media_out::generated_media_filename(
        parsed.title.as_deref(),
        "音乐",
        ext,
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
    Ok(super::media_out::media_output(
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

async fn dispatch_minimax_music(
    ctx: &ToolContext<'_>,
    parsed: &MusicGenArgs,
    creds: &crate::context::ImageGenCreds,
) -> anyhow::Result<common::ToolOutput> {
    let model = if creds.music_model.trim().is_empty() {
        providers::minimax::defaults::DEFAULT_MUSIC_MODEL.to_string()
    } else {
        creds.music_model.trim().to_string()
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

    let is_instrumental = parsed.lyrics.as_deref().is_none()
        || parsed
            .lyrics
            .as_deref()
            .is_some_and(|l| l.trim().is_empty());

    let req = MiniMaxMusicRequest {
        model: model.clone(),
        prompt: parsed.prompt.clone(),
        lyrics: parsed.lyrics.as_deref().unwrap_or("").to_string(),
        output_format: "url".to_string(),
        is_instrumental,
        lyrics_optimizer: parsed.auto_lyrics.unwrap_or(false),
        audio_url: parsed.cover_audio_url.clone(),
        ..MiniMaxMusicRequest::default()
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()?;
    let result = minimax_generate_music(&client, &config, &req).await?;

    let dir = generated_dir(&ctx.workspace_dir, GeneratedKind::Audio);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(super::media_out::generated_media_filename(
        parsed.title.as_deref(),
        "音乐",
        "mp3",
    ));
    std::fs::write(&path, &result.audio_bytes)?;
    let rel = path
        .strip_prefix(&ctx.workspace_dir)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string());

    Ok(super::media_out::media_output(
        format!(
            "音乐已生成：{rel}\nprovider=minimax\nmodel={model}\nduration_ms={}",
            result.duration_ms
        ),
        common::MediaKind::Audio,
        &rel,
        &result.mime_type,
        "音乐已生成",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(
        prompt: &str,
        model: Option<&str>,
        format: Option<&str>,
        reference_images: Option<Vec<String>>,
    ) -> MusicGenArgs {
        MusicGenArgs {
            prompt: prompt.into(),
            title: None,
            lyrics: None,
            model: model.map(str::to_string),
            reference_images,
            format: format.map(str::to_string),
            cover_audio_url: None,
            auto_lyrics: None,
        }
    }

    #[test]
    fn compose_prompt_without_lyrics_is_passthrough() {
        assert_eq!(compose_music_prompt("  lofi beats  ", None), "lofi beats");
        assert_eq!(compose_music_prompt("jazz", Some("  ")), "jazz");
    }

    #[test]
    fn compose_prompt_with_lyrics_appends_lyrics_block() {
        let out = compose_music_prompt(
            "indie folk, female vocal",
            Some("[Verse]\nhello world\n[Chorus]\nsing along"),
        );
        assert!(out.starts_with("indie folk, female vocal"));
        assert!(out.contains("Sing the following lyrics exactly"));
        assert!(out.contains("Lyrics:\n[Verse]\nhello world\n[Chorus]\nsing along"));
    }

    #[test]
    fn compose_prompt_does_not_double_lyrics_prefix() {
        let out = compose_music_prompt("pop", Some("Lyrics:\n[Verse]\nhi"));
        assert!(out.contains("Lyrics:\n[Verse]\nhi"));
        assert_eq!(out.matches("Lyrics:").count(), 1);
    }

    #[test]
    fn reject_empty_prompt() {
        assert!(validate_music_gen_args(&args("  ", None, None, None), "").is_err());
    }

    #[test]
    fn reject_wav_on_clip() {
        let err = validate_music_gen_args(&args("lofi", Some("clip"), Some("wav"), None), "")
            .unwrap_err()
            .to_string();
        assert!(err.contains("wav") || err.contains("pro"));
    }

    #[test]
    fn accept_pro_wav() {
        let (model_id, fmt) =
            validate_music_gen_args(&args("piano", Some("pro"), Some("wav"), None), "").unwrap();
        assert_eq!(model_id, "lyria-3-pro-preview");
        assert_eq!(fmt, MusicAudioFormat::Wav);
    }

    #[test]
    fn reject_wav_for_configured_model_that_only_contains_pro() {
        let err = validate_music_gen_args(
            &args("piano", None, Some("wav"), None),
            "music-production-v1",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("wav") || err.contains("pro"));
    }

    #[test]
    fn reject_too_many_images() {
        let images = Some((0..11).map(|i| format!("a{i}.jpg")).collect());
        assert!(validate_music_gen_args(&args("x", None, None, images), "").is_err());
    }

    #[test]
    fn configured_model_is_used_when_tool_arg_is_missing() {
        let (model, _) =
            validate_music_gen_args(&args("piano", None, None, None), "lyria-3-pro-preview")
                .unwrap();
        assert_eq!(model, "lyria-3-pro-preview");
    }

    #[test]
    fn arbitrary_configured_model_is_passed_through() {
        let (model, _) =
            validate_music_gen_args(&args("piano", None, None, None), "custom-music-model-v7")
                .unwrap();
        assert_eq!(model, "custom-music-model-v7");
    }

    #[test]
    fn explicit_invalid_model_is_rejected() {
        assert!(validate_music_gen_args(
            &args("piano", Some("custom-music-model-v7"), None, None),
            ""
        )
        .is_err());
    }

    #[test]
    fn explicit_alias_overrides_configured_model() {
        let (model, _) = validate_music_gen_args(
            &args("piano", Some("clip"), None, None),
            "lyria-3-pro-preview",
        )
        .unwrap();
        assert_eq!(model, "lyria-3-clip-preview");
    }

    #[test]
    fn empty_configuration_falls_back_to_clip() {
        let (model, _) = validate_music_gen_args(&args("piano", None, None, None), "").unwrap();
        assert_eq!(model, "lyria-3-clip-preview");
    }
}
