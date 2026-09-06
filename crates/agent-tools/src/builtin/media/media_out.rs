//! 生成类工具结果：统一落盘文件名与结构化 `ToolOutput::Media` 构建。

use types::{MediaAsset, MediaKind, ToolOutput};

/// 构造 `ToolOutput::Media`，替代 sidecar 拼接。
pub fn media_output(
    text: impl Into<String>,
    kind: MediaKind,
    path: &str,
    mime_type: &str,
    label: &str,
) -> ToolOutput {
    let asset = MediaAsset::workspace(kind, path, mime_type).with_label(label);
    ToolOutput::Media {
        text: text.into(),
        assets: vec![asset],
    }
}

/// 生成可读文件名：`{标题}-{时间戳}-{短uuid}.{ext}`。
///
/// `title` 优先；清洗后为空则用 `fallback_zh`（如「音乐」「图片」）。
pub fn generated_media_filename(title: Option<&str>, fallback_zh: &str, ext: &str) -> String {
    let stem = sanitize_media_title(title).unwrap_or_else(|| {
        sanitize_media_title(Some(fallback_zh)).unwrap_or_else(|| "生成".into())
    });
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let id = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let ext = ext.trim_start_matches('.');
    format!("{stem}-{stamp}-{id}.{ext}")
}

/// 清洗为适合跨平台文件名的短标题（保留中文/字母/数字）。
pub fn sanitize_media_title(raw: Option<&str>) -> Option<String> {
    let s = raw?.trim();
    if s.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(s.len().min(64));
    let mut last_was_sep = false;
    for ch in s.chars() {
        let ok = ch.is_alphanumeric()
            || ('\u{4e00}'..='\u{9fff}').contains(&ch)
            || ('\u{3400}'..='\u{4dbf}').contains(&ch)
            || matches!(ch, '_' | '-' | '·' | '—');
        if ok {
            out.push(ch);
            last_was_sep = false;
        } else if matches!(
            ch,
            ' ' | '\t' | '\n' | '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
        ) && !last_was_sep
            && !out.is_empty()
        {
            out.push('-');
            last_was_sep = true;
        }
        // 标题部分不宜过长（按字符计）
        if out.chars().count() >= 40 {
            break;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        None
    } else {
        Some(trimmed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_chinese_and_strips_path_chars() {
        assert_eq!(
            sanitize_media_title(Some("  采菌子歌 / 云南  ")).as_deref(),
            Some("采菌子歌-云南")
        );
        assert_eq!(
            sanitize_media_title(Some("../evil")).as_deref(),
            Some("evil")
        );
        assert!(sanitize_media_title(Some("   ")).is_none());
        assert!(sanitize_media_title(None).is_none());
    }

    #[test]
    fn filename_uses_title_then_stamp() {
        let name = generated_media_filename(Some("采菌子歌"), "音乐", "mp3");
        assert!(name.starts_with("采菌子歌-"));
        assert!(name.ends_with(".mp3"));
        // 格式：stem-YYYYMMDD-HHMMSS-xxxxxxxx.mp3
        let parts: Vec<_> = name.trim_end_matches(".mp3").rsplitn(3, '-').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].len(), 8); // uuid
    }

    #[test]
    fn filename_falls_back_to_chinese_kind() {
        let name = generated_media_filename(None, "图片", "png");
        assert!(name.starts_with("图片-"));
        assert!(name.ends_with(".png"));
    }
}
