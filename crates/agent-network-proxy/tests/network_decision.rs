use network_proxy::{
    NetworkDecision, NetworkPolicyDecider, NetworkPolicyRequest, NetworkPolicyRequestArgs,
    NetworkProtocol, NetworkProxyState,
};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use types::{
    NetworkAccess, NetworkApprovalProtocol, NetworkDecisionSource, NetworkPolicy,
    NetworkPolicyDecision,
};

fn state() -> NetworkProxyState {
    NetworkProxyState::new(NetworkPolicy {
        enabled: true,
        domains: BTreeMap::from([("allowed.example".to_string(), NetworkAccess::Allow)]),
        allow_local_binding: true,
        ..NetworkPolicy::default()
    })
    .unwrap()
}

fn request(host: &str) -> NetworkPolicyRequest {
    NetworkPolicyRequest::new(NetworkPolicyRequestArgs {
        protocol: NetworkProtocol::HttpsConnect,
        host: host.to_string(),
        port: 443,
        environment_id: Some("local".to_string()),
        client_addr: Some("127.0.0.1:50000".to_string()),
        method: Some("CONNECT".to_string()),
        command: Some("curl https://blocked.example".to_string()),
        exec_policy_hint: None,
    })
}

#[tokio::test]
async fn allowlist_miss_can_be_attributed_to_an_asking_decider() {
    let state = state();
    let decider: Arc<dyn NetworkPolicyDecider> =
        Arc::new(|request: NetworkPolicyRequest| async move {
            assert_eq!(request.protocol, NetworkProtocol::HttpsConnect);
            assert_eq!(request.host, "blocked.example");
            assert_eq!(request.port, 443);
            NetworkDecision::ask_with_source("not_allowed", NetworkDecisionSource::ModeGuard)
        });
    let request = request("blocked.example");

    let decision = state
        .evaluate_host_policy(Some(&decider), &request)
        .await
        .unwrap();
    assert_eq!(
        decision,
        NetworkDecision::Deny {
            reason: "not_allowed".to_string(),
            source: NetworkDecisionSource::Decider,
            decision: NetworkPolicyDecision::Ask,
        }
    );

    let payload = decision
        .to_policy_decision_payload(&request)
        .expect("deny decisions must preserve proxy attribution");
    assert_eq!(payload.decision, NetworkPolicyDecision::Ask);
    assert_eq!(payload.source, NetworkDecisionSource::Decider);
    assert_eq!(payload.protocol, Some(NetworkApprovalProtocol::Https));
    assert_eq!(payload.host.as_deref(), Some("blocked.example"));
    assert_eq!(payload.reason.as_deref(), Some("not_allowed"));
    assert_eq!(payload.port, Some(443));
}

#[tokio::test]
async fn allowlist_miss_can_be_allowed_by_the_decider() {
    let state = state();
    let decider: Arc<dyn NetworkPolicyDecider> =
        Arc::new(|_: NetworkPolicyRequest| async move { NetworkDecision::Allow });

    assert_eq!(
        state
            .evaluate_host_policy(Some(&decider), &request("blocked.example"))
            .await
            .unwrap(),
        NetworkDecision::Allow
    );
}

#[test]
fn every_proxy_protocol_maps_to_the_structured_approval_protocol() {
    for (protocol, approval_protocol) in [
        (NetworkProtocol::Http, NetworkApprovalProtocol::Http),
        (
            NetworkProtocol::HttpsConnect,
            NetworkApprovalProtocol::Https,
        ),
        (
            NetworkProtocol::Socks5Tcp,
            NetworkApprovalProtocol::Socks5Tcp,
        ),
        (
            NetworkProtocol::Socks5Udp,
            NetworkApprovalProtocol::Socks5Udp,
        ),
    ] {
        let mut request = request("blocked.example");
        request.protocol = protocol;

        assert_eq!(
            NetworkDecision::deny("not_allowed")
                .to_policy_decision_payload(&request)
                .unwrap()
                .protocol,
            Some(approval_protocol)
        );
    }
}

#[tokio::test]
async fn hard_policy_denials_do_not_call_the_decider() {
    let state = NetworkProxyState::new(NetworkPolicy {
        enabled: true,
        domains: BTreeMap::from([("blocked.example".to_string(), NetworkAccess::Deny)]),
        allow_local_binding: true,
        ..NetworkPolicy::default()
    })
    .unwrap();
    let decider: Arc<dyn NetworkPolicyDecider> = Arc::new(|_: NetworkPolicyRequest| async move {
        panic!("explicit deny must not be overridable by the decider")
    });
    let request = request("blocked.example");

    assert_eq!(
        state
            .evaluate_host_policy(Some(&decider), &request)
            .await
            .unwrap(),
        NetworkDecision::Deny {
            reason: "denied".to_string(),
            source: NetworkDecisionSource::BaselinePolicy,
            decision: NetworkPolicyDecision::Deny,
        }
    );
}

#[tokio::test]
async fn local_address_defense_does_not_call_the_decider() {
    let state = NetworkProxyState::new(NetworkPolicy {
        enabled: true,
        domains: BTreeMap::from([("*".to_string(), NetworkAccess::Allow)]),
        allow_local_binding: false,
        ..NetworkPolicy::default()
    })
    .unwrap();
    let called = Arc::new(AtomicBool::new(false));
    let called_by_decider = Arc::clone(&called);
    let decider: Arc<dyn NetworkPolicyDecider> = Arc::new(move |_: NetworkPolicyRequest| {
        let called = Arc::clone(&called_by_decider);
        async move {
            called.store(true, Ordering::SeqCst);
            NetworkDecision::Allow
        }
    });
    let request = request("127.0.0.1");

    assert_eq!(
        state
            .evaluate_host_policy(Some(&decider), &request)
            .await
            .unwrap(),
        NetworkDecision::Deny {
            reason: "not_allowed_local".to_string(),
            source: NetworkDecisionSource::BaselinePolicy,
            decision: NetworkPolicyDecision::Deny,
        }
    );
    assert!(!called.load(Ordering::SeqCst));
}
