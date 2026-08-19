//! Codex-compatible policy core for the managed subprocess network proxy.
//!
//! This crate deliberately does not open proxy sockets yet. It owns the host
//! enforcement boundary that a later HTTP/HTTPS/SOCKS listener must call.

mod network_policy;
mod policy;
mod runtime;

pub use network_policy::{
    NetworkDecision, NetworkPolicyDecider, NetworkPolicyDeciderFuture, NetworkPolicyRequest,
    NetworkPolicyRequestArgs, NetworkProtocol,
};
pub use policy::{is_loopback_host, is_non_public_ip, normalize_host, Host};
pub use runtime::{HostBlockDecision, HostBlockReason, NetworkProxyState};
pub use types::{NetworkDecisionSource, NetworkPolicyDecision};
