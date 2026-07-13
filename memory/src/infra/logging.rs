//! 全局 tracing 日志初始化：控制台输出 + 按日滚动的文件日志。
//!
//! 日志目录位于 `~/.astro/logs/`，文件名格式为 `{component}.log`。
//! 过滤器默认 `{component}=info,memory=info,agent=info`，可通过 `RUST_LOG` 环境变量覆盖。

use std::sync::OnceLock;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::workspace::{default_memory_dir, ensure_default_workspace};

/// 非阻塞文件写入器的生命周期守卫；必须保持存活，否则日志可能丢失。
static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

/// 初始化控制台 + `~/.astro/logs/{component}.log` 按日滚动文件日志。
///
/// 调用前会确保默认工作区存在。可安全重复调用：第二次及以后 `try_init` 失败时不报错。
///
/// # 参数
///
/// - `component`：日志文件名前缀，同时作为默认过滤器的 crate 名（如 `"agent"`、`"memory"`）。
pub fn init_logging(component: &str) -> anyhow::Result<()> {
    let _ = ensure_default_workspace()?;
    let log_dir = default_memory_dir().join("logs");
    std::fs::create_dir_all(&log_dir)?;

    let file_appender =
        tracing_appender::rolling::daily(&log_dir, format!("{component}.log"));
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let _ = LOG_GUARD.set(guard);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!("{component}=info,memory=info,agent=info"))
    });

    let result = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true))
        .with(
            fmt::layer()
                .with_ansi(false)
                .with_target(true)
                .with_writer(non_blocking),
        )
        .try_init();

    if result.is_ok() {
        tracing::info!(
            component,
            path = %log_dir.join(format!("{component}.log")).display(),
            "file logging enabled"
        );
    }

    Ok(())
}

/// 返回日志目录路径（`~/.astro/logs`），不保证目录已创建。
pub fn logs_dir() -> std::path::PathBuf {
    default_memory_dir().join("logs")
}
