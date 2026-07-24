//! 记忆工具：单一 `memory` 写操作（add / replace / remove）。
//!
//! 会话历史检索请用 `search`（scope=session|…），见 `context_tools`。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;

/// `memory` tool actions.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MemoryAction {
    /// Append a refined memory entry.
    Add,
    /// Replace an entry by unique substring match.
    Replace,
    /// Remove an entry by unique substring match.
    Remove,
}

/// Memory write target: `memory` → MEMORY.md; `user` → USER.md.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum MemoryTarget {
    /// Long-term refined memory (`MEMORY.md`).
    #[default]
    Memory,
    /// User profile (`USER.md`).
    User,
}

/// Arguments for the `memory` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MemoryArgs {
    /// Action: `add` / `replace` / `remove`.
    pub action: MemoryAction,
    /// Write target; default `memory`.
    #[serde(default)]
    pub target: MemoryTarget,
    /// New content for add / replace.
    #[serde(default)]
    pub content: Option<String>,
    /// Substring used to locate the entry for replace / remove.
    #[serde(default)]
    pub old_text: Option<String>,
}

/// 向注册表注册 `memory` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "memory".to_string(),
        toolset: "memory".to_string(),
        description: "Manage persistent memory/user profile (write). action=add|replace|remove; target=memory|user. Session snapshot refreshes on next session or refresh_memory. To search memory content use context_search.".to_string(),
        schema: schema_for_args::<MemoryArgs>(),
        check_fn: None,
        icon: "brain",
        ..crate::registry::ToolEntry::lifecycle_defaults().exclusive().top_level_only()
    });
}

/// 将记忆工具调用委托给 `memory::dispatch_memory_tool`。
pub fn dispatch(
    ctx: &mut ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    memory::dispatch_memory_tool(ctx.memory, name, args)
}

fn handle(
    ctx: &mut ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    match name {
        "memory" => dispatch(ctx, name, args),
        other => anyhow::bail!("未知记忆工具: {other}"),
    }
}

crate::submit_builtin_tool! {
    register: register,
    names: ["memory"],
    sync_named: handle,
}
