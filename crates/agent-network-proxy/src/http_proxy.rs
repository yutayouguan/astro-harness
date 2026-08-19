use crate::connect_policy::{connect_checked, ConnectError};
use crate::{
    BlockedRequest, NetworkDecision, NetworkDecisionSource, NetworkPolicyDecider,
    NetworkPolicyDecision, NetworkPolicyRequest, NetworkPolicyRequestArgs, NetworkProtocol,
    NetworkProxyState,
};
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{copy_bidirectional, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::{sleep, timeout};

const MAX_REQUEST_HEAD_BYTES: usize = 32 * 1024;
const MAX_CONCURRENT_CONNECTIONS: usize = 256;
const REQUEST_HEAD_TIMEOUT: Duration = Duration::from_secs(5);
const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_millis(100);
const OVERLOAD_RESPONSE_TIMEOUT: Duration = Duration::from_secs(1);

pub(crate) async fn run_http_proxy_with_listener(
    state: Arc<NetworkProxyState>,
    listener: TcpListener,
    policy_decider: Option<Arc<dyn NetworkPolicyDecider>>,
) -> Result<()> {
    run_http_proxy_with_connection_limit(
        state,
        listener,
        policy_decider,
        MAX_CONCURRENT_CONNECTIONS,
    )
    .await
}

async fn run_http_proxy_with_connection_limit(
    state: Arc<NetworkProxyState>,
    listener: TcpListener,
    policy_decider: Option<Arc<dyn NetworkPolicyDecider>>,
    max_connections: usize,
) -> Result<()> {
    let mut connections = JoinSet::new();
    let connection_permits = Arc::new(Semaphore::new(max_connections));
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (mut stream, client_addr) = match accepted {
                    Ok(connection) => connection,
                    Err(_) => {
                        sleep(ACCEPT_ERROR_BACKOFF).await;
                        continue;
                    }
                };
                let Ok(permit) = Arc::clone(&connection_permits).try_acquire_owned() else {
                    let _ = timeout(
                        OVERLOAD_RESPONSE_TIMEOUT,
                        write_empty_response(&mut stream, "503 Service Unavailable", &[]),
                    )
                    .await;
                    continue;
                };
                let state = Arc::clone(&state);
                let policy_decider = policy_decider.clone();
                connections.spawn(async move {
                    let _permit = permit;
                    let _ = handle_connection(state, policy_decider, stream, client_addr).await;
                });
            }
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
        }
    }
}

async fn handle_connection(
    state: Arc<NetworkProxyState>,
    policy_decider: Option<Arc<dyn NetworkPolicyDecider>>,
    mut client: TcpStream,
    client_addr: SocketAddr,
) -> Result<()> {
    let request = match read_connect_request(&mut client).await {
        Ok(request) => request,
        Err(RequestHeadError::MethodNotAllowed) => {
            write_empty_response(&mut client, "405 Method Not Allowed", &[]).await?;
            return Ok(());
        }
        Err(RequestHeadError::TimedOut) => {
            write_empty_response(&mut client, "408 Request Timeout", &[]).await?;
            return Ok(());
        }
        Err(RequestHeadError::TooLarge) => {
            write_empty_response(&mut client, "431 Request Header Fields Too Large", &[]).await?;
            return Ok(());
        }
        Err(RequestHeadError::BadRequest) => {
            write_empty_response(&mut client, "400 Bad Request", &[]).await?;
            return Ok(());
        }
        Err(RequestHeadError::Io(error)) => return Err(error.into()),
    };

    let policy_request = NetworkPolicyRequest::new(NetworkPolicyRequestArgs {
        protocol: NetworkProtocol::HttpsConnect,
        host: request.host.clone(),
        port: request.port,
        environment_id: None,
        client_addr: Some(client_addr.to_string()),
        method: Some("CONNECT".to_string()),
        command: None,
        exec_policy_hint: None,
    });
    let policy_decision = state
        .evaluate_host_policy(policy_decider.as_ref(), &policy_request)
        .await?;
    match &policy_decision {
        NetworkDecision::Allow => {}
        NetworkDecision::Deny {
            reason,
            source,
            decision,
        } => {
            state.record_blocked_request(
                BlockedRequest::from_denial(&policy_request, &policy_decision)
                    .expect("matched network denial must produce a blocked request"),
            );
            let error_kind = match reason.as_str() {
                "denied" => "blocked-by-denylist",
                "not_allowed" | "not_allowed_local" => "blocked-by-allowlist",
                _ => "blocked-by-policy",
            };
            write_empty_response(
                &mut client,
                "403 Forbidden",
                &[
                    ("x-proxy-error", error_kind),
                    ("x-network-policy-decision", decision_name(*decision)),
                    ("x-network-decision-source", source_name(*source)),
                ],
            )
            .await?;
            return Ok(());
        }
    }

    let mut upstream = match connect_checked(&state, &request.host, request.port).await {
        Ok(upstream) => upstream,
        Err(ConnectError::PolicyDenied) => {
            state.record_blocked_request(BlockedRequest::rebinding_denial(&policy_request));
            write_empty_response(
                &mut client,
                "403 Forbidden",
                &[
                    ("x-proxy-error", "blocked-by-allowlist"),
                    ("x-network-policy-decision", "deny"),
                    ("x-network-decision-source", "proxy_state"),
                ],
            )
            .await?;
            return Ok(());
        }
        Err(_) => {
            write_empty_response(&mut client, "502 Bad Gateway", &[]).await?;
            return Ok(());
        }
    };
    client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;
    if !request.buffered_tunnel_bytes.is_empty() {
        upstream.write_all(&request.buffered_tunnel_bytes).await?;
    }
    copy_bidirectional(&mut client, &mut upstream).await?;
    Ok(())
}

