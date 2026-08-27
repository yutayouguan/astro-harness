//! 聊天交互模式（Agent / Plan / Ask）的工具能力档。
//!
//! 交互模式下的工具可见性过滤。

pub use types::InteractionMode;

/// Plan / Ask 下明确允许的工具名（其余非 MCP 默认拒绝；MCP 默认拒绝）。
/// `memory` 全写，不在此列；`skills` / `file_ops` / `todo` 另有 action 级限制。
const READONLY_ALLOW: &[&str] = &[
    "file_ops", // action 级再拦写
    "web_search",
    "web_fetch",
    "context_search",
    "skills", // action 级仅 list/load/view/curate
    "ask_user",
    "send_user_message_async",
    "todo", // Ask 模式下硬拦
    "switch_mode",
    "present",
];

/// `file_ops` 只读 operation。
const FILE_OPS_READ: &[&str] = &["read", "list", "search"];

/// `skills` 只读 / 加载类 action（禁 manage 写盘）。
const SKILLS_READ: &[&str] = &["list", "load", "view", "curate", "search"];

/// 工具是否出现在 API schema 中（只读门禁下）。
pub fn tool_visible_in_mode(mode: InteractionMode, name: &str) -> bool {
    if !mode.is_readonly_gate() {
        return true;
    }
    if name.starts_with("mcp__") {
        return false;
    }
    if mode == InteractionMode::Ask && name == "todo" {
        return false;
    }
    if name == "memory" || name == "pin_context" {
        return false;
    }
    READONLY_ALLOW.contains(&name)
}

