//! Agent 工具调用的 JSONL 审计日志（轻量落盘，失败可忽略）。
//!
//! 每次工具发起调用时追加一行 JSON 到 `~/.astro/agents/{id}/tool-calls.jsonl`，
//! 记录时间戳、工具名与参数。与 [`usage_stats`](crate::usage_stats) 的聚合计数独立，
//! 侧重保留原始调用轨迹。

use std::fs;
use std::path::PathBuf;

use crate::workspace::{default_memory_dir, ensure_default_workspace};

/// 返回指定 Agent 的工具调用日志文件路径。
///
/// 路径为 `{memory_dir}/agents/{agent_id}/tool-calls.jsonl`。
fn tool_calls_path(agent_id: &str) -> PathBuf {
    default_memory_dir()
        .join("agents")
        .join(agent_id)
        .join("tool-calls.jsonl")
}

/// 追加记录一次工具发起调用（时间戳、名称、参数）。
///
/// 写入前确保默认工作区存在；以 append 模式打开 JSONL 文件。
/// 调用方通常可忽略错误，不影响主流程。
pub fn record_tool_call(agent_id: &str, name: &str, args: &serde_json::Value) -> anyhow::Result<()> {
    let _ = ensure_default_workspace()?;
    let path = tool_calls_path(agent_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let line = serde_json::json!({
        "ts": chrono::Local::now().to_rfc3339(),
        "name": name,
        "args": args,
    });
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{line}")?;
    Ok(())
}
