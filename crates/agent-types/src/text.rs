//! UTF-8 安全截断，供工具结果回灌 LLM 时统一限长。

/// 工具结果默认上限（64 KiB）。
pub const MAX_TOOL_RESULT_BYTES: usize = 64 * 1024;

/// 按字节上限截断字符串，保证落在 UTF-8 字符边界上。
pub fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// 按字符数截断字符串；超限时追加省略号。
pub fn truncate_chars(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let mut out: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

/// 超限时追加 `[truncated]` 提示，便于模型分段续读。
pub fn truncate_tool_result(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let kept = truncate_utf8(s, max_bytes);
    format!(
        "{kept}\n\n[truncated] returned {kept_len}/{total} bytes (cap {max_bytes}). \
         Prefer narrower commands or exec_command with offset/limit for large content.",
        kept_len = kept.len(),
        total = s.len(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_unchanged() {
        assert_eq!(truncate_tool_result("hi", 10), "hi");
    }

    #[test]
    fn respects_char_boundary() {
        let s = "你好世界"; // each char 3 bytes
        let out = truncate_utf8(s, 5);
        assert_eq!(out, "你");
        assert!(!out.contains('\u{FFFD}'));
    }

    #[test]
    fn chars_append_ellipsis_only_when_truncated() {
        assert_eq!(truncate_chars("你好世界", 2), "你好…");
        assert_eq!(truncate_chars("你好", 2), "你好");
        assert_eq!(truncate_chars("", 0), "");
        assert_eq!(truncate_chars("a", 0), "…");
    }

    #[test]
    fn tool_result_marks_truncated() {
        let s = "a".repeat(100);
        let out = truncate_tool_result(&s, 20);
        assert!(out.contains("[truncated]"));
        assert!(out.starts_with(&"a".repeat(20)));
    }
}
