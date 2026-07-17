//! 工具统一分发：按名称从自注册 handler 表查找并执行。
//!
//! 所有 Agent 侧的工具执行均经 [`dispatch_tool`] 入口，确保禁用工具、
//! 调用统计与错误格式保持一致。中央 match 已移除；内置路由由
//! [`crate::registry::BuiltinToolRegistrar`] inventory 构建。

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::context::ToolContext;
use crate::registry::{BuiltinToolHandler, BuiltinToolRegistrar};

/// 从 inventory 构建的内置工具 name → handler 表（启动时检测重名）。
fn handler_table() -> &'static HashMap<&'static str, BuiltinToolHandler> {
    static TABLE: OnceLock<HashMap<&'static str, BuiltinToolHandler>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut map = HashMap::new();
        for hook in inventory::iter::<BuiltinToolRegistrar> {
            for &name in hook.names {
                if map.insert(name, hook.handler).is_some() {
                    panic!("duplicate builtin tool handler name: {name}");
                }
            }
        }
        map
    })
}

/// 当前已注册的内置 handler 名称（含兼容别名；测试 / 观测用）。
pub fn builtin_handler_names() -> Vec<&'static str> {
    let mut names: Vec<_> = handler_table().keys().copied().collect();
    names.sort_unstable();
    names
}

/// 按工具名将调用路由到对应内置实现。
///
/// # 流程
/// 1. 通过 `registry_allows` 闭包检查 toolset 是否启用，禁用时立即返回错误。
/// 2. 调用 `home::record_tool_call` 写审计日志，并 `record_usage_tool_call` 累加用量。
/// 3. 从自注册 handler 表按 `name` 查找并 `await`；未命中时尝试 Skill soft-alias。
///
/// # 参数
/// - `registry_allows`：通常传入 `registry.is_tool_allowed`，用于读取 `tools-enabled.json` 状态。
/// - `ctx`：可变执行上下文，部分工具（如 `memory`、`create_agent`）会修改其中的 `memory` 或 `workspace_dir`。
///
/// # 约束
/// - 未知工具名返回 `未知工具` 错误；MCP 工具不由本函数处理。
pub async fn dispatch_tool(
    registry_allows: impl Fn(&str) -> bool,
    ctx: &mut ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    if !registry_allows(name) {
        let toolset = home::tool_name_to_toolset(name);
        anyhow::bail!("工具已禁用（tools-enabled.json → {toolset}=false）: {name}");
    }

    // 先记账再执行：即使失败也计入一次「发起调用」
    let agent_id = ctx.memory.agent_id.clone();
    let _ = home::record_tool_call(&agent_id, name, args);
    let _ = usage::record_tool_call(
        &agent_id,
        name,
        args,
        Some(ctx.session_id.as_str()),
        ctx.turn_id.as_deref(),
    );

    if let Some(handler) = handler_table().get(name) {
        return handler(ctx, name, args).await;
    }

    // Soft-alias：模型常把 Skill 名当成工具名；若命中已启用 Skill，改走 skills 工具。
    if home::is_tool_call_allowed("skills")
        && skills::list_installed()
            .into_iter()
            .any(|s| s.name == name && s.enabled)
    {
        let rewritten = serde_json::json!({
            "action": "load",
            "skill_id": name,
            "input": args,
        });
        if let Some(handler) = handler_table().get("skills") {
            return handler(ctx, "skills", &rewritten).await;
        }
    }

    anyhow::bail!("未知工具: {name}")
}
