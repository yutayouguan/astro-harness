//! Structured decisions emitted by the managed subprocess network boundary.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NetworkPolicyDecision {
    Deny,
    Ask,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetworkDecisionSource {
    BaselinePolicy,
    ModeGuard,
    ProxyState,
    Decider,
}

#[derive(Debug, Clone, Copy, Hash, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetworkApprovalProtocol {
    Http,
    #[serde(alias = "https_connect", alias = "http-connect")]
    Https,
    Socks5Tcp,
    Socks5Udp,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct NetworkApprovalContext {
    pub host: String,
    pub protocol: NetworkApprovalProtocol,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetworkPolicyRuleAction {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct NetworkPolicyAmendment {
    pub host: String,
    pub action: NetworkPolicyRuleAction,
}

/// Codex-compatible payload attached to a sandbox denial by a managed proxy.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NetworkPolicyDecisionPayload {
    pub decision: NetworkPolicyDecision,
    pub source: NetworkDecisionSource,
    #[serde(default)]
    pub protocol: Option<NetworkApprovalProtocol>,
    pub host: Option<String>,
    pub reason: Option<String>,
    pub port: Option<u16>,
}

impl NetworkPolicyDecisionPayload {
    pub fn is_ask_from_decider(&self) -> bool {
        self.decision == NetworkPolicyDecision::Ask && self.source == NetworkDecisionSource::Decider
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_accepts_proxy_protocol_aliases() {
        for protocol in ["https_connect", "http-connect"] {
            let payload: NetworkPolicyDecisionPayload = serde_json::from_value(serde_json::json!({
                "decision": "ask",
                "source": "decider",
                "protocol": protocol,
                "host": "example.com",
                "reason": "not_allowed",
                "port": 443
            }))
            .unwrap();

            assert_eq!(payload.protocol, Some(NetworkApprovalProtocol::Https));
            assert!(payload.is_ask_from_decider());
        }
    }

    #[test]
    fn network_approval_context_uses_canonical_wire_contract() {
        let context = NetworkApprovalContext {
            host: "api.example.com".to_string(),
            protocol: NetworkApprovalProtocol::Https,
        };

        assert_eq!(
            serde_json::to_value(&context).unwrap(),
            serde_json::json!({
                "host": "api.example.com",
                "protocol": "https"
            })
        );
    }

    #[test]
    fn network_policy_amendment_roundtrips_canonical_actions() {
        for action in [
            NetworkPolicyRuleAction::Allow,
            NetworkPolicyRuleAction::Deny,
        ] {
            let amendment = NetworkPolicyAmendment {
                host: "api.example.com".to_string(),
                action,
            };
            let encoded = serde_json::to_value(&amendment).unwrap();
            let expected_action = match action {
                NetworkPolicyRuleAction::Allow => "allow",
                NetworkPolicyRuleAction::Deny => "deny",
            };

            assert_eq!(
                encoded,
                serde_json::json!({
                    "host": "api.example.com",
                    "action": expected_action
                })
            );
            assert_eq!(
                serde_json::from_value::<NetworkPolicyAmendment>(encoded).unwrap(),
                amendment
            );
        }
    }
}
