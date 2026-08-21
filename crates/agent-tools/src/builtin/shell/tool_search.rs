//! 工具搜索：BM25 索引 + 缓存（对齐 Codex ToolSearchHandler）。

use std::sync::Mutex;

use bm25::{Document, Language, SearchEngineBuilder};
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
    /// Search query: matched against tool name and description using BM25 ranking.
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
            "Search available tools by keyword. Returns matching tool names and descriptions, ranked by relevance (BM25). Use this to discover specialized tools not in the default tool list."
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

// ── BM25 缓存 ──────────────────────────────────────────────

struct ToolSearchEntry {
    name: String,
    description: String,
}

struct CachedIndex {
    /// 缓存的 BM25 引擎（类型擦除为搜索回调，避免泛型逃逸）。
    engine: bm25::SearchEngine<usize>,
    /// 建索引时的工具数量，用于失效判断。
    tool_count: usize,
    /// 与索引对齐的工具元数据。
    entries: Vec<ToolSearchEntry>,
}

static CACHE: Mutex<Option<CachedIndex>> = Mutex::new(None);

fn build_index(catalog: &[crate::catalog::ToolCatalogItem]) -> CachedIndex {
    let entries: Vec<ToolSearchEntry> = catalog
        .iter()
        .map(|t| ToolSearchEntry {
            name: t.name.clone(),
            description: t.description.clone(),
        })
        .collect();

    let documents: Vec<Document<usize>> = entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| {
            // 工具名重复一次以提升名称匹配权重
            let text = format!("{} {} {}", entry.name, entry.name, entry.description);
            Document::new(idx, text)
        })
        .collect();

    let engine = SearchEngineBuilder::<usize>::with_documents(Language::English, documents).build();

    CachedIndex {
        engine,
        tool_count: catalog.len(),
        entries,
    }
}

/// 在内置工具目录中按关键字搜索，使用 BM25 索引，返回按相关性排序的匹配结果。
pub async fn dispatch(_ctx: &ToolContext<'_>, args: &ToolSearchArgs) -> anyhow::Result<String> {
    let query = args.query.trim();
    if query.is_empty() {
        anyhow::bail!("tool_search requires a non-empty query");
    }

    let catalog = crate::catalog::builtin_catalog();
    let limit = args.limit.min(50);

    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() || guard.as_ref().unwrap().tool_count != catalog.len() {
        *guard = Some(build_index(&catalog));
    }
    let cached = guard.as_ref().unwrap();

    let results = cached.engine.search(query, limit);

    let matches: Vec<serde_json::Value> = results
        .into_iter()
        .map(|r| {
            let entry = &cached.entries[r.document.id];
            serde_json::json!({
                "name": entry.name,
                "description": entry.description,
                "relevance": (r.score as f64 * 100.0).round() / 100.0,
            })
        })
        .collect();

    Ok(serde_json::to_string_pretty(&matches)?)
}
