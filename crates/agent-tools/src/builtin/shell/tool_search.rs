//! 工具搜索：按关键字在内置工具目录中检索匹配的工具名和描述。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

fn default_limit() -> usize {
    10
}

/// Arguments for the `tool_search` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ToolSearchArgs {
    /// Search query: matched against tool name and description (case-insensitive substring).
    pub query: String,
    /// Maximum number of results to return (default 10).
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// 向注册表注册 `tool_search` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "tool_search".to_string(),
        toolset: "system".to_string(),
        description:
            "Search available tools by keyword. Returns matching tool names and descriptions."
                .to_string(),
        schema: schema_for_args::<ToolSearchArgs>(),
        check_fn: None,
        icon: "search",
        ..ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["tool_search"],
    async_ctx: dispatch,
    args: ToolSearchArgs,
}

/// 在内置工具目录中按关键字搜索，返回匹配的工具名和描述。
pub async fn dispatch(_ctx: &ToolContext<'_>, args: &ToolSearchArgs) -> anyhow::Result<String> {
    let q = args.query.to_lowercase();
    if q.is_empty() {
        anyhow::bail!("tool_search requires a non-empty query");
    }
    let catalog = crate::catalog::builtin_catalog();
    let matches: Vec<_> = catalog
        .iter()
        .filter(|t| t.name.to_lowercase().contains(&q) || t.description.to_lowercase().contains(&q))
        .take(args.limit)
        .map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
            })
        })
        .collect();
    Ok(serde_json::to_string_pretty(&matches)?)
}
