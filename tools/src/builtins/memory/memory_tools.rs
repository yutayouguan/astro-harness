//! 记忆工具：单一 `memory` 写操作；`session_search` 走 `session` crate。
//!
//! `memory` 通过 `action` + `target` 覆盖 add / replace / remove；
//! 实际逻辑委托给 `memory` crate。`session_search` 检索历史消息（FTS），
//! 而非会话摘要表。

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

/// Arguments for the `session_search` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SessionSearchArgs {
    /// FTS5 full-text query (matches message body / tool names, etc.).
    pub query: String,
    /// Max results; default 5, max 10 (matches DB LIMIT).
    #[serde(default)]
    pub limit: Option<u32>,
}

/// 向注册表注册记忆相关工具（`memory` + `session_search`）。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "memory".to_string(),
        toolset: "memory".to_string(),
        description: "Manage persistent memory/user profile. action=add|replace|remove; target=memory|user. Session snapshot refreshes on next session or refresh_memory.".to_string(),
        schema: schema_for_args::<MemoryArgs>(),
        check_fn: None,
        icon: "brain",
        ..crate::registry::ToolEntry::lifecycle_defaults().exclusive()
    });

    registry.register(crate::registry::ToolEntry {
        name: "session_search".to_string(),
        toolset: "session_search".to_string(),
        description: "Search historical conversation messages with FTS5 (content, tool names, tool calls). Empty query may return nothing; limit defaults to 5, max 10."
            .to_string(),
        schema: schema_for_args::<SessionSearchArgs>(),
        check_fn: None,
        icon: "file-search",
        ..crate::registry::ToolEntry::lifecycle_defaults().exclusive()
    });
}

/// 将记忆工具调用委托给 `memory::dispatch_memory_tool`。
///
/// 需要可变 `ToolContext` 以访问 `MemoryManager`。
pub fn dispatch(
    ctx: &mut ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    memory::dispatch_memory_tool(ctx.memory, name, args)
}

/// 将 `session_search` 委托给 `session::dispatch_session_tool`。
pub fn dispatch_session_search(
    ctx: &ToolContext<'_>,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    session::dispatch_session_tool(ctx.sessions, "session_search", args)
}

/// 本模块统一入口：`memory` 与 `session_search`。
fn handle(
    ctx: &mut ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    match name {
        "memory" => dispatch(ctx, name, args),
        "session_search" => dispatch_session_search(ctx, args),
        other => anyhow::bail!("未知记忆工具: {other}"),
    }
}

crate::submit_builtin_tool! {
    register: register,
    names: ["memory", "session_search"],
    sync_named: handle,
}
