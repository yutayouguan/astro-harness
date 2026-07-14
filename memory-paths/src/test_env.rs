//! 测试辅助：串行化并安全地临时覆盖 `ASTRO_MEMORY_DIR`。
//!
//! 多个模块的单测都会改写该环境变量；若不加全局锁，并行执行会互相污染。

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

/// 跨 memory-paths / memory-agent / memory crate 内所有单测共享的环境变量锁。
pub fn lock_astro_memory_dir() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// 持有锁期间将 `ASTRO_MEMORY_DIR` 设为 `path`，Drop 时恢复原值。
pub struct AstroMemoryDirGuard {
    _lock: MutexGuard<'static, ()>,
    prev: Option<String>,
}

impl AstroMemoryDirGuard {
    pub fn set(path: &Path) -> Self {
        let _lock = lock_astro_memory_dir();
        let prev = std::env::var("ASTRO_MEMORY_DIR").ok();
        std::env::set_var("ASTRO_MEMORY_DIR", path);
        Self { _lock, prev }
    }
}

impl Drop for AstroMemoryDirGuard {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => std::env::set_var("ASTRO_MEMORY_DIR", v),
            None => std::env::remove_var("ASTRO_MEMORY_DIR"),
        }
    }
}
