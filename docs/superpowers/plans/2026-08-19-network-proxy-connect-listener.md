# Network Proxy CONNECT Listener Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Add a real loopback-only HTTP/1 CONNECT proxy listener that applies `NetworkProxyState` before dialing and tunnelling an upstream TCP connection.

**Architecture:** `NetworkProxyBuilder` reserves a loopback `TcpListener`; `NetworkProxy::run` owns one accept loop and returns a `NetworkProxyHandle`. Each connection parses exactly one bounded HTTP header, accepts only `CONNECT`, builds the existing `NetworkPolicyRequest`, evaluates the policy/decider, resolves and re-checks the actual dial address, and then uses `copy_bidirectional`. Plain absolute-form HTTP, SOCKS, MITM, child-process environment injection, and orchestrator retry are excluded from this batch.

**Tech Stack:** Rust 2021, Tokio TCP/I/O/time/task APIs, existing `agent-network-proxy` policy core, integration tests with real loopback sockets.

---

### Task 1: Lock the listener contract with real TCP tests

**Files:**
- Create: `crates/agent-network-proxy/tests/http_connect.rs`

- [x] **Step 1: Write the failing public API and socket behavior tests**

```rust
use network_proxy::{NetworkProxy, NetworkProxyState};
use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{timeout, Duration};
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
            format!("CONNECT {target_addr} HTTP/1.1\r\nHost: {target_addr}\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    assert!(read_header(&mut client).await.starts_with("HTTP/1.1 200"));
    client.write_all(b"ping").await.unwrap();
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
            format!("CONNECT {target_addr} HTTP/1.1\r\nHost: {target_addr}\r\n\r\n")
                .as_bytes(),
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
```

- [x] **Step 2: Run the tests and verify RED**

Run: `cargo test -p network-proxy --test http_connect`

Expected: compile failure for missing `NetworkProxy` / `NetworkProxyBuilder`, followed by behavioral failures after the smallest API surface is introduced.

### Task 2: Add Codex-named proxy lifecycle types

**Files:**
- Create: `crates/agent-network-proxy/src/proxy.rs`
- Modify: `crates/agent-network-proxy/src/lib.rs`
- Modify: `crates/agent-network-proxy/Cargo.toml`

- [x] **Step 1: Implement the builder and reserved loopback listener**

```rust
#[derive(Clone, Default)]
pub struct NetworkProxyBuilder {
    state: Option<Arc<NetworkProxyState>>,
    http_addr: Option<SocketAddr>,
    policy_decider: Option<Arc<dyn NetworkPolicyDecider>>,
}

impl NetworkProxyBuilder {
    pub fn state(mut self, state: Arc<NetworkProxyState>) -> Self {
        self.state = Some(state);
        self
    }

    pub fn http_addr(mut self, addr: SocketAddr) -> Self {
        self.http_addr = Some(addr);
        self
    }

    pub fn policy_decider_arc(mut self, decider: Arc<dyn NetworkPolicyDecider>) -> Self {
        self.policy_decider = Some(decider);
        self
    }
}
```

`build(self) -> Result<NetworkProxy>` must reject non-loopback addresses, bind immediately to reserve the selected ephemeral port, and require a policy state.

- [x] **Step 2: Implement run/shutdown ownership**

```rust
impl NetworkProxy {
    pub fn builder() -> NetworkProxyBuilder {
        NetworkProxyBuilder::default()
    }

    pub fn http_addr(&self) -> SocketAddr {
        self.http_addr
    }
}
```

`run(&self) -> Result<NetworkProxyHandle>` may take the listener once. `NetworkProxyHandle::wait(self)` awaits the accept task; `shutdown(self)` aborts and awaits it. The accept task owns a `JoinSet` so aborting the listener also aborts active tunnel tasks.

- [x] **Step 3: Run the lifecycle tests and keep the tunnel test RED**

Run: `cargo test -p network-proxy --test http_connect`

Expected: builder/bind assertions pass; CONNECT forwarding remains failing until Task 3.

### Task 3: Enforce policy and forward CONNECT tunnels

**Files:**
- Create: `crates/agent-network-proxy/src/http_proxy.rs`
- Create: `crates/agent-network-proxy/src/connect_policy.rs`
- Modify: `crates/agent-network-proxy/src/proxy.rs`
- Modify: `crates/agent-network-proxy/src/runtime.rs`

- [x] **Step 1: Parse one bounded CONNECT request**

Read at most 32 KiB and require `\r\n\r\n` within five seconds. Parse bracketed IPv6 or `host:port`; return `400` for malformed authority, `405` for non-CONNECT methods, and close the client connection.

- [x] **Step 2: Evaluate the existing structured policy request**

```rust
let request = NetworkPolicyRequest::new(NetworkPolicyRequestArgs {
    protocol: NetworkProtocol::HttpsConnect,
    host,
    port,
    environment_id: None,
    client_addr: Some(client_addr.to_string()),
    method: Some("CONNECT".to_string()),
    command: None,
    exec_policy_hint: None,
});
```

Return `403` for `NetworkDecision::Deny`; only `NetworkDecision::Allow` may dial upstream.

- [x] **Step 3: Re-check each resolved address before dialing**

Resolve the target under a timeout. Public addresses may be dialled; a non-public resolved address is permitted only when `allow_local_binding=true` or the original target is the same explicit IP/`localhost` exception already accepted by `NetworkProxyState`. Never treat an allowlisted public hostname as permission to connect to a private rebinding result.

- [x] **Step 4: Establish and forward the tunnel**

Write `HTTP/1.1 200 Connection Established\r\n\r\n`, then call `tokio::io::copy_bidirectional`. On resolution/dial failure, return `502` before a success response.

- [x] **Step 5: Run all network proxy tests**

Run: `cargo test -p network-proxy`

Expected: every listener, policy, and decision test passes.

### Task 4: Document, review, verify, and commit

**Files:**
- Modify: `docs/04-详细设计阶段/01-核心引擎层/07-Agent生命周期详细设计.md`
- Modify: `AGENTS.md`

- [x] **Step 1: Update the architecture baseline to v2.27**

Record the real loopback CONNECT enforcement, bounded parser, actual-address recheck, handle lifecycle, and explicit exclusions: plain HTTP forwarding, SOCKS, MITM, environment injection, blocked-request persistence, and orchestrator retry.

- [x] **Step 2: Run layered verification**

```bash
cargo fmt --all -- --check
cargo test -p network-proxy -p types
cargo check --workspace --all-targets
cargo clippy -p network-proxy -p types --all-targets --no-deps -- -D warnings
git diff --check
```

- [x] **Step 3: Request an independent code review and fix Critical/Important findings**

Review only the explicit batch files; focus on parser bounds, DNS rebinding, listener exposure, task shutdown, and policy bypasses.

- [x] **Step 4: Stage explicit files and commit**

```bash
git add -- AGENTS.md Cargo.lock crates/agent-network-proxy \
  'docs/04-详细设计阶段/01-核心引擎层/07-Agent生命周期详细设计.md' \
  docs/superpowers/plans/2026-08-19-network-proxy-connect-listener.md
git commit -m "feat: add managed connect proxy listener"
```
