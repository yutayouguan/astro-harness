use network_proxy::{NetworkDecision, NetworkPolicyRequest, NetworkProxy, NetworkProxyState};
use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{sleep, timeout, Duration};
use types::{NetworkAccess, NetworkPolicy};

fn state_for(domain: Option<String>) -> NetworkProxyState {
    NetworkProxyState::new(NetworkPolicy {
        enabled: true,
        domains: domain
            .map(|domain| BTreeMap::from([(domain, NetworkAccess::Allow)]))
            .unwrap_or_default(),
        ..NetworkPolicy::default()
    })
    .unwrap()
}

async fn read_header(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    timeout(Duration::from_secs(2), async {
        while !bytes.ends_with(b"\r\n\r\n") {
            bytes.push(stream.read_u8().await.unwrap());
        }
    })
    .await
    .unwrap();
    String::from_utf8(bytes).unwrap()
}

#[tokio::test]
async fn connect_tunnel_forwards_bytes_after_policy_allows_target() {
    let target = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let target_addr = target.local_addr().unwrap();
    let state = Arc::new(state_for(Some(target_addr.ip().to_string())));
    let proxy = NetworkProxy::builder().state(state).build().await.unwrap();
    let proxy_addr = proxy.http_addr();
    let handle = proxy.run().await.unwrap();

    let target_task = tokio::spawn(async move {
        let (mut stream, _) = target.accept().await.unwrap();
        let mut bytes = [0_u8; 4];
        stream.read_exact(&mut bytes).await.unwrap();
        stream.write_all(&bytes).await.unwrap();
    });
    let mut client = TcpStream::connect(proxy_addr).await.unwrap();
    client
        .write_all(
            format!("CONNECT {target_addr} HTTP/1.1\r\nHost: {target_addr}\r\n\r\nping").as_bytes(),
        )
        .await
        .unwrap();
    assert!(read_header(&mut client).await.starts_with("HTTP/1.1 200"));
    let mut echoed = [0_u8; 4];
    client.read_exact(&mut echoed).await.unwrap();
    assert_eq!(&echoed, b"ping");

    handle.shutdown().await.unwrap();
    target_task.await.unwrap();
}

