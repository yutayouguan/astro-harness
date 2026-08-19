# Attempt-scoped Managed Network Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Route foreground `terminal action=run` and `code_exec` subprocess traffic through an attempt-scoped managed CONNECT proxy, constrain Seatbelt to that proxy's exact loopback port, and surface proxy policy blocks as typed `SandboxErr::Denied` values.

**Architecture:** `ToolOrchestrator::run` resolves the selected custom profile's own network policy, starts one `StartedNetworkProxy` before constructing the initial `SandboxAttempt`, and carries its `Arc` through `ToolExecutionGrants` into `ToolContext`. `NetworkProxy::prepare` rewrites only the child environment and produces the matching `ManagedNetworkSandboxContext`; `SandboxPolicy` embeds that context so macOS permits only the bound proxy port. Foreground process tools consume the attempt-local blocked-request queue after exit and convert the latest block into the existing Codex-compatible network decision payload. Built-in/full-access profiles, background terminal jobs, other tools, session-scoped listeners, approval/retry, plain HTTP forwarding, SOCKS, MCP, providers, and in-process web tools remain outside this batch.

**Tech Stack:** Rust 2021, Tokio TCP/process runtime, macOS Seatbelt profiles, existing `network-proxy`, `sandbox`, `tools`, `agent`, `memory`, and `types` crates; unit and real-loopback integration tests.

---

### Task 1: Add the attempt-owned proxy contracts and blocked-request queue

**Files:**
- Modify: `crates/agent-network-proxy/Cargo.toml`
- Modify: `crates/agent-network-proxy/src/runtime.rs`
- Modify: `crates/agent-network-proxy/src/proxy.rs`
- Modify: `crates/agent-network-proxy/src/http_proxy.rs`
- Modify: `crates/agent-network-proxy/src/lib.rs`
- Create: `crates/agent-network-proxy/tests/managed_attempt.rs`
- Modify: `crates/agent-network-proxy/tests/http_connect.rs`
- Modify: `Cargo.lock`

- [ ] **Step 1: Write failing public-contract tests**

Create `managed_attempt.rs` with helpers that construct an enabled `NetworkPolicy`, then lock these Codex-aligned contracts:

```rust
use network_proxy::{
    ManagedNetworkSandboxContext, NetworkProxyState, PreparedManagedNetwork,
    StartedNetworkProxy,
};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};
use types::{NetworkAccess, NetworkPolicy};

fn enabled_policy(allow_local_binding: bool) -> NetworkPolicy {
    NetworkPolicy {
        enabled: true,
        domains: BTreeMap::from([("example.com".into(), NetworkAccess::Allow)]),
        allow_local_binding,
        ..NetworkPolicy::default()
    }
}

#[tokio::test]
async fn prepare_overrides_proxy_keys_and_preserves_unrelated_env() {
    let started = StartedNetworkProxy::start(Arc::new(
        NetworkProxyState::new(enabled_policy(false)).unwrap(),
    ))
    .await
    .unwrap();
    let prepared = started.proxy().prepare(HashMap::from([
        ("PATH".into(), "/safe/bin".into()),
        ("HTTPS_PROXY".into(), "http://stale.invalid:1".into()),
        ("NO_PROXY".into(), "localhost".into()),
    ]));
    let endpoint = format!("http://{}", started.proxy().http_addr());

    assert_eq!(prepared.env.get("PATH"), Some(&"/safe/bin".to_string()));
    for key in [
        "HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy", "ALL_PROXY", "all_proxy",
    ] {
        assert_eq!(prepared.env.get(key), Some(&endpoint), "{key}");
    }
    assert_eq!(
        prepared.env.get("ASTRO_NETWORK_PROXY_ACTIVE").map(String::as_str),
        Some("1")
    );
    assert_eq!(prepared.env.get("NO_PROXY"), Some(&String::new()));
    assert_eq!(prepared.env.get("no_proxy"), Some(&String::new()));
    assert_eq!(
        prepared.sandbox_context,
        ManagedNetworkSandboxContext {
            loopback_ports: vec![started.proxy().http_addr().port()],
            allow_local_binding: false,
        }
    );
}

#[tokio::test]
async fn prepare_allows_explicit_local_bypass_only_when_configured() {
    let started = StartedNetworkProxy::start(Arc::new(
        NetworkProxyState::new(enabled_policy(true)).unwrap(),
    ))
    .await
    .unwrap();
    let PreparedManagedNetwork { env, sandbox_context } =
        started.proxy().prepare(HashMap::new());

    assert_eq!(
        env.get("NO_PROXY").map(String::as_str),
        Some("localhost,127.0.0.1,::1,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16")
    );
    assert!(sandbox_context.allow_local_binding);
}

#[tokio::test]
async fn dropping_started_proxy_closes_the_reserved_listener() {
    let started = StartedNetworkProxy::start(Arc::new(
        NetworkProxyState::new(enabled_policy(false)).unwrap(),
    ))
    .await
    .unwrap();
    let address = started.proxy().http_addr();
    drop(started);

    timeout(Duration::from_secs(1), async {
        loop {
            if TcpStream::connect(address).await.is_err() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
```

