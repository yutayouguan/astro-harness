//! 全局 tracing 日志初始化：控制台输出 + 按日滚动的双文件日志。
//!
//! 日志目录位于 `~/.astro/logs/`：
//! - `agent.log.YYYY-MM-DD`：INFO 及以上（受 EnvFilter 约束）
//! - `errors.log.YYYY-MM-DD`：WARN 及以上
//!
//! 过滤器默认 `info,memory=info,agent=info`，可通过 `RUST_LOG` 环境变量覆盖。

use std::sync::OnceLock;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{
    filter::LevelFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer,
};

use crate::workspace::default_memory_dir;

/// 非阻塞文件写入器的生命周期守卫；必须保持存活，否则日志可能丢失。
static LOG_GUARDS: OnceLock<(WorkerGuard, WorkerGuard)> = OnceLock::new();

/// 初始化控制台 + `~/.astro/logs/agent.log.YYYY-MM-DD` 与
/// `errors.log.YYYY-MM-DD` 按日滚动文件日志。
///
/// 可安全重复调用：第二次及以后 `try_init` 失败时不报错。
///
/// # 参数
///
/// - `_component`：保留以兼容调用方；文件名前缀已统一为 `agent.log`。
pub fn init_logging(_component: &str) -> anyhow::Result<()> {
    let log_dir = default_memory_dir().join("logs");
    std::fs::create_dir_all(&log_dir)?;

    let agent_appender = tracing_appender::rolling::daily(&log_dir, "agent.log");
    let (agent_nb, g1) = tracing_appender::non_blocking(agent_appender);
    let err_appender = tracing_appender::rolling::daily(&log_dir, "errors.log");
    let (err_nb, g2) = tracing_appender::non_blocking(err_appender);
    let _ = LOG_GUARDS.set((g1, g2));

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,memory=info,agent=info"));

    let agent_file = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_writer(agent_nb);

    let errors_file = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_writer(err_nb)
        .with_filter(LevelFilter::WARN);

    let result = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true))
        .with(agent_file)
        .with(errors_file)
        .try_init();

    if result.is_ok() {
        tracing::info!(
            component = _component,
            agent_file_pattern = %log_dir.join("agent.log.YYYY-MM-DD").display(),
            errors_file_pattern = %log_dir.join("errors.log.YYYY-MM-DD").display(),
            "file logging enabled (agent + errors)"
        );
    }

    Ok(())
}

/// 返回日志目录路径（`~/.astro/logs`），不保证目录已创建。
pub fn logs_dir() -> std::path::PathBuf {
    default_memory_dir().join("logs")
}
