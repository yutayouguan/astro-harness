//! 用量事件库、统计与定价估算。

pub mod db;
pub mod pricing;
pub mod stats;
pub mod trace_insights;

#[cfg(test)]
pub(crate) mod test_env {
    use std::sync::{Mutex, MutexGuard};

    /// 跨 usage 子模块串行化 `ASTRO_MEMORY_DIR` 相关测试。
    pub fn lock_astro_memory_dir() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }
}