Extend the existing real CONNECT denial test to call `proxy.take_blocked_requests()` after the 403 and assert exactly one record with `host`, `port`, `HttpsConnect`, `Deny`, `BaselinePolicy`, client address, and `CONNECT`. Extend the rebinding test to assert a `ProxyState` blocked record. Add a `runtime.rs` unit test that records 65 synthetic denials, takes the queue, and verifies it contains entries 1 through 64 in FIFO order. This proves records come from policy enforcement, remain bounded, and do not depend on stderr parsing.

- [ ] **Step 2: Run the new tests and verify RED**

Run:

```bash
cargo test -p network-proxy --test managed_attempt
cargo test -p network-proxy --test http_connect connect_denial
```

Expected: compile failures for missing `StartedNetworkProxy`, `ManagedNetworkSandboxContext`, `PreparedManagedNetwork`, `prepare`, and blocked-request APIs.

- [ ] **Step 3: Implement the portable types and bounded FIFO**

Add to `runtime.rs`:

```rust
use std::collections::VecDeque;
use std::sync::Mutex;

const MAX_BLOCKED_REQUESTS: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockedRequest {
    pub host: String,
    pub port: u16,
    pub protocol: NetworkProtocol,
    pub reason: String,
    pub decision: NetworkPolicyDecision,
    pub source: NetworkDecisionSource,
    pub client_addr: Option<String>,
    pub method: Option<String>,
}

impl BlockedRequest {
    pub fn to_policy_decision_payload(&self) -> types::NetworkPolicyDecisionPayload {
        let protocol = match self.protocol {
            NetworkProtocol::Http => types::NetworkApprovalProtocol::Http,
            NetworkProtocol::HttpsConnect => types::NetworkApprovalProtocol::Https,
            NetworkProtocol::Socks5Tcp => types::NetworkApprovalProtocol::Socks5Tcp,
            NetworkProtocol::Socks5Udp => types::NetworkApprovalProtocol::Socks5Udp,
        };
        types::NetworkPolicyDecisionPayload {
            decision: self.decision,
            source: self.source,
            protocol: Some(protocol),
            host: Some(self.host.clone()),
            reason: Some(self.reason.clone()),
            port: Some(self.port),
        }
    }
}
```

Also implement `BlockedRequest::from_denial(request: &NetworkPolicyRequest, decision: &NetworkDecision) -> Option<Self>` by copying `host`, `port`, `protocol`, `client_addr`, and `method` from the request and `reason`, `decision`, and `source` from the deny variant. Add `blocked_requests: Mutex<VecDeque<BlockedRequest>>` to `NetworkProxyState`. Implement synchronous `record_blocked_request` and `take_blocked_requests`; on insert, pop the oldest item whenever the queue is already at 64. A poisoned mutex returns its inner queue so a reporting path cannot panic after the network decision has already been enforced.

