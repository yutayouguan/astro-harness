//! 视频理解：Google Interactions 原生（Files / inline / YouTube）。
//! 不走 OpenAI。

use base64::Engine;
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

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

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
    let client = reqwest::Client::new();

    let (video_part, input_kind, uploaded_name) =
        resolve_video_input(&client, &config, ctx, video_url).await?;

    let text = match google_interactions_video(
        &client, &config, &model, &prompt, &video_part, mode,
    )
    .await
    {
        Ok(t) => t,
        Err(e) => {
            if let Some(name) = uploaded_name.as_deref() {
                let _ = google_files_delete(&client, &config, name).await;
            }
            return Err(e);
        }
    };

    if let Some(name) = uploaded_name.as_deref() {
        let _ = google_files_delete(&client, &config, name).await;
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
            let path = ctx.workspace_dir.join(video_url);
            if !path.exists() {
                anyhow::bail!("本地文件不存在: {}", path.display());
            }
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

async fn download_bytes(client: &reqwest::Client, url: &str) -> anyhow::Result<Vec<u8>> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("下载视频失败: {e}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("下载视频 HTTP {}", resp.status());
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| anyhow::anyhow!("读取视频字节失败: {e}"))?;
    Ok(bytes.to_vec())
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
}
