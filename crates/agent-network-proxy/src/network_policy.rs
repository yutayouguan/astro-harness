use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use types::{
    NetworkApprovalProtocol, NetworkDecisionSource, NetworkPolicyDecision,
    NetworkPolicyDecisionPayload,
};

const REASON_POLICY_DENIED: &str = "policy_denied";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkProtocol {
    Http,
    HttpsConnect,
    Socks5Tcp,
    Socks5Udp,
}

impl NetworkProtocol {
    pub const fn as_policy_protocol(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::HttpsConnect => "https_connect",
            Self::Socks5Tcp => "socks5_tcp",
            Self::Socks5Udp => "socks5_udp",
        }
    }

    pub const fn approval_protocol(self) -> NetworkApprovalProtocol {
        match self {
            Self::Http => NetworkApprovalProtocol::Http,
            Self::HttpsConnect => NetworkApprovalProtocol::Https,
            Self::Socks5Tcp => NetworkApprovalProtocol::Socks5Tcp,
            Self::Socks5Udp => NetworkApprovalProtocol::Socks5Udp,
        }
    }
}

#[derive(Clone, Debug)]
pub struct NetworkPolicyRequest {
    pub protocol: NetworkProtocol,
    pub host: String,
    pub port: u16,
    pub environment_id: Option<String>,
    pub client_addr: Option<String>,
    pub method: Option<String>,
    pub command: Option<String>,
    pub exec_policy_hint: Option<String>,
    pub execution_id: Option<String>,
}

pub struct NetworkPolicyRequestArgs {
    pub protocol: NetworkProtocol,
    pub host: String,
    pub port: u16,
    pub environment_id: Option<String>,
    pub client_addr: Option<String>,
    pub method: Option<String>,
    pub command: Option<String>,
    pub exec_policy_hint: Option<String>,
}

impl NetworkPolicyRequest {
    pub fn new(args: NetworkPolicyRequestArgs) -> Self {
        Self {
            protocol: args.protocol,
            host: args.host,
            port: args.port,
            environment_id: args.environment_id,
            client_addr: args.client_addr,
            method: args.method,
            command: args.command,
            exec_policy_hint: args.exec_policy_hint,
            execution_id: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetworkDecision {
    Allow,
    Deny {
        reason: String,
        source: NetworkDecisionSource,
        decision: NetworkPolicyDecision,
    },
}

impl NetworkDecision {
    pub fn deny(reason: impl Into<String>) -> Self {
        Self::deny_with_source(reason, NetworkDecisionSource::Decider)
    }

    pub fn ask(reason: impl Into<String>) -> Self {
        Self::ask_with_source(reason, NetworkDecisionSource::Decider)
    }

    pub fn deny_with_source(reason: impl Into<String>, source: NetworkDecisionSource) -> Self {
        Self::blocked(reason, source, NetworkPolicyDecision::Deny)
    }

    pub fn ask_with_source(reason: impl Into<String>, source: NetworkDecisionSource) -> Self {
        Self::blocked(reason, source, NetworkPolicyDecision::Ask)
    }

    fn blocked(
        reason: impl Into<String>,
        source: NetworkDecisionSource,
        decision: NetworkPolicyDecision,
    ) -> Self {
        let reason = reason.into();
        Self::Deny {
            reason: if reason.is_empty() {
                REASON_POLICY_DENIED.to_string()
            } else {
                reason
            },
            source,
            decision,
        }
    }

    pub fn to_policy_decision_payload(
        &self,
        request: &NetworkPolicyRequest,
    ) -> Option<NetworkPolicyDecisionPayload> {
        let Self::Deny {
            reason,
            source,
            decision,
        } = self
        else {
            return None;
        };
        Some(NetworkPolicyDecisionPayload {
            decision: *decision,
            source: *source,
            protocol: Some(request.protocol.approval_protocol()),
            host: Some(request.host.clone()),
            reason: Some(reason.clone()),
            port: Some(request.port),
        })
    }
}

pub trait NetworkPolicyDecider: Send + Sync + 'static {
    fn decide(&self, request: NetworkPolicyRequest) -> NetworkPolicyDeciderFuture<'_>;
}

pub type NetworkPolicyDeciderFuture<'a> =
    Pin<Box<dyn Future<Output = NetworkDecision> + Send + 'a>>;

impl<D: NetworkPolicyDecider + ?Sized> NetworkPolicyDecider for Arc<D> {
    fn decide(&self, request: NetworkPolicyRequest) -> NetworkPolicyDeciderFuture<'_> {
        Box::pin(async move { (**self).decide(request).await })
    }
}

impl<F, Fut> NetworkPolicyDecider for F
where
    F: Fn(NetworkPolicyRequest) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = NetworkDecision> + Send + 'static,
{
    fn decide(&self, request: NetworkPolicyRequest) -> NetworkPolicyDeciderFuture<'_> {
        Box::pin((self)(request))
    }
}

pub(crate) fn map_decider_decision(decision: NetworkDecision) -> NetworkDecision {
    match decision {
        NetworkDecision::Allow => NetworkDecision::Allow,
        NetworkDecision::Deny {
            reason, decision, ..
        } => NetworkDecision::Deny {
            reason,
            source: NetworkDecisionSource::Decider,
            decision,
        },
    }
}
