//! 记忆工具：持久化记忆读写与会话全文检索。
//!
//! 将 `memory_add` / `memory_replace` / `memory_remove` / `session_search`
//! 注册到 `memory` 与 `session_search` toolset，实际逻辑委托给 `memory` crate。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;

/// 记忆写入目标：`project` → MEMORY.md；`user` → USER.md；`daily` → 当日日志。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MemoryTarget {
    /// 项目级长期记忆（`MEMORY.md`）。
    Project,
    /// 用户偏好记忆（`USER.md`）。
    User,
    /// 当日 mermaid 日志（`YYYY-MM-DD.md`）。
    Daily,
}

impl Default for MemoryTarget {
    /// 默认写入项目级 `MEMORY.md`。
    fn default() -> Self {
        Self::Project
    }
}

/// `memory_add` 工具的参数结构。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MemoryAddArgs {
    /// 要追加的简洁记忆条目。
    pub entry: String,
    /// 写入目标文件，默认 `project`。
    #[serde(default)]
    pub target: MemoryTarget,
}

/// `memory_replace` 工具的参数结构。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MemoryReplaceArgs {
    /// 要被替换的原文本片段。
    pub old_text: String,
    /// 替换后的新文本。
    pub new_text: String,
    /// 操作目标文件，默认 `project`。
    #[serde(default)]
    pub target: MemoryTarget,
}

/// `memory_remove` 工具的参数结构。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MemoryRemoveArgs {
    /// 要删除的记忆文本片段。
    pub text: String,
    /// 操作目标文件，默认 `project`。
    #[serde(default)]
    pub target: MemoryTarget,
}

/// `session_search` 工具的参数结构。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SessionSearchArgs {
    /// FTS5 全文检索关键词。
    pub query: String,
    /// 返回条数上限，默认 5，最大 10（与数据库 LIMIT 一致）。
    #[serde(default)]
    pub limit: Option<u32>,
}

/// 向注册表注册全部记忆相关工具（4 个函数）。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "memory_add".to_string(),
        toolset: "memory".to_string(),
        description: "Add a persistent memory entry. Use target=project for long-term MEMORY.md, user for USER.md, daily for today's mermaid/YYYY-MM-DD.md.".to_string(),
        schema: schema_for_args::<MemoryAddArgs>(),
        check_fn: None,
        icon: "brain",
    });

    registry.register(crate::registry::ToolEntry {
        name: "memory_replace".to_string(),
        toolset: "memory".to_string(),
        description: "Replace an outdated memory entry (project/user/daily).".to_string(),
        schema: schema_for_args::<MemoryReplaceArgs>(),
        check_fn: None,
        icon: "pen-line",
    });

    registry.register(crate::registry::ToolEntry {
        name: "memory_remove".to_string(),
        toolset: "memory".to_string(),
        description: "Remove a memory entry that is no longer relevant (project/user/daily).".to_string(),
        schema: schema_for_args::<MemoryRemoveArgs>(),
        check_fn: None,
        icon: "trash-2",
    });

    registry.register(crate::registry::ToolEntry {
        name: "session_search".to_string(),
        toolset: "session_search".to_string(),
        description: "Search past conversation sessions with FTS5. Empty query lists recent sessions (summaries). limit defaults to 5, max 10."
            .to_string(),
        schema: schema_for_args::<SessionSearchArgs>(),
        check_fn: None,
        icon: "file-search",
    });
}

/// 将记忆工具调用委托给 `memory::dispatch_memory_tool`。
///
/// 需要可变 `ToolContext` 以访问 `MemoryManager`。
pub fn dispatch(ctx: &mut ToolContext<'_>, name: &str, args: &serde_json::Value) -> anyhow::Result<String> {
    memory::dispatch_memory_tool(ctx.memory, name, args)
}
