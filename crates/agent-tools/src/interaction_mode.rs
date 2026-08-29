//! 聊天交互模式（Agent / Plan）的工具能力档。
//!
//! 交互模式下的工具可见性过滤。

pub use types::InteractionMode;

/// Plan 下明确允许的工具名（其余非 MCP 默认拒绝；MCP 默认拒绝）。
/// `memory` 全写，不在此列；`skills` / `todo` 另有 action 级限制。
const READONLY_ALLOW: &[&str] = &[
    "web_search",
    "web_fetch",
    "browser_open",
    "browser_snapshot",
    "browser_scroll",
    "browser_wait",
    "browser_screenshot",
    "browser_close",
    "context_search",
    "skills", // action 级仅 list/load/view/curate
    "ask_user",
    "send_user_message_async",
    "todo",
    "switch_mode",
    "present",
];

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
    if !READONLY_ALLOW.contains(&name) {
        return Err(format!(
            "[blocked by {} mode] Tool `{name}` is not available. Stay read-only, or call switch_mode(to=\"agent\", …) after the plan is ready.",
            mode.as_str()
        ));
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
    if name == "exec_command" {
        return Err(format!(
            "[blocked by {} mode] exec_command is disabled (side effects). Call switch_mode(to=\"agent\") when ready to execute.",
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
    fn system_guidance_mentions_mode() {
        assert!(InteractionMode::Agent.system_guidance().contains("Agent"));
        assert!(InteractionMode::Agent
            .system_guidance()
            .contains("compact todo"));
        assert!(InteractionMode::Plan.system_guidance().contains("Plan"));
        assert!(InteractionMode::Plan
            .system_guidance()
            .contains("explicit user approval"));
        // 中英并列，避免英文 UI 丢失指引
        for mode in [InteractionMode::Agent, InteractionMode::Plan] {
            let g = mode.system_guidance();
            assert!(
                g.contains("Interaction mode:") && g.contains("交互模式："),
                "expected bilingual guidance for {:?}: {g}",
                mode
            );
        }
    }

    #[test]
    fn plan_hides_terminal_in_schema() {
        assert!(!tool_visible_in_mode(InteractionMode::Plan, "exec_command"));
        assert!(tool_visible_in_mode(InteractionMode::Plan, "web_search"));
        assert!(tool_visible_in_mode(InteractionMode::Plan, "switch_mode"));
        assert!(!tool_visible_in_mode(InteractionMode::Plan, "memory"));
    }
}