/// 执行前硬拦；`Ok(())` 放行，`Err(msg)` 为给模型看的拒绝文案。
pub fn check_tool_call(
    mode: InteractionMode,
    name: &str,
    args: &serde_json::Value,
) -> Result<(), String> {
    if !mode.is_readonly_gate() {
        return Ok(());
    }
    if name.starts_with("mcp__") {
        return Err(format!(
            "[blocked by {} mode] MCP tools are disabled in {} mode. Use switch_mode to Agent if you need them.",
            mode.as_str(),
            mode.as_str()
        ));
    }
    if name == "memory" || name == "pin_context" {
        return Err(format!(
            "[blocked by {} mode] `{name}` writes persistent memory/context. Call switch_mode(to=\"agent\") if needed.",
            mode.as_str()
        ));
    }
    if mode == InteractionMode::Ask && name == "todo" {
        return Err(
            "[blocked by ask mode] todo writes checklist files. Stay read-only, or switch to Plan/Agent."
                .into(),
        );
    }
    if !READONLY_ALLOW.contains(&name) {
        return Err(format!(
            "[blocked by {} mode] Tool `{name}` is not available. Stay read-only, or call switch_mode(to=\"agent\", …) after the plan is ready.",
            mode.as_str()
        ));
    }
    if name == "file_ops" {
        let op = args
            .get("operation")
            .or_else(|| args.get("action"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if !FILE_OPS_READ.iter().any(|o| *o == op) {
            return Err(format!(
                "[blocked by {} mode] file_ops operation `{op}` writes or mutates the workspace. Only read/list/search are allowed. Call switch_mode(to=\"agent\") to execute.",
                mode.as_str()
            ));
        }
    }
    if name == "skills" {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("load")
            .trim()
            .to_ascii_lowercase();
        // manage / create / update / patch / delete 等写盘
        if action == "manage"
            || action == "create"
            || action == "update"
            || action == "patch"
            || action == "delete"
            || action == "write"
        {
            return Err(format!(
                "[blocked by {} mode] skills action `{action}` mutates skill files. Only list/load/view/curate are allowed.",
                mode.as_str()
            ));
        }
        if !SKILLS_READ.iter().any(|a| *a == action) && !action.is_empty() {
            // 未知 action：保守拦截写类
            if action.contains("write") || action.contains("edit") || action.contains("remove") {
                return Err(format!(
                    "[blocked by {} mode] skills action `{action}` is not allowed in read-only mode.",
                    mode.as_str()
                ));
            }
        }
    }
    if name == "terminal" {
        return Err(format!(
            "[blocked by {} mode] terminal is disabled (side effects). Call switch_mode(to=\"agent\") when ready to execute.",
            mode.as_str()
        ));
    }
    Ok(())
}

/// 过滤 OpenAI 风格 tools schema 列表。
pub fn filter_schemas(
    mode: InteractionMode,
    schemas: Vec<serde_json::Value>,
) -> Vec<serde_json::Value> {
    if !mode.is_readonly_gate() {
        return schemas;
    }
    schemas
        .into_iter()
        .filter(|s| {
            s.pointer("/function/name")
                .and_then(|n| n.as_str())
                .is_some_and(|name| tool_visible_in_mode(mode, name))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plan_blocks_write_file_ops() {
        let err = check_tool_call(
            InteractionMode::Plan,
            "file_ops",
            &json!({ "operation": "write", "path": "a.md", "content": "x" }),
        )
        .unwrap_err();
        assert!(err.contains("blocked"));
    }

    #[test]
    fn plan_allows_read_file_ops() {
        assert!(check_tool_call(
            InteractionMode::Plan,
            "file_ops",
            &json!({ "operation": "read", "path": "a.md" }),
        )
        .is_ok());
    }

    #[test]
    fn plan_blocks_memory_and_skills_manage() {
        assert!(check_tool_call(
            InteractionMode::Plan,
            "memory",
            &json!({ "action": "add", "content": "x" }),
        )
        .is_err());
        assert!(check_tool_call(
            InteractionMode::Plan,
            "skills",
            &json!({ "action": "manage", "op": "create" }),
        )
        .is_err());
        assert!(check_tool_call(
            InteractionMode::Plan,
            "skills",
            &json!({ "action": "load", "skill_id": "x" }),
        )
        .is_ok());
    }

    #[test]
    fn ask_blocks_todo() {
        assert!(check_tool_call(InteractionMode::Ask, "todo", &json!({ "title": "t" }),).is_err());
        assert!(!tool_visible_in_mode(InteractionMode::Ask, "todo"));
        assert!(tool_visible_in_mode(InteractionMode::Plan, "todo"));
    }

    #[test]
    fn system_guidance_mentions_mode() {
        assert!(InteractionMode::Agent.system_guidance().contains("Agent"));
        assert!(InteractionMode::Agent
            .system_guidance()
            .contains("compact todo"));
        assert!(InteractionMode::Plan.system_guidance().contains("Plan"));
        assert!(InteractionMode::Plan
            .system_guidance()
            .contains("explicit user approval"));
        assert!(InteractionMode::Ask.system_guidance().contains("Ask"));
        // 中英并列，避免英文 UI 丢失指引
        for mode in [
            InteractionMode::Agent,
            InteractionMode::Plan,
            InteractionMode::Ask,
        ] {
            let g = mode.system_guidance();
            assert!(
                g.contains("Interaction mode:") && g.contains("交互模式："),
                "expected bilingual guidance for {:?}: {g}",
                mode
            );
        }
    }

    #[test]
    fn agent_allows_write() {
        assert!(check_tool_call(
            InteractionMode::Agent,
            "file_ops",
            &json!({ "operation": "write", "path": "a.md", "content": "x" }),
        )
        .is_ok());
    }

    #[test]
    fn plan_hides_terminal_in_schema() {
        assert!(!tool_visible_in_mode(InteractionMode::Plan, "terminal"));
        assert!(tool_visible_in_mode(InteractionMode::Plan, "web_search"));
        assert!(tool_visible_in_mode(InteractionMode::Plan, "switch_mode"));
        assert!(!tool_visible_in_mode(InteractionMode::Plan, "memory"));
    }
}
