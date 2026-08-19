use crate::policy::{
    compile_allowlist_globset, compile_denylist_globset, is_loopback_host, is_non_public_ip,
    normalize_host, unscoped_ip_literal, Host,
};
use crate::{NetworkDecision, NetworkPolicyDecider, NetworkPolicyRequest, NetworkProtocol};
use anyhow::{ensure, Result};
use globset::GlobSet;
use std::collections::VecDeque;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::lookup_host;
use tokio::time::timeout;
use types::{
    NetworkAccess, NetworkApprovalProtocol, NetworkDecisionSource, NetworkPolicy,
    NetworkPolicyDecision, NetworkPolicyDecisionPayload,
};

const DNS_LOOKUP_TIMEOUT: Duration = Duration::from_secs(2);
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
    pub(crate) fn from_denial(
        request: &NetworkPolicyRequest,
        decision: &NetworkDecision,
    ) -> Option<Self> {
        let NetworkDecision::Deny {
            reason,
            source,
            decision,
        } = decision
        else {
            return None;
        };
        Some(Self {
            host: request.host.clone(),
            port: request.port,
            protocol: request.protocol,
            reason: reason.clone(),
            decision: *decision,
            source: *source,
            client_addr: request.client_addr.clone(),
            method: request.method.clone(),
        })
    }

    pub(crate) fn rebinding_denial(request: &NetworkPolicyRequest) -> Self {
        Self {
            host: request.host.clone(),
            port: request.port,
            protocol: request.protocol,
            reason: HostBlockReason::NotAllowedLocal.as_str().to_string(),
            decision: NetworkPolicyDecision::Deny,
            source: NetworkDecisionSource::ProxyState,
            client_addr: request.client_addr.clone(),
            method: request.method.clone(),
        }
    }

    pub fn to_policy_decision_payload(&self) -> NetworkPolicyDecisionPayload {
        let protocol = match self.protocol {
            NetworkProtocol::Http => NetworkApprovalProtocol::Http,
            NetworkProtocol::HttpsConnect => NetworkApprovalProtocol::Https,
            NetworkProtocol::Socks5Tcp => NetworkApprovalProtocol::Socks5Tcp,
            NetworkProtocol::Socks5Udp => NetworkApprovalProtocol::Socks5Udp,
        };
        NetworkPolicyDecisionPayload {
            decision: self.decision,
            source: self.source,
            protocol: Some(protocol),
            host: Some(self.host.clone()),
            reason: Some(self.reason.clone()),
            port: Some(self.port),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostBlockReason {
    Denied,
    NotAllowed,
    NotAllowedLocal,
}

impl HostBlockReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Denied => "denied",
            Self::NotAllowed => "not_allowed",
            Self::NotAllowedLocal => "not_allowed_local",
        }
    }
}

impl std::fmt::Display for HostBlockReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostBlockDecision {
    Allowed,
    Blocked(HostBlockReason),
}

#[derive(Debug)]
pub struct NetworkProxyState {
    allowed_domains: Vec<String>,
    allow_set: GlobSet,
    deny_set: GlobSet,
    allow_local_binding: bool,
    blocked_requests: Mutex<VecDeque<BlockedRequest>>,
}

impl NetworkProxyState {
    pub fn new(policy: NetworkPolicy) -> Result<Self> {
        ensure!(policy.enabled, "network proxy is disabled");
        let allowed_domains = policy
            .domains
            .iter()
            .filter(|(_, access)| **access == NetworkAccess::Allow)
            .map(|(domain, _)| domain.clone())
            .collect::<Vec<_>>();
        let denied_domains = policy
            .domains
            .iter()
            .filter(|(_, access)| **access == NetworkAccess::Deny)
            .map(|(domain, _)| domain.clone())
            .collect::<Vec<_>>();
        Ok(Self {
            allow_set: compile_allowlist_globset(&allowed_domains)?,
            deny_set: compile_denylist_globset(&denied_domains)?,
            allowed_domains,
            allow_local_binding: policy.allow_local_binding,
            blocked_requests: Mutex::new(VecDeque::new()),
        })
    }

