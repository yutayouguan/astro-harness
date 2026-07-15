//! 音频理解：Google Interactions 原生；OpenAI Chat describe / Whisper transcribe。
//!
//! 参考：https://ai.google.dev/gemini-api/docs/audio?hl=zh-cn

use base64::Engine;
use providers::http_stream::openai_compatible_base;
use providers::interactions_http::{
    default_audio_understand_prompt, google_interactions_audio, AudioMediaKind, AudioMediaPart,
    AudioUnderstandMode,
};
use providers::media_http::{
    default_vision_model, default_whisper_model, openai_audio_describe,
    openai_audio_transcriptions, whisper_text_to_transcribe_json,
};
use providers::trait_::ProviderConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::{ImageGenCreds, ToolContext};
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `audio_understand` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AudioUnderstandArgs {
    /// 工作区相对路径、http(s) 音频 URL、或 YouTube URL。
    pub audio_url: String,
    #[serde(default)]
    pub prompt: Option<String>,
    /// describe | transcribe；缺省 describe。
    #[serde(default)]
    pub mode: Option<String>,
    /// 可选时间窗起点 MM:SS。
    #[serde(default)]
    pub start: Option<String>,
    /// 可选时间窗终点 MM:SS。
    #[serde(default)]
    pub end: Option<String>,
}

/// 向注册表登记 `audio_understand` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "audio_understand".to_string(),
        toolset: "audio_understand".to_string(),
        description: "Analyze audio. Modes: describe (default), transcribe (structured JSON with speakers/timestamps/emotion). Pass audio_url (workspace path, http(s), or YouTube). Google uses Interactions API; OpenAI uses chat input_audio for describe and Whisper for transcribe."
            .to_string(),
        schema: schema_for_args::<AudioUnderstandArgs>(),
        check_fn: None,
        icon: "ear",
    });
}

/// 解析音频并调用理解模型，返回描述文本或结构化转写 JSON。
pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: AudioUnderstandArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("audio_understand 参数无效: {e}"))?;
    let audio_url = parsed.audio_url.trim();
    if audio_url.is_empty() {
        anyhow::bail!("audio_understand 需要 audio_url");
    }
    let mode = AudioUnderstandMode::parse(parsed.mode.as_deref().unwrap_or(""))?;
    validate_mmss(parsed.start.as_deref())?;
    validate_mmss(parsed.end.as_deref())?;

    let mut prompt = parsed
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_audio_understand_prompt(mode))
        .to_string();
    if let (Some(s), Some(e)) = (parsed.start.as_deref(), parsed.end.as_deref()) {
        prompt.push_str(&format!("\nProvide content from {s} to {e}."));
    } else if let Some(s) = parsed.start.as_deref() {
        prompt.push_str(&format!("\nStart from {s}."));
    } else if let Some(e) = parsed.end.as_deref() {
        prompt.push_str(&format!("\nEnd at {e}."));
    }

    let is_yt = is_youtube_url(audio_url);
    let mut errors = Vec::new();

    if let Some(creds) = ctx.image_gen_targets.google() {
        match call_google(ctx, creds, &prompt, audio_url, mode, is_yt).await {
            Ok(msg) => return Ok(msg),
            Err(e) => errors.push(format!("google: {e}")),
        }
    }

    if is_yt {
        errors.push("openai: YouTube 仅支持 Google Interactions".into());
        anyhow::bail!(
            "audio_understand 失败：{}。请配置 Google API Key（YouTube 需 Interactions）。",
            errors.join("；")
        );
    }

    match call_openai(ctx, &prompt, audio_url, mode).await {
        Ok(msg) => Ok(msg),
        Err(e) => {
            errors.push(format!("openai: {e}"));
            anyhow::bail!(
                "audio_understand 失败：{}。请配置 Google 或 OpenAI API Key。",
                errors.join("；")
            )
        }
    }
}

