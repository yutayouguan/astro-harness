//! 定时任务工具集：注册 `cron_*` / `scheduled` 别名，并转发到 memory crate。
//!
//! 实际持久化与调度逻辑在 [`cron::dispatch_cron_tool`]；本模块只负责 schema 与注册。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// Args to create a scheduled job (`cron_add` / `scheduled`).
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct CronAddArgs {
    /// Schedule expression, e.g. `every:30m` or five-field cron.
    #[serde(default)]
    pub cron: Option<String>,
    /// Alias of the `cron` field.
    #[serde(default)]
    pub schedule: Option<String>,
    /// What the agent should do when the job fires.
    pub task: String,
}

/// Empty schema for tools with no parameters (e.g. `cron_list`).
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct EmptyArgs {}

/// Args for id-based job ops (enable / disable / remove).
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct CronIdArgs {
    /// Full id or its 8-character prefix.
    pub id: String,
}

/// 注册 `cron_add` / `cron_list` / `cron_remove` / `cron_enable` / `cron_disable` 及 `scheduled` 别名。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "cron_add".to_string(),
        toolset: "scheduled".to_string(),
        description: "Create a scheduled task. schedule supports every:5m / every:1h / every:1d \
                      or five-field cron (min hour day month weekday)."
            .to_string(),
        schema: schema_for_args::<CronAddArgs>(),
        check_fn: None,
        icon: "calendar-check",
        ..ToolEntry::lifecycle_defaults()
    });

    registry.register(ToolEntry {
        name: "cron_list".to_string(),
        toolset: "scheduled".to_string(),
        description: "List scheduled cron jobs stored in ~/.astro/cron/jobs.json.".to_string(),
        schema: schema_for_args::<EmptyArgs>(),
        check_fn: None,
        icon: "clipboard-list",
        ..ToolEntry::lifecycle_defaults()
    });

    registry.register(ToolEntry {
        name: "cron_remove".to_string(),
        toolset: "scheduled".to_string(),
        description: "Remove a scheduled job by id (full or 8-char prefix).".to_string(),
        schema: schema_for_args::<CronIdArgs>(),
        check_fn: None,
        icon: "trash-2",
        ..ToolEntry::lifecycle_defaults()
    });

    registry.register(ToolEntry {
        name: "cron_enable".to_string(),
        toolset: "scheduled".to_string(),
        description: "Enable a scheduled job.".to_string(),
        schema: schema_for_args::<CronIdArgs>(),
        check_fn: None,
        icon: "check-circle-2",
        ..ToolEntry::lifecycle_defaults()
    });

    registry.register(ToolEntry {
        name: "cron_disable".to_string(),
        toolset: "scheduled".to_string(),
        description: "Disable a scheduled job without deleting it.".to_string(),
        schema: schema_for_args::<CronIdArgs>(),
        check_fn: None,
        icon: "pause",
        ..ToolEntry::lifecycle_defaults()
    });

    // 面板 id 别名
    registry.register(ToolEntry {
        name: "scheduled".to_string(),
        toolset: "scheduled".to_string(),
        description: "Alias of cron_add: create a scheduled task.".to_string(),
        schema: schema_for_args::<CronAddArgs>(),
        check_fn: None,
        icon: "calendar-check",
        ..ToolEntry::lifecycle_defaults()
    });
}

/// 将工具名与参数转交给 [`cron::dispatch_cron_tool`]。
///
/// # 参数
/// - `name`：工具名（含 `scheduled` 别名）
/// - `args`：JSON 参数对象
pub fn dispatch(name: &str, args: &serde_json::Value) -> anyhow::Result<String> {
    cron::dispatch_cron_tool(name, args)
}

fn handle(
    _ctx: &mut crate::context::ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    dispatch(name, args)
}

crate::submit_builtin_tool! {
    register: register,
    names: ["cron_add", "cron_list", "cron_remove", "cron_enable", "cron_disable", "scheduled"],
    sync_named: handle,
}
