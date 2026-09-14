//! 工具执行结果的结构化返回类型，替代原先的纯 `String`。

use crate::media::MediaAsset;
use serde::{Deserialize, Serialize};

pub const MAX_TOOL_RESULT_METADATA_BYTES: usize = 16 * 1024;

/// Host-owned metadata returned alongside a tool result.
///
/// This is persisted in `ResponseItem` passthrough metadata for attribution and
/// auditing, but remains separate from the model-visible text payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolResultMetadata {
    Value(serde_json::Value),
    OmittedDueToSizeLimit,
}

impl ToolResultMetadata {
    pub fn capture(value: serde_json::Value) -> Self {
        if serde_json::to_vec(&value)
            .is_ok_and(|encoded| encoded.len() <= MAX_TOOL_RESULT_METADATA_BYTES)
        {
            Self::Value(value)
        } else {
            Self::OmittedDueToSizeLimit
        }
    }

    pub fn stored_value(&self) -> serde_json::Value {
        match self {
            Self::Value(value) => value.clone(),
            Self::OmittedDueToSizeLimit => {
                serde_json::Value::String("omitted_due_to_size_limit".into())
            }
        }
    }

    fn estimated_len(&self) -> usize {
        match self {
            Self::Value(value) => serde_json::to_vec(value).map_or(0, |encoded| encoded.len()),
            Self::OmittedDueToSizeLimit => "omitted_due_to_size_limit".len(),
        }
    }
}

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
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
    /// A tool result with bounded, host-owned metadata.
    WithMetadata {
        output: Box<ToolOutput>,
        metadata: ToolResultMetadata,
    },
}

impl ToolOutput {
    /// 面向模型的文本视图。
    pub fn text(&self) -> &str {
        match self {
            Self::Text(s) => s,
            Self::Media { text, .. } => text,
            Self::FileChanges { text, .. } => text,
            Self::WithMetadata { output, .. } => output.text(),
        }
    }

    pub fn into_text(self) -> String {
        match self {
            Self::Text(s) => s,
            Self::Media { text, .. } => text,
            Self::FileChanges { text, .. } => text,
            Self::WithMetadata { output, .. } => output.into_text(),
        }
    }

    pub fn media(&self) -> &[MediaAsset] {
        match self {
            Self::Text(_) => &[],
            Self::Media { assets, .. } => assets,
            Self::FileChanges { .. } => &[],
            Self::WithMetadata { output, .. } => output.media(),
        }
    }

    pub fn file_changes(&self) -> &[ToolFileChange] {
        match self {
            Self::FileChanges { changes, .. } => changes,
            Self::WithMetadata { output, .. } => output.file_changes(),
            _ => &[],
        }
    }

    pub fn metadata(&self) -> Option<&ToolResultMetadata> {
        match self {
            Self::WithMetadata { metadata, .. } => Some(metadata),
            _ => None,
        }
    }

    pub fn with_metadata(self, value: serde_json::Value) -> Self {
        let metadata = ToolResultMetadata::capture(value);
        match self {
            Self::WithMetadata { output, .. } => Self::WithMetadata { output, metadata },
            output => Self::WithMetadata {
                output: Box::new(output),
                metadata,
            },
        }
    }

    /// Replace only the model-visible text while retaining media, file changes,
    /// and host-owned result metadata.
    pub fn with_text(self, text: String) -> Self {
        match self {
            Self::Text(_) => Self::Text(text),
            Self::Media { assets, .. } => Self::Media { text, assets },
            Self::FileChanges { changes, .. } => Self::FileChanges { text, changes },
            Self::WithMetadata { output, metadata } => Self::WithMetadata {
                output: Box::new(output.with_text(text)),
                metadata,
            },
        }
    }

    /// Consume the model-visible text/media projection. Host-only metadata is
    /// intentionally not exposed to nested runtimes such as Code Mode.
    pub fn into_parts(self) -> (String, Vec<MediaAsset>) {
        match self {
            Self::Text(s) => (s, Vec::new()),
            Self::Media { text, assets } => (text, assets),
            Self::FileChanges { text, .. } => (text, Vec::new()),
            Self::WithMetadata { output, .. } => output.into_parts(),
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
                            change.root.as_ref().map_or(0, String::len)
                                + change.path.len()
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
            Self::WithMetadata { output, metadata } => {
                output.estimated_len() + metadata.estimated_len()
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
            root: None,
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

    #[test]
    fn metadata_is_bounded_and_survives_text_replacement() {
        let output = ToolOutput::Media {
            text: "before".into(),
            assets: vec![],
        }
        .with_metadata(serde_json::json!({"provider": {"request_id": "r-1"}}))
        .with_text("after".into());
        assert_eq!(output.text(), "after");
        assert_eq!(
            output.metadata().unwrap().stored_value(),
            serde_json::json!({"provider": {"request_id": "r-1"}})
        );

        let oversized = ToolOutput::from("ok").with_metadata(serde_json::json!({
            "value": "x".repeat(MAX_TOOL_RESULT_METADATA_BYTES)
        }));
        assert_eq!(
            oversized.metadata().unwrap().stored_value(),
            serde_json::Value::String("omitted_due_to_size_limit".into())
        );

        let replaced = ToolOutput::from("ok")
            .with_metadata(serde_json::json!({"version":1}))
            .with_metadata(serde_json::json!({"version":2}));
        assert_eq!(
            replaced.metadata().unwrap().stored_value(),
            serde_json::json!({"version":2})
        );
    }
}
