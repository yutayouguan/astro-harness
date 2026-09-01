use crate::{ScrolledResponseItem, SearchHit};

/// 将 [`SearchHit`] 列表格式化为「相关历史消息」Markdown。
pub(crate) fn format_session_search_hits(hits: &[SearchHit]) -> String {
    if hits.is_empty() {
        return "未找到相关历史消息".to_string();
    }

    let body = hits
        .iter()
        .map(|h| {
            let text = if h.snippet.trim().is_empty() {
                h.context.as_str()
            } else {
                h.snippet.as_str()
            };
            let text = if text.len() > 500 {
                let mut end = 500;
                while end > 0 && !text.is_char_boundary(end) {
                    end -= 1;
                }
                format!("{}…", &text[..end])
            } else {
                text.to_string()
            };
            format!("- [{}] {}", h.session_id, text)
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!("## 相关历史消息\n{body}")
}

/// 将召回消息列表格式化为 `[id] role: content [anchor]` 多行文本。
///
/// `is_anchor` 为 true 时在行尾附加 ` [anchor]` 标记，供 LLM 识别 FTS 锚点。
pub fn format_recalled_context(items: &[ScrolledResponseItem]) -> String {
    if items.is_empty() {
        return String::new();
    }

    items
        .iter()
        .map(|entry| {
            let marker = if entry.is_anchor { " [anchor]" } else { "" };
            format!(
                "[{}] {}: {}{}",
                entry.id,
                entry.item.role().unwrap_or("item"),
                entry.item.text(),
                marker
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
