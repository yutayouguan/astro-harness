//! 定时任务工具集：注册 `cron_*` / `scheduled` 别名，并转发到 memory crate。
//!
//! 实际持久化与调度逻辑在 [`cron::dispatch_cron_tool`]；本模块只负责 schema 与注册。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// 创建定时任务的参数（`cron_add` / `scheduled` 共用）。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct CronAddArgs {
    /// 调度表达式，如 `every:30m` 或五段 cron。
    #[serde(default)]
    pub cron: Option<String>,
    /// `cron` 字段的别名。
    #[serde(default)]
    pub schedule: Option<String>,
    /// 任务触发时 Agent 应执行的内容。
    pub task: String,
}

/// 无参数工具（如 `cron_list`）的空 schema。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct EmptyArgs {}

/// 按 id 操作任务的参数（启用 / 禁用 / 删除）。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct CronIdArgs {
    /// 完整 id 或其 8 字符前缀。
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