fn validate_mmss(v: Option<&str>) -> anyhow::Result<()> {
    let Some(s) = v.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let re_ok = s.len() <= 5
        && s.contains(':')
        && s.split_once(':')
            .map(|(a, b)| {
                a.chars().all(|c| c.is_ascii_digit())
                    && b.chars().all(|c| c.is_ascii_digit())
                    && b.len() == 2
                    && !a.is_empty()
                    && a.len() <= 2
            })
            .unwrap_or(false);
    if !re_ok {
        anyhow::bail!("时间格式无效（需要 MM:SS）: {s}");
    }
    Ok(())
}

/// 判断是否为 YouTube URL：须为 http(s)，且 host 等于/以 `.youtube.com` 结尾，或等于 `youtu.be`。
/// 非 URL（本地路径）或包含 `youtube.com` 字样的文件名/主机（如 `notyoutube.com`）均不算 YouTube。
fn is_youtube_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    host == "youtube.com" || host.ends_with(".youtube.com") || host == "youtu.be"
}

/// 从 http(s) URL 中剥离 query/fragment 后取路径部分；解析失败则原样返回。
fn url_path_without_query(url: &str) -> String {
    match reqwest::Url::parse(url) {
        Ok(parsed) => parsed.path().to_string(),
        Err(_) => url.to_string(),
    }
}

/// 依据 URL 路径（已剥离 query/fragment）推断 mime；非路径部分不参与扩展名判断。
fn mime_from_url(url: &str) -> &'static str {
    mime_from_path(std::path::Path::new(&url_path_without_query(url)))
}

/// 依据 URL 路径的 basename（已剥离 query/fragment）生成 Whisper 下载文件名；缺省 `audio.mp3`。
fn filename_from_url(url: &str) -> String {
    url_path_without_query(url)
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("audio.mp3")
        .to_string()
}

fn mime_from_path(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "wav" => "audio/wav",
        "mp3" => "audio/mp3",
        "aiff" | "aif" => "audio/aiff",
        "aac" => "audio/aac",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        _ => "audio/mp3",
    }
}

async fn call_google(
    ctx: &ToolContext<'_>,
    creds: &ImageGenCreds,
    prompt: &str,
    audio_url: &str,
    mode: AudioUnderstandMode,
    is_yt: bool,
) -> anyhow::Result<String> {
    let model = if creds.vision_model.trim().is_empty() {
        "gemini-3.5-flash".to_string()
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
    let media = resolve_google_media(ctx, audio_url, is_yt)?;
    let client = reqwest::Client::new();
    let text = google_interactions_audio(&client, prompt, &media, mode, &config).await?;
    Ok(format!(
        "{text}\nprovider=google\nmodel={model}\nmode={}",
        mode.as_str()
    ))
}

fn resolve_google_media(
    ctx: &ToolContext<'_>,
    audio_url: &str,
    is_yt: bool,
) -> anyhow::Result<AudioMediaPart> {
    if is_yt {
        return Ok(AudioMediaPart::Uri {
            media_type: AudioMediaKind::Video,
            mime_type: "video/mp4".into(),
            uri: audio_url.to_string(),
        });
    }
    if audio_url.starts_with("http://") || audio_url.starts_with("https://") {
        return Ok(AudioMediaPart::Uri {
            media_type: AudioMediaKind::Audio,
            mime_type: mime_from_url(audio_url).to_string(),
            uri: audio_url.to_string(),
        });
    }
    let path = crate::path_safe::resolve_safe(&ctx.workspace_dir, audio_url)?;
    if !path.exists() {
        anyhow::bail!("本地文件不存在: {}", path.display());
    }
    let bytes = std::fs::read(&path)
        .map_err(|e| anyhow::anyhow!("读取音频失败 {}: {e}", path.display()))?;
    let mime = mime_from_path(&path);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(AudioMediaPart::Inline {
        media_type: AudioMediaKind::Audio,
        mime_type: mime.to_string(),
        data_b64: b64,
    })
}

async fn call_openai(
    ctx: &ToolContext<'_>,
    prompt: &str,
    audio_url: &str,
    mode: AudioUnderstandMode,
) -> anyhow::Result<String> {
    let (bytes, mime, filename) = load_audio_bytes(ctx, audio_url).await?;
    match mode {
        AudioUnderstandMode::Describe => {
            let (config, model) = resolve_openai_describe_config(ctx)?;
            let client = reqwest::Client::new();
            let text = openai_audio_describe(&client, prompt, &bytes, &mime, &config).await?;
            Ok(format!(
                "{text}\nprovider=openai\nmodel={model}\nmode=describe"
            ))
        }
        AudioUnderstandMode::Transcribe => {
            let (config, model) = resolve_openai_whisper_config(ctx)?;
            let client = reqwest::Client::new();
            let text = openai_audio_transcriptions(&client, &bytes, &filename, &config).await?;
            let json = whisper_text_to_transcribe_json(&text);
            Ok(format!(
                "{}\nprovider=openai\nmodel={model}\nmode=transcribe\nfallback=openai",
                serde_json::to_string_pretty(&json)?
            ))
        }
    }
}

async fn load_audio_bytes(
    ctx: &ToolContext<'_>,
    audio_url: &str,
) -> anyhow::Result<(Vec<u8>, String, String)> {
    if audio_url.starts_with("http://") || audio_url.starts_with("https://") {
        let client = reqwest::Client::new();
        let resp = client
            .get(audio_url)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("下载音频失败: {e}"))?;
        if !resp.status().is_success() {
            anyhow::bail!("下载音频 HTTP {}", resp.status());
        }
        let mime = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("audio/mp3")
            .to_string();
        let bytes = resp.bytes().await?.to_vec();
        let filename = filename_from_url(audio_url);
        return Ok((bytes, mime, filename));
    }
    let path = crate::path_safe::resolve_safe(&ctx.workspace_dir, audio_url)?;
    if !path.exists() {
        anyhow::bail!("本地文件不存在: {}", path.display());
    }
    let bytes = std::fs::read(&path)?;
    let mime = mime_from_path(&path).to_string();
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("audio.mp3")
        .to_string();
    Ok((bytes, mime, filename))
}

