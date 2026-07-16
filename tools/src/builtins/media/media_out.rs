//! 生成类工具结果：附加 `astro_media_v1` sidecar。

use common::{append_media_sidecar, MediaAsset, MediaKind};

/// 在人类可读工具结果后附加结构化媒体 sidecar。
pub fn with_generated_media(
    text: impl AsRef<str>,
    kind: MediaKind,
    path: &str,
    mime_type: &str,
    label: &str,
) -> String {
    let asset = MediaAsset::workspace(kind, path, mime_type).with_label(label);
    append_media_sidecar(text.as_ref(), &[asset])
}