- [ ] **Step 4: Record both policy and rebinding denials**

In `http_proxy.rs`, construct the `BlockedRequest` from the already-normalized `NetworkPolicyRequest` and `NetworkDecision::Deny` before writing the 403. For `ConnectError::PolicyDenied`, record the same request with:

```rust
BlockedRequest {
    host: policy_request.host.clone(),
    port: policy_request.port,
    protocol: policy_request.protocol,
    reason: HostBlockReason::NotAllowedLocal.as_str().to_string(),
    decision: NetworkPolicyDecision::Deny,
    source: NetworkDecisionSource::ProxyState,
    client_addr: policy_request.client_addr.clone(),
    method: policy_request.method.clone(),
}
```

Do not record malformed requests, overloads, DNS failures, upstream connection failures, or 502 responses as policy denials.

- [ ] **Step 5: Implement environment preparation and listener ownership**

Add to `proxy.rs`:

```rust
pub const DEFAULT_NO_PROXY_VALUE: &str = concat!(
    "localhost,127.0.0.1,::1,",
    "10.0.0.0/8,",
    "172.16.0.0/12,",
    "192.168.0.0/16"
);

#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ManagedNetworkSandboxContext {
    pub loopback_ports: Vec<u16>,
    pub allow_local_binding: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedManagedNetwork {
    pub env: HashMap<String, String>,
    pub sandbox_context: ManagedNetworkSandboxContext,
}

pub struct StartedNetworkProxy {
    proxy: NetworkProxy,
    _handle: NetworkProxyHandle,
}
```

`NetworkProxy::prepare(mut env)` must always overwrite upper/lowercase HTTP, HTTPS, ALL_PROXY, and NO_PROXY keys, insert `ASTRO_NETWORK_PROXY_ACTIVE=1`, preserve all unrelated entries, and return the bound HTTP port in `sandbox_context`. `StartedNetworkProxy::start(state)` performs `NetworkProxy::builder().state(state).build().await?` followed by `proxy.run().await?`; `proxy(&self) -> &NetworkProxy` exposes only the running proxy. Re-export all four public contracts from `lib.rs`.

Add `serde = { workspace = true }` to `agent-network-proxy`; the portable sandbox context is serialized as part of `SandboxPolicy` audits.

- [ ] **Step 6: Run, format, and commit Task 1**

```bash
cargo fmt --all
cargo test -p network-proxy
cargo clippy -p network-proxy --all-targets --no-deps -- -D warnings
git diff --check
git add -- Cargo.lock crates/agent-network-proxy
git commit -m "feat: add managed network attempt contracts"
```

Expected: all `network-proxy` tests pass; only explicit proxy files are committed.

### Task 2: Constrain the OS sandbox to exact managed proxy ports

**Files:**
- Modify: `crates/agent-sandbox/Cargo.toml`
- Modify: `crates/agent-sandbox/src/lib.rs`
- Modify: `Cargo.lock`

- [ ] **Step 1: Write failing sandbox policy tests**

Add inline tests beside the existing Seatbelt tests:

