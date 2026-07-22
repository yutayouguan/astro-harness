//! 轻量 HTTP Webhook 服务器：接收外部 POST 请求触发工作流。
//!
//! 路由: POST /webhook/{workflow_id}
//! 验证: 如果工作流配置了 secret，检查 X-Webhook-Secret 请求头。

use std::net::SocketAddr;

use workflow::model::NodeType;
use workflow::run_db::WorkflowRunDb;
use workflow::store::WorkflowStore;

/// 启动 webhook HTTP 服务器（在独立线程中调用）。
///
/// 返回实际绑定的地址。如果 `ASTRO_WEBHOOK_PORT` 环境变量存在则使用该端口，
/// 否则系统自动分配。
pub async fn run_webhook_server() -> anyhow::Result<()> {
    let port: u16 = std::env::var("ASTRO_WEBHOOK_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;
    tracing::info!("Webhook server: http://{}", local_addr);

    loop {
        let (stream, _) = listener.accept().await?;
        tokio::task::spawn(async move {
            if let Err(e) = handle_connection(stream).await {
                tracing::debug!(error = %e, "webhook connection error");
            }
        });
    }
}

async fn handle_connection(stream: tokio::net::TcpStream) -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut buf = vec![0u8; 65536];
    let mut stream = stream;
    let n = stream.read(&mut buf).await?;
    let raw = String::from_utf8_lossy(&buf[..n]);

    let (method, path, headers, body) = parse_http_request(&raw);

    if method != "POST" || !path.starts_with("/webhook/") {
        let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nNot Found";
        stream.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    let workflow_id = &path["/webhook/".len()..];
    if workflow_id.is_empty() {
        let resp = "HTTP/1.1 400 Bad Request\r\nContent-Length: 22\r\n\r\nMissing workflow_id";
        stream.write_all(resp.as_bytes()).await?;
        return Ok(());
    }

    let store = WorkflowStore::open_default()?;
    let wf = match store.get(workflow_id)? {
        Some(wf) if wf.enabled => wf,
        Some(_) => {
            let resp = "HTTP/1.1 403 Forbidden\r\nContent-Length: 18\r\n\r\nWorkflow disabled";
            stream.write_all(resp.as_bytes()).await?;
            return Ok(());
        }
        None => {
            let resp = "HTTP/1.1 404 Not Found\r\nContent-Length: 18\r\n\r\nWorkflow not found";
            stream.write_all(resp.as_bytes()).await?;
            return Ok(());
        }
    };

    // 验证 webhook secret
    let trigger_node = wf.nodes.iter().find(|n| {
        !n.disabled && n.node_type == NodeType::WebhookTrigger
    });
    if let Some(node) = trigger_node {
        let secret = node.config.get("secret").and_then(|v| v.as_str()).unwrap_or("");
        if !secret.is_empty() {
            let req_secret = headers.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("x-webhook-secret"))
                .map(|(_, v)| v.as_str())
                .unwrap_or("");
            if req_secret != secret {
                let resp = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 14\r\n\r\nInvalid secret";
                stream.write_all(resp.as_bytes()).await?;
                return Ok(());
            }
        }
    }

    let trigger_input: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or(serde_json::json!({ "raw_body": body }));

    let wf_name = wf.name.clone();
    let _wf_id = workflow_id.to_string();

    let result = tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        rt.block_on(async {
            let run_db = WorkflowRunDb::open_default()?;
            workflow::engine::execute_workflow(&wf, trigger_input, "webhook", &run_db).await
        })
    })
    .await;

    match result {
        Ok(Ok(run_result)) => {
            tracing::info!(workflow = %wf_name, run_id = %run_result.run_id, "webhook 触发成功");
            let json = serde_json::to_string(&run_result).unwrap_or_default();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                json.len(), json
            );
            stream.write_all(resp.as_bytes()).await?;
        }
        Ok(Err(e)) => {
            tracing::error!(workflow = %wf_name, error = %e, "webhook 触发失败");
            let json = serde_json::json!({"error": e.to_string()}).to_string();
            let resp = format!(
                "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                json.len(), json
            );
            stream.write_all(resp.as_bytes()).await?;
        }
        Err(e) => {
            let json = serde_json::json!({"error": e.to_string()}).to_string();
            let resp = format!(
                "HTTP/1.1 500 Internal Server Error\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                json.len(), json
            );
            stream.write_all(resp.as_bytes()).await?;
        }
    }

    Ok(())
}

fn parse_http_request(raw: &str) -> (String, String, Vec<(String, String)>, String) {
    let mut lines = raw.split("\r\n");
    let first = lines.next().unwrap_or("");
    let parts: Vec<&str> = first.splitn(3, ' ').collect();
    let method = parts.first().copied().unwrap_or("").to_string();
    let path = parts.get(1).copied().unwrap_or("").to_string();

    let mut headers = Vec::new();
    let mut body_start = false;
    let mut body = String::new();
    for line in lines {
        if body_start {
            body.push_str(line);
            continue;
        }
        if line.is_empty() {
            body_start = true;
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    (method, path, headers, body)
}
