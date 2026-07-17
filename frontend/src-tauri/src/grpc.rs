//! 桌面壳与可选独立 / 内嵌 backend 之间的 gRPC 地址约定。

/// 客户端连接地址（内嵌分配端口后走进程内实际地址）。
pub fn default_grpc_address() -> String {
    common::resolve_grpc_address()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_url_adds_http() {
        assert_eq!(
            endpoint_url("127.0.0.1:50051"),
            "http://127.0.0.1:50051"
        );
        assert_eq!(
            endpoint_url("http://127.0.0.1:50051"),
            "http://127.0.0.1:50051"
        );
    }

    #[test]
    fn embed_flag_parses_off_values() {
        fn enabled(raw: Option<&str>) -> bool {
            match raw {
                Some(v) => {
                    let v = v.trim().to_ascii_lowercase();
                    !(v.is_empty() || v == "0" || v == "false" || v == "no" || v == "off")
                }
                None => true,
            }
        }
        assert!(enabled(None));
        assert!(enabled(Some("1")));
        assert!(!enabled(Some("0")));
        assert!(!enabled(Some("false")));
        assert!(!enabled(Some("OFF")));
    }
}
