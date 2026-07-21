//! 聊天交互模式（Agent / Plan / Ask / MultiTask）的工具能力档。
//!
//! Plan / Ask：只读向；写文件、有副作用终端、委派等硬拦。
//! Ask 比 Plan 更严（禁 todo）。
//! Agent / MultiTask：不额外限制（仍受 tools_enabled 约束）。

use serde::{Deserialize, Serialize};

/// 与前端 `ChatInteractionMode` 对齐的交互模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InteractionMode {
    #[default]
    Agent,
    Plan,
    Ask,
    Multitask,
}

impl InteractionMode {
    /// 解析字符串；未知值回落 Agent。
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "plan" => Self::Plan,
            "ask" => Self::Ask,
            "multitask" => Self::Multitask,
            _ => Self::Agent,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Plan => "plan",
            Self::Ask => "ask",
            Self::Multitask => "multitask",
        }
    }

    /// 写入 system prompt 的模式行为说明（非用户消息）。
    ///
    /// 工具可见性/硬拦仍由 `filter_schemas` / `check_tool_call` 强制；此处只告诉模型当前档位意图。
    /// 中英并列，避免仅依赖 UI locale 时英文界面丢失指引。
    pub fn system_guidance(self) -> &'static str {
        match self {
            Self::Agent => {
                "# Interaction mode: Agent / 交互模式：Agent\n\
Tools enabled. For complex multi-step work, call switch_mode(to=\"plan\", reason=…) first, then return to Agent after authorization.\n\
可执行工具完成任务。复杂多步工作可先调用 switch_mode(to=\"plan\", reason=…) 进入规划，再在授权后回到 Agent 执行。"
            }
            Self::Plan => {
                "# Interaction mode: Plan (read-only planning) / 交互模式：Plan（只读规划）\n\
Read-only: file_ops(read/list/search), web_search, todo. No writes, terminal, code_exec, subagent, pipeline, or memory. When ready, call switch_mode(to=\"agent\", reason=…, summary=plan summary).\n\
可用 file_ops(read/list/search)、web_search、todo 等只读工具。禁止写文件、terminal、code_exec、subagent、pipeline、memory。\n\
计划就绪后调用 switch_mode(to=\"agent\", reason=…, summary=计划摘要) 请求执行授权。"
            }
            Self::Ask => {
                "# Interaction mode: Ask (read-only Q&A) / 交互模式：Ask（只读问答）\n\
Explain and retrieve; do not modify files or run side effects. To implement, call switch_mode(to=\"agent\", reason=…, summary=plan).\n\
以解释与检索为主，不要修改文件或执行有副作用的操作。若需落地实现，可 switch_mode(to=\"agent\", reason=…, summary=计划摘要)。"
            }
            Self::Multitask => {
                "# Interaction mode: MultiTask / 交互模式：MultiTask\n\
Split the goal with subagent (parallel one-shot) or pipeline (serial roles); then summarize.\n\
用 subagent 并行拆临时子任务，或用 pipeline 串行多角色，再汇总结果。"
            }
        }
    }

    /// Plan / Ask 启用只读工具门禁。
    pub fn is_readonly_gate(self) -> bool {
        matches!(self, Self::Plan | Self::Ask)
    }
}

/// Plan / Ask 下明确允许的工具名（其余非 MCP 默认拒绝；MCP 默认拒绝）。
/// `memory` 全写，不在此列；`skills` / `file_ops` / `todo` 另有 action 级限制。
const READONLY_ALLOW: &[&str] = &[
    "file_ops", // action 级再拦写
    "web_search",
    "web_extract",
    "http_fetch",
    "search",
    "skills", // action 级仅 list/load/view/curate
    "ask_user",
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
        assert!(InteractionMode::Plan.system_guidance().contains("Plan"));
        assert!(InteractionMode::Ask.system_guidance().contains("Ask"));
        assert!(InteractionMode::Multitask
            .system_guidance()
            .contains("MultiTask"));
        // 中英并列，避免英文 UI 丢失指引
        for mode in [
            InteractionMode::Agent,
            InteractionMode::Plan,
            InteractionMode::Ask,
            InteractionMode::Multitask,
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
