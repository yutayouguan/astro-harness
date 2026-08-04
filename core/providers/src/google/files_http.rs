//! Google Files API（resumable upload + ACTIVE 轮询）。
//! 与 OpenAI 无关；鉴权用 `x-goog-api-key`。

use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use tokio::time::sleep;

use super::veo_http::google_native_base;
use crate::types::request::ProviderConfig;

/// Gemini 官方建议：请求总大小超过约 20MB 时应改走 Files API，而非内嵌 base64。
pub const INLINE_MAX_BYTES: u64 = 20 * 1024 * 1024;
pub const FILES_POLL_INTERVAL: Duration = Duration::from_secs(5);
pub const FILES_POLL_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, Clone)]
pub struct UploadedFile {
    pub name: String,
    pub uri: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileState {
    Processing,
    Active {
        name: String,
        uri: String,
        mime_type: String,
    },
    Failed,
}

fn trim_slash(s: &str) -> String {
    s.trim_end_matches('/').to_string()
}

fn native_root(config: &ProviderConfig) -> String {
    trim_slash(&google_native_base(config))
}

fn v1beta_root(config: &ProviderConfig) -> String {
    let n = native_root(config);
    if n.contains("/v1beta") {
        n
    } else {
        format!("{n}/v1beta")
    }
}

/// 从 start 响应头提取可恢复上传 URL。
pub fn upload_url_from_start_headers(headers: &reqwest::header::HeaderMap) -> Result<String> {
    headers
        .get("x-goog-upload-url")
        .or_else(|| headers.get("X-Goog-Upload-URL"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow!("Files API start 响应缺少 x-goog-upload-url"))
}

fn file_object(v: &Value) -> &Value {
    v.get("file").unwrap_or(v)
}

/// 解析 Files get / upload 完成响应当中的 state。
pub fn parse_file_status(v: &Value) -> Result<FileState> {
    let f = file_object(v);
    let state = f
        .get("state")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_ascii_uppercase();
    match state.as_str() {
        "ACTIVE" => {
            let name = f
                .get("name")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let uri = f
                .get("uri")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let mime_type = f
                .get("mimeType")
                .or_else(|| f.get("mime_type"))
                .and_then(|x| x.as_str())
                .unwrap_or("video/mp4")
                .to_string();
            if name.is_empty() || uri.is_empty() {
                anyhow::bail!("ACTIVE 文件缺少 name/uri");
            }
            Ok(FileState::Active {
                name,
                uri,
                mime_type,
            })
        }
        "FAILED" => Ok(FileState::Failed),
        _ => Ok(FileState::Processing),
    }
}

/// resumable 上传并等到 ACTIVE。
pub async fn google_files_upload_and_wait(
    client: &Client,
    config: &ProviderConfig,
    bytes: &[u8],
    mime_type: &str,
    display_name: &str,
) -> Result<UploadedFile> {
    if config.api_key.trim().is_empty() {
        anyhow::bail!("Google API Key 为空");
    }
    let key = config.api_key.trim();
    let root = native_root(config);
    let start_url = if root.contains("/upload/") {
        format!("{root}/v1beta/files")
    } else {
        // https://generativelanguage.googleapis.com/upload/v1beta/files
        let host = root.trim_end_matches("/v1beta").to_string();
        format!("{host}/upload/v1beta/files")
    };

    let start = client
        .post(&start_url)
        .header("x-goog-api-key", key)
        .header("X-Goog-Upload-Protocol", "resumable")
        .header("X-Goog-Upload-Command", "start")
        .header(
            "X-Goog-Upload-Header-Content-Length",
            bytes.len().to_string(),
        )
        .header("X-Goog-Upload-Header-Content-Type", mime_type)
        .header("content-type", "application/json")
        .json(&json!({ "file": { "display_name": display_name } }))
        .send()
        .await
        .with_context(|| format!("Files API start 失败: {start_url}"))?;
    if !start.status().is_success() {
        let status = start.status();
        let body = start.text().await.unwrap_or_default();
        anyhow::bail!("Files API start HTTP {status}: {body}");
    }
    let upload_url = upload_url_from_start_headers(start.headers())?;

    let uploaded = client
        .post(&upload_url)
        .header("Content-Length", bytes.len().to_string())
        .header("X-Goog-Upload-Offset", "0")
        .header("X-Goog-Upload-Command", "upload, finalize")
        .body(bytes.to_vec())
        .send()
        .await
        .context("Files API upload finalize 失败")?;
    let status = uploaded.status();
    let v: Value = uploaded
        .json()
        .await
        .context("解析 Files upload 响应失败")?;
    if !status.is_success() {
        anyhow::bail!(
            "Files API upload HTTP {status}: {}",
            v.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("upload failed")
        );
    }

    // 上传响应可能已是 ACTIVE
    match parse_file_status(&v)? {
        FileState::Active {
            name,
            uri,
            mime_type,
        } => {
            return Ok(UploadedFile {
                name,
                uri,
                mime_type,
            });
        }
        FileState::Failed => anyhow::bail!("Files API 处理失败"),
        FileState::Processing => {}
    }

    let name = file_object(&v)
        .get("name")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow!("upload 响应无 file.name"))?
        .to_string();

    let get_url = format!("{}/{}", v1beta_root(config), name);
    let deadline = Instant::now() + FILES_POLL_TIMEOUT;
    loop {
        if Instant::now() > deadline {
            anyhow::bail!("Files API 轮询超时（等待 ACTIVE）: {name}");
        }
        sleep(FILES_POLL_INTERVAL).await;
        let resp = client
            .get(&get_url)
            .header("x-goog-api-key", key)
            .send()
            .await
            .with_context(|| format!("Files API get 失败: {get_url}"))?;
        let st = resp.status();
        let body: Value = resp.json().await.context("解析 Files get 响应失败")?;
        if !st.is_success() {
            anyhow::bail!("Files API get HTTP {st}");
        }
        match parse_file_status(&body)? {
            FileState::Active {
                name,
                uri,
                mime_type,
            } => {
                return Ok(UploadedFile {
                    name,
                    uri,
                    mime_type,
                });
            }
            FileState::Failed => anyhow::bail!("Files API 处理失败: {name}"),
            FileState::Processing => continue,
        }
    }
}

/// 删除已上传文件（best-effort）。
pub async fn google_files_delete(
    client: &Client,
    config: &ProviderConfig,
    file_name: &str,
) -> Result<()> {
    if config.api_key.trim().is_empty() || file_name.trim().is_empty() {
        anyhow::bail!("delete 参数无效");
    }
    let url = format!("{}/{}", v1beta_root(config), file_name.trim());
    let resp = client
        .delete(&url)
        .header("x-goog-api-key", config.api_key.trim())
        .send()
        .await
        .with_context(|| format!("Files API delete 失败: {url}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Files API delete HTTP {status}: {text}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn inline_max_is_20mb() {
        assert_eq!(INLINE_MAX_BYTES, 20 * 1024 * 1024);
    }

    #[test]
    fn parse_active_state() {
        let v = json!({
            "state": "ACTIVE",
            "uri": "https://generativelanguage.googleapis.com/v1beta/files/abc",
            "name": "files/abc",
            "mimeType": "video/mp4"
        });
        match parse_file_status(&v).unwrap() {
            FileState::Active {
                uri,
                name,
                mime_type,
            } => {
                assert!(uri.contains("files/abc"));
                assert_eq!(name, "files/abc");
                assert_eq!(mime_type, "video/mp4");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_failed_and_processing() {
        assert!(matches!(
            parse_file_status(&json!({ "state": "FAILED" })).unwrap(),
            FileState::Failed
        ));
        assert!(matches!(
            parse_file_status(&json!({ "state": "PROCESSING" })).unwrap(),
            FileState::Processing
        ));
        // nested under "file"
        let v = json!({ "file": { "state": "ACTIVE", "name": "files/x", "uri": "u", "mime_type": "video/webm" } });
        match parse_file_status(&v).unwrap() {
            FileState::Active { mime_type, .. } => assert_eq!(mime_type, "video/webm"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn upload_url_from_headers() {
        let mut map = reqwest::header::HeaderMap::new();
        map.insert(
            "x-goog-upload-url",
            "https://example.com/upload?foo=1".parse().unwrap(),
        );
        assert_eq!(
            upload_url_from_start_headers(&map).unwrap(),
            "https://example.com/upload?foo=1"
        );
    }
}
