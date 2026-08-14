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

/// 当前已注册的内置 handler 名称（测试 / 观测用）。
pub fn builtin_handler_names() -> Vec<&'static str> {
    let mut names: Vec<_> = handler_table().keys().copied().collect();
    names.sort_unstable();
    names
}

/// 按工具名将调用路由到对应实现（内置 + 动态 MCP 统一入口）。
///
/// # 流程
/// 1. 通过 `registry_allows` 闭包检查 toolset 是否启用
/// 2. 记账（审计日志 + 用量统计）
/// 3. 内置 handler 表查找 → 动态 handler 查找 → Skill soft-alias → 未知工具
pub async fn dispatch_tool(
    registry_allows: impl Fn(&str) -> bool,
    ctx: &mut ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
    dynamic_handler: Option<&crate::registry::DynToolHandler>,
) -> anyhow::Result<types::ToolOutput> {
    if !registry_allows(name) {
        let toolset = home::tool_name_to_toolset(name);
        anyhow::bail!("工具已禁用（tools-enabled.json → {toolset}=false）: {name}");
    }

    let agent_id = ctx.memory.agent_id.clone();
    let _ = home::record_tool_call(&agent_id, name, args);
    let _ = usage::record_tool_call(
        &agent_id,
        name,
        args,
        Some(ctx.session_id.as_str()),
        ctx.turn_id.as_deref(),
    );

    // 1. 内置 handler（静态 inventory 注册）
    if let Some(handler) = handler_table().get(name) {
        return handler(ctx, name, args).await;
    }

    // 2. 动态 handler（MCP 工具等运行时注册）
    if let Some(dyn_handler) = dynamic_handler {
        return dyn_handler(name, args).await;
    }

    // 3. Skill soft-alias
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
