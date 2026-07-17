//! 桌面壳与可选独立 / 内嵌 backend 之间的 gRPC 地址约定。

use std::time::Duration;

/// 默认监听地址；可由环境变量 `ASTRO_GRPC_ADDR` 覆盖。
pub fn default_grpc_address() -> String {
    std::env::var("ASTRO_GRPC_ADDR").unwrap_or_else(|_| "127.0.0.1:50051".into())
}

/// 将裸主机端口补全为 tonic 可用的 `http://…` URL。
pub fn endpoint_url(grpc_address: &str) -> String {
    if grpc_address.starts_with("http://") || grpc_address.starts_with("https://") {
        grpc_address.to_string()
    } else {
        format!("http://{grpc_address}")
    }
}

/// 是否在桌面壳同进程内嵌 gRPC backend。
///
/// 默认开启；显式 `ASTRO_EMBED_BACKEND=0` / `false` / `no` / `off` 时关闭，
/// 以便对接外部 `cargo run -p backend`。
pub fn embed_backend_enabled() -> bool {
    match std::env::var("ASTRO_EMBED_BACKEND") {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            !(v.is_empty() || v == "0" || v == "false" || v == "no" || v == "off")
        }
        Err(_) => true,
    }
}

/// 短轮询直到 gRPC 地址可 TCP 连通，或超时。
///
/// 用于内嵌 server 刚 spawn 时减少首连闪错；失败不视为 fatal。
pub async fn wait_grpc_ready(timeout: Duration) -> bool {
    let addr = default_grpc_address();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if tokio::net::TcpStream::connect(&addr).await.is_ok() {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
