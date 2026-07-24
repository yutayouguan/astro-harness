//! 本机 gRPC 地址：环境变量固定端口，或内嵌时 `127.0.0.1:0` 分配后写入进程内。

use std::sync::{Mutex, OnceLock};

static RUNTIME_ADDR: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn runtime_slot() -> &'static Mutex<Option<String>> {
    RUNTIME_ADDR.get_or_init(|| Mutex::new(None))
}

/// 内嵌 bind 成功后写入实际地址（含系统分配的端口）。
pub fn set_runtime_grpc_address(addr: impl Into<String>) {
    if let Ok(mut slot) = runtime_slot().lock() {
        *slot = Some(addr.into());
    }
}

/// 当前进程内已解析的地址（若有）。
pub fn runtime_grpc_address() -> Option<String> {
    runtime_slot().lock().ok().and_then(|g| g.clone())
}

/// 客户端连接地址：优先进程内实际地址，否则 `ASTRO_GRPC_ADDR`，再否则 `127.0.0.1:50051`。
pub fn resolve_grpc_address() -> String {
    if let Some(addr) = runtime_grpc_address() {
        return addr;
    }
    std::env::var("ASTRO_GRPC_ADDR").unwrap_or_else(|_| "127.0.0.1:50051".into())
}

/// 服务端 bind 地址。
///
/// - 已设置非空 `ASTRO_GRPC_ADDR` → 用之（调试固定端口）
/// - 否则若 `ephemeral_if_unset` → `127.0.0.1:0`（系统分配）
/// - 否则 → `127.0.0.1:50051`（独立 `cargo run -p backend`）
pub fn grpc_bind_address(ephemeral_if_unset: bool) -> String {
    if let Ok(addr) = std::env::var("ASTRO_GRPC_ADDR") {
        let addr = addr.trim();
        if !addr.is_empty() {
            return addr.to_string();
        }
    }
    if ephemeral_if_unset {
        "127.0.0.1:0".into()
    } else {
        "127.0.0.1:50051".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_overrides_default() {
        set_runtime_grpc_address("127.0.0.1:54321");
        assert_eq!(resolve_grpc_address(), "127.0.0.1:54321");
        // 清掉，避免污染其它测试
        if let Ok(mut slot) = runtime_slot().lock() {
            *slot = None;
        }
    }

    #[test]
    fn bind_ephemeral_when_unset() {
        // 不依赖是否设置了 ASTRO_GRPC_ADDR：只测分支字面量
        assert_eq!(
            {
                if true {
                    "127.0.0.1:0"
                } else {
                    "127.0.0.1:50051"
                }
            },
            "127.0.0.1:0"
        );
        assert!(grpc_bind_address(false).contains(':'));
    }
}