struct ConnectRequest {
    host: String,
    port: u16,
    buffered_tunnel_bytes: Vec<u8>,
}

enum RequestHeadError {
    BadRequest,
    MethodNotAllowed,
    TimedOut,
    TooLarge,
    Io(std::io::Error),
}

async fn read_connect_request(client: &mut TcpStream) -> Result<ConnectRequest, RequestHeadError> {
    timeout(REQUEST_HEAD_TIMEOUT, read_connect_request_inner(client))
        .await
        .map_err(|_| RequestHeadError::TimedOut)?
}

async fn read_connect_request_inner(
    client: &mut TcpStream,
) -> Result<ConnectRequest, RequestHeadError> {
    let mut bytes = Vec::with_capacity(1024);
    let header_end = loop {
        if let Some(position) = find_header_end(&bytes) {
            if position > MAX_REQUEST_HEAD_BYTES {
                return Err(RequestHeadError::TooLarge);
            }
            break position;
        }
        if bytes.len() >= MAX_REQUEST_HEAD_BYTES {
            return Err(RequestHeadError::TooLarge);
        }
        let mut chunk = [0_u8; 1024];
        let read = client
            .read(&mut chunk)
            .await
            .map_err(RequestHeadError::Io)?;
        if read == 0 {
            return Err(RequestHeadError::BadRequest);
        }
        bytes.extend_from_slice(&chunk[..read]);
    };

    let head =
        std::str::from_utf8(&bytes[..header_end]).map_err(|_| RequestHeadError::BadRequest)?;
    let mut lines = head.split("\r\n");
    let request_line = lines.next().ok_or(RequestHeadError::BadRequest)?;
    let mut parts = request_line.split_ascii_whitespace();
    let method = parts.next().ok_or(RequestHeadError::BadRequest)?;
    let authority = parts.next().ok_or(RequestHeadError::BadRequest)?;
    let version = parts.next().ok_or(RequestHeadError::BadRequest)?;
    if parts.next().is_some() || !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return Err(RequestHeadError::BadRequest);
    }
    if method != "CONNECT" {
        return Err(RequestHeadError::MethodNotAllowed);
    }
    for line in lines.take_while(|line| !line.is_empty()) {
        let (name, _) = line.split_once(':').ok_or(RequestHeadError::BadRequest)?;
        if name.is_empty() || !name.bytes().all(is_header_name_byte) {
            return Err(RequestHeadError::BadRequest);
        }
    }
    let (host, port) = parse_authority(authority).ok_or(RequestHeadError::BadRequest)?;
    Ok(ConnectRequest {
        host,
        port,
        buffered_tunnel_bytes: bytes[header_end..].to_vec(),
    })
}

fn is_header_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
}

