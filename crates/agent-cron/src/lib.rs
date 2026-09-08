//! 定时任务：定义、调度计算、JSON 持久化与运行记录。
//!
//! - [`jobs`]：`~/.astro/automation/cron/jobs.json`、到期 tick、工具分发
//! - [`run_db`]：`~/.astro/automation/cron/cron.db` 执行历史
//!
//! 实际触发执行在 `agent::exec::cron` / `server::cron_runner`。

pub mod jobs;
pub mod run_db;

pub use jobs::*;
pub use run_db::{cron_db_path, CronRunDb, CronRunFilters, CronRunRow, NewCronRun};
