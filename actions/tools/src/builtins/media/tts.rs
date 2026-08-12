//! 文本转语音：Google Gemini Interactions TTS 优先，OpenAI `audio/speech` 备用。
//!
//! Google 凭证来自 [`ToolContext::image_gen_targets`]（与出图共用 Google key）；
//! 配置了 Google 时失败**不**回退 OpenAI。无 Google 时才走 OpenAI
//!（依次尝试 targets、当前聊天 OpenAI、`OPENAI_API_KEY`）。
//! 音频写入工作区 `generated/audio/`。

use home::{generated_dir, GeneratedKind};
use providers::compat::openai_compatible_base;
use providers::interactions_http::{
    build_tts_input, google_interactions_tts, InteractionSpeechConfig, InteractionTtsRequest,
};
use providers::media_http::default_tts_model;
use providers::minimax::tts_http::{minimax_tts, MiniMaxTtsRequest, VoiceSetting};
use providers::ProviderConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// Multi-speaker entry (names should match roles in the transcript).
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TtsSpeaker {
    /// Speaker name (appears in text / transcript).
    pub speaker: String,
    /// Google preset voice, e.g. `Kore` / `Puck`.
    pub voice: String,
}

/// Arguments for the `tts` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TtsArgs {
    /// Text to synthesize (required; may include markers like `[whispers]`).
    pub text: String,
    /// Short title for the filename; default "Speech".
    #[serde(default)]
    pub title: Option<String>,
    /// Single-speaker voice: Google presets like `Kore`; OpenAI path may use `alloy`, etc.
    /// If `speakers` is also set, `speakers` wins.
    #[serde(default)]
    pub voice: Option<String>,
    /// Multi-speaker (max 2); Google Interactions only.
    #[serde(default)]
    pub speakers: Option<Vec<TtsSpeaker>>,
    /// Director / style notes (accent, tone, pacing); Google Interactions only.
    #[serde(default)]
    pub style: Option<String>,
    /// Stream generation and aggregate to disk; Google Interactions only (default false).
    #[serde(default)]
    pub stream: Option<bool>,
}

/// 向注册表登记 `tts` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "speech_gen".to_string(),
        toolset: "speech_gen".to_string(),
        description: "Convert text to speech. Google: Gemini TTS (multi-speaker, style, stream). OpenAI: /audio/speech fallback. Advanced features are Google-only."
            .to_string(),
        schema: schema_for_args::<TtsArgs>(),
        check_fn: None,
        icon: "mic",
            ..ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["speech_gen"],
    async_ctx: dispatch,
}

/// 请求 TTS，将音频保存到工作区并返回路径。
pub async fn dispatch(
    ctx: &ToolContext<'_>,
    args: &serde_json::Value,
) -> anyhow::Result<common::ToolOutput> {
    let parsed: TtsArgs =
        serde_json::from_value(args.clone()).map_err(|e| anyhow::anyhow!("tts 参数无效: {e}"))?;
    let text = parsed.text.trim();
    if text.is_empty() {
        anyhow::bail!("tts 需要 text");
    }

    if let Some(ref speakers) = parsed.speakers {
        if speakers.len() > 2 {
            anyhow::bail!("tts speakers 最多 2 个");
        }
        for (i, s) in speakers.iter().enumerate() {
            if s.speaker.trim().is_empty() || s.voice.trim().is_empty() {
                anyhow::bail!("tts speakers[{i}] 需要非空 speaker 与 voice");
            }
        }
    }

    let stream = parsed.stream.unwrap_or(false);
    let style = parsed
        .style
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let advanced = parsed.speakers.is_some() || style.is_some() || parsed.stream == Some(true);

    if let Some(creds) = ctx.image_gen_targets.google() {
        return synthesize_google(ctx, text, &parsed, style, stream, creds).await;
    }

    if let Some(creds) = ctx.image_gen_targets.minimax() {
        return synthesize_minimax(ctx, text, &parsed, creds).await;
    }

    let voice = parsed
        .voice
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("alloy");
    let output = synthesize_openai(ctx, text, voice, parsed.title.as_deref()).await?;
    if advanced {
        // Append note to the text portion of the ToolOutput
        let output = match output {
            common::ToolOutput::Media { text, assets } => common::ToolOutput::Media {
                text: format!(
                    "{text}\nnote: speakers/style/stream 仅 Google Interactions TTS 生效"
                ),
                assets,
            },
            other => other,
        };
        Ok(output)
    } else {
        Ok(output)
    }
}

async fn synthesize_google(
    ctx: &ToolContext<'_>,
    text: &str,
    parsed: &TtsArgs,
    style: Option<&str>,
    stream: bool,
    creds: &crate::context::ImageGenCreds,
) -> anyhow::Result<common::ToolOutput> {
    let model = if creds.tts_model.trim().is_empty() {
        default_tts_model().to_string()
    } else {
        creds.tts_model.trim().to_string()
    };
    let speech_config = resolve_speech_config(parsed)?;
    let input = build_tts_input(text, style);
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
    let req = InteractionTtsRequest {
        model: model.clone(),
        input,
        speech_config,
        stream,
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let result = google_interactions_tts(&client, &config, &req).await?;
    let dir = generated_dir(&ctx.workspace_dir, GeneratedKind::Audio);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(super::media_out::generated_media_filename(
        parsed.title.as_deref(),
        "语音",
        "wav",
    ));
    std::fs::write(&path, &result.wav_bytes)?;
    let rel = path
        .strip_prefix(&ctx.workspace_dir)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string());
    Ok(super::media_out::media_output(
        format!(
            "语音已生成：{rel}\nprovider=google\nmodel={model}\ninteraction_id={}\nstream={}",
            result.interaction_id, stream
        ),
        common::MediaKind::Audio,
        &rel,
        "audio/wav",
        "语音已生成",
    ))
}

