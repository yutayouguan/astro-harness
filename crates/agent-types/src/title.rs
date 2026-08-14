//! 会话标题清洗：去掉 Markdown/引号包装，并限制 Unicode 长度。

/// 清洗模型返回的标题：取首行、去粗体/引号包装，截断到 `max_chars`。
///
/// 空结果表示不可用（调用方应跳过写入）。
pub fn sanitize_title(raw: &str, max_chars: usize) -> String {
    let mut s = raw.lines().next().unwrap_or("").trim().to_string();
    if s.is_empty() {
        return String::new();
    }

    // 反复去掉外层粗体/斜体包装
    loop {
        let trimmed = s.trim();
        let next = if let Some(inner) = strip_wrapping(trimmed, "**") {
            inner
        } else if let Some(inner) = strip_wrapping(trimmed, "__") {
            inner
        } else if let Some(inner) = strip_wrapping(trimmed, "*") {
            inner
        } else if let Some(inner) = strip_wrapping(trimmed, "_") {
            inner
        } else {
            break;
        };
        let next = next.trim().to_string();
        if next == s {
            break;
        }
        s = next;
    }

    s = strip_quote_wrappers(s.trim()).trim().to_string();
    // 残留星号（模型偶发）
    s = s.replace("**", "").trim().to_string();

    if max_chars == 0 {
        return String::new();
    }
    s.chars()
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_string()
}

fn strip_wrapping<'a>(s: &'a str, marker: &str) -> Option<&'a str> {
    s.strip_prefix(marker)?.strip_suffix(marker)
}

fn strip_quote_wrappers(s: &str) -> String {
    let pairs = [
        ('「', '」'),
        ('『', '』'),
        ('"', '"'),
        ('“', '”'),
        ('\'', '\''),
        ('‘', '’'),
    ];
    for (open, close) in pairs {
        let mut chars = s.chars();
        if chars.next() == Some(open) {
            let mut body: Vec<char> = chars.collect();
            if body.last().copied() == Some(close) {
                body.pop();
                return body.into_iter().collect();
            }
        }
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_title_removes_wrappers_and_limits_chars() {
        assert_eq!(
            sanitize_title(" **「Rust 会话管理」**\n解释", 20),
            "Rust 会话管理"
        );
    }

    #[test]
    fn empty_title_is_rejected() {
        assert!(sanitize_title(" \n ** ", 20).is_empty());
    }

    #[test]
    fn truncates_to_max_chars() {
        let long = "一二三四五六七八九十";
        assert_eq!(sanitize_title(long, 4), "一二三四");
    }
}