fn parse_authority(authority: &str) -> Option<(String, u16)> {
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let end = rest.find(']')?;
        let host = &rest[..end];
        host.parse::<std::net::Ipv6Addr>().ok()?;
        let port = rest[end + 1..].strip_prefix(':')?;
        (host, port)
    } else {
        let (host, port) = authority.rsplit_once(':')?;
        if host.contains(':') {
            return None;
        }
        (host, port)
    };
    if host.is_empty()
        || host
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || b"/@?#\\".contains(&byte))
    {
        return None;
    }
    let port = port.parse::<u16>().ok().filter(|port| *port != 0)?;
    Some((crate::normalize_host(host), port))
}

async fn write_empty_response(
    stream: &mut TcpStream,
    status: &str,
    headers: &[(&str, &str)],
) -> std::io::Result<()> {
    let mut response = format!("HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: 0\r\n");
    for (name, value) in headers {
        response.push_str(name);
        response.push_str(": ");
        response.push_str(value);
        response.push_str("\r\n");
    }
    response.push_str("\r\n");
    stream.write_all(response.as_bytes()).await
}

const fn decision_name(decision: NetworkPolicyDecision) -> &'static str {
    match decision {
        NetworkPolicyDecision::Deny => "deny",
        NetworkPolicyDecision::Ask => "ask",
    }
}

const fn source_name(source: NetworkDecisionSource) -> &'static str {
    match source {
        NetworkDecisionSource::BaselinePolicy => "baseline_policy",
        NetworkDecisionSource::ModeGuard => "mode_guard",
        NetworkDecisionSource::ProxyState => "proxy_state",
        NetworkDecisionSource::Decider => "decider",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use types::NetworkPolicy;

    #[test]
    fn authority_parser_handles_dns_ipv4_and_bracketed_ipv6() {
        assert_eq!(
            parse_authority("example.com:443"),
            Some(("example.com".to_string(), 443))
        );
        assert_eq!(
            parse_authority("127.0.0.1:8443"),
            Some(("127.0.0.1".to_string(), 8443))
        );
        assert_eq!(parse_authority("[::1]:443"), Some(("::1".to_string(), 443)));
        assert_eq!(parse_authority("[example.com]:443"), None);
        assert_eq!(parse_authority("::1:443"), None);
        assert_eq!(parse_authority("example.com:0"), None);
    }

    #[test]
    fn rebinding_denial_records_proxy_state_attribution() {
        let state = NetworkProxyState::new(NetworkPolicy {
            enabled: true,
            ..NetworkPolicy::default()
        })
        .unwrap();
        let request = NetworkPolicyRequest::new(NetworkPolicyRequestArgs {
            protocol: NetworkProtocol::HttpsConnect,
            host: "api.example.com".to_string(),
            port: 443,
            environment_id: None,
            client_addr: Some("127.0.0.1:50000".to_string()),
            method: Some("CONNECT".to_string()),
            command: None,
            exec_policy_hint: None,
        });

        state.record_blocked_request(BlockedRequest::rebinding_denial(&request));

        let blocked = state.take_blocked_requests();
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].host, "api.example.com");
        assert_eq!(blocked[0].port, 443);
        assert_eq!(blocked[0].reason, "not_allowed_local");
        assert_eq!(blocked[0].decision, NetworkPolicyDecision::Deny);
        assert_eq!(blocked[0].source, NetworkDecisionSource::ProxyState);
    }

    #[tokio::test]
    async fn connection_limit_rejects_excess_clients() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let state = Arc::new(
            NetworkProxyState::new(types::NetworkPolicy {
                enabled: true,
                ..types::NetworkPolicy::default()
            })
            .unwrap(),
        );
        let proxy_task = tokio::spawn(run_http_proxy_with_connection_limit(
            state, listener, None, 1,
        ));

        let _first = TcpStream::connect(address).await.unwrap();
        tokio::time::sleep(Duration::from_millis(25)).await;
        let mut excess = TcpStream::connect(address).await.unwrap();
        let mut response = String::new();
        timeout(Duration::from_secs(2), excess.read_to_string(&mut response))
            .await
            .unwrap()
            .unwrap();

        assert!(response.starts_with("HTTP/1.1 503 Service Unavailable"));
        proxy_task.abort();
    }
}
