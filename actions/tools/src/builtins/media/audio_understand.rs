//! 音频理解：Google Interactions 原生；OpenAI Chat describe / Whisper transcribe。
//!
//! 参考：https://ai.google.dev/gemini-api/docs/audio?hl=zh-cn

use base64::Engine;
use providers::compat::openai_compatible_base;
use providers::interactions_http::{
    default_audio_understand_prompt, google_interactions_audio, AudioMediaKind, AudioMediaPart,
    AudioUnderstandMode,
};
use providers::media_http::{
    default_vision_model, default_whisper_model, openai_audio_describe,
    openai_audio_transcriptions, whisper_text_to_transcribe_json,
};
use providers::ProviderConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::{ImageGenCreds, ToolContext};
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// 远程音频下载的超时时长。
const DOWNLOAD_TIMEOUT_SECS: u64 = 60;
/// 远程音频下载的字节数上限（25 MiB），适用于 `Content-Length` 预检与累计字节数双重校验。
const MAX_AUDIO_DOWNLOAD_BYTES: u64 = 25 * 1024 * 1024;

/// Arguments for the `audio_understand` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AudioUnderstandArgs {
    /// Workspace-relative path, http(s) audio URL, or YouTube URL.
    pub audio_url: String,
    #[serde(default)]
    pub prompt: Option<String>,
    /// describe | transcribe; default describe.
    #[serde(default)]
    pub mode: Option<String>,
    /// Optional time-window start MM:SS.
    #[serde(default)]
    pub start: Option<String>,
    /// Optional time-window end MM:SS.
    #[serde(default)]
    pub end: Option<String>,
}

/// 向注册表登记 `audio_understand` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "audio_analyze".to_string(),
        toolset: "audio_analyze".to_string(),
        description: "Analyze audio (workspace path, http(s), or YouTube). Modes: describe, transcribe. Google Interactions API; OpenAI Whisper fallback for transcribe."
            .to_string(),
        schema: schema_for_args::<AudioUnderstandArgs>(),
        check_fn: None,
        icon: "ear",
            ..ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["audio_analyze"],
    async_ctx: dispatch,
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
    let media = resolve_google_media(ctx, audio_url, is_yt).await?;
    let client = reqwest::Client::new();
    let text = google_interactions_audio(&client, prompt, &media, mode, &config).await?;
    Ok(format!(
        "{text}\nprovider=google\nmodel={model}\nmode={}",
        mode.as_str()
    ))
}

/// 解析 Google Interactions 所需的音频媒体部分。
///
/// - YouTube URL：以 `Uri` + `Video` 直传，Google 侧原生解析 YouTube 链接。
/// - 其余情况（本地路径 / 任意 http(s) 音频 URL）：均通过 [`load_audio_bytes`]
///   下载/读取字节后，以 `Inline`（base64）方式发送，绝不将任意 http(s) URL
///   作为 `uri` 字段传给 Google（避免 SSRF/无鉴权外链拉取风险）。
async fn resolve_google_media(
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
    let (bytes, mime, _filename) = load_audio_bytes(ctx, audio_url).await?;
    Ok(build_inline_audio_part(&bytes, &mime))
}

