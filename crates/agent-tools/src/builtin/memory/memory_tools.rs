//! 记忆工具：单一 `memory` 写操作（add / replace / remove）。
//!
//! 会话历史检索请用 `search`（scope=session|…），见 `context_tools`。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;

/// `memory` 工具操作类型。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MemoryAction {
    /// 追加一条精炼的记忆条目。
    Add,
    /// 按唯一子串匹配替换一条条目。
    Replace,
    /// 按唯一子串匹配移除一条条目。
    Remove,
}

/// 记忆写入目标：`memory` → MEMORY.md；`user` → USER.md。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum MemoryTarget {
    /// 长期精炼记忆（`MEMORY.md`）。
    #[default]
    Memory,
    /// 用户画像（`USER.md`）。
    User,
}

/// `memory` 工具的参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MemoryArgs {
    /// 操作：`add` / `replace` / `remove`。
    pub action: MemoryAction,
    /// 写入目标；默认 `memory`。
    #[serde(default)]
    pub target: MemoryTarget,
    /// add / replace 的新内容。
    #[serde(default)]
    pub content: Option<String>,
    /// 用于定位 replace / remove 条目的子串。
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
        ..crate::registry::ToolEntry::lifecycle_defaults().exclusive()
    });
}

/// 将记忆工具调用委托给 `memory::dispatch_memory_tool`。
pub fn dispatch(
    ctx: &mut ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    let mut memory = ctx.memory_mut();
    memory::dispatch_memory_tool(&mut memory, name, args)
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