async fn synthesize_minimax(
    ctx: &ToolContext<'_>,
    text: &str,
    parsed: &TtsArgs,
    creds: &crate::context::ImageGenCreds,
) -> anyhow::Result<common::ToolOutput> {
    let model = if creds.tts_model.trim().is_empty() {
        providers::minimax::defaults::DEFAULT_TTS_MODEL.to_string()
    } else {
        creds.tts_model.trim().to_string()
    };
    let voice_id = parsed
        .voice
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("")
        .to_string();
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
    let req = MiniMaxTtsRequest {
        model: model.clone(),
        text: text.to_string(),
        voice_setting: VoiceSetting {
            voice_id,
            ..VoiceSetting::default()
        },
        output_format: "url".to_string(),
        ..MiniMaxTtsRequest::default()
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;
    let result = minimax_tts(&client, &config, &req).await?;

    let dir = generated_dir(&ctx.workspace_dir, GeneratedKind::Audio);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(super::media_out::generated_media_filename(
        parsed.title.as_deref(),
        "语音",
        "mp3",
    ));
    std::fs::write(&path, &result.audio_bytes)?;
    let rel = path
        .strip_prefix(&ctx.workspace_dir)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string());
    Ok(super::media_out::media_output(
        format!(
            "语音已生成：{rel}\nprovider=minimax\nmodel={model}\nduration_ms={}",
            result.duration_ms
        ),
        common::MediaKind::Audio,
        &rel,
        &result.mime_type,
        "语音已生成",
    ))
}

fn resolve_speech_config(parsed: &TtsArgs) -> anyhow::Result<Vec<InteractionSpeechConfig>> {
    if let Some(ref speakers) = parsed.speakers {
        if !speakers.is_empty() {
            return Ok(speakers
                .iter()
                .map(|s| InteractionSpeechConfig {
                    speaker: Some(s.speaker.trim().to_string()),
                    voice: normalize_google_voice(&s.voice),
                })
                .collect());
        }
    }
    let voice = parsed
        .voice
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Kore");
    Ok(vec![InteractionSpeechConfig {
        speaker: None,
        voice: normalize_google_voice(voice),
    }])
}

fn normalize_google_voice(voice: &str) -> String {
    let v = voice.trim();
    if v.is_empty()
        || matches!(
            v.to_ascii_lowercase().as_str(),
            "alloy" | "echo" | "fable" | "onyx" | "nova" | "shimmer"
        )
    {
        "Kore".to_string()
    } else {
        v.to_string()
    }
}

async fn synthesize_openai(
    ctx: &ToolContext<'_>,
    text: &str,
    voice: &str,
    title: Option<&str>,
) -> anyhow::Result<common::ToolOutput> {
    let (api_key, base) = resolve_openai_tts(ctx)?;
    let url = format!("{base}/audio/speech");
    let body = serde_json::json!({
        "model": "gpt-4o-mini-tts",
        "input": text,
        "voice": voice,
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .bearer_auth(&api_key)
        .json(&body)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI TTS HTTP {status}: {err}");
    }
    let bytes = resp.bytes().await?;
    let dir = generated_dir(&ctx.workspace_dir, GeneratedKind::Audio);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(super::media_out::generated_media_filename(
        title, "语音", "mp3",
    ));
    std::fs::write(&path, &bytes)?;
    let rel = path
        .strip_prefix(&ctx.workspace_dir)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string());
    Ok(super::media_out::media_output(
        format!("语音已生成：{rel}\nprovider=openai\nmodel=gpt-4o-mini-tts"),
        common::MediaKind::Audio,
        &rel,
        "audio/mpeg",
        "语音已生成",
    ))
}

fn resolve_openai_tts(ctx: &ToolContext<'_>) -> anyhow::Result<(String, String)> {
    if let Some(creds) = ctx.image_gen_targets.openai() {
        let base = if creds.base_url.trim().is_empty() {
            "https://api.openai.com/v1".to_string()
        } else {
            openai_compatible_base(&creds.base_url)
        };
        return Ok((creds.api_key.clone(), base));
    }
    if !ctx.credentials.api_key.is_empty() && ctx.credentials.provider == "openai" {
        let base = if ctx.credentials.base_url.trim().is_empty() {
            "https://api.openai.com/v1".to_string()
        } else {
            openai_compatible_base(&ctx.credentials.base_url)
        };
        return Ok((ctx.credentials.api_key.clone(), base));
    }
    let env_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    if env_key.is_empty() {
        anyhow::bail!("无可用 OpenAI API Key（且未配置 Google）");
    }
    Ok((env_key, "https://api.openai.com/v1".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_maps_openai_voices() {
        assert_eq!(normalize_google_voice("alloy"), "Kore");
        assert_eq!(normalize_google_voice("Puck"), "Puck");
    }

    #[test]
    fn resolve_speakers_prefers_speakers_over_voice() {
        let args = TtsArgs {
            text: "Joe: hi".into(),
            title: None,
            voice: Some("Fenrir".into()),
            speakers: Some(vec![TtsSpeaker {
                speaker: "Joe".into(),
                voice: "Kore".into(),
            }]),
            style: None,
            stream: None,
        };
        let sc = resolve_speech_config(&args).unwrap();
        assert_eq!(sc.len(), 1);
        assert_eq!(sc[0].speaker.as_deref(), Some("Joe"));
        assert_eq!(sc[0].voice, "Kore");
    }
}
