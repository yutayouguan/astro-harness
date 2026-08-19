use network_proxy::{
    ManagedNetworkSandboxContext, NetworkProxyState, PreparedManagedNetwork, StartedNetworkProxy,
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

    assert_eq!(
        prepared.env.get("PATH").map(String::as_str),
        Some("/safe/bin")
    );
    for key in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "http_proxy",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        assert_eq!(prepared.env.get(key), Some(&endpoint), "{key}");
    }
    assert_eq!(
        prepared
            .env
            .get("ASTRO_NETWORK_PROXY_ACTIVE")
            .map(String::as_str),
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
    let PreparedManagedNetwork {
        env,
        sandbox_context,
    } = started.proxy().prepare(HashMap::new());

    assert_eq!(
        env.get("NO_PROXY").map(String::as_str),
        Some("localhost,127.0.0.1,::1,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16")
    );
    assert_eq!(env.get("NO_PROXY"), env.get("no_proxy"));
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