```rust
#[test]
fn managed_network_profile_allows_only_exact_proxy_port() {
    let root = tempfile::tempdir().unwrap();
    let policy = SandboxPolicy::new(SandboxMode::WorkspaceWrite, root.path(), [], false)
        .unwrap()
        .with_managed_network(ManagedNetworkSandboxContext {
            loopback_ports: vec![43117],
            allow_local_binding: false,
        });
    let profile = macos_profile(&policy);

    assert!(profile.contains(
        "(allow network-outbound (remote ip \"localhost:43117\"))"
    ));
    assert!(!profile.contains("(allow network*)"));
    assert!(!profile.contains("localhost:*"));
    assert!(!profile.contains("network-bind"));
}

#[test]
fn local_binding_adds_only_minimal_loopback_rules() {
    let root = tempfile::tempdir().unwrap();
    let policy = SandboxPolicy::new(SandboxMode::WorkspaceWrite, root.path(), [], false)
        .unwrap()
        .with_managed_network(ManagedNetworkSandboxContext {
            loopback_ports: vec![43117],
            allow_local_binding: true,
        });
    let profile = macos_profile(&policy);

    assert!(profile.contains("(allow network-bind (local ip \"*:*\"))"));
    assert!(profile.contains("(allow network-inbound (local ip \"localhost:*\"))"));
    assert!(profile.contains("(allow network-outbound (remote ip \"localhost:*\"))"));
    assert!(profile.contains("(allow network-outbound (remote ip \"*:53\"))"));
    assert!(!profile.contains("(allow network*)"));
}

#[test]
fn managed_proxy_port_changes_policy_hash() {
    let root = tempfile::tempdir().unwrap();
    let first = SandboxPolicy::new(SandboxMode::ReadOnly, root.path(), [], false)
        .unwrap()
        .with_managed_network(ManagedNetworkSandboxContext {
            loopback_ports: vec![41001],
            allow_local_binding: false,
        });
    let second = SandboxPolicy::new(SandboxMode::ReadOnly, root.path(), [], false)
        .unwrap()
        .with_managed_network(ManagedNetworkSandboxContext {
            loopback_ports: vec![41002],
            allow_local_binding: false,
        });
    assert_ne!(first.profile_hash_material(), second.profile_hash_material());
}
```

- [ ] **Step 2: Run the focused tests and verify RED**

Run:

```bash
cargo test -p sandbox managed_network_profile
cargo test -p sandbox managed_proxy_port_changes_policy_hash
```

Expected: compile failures for the missing dependency, field, and `with_managed_network` method.

- [ ] **Step 3: Extend `SandboxPolicy` without changing legacy behavior**

Add `network-proxy = { path = "../agent-network-proxy" }` to the sandbox crate and import `ManagedNetworkSandboxContext`. Extend the policy:

```rust
pub struct SandboxPolicy {
    pub mode: SandboxMode,
    pub writable_roots: Vec<PathBuf>,
    pub network_access: bool,
    pub managed_network: Option<ManagedNetworkSandboxContext>,
}

pub fn with_managed_network(mut self, context: ManagedNetworkSandboxContext) -> Self {
    self.network_access = false;
    let mut loopback_ports = context
        .loopback_ports
        .into_iter()
        .filter(|port| *port != 0)
        .collect::<Vec<_>>();
    loopback_ports.sort_unstable();
    loopback_ports.dedup();
    self.managed_network = Some(ManagedNetworkSandboxContext {
        loopback_ports,
        allow_local_binding: context.allow_local_binding,
    });
    self
}
```

All existing constructors set `managed_network: None`. Include the sorted/deduplicated ports and `allow_local_binding` flag in `profile_hash_material`; do not include environment variables or the proxy URL.

- [ ] **Step 4: Generate exact-port Seatbelt rules**

In `macos_profile`, preserve existing `(allow network*)` only for legacy `network_access=true` policies that have no managed context. For a managed context emit:

```text
(allow network-outbound (remote ip "localhost:<port>"))
```

for every sorted/deduplicated non-zero port. When `allow_local_binding=true`, additionally emit exactly the four official Codex-compatible local rules covered by the test (bind, loopback inbound, loopback outbound, DNS). Never emit unrestricted network access in the managed branch.

- [ ] **Step 5: Run regression tests and commit Task 2**

```bash
cargo fmt --all
cargo test -p sandbox -p network-proxy
cargo clippy -p sandbox -p network-proxy --all-targets --no-deps -- -D warnings
git diff --check
git add -- Cargo.lock crates/agent-sandbox
git commit -m "feat: restrict sandbox to managed proxy ports"
```

Expected: prior full-access/network-access tests remain green and exact-port tests pass.

### Task 3: Start and propagate one managed proxy per process attempt

