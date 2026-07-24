//! 定时任务工具：单一 `cron`（action 分发），转发到 [`cron::dispatch_cron_tool`]。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `cron` 工具动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum CronAction {
    /// 创建任务。
    Add,
    /// 列出全部任务。
    List,
    /// 按 id 删除。
    Remove,
    /// 启用。
    Enable,
    /// 禁用（保留定义）。
    Disable,
}

/// 单一 `cron` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct CronArgs {
    /// `add` | `list` | `remove` | `enable` | `disable`。
    pub action: CronAction,
    /// 调度表达式（`add`）：`every:30m` / `every:1h` / 五段 cron；亦接受别名字段 `cron`。
    #[serde(default)]
    pub schedule: Option<String>,
    /// `schedule` 的别名（仅 `add`）。
    #[serde(default)]
    pub cron: Option<String>,
    /// 触发时 Agent 执行的任务文案（`add`）。
    #[serde(default)]
    pub task: Option<String>,
    /// 任务 id 或 8 字符前缀（`remove` / `enable` / `disable`）。
    #[serde(default)]
    pub id: Option<String>,
}

/// 注册统一 `cron` 工具（toolset = `cron`）。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "cron".to_string(),
        toolset: "cron".to_string(),
        description: "Manage scheduled jobs. action=add|list|remove|enable|disable. \
                      add: schedule (or cron) + task — every:5m / every:1h / every:1d or five-field cron. \
                      list: no extra args. remove/enable/disable: id (full or 8-char prefix). \
                      Jobs live in ~/.astro/cron/jobs.json."
            .to_string(),
        schema: schema_for_args::<CronArgs>(),
        check_fn: None,
        icon: "calendar-check",
        ..ToolEntry::lifecycle_defaults()
    });
}

/// 将参数转交给 [`cron::dispatch_cron_tool`]。
pub fn dispatch(_name: &str, args: &serde_json::Value) -> anyhow::Result<String> {
    cron::dispatch_cron_tool(args)
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
    names: ["cron"],
    sync_named: handle,
}
