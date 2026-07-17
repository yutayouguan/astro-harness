//! 默认目录 tick 入口。

use super::model::CronJob;
use super::store::CronStore;

/// 对默认 cron 目录执行一次 tick（供 backend 调度器调用）
pub fn tick_default() -> anyhow::Result<Vec<CronJob>> {
    CronStore::open_default()?.tick()
}
