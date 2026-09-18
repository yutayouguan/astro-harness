//! 检索文本切分与 FTS5 查询构造。
//!
//! SQLite FTS5 内置分词器没有中文分词能力：`unicode61` 会把一整串连续汉字当作**单个**
//! token（`今天天气怎么样` 就是一个词条），`trigram` 又要求查询串至少 3 个字符。
//! 结果是中文最常见的 1–2 字查询在索引层一条都命中不了。
//!
//! 这里把中文按字切开后交给 `unicode61`：每个汉字成为独立 token，短语查询
//! （`"会 话"`）要求 token 相邻，于是任意长度的中文子串都能精确命中且顺序受约束。
//! 英文、数字与代码保持原词，行为不变。

/// 该字符是否需要在检索索引里按字切开。
///
/// 覆盖 CJK 统一表意文字（含扩展 A/B 及更高区）、兼容表意文字、日文假名与韩文音节。
/// CJK 标点不在其中——它们在 `unicode61` 里本来就是分隔符。
pub fn is_segmentable(c: char) -> bool {
    matches!(
        c as u32,
        0x2E80..=0x2EFF      // CJK 部首补充
        | 0x3040..=0x30FF    // 平假名 / 片假名
        | 0x31F0..=0x31FF    // 片假名语音扩展
        | 0x3400..=0x4DBF    // CJK 扩展 A
        | 0x4E00..=0x9FFF    // CJK 统一表意文字
        | 0xAC00..=0xD7A3    // 韩文音节
        | 0xF900..=0xFAFF    // CJK 兼容表意文字
        | 0x20000..=0x3FFFF  // CJK 扩展 B 及更高区
    )
}

/// 生成写入索引的切分文本：CJK 字符两侧插空格，其余原样，空白折叠为单空格。
///
/// 返回值只用于 FTS 索引列，不用于展示；原文始终保留在独立的列里。
pub fn segment_for_index(text: &str) -> String {
    let mut spaced = String::with_capacity(text.len() + text.len() / 2);
    for c in text.chars() {
        if is_segmentable(c) {
            spaced.push(' ');
            spaced.push(c);
            spaced.push(' ');
        } else {
            spaced.push(c);
        }
    }
    collapse_whitespace(&spaced)
}

/// 折叠连续空白为单个空格，并去掉首尾空白。
fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(c);
    }
    out
}

/// 把用户查询改写成 FTS5 的 `MATCH` 表达式。
///
/// 与 [`segment_for_index`] 使用同一套切分，保证索引侧与查询侧一致；整体包成短语
/// 以免用户输入被当成 FTS 运算符执行，末位 `*` 让英文查询支持前缀匹配
/// （`retry` 命中 `retrying`），用来替代被移除的 trigram 子串索引。
///
/// 查询切分后为空（例如纯标点输入）时返回 `None`，调用方应直接返回空结果。
pub fn match_query(raw: &str) -> Option<String> {
    let segmented = segment_for_index(raw);
    if !segmented.chars().any(is_token_char) {
        return None;
    }
    let escaped = segmented.replace('"', "\"\"");
    Some(format!("\"{escaped}\"*"))
}

/// 该字符能否产生一个 `unicode61` token。
///
/// `unicode61` 只把字母与数字当作 token 字符，其余（标点、符号、空白）都是分隔符。
fn is_token_char(c: char) -> bool {
    c.is_alphanumeric() || is_segmentable(c)
}

/// 在原文里定位查询片段（大小写不敏感），用于生成展示摘要。
///
/// 匹配走原始输入而不是切分结果：展示的是原文，切片也必须落在原文的字符边界上。
pub fn find_match_range(text: &str, query: &str) -> Option<(usize, usize)> {
    let needle = query.trim();
    if needle.is_empty() {
        return None;
    }
    let haystack = text.to_lowercase();
    let needle_lower = needle.to_lowercase();
    let start = haystack.find(&needle_lower)?;
    let end = start + needle_lower.len();
    if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
        return None;
    }
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_chinese_per_character() {
        assert_eq!(segment_for_index("会话列表"), "会 话 列 表");
    }

    #[test]
    fn keeps_english_and_code_intact() {
        assert_eq!(
            segment_for_index("state.db retry_after_ack"),
            "state.db retry_after_ack"
        );
        assert_eq!(
            segment_for_index("修复 agent-db 的 WAL"),
            "修 复 agent-db 的 WAL"
        );
    }

    #[test]
    fn collapses_whitespace_and_trims() {
        assert_eq!(segment_for_index("  会话\n\n列表  "), "会 话 列 表");
        assert_eq!(segment_for_index(""), "");
    }

    #[test]
    fn segments_kana_and_hangul() {
        assert_eq!(segment_for_index("ひらがな"), "ひ ら が な");
        assert_eq!(segment_for_index("한글"), "한 글");
    }

    #[test]
    fn cjk_punctuation_stays_a_separator() {
        assert!(!is_segmentable('，'));
        assert_eq!(segment_for_index("会话，列表"), "会 话 ， 列 表");
    }

    #[test]
    fn match_query_wraps_phrase_and_prefixes() {
        assert_eq!(match_query("会话").as_deref(), Some("\"会 话\"*"));
        assert_eq!(match_query("retry").as_deref(), Some("\"retry\"*"));
    }

    #[test]
    fn match_query_escapes_quotes_and_operators() {
        assert_eq!(match_query("a\"b").as_deref(), Some("\"a\"\"b\"*"));
        // 运算符被整体包进短语，不会作为 FTS 语法执行。
        assert_eq!(match_query("OR").as_deref(), Some("\"OR\"*"));
    }

    #[test]
    fn match_query_is_none_for_empty_input() {
        assert_eq!(match_query(""), None);
        assert_eq!(match_query("   "), None);
        assert_eq!(match_query("，。"), None);
        assert_eq!(match_query("***"), None);
    }

    #[test]
    fn find_match_range_is_case_insensitive_on_original_text() {
        let text = "Retrying the CONNECTION";
        assert_eq!(find_match_range(text, "retrying"), Some((0, 8)));
        assert_eq!(find_match_range(text, "connection"), Some((13, 23)));
        assert_eq!(find_match_range(text, "会话"), None);
    }
}
