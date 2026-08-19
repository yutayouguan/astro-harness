use network_proxy::{HostBlockDecision, HostBlockReason, NetworkProxyState};
use std::collections::BTreeMap;
use types::{NetworkAccess, NetworkPolicy};

fn state(domains: &[(&str, NetworkAccess)], allow_local_binding: bool) -> NetworkProxyState {
    NetworkProxyState::new(NetworkPolicy {
        enabled: true,
        domains: domains
            .iter()
            .map(|(domain, access)| ((*domain).to_string(), *access))
            .collect::<BTreeMap<_, _>>(),
        allow_local_binding,
        ..NetworkPolicy::default()
    })
    .unwrap()
}

#[tokio::test]
async fn deny_wins_over_an_overlapping_allow_rule() {
    let state = state(
        &[
            ("**.example.com", NetworkAccess::Allow),
            ("api.example.com", NetworkAccess::Deny),
        ],
        true,
    );

    assert_eq!(
        state.host_blocked("api.example.com", 443).await.unwrap(),
        HostBlockDecision::Blocked(HostBlockReason::Denied)
    );
    assert_eq!(
        state.host_blocked("www.example.com", 443).await.unwrap(),
        HostBlockDecision::Allowed
    );
}

#[tokio::test]
async fn scoped_wildcards_distinguish_subdomains_from_the_apex() {
    let subdomains_only = state(&[("*.example.com", NetworkAccess::Allow)], true);
    assert_eq!(
        subdomains_only
            .host_blocked("api.example.com", 443)
            .await
            .unwrap(),
        HostBlockDecision::Allowed
    );
    assert_eq!(
        subdomains_only
            .host_blocked("example.com", 443)
            .await
            .unwrap(),
        HostBlockDecision::Blocked(HostBlockReason::NotAllowed)
    );

    let apex_and_subdomains = state(&[("**.example.com", NetworkAccess::Allow)], true);
    assert_eq!(
        apex_and_subdomains
            .host_blocked("example.com", 443)
            .await
            .unwrap(),
        HostBlockDecision::Allowed
    );
}

#[tokio::test]
async fn global_wildcard_does_not_implicitly_allow_local_hosts() {
    let wildcard = state(&[("*", NetworkAccess::Allow)], false);
    assert_eq!(
        wildcard.host_blocked("127.0.0.1", 80).await.unwrap(),
        HostBlockDecision::Blocked(HostBlockReason::NotAllowedLocal)
    );

    let explicit = state(&[("127.0.0.1", NetworkAccess::Allow)], false);
    assert_eq!(
        explicit.host_blocked("127.0.0.1", 80).await.unwrap(),
        HostBlockDecision::Allowed
    );
}

#[tokio::test]
async fn dns_lookup_failure_is_blocked_fail_closed() {
    let state = state(&[("does-not-resolve.invalid", NetworkAccess::Allow)], false);

    assert_eq!(
        state
            .host_blocked("does-not-resolve.invalid", 443)
            .await
            .unwrap(),
        HostBlockDecision::Blocked(HostBlockReason::NotAllowedLocal)
    );
}

#[test]
fn global_deny_wildcard_is_rejected() {
    for pattern in ["*", "**.*", "  **.*  "] {
        let err = NetworkProxyState::new(NetworkPolicy {
            enabled: true,
            domains: BTreeMap::from([(pattern.to_string(), NetworkAccess::Deny)]),
            ..NetworkPolicy::default()
        })
        .unwrap_err();

        assert!(err.to_string().contains("allow-only"), "{pattern}");
    }
}

#[test]
fn disabled_policy_cannot_create_an_evaluable_proxy_state() {
    let err = NetworkProxyState::new(NetworkPolicy {
        enabled: false,
        domains: BTreeMap::from([("*".to_string(), NetworkAccess::Allow)]),
        ..NetworkPolicy::default()
    })
    .unwrap_err();

    assert!(err.to_string().contains("disabled"));
}