#[tokio::test]
async fn connect_denial_returns_forbidden_without_dialing_target() {
    let target = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let target_addr = target.local_addr().unwrap();
    let proxy = NetworkProxy::builder()
        .state(Arc::new(state_for(None)))
        .build()
        .await
        .unwrap();
    let handle = proxy.run().await.unwrap();
    let mut client = TcpStream::connect(proxy.http_addr()).await.unwrap();
    client
        .write_all(
            format!("CONNECT {target_addr} HTTP/1.1\r\nHost: {target_addr}\r\n\r\n").as_bytes(),
        )
        .await
        .unwrap();

    assert!(read_header(&mut client).await.starts_with("HTTP/1.1 403"));
    assert!(timeout(Duration::from_millis(100), target.accept())
        .await
        .is_err());
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn plain_http_requests_are_rejected_without_forwarding() {
    let proxy = NetworkProxy::builder()
        .state(Arc::new(state_for(Some("*".to_string()))))
        .build()
        .await
        .unwrap();
    let handle = proxy.run().await.unwrap();
    let mut client = TcpStream::connect(proxy.http_addr()).await.unwrap();
    client
        .write_all(b"GET http://example.com/ HTTP/1.1\r\nHost: example.com\r\n\r\n")
        .await
        .unwrap();

    assert!(read_header(&mut client).await.starts_with("HTTP/1.1 405"));
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn builder_rejects_non_loopback_bind_addresses() {
    let err = NetworkProxy::builder()
        .state(Arc::new(state_for(Some("*".to_string()))))
        .http_addr(SocketAddr::from(([0, 0, 0, 0], 0)))
        .build()
        .await
        .unwrap_err();

    assert!(err.to_string().contains("loopback"));
}

#[tokio::test]
async fn decider_ask_is_preserved_in_forbidden_response_headers() {
    let state = NetworkProxyState::new(NetworkPolicy {
        enabled: true,
        allow_local_binding: true,
        ..NetworkPolicy::default()
    })
    .unwrap();
    let proxy = NetworkProxy::builder()
        .state(Arc::new(state))
        .policy_decider(
            |_: NetworkPolicyRequest| async move { NetworkDecision::ask("not_allowed") },
        )
        .build()
        .await
        .unwrap();
    let handle = proxy.run().await.unwrap();
    let mut client = TcpStream::connect(proxy.http_addr()).await.unwrap();
    client
        .write_all(b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n")
        .await
        .unwrap();

    let response = read_header(&mut client).await;
    assert!(response.starts_with("HTTP/1.1 403"));
    assert!(response.contains("x-network-policy-decision: ask\r\n"));
    assert!(response.contains("x-network-decision-source: decider\r\n"));
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn malformed_and_oversized_request_heads_are_rejected() {
    let proxy = NetworkProxy::builder()
        .state(Arc::new(state_for(Some("*".to_string()))))
        .build()
        .await
        .unwrap();
    let handle = proxy.run().await.unwrap();

    let mut malformed = TcpStream::connect(proxy.http_addr()).await.unwrap();
    malformed
        .write_all(b"CONNECT missing-port HTTP/1.1\r\n\r\n")
        .await
        .unwrap();
    assert!(read_header(&mut malformed)
        .await
        .starts_with("HTTP/1.1 400"));

    let mut oversized = TcpStream::connect(proxy.http_addr()).await.unwrap();
    let prefix = b"CONNECT example.com:443 HTTP/1.1\r\nX-Fill: ";
    let mut request = prefix.to_vec();
    request.extend(std::iter::repeat_n(b'x', 32 * 1024 + 1 - prefix.len() - 4));
    request.extend_from_slice(b"\r\n\r\n");
    oversized.write_all(&request).await.unwrap();
    assert!(read_header(&mut oversized)
        .await
        .starts_with("HTTP/1.1 431"));

    let mut bad_version = TcpStream::connect(proxy.http_addr()).await.unwrap();
    bad_version
        .write_all(b"CONNECT 127.0.0.1:1 HTTP/1.99\r\nHost: 127.0.0.1:1\r\n\r\n")
        .await
        .unwrap();
    assert!(read_header(&mut bad_version)
        .await
        .starts_with("HTTP/1.1 400"));

    let mut bad_header = TcpStream::connect(proxy.http_addr()).await.unwrap();
    bad_header
        .write_all(b"CONNECT 127.0.0.1:1 HTTP/1.1\r\nBrokenHeader\r\n\r\n")
        .await
        .unwrap();
    assert!(read_header(&mut bad_header)
        .await
        .starts_with("HTTP/1.1 400"));

    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn reserved_listener_can_only_be_started_once() {
    let proxy = NetworkProxy::builder()
        .state(Arc::new(state_for(Some("*".to_string()))))
        .build()
        .await
        .unwrap();
    let handle = proxy.run().await.unwrap();

    let err = match proxy.run().await {
        Ok(_) => panic!("reserved listener started twice"),
        Err(error) => error,
    };
    assert!(err.to_string().contains("already running"));
    handle.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancelling_wait_stops_the_proxy_listener() {
    let proxy = NetworkProxy::builder()
        .state(Arc::new(state_for(Some("*".to_string()))))
        .build()
        .await
        .unwrap();
    let proxy_addr = proxy.http_addr();
    let handle = proxy.run().await.unwrap();

    let waiter = tokio::spawn(handle.wait());
    tokio::task::yield_now().await;
    waiter.abort();
    assert!(waiter.await.unwrap_err().is_cancelled());
    sleep(Duration::from_millis(25)).await;

    assert!(TcpStream::connect(proxy_addr).await.is_err());
}