/// 纯函数：由已读取的字节与 mime 构造 Google Inline 音频媒体部分。
/// 抽出以便单测直接覆盖「http(s) 字节 → Inline」映射，无需网络。
fn build_inline_audio_part(bytes: &[u8], mime: &str) -> AudioMediaPart {
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    AudioMediaPart::Inline {
        media_type: AudioMediaKind::Audio,
        mime_type: mime.to_string(),
        data_b64: b64,
    }
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
        let (bytes, mime) = download_audio_bytes(audio_url).await?;
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

/// 下载远程音频字节，带超时与大小上限保护：
/// - 连接/读取超时 `DOWNLOAD_TIMEOUT_SECS` 秒；
/// - `Content-Length` 声明超过 `MAX_AUDIO_DOWNLOAD_BYTES` 直接拒绝（无需下载）；
/// - 服务器未声明或谎报长度时，按累计已读字节数二次校验，超限立即中止（防止无限占用内存）。
async fn download_audio_bytes(audio_url: &str) -> anyhow::Result<(Vec<u8>, String)> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(DOWNLOAD_TIMEOUT_SECS))
        .build()
        .map_err(|e| anyhow::anyhow!("创建下载客户端失败: {e}"))?;
    let mut resp = client
        .get(audio_url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("下载音频失败: {e}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("下载音频 HTTP {}", resp.status());
    }
    if let Some(len) = resp.content_length() {
        if len > MAX_AUDIO_DOWNLOAD_BYTES {
            anyhow::bail!(
                "音频文件过大（声明大小 {len} 字节），超过 {} MiB 限制",
                MAX_AUDIO_DOWNLOAD_BYTES / (1024 * 1024)
            );
        }
    }
    let mime = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .filter(|m| m.starts_with("audio/") || m.starts_with("video/"))
        .map(str::to_string)
        .unwrap_or_else(|| mime_from_url(audio_url).to_string());
    let mut bytes: Vec<u8> = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| anyhow::anyhow!("下载音频失败: {e}"))?
    {
        bytes.extend_from_slice(&chunk);
        if bytes.len() as u64 > MAX_AUDIO_DOWNLOAD_BYTES {
            anyhow::bail!(
                "音频文件过大（已下载超过 {} MiB），已中止下载",
                MAX_AUDIO_DOWNLOAD_BYTES / (1024 * 1024)
            );
        }
    }
    Ok((bytes, mime))
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
        assert_eq!(
            mime_from_path(std::path::Path::new("a.unknown")),
            "audio/mp3"
        );
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

    /// Final-review Finding 1：纯函数覆盖「已知字节+mime → Google Inline 媒体部分」映射，
    /// 无需网络即可验证 http(s) 音频最终以 base64 Inline（而非 Uri）形式发出。
    #[test]
    fn build_inline_audio_part_encodes_bytes_as_base64() {
        let part = build_inline_audio_part(b"hello-audio-bytes", "audio/mp3");
        match part {
            AudioMediaPart::Inline {
                media_type,
                mime_type,
                data_b64,
            } => {
                assert_eq!(media_type, AudioMediaKind::Audio);
                assert_eq!(mime_type, "audio/mp3");
                assert_eq!(
                    data_b64,
                    base64::engine::general_purpose::STANDARD.encode(b"hello-audio-bytes")
                );
            }
            AudioMediaPart::Uri { .. } => panic!("应始终构造 Inline"),
        }
    }
}

#[cfg(test)]
mod path_escape_tests {
    use super::*;
    use crate::context::{ImageGenTargets, ToolContext};
    use memory::MemoryManager;
    use tempfile::TempDir;

    fn build_ctx<'a>(
        memory: &'a mut MemoryManager,
        sessions: &'a session::SessionStore,
        memory_dir: std::path::PathBuf,
        workspace_dir: std::path::PathBuf,
        targets: &'a ImageGenTargets,
    ) -> ToolContext<'a> {
        ToolContext {
            memory,
            sessions,
            memory_dir,
            workspace_dir,
            project_root: None,
            image_gen_targets: targets,
            session_id: "test".into(),
            turn_id: None,
            chat_api_key: String::new(),
            chat_base_url: String::new(),
            chat_provider: String::new(),
            chat_model: String::new(),
            chat_targets: vec![],
            execution: None,
            hook_bus: None,
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
        let sessions =
            session::SessionStore::open_sessions_dir(&dir.path().join("sessions")).unwrap();
        let targets = ImageGenTargets::default();
        let ctx = build_ctx(
            &mut memory,
            &sessions,
            dir.path().to_path_buf(),
            ws,
            &targets,
        );

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
        let sessions =
            session::SessionStore::open_sessions_dir(&dir.path().join("sessions")).unwrap();
        let targets = ImageGenTargets::default();
        let ctx = build_ctx(
            &mut memory,
            &sessions,
            dir.path().to_path_buf(),
            ws,
            &targets,
        );

        let (bytes, mime, filename) = load_audio_bytes(&ctx, "ok.wav").await.unwrap();
        assert_eq!(bytes, b"ok-bytes");
        assert_eq!(mime, "audio/wav");
        assert_eq!(filename, "ok.wav");
    }

    /// Finding 1：`resolve_google_media` 同样须拒绝越界本地路径。
    #[tokio::test]
    async fn resolve_google_media_rejects_path_escape() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().join("ws");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.mp3"), b"top-secret-bytes").unwrap();

        let mut memory = MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions =
            session::SessionStore::open_sessions_dir(&dir.path().join("sessions")).unwrap();
        let targets = ImageGenTargets::default();
        let ctx = build_ctx(
            &mut memory,
            &sessions,
            dir.path().to_path_buf(),
            ws,
            &targets,
        );

        let err = resolve_google_media(&ctx, "../outside/secret.mp3", false)
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("越界") || err.to_string().contains(".."),
            "unexpected error: {err}"
        );
    }

    /// Final-review Finding 1：非 YouTube 的本地路径须解析为 `Inline`（而非 `Uri`）。
    /// 通过本地文件路径覆盖 `resolve_google_media` 的非 YouTube 分支，
    /// 避免真实网络请求；http(s) → Inline 的字节映射由 `build_inline_audio_part` 覆盖。
    #[tokio::test]
    async fn resolve_google_media_non_youtube_resolves_to_inline() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("clip.wav"), b"pcm-bytes").unwrap();

        let mut memory = MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions =
            session::SessionStore::open_sessions_dir(&dir.path().join("sessions")).unwrap();
        let targets = ImageGenTargets::default();
        let ctx = build_ctx(
            &mut memory,
            &sessions,
            dir.path().to_path_buf(),
            ws,
            &targets,
        );

        let media = resolve_google_media(&ctx, "clip.wav", false).await.unwrap();
        match media {
            AudioMediaPart::Inline {
                media_type,
                mime_type,
                data_b64,
            } => {
                assert_eq!(media_type, AudioMediaKind::Audio);
                assert_eq!(mime_type, "audio/wav");
                assert_eq!(
                    data_b64,
                    base64::engine::general_purpose::STANDARD.encode(b"pcm-bytes")
                );
            }
            AudioMediaPart::Uri { .. } => {
                panic!("非 YouTube 音频不应以 Uri 形式传给 Google，须下载为 Inline");
            }
        }
    }

    /// Final-review Finding 1：YouTube 分支仍须保持 `Uri` + `Video`。
    #[tokio::test]
    async fn resolve_google_media_youtube_resolves_to_uri_video() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();

        let mut memory = MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions =
            session::SessionStore::open_sessions_dir(&dir.path().join("sessions")).unwrap();
        let targets = ImageGenTargets::default();
        let ctx = build_ctx(
            &mut memory,
            &sessions,
            dir.path().to_path_buf(),
            ws,
            &targets,
        );

        let media = resolve_google_media(&ctx, "https://youtu.be/abc", true)
            .await
            .unwrap();
        match media {
            AudioMediaPart::Uri {
                media_type, uri, ..
            } => {
                assert_eq!(media_type, AudioMediaKind::Video);
                assert_eq!(uri, "https://youtu.be/abc");
            }
            AudioMediaPart::Inline { .. } => panic!("YouTube 应保持 Uri+Video"),
        }
    }
}

