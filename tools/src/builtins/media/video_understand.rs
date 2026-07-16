//! 视频理解：Google Interactions 原生（Files / inline / YouTube）。
//! 不走 OpenAI。

use base64::Engine;
use futures::StreamExt;
use providers::files_http::{
    google_files_delete, google_files_upload_and_wait, INLINE_MAX_BYTES,
};
use providers::interactions_http::{
    default_video_understand_prompt, google_interactions_video, try_parse_timeline_events,
    VideoInputPart, VideoUnderstandMode,
};
use providers::media_http::default_vision_model;
use providers::trait_::ProviderConfig;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use std::path::PathBuf;
use std::time::Duration;

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// 远程 http(s) 视频下载的硬上限：略高于 `INLINE_MAX_BYTES`，避免恶意/超大远程文件把整段响应体读进内存。
/// 超限时直接报错，不做 Files API 兜底（保持本次修复范围可控）。
const MAX_REMOTE_DOWNLOAD_BYTES: u64 = INLINE_MAX_BYTES + 5 * 1024 * 1024;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Files API（start/upload/delete）与轮询 GET 共用：单次请求超时上限，避免轮询在网络异常时无限期挂起。
const FILES_REQUEST_TIMEOUT: Duration = Duration::from_secs(180);
/// Interactions 视频理解请求：视频体量大、可能耗时较久，但同样需要上限避免无限期挂起。
const INTERACTIONS_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

fn build_files_client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(FILES_REQUEST_TIMEOUT)
        .build()?)
}

fn build_interactions_client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(INTERACTIONS_REQUEST_TIMEOUT)
        .build()?)
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct VideoUnderstandArgs {
    /// 工作区相对路径、http(s) 直链，或公开 YouTube URL。
    pub video_url: String,
    #[serde(default)]
    pub prompt: Option<String>,
    /// `qa` | `summarize` | `timeline`；默认 `qa`。
    #[serde(default)]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VideoSourceKind {
    Youtube,
    RemoteHttp,
    LocalPath,
}

fn classify_video_source(url: &str) -> VideoSourceKind {
    let lower = url.trim().to_ascii_lowercase();
    if lower.contains("youtube.com/") || lower.contains("youtu.be/") {
        return VideoSourceKind::Youtube;
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return VideoSourceKind::RemoteHttp;
    }
    VideoSourceKind::LocalPath
}

fn should_use_files_api(len: u64) -> bool {
    len >= INLINE_MAX_BYTES
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

fn mime_from_path(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "mp4" => "video/mp4",
        "mpeg" | "mpg" => "video/mpeg",
        "mov" => "video/mov",
        "avi" => "video/avi",
        "flv" => "video/x-flv",
        "webm" => "video/webm",
        "wmv" => "video/wmv",
        "3gp" | "3gpp" => "video/3gpp",
        _ => "video/mp4",
    }
}

/// 向注册表登记 `video_understand` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "video_understand".into(),
        toolset: "video_understand".into(),
        description: "Analyze a video with Google Gemini Interactions (workspace path, http(s), or public YouTube). Modes: qa, summarize, timeline (JSON events). Supports MM:SS timestamps in the prompt. Google only.".into(),
        schema: schema_for_args::<VideoUnderstandArgs>(),
        check_fn: None,
        icon: "film",
    });
}

/// 调用 Google Interactions 视频理解并返回文本（timeline 模式附带 JSON）。
pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: VideoUnderstandArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("video_understand 参数无效: {e}"))?;
    let video_url = parsed.video_url.trim();
    if video_url.is_empty() {
        anyhow::bail!("video_understand 需要 video_url");
    }
    let mode: VideoUnderstandMode = parsed
        .mode
        .as_deref()
        .unwrap_or("qa")
        .parse()?;
    let prompt = parsed
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_video_understand_prompt(mode))
        .to_string();

    let Some(creds) = ctx.image_gen_targets.google() else {
        anyhow::bail!("video_understand 需要 Google API Key（不支持 OpenAI）");
    };
    let model = if creds.vision_model.trim().is_empty() {
        default_vision_model("google").to_string()
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
    let files_client = build_files_client()?;
    let interactions_client = build_interactions_client()?;

    let (video_part, input_kind, uploaded_name) =
        resolve_video_input(&files_client, &config, ctx, video_url).await?;

    let text = match google_interactions_video(
        &interactions_client, &config, &model, &prompt, &video_part, mode,
    )
    .await
    {
        Ok(t) => t,
        Err(e) => {
            if let Some(name) = uploaded_name.as_deref() {
                let _ = google_files_delete(&files_client, &config, name).await;
            }
            return Err(e);
        }
    };

    if let Some(name) = uploaded_name.as_deref() {
        let _ = google_files_delete(&files_client, &config, name).await;
    }

    let body = if mode == VideoUnderstandMode::Timeline {
        match try_parse_timeline_events(&text) {
            Ok(v) => format!(
                "{}\n\nprovider=google\nmodel={model}\nmode=timeline\ninput={input_kind}",
                serde_json::to_string_pretty(&v).unwrap_or(text)
            ),
            Err(_) => format!(
                "{text}\n\nprovider=google\nmodel={model}\nmode=timeline\ninput={input_kind}\nparse_error=true"
            ),
        }
    } else {
        format!(
            "{text}\n\nprovider=google\nmodel={model}\nmode={}\ninput={input_kind}\nhint: 引用时间点请用 MM:SS（如 01:15）",
            mode.as_str()
        )
    };
    Ok(body)
}

