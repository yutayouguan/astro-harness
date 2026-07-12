//! 桌面壳与可选独立 backend 之间的 gRPC 地址约定。

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