/// Final-review Finding 2：远程音频下载的大小上限校验。
/// 用最小化的原始 TCP/HTTP 服务器模拟远端响应，避免引入额外 mock 依赖。
#[cfg(test)]
mod download_limit_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// 启动一个仅处理一次连接的极简 HTTP 服务器：丢弃请求，写回状态行/指定 headers/指定长度的正文后关闭连接。
    /// 返回可直接请求的 URL 与后台线程 handle（测试内 join 以确保写完成）。
    fn spawn_raw_http_server(
        extra_headers: &str,
        body_len: usize,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local_addr");
        let extra_headers = extra_headers.to_string();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let mut head = String::new();
                head.push_str("HTTP/1.1 200 OK\r\n");
                head.push_str("Content-Type: audio/mpeg\r\n");
                head.push_str("Connection: close\r\n");
                head.push_str(&extra_headers);
                head.push_str("\r\n");
                let _ = stream.write_all(head.as_bytes());
                let chunk = vec![b'a'; 64 * 1024];
                let mut written = 0usize;
                while written < body_len {
                    let n = std::cmp::min(chunk.len(), body_len - written);
                    if stream.write_all(&chunk[..n]).is_err() {
                        break;
                    }
                    written += n;
                }
                let _ = stream.flush();
                let _ = stream.shutdown(std::net::Shutdown::Write);
            }
        });
        (format!("http://{addr}/audio.mp3"), handle)
    }

    /// 声明的 `Content-Length` 超限时须立即拒绝，无需读取正文。
    #[tokio::test]
    async fn rejects_declared_content_length_over_limit() {
        let over_limit = MAX_AUDIO_DOWNLOAD_BYTES + 1;
        let (url, handle) = spawn_raw_http_server(&format!("Content-Length: {over_limit}\r\n"), 0);

        let err = download_audio_bytes(&url).await.unwrap_err();
        assert!(
            err.to_string().contains("过大") || err.to_string().contains("MiB"),
            "unexpected error: {err}"
        );
        handle.join().ok();
    }

    /// 未声明（或谎报较小）`Content-Length` 时，仍须在累计读取超限的瞬间中止，防止无限占用内存。
    #[tokio::test]
    async fn aborts_when_streamed_bytes_exceed_limit_without_content_length() {
        let over_limit_len = (MAX_AUDIO_DOWNLOAD_BYTES + 64 * 1024) as usize;
        let (url, handle) = spawn_raw_http_server("", over_limit_len);

        let err = download_audio_bytes(&url).await.unwrap_err();
        assert!(
            err.to_string().contains("过大") || err.to_string().contains("MiB"),
            "unexpected error: {err}"
        );
        handle.join().ok();
    }

    /// 正常大小的下载应成功返回字节与 mime。
    #[tokio::test]
    async fn downloads_small_body_successfully() {
        let (url, handle) = spawn_raw_http_server("", 128);

        let (bytes, mime) = download_audio_bytes(&url).await.unwrap();
        assert_eq!(bytes.len(), 128);
        assert_eq!(mime, "audio/mpeg");
        handle.join().ok();
    }
}
