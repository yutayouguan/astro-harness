//! 工具执行结果的结构化返回类型，替代原先的纯 `String`。

use crate::media::MediaAsset;

#[derive(Debug, Clone)]
pub enum ToolOutput {
    /// 纯文本结果（绝大多数工具）。
    Text(String),
    /// 带媒体资产的结果（image_gen, music_gen, speech_gen, video_gen 等）。
    Media {
        text: String,
        assets: Vec<MediaAsset>,
    },
}

impl ToolOutput {
    /// 面向模型的文本视图。
    pub fn text(&self) -> &str {
        match self {
            Self::Text(s) => s,
            Self::Media { text, .. } => text,
        }
    }

    pub fn into_text(self) -> String {
        match self {
            Self::Text(s) => s,
            Self::Media { text, .. } => text,
        }
    }

    pub fn media(&self) -> &[MediaAsset] {
        match self {
            Self::Text(_) => &[],
            Self::Media { assets, .. } => assets,
        }
    }

    pub fn into_parts(self) -> (String, Vec<MediaAsset>) {
        match self {
            Self::Text(s) => (s, Vec::new()),
            Self::Media { text, assets } => (text, assets),
        }
    }

    /// 字节长度估算（供 spill 阈值判断）。
    pub fn estimated_len(&self) -> usize {
        match self {
            Self::Text(s) => s.len(),
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