fn resolve_openai_describe_config(
    ctx: &ToolContext<'_>,
) -> anyhow::Result<(ProviderConfig, String)> {
    if let Some(creds) = ctx.image_gen_targets.openai() {
        let model = if creds.vision_model.trim().is_empty() {
            default_vision_model("openai").to_string()
        } else {
            creds.vision_model.trim().to_string()
        };
        let base = if creds.base_url.trim().is_empty() {
            Some("https://api.openai.com/v1".into())
        } else {
            Some(openai_compatible_base(&creds.base_url))
        };
        return Ok((
            ProviderConfig {
                api_key: creds.api_key.clone(),
                base_url: base,
                model: model.clone(),
                ..ProviderConfig::default()
            },
            model,
        ));
    }
    if !ctx.chat_api_key.is_empty() && ctx.chat_provider == "openai" {
        let model = if ctx.chat_model.trim().is_empty() {
            default_vision_model("openai").to_string()
        } else {
            ctx.chat_model.trim().to_string()
        };
        let base = if ctx.chat_base_url.trim().is_empty() {
            Some("https://api.openai.com/v1".into())
        } else {
            Some(openai_compatible_base(&ctx.chat_base_url))
        };
        return Ok((
            ProviderConfig {
                api_key: ctx.chat_api_key.clone(),
                base_url: base,
                model: model.clone(),
                ..ProviderConfig::default()
            },
            model,
        ));
    }
    let env_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    if env_key.is_empty() {
        anyhow::bail!("无可用 OpenAI API Key");
    }
    let model = default_vision_model("openai").to_string();
    Ok((
        ProviderConfig {
            api_key: env_key,
            base_url: Some("https://api.openai.com/v1".into()),
            model: model.clone(),
            ..ProviderConfig::default()
        },
        model,
    ))
}