**Files:**
- Modify: `crates/agent-core/Cargo.toml`
- Modify: `crates/agent-core/src/streaming/tools_exec.rs`
- Modify: `crates/agent-core/src/runtime/tool_dispatch.rs`
- Modify: `crates/agent-tools/Cargo.toml`
- Modify: `crates/agent-tools/src/engine/context.rs`
- Modify: `crates/agent-tools/src/engine/dispatch.rs`
- Modify: `crates/agent-tools/src/builtin/hitl/switch_mode.rs`
- Modify: `crates/agent-tools/src/builtin/media/audio_understand.rs`
- Modify: `crates/agent-tools/src/builtin/media/image_gen.rs`
- Modify: `crates/agent-tools/src/builtin/media/video_understand.rs`
- Modify: `crates/agent-tools/src/builtin/shell/code_exec.rs`
- Modify: `crates/agent-tools/src/builtin/shell/file_ops.rs`
- Modify: `crates/agent-tools/src/builtin/shell/terminal.rs`
- Modify: `crates/agent-tools/tests/hitl_tools_test.rs`
- Modify: `crates/agent-tools/tests/tools_test.rs`
- Modify: `Cargo.lock`

- [ ] **Step 1: Add failing profile-selection and propagation tests**

Add pure tests in `tools_exec.rs` for a helper named `managed_network_policy_for_call`:

```rust
#[test]
fn managed_network_uses_only_selected_custom_leaf_policy() {
    let mut settings = memory::LoadedPermissionSettings::default();
    settings.network_proxy_enabled = true;
    settings.selection.profile_id = "leaf".into();
    settings.permissions.profiles.insert(
        "parent".into(),
        types::PermissionProfile {
            network: types::NetworkPolicy { enabled: true, ..Default::default() },
            ..Default::default()
        },
    );
    settings.permissions.profiles.insert(
        "leaf".into(),
        types::PermissionProfile {
            extends: Some("parent".into()),
            network: types::NetworkPolicy::default(),
            ..Default::default()
        },
    );
    let terminal = types::ParsedToolCall::new("terminal", serde_json::json!({"command":"true"}));

    assert!(managed_network_policy_for_call(&terminal, &settings, "leaf").is_none());
    settings.permissions.profiles.get_mut("leaf").unwrap().network.enabled = true;
    assert!(managed_network_policy_for_call(&terminal, &settings, "leaf").is_some());
}
```

Add cases proving all of the following return `None`: global proxy disabled, built-in `:danger-full-access`, unknown profile, `terminal action=status`, and a non-process tool. Add positive cases for default/explicit `terminal action=run` and `code_exec`.

Add a `tools_exec.rs` test that builds a `SandboxAttempt` with `Some(Arc<StartedNetworkProxy>)`, calls `sandbox_policy_for_attempt`, and asserts the result contains the exact bound port and local-binding flag. Run the same assertion with an escalated filesystem policy to prove the early override path cannot drop the managed network context. Add a `tool_dispatch.rs` test that `ToolExecutionGrants::default().managed_network` is `None`; the real dispatch path must copy the attempt lease into `ToolContext` rather than reconstructing it from env.

- [ ] **Step 2: Run focused tests and verify RED**

```bash
cargo test -p agent managed_network_uses_only_selected_custom_leaf_policy
cargo test -p agent sandbox_policy_for_attempt_carries_managed_network_context
cargo test -p agent tool_execution_grants_default_to_no_managed_network
```

Expected: compile failures for the helper and `managed_network` fields.

- [ ] **Step 3: Add the attempt lease across all three boundaries**

Add `network-proxy` dependencies to `agent-core` and `agent-tools`. Extend the contracts exactly once:

```rust
struct SandboxAttempt {
    workspace_write_grant: bool,
    sandbox_policy: Option<sandbox::SandboxPolicy>,
    network_grant: tools::InProcessNetworkGrant,
    managed_network: Option<Arc<network_proxy::StartedNetworkProxy>>,
}

pub(crate) struct ToolExecutionGrants {
    pub(crate) workspace_write: bool,
    pub(crate) sandbox_policy: Option<sandbox::SandboxPolicy>,
    pub(crate) network: tools::InProcessNetworkGrant,
    pub(crate) managed_network: Option<Arc<network_proxy::StartedNetworkProxy>>,
}

pub struct ToolContext<'a> {
    // existing fields unchanged
    pub managed_network: Option<Arc<network_proxy::StartedNetworkProxy>>,
}
```

