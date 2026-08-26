//! MCP 工具命名：`mcp__{server_id}__{tool_name}`。
//!
//! `server_id` 中的 `__` 会被压成 `_`，避免与分隔符冲突。

/// 工具集 id：所有 MCP 工具归入此 toolset。
pub const MCP_TOOLSET: &str = "mcp";

/// 限定工具名前缀。
pub const MCP_PREFIX: &str = "mcp__";

/// 生成 LLM 可见的限定工具名（自动 sanitize `server_id` 和 `tool_name`）。
pub fn qualify_tool_name(server_id: &str, tool_name: &str) -> String {
    let sid = sanitize_server_id(server_id);
    let tn = sanitize_tool_name(tool_name);
    format!("{MCP_PREFIX}{sid}__{tn}")
}

/// 解析限定名 → `(server_id, native_tool_name)`；非 MCP 名返回 `None`。
pub fn parse_qualified_name(name: &str) -> Option<(&str, &str)> {
    let rest = name.strip_prefix(MCP_PREFIX)?;
    rest.split_once("__")
}

/// 是否为合法 MCP 限定工具名（含前缀且其后仍含 `__`）。
pub fn is_mcp_tool_name(name: &str) -> bool {
    name.starts_with(MCP_PREFIX) && name[MCP_PREFIX.len()..].contains("__")
}

/// 确保 `server_id` 不含 `__`，避免限定名歧义；非法字符替换为 `_`。
pub fn sanitize_server_id(id: &str) -> String {
    sanitize_name_chars(&id.replace("__", "_"))
}

/// 确保工具名仅含 `[a-zA-Z0-9_-]`，其余字符替换为 `_`。
pub fn sanitize_tool_name(name: &str) -> String {
    sanitize_name_chars(name)
}

fn sanitize_name_chars(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let q = qualify_tool_name("srv1", "read_file");
        assert_eq!(q, "mcp__srv1__read_file");
        assert_eq!(parse_qualified_name(&q), Some(("srv1", "read_file")));
    }

    #[test]
    fn tool_name_may_contain_underscore() {
        let q = qualify_tool_name("abc", "read_file_v2");
        assert_eq!(parse_qualified_name(&q), Some(("abc", "read_file_v2")));
    }

    #[test]
    fn sanitizes_double_underscore_in_server_id() {
        let q = qualify_tool_name("a__b", "tool");
        assert_eq!(q, "mcp__a_b__tool");
        assert_eq!(parse_qualified_name(&q), Some(("a_b", "tool")));
    }

    #[test]
    fn rejects_non_mcp() {
        assert!(parse_qualified_name("web_search").is_none());
        assert!(!is_mcp_tool_name("web_search"));
    }

    #[test]
    fn sanitizes_dots_in_tool_name() {
        let q = qualify_tool_name("srv", "search.documents");
        assert_eq!(q, "mcp__srv__search_documents");
    }

    #[test]
    fn sanitizes_special_chars_in_tool_name() {
        let q = qualify_tool_name("srv", "ns:read/file");
        assert_eq!(q, "mcp__srv__ns_read_file");
    }

    #[test]
    fn sanitizes_dots_in_server_id() {
        let q = qualify_tool_name("my.server", "tool");
        assert_eq!(q, "mcp__my_server__tool");
    }
}
