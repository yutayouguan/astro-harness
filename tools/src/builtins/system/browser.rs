//! 浏览器工具：通过 HTTP GET 抓取公开 URL 的文本内容。
//!
//! 提供轻量级网页抓取；完整浏览器自动化（点击、填表等）应使用 MCP browser 工具。
//! 不跟随重定向；拒绝明显内网/本机地址以降低 SSRF 风险。正文按 UTF-8 安全截断。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

const MAX_BODY_BYTES: usize = 12_000;

/// Arguments for the `browser` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct BrowserArgs {
    /// Target URL; must start with `http://` or `https://`; public host only (no localhost/private).
    pub url: String,
}

/// 向注册表注册 `browser` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "browser".to_string(),
        toolset: "browser".to_string(),
        description: "HTTP GET a public http(s) URL and return text (no redirects, no click/fill). Body capped at ~12KB. For full browser automation use MCP browser tools."
            .to_string(),
        schema: schema_for_args::<BrowserArgs>(),
        check_fn: None,
        icon: "globe-2",
            ..ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["browser"],
    async_ctx: dispatch,
}

/// 执行 HTTP GET 请求并返回响应状态、Content-Type 与正文。
///
/// 正文超过 [`MAX_BODY_BYTES`] 时按 UTF-8 字符边界截断；请求超时 30 秒；不跟随重定向。
pub async fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: BrowserArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("browser 参数无效: {e}"))?;
    let url = parsed.url.trim();
    if url.is_empty() {
        anyhow::bail!("browser 需要 url");
    }
    assert_public_http_url(url)?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let resp = client.get(url).send().await?;
    let status = resp.status();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let body = resp.text().await?;
    let truncated = if body.len() > MAX_BODY_BYTES {
        let kept = common::truncate_utf8(&body, MAX_BODY_BYTES);
        format!(
            "{kept}…\n\n[已截断，返回 {}/{} 字节（上限 {MAX_BODY_BYTES}）]",
            kept.len(),
            body.len()
        )
    } else {
        body
    };
    Ok(format!(
        "status={status}\ncontent-type={content_type}\n\n{truncated}"
    ))
}

fn assert_public_http_url(raw: &str) -> anyhow::Result<()> {
    let u = reqwest::Url::parse(raw).map_err(|e| anyhow::anyhow!("URL 无效: {e}"))?;
    match u.scheme() {
        "http" | "https" => {}
        other => anyhow::bail!("仅支持 http(s) URL，收到 {other}"),
    }
    let host = u
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("URL 缺少主机名"))?;
    if is_blocked_host(host) {
        anyhow::bail!("拒绝访问本机/内网地址: {host}");
    }
    // 解析 A/AAAA，拦截 DNS 指向私网的情况（简单 SSRF 防护）
    let port = u.port_or_known_default().unwrap_or(80);
    let addrs = format!("{host}:{port}")
        .to_socket_addrs()
        .map_err(|e| anyhow::anyhow!("无法解析主机 {host}: {e}"))?;
    for addr in addrs {
        if is_blocked_ip(addr.ip()) {
            anyhow::bail!("拒绝访问解析到私网/本机的地址: {}", addr.ip());
        }
    }
    Ok(())
}

fn is_blocked_host(host: &str) -> bool {
    let h = host
        .trim()
        .trim_matches(|c| c == '[' || c == ']')
        .to_ascii_lowercase();
    matches!(
        h.as_str(),
        "localhost" | "localhost.localdomain" | "0.0.0.0" | "::1" | "metadata.google.internal"
    ) || h.ends_with(".localhost")
        || h.ends_with(".local")
        || h.parse::<IpAddr>().map(is_blocked_ip).unwrap_or(false)
}

fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_blocked_v4(v4),
        IpAddr::V6(v6) => is_blocked_v6(v6),
    }
}

fn is_blocked_v4(ip: Ipv4Addr) -> bool {
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || ip.octets()[0] == 100 && (ip.octets()[1] & 0b1100_0000) == 0b0100_0000
    // 100.64/10
}

fn is_blocked_v6(ip: Ipv6Addr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() {
        return true;
    }
    // fc00::/7 unique local
    let segments = ip.segments();
    if (segments[0] & 0xfe00) == 0xfc00 {
        return true;
    }
    // fe80::/10 link-local
    if (segments[0] & 0xffc0) == 0xfe80 {
        return true;
    }
    ip.to_ipv4_mapped().map(is_blocked_v4).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_localhost() {
        assert!(assert_public_http_url("http://localhost/a").is_err());
        assert!(assert_public_http_url("http://127.0.0.1/").is_err());
    }

    #[test]
    fn blocks_private_literal() {
        assert!(assert_public_http_url("http://192.168.1.1/").is_err());
        assert!(assert_public_http_url("http://10.0.0.1/").is_err());
    }

    #[test]
    fn accepts_public_host_shape() {
        let u = "https://example.com/path";
        let parsed = reqwest::Url::parse(u).unwrap();
        assert_eq!(parsed.scheme(), "https");
        assert!(!is_blocked_host(parsed.host_str().unwrap()));
    }

    #[test]
    fn truncate_respects_utf8() {
        let body = "你好".repeat(5000);
        let kept = common::truncate_utf8(&body, MAX_BODY_BYTES);
        assert!(kept.len() <= MAX_BODY_BYTES);
        assert!(std::str::from_utf8(kept.as_bytes()).is_ok());
    }
}