`SandboxAttempt::escalated` must clone the same lease; it must not start a second listener. Add `managed_network: None` to every direct `ToolContext` fixture and preserve `Default` behavior for `ToolExecutionGrants`.

- [ ] **Step 4: Resolve and start the listener before sandbox policy construction**

Implement:

```rust
fn managed_network_policy_for_call(
    call: &types::ParsedToolCall,
    settings: &memory::LoadedPermissionSettings,
    active_profile_id: &str,
) -> Option<types::NetworkPolicy> {
    if !settings.network_proxy_enabled
        || active_profile_id == types::DANGER_FULL_ACCESS_PROFILE
        || !call_uses_managed_network(call)
    {
        return None;
    }
    settings
        .permissions
        .profiles
        .get(active_profile_id)
        .map(|profile| profile.network.clone())
        .filter(|policy| policy.enabled)
}
```

`call_uses_managed_network` returns true only for `code_exec` and `terminal` whose action is absent/empty/`run`; it returns false for `list/status/poll/wait/kill` and all other tools.

At the start of async `ToolOrchestrator::run`, after permission preflight but before `SandboxAttempt::initial`, load one immutable permission snapshot, resolve the session override profile id, and call:

```rust
let managed_network = match managed_network_policy_for_call(call, &settings, &profile_id) {
    Some(policy) => Some(Arc::new(
        network_proxy::StartedNetworkProxy::start(Arc::new(
            network_proxy::NetworkProxyState::new(policy)?,
        ))
        .await?,
    )),
    None => None,
};
```

Proxy setup errors return an execution error before `run_attempt`; never fall back to an unproxied child.

- [ ] **Step 5: Attach the already-bound port to the initial sandbox policy**

In `sandbox_policy_for_attempt`, refactor the current early return for `attempt.sandbox_policy` into base-policy selection. After resolving either the attempt override or the normal filesystem/mode policy, attach:

```rust
if let Some(started) = &attempt.managed_network {
    let prepared = started.proxy().prepare(HashMap::new());
    policy = policy.with_managed_network(prepared.sandbox_context);
}
```

Then pass the same lease through `run_attempt` into `ToolExecutionGrants` and from `dispatch_named_tool` into `ToolContext`. Do not alter `network_grant`; it remains the separate in-process HTTP grant.

- [ ] **Step 6: Run propagation regressions and commit Task 3**

```bash
cargo fmt --all
cargo test -p agent managed_network
cargo test -p tools context
cargo check -p agent -p tools --all-targets
cargo clippy -p agent -p tools --all-targets --no-deps -- -D warnings
git diff --check
git add -- Cargo.lock crates/agent-core/Cargo.toml crates/agent-core/src/streaming/tools_exec.rs \
  crates/agent-core/src/runtime/tool_dispatch.rs crates/agent-tools/Cargo.toml \
  crates/agent-tools/src/engine/context.rs crates/agent-tools/src/engine/dispatch.rs \
  crates/agent-tools/src/builtin/hitl/switch_mode.rs \
  crates/agent-tools/src/builtin/media/audio_understand.rs \
  crates/agent-tools/src/builtin/media/image_gen.rs \
  crates/agent-tools/src/builtin/media/video_understand.rs \
  crates/agent-tools/src/builtin/shell/code_exec.rs \
  crates/agent-tools/src/builtin/shell/file_ops.rs \
  crates/agent-tools/src/builtin/shell/terminal.rs \
  crates/agent-tools/tests/hitl_tools_test.rs crates/agent-tools/tests/tools_test.rs
git commit -m "feat: propagate managed network attempt leases"
```

