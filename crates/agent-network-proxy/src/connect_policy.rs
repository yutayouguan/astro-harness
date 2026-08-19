use crate::policy::{is_non_public_ip, normalize_host, unscoped_ip_literal};
use crate::{HostBlockDecision, NetworkProxyState};
use anyhow::{anyhow, Context, Result};
use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{lookup_host, TcpStream};
use tokio::time::timeout;

const DNS_LOOKUP_TIMEOUT: Duration = Duration::from_secs(2);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub(crate) enum ConnectError {
    #[error("network target rejected by policy")]
    PolicyDenied,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub(crate) async fn connect_checked(
    state: &Arc<NetworkProxyState>,
    host: &str,
    port: u16,
) -> std::result::Result<TcpStream, ConnectError> {
    let addresses = timeout(DNS_LOOKUP_TIMEOUT, lookup_host((host, port)))
        .await
        .context("target DNS lookup timed out")?
        .with_context(|| format!("resolve CONNECT target {host}:{port}"))?
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err(anyhow!("CONNECT target resolved to no addresses").into());
    }

    let mut last_error = None;
    let mut rejected_non_public = false;
    for address in addresses {
        match ensure_resolved_address_allowed(state, host, address).await {
            Ok(()) => {}
            Err(ConnectError::PolicyDenied) => {
                rejected_non_public = true;
                continue;
            }
            Err(error) => return Err(error),
        }
        match timeout(CONNECT_TIMEOUT, TcpStream::connect(address)).await {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(error)) => last_error = Some(error),
            Err(_) => {
                last_error = Some(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("connect to {address} timed out"),
                ));
            }
        }
    }

    if let Some(error) = last_error {
        return Err(anyhow!(error)
            .context(format!("connect to {host}:{port}"))
            .into());
    }
    if rejected_non_public {
        return Err(ConnectError::PolicyDenied);
    }
    Err(anyhow!("CONNECT target has no usable addresses").into())
}

async fn ensure_resolved_address_allowed(
    state: &NetworkProxyState,
    host: &str,
    address: SocketAddr,
) -> std::result::Result<(), ConnectError> {
    if is_non_public_ip(address.ip()) && !allows_non_public_target(state, host, address).await? {
        return Err(ConnectError::PolicyDenied);
    }
    Ok(())
}

async fn allows_non_public_target(
    state: &NetworkProxyState,
    host: &str,
    address: SocketAddr,
) -> Result<bool> {
    if state.allow_local_binding() {
        return Ok(true);
    }
    if !target_matches_non_public_addr(host, address.ip()) {
        return Ok(false);
    }
    Ok(state.host_blocked(host, address.port()).await? == HostBlockDecision::Allowed)
}

fn target_matches_non_public_addr(host: &str, address: IpAddr) -> bool {
    let host = normalize_host(host);
    let host_no_scope = unscoped_ip_literal(&host).unwrap_or(&host);
    if let Ok(host_address) = host_no_scope.parse::<IpAddr>() {
        return host_address == address;
    }
    host == "localhost" && address.is_loopback()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::net::{Ipv4Addr, Ipv6Addr};
    use types::{NetworkAccess, NetworkPolicy};

    #[test]
    fn public_hostname_never_matches_a_private_resolution() {
        assert!(!target_matches_non_public_addr(
            "api.example.com",
            Ipv4Addr::LOCALHOST.into()
        ));
    }

    #[test]
    fn explicit_literals_and_localhost_match_only_their_destination() {
        assert!(target_matches_non_public_addr(
            "127.0.0.1",
            Ipv4Addr::LOCALHOST.into()
        ));
        assert!(target_matches_non_public_addr(
            "localhost",
            Ipv6Addr::LOCALHOST.into()
        ));
        assert!(!target_matches_non_public_addr(
            "localhost",
            Ipv4Addr::new(10, 0, 0, 1).into()
        ));
    }

    #[tokio::test]
    async fn private_rebinding_destination_is_a_policy_denial() {
        let state = NetworkProxyState::new(NetworkPolicy {
            enabled: true,
            domains: BTreeMap::from([("api.example.com".to_string(), NetworkAccess::Allow)]),
            ..NetworkPolicy::default()
        })
        .unwrap();

        let error = ensure_resolved_address_allowed(
            &state,
            "api.example.com",
            SocketAddr::from((Ipv4Addr::LOCALHOST, 443)),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, ConnectError::PolicyDenied));
    }
}
