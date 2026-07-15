//! Astro 独立 gRPC 后端入口。
//!
//! 引导工作区、启动 [`AstroServiceImpl`]，并在后台每 30s 认领并执行到期 cron。

pub mod cron_runner;
pub mod grpc;
pub mod session_events;

pub use session_events::{
    event_matches, to_proto, MemoryUpdatedPayload, PendingChangedPayload, SessionEventHub,
    SessionEventMsg, SubscribeFilter,
};

use std::time::Duration;

use crate::grpc::AstroServiceImpl;
use cron::cron_dir;
use home::{default_memory_dir, init_logging, logs_dir};
use memory::ensure_workspace;
use proto::astro_service_server::AstroServiceServer;
use tonic::transport::Server;

/// 启动日志、工作区、cron ticker 与 gRPC 服务（默认 `127.0.0.1:50051`）。
///
/// 地址可由环境变量 `ASTRO_GRPC_ADDR` 覆盖。
///
/// # 错误
/// 地址解析失败、工作区初始化失败，或 tonic 服务异常退出。
pub async fn run() -> anyhow::Result<()> {
    init_logging("agent")?;

    let addr = std::env::var("ASTRO_GRPC_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:50051".to_string())
        .parse()?;
    let memory_dir = default_memory_dir();
    let report = ensure_workspace(&memory_dir)?;
    if !report.created_files.is_empty() {
        tracing::info!(
            "Workspace bootstrapped: {}",
            report.created_files.join(", ")
        );
    }
    let service = AstroServiceImpl::new(memory_dir.clone());

    // 后台 cron ticker：每 30s claim_due + execute_job
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            cron_runner::tick_and_execute().await;
        }
    });

    tracing::info!("Astro Backend v0.1.0");
    tracing::info!("gRPC Server: {}", addr);
    tracing::info!("Memory dir: {}", memory_dir.display());
    tracing::info!("Logs dir: {}", logs_dir().display());
    tracing::info!("Cron dir: {}", cron_dir().display());
    tracing::info!("Providers: google, openai, claude, deepseek, minmax, zhipu, mimo, ollama");

    Server::builder()
        .add_service(AstroServiceServer::new(service))
        .serve(addr)
        .await?;

    Ok(())
}