Before staging, inspect `git diff --name-only`; every staged path must appear in this task's file list. Expected: no proxy is started for disabled/global/full-access/non-process cases, and all legacy contexts compile with `None`.

### Task 4: Inject proxy env and convert blocked requests into typed denials

**Files:**
- Modify: `crates/agent-tools/src/engine/context.rs`
- Modify: `crates/agent-tools/src/builtin/shell/terminal.rs`
- Modify: `crates/agent-tools/src/builtin/shell/code_exec.rs`

- [ ] **Step 1: Write failing terminal and code-exec behavior tests**

Add inline async tests using a real `StartedNetworkProxy` and a custom `ToolContext`:

1. Foreground terminal executes `printf '%s' "$HTTPS_PROXY"` and returns the current `http://127.0.0.1:<port>` endpoint.
2. `terminal background=true` returns an error containing `managed network does not support background jobs` and `jobs::list` remains empty for the session.
3. A terminal CONNECT to a denied loopback target returns an `anyhow::Error` that downcasts to `SandboxErr::Denied` with host, port, `Deny`, and `BaselinePolicy`.
4. `code_exec` prints the proxy key and a sentinel safe key while a fake API token remains absent, proving the order is scrub first, managed env second.
5. With `managed_network=None`, existing terminal and code-exec output is unchanged.

For the typed denial test, invoke a portable client from the child using Python's socket library to send a raw CONNECT request to `$HTTPS_PROXY`; this avoids depending on `curl`. Strip the `http://` prefix in the test script and read the 403 before exit.

- [ ] **Step 2: Run focused tests and verify RED**

```bash
cargo test -p tools terminal_uses_managed_proxy_environment
cargo test -p tools terminal_managed_network_denial_is_typed
cargo test -p tools code_exec_adds_proxy_after_secret_scrub
```

Expected: environment assertions fail; denial is still ordinary output; background currently starts.

- [ ] **Step 3: Add `ToolContext` helpers with one denial conversion point**

Implement helpers:

```rust
pub fn prepare_managed_network_env(
    &self,
    env: HashMap<String, String>,
) -> Option<network_proxy::PreparedManagedNetwork> {
    self.managed_network
        .as_ref()
        .map(|started| started.proxy().prepare(env))
}

pub fn take_managed_network_denial(
    &self,
) -> Option<types::NetworkPolicyDecisionPayload> {
    self.managed_network
        .as_ref()?
        .proxy()
        .take_blocked_requests()
        .pop()
        .map(|blocked| blocked.to_policy_decision_payload())
}
```

The queue is drained per attempt; choosing `.pop()` reports the latest denial deterministically when a process made several blocked requests.

- [ ] **Step 4: Wire foreground terminal and reject background before spawn**

Immediately after resolving cwd, reject `background=true` when `ctx.managed_network.is_some()`. For foreground execution, collect `std::env::vars()` into a `HashMap`, pass it through `prepare_managed_network_env`, and apply the complete returned environment with `env_clear().envs(env)` before spawn. This makes stale inherited proxy variables impossible.

After `wait_with_output` and before the filesystem denial classifier or transform hook, check `take_managed_network_denial()`. If present, audit `network_policy_denied` and return:

```rust
return Err(sandbox::SandboxErr::Denied {
    output: Box::new(output),
    network_policy_decision: Some(decision),
}
.into());
```

Do not classify 502/upstream failures as policy denials and do not request filesystem escalation.

- [ ] **Step 5: Wire `code_exec` after the existing secret scrub**

Preserve the current `safe_child_env()`/scrub result. Only after that map is finalized, call `prepare_managed_network_env`; pass the returned complete map to the existing `env_clear().envs(...)` path. After process exit, perform the same blocked-request check before filesystem classification. Never copy missing parent keys back into the scrubbed map.

- [ ] **Step 6: Prove the orchestrator skips filesystem retry for network denial**

