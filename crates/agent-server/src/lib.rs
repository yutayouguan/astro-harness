#![deny(clippy::unwrap_used)]

//! Astro 独立 gRPC 后端入口。
//!
//! 引导工作区、启动 [`AstroServiceImpl`]，并在独立线程的 current_thread 运行时中
//! 每 30s 认领并执行到期 cron（因 `SessionStore`/`AgentLoop` 非 Send）。
//! 也可由桌面壳同进程调用 [`run_embedded`]（跳过二次日志初始化）。

pub mod cron_runner;
pub mod grpc;
pub mod thread_listener;
pub mod thread_manager;
pub mod thread_state;
pub mod transport;
pub mod webhook_server;
pub mod workflow_ticker;

pub use thread_listener::{run_listener_commands, run_thread_listener};
pub use thread_manager::{ManagedThread, ThreadManager};
pub use thread_state::{
    ItemSnapshot, ListenerCommand, ThreadActivity, ThreadHistoryBuilder, ThreadSnapshot,
    ThreadState, ThreadStateManager, TurnSnapshot,
};
pub use transport::{ConnectionGeneration, ConnectionRegistry};

pub(crate) const POST_TURN_SIDE_EFFECT_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(5 * 60);
pub(crate) const POST_TURN_COMPLETION_MARKER_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(30);
pub(crate) const BACKGROUND_EXTENSION_SINK_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(6 * 60);

/// 用于工作区级事件的持久 Thread，不属于任何单个聊天会话。
pub const WORKSPACE_EVENT_THREAD_ID: &str = "astro-workspace-events";

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
/// 写入 [`types::set_runtime_grpc_address`]，并经 `ready` 回传给壳。
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
    let bind_spec = types::grpc_bind_address(ephemeral_if_unset);
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
    types::set_runtime_grpc_address(&addr_str);
    if let Some(tx) = ready {
        let _ = tx.send(addr_str.clone());
    }

    // 后台 cron ticker：独立 current_thread 运行时。
    // AgentLoop / SessionStore 不是 Send，不能进多线程 tokio::spawn。
    std::thread::Builder::new()
        .name("astro-cron".into())
        .spawn(|| {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(err) => {
                    tracing::error!(error = %err, "cron runtime build failed");
                    return;
                }
            };
            rt.block_on(async move {
                // 启动时回收上次进程残留的 running 记录。
                if let Err(err) = agent::exec::cron::reconcile_orphaned_runs().await {
                    tracing::warn!(error = %err, "cron: reconcile orphaned runs failed");
                }
                let mut interval = tokio::time::interval(Duration::from_secs(30));
                loop {
                    interval.tick().await;
                    cron_runner::tick_and_execute().await;
                }
            });
        })
        .context("spawn astro-cron thread")?;

    // 后台工作流定时触发 ticker
    std::thread::Builder::new()
        .name("astro-workflow-ticker".into())
        .spawn(|| {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(err) => {
                    tracing::error!(error = %err, "workflow ticker runtime build failed");
                    return;
                }
            };
            rt.block_on(async move {
                // 首次启动延迟 10s，让系统先稳定
                tokio::time::sleep(Duration::from_secs(10)).await;
                let mut interval = tokio::time::interval(Duration::from_secs(30));
                loop {
                    interval.tick().await;
                    workflow_ticker::tick_workflows().await;
                }
            });
        })
        .context("spawn astro-workflow-ticker thread")?;

    // Webhook HTTP 服务器
    std::thread::Builder::new()
        .name("astro-webhook".into())
        .spawn(|| {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(err) => {
                    tracing::error!(error = %err, "webhook server runtime build failed");
                    return;
                }
            };
            rt.block_on(async move {
                if let Err(e) = webhook_server::run_webhook_server().await {
                    tracing::error!(error = %e, "webhook server failed");
                }
            });
        })
        .context("spawn astro-webhook thread")?;

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