fn resolve_openai_whisper_config(
    ctx: &ToolContext<'_>,
) -> anyhow::Result<(ProviderConfig, String)> {
    let model = default_whisper_model().to_string();
    if let Some(creds) = ctx.image_gen_targets.openai() {
        let base = if creds.base_url.trim().is_empty() {
            Some("https://api.openai.com/v1".into())
        } else {
            Some(openai_compatible_base(&creds.base_url))
        };
        return Ok((
            ProviderConfig {
                api_key: creds.api_key.clone(),
                base_url: base,
                model: model.clone(),
                ..ProviderConfig::default()
            },
            model,
        ));
    }
    if !ctx.chat_api_key.is_empty() && ctx.chat_provider == "openai" {
        let base = if ctx.chat_base_url.trim().is_empty() {
            Some("https://api.openai.com/v1".into())
        } else {
            Some(openai_compatible_base(&ctx.chat_base_url))
        };
        return Ok((
            ProviderConfig {
                api_key: ctx.chat_api_key.clone(),
                base_url: base,
                model: model.clone(),
                ..ProviderConfig::default()
            },
            model,
        ));
    }
    let env_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
    if env_key.is_empty() {
        anyhow::bail!("无可用 OpenAI API Key");
    }
    Ok((
        ProviderConfig {
            api_key: env_key,
            base_url: Some("https://api.openai.com/v1".into()),
            model: model.clone(),
            ..ProviderConfig::default()
        },
        model,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_bad_timestamp() {
        assert!(validate_mmss(Some("3")).is_err());
        assert!(validate_mmss(Some("02:30")).is_ok());
    }

    #[test]
    fn accepts_missing_or_empty_timestamp() {
        assert!(validate_mmss(None).is_ok());
        assert!(validate_mmss(Some("")).is_ok());
        assert!(validate_mmss(Some("  ")).is_ok());
    }

    #[test]
    fn rejects_seconds_with_wrong_width() {
        assert!(validate_mmss(Some("1:5")).is_err());
        assert!(validate_mmss(Some("1:500")).is_err());
        assert!(validate_mmss(Some(":30")).is_err());
    }

    #[test]
    fn youtube_detect() {
        assert!(is_youtube_url("https://youtu.be/abc"));
        assert!(is_youtube_url("https://www.youtube.com/watch?v=abc"));
        assert!(is_youtube_url("HTTPS://M.YOUTUBE.COM/watch?v=abc"));
        assert!(is_youtube_url("http://youtube.com/watch?v=abc"));
        assert!(!is_youtube_url("https://example.com/a.mp3"));
    }

    /// Finding 2：伪装/相似域名与本地文件名不应被误判为 YouTube。
    #[test]
    fn youtube_detect_rejects_lookalike_and_local() {
        assert!(!is_youtube_url("https://notyoutube.com/watch?v=abc"));
        assert!(!is_youtube_url("https://youtube.com.evil.com/watch?v=abc"));
        assert!(!is_youtube_url("https://evilyoutube.com/watch?v=abc"));
        // 本地文件名中含 "youtube.com" 字样，但不是 URL，绝不能判定为 YouTube。
        assert!(!is_youtube_url("clips/youtube.com/local.mp3"));
        assert!(!is_youtube_url("youtube.com.mp3"));
        assert!(!is_youtube_url("ftp://youtube.com/x"));
    }

    #[test]
    fn mime_from_path_maps_common_extensions() {
        assert_eq!(mime_from_path(std::path::Path::new("a.wav")), "audio/wav");
        assert_eq!(mime_from_path(std::path::Path::new("a.M4A")), "audio/mp4");
        assert_eq!(mime_from_path(std::path::Path::new("a.unknown")), "audio/mp3");
    }

    /// Finding 3：mime 须依据 URL 路径部分，剥离 `?query`/`#fragment` 后再取扩展名。
    #[test]
    fn mime_from_url_strips_query_and_fragment() {
        assert_eq!(
            mime_from_url("https://example.com/a/b.wav?x=1&y=2"),
            "audio/wav"
        );
        assert_eq!(
            mime_from_url("https://example.com/clip.m4a#t=10"),
            "audio/mp4"
        );
        assert_eq!(mime_from_url("https://example.com/noext"), "audio/mp3");
    }

    /// Finding 3：Whisper 下载文件名须取路径 basename（剥离 query），缺省 `audio.mp3`。
    #[test]
    fn filename_from_url_strips_query_and_defaults() {
        assert_eq!(
            filename_from_url("https://example.com/dir/song.mp3?x=1&y=2"),
            "song.mp3"
        );
        assert_eq!(
            filename_from_url("https://example.com/dir/song.wav#frag"),
            "song.wav"
        );
        assert_eq!(filename_from_url("https://example.com/"), "audio.mp3");
        assert_eq!(filename_from_url("https://example.com"), "audio.mp3");
    }

    #[test]
    fn parses_mode_default_and_invalid() {
        assert_eq!(
            AudioUnderstandMode::parse("").unwrap(),
            AudioUnderstandMode::Describe
        );
        assert!(AudioUnderstandMode::parse("bogus").is_err());
    }
}

#[cfg(test)]
mod path_escape_tests {
    use super::*;
    use crate::context::{ImageGenTargets, ToolContext};
    use memory::MemoryManager;
    use providers::registry::ProviderRegistry;
    use tempfile::TempDir;

    fn build_ctx<'a>(
        memory: &'a mut MemoryManager,
        memory_dir: std::path::PathBuf,
        workspace_dir: std::path::PathBuf,
        targets: &'a ImageGenTargets,
        providers: &'a ProviderRegistry,
    ) -> ToolContext<'a> {
        ToolContext {
            memory,
            memory_dir,
            workspace_dir,
            project_root: None,
            image_gen_targets: targets,
            providers,
            session_id: "test".into(),
            turn_id: None,
            chat_api_key: String::new(),
            chat_base_url: String::new(),
            chat_provider: String::new(),
            chat_model: String::new(),
            chat_targets: vec![],
            delegate_runner: None,
            async_spawner: None,
            orchestration_spawner: None,
        }
    }

    /// Finding 1：本地路径须经 `resolve_safe` 校验，`..` 越界须被拒绝（而非读取 workspace 外文件）。
    #[tokio::test]
    async fn load_audio_bytes_rejects_path_escape() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().join("ws");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.mp3"), b"top-secret-bytes").unwrap();

        let mut memory = MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let targets = ImageGenTargets::default();
        let providers = ProviderRegistry::new();
        let ctx = build_ctx(&mut memory, dir.path().to_path_buf(), ws, &targets, &providers);

        let err = load_audio_bytes(&ctx, "../outside/secret.mp3")
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("越界") || err.to_string().contains(".."),
            "unexpected error: {err}"
        );
    }

    /// Finding 1：workspace 内的相对路径应正常解析并读取到文件内容。
    #[tokio::test]
    async fn load_audio_bytes_accepts_in_workspace_path() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("ok.wav"), b"ok-bytes").unwrap();

        let mut memory = MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let targets = ImageGenTargets::default();
        let providers = ProviderRegistry::new();
        let ctx = build_ctx(&mut memory, dir.path().to_path_buf(), ws, &targets, &providers);

        let (bytes, mime, filename) = load_audio_bytes(&ctx, "ok.wav").await.unwrap();
        assert_eq!(bytes, b"ok-bytes");
        assert_eq!(mime, "audio/wav");
        assert_eq!(filename, "ok.wav");
    }

    /// Finding 1：`resolve_google_media` 同样须拒绝越界本地路径。
    #[test]
    fn resolve_google_media_rejects_path_escape() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().join("ws");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.mp3"), b"top-secret-bytes").unwrap();

        let mut memory = MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let targets = ImageGenTargets::default();
        let providers = ProviderRegistry::new();
        let ctx = build_ctx(&mut memory, dir.path().to_path_buf(), ws, &targets, &providers);

        let err = resolve_google_media(&ctx, "../outside/secret.mp3", false).unwrap_err();
        assert!(
            err.to_string().contains("越界") || err.to_string().contains(".."),
            "unexpected error: {err}"
        );
    }
}
