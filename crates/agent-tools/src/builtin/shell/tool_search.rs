//! 工具搜索：从当前 Step 的完整注册表中搜索 deferred 工具。

use bm25::{Document, Language, SearchEngineBuilder};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry, ToolRegistryView};
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
    registered_name: String,
    tool_name: types::ToolName,
    toolset: String,
    description: String,
    parameters: serde_json::Value,
}

impl ToolSearchEntry {
    /// `tool_search` 返回的 Responses API 原生可加载 schema。
    fn loadable_spec(&self) -> serde_json::Value {
        let function = serde_json::json!({
            "type": "function",
            "name": self.tool_name.name(),
            "description": self.description,
            "strict": false,
            "defer_loading": true,
            "parameters": self.parameters,
        });
        match self.tool_name.namespace() {
            Some(namespace) => serde_json::json!({
                "type": "namespace",
                "name": namespace,
                "description": format!("Tools in the {namespace} namespace."),
                "tools": [function],
            }),
            None => function,
        }
    }
}

fn searchable_entries(entries: Vec<ToolEntry>) -> Vec<ToolSearchEntry> {
    let mut entries: Vec<ToolSearchEntry> = entries
        .into_iter()
        .map(|entry| {
            let tool_name = entry.tool_name();
            ToolSearchEntry {
                registered_name: entry.name,
                tool_name,
                toolset: entry.toolset,
                description: entry.description,
                parameters: crate::schema::sanitize_tool_schema(entry.schema),
            }
        })
        .collect();
    entries.sort_by(|a, b| a.registered_name.cmp(&b.registered_name));
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
                entry.registered_name,
                entry.tool_name.wire_name(),
                entry.toolset,
                entry.description
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
            (entry.registered_name.clone(), entry.loadable_spec())
        })
        .collect()
}

fn coalesce_loadable_specs(matches: Vec<(String, serde_json::Value)>) -> Vec<serde_json::Value> {
    let mut specs: Vec<serde_json::Value> = Vec::new();
    for (_, mut spec) in matches {
        if spec.get("type").and_then(serde_json::Value::as_str) == Some("namespace") {
            let namespace = spec.get("name").and_then(serde_json::Value::as_str);
            if let Some(existing) = specs.iter_mut().find(|existing| {
                existing.get("type").and_then(serde_json::Value::as_str) == Some("namespace")
                    && existing.get("name").and_then(serde_json::Value::as_str) == namespace
            }) {
                if let (Some(existing_tools), Some(tools)) = (
                    existing
                        .get_mut("tools")
                        .and_then(serde_json::Value::as_array_mut),
                    spec.get_mut("tools")
                        .and_then(serde_json::Value::as_array_mut),
                ) {
                    existing_tools.append(tools);
                }
                continue;
            }
        }
        specs.push(spec);
    }
    specs
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
        let matches = search(
            &searchable_entries(registry.searchable_deferred_entries()?),
            query,
            limit,
        );
        let specs = coalesce_loadable_specs(matches);
        return Ok(serde_json::to_string_pretty(&specs)?);
    }

    // 独立工具测试/调用没有 Session 注册表时，仍以全量内置工具构建一次性索引。
    let mut registry = ToolRegistry::new();
    crate::register_all(&mut registry);
    let specs = coalesce_loadable_specs(search(
        &searchable_entries(registry.searchable_deferred_entries()?),
        query,
        limit,
    ));
    Ok(serde_json::to_string_pretty(&specs)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searchable_entries_include_full_deferred_schema() {
        let mut registry = ToolRegistry::new();
        crate::register_all(&mut registry);
        let entries = searchable_entries(
            registry
                .searchable_deferred_entries()
                .expect("deferred tool view"),
        );

        let web_search = entries
            .iter()
            .find(|entry| entry.registered_name == "web_search")
            .expect("web_search");
        assert_eq!(web_search.toolset, "web_search");
        assert!(web_search.parameters["properties"]["query"].is_object());
        assert!(entries
            .iter()
            .any(|entry| entry.registered_name == "web_fetch"));
    }

    #[test]
    fn search_ranks_deferred_web_tools_and_returns_loadable_specs() {
        let mut registry = ToolRegistry::new();
        crate::register_all(&mut registry);
        let entries = searchable_entries(
            registry
                .searchable_deferred_entries()
                .expect("deferred tool view"),
        );
        let matches = search(&entries, "search the web", 5);
        let (_, spec) = matches
            .iter()
            .find(|(name, _)| name == "web_search")
            .expect("web_search match");
        assert_eq!(spec["type"], "function");
        assert_eq!(spec["defer_loading"], true);
        assert!(spec["parameters"]["properties"]["query"].is_object());
    }

    #[test]
    fn coalesces_tools_from_the_same_native_namespace() {
        let spec = |name: &str| {
            serde_json::json!({
                "type": "namespace",
                "name": "mcp__calendar",
                "description": "Calendar tools",
                "tools": [{"type": "function", "name": name}],
            })
        };
        let specs = coalesce_loadable_specs(vec![
            ("internal-list".into(), spec("list")),
            ("internal-create".into(), spec("create")),
        ]);

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0]["tools"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn media_generation_search_entries_share_one_namespace() {
        let mut registry = ToolRegistry::new();
        crate::register_all(&mut registry);
        let matches = searchable_entries(
            registry
                .searchable_deferred_entries()
                .expect("deferred tool view"),
        )
        .into_iter()
        .filter(|entry| entry.tool_name.namespace() == Some("media"))
        .map(|entry| (entry.registered_name.clone(), entry.loadable_spec()))
        .collect();
        let specs = coalesce_loadable_specs(matches);

        assert_eq!(specs.len(), 1);
        let names = specs[0]["tools"]
            .as_array()
            .expect("media namespace tools")
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            names,
            std::collections::HashSet::from(["image_gen", "video_gen", "speech_gen", "music_gen"])
        );
    }
}