async fn resolve_video_input(
    client: &reqwest::Client,
    config: &ProviderConfig,
    ctx: &ToolContext<'_>,
    video_url: &str,
) -> anyhow::Result<(VideoInputPart, &'static str, Option<String>)> {
    match classify_video_source(video_url) {
        VideoSourceKind::Youtube => Ok((
            VideoInputPart::Uri {
                mime_type: None,
                uri: video_url.to_string(),
            },
            "youtube",
            None,
        )),
        VideoSourceKind::RemoteHttp => {
            let bytes = download_bytes(client, video_url).await?;
            bytes_to_part(client, config, &bytes, mime_from_url(video_url), "remote").await
        }
        VideoSourceKind::LocalPath => {
            let path = resolve_workspace_file(ctx, video_url)?;
            let bytes = std::fs::read(&path)
                .map_err(|e| anyhow::anyhow!("读取视频失败 {}: {e}", path.display()))?;
            let mime = mime_from_path(&path);
            let display = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("video");
            bytes_to_part(client, config, &bytes, mime, display).await
        }
    }
}

async fn bytes_to_part(
    client: &reqwest::Client,
    config: &ProviderConfig,
    bytes: &[u8],
    mime: &str,
    display_name: &str,
) -> anyhow::Result<(VideoInputPart, &'static str, Option<String>)> {
    if should_use_files_api(bytes.len() as u64) {
        let uploaded =
            google_files_upload_and_wait(client, config, bytes, mime, display_name).await?;
        Ok((
            VideoInputPart::Uri {
                mime_type: Some(uploaded.mime_type),
                uri: uploaded.uri,
            },
            "file_api",
            Some(uploaded.name),
        ))
    } else {
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        Ok((
            VideoInputPart::Inline {
                mime_type: mime.to_string(),
                data_b64: b64,
            },
            "inline",
            None,
        ))
    }
}

fn remote_too_large_err(len: u64) -> anyhow::Error {
    anyhow::anyhow!(
        "远程视频过大（约 {:.1}MB，上限 {:.0}MB）：video_understand 暂不支持下载超大远程 http(s) 视频；\
请改用工作区本地路径（会按体量自动走 inline 或 Files API）、公开 YouTube 链接，或先压缩/裁剪后重试",
        len as f64 / (1024.0 * 1024.0),
        MAX_REMOTE_DOWNLOAD_BYTES as f64 / (1024.0 * 1024.0)
    )
}

