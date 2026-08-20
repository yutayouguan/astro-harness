//! MCP OAuth 浏览器授权桥接：仅监听 loopback，token 由 agent-mcp 写入系统 Keychain。

use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

const CALLBACK_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MAX_CALLBACK_REQUEST_BYTES: usize = 16 * 1024;

type FlowResult = Result<(), String>;
type FlowRegistry = Mutex<HashMap<String, JoinHandle<FlowResult>>>;

fn flows() -> &'static FlowRegistry {
    static FLOWS: OnceLock<FlowRegistry> = OnceLock::new();
    FLOWS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpOAuthBeginResult {
    flow_id: String,
    authorization_url: String,
}

fn load_server(server_id: &str) -> Result<mcp::McpServerConfig, String> {
    let project_root = worktree::resolve_project_root(None);
    mcp::load_mcp_servers_layered(project_root.as_deref())
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|config| mcp::sanitize_server_id(&config.id) == mcp::sanitize_server_id(server_id))
        .ok_or_else(|| {
            format!(
                "MCP server not found: {}",
                mcp::sanitize_server_id(server_id)
            )
        })
}

/// 开始标准 MCP OAuth 2.1 + PKCE 流程并返回要在系统浏览器打开的 URL。
#[tauri::command]
pub async fn begin_mcp_oauth(server_id: String) -> Result<McpOAuthBeginResult, String> {
    let config = load_server(&server_id)?;
    if !config.enabled {
        return Err("enable the MCP server before authentication".into());
    }
    if !mcp::auth::is_oauth_available(&config) {
        return Err("this MCP server does not support Astro OAuth login".into());
    }

    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|error| format!("bind OAuth callback listener: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("read OAuth callback address: {error}"))?
        .port();
    let flow_id = uuid::Uuid::new_v4().to_string();
    let callback_path = format!("/mcp/oauth/callback/{flow_id}");
    let redirect_uri = format!("http://127.0.0.1:{port}{callback_path}");
    let session = mcp::auth::begin_oauth(&config, &redirect_uri)
        .await
        .map_err(|error| error.to_string())?;
    let authorization_url = session.authorization_url().to_string();

    let handle = tokio::spawn(async move {
        let accepted = tokio::time::timeout(CALLBACK_TIMEOUT, listener.accept())
            .await
            .map_err(|_| "MCP OAuth callback timed out".to_string())?
            .map_err(|error| format!("accept MCP OAuth callback: {error}"))?;
        let (mut stream, _) = accepted;
        let callback_url = read_callback_url(&mut stream, port, &callback_path).await;
        let result = match callback_url {
            Ok(callback_url) => session
                .complete(callback_url.as_str())
                .await
                .map_err(|error| error.to_string()),
            Err(error) => Err(error),
        };
        write_browser_response(&mut stream, result.is_ok()).await;
        result
    });
    flows().lock().await.insert(flow_id.clone(), handle);

    Ok(McpOAuthBeginResult {
        flow_id,
        authorization_url,
    })
}

/// 等待浏览器回调完成。回调 code/state 不经过前端，也不会写入日志。
#[tauri::command]
pub async fn complete_mcp_oauth(flow_id: String) -> Result<(), String> {
    let handle = flows()
        .lock()
        .await
        .remove(&flow_id)
        .ok_or_else(|| "MCP OAuth flow not found or already completed".to_string())?;
    handle
        .await
        .map_err(|error| format!("MCP OAuth flow task failed: {error}"))?
}

#[tauri::command]
pub async fn cancel_mcp_oauth(flow_id: String) -> Result<(), String> {
    if let Some(handle) = flows().lock().await.remove(&flow_id) {
        handle.abort();
    }
    Ok(())
}

/// 删除系统 Keychain 中的 registration/token。调用方随后重连 Server。
#[tauri::command]
pub async fn logout_mcp_oauth(server_id: String) -> Result<(), String> {
    let config = load_server(&server_id)?;
    mcp::auth::clear_oauth_credentials(&config)
        .await
        .map_err(|error| error.to_string())
}

async fn read_callback_url(
    stream: &mut TcpStream,
    port: u16,
    expected_path: &str,
) -> Result<url::Url, String> {
    let mut request = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    loop {
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|error| format!("read MCP OAuth callback: {error}"))?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if request.len() > MAX_CALLBACK_REQUEST_BYTES {
            return Err("MCP OAuth callback request is too large".into());
        }
    }
    let first_line = std::str::from_utf8(&request)
        .map_err(|_| "MCP OAuth callback was not valid UTF-8".to_string())?
        .lines()
        .next()
        .ok_or_else(|| "MCP OAuth callback request was empty".to_string())?;
    let mut parts = first_line.split_whitespace();
    if parts.next() != Some("GET") {
        return Err("MCP OAuth callback must use GET".into());
    }
    let target = parts
        .next()
        .ok_or_else(|| "MCP OAuth callback target is missing".to_string())?;
    let callback = url::Url::parse(&format!("http://127.0.0.1:{port}{target}"))
        .map_err(|error| format!("invalid MCP OAuth callback: {error}"))?;
    if callback.path() != expected_path {
        return Err("MCP OAuth callback path did not match the active flow".into());
    }
    Ok(callback)
}

async fn write_browser_response(stream: &mut TcpStream, success: bool) {
    let (status, title, message) = if success {
        (
            "200 OK",
            "Authentication complete",
            "You can return to Astro Agent.",
        )
    } else {
        (
            "400 Bad Request",
            "Authentication failed",
            "Return to Astro Agent to see the error.",
        )
    };
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\"><title>{title}</title><style>body{{font:16px system-ui;color:#e8e8e8;background:#171717;display:grid;place-items:center;min-height:100vh;margin:0}}main{{max-width:32rem;padding:2rem}}h1{{font-size:1.4rem}}</style></head><body><main><h1>{title}</h1><p>{message}</p></main></body></html>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn callback_parser_rejects_another_flow_path() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = tokio::spawn(async move {
            let mut stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
                .await
                .unwrap();
            stream
                .write_all(b"GET /mcp/oauth/callback/other?code=x&state=y HTTP/1.1\r\n\r\n")
                .await
                .unwrap();
        });
        let (mut server, _) = listener.accept().await.unwrap();
        let error = read_callback_url(&mut server, port, "/mcp/oauth/callback/current")
            .await
            .unwrap_err();
        assert!(error.contains("did not match"));
        client.await.unwrap();
    }

    #[tokio::test]
    async fn callback_parser_keeps_code_and_state_inside_backend_url() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = tokio::spawn(async move {
            let mut stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
                .await
                .unwrap();
            stream
                .write_all(b"GET /mcp/oauth/callback/current?code=secret-code&state=one-time-state HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
                .await
                .unwrap();
        });
        let (mut server, _) = listener.accept().await.unwrap();
        let callback = read_callback_url(&mut server, port, "/mcp/oauth/callback/current")
            .await
            .unwrap();
        assert_eq!(callback.query_pairs().count(), 2);
        assert_eq!(callback.host_str(), Some("127.0.0.1"));
        client.await.unwrap();
    }
}
