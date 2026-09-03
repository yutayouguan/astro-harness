//! 工具执行结果的结构化返回类型，替代原先的纯 `String`。

use crate::media::MediaAsset;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolFileChangeKind {
    Add,
    Update,
    Delete,
    Move,
}

/// Exact before/after state produced by one structured file mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolFileChange {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub move_path: Option<String>,
    pub kind: ToolFileChangeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_content: Option<String>,
    pub additions: usize,
    pub deletions: usize,
    pub reversible: bool,
}

#[derive(Debug, Clone)]
pub enum ToolOutput {
    /// 纯文本结果（绝大多数工具）。
    Text(String),
    /// 带媒体资产的结果（image_gen, music_gen, speech_gen, video_gen 等）。
    Media {
        text: String,
        assets: Vec<MediaAsset>,
    },
    /// Text result plus exact file mutations for turn-scoped review and undo.
    FileChanges {
        text: String,
        changes: Vec<ToolFileChange>,
    },
}

impl ToolOutput {
    /// 面向模型的文本视图。
    pub fn text(&self) -> &str {
        match self {
            Self::Text(s) => s,
            Self::Media { text, .. } => text,
            Self::FileChanges { text, .. } => text,
        }
    }

    pub fn into_text(self) -> String {
        match self {
            Self::Text(s) => s,
            Self::Media { text, .. } => text,
            Self::FileChanges { text, .. } => text,
        }
    }

    pub fn media(&self) -> &[MediaAsset] {
        match self {
            Self::Text(_) => &[],
            Self::Media { assets, .. } => assets,
            Self::FileChanges { .. } => &[],
        }
    }

    pub fn file_changes(&self) -> &[ToolFileChange] {
        match self {
            Self::FileChanges { changes, .. } => changes,
            _ => &[],
        }
    }

    pub fn into_parts(self) -> (String, Vec<MediaAsset>) {
        match self {
            Self::Text(s) => (s, Vec::new()),
            Self::Media { text, assets } => (text, assets),
            Self::FileChanges { text, .. } => (text, Vec::new()),
        }
    }

    /// 字节长度估算（供 spill 阈值判断）。
    pub fn estimated_len(&self) -> usize {
        match self {
            Self::Text(s) => s.len(),
            Self::FileChanges { text, changes } => {
                text.len()
                    + changes
                        .iter()
                        .map(|change| {
                            change.path.len()
                                + change.move_path.as_ref().map_or(0, String::len)
                                + change.before_content.as_ref().map_or(0, String::len)
                                + change.after_content.as_ref().map_or(0, String::len)
                        })
                        .sum::<usize>()
            }
            Self::Media { text, assets } => {
                text.len()
                    + assets
                        .iter()
                        .map(|a| {
                            let ref_len = match &a.reference {
                                crate::media::MediaRef::WorkspacePath(p) => p.len(),
                                crate::media::MediaRef::DataUrl(u) => u.len(),
                                crate::media::MediaRef::RemoteUri(u) => u.len(),
                            };
                            a.mime_type.len()
                                + ref_len
                                + a.label.as_ref().map_or(0, |l| l.len())
                                + 64 // JSON 结构开销
                        })
                        .sum::<usize>()
            }
        }
    }
}

impl From<String> for ToolOutput {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<&str> for ToolOutput {
    fn from(s: &str) -> Self {
        Self::Text(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{MediaKind, MediaRef};

    #[test]
    fn text_variant_basics() {
        let out = ToolOutput::from("hello");
        assert_eq!(out.text(), "hello");
        assert!(out.media().is_empty());
        assert_eq!(out.estimated_len(), 5);
    }

    #[test]
    fn media_variant_basics() {
        let asset = MediaAsset {
            kind: MediaKind::Image,
            mime_type: "image/png".into(),
            reference: MediaRef::WorkspacePath("img.png".into()),
            label: Some("图片已生成".into()),
            id: None,
        };
        let out = ToolOutput::Media {
            text: "done".into(),
            assets: vec![asset],
        };
        assert_eq!(out.text(), "done");
        assert_eq!(out.media().len(), 1);
        assert!(out.estimated_len() > 4);
    }

    #[test]
    fn file_changes_preserve_text_and_snapshots() {
        let change = ToolFileChange {
            path: "a.txt".into(),
            move_path: None,
            kind: ToolFileChangeKind::Update,
            before_content: Some("old\n".into()),
            after_content: Some("new\n".into()),
            additions: 1,
            deletions: 1,
            reversible: true,
        };
        let out = ToolOutput::FileChanges {
            text: "done".into(),
            changes: vec![change.clone()],
        };
        assert_eq!(out.text(), "done");
        assert_eq!(out.file_changes(), &[change]);
        assert!(out.estimated_len() > 4);
    }

    #[test]
    fn into_parts_text() {
        let (text, media) = ToolOutput::from("hi").into_parts();
        assert_eq!(text, "hi");
        assert!(media.is_empty());
    }

    #[test]
    fn into_parts_media() {
        let asset = MediaAsset {
            kind: MediaKind::Audio,
            mime_type: "audio/mp3".into(),
            reference: MediaRef::WorkspacePath("a.mp3".into()),
            label: None,
            id: None,
        };
        let (text, media) = ToolOutput::Media {
            text: "audio".into(),
            assets: vec![asset],
        }
        .into_parts();
        assert_eq!(text, "audio");
        assert_eq!(media.len(), 1);
    }
}
