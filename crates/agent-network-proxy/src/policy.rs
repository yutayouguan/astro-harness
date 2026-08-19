use anyhow::{bail, ensure, Context, Result};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// A normalized host string for policy evaluation.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Host(String);

impl Host {
    pub fn parse(input: &str) -> Result<Self> {
        let normalized = normalize_host(input);
        ensure!(!normalized.is_empty(), "host is empty");
        Ok(Self(normalized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Returns true if the host is a loopback hostname or IP literal.
pub fn is_loopback_host(host: &Host) -> bool {
    let host = host.as_str();
    let host = unscoped_ip_literal(host).unwrap_or(host);
    host == "localhost"
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

pub fn is_non_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_non_public_ipv4(ip),
        IpAddr::V6(ip) => is_non_public_ipv6(ip),
    }
}

fn is_non_public_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_broadcast()
        || ipv4_in_cidr(ip, [0, 0, 0, 0], 8)
        || ipv4_in_cidr(ip, [100, 64, 0, 0], 10)
        || ipv4_in_cidr(ip, [192, 0, 0, 0], 24)
        || ipv4_in_cidr(ip, [192, 0, 2, 0], 24)
        || ipv4_in_cidr(ip, [198, 18, 0, 0], 15)
        || ipv4_in_cidr(ip, [198, 51, 100, 0], 24)
        || ipv4_in_cidr(ip, [203, 0, 113, 0], 24)
        || ipv4_in_cidr(ip, [240, 0, 0, 0], 4)
}

fn ipv4_in_cidr(ip: Ipv4Addr, base: [u8; 4], prefix: u8) -> bool {
    let ip = u32::from(ip);
    let base = u32::from(Ipv4Addr::from(base));
    let mask = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    (ip & mask) == (base & mask)
}

fn is_non_public_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4() {
        return is_non_public_ipv4(v4) || ip.is_loopback();
    }
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
}

/// Normalize host fragments for policy matching.
pub fn normalize_host(host: &str) -> String {
    let host = host.trim();
    if host.starts_with('[') {
        if let Some(end) = host.find(']') {
            return normalize_dns_host_or_ip_literal(&host[1..end]);
        }
    }
    if host.bytes().filter(|byte| *byte == b':').count() == 1 {
        return normalize_dns_host_or_ip_literal(host.split(':').next().unwrap_or_default());
    }
    normalize_dns_host_or_ip_literal(host)
}

fn normalize_dns_host_or_ip_literal(host: &str) -> String {
    let host = host.to_ascii_lowercase();
    let host = host.trim_end_matches('.');
    normalize_ip_literal(host).unwrap_or_else(|| host.to_string())
}

pub(crate) fn unscoped_ip_literal(host: &str) -> Option<&str> {
    let (ip, _) = host.split_once('%')?;
    ip.parse::<IpAddr>().ok()?;
    Some(ip)
}

fn normalize_ip_literal(host: &str) -> Option<String> {
    if host.parse::<IpAddr>().is_ok() {
        return Some(host.to_string());
    }
    for delimiter in ["%25", "%"] {
        if let Some((ip, scope)) = host.split_once(delimiter) {
            if ip.parse::<IpAddr>().is_ok() {
                return Some(format!("{ip}%{scope}"));
            }
        }
    }
    None
}

fn normalize_pattern(pattern: &str) -> String {
    let pattern = pattern.trim();
    if pattern == "*" {
        return "*".to_string();
    }
    let (prefix, remainder) = if let Some(domain) = pattern.strip_prefix("**.") {
        ("**.", domain)
    } else if let Some(domain) = pattern.strip_prefix("*.") {
        ("*.", domain)
    } else {
        ("", pattern)
    };
    let remainder = normalize_host(remainder);
    format!("{prefix}{remainder}")
}

pub(crate) fn compile_allowlist_globset(patterns: &[String]) -> Result<GlobSet> {
    compile_globset(patterns, true)
}

pub(crate) fn compile_denylist_globset(patterns: &[String]) -> Result<GlobSet> {
    compile_globset(patterns, false)
}

fn compile_globset(patterns: &[String], global_wildcard_allowed: bool) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    let mut seen = HashSet::new();
    for pattern in patterns {
        let pattern = normalize_pattern(pattern);
        if !global_wildcard_allowed && is_global_wildcard_domain_pattern(&pattern) {
            bail!("global wildcard '*' is allow-only");
        }
        for candidate in expand_domain_pattern(&pattern) {
            if seen.insert(candidate.clone()) {
                let glob = GlobBuilder::new(&candidate)
                    .case_insensitive(true)
                    .build()
                    .with_context(|| format!("invalid domain pattern: {candidate}"))?;
                builder.add(glob);
            }
        }
    }
    Ok(builder.build()?)
}

fn is_global_wildcard_domain_pattern(pattern: &str) -> bool {
    expand_domain_pattern(&normalize_pattern(pattern))
        .iter()
        .any(|candidate| candidate == "*")
}

fn expand_domain_pattern(pattern: &str) -> Vec<String> {
    if let Some(domain) = pattern.strip_prefix("**.") {
        vec![domain.to_string(), format!("?*.{domain}")]
    } else if let Some(domain) = pattern.strip_prefix("*.") {
        vec![format!("?*.{domain}")]
    } else {
        vec![pattern.to_string()]
    }
}
