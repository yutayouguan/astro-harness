//! 一等媒体资产：对齐 Agno `Image` / `Audio` / `Video` / `File` 的统一引用模型。
//!
//! 设计约束：
//! - 输入/工具结果/输出共用同一类型，不绑定具体 Provider
//! - 默认引用工作区相对路径（生成类工具落盘后的主形态）
//! - 工具结果仍可带人类可读文本；结构化媒体走 [`MediaAsset`] 或 `astro_media_v1` sidecar

use serde::{Deserialize, Serialize};

/// 媒体种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Image,
    Audio,
    Video,
    File,
}

/// 媒体内容来源（三者择一语义；序列化时按字段存在与否表达）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaRef {
    /// 工作区相对路径，如 `generated/images/img-….png`。
    WorkspacePath(String),
    /// `data:image/png;base64,…` 或同类 data URL。
    DataUrl(String),
    /// 远程 http(s) / gs:// 等 URI。
    RemoteUri(String),
}

/// 贯穿输入 → 消息 → 工具结果 → UI 的统一媒体对象。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaAsset {
    pub kind: MediaKind,
    /// MIME，如 `image/png`；未知时可为空。
    #[serde(default)]
    pub mime_type: String,
    pub reference: MediaRef,
    /// 展示用短标签（如「图片已生成」）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// 可选追踪 id。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

impl MediaAsset {
    pub fn workspace(
        kind: MediaKind,
        path: impl Into<String>,
        mime_type: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            mime_type: mime_type.into(),
            reference: MediaRef::WorkspacePath(path.into()),
            label: None,
            id: None,
        }
    }

    pub fn data_url(kind: MediaKind, url: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self {
            kind,
            mime_type: mime_type.into(),
            reference: MediaRef::DataUrl(url.into()),
            label: None,
            id: None,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// 工作区相对路径（若有）。
    pub fn workspace_path(&self) -> Option<&str> {
        match &self.reference {
            MediaRef::WorkspacePath(p) => Some(p.as_str()),
            _ => None,
        }
    }
}

const SIDECAR_PREFIX: &str = "astro_media_v1:";

/// 在工具结果文本末尾追加机器可读 sidecar（人类可读行保持不变）。
pub fn append_media_sidecar(text: &str, media: &[MediaAsset]) -> String {
    if media.is_empty() {
        return text.to_string();
    }
    let json = serde_json::to_string(media).unwrap_or_else(|_| "[]".into());
    if text.is_empty() {
        format!("{SIDECAR_PREFIX}{json}")
    } else {
        format!("{text}\n{SIDECAR_PREFIX}{json}")
    }
}

/// 解析并剥离 sidecar；若无 sidecar 则回落解析「图片/视频/语音/音乐已生成」文案。
pub fn extract_tool_media(text: &str) -> (String, Vec<MediaAsset>) {
    let (without, from_sidecar) = strip_media_sidecar(text);
    if !from_sidecar.is_empty() {
        return (without, from_sidecar);
    }
    (without.clone(), parse_generated_labels(&without))
}

fn strip_media_sidecar(text: &str) -> (String, Vec<MediaAsset>) {
    let mut media = Vec::new();
    let mut kept = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(json) = trimmed.strip_prefix(SIDECAR_PREFIX) {
            if let Ok(parsed) = serde_json::from_str::<Vec<MediaAsset>>(json) {
                media.extend(parsed);
                continue;
            }
        }
        kept.push(line);
    }
    (kept.join("\n"), media)
}

/// 从生成类工具固定文案解析媒体路径。
pub fn parse_generated_labels(text: &str) -> Vec<MediaAsset> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in text.lines() {
        let (kind, label) = if line.contains("图片已生成") {
            (MediaKind::Image, "图片已生成")
        } else if line.contains("视频已生成") {
            (MediaKind::Video, "视频已生成")
        } else if line.contains("语音已生成") {
            (MediaKind::Audio, "语音已生成")
        } else if line.contains("音乐已生成") {
            (MediaKind::Audio, "音乐已生成")
        } else {
            continue;
        };
        if let Some(path) = extract_labeled_path(line) {
            if seen.insert(path.clone()) {
                let mime = guess_mime(kind, &path);
                out.push(MediaAsset::workspace(kind, path, mime).with_label(label));
            }
        }
    }
    out
}

fn extract_labeled_path(line: &str) -> Option<String> {
    for marker in ["已生成：", "已生成:"] {
        if let Some(idx) = line.find(marker) {
            let rest = line[idx + marker.len()..].trim();
            let path = rest
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_matches(|c| c == '"' || c == '\'');
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
    }
    None
}

fn guess_mime(kind: MediaKind, path: &str) -> String {
    let ext = path
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match (kind, ext.as_str()) {
        (MediaKind::Image, "jpg" | "jpeg") => "image/jpeg".into(),
        (MediaKind::Image, "webp") => "image/webp".into(),
        (MediaKind::Image, "gif") => "image/gif".into(),
        (MediaKind::Image, _) => "image/png".into(),
        (MediaKind::Video, "webm") => "video/webm".into(),
        (MediaKind::Video, _) => "video/mp4".into(),
        (MediaKind::Audio, "mp3") => "audio/mpeg".into(),
        (MediaKind::Audio, "m4a") => "audio/mp4".into(),
        (MediaKind::Audio, _) => "audio/wav".into(),
        (MediaKind::File, _) => "application/octet-stream".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_roundtrip() {
        let media =
            vec![
                MediaAsset::workspace(MediaKind::Image, "generated/images/a.png", "image/png")
                    .with_label("图片已生成"),
            ];
        let text = append_media_sidecar(
            "图片已生成：generated/images/a.png\nprovider=google",
            &media,
        );
        let (plain, parsed) = extract_tool_media(&text);
        assert!(plain.contains("图片已生成：generated/images/a.png"));
        assert!(!plain.contains(SIDECAR_PREFIX));
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].workspace_path(), Some("generated/images/a.png"));
    }

    #[test]
    fn parses_labels_without_sidecar() {
        let text = "语音已生成：generated/audio/tts-1.wav\nprovider=openai";
        let items = parse_generated_labels(text);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, MediaKind::Audio);
    }
}
