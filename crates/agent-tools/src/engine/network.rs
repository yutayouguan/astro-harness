//! 进程内 HTTP 工具的单次主机授权与 SSRF 防护。

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};

/// 只在一次工具调用内有效的进程内网络授权。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InProcessNetworkGrant {
    unrestricted: bool,
    hosts: BTreeSet<String>,
}

impl InProcessNetworkGrant {
    pub fn for_hosts(hosts: impl IntoIterator<Item = String>) -> Self {
        Self {
            unrestricted: false,
            hosts: hosts
                .into_iter()
                .filter_map(|host| normalize_host(&host))
                .collect(),
        }
    }

    pub fn unrestricted() -> Self {
        Self {
            unrestricted: true,
            hosts: BTreeSet::new(),
        }
    }

    pub fn allows_host(&self, host: &str) -> bool {
        self.unrestricted
            || normalize_host(host)
                .map(|host| self.hosts.contains(&host))
                .unwrap_or(false)
    }

    pub fn is_empty(&self) -> bool {
        !self.unrestricted && self.hosts.is_empty()
    }

    pub fn hosts(&self) -> Vec<String> {
        self.hosts.iter().cloned().collect()
    }
}

fn normalize_host(host: &str) -> Option<String> {
    let normalized = host
        .trim()
        .trim_matches(|c| c == '[' || c == ']')
        .trim_end_matches('.')
        .to_ascii_lowercase();
    (!normalized.is_empty()).then_some(normalized)
}

/// 构造同时执行主机授权与 SSRF 检查的重定向策略。
pub(crate) fn public_redirect_policy(
    max_redirects: usize,
    grant: InProcessNetworkGrant,
) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= max_redirects {
            return attempt.error(std::io::Error::other(format!(
                "redirect limit exceeded ({max_redirects})"
            )));
        }
        match assert_public_http_url(attempt.url().as_str(), &grant) {
            Ok(()) => attempt.follow(),
            Err(error) => attempt.error(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("redirect blocked by network policy: {error}"),
            )),
        }
    })
}

/// 校验 URL 仅使用 HTTP(S)，主机已授权，且 DNS 没有解析到本机/私网。
pub(crate) fn assert_public_http_url(
    raw: &str,
    grant: &InProcessNetworkGrant,
) -> anyhow::Result<()> {
    let url = reqwest::Url::parse(raw).map_err(|error| anyhow::anyhow!("URL 无效: {error}"))?;
    match url.scheme() {
        "http" | "https" => {}
        other => anyhow::bail!("仅支持 http(s) URL，收到 {other}"),
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("URL 缺少主机名"))?;
    if !grant.allows_host(host) {
        anyhow::bail!("主机未在本次网络授权中: {host}");
    }
    if is_blocked_host(host) {
        anyhow::bail!("拒绝访问本机/内网地址: {host}");
    }
    let port = url.port_or_known_default().unwrap_or(80);
    let addrs = (host, port)
        .to_socket_addrs()
        .map_err(|error| anyhow::anyhow!("无法解析主机 {host}: {error}"))?;
    let mut resolved = false;
    for addr in addrs {
        resolved = true;
        if is_blocked_ip(addr.ip()) {
            anyhow::bail!("拒绝访问解析到私网/本机的地址: {}", addr.ip());
        }
    }
    if !resolved {
        anyhow::bail!("主机未解析到任何地址: {host}");
    }
    Ok(())
}

fn is_blocked_host(host: &str) -> bool {
    let Some(host) = normalize_host(host) else {
        return true;
    };
    matches!(
        host.as_str(),
        "localhost" | "localhost.localdomain" | "0.0.0.0" | "::1" | "metadata.google.internal"
    ) || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.parse::<IpAddr>().map(is_blocked_ip).unwrap_or(false)
}

fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_blocked_v4(ip),
        IpAddr::V6(ip) => is_blocked_v6(ip),
    }
}

fn is_blocked_v4(ip: Ipv4Addr) -> bool {
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.octets()[0] == 0
        || (ip.octets()[0] == 100 && (ip.octets()[1] & 0b1100_0000) == 0b0100_0000)
        || (ip.octets()[0] == 198 && matches!(ip.octets()[1], 18 | 19))
}

fn is_blocked_v6(ip: Ipv6Addr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() {
        return true;
    }
    let segments = ip.segments();
    if (segments[0] & 0xfe00) == 0xfc00 || (segments[0] & 0xffc0) == 0xfe80 {
        return true;
    }
    if ip.is_multicast() || (segments[0] == 0x2001 && segments[1] == 0x0db8) {
        return true;
    }
    ip.to_ipv4_mapped().map(is_blocked_v4).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_normalizes_and_matches_exact_hosts() {
        let grant = InProcessNetworkGrant::for_hosts([
            "EXAMPLE.com.".to_string(),
            "www.rust-lang.org".to_string(),
        ]);
        assert!(grant.allows_host("example.com"));
        assert!(grant.allows_host("WWW.RUST-LANG.ORG"));
        assert!(!grant.allows_host("sub.example.com"));
    }

    #[test]
    fn public_url_requires_granted_non_private_host() {
        let grant = InProcessNetworkGrant::for_hosts([
            "example.com".to_string(),
            "127.0.0.1".to_string(),
            "198.18.0.1".to_string(),
            "224.0.0.1".to_string(),
        ]);
        assert!(assert_public_http_url("http://127.0.0.1/", &grant).is_err());
        assert!(assert_public_http_url("http://198.18.0.1/", &grant).is_err());
        assert!(assert_public_http_url("http://224.0.0.1/", &grant).is_err());
        assert!(assert_public_http_url("https://not-approved.example/", &grant).is_err());
    }
}