    pub async fn host_blocked(&self, host: &str, port: u16) -> Result<HostBlockDecision> {
        let host = match Host::parse(host) {
            Ok(host) => host,
            Err(_) => return Ok(HostBlockDecision::Blocked(HostBlockReason::NotAllowed)),
        };
        let host_str = host.as_str();
        if globset_matches_host_or_unscoped(&self.deny_set, host_str) {
            return Ok(HostBlockDecision::Blocked(HostBlockReason::Denied));
        }

        let is_allowlisted = globset_matches_host_or_unscoped(&self.allow_set, host_str);
        if !self.allow_local_binding {
            let host_no_scope = unscoped_ip_literal(host_str).unwrap_or(host_str);
            let local_literal = is_loopback_host(&host)
                || host_no_scope.parse::<IpAddr>().is_ok_and(is_non_public_ip);
            if local_literal {
                if !is_explicit_local_allowlisted(&self.allowed_domains, &host) {
                    return Ok(HostBlockDecision::Blocked(HostBlockReason::NotAllowedLocal));
                }
            } else if host_resolves_to_non_public_ip(host_str, port).await {
                return Ok(HostBlockDecision::Blocked(HostBlockReason::NotAllowedLocal));
            }
        }

        if self.allowed_domains.is_empty() || !is_allowlisted {
            Ok(HostBlockDecision::Blocked(HostBlockReason::NotAllowed))
        } else {
            Ok(HostBlockDecision::Allowed)
        }
    }

    pub fn allow_local_binding(&self) -> bool {
        self.allow_local_binding
    }

    pub(crate) fn record_blocked_request(&self, request: BlockedRequest) {
        let mut requests = self
            .blocked_requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if requests.len() == MAX_BLOCKED_REQUESTS {
            requests.pop_front();
        }
        requests.push_back(request);
    }

    pub fn take_blocked_requests(&self) -> Vec<BlockedRequest> {
        let mut requests = self
            .blocked_requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        requests.drain(..).collect()
    }

    pub async fn evaluate_host_policy(
        &self,
        decider: Option<&Arc<dyn NetworkPolicyDecider>>,
        request: &NetworkPolicyRequest,
    ) -> Result<NetworkDecision> {
        let decision = match self.host_blocked(&request.host, request.port).await? {
            HostBlockDecision::Allowed => NetworkDecision::Allow,
            HostBlockDecision::Blocked(HostBlockReason::NotAllowed) => {
                if let Some(decider) = decider {
                    crate::network_policy::map_decider_decision(
                        decider.decide(request.clone()).await,
                    )
                } else {
                    NetworkDecision::deny_with_source(
                        HostBlockReason::NotAllowed.as_str(),
                        NetworkDecisionSource::BaselinePolicy,
                    )
                }
            }
            HostBlockDecision::Blocked(reason) => NetworkDecision::deny_with_source(
                reason.as_str(),
                NetworkDecisionSource::BaselinePolicy,
            ),
        };
        Ok(decision)
    }
}

fn globset_matches_host_or_unscoped(set: &GlobSet, host: &str) -> bool {
    set.is_match(host) || unscoped_ip_literal(host).is_some_and(|ip| set.is_match(ip))
}

fn is_explicit_local_allowlisted(allowed_domains: &[String], host: &Host) -> bool {
    let normalized_host = host.as_str();
    let unscoped_host = unscoped_ip_literal(normalized_host);
    allowed_domains.iter().any(|pattern| {
        let pattern = pattern.trim();
        if pattern == "*"
            || pattern.starts_with("*.")
            || pattern.starts_with("**.")
            || pattern.contains('*')
            || pattern.contains('?')
        {
            return false;
        }
        let normalized_pattern = normalize_host(pattern);
        normalized_pattern == normalized_host
            || unscoped_host.is_some_and(|ip| normalized_pattern == ip)
    })
}

async fn host_resolves_to_non_public_ip(host: &str, port: u16) -> bool {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_non_public_ip(ip);
    }
    let addresses = match timeout(DNS_LOOKUP_TIMEOUT, lookup(host, port)).await {
        Ok(Ok(addresses)) => addresses,
        Ok(Err(_)) | Err(_) => return true,
    };
    addresses
        .into_iter()
        .any(|address| is_non_public_ip(address.ip()))
}

async fn lookup(host: &str, port: u16) -> std::io::Result<Vec<SocketAddr>> {
    Ok(lookup_host((host, port)).await?.collect())
}
