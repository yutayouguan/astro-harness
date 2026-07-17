//! Astro 独立 gRPC 后端入口。
//!
//! 引导工作区、启动 [`AstroServiceImpl`]，并在后台每 30s 认领并执行到期 cron。
//! 也可由桌面壳同进程调用 [`run_embedded`]（跳过二次日志初始化）。

pub mod cron_runner;
pub mod grpc;
pub mod session_events;

pub use session_events::{
    event_matches, to_proto, MemoryUpdatedPayload, PendingChangedPayload, SessionEventHub,
    SessionEventMsg, SessionMetadataChangedPayload, SubscribeFilter,
};

use std::time::Duration;

use crate::grpc::AstroServiceImpl;
use anyhow::Context;
use cron::cron_dir;
use home::{default_memory_dir, init_logging, logs_dir};
use memory::ensure_workspace;
use proto::astro_service_server::AstroServiceServer;
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

/// 启动日志后进入 [`serve`]（独立 `cargo run -p backend`：默认固定 `50051`）。
///
/// # 错误
/// 日志初始化失败，或 [`serve`] 失败。
pub async fn run() -> anyhow::Result<()> {
    init_logging("agent")?;
    serve(None, false).await
}

/// 供 Tauri 同进程内嵌：不做 `init_logging`（壳侧已初始化）。
///
/// 未设置 `ASTRO_GRPC_ADDR` 时 bind `127.0.0.1:0`，系统分配端口；bind 成功后把实际地址
/// 写入 [`common::set_runtime_grpc_address`]，并经 `ready` 回传给壳。
///
/// # 错误
/// 同 [`serve`]。
pub async fn run_embedded(ready: Option<oneshot::Sender<String>>) -> anyhow::Result<()> {
    serve(ready, true).await
}

/// 工作区、cron ticker 与 gRPC 服务。
///
/// - `ephemeral_if_unset`：无 `ASTRO_GRPC_ADDR` 时用 `127.0.0.1:0`
/// - 有 `ASTRO_GRPC_ADDR`：始终用该固定地址
///
/// # 错误
/// 地址解析失败、工作区初始化失败、端口占用 / bind 失败，或 tonic 服务异常退出。
pub async fn serve(
    ready: Option<oneshot::Sender<String>>,
    ephemeral_if_unset: bool,
) -> anyhow::Result<()> {
    let bind_spec = common::grpc_bind_address(ephemeral_if_unset);
    let addr: std::net::SocketAddr = bind_spec
        .parse()
        .with_context(|| format!("parse gRPC bind address `{bind_spec}`"))?;
    let memory_dir = default_memory_dir();
    let report = ensure_workspace(&memory_dir)?;
    if !report.created_files.is_empty() {
        tracing::info!(
            "Workspace bootstrapped: {}",
            report.created_files.join(", ")
        );
    }
    let service = AstroServiceImpl::new(memory_dir.clone());

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind gRPC {addr}"))?;
    let local = listener
        .local_addr()
        .context("read bound gRPC local_addr")?;
    let addr_str = local.to_string();
    common::set_runtime_grpc_address(&addr_str);
    if let Some(tx) = ready {
        let _ = tx.send(addr_str.clone());
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
    tracing::info!("gRPC Server: {}", addr_str);
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
