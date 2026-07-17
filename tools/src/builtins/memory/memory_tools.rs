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

/// `memory` 工具动作。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MemoryAction {
    /// 追加一条精炼记忆。
    Add,
    /// 按子串唯一匹配替换条目。
    Replace,
    /// 按子串唯一匹配删除条目。
    Remove,
}

/// 记忆写入目标：`memory` → MEMORY.md；`user` → USER.md。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MemoryTarget {
    /// 长期精炼记忆（`MEMORY.md`）。
    Memory,
    /// 用户档案（`USER.md`）。
    User,
}

impl Default for MemoryTarget {
    fn default() -> Self {
        Self::Memory
    }
}

/// 单一 `memory` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MemoryArgs {
    /// 操作类型：`add` / `replace` / `remove`。
    pub action: MemoryAction,
    /// 写入目标，默认 `memory`。
    #[serde(default)]
    pub target: MemoryTarget,
    /// add / replace 的新内容。
    #[serde(default)]
    pub content: Option<String>,
    /// replace / remove 用于定位条目的子串。
    #[serde(default)]
    pub old_text: Option<String>,
}

/// `session_search` 工具的参数结构。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SessionSearchArgs {
    /// FTS5 全文检索关键词（匹配历史消息正文 / 工具名等）。
    pub query: String,
    /// 返回条数上限，默认 5，最大 10（与数据库 LIMIT 一致）。
    #[serde(default)]
    pub limit: Option<u32>,
}

/// 向注册表注册记忆相关工具（`memory` + `session_search`）。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "memory".to_string(),
        toolset: "memory".to_string(),
        description: "Manage persistent memory. action=add|replace|remove; target=memory (MEMORY.md) or user (USER.md). Use content for add/replace and old_text substring for replace/remove. Writes update live/disk; session prompt snapshot is not refreshed until next session or refresh_memory.".to_string(),
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

crate::submit_builtin_tool!(register);

/// 将记忆工具调用委托给 `memory::dispatch_memory_tool`。
///
/// 需要可变 `ToolContext` 以访问 `MemoryManager`。
pub fn dispatch(ctx: &mut ToolContext<'_>, name: &str, args: &serde_json::Value) -> anyhow::Result<String> {
    memory::dispatch_memory_tool(ctx.memory, name, args)
}

/// 将 `session_search` 委托给 `session::dispatch_session_tool`。
pub fn dispatch_session_search(
    ctx: &ToolContext<'_>,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    session::dispatch_session_tool(ctx.sessions, "session_search", args)
}
