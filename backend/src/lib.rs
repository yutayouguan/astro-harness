//! Astro 独立 gRPC 后端入口。
//!
//! 引导工作区、启动 [`AstroServiceImpl`]，并在后台每 30s 认领并执行到期 cron。
//! 也可由桌面壳同进程调用 [`run_embedded`]（跳过二次日志初始化）。

pub mod cron_runner;
pub mod grpc;
pub mod session_events;

pub use session_events::{
    event_matches, to_proto, MemoryUpdatedPayload, PendingChangedPayload,
    SessionEventHub, SessionEventMsg, SessionMetadataChangedPayload, SubscribeFilter,
};

use std::time::Duration;

use anyhow::Context;
use crate::grpc::AstroServiceImpl;
use cron::cron_dir;
use home::{default_memory_dir, init_logging, logs_dir};
use memory::ensure_workspace;
use proto::astro_service_server::AstroServiceServer;
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

/// 启动日志后进入 [`serve`]（独立 `cargo run -p backend` 入口）。
///
/// # 错误
/// 日志初始化失败，或 [`serve`] 失败。
pub async fn run() -> anyhow::Result<()> {
    init_logging("agent")?;
    serve(None).await
}

/// 供 Tauri 同进程内嵌：不做 `init_logging`（壳侧已初始化）。
///
/// `ready` 在 **TCP bind 成功** 后发送一次；bind 失败则 channel 关闭且函数返回错误，
/// 调用方勿把「端口上已有其它进程在听」当成内嵌就绪。
///
/// # 错误
/// 同 [`serve`]。
pub async fn run_embedded(ready: Option<oneshot::Sender<()>>) -> anyhow::Result<()> {
    serve(ready).await
}

/// 工作区、cron ticker 与 gRPC 服务（默认 `127.0.0.1:50051`）。
///
/// 地址可由环境变量 `ASTRO_GRPC_ADDR` 覆盖。先 `TcpListener::bind`，成功后再通知 `ready`。
///
/// # 错误
/// 地址解析失败、工作区初始化失败、端口占用 / bind 失败，或 tonic 服务异常退出。
pub async fn serve(ready: Option<oneshot::Sender<()>>) -> anyhow::Result<()> {
    let addr: std::net::SocketAddr = std::env::var("ASTRO_GRPC_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:50051".to_string())
        .parse()
        .context("parse ASTRO_GRPC_ADDR")?;
    let memory_dir = default_memory_dir();
    let report = ensure_workspace(&memory_dir)?;
    if !report.created_files.is_empty() {
        tracing::info!(
            "Workspace bootstrapped: {}",
            report.created_files.join(", ")
        );
    }
    let service = AstroServiceImpl::new(memory_dir.clone());

    // 先占端口：失败则 ready 不会触发，避免壳误连占用端口的其它进程。
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind gRPC {addr}"))?;
    if let Some(tx) = ready {
        let _ = tx.send(());
    }

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
        .serve_with_incoming(TcpListenerStream::new(listener))
        .await
        .context("gRPC serve")?;

    Ok(())
}
