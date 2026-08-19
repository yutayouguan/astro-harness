//! Codex-compatible policy core for the managed subprocess network proxy.
//!
//! The crate owns the managed-network enforcement boundary and a loopback-only
//! HTTP/1 CONNECT listener. Plain HTTP forwarding and SOCKS are not implemented.

mod connect_policy;
mod http_proxy;
mod network_policy;
mod policy;
mod proxy;
mod runtime;

pub use network_policy::{
    NetworkDecision, NetworkPolicyDecider, NetworkPolicyDeciderFuture, NetworkPolicyRequest,
    NetworkPolicyRequestArgs, NetworkProtocol,
};
pub use policy::{is_loopback_host, is_non_public_ip, normalize_host, Host};
pub use proxy::{
    ManagedNetworkSandboxContext, NetworkProxy, NetworkProxyBuilder, NetworkProxyHandle,
    PreparedManagedNetwork, StartedNetworkProxy, DEFAULT_NO_PROXY_VALUE, PROXY_ACTIVE_ENV_KEY,
};
pub use runtime::{BlockedRequest, HostBlockDecision, HostBlockReason, NetworkProxyState};
pub use types::{NetworkDecisionSource, NetworkPolicyDecision};
