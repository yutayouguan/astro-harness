//! 定时任务工具（Namespace 模式）：`cron.add`、`cron.list`、`cron.remove`、`cron.enable`、`cron.disable`。
//!
//! 每个操作有独立 schema，不再共用 `action` 枚举。底层仍转发到 [`cron::dispatch_cron_tool`]。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `cron.add` 参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct CronAddArgs {
    /// 调度表达式：`every:30m` / `every:1h` / `every:1d` 或五段 cron。
    pub schedule: String,
    /// 触发时 Agent 执行的任务文案。
    pub task: String,
}

/// `cron.list` 无额外参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct CronListArgs {}

/// `cron.remove` / `cron.enable` / `cron.disable` 参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct CronIdArgs {
    /// 任务 id 或 8 字符前缀。
    pub id: String,
}

const CRON_NAMESPACE: &str = "cron";

/// 注册 5 个 cron 命名空间工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "cron.add".to_string(),
        toolset: "cron".to_string(),
        namespace: CRON_NAMESPACE.to_string(),
        description: "Create a scheduled job. schedule: every:5m / every:1h / every:1d or five-field cron. task: what the agent should do when triggered.".to_string(),
        schema: schema_for_args::<CronAddArgs>(),
        icon: "calendar-plus",
        ..ToolEntry::lifecycle_defaults()
    });
    registry.register(ToolEntry {
        name: "cron.list".to_string(),
        toolset: "cron".to_string(),
        namespace: CRON_NAMESPACE.to_string(),
        description: "List all scheduled jobs with their status and next run time.".to_string(),
        schema: schema_for_args::<CronListArgs>(),
        icon: "calendar-check",
        ..ToolEntry::lifecycle_defaults()
    });
    registry.register(ToolEntry {
        name: "cron.remove".to_string(),
        toolset: "cron".to_string(),
        namespace: CRON_NAMESPACE.to_string(),
        description: "Remove a scheduled job by id (full or 8-char prefix).".to_string(),
        schema: schema_for_args::<CronIdArgs>(),
        icon: "calendar-x",
        ..ToolEntry::lifecycle_defaults()
    });
    registry.register(ToolEntry {
        name: "cron.enable".to_string(),
        toolset: "cron".to_string(),
        namespace: CRON_NAMESPACE.to_string(),
        description: "Enable a paused scheduled job by id.".to_string(),
        schema: schema_for_args::<CronIdArgs>(),
        icon: "calendar-check",
        ..ToolEntry::lifecycle_defaults()
    });
    registry.register(ToolEntry {
        name: "cron.disable".to_string(),
        toolset: "cron".to_string(),
        namespace: CRON_NAMESPACE.to_string(),
        description: "Disable a scheduled job (keep definition) by id.".to_string(),
        schema: schema_for_args::<CronIdArgs>(),
        icon: "calendar-off",
        ..ToolEntry::lifecycle_defaults()
    });
}

fn handle(
    _ctx: &mut crate::context::ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    let action = match name {
        "cron.add" => "add",
        "cron.list" => "list",
        "cron.remove" => "remove",
        "cron.enable" => "enable",
        "cron.disable" => "disable",
        _ => anyhow::bail!("unknown cron action: {name}"),
    };
    let mut patched = args.clone();
    if let Some(obj) = patched.as_object_mut() {
        obj.insert("action".to_string(), serde_json::json!(action));
    }
    cron::dispatch_cron_tool(&patched)
}

crate::submit_builtin_tool! {
    register: register,
    names: ["cron.add", "cron.list", "cron.remove", "cron.enable", "cron.disable"],
    sync_named: handle,
}
