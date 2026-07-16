//! 会话工具分发：目前仅提供 `session_search`。

use anyhow::Result;

use crate::store::{SearchHit, SessionStore};

/// 分发会话相关工具。
///
/// 目前 `session_search` 直接调用 FTS 搜索并返回 Markdown 列表。
pub fn dispatch_session_tool(
    store: &SessionStore,
    name: &str,
    args: &serde_json::Value,
) -> Result<String> {
    match name {
        "session_search" => {
            let query = args["query"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("缺少 query 参数"))?;
            let limit = args["limit"].as_u64().unwrap_or(5).clamp(1, 10) as usize;
            let hits = store.search_messages(query, None, None, limit as i64)?;
            Ok(format_session_search_hits(&hits))
        }
        other => anyhow::bail!("未知会话工具: {other}"),
    }
}

fn format_session_search_hits(hits: &[SearchHit]) -> String {
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
            let tool = h
                .tool_name
                .as_deref()
                .map(|name| format!(" tool={name}"))
                .unwrap_or_default();
            format!("- [{}] {}{}{}", h.session_id, h.role, tool, if text.is_empty() { String::new() } else { format!(": {text}") })
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!("## 相关历史消息\n{body}")
}
