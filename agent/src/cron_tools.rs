//! Cron / 定时任务工具注册。
//!
//! 向本地 `ToolRegistry` 批量注册 `scheduled` 工具集下的增删查与启停工具，
//! 供 Agent 通过 function calling 管理 `~/.astro/cron/jobs.json` 中的任务。

use crate::tool_registry::{ToolEntry, ToolRegistry};

/// 注册定时任务相关工具：`cron_add`、`cron_list`、`cron_remove`、`cron_enable`、`cron_disable`。
///
/// 均归属 `scheduled` 工具集；实际持久化与调度由 `cron_exec` 与 memory 层完成。
///
/// # 参数
///
/// - `registry`：可变工具注册表，重复调用会覆盖同名条目。
pub fn register_cron_tools(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "cron_add".to_string(),
        toolset: "scheduled".to_string(),
        description: "Create a scheduled task. schedule supports every:5m / every:1h / every:1d \
                      or five-field cron (min hour day month weekday)."
            .to_string(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {
                "cron": {
                    "type": "string",
                    "description": "Schedule expression, e.g. every:30m or 0 9 * * 1"
                },
                "schedule": {
                    "type": "string",
                    "description": "Alias of cron"
                },
                "task": {
                    "type": "string",
                    "description": "What the agent should do when the job fires"
                }
            },
            "required": ["task"]
        }),
        check_fn: None,
        emoji: "⏰",
    });

    registry.register(ToolEntry {
        name: "cron_list".to_string(),
        toolset: "scheduled".to_string(),
        description: "List scheduled cron jobs stored in ~/.astro/cron/jobs.json.".to_string(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {}
        }),
        check_fn: None,
        emoji: "📋",
    });

    registry.register(ToolEntry {
        name: "cron_remove".to_string(),
        toolset: "scheduled".to_string(),
        description: "Remove a scheduled job by id (full or 8-char prefix).".to_string(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" }
            },
            "required": ["id"]
        }),
        check_fn: None,
        emoji: "🗑️",
    });

    registry.register(ToolEntry {
        name: "cron_enable".to_string(),
        toolset: "scheduled".to_string(),
        description: "Enable a scheduled job.".to_string(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" }
            },
            "required": ["id"]
        }),
        check_fn: None,
        emoji: "✅",
    });

    registry.register(ToolEntry {
        name: "cron_disable".to_string(),
        toolset: "scheduled".to_string(),
        description: "Disable a scheduled job without deleting it.".to_string(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {
                "id": { "type": "string" }
            },
            "required": ["id"]
        }),
        check_fn: None,
        emoji: "⏸️",
    });
}
