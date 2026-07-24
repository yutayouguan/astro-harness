//! 导出内置工具 OpenAI 风格 schema，便于核对上下文「工具定义」占用。
//!
//! ```bash
//! cargo run -p tools --bin dump_tool_schemas
//! cargo run -p tools --bin dump_tool_schemas -- /tmp/tool-schemas.json
//! ```

use std::env;
use std::path::PathBuf;

use tools::{register_all, sanitize_tool_schema, ToolRegistry};

fn estimate_tokens(chars: usize) -> u32 {
    u32::try_from(chars.div_ceil(4)).unwrap_or(u32::MAX)
}

fn main() -> anyhow::Result<()> {
    let out = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tool-schemas.json"));

    let mut reg = ToolRegistry::new();
    register_all(&mut reg);

    let mut tools: Vec<serde_json::Value> = Vec::new();
    for e in reg.all_tools() {
        let api = serde_json::json!({
            "type": "function",
            "function": {
                "name": e.name,
                "description": e.description,
                "parameters": sanitize_tool_schema(e.schema.clone()),
            }
        });
        let chars = api.to_string().len();
        tools.push(serde_json::json!({
            "name": e.name,
            "toolset": e.toolset,
            "chars": chars,
            "est_tokens": estimate_tokens(chars),
            "schema": api,
        }));
    }

    tools.sort_by(|a, b| {
        let ta = b.get("est_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        let tb = a.get("est_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        ta.cmp(&tb).then_with(|| {
            let na = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let nb = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
            na.cmp(nb)
        })
    });

    let total_chars: usize = tools
        .iter()
        .filter_map(|t| t.get("chars").and_then(|c| c.as_u64()))
        .map(|c| c as usize)
        .sum();
    let total_tokens = estimate_tokens(total_chars);

    let doc = serde_json::json!({
        "note": "内置工具 schema 导出（按 est_tokens 降序）。est_tokens = ceil(chars/4)，与上下文用量「工具定义」算法一致。不含 MCP。",
        "count": tools.len(),
        "total_chars": total_chars,
        "total_est_tokens": total_tokens,
        "tools": tools,
    });

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&out, serde_json::to_string_pretty(&doc)?)?;
    eprintln!(
        "wrote {} tools → {} ({} chars, ~{} tokens)",
        tools.len(),
        out.display(),
        total_chars,
        total_tokens
    );
    Ok(())
}