async fn download_bytes(client: &reqwest::Client, url: &str) -> anyhow::Result<Vec<u8>> {
    use anyhow::Context;
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("下载视频失败: {url}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("下载视频 HTTP {}", resp.status());
    }
    if let Some(len) = resp.content_length() {
        if len > MAX_REMOTE_DOWNLOAD_BYTES {
            return Err(remote_too_large_err(len));
        }
    }

    // 即便 Content-Length 缺失或不可信，也按上限截断式读取，避免把整段响应体无限制读进内存。
    let mut buf: Vec<u8> = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| anyhow::anyhow!("读取视频字节失败: {e}"))?;
        if buf.len() as u64 + chunk.len() as u64 > MAX_REMOTE_DOWNLOAD_BYTES {
            return Err(remote_too_large_err(buf.len() as u64 + chunk.len() as u64));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

fn mime_from_url(url: &str) -> &'static str {
    let path = url.split('?').next().unwrap_or(url);
    mime_from_path(std::path::Path::new(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_youtube() {
        assert!(matches!(
            classify_video_source("https://www.youtube.com/watch?v=9hE5-98ZeCg"),
            VideoSourceKind::Youtube
        ));
        assert!(matches!(
            classify_video_source("https://youtu.be/9hE5-98ZeCg"),
            VideoSourceKind::Youtube
        ));
    }

    #[test]
    fn classify_http_and_local() {
        assert!(matches!(
            classify_video_source("https://cdn.example.com/a.mp4"),
            VideoSourceKind::RemoteHttp
        ));
        assert!(matches!(
            classify_video_source("generated/videos/x.mp4"),
            VideoSourceKind::LocalPath
        ));
    }

    #[test]
    fn mime_mp4() {
        assert_eq!(
            mime_from_path(std::path::Path::new("a.MP4")),
            "video/mp4"
        );
        assert_eq!(
            mime_from_path(std::path::Path::new("a.webm")),
            "video/webm"
        );
    }

    #[test]
    fn chooses_inline_under_threshold() {
        assert!(!should_use_files_api(INLINE_MAX_BYTES - 1));
        assert!(should_use_files_api(INLINE_MAX_BYTES));
        assert!(should_use_files_api(INLINE_MAX_BYTES + 1));
    }

    #[test]
    fn remote_download_cap_is_slightly_above_inline_max() {
        assert!(MAX_REMOTE_DOWNLOAD_BYTES > INLINE_MAX_BYTES);
    }

    /// 读掉客户端请求行/头，避免关闭连接时因内核接收缓冲区里还有未读数据而触发 RST（而非正常 FIN），
    /// 导致客户端把这当作连接错误而不是我们期望的「响应体过大被拒绝」。
    fn drain_request(stream: &mut std::net::TcpStream) {
        use std::io::Read;
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf);
    }

    #[tokio::test]
    async fn download_bytes_rejects_oversized_content_length() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let big_len = MAX_REMOTE_DOWNLOAD_BYTES + 1;
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                drain_request(&mut stream);
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {big_len}\r\nContent-Type: video/mp4\r\nConnection: close\r\n\r\n"
                );
                // 只写响应头，故意不写 body：验证仅凭 Content-Length 就能提前拒绝，无需等待/读取超大 body。
                let _ = stream.write_all(header.as_bytes());
            }
        });

        let client = reqwest::Client::new();
        let url = format!("http://{addr}/video.mp4");
        let err = download_bytes(&client, &url).await.unwrap_err();
        assert!(err.to_string().contains("远程视频过大"), "unexpected: {err}");
    }

    #[tokio::test]
    async fn download_bytes_rejects_oversized_stream_without_content_length() {
        use std::io::Write;
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                drain_request(&mut stream);
                // 无 Content-Length，用 chunked 传输持续发送超过上限的字节，验证流式读取会中途截断拒绝。
                let header =
                    "HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nTransfer-Encoding: chunked\r\n\r\n";
                if stream.write_all(header.as_bytes()).is_err() {
                    return;
                }
                let chunk = vec![0u8; 1024 * 1024];
                let chunk_header = format!("{:x}\r\n", chunk.len());
                let mut sent = 0u64;
                let target = MAX_REMOTE_DOWNLOAD_BYTES + 2 * 1024 * 1024;
                while sent < target {
                    if stream.write_all(chunk_header.as_bytes()).is_err()
                        || stream.write_all(&chunk).is_err()
                        || stream.write_all(b"\r\n").is_err()
                    {
                        return;
                    }
                    sent += chunk.len() as u64;
                }
                let _ = stream.write_all(b"0\r\n\r\n");
            }
        });

        let client = reqwest::Client::new();
        let url = format!("http://{addr}/video.mp4");
        let err = download_bytes(&client, &url).await.unwrap_err();
        assert!(err.to_string().contains("远程视频过大"), "unexpected: {err}");
    }
}

#[cfg(test)]
mod path_tests {
    use super::*;
    use crate::context::{ImageGenTargets, ToolContext};
    use memory::MemoryManager;
    use providers::registry::ProviderRegistry;
    use tempfile::TempDir;

    #[test]
    fn resolve_workspace_file_rejects_escape() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().join("ws");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.mp4"), b"x").unwrap();

        let mut memory = MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions =
            session::SessionStore::open_sessions_dir(&dir.path().join("sessions")).unwrap();
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
            delegate_runner: None,
            async_spawner: None,
            orchestration_spawner: None,
            hook_bus: None,
        };

        let err = resolve_workspace_file(&ctx, "../outside/secret.mp4").unwrap_err();
        assert!(
            err.to_string().contains("工作区内"),
            "unexpected: {err}"
        );
    }
}