Add/extend the existing `tools_exec.rs` test around `OrchestratorRunResult::SandboxDenied` so a denial containing `network_policy_decision: Some(...)` finalizes immediately and the test executor records exactly one attempt. Assert `review_sandbox_denial` is not invoked and no unrestricted filesystem policy is constructed.

- [ ] **Step 7: Run tool/core regressions and commit Task 4**

```bash
cargo fmt --all
cargo test -p tools terminal
cargo test -p tools code_exec
cargo test -p agent network_denial
cargo check -p tools -p agent --all-targets
cargo clippy -p tools -p agent --all-targets --no-deps -- -D warnings
git diff --check
git add -- crates/agent-tools/src/engine/context.rs \
  crates/agent-tools/src/builtin/shell/terminal.rs \
  crates/agent-tools/src/builtin/shell/code_exec.rs \
  crates/agent-core/src/streaming/tools_exec.rs
git commit -m "feat: enforce managed network for process tools"
```

Expected: foreground process tools receive only the managed endpoint, blocked requests become typed denials, background jobs fail closed, and orchestrator attempt count stays one.

### Task 5: Document the landed boundary and run full verification

**Files:**
- Modify: `docs/04-详细设计阶段/01-核心引擎层/07-Agent生命周期详细设计.md`
- Modify: `AGENTS.md`
- Modify: `docs/superpowers/plans/2026-08-19-attempt-scoped-managed-network-integration.md`

- [ ] **Step 1: Update the architecture baseline to v2.28**

Document the real call chain:

```text
ToolOrchestrator::run
  -> StartedNetworkProxy::start
  -> SandboxAttempt.managed_network
  -> SandboxPolicy.managed_network exact port
  -> ToolExecutionGrants
  -> ToolContext
  -> terminal/code_exec prepared env
  -> BlockedRequest
  -> SandboxErr::Denied.network_policy_decision
```

Record these invariants explicitly: attempt ownership, global-plus-leaf-profile gate, no inherited network policy, no full-access proxy, no background lease, no in-process/MCP/provider change, no approval/retry, and 502 is not a policy denial. Update the `AGENTS.md` lifecycle summary and crate map only where the implementation is now true.

- [ ] **Step 2: Run the full layered verification**

```bash
cargo test -p network-proxy -p sandbox -p tools -p agent -p types -p memory
cargo check --workspace --all-targets
cargo clippy -p network-proxy -p sandbox -p tools -p agent -p types -p memory \
  --all-targets --no-deps -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Expected: every command exits 0. If an unrelated workspace target fails, capture the exact pre-existing failure and still keep all six changed crates green; do not weaken or skip a batch-owned test.

- [ ] **Step 3: Perform security-focused manual checks**

Run these read-only inspections:

```bash
rg -n '\(allow network\*\)' crates/agent-sandbox/src/lib.rs
rg -n 'managed_network|StartedNetworkProxy' crates/agent-core crates/agent-tools crates/agent-sandbox
rg -n 'ToolContext \{' crates/agent-tools
git diff --stat
git diff --check
```

Confirm unrestricted Seatbelt remains only in the legacy `network_access=true` branch, every `ToolContext` fixture explicitly chooses `None` or a test lease, and no session/global proxy state was introduced.

- [ ] **Step 4: Request independent review and fix all Critical/Important findings**

Use `superpowers:requesting-code-review`. Review only the explicit batch commits and focus on direct-connect bypass, stale inherited proxy variables, DNS rebinding attribution, exact-port Seatbelt rules, listener lifetime, background escape, typed error ordering, and filesystem retry suppression. Re-run the affected crate tests after each correction.

- [ ] **Step 5: Mark this plan complete and commit documentation**

Change every completed checkbox in this file to `[x]`, then:

```bash
git add -- AGENTS.md \
  'docs/04-详细设计阶段/01-核心引擎层/07-Agent生命周期详细设计.md' \
  docs/superpowers/plans/2026-08-19-attempt-scoped-managed-network-integration.md
git commit -m "docs: record managed network attempt lifecycle"
```

Do not stage the unrelated untracked requirements/design directories. Do not push.
