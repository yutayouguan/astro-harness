//! 工具搜索：从当前 Step 的完整注册表中搜索 deferred 工具。

use bm25::{Document, Language, SearchEngineBuilder};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

fn default_limit() -> usize {
    10
}

/// `tool_search` 工具的参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ToolSearchArgs {
    /// 搜索查询：使用 BM25 排名匹配工具名称和描述。
    pub query: String,
    /// 返回的最大结果数（默认 10）。
    #[serde(default = "default_limit")]
    pub limit: usize,
}

/// 向注册表注册 `tool_search` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "tool_search".to_string(),
        toolset: "system".to_string(),
        description:
            "Search deferred tools by keyword. Returns complete loadable tool definitions ranked by relevance (BM25). Use this to discover specialized built-in and MCP tools that are not in the default tool list."
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

struct ToolSearchEntry {
    name: String,
    toolset: String,
    description: String,
    parameters: serde_json::Value,
}

impl ToolSearchEntry {
    /// `tool_search` 返回的 Codex 兼容可加载函数 schema。
    fn loadable_spec(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "function",
            "name": self.name,
            "description": self.description,
            "strict": false,
            "defer_loading": true,
            "parameters": self.parameters,
        })
    }
}

fn searchable_entries(registry: &ToolRegistry) -> Vec<ToolSearchEntry> {
    let mut entries: Vec<ToolSearchEntry> = registry
        .searchable_deferred_tools()
        .into_iter()
        .map(|entry| ToolSearchEntry {
            name: entry.name.clone(),
            toolset: entry.toolset.clone(),
            description: entry.description.clone(),
            parameters: crate::schema::sanitize_tool_schema(entry.schema.clone()),
        })
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

fn search(
    entries: &[ToolSearchEntry],
    query: &str,
    limit: usize,
) -> Vec<(String, serde_json::Value)> {
    if entries.is_empty() || limit == 0 {
        return Vec::new();
    }
    let documents: Vec<Document<usize>> = entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| {
            // 工具名重复一次以提升名称匹配权重。
            let text = format!(
                "{} {} {} {}",
                entry.name, entry.name, entry.toolset, entry.description
            );
            Document::new(idx, text)
        })
        .collect();
    let engine = SearchEngineBuilder::<usize>::with_documents(Language::English, documents).build();

    engine
        .search(query, limit)
        .into_iter()
        .map(|result| {
            let entry = &entries[result.document.id];
            (entry.name.clone(), entry.loadable_spec())
        })
        .collect()
}

/// 搜索当前 Step 的 deferred 工具并返回完整可加载 schema。
///
/// 结果会由 Responses 适配器序列化为 `tool_search_output`；搜索本身不改写
/// 注册表，下一 Step 只把该输出中实际返回的工具加入可调用路由。
pub async fn dispatch(ctx: &ToolContext<'_>, args: &ToolSearchArgs) -> anyhow::Result<String> {
    let query = args.query.trim();
    if query.is_empty() {
        anyhow::bail!("tool_search requires a non-empty query");
    }

    let limit = args.limit.min(50);
    if let Some(registry) = ctx.tool_registry {
        let registry = registry
            .read()
            .map_err(|_| anyhow::anyhow!("tool registry lock poisoned"))?;
        let matches = search(&searchable_entries(&registry), query, limit);
        let specs: Vec<_> = matches.into_iter().map(|(_, spec)| spec).collect();
        return Ok(serde_json::to_string_pretty(&specs)?);
    }

    // 独立工具测试/调用没有 Session 注册表时，仍以全量内置工具构建一次性索引。
    let mut registry = ToolRegistry::new();
    crate::register_all(&mut registry);
    let specs: Vec<_> = search(&searchable_entries(&registry), query, limit)
        .into_iter()
        .map(|(_, spec)| spec)
        .collect();
    Ok(serde_json::to_string_pretty(&specs)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searchable_entries_include_full_deferred_schema() {
        let mut registry = ToolRegistry::new();
        crate::register_all(&mut registry);
        let entries = searchable_entries(&registry);

        let web_search = entries
            .iter()
            .find(|entry| entry.name == "web_search")
            .expect("web_search");
        assert_eq!(web_search.toolset, "web_search");
        assert!(web_search.parameters["properties"]["query"].is_object());
        assert!(entries.iter().any(|entry| entry.name == "web_fetch"));
    }

    #[test]
    fn search_ranks_deferred_web_tools_and_returns_loadable_specs() {
        let mut registry = ToolRegistry::new();
        crate::register_all(&mut registry);
        let entries = searchable_entries(&registry);
        let matches = search(&entries, "search the web", 5);
        let (_, spec) = matches
            .iter()
            .find(|(name, _)| name == "web_search")
            .expect("web_search match");
        assert_eq!(spec["type"], "function");
        assert_eq!(spec["defer_loading"], true);
        assert!(spec["parameters"]["properties"]["query"].is_object());
    }
}
