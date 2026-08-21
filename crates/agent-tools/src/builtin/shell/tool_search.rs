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

/// 在内置工具目录中按关键字搜索，使用 BM25 风格评分，返回按相关性排序的匹配结果。
pub async fn dispatch(_ctx: &ToolContext<'_>, args: &ToolSearchArgs) -> anyhow::Result<String> {
    let q = args.query.to_lowercase();
    let terms: Vec<&str> = q.split_whitespace().collect();
    if terms.is_empty() {
        anyhow::bail!("tool_search requires a non-empty query");
    }

    let catalog = crate::catalog::builtin_catalog();
    let mut scored: Vec<_> = catalog
        .iter()
        .map(|t| {
            let name_lower = t.name.to_lowercase();
            let desc_lower = t.description.to_lowercase();
            let text = format!("{} {}", name_lower, desc_lower);
            // BM25-inspired scoring: term frequency + name bonus
            let mut score: f64 = 0.0;
            for term in &terms {
                let tf = text.matches(term).count() as f64;
                if tf > 0.0 {
                    // BM25: tf / (tf + k1) where k1 = 1.2
                    score += tf / (tf + 1.2);
                }
                // Name exact match bonus
                if name_lower.contains(term) {
                    score += 2.0;
                }
            }
            (t, score)
        })
        .filter(|(_, score)| *score > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let matches: Vec<_> = scored
        .into_iter()
        .take(args.limit)
        .map(|(t, score)| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "relevance": (score * 100.0).round() / 100.0,
            })
        })
        .collect();

    Ok(serde_json::to_string_pretty(&matches)?)
}
