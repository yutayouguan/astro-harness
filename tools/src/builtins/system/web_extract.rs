//! 网页正文抽取：HTTP GET 后将 HTML 粗过滤为可读文本。
//!
//! 与 Hermes `web_extract` 意图对齐：抓取公开 URL、抽正文、截断过长结果。
//! 不做 Firecrawl/Tavily 等多后端；SSRF 防护对齐 [`super::browser`]。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) ",
    "AppleWebKit/537.36 (KHTML, like Gecko) ",
    "Chrome/122.0.0.0 Safari/537.36"
);

/// 单 URL 抽取正文的默认字符上限。
const DEFAULT_MAX_CHARS: usize = 12_000;

/// 硬上限，防止把整页塞进上下文。
const HARD_MAX_CHARS: usize = 48_000;

/// `web_extract` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct WebExtractArgs {
    /// 单个 URL（与 `urls` 二选一；都给时优先 `urls`）。
    #[serde(default)]
    pub url: Option<String>,
    /// 多个 URL（最多 5 个）。
    #[serde(default)]
    pub urls: Option<Vec<String>>,
    /// 每个 URL 返回正文的最大字符数（默认 12000，硬上限 48000）。
    #[serde(default)]
    pub max_chars: Option<usize>,
}

/// 向注册表注册 `web_extract`（归属 `web_search` 工具集）。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "web_extract".to_string(),
        toolset: "web_search".to_string(),
        description: "Fetch one or more public http(s) URLs and extract readable text (HTML stripped). \
             Use after web_search when you need page body, not just snippets. \
             Rejects localhost/private IPs. Each page capped by max_chars (default 12000)."
            .to_string(),
        schema: schema_for_args::<WebExtractArgs>(),
        check_fn: None,
        icon: "file-text",
        ..ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool!(register);

/// 拉取并抽取网页正文。
pub async fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: WebExtractArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("web_extract 参数无效: {e}"))?;
    let max_chars = parsed
        .max_chars
        .unwrap_or(DEFAULT_MAX_CHARS)
        .clamp(500, HARD_MAX_CHARS);

    let mut targets: Vec<String> = Vec::new();
    if let Some(urls) = parsed.urls {
        for u in urls {
            let t = u.trim().to_string();
            if !t.is_empty() {
                targets.push(t);
            }
        }
    }
    if targets.is_empty() {
        if let Some(u) = parsed.url.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            targets.push(u.to_string());
        }
    }
    if targets.is_empty() {
        anyhow::bail!("web_extract 需要 url 或 urls");
    }
    if targets.len() > 5 {
        anyhow::bail!("web_extract 一次最多 5 个 URL");
    }

    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(25))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?;

    let mut sections = Vec::with_capacity(targets.len());
    for (i, url) in targets.iter().enumerate() {
        assert_public_http_url(url)?;
        match fetch_and_extract(&client, url, max_chars).await {
            Ok(body) => sections.push(format!("### [{}/{}] {}\n\n{}", i + 1, targets.len(), url, body)),
            Err(e) => sections.push(format!(
                "### [{}/{}] {}\n\n错误: {e}",
                i + 1,
                targets.len(),
                url
            )),
        }
    }
    Ok(sections.join("\n\n---\n\n"))
}

async fn fetch_and_extract(
    client: &reqwest::Client,
    url: &str,
    max_chars: usize,
) -> anyhow::Result<String> {
    let resp = client.get(url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("HTTP {status}");
    }
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = resp.bytes().await?;
    // 防止超大响应占满内存
    const MAX_DOWNLOAD: usize = 2 * 1024 * 1024;
    let slice = if bytes.len() > MAX_DOWNLOAD {
        &bytes[..MAX_DOWNLOAD]
    } else {
        &bytes[..]
    };
    let raw = String::from_utf8_lossy(slice);
    let text = if content_type.contains("html")
        || raw.trim_start().starts_with('<')
        || raw.to_ascii_lowercase().contains("<html")
    {
        html_to_text(&raw)
    } else {
        collapse_ws(&raw)
    };
    let truncated = if text.chars().count() > max_chars {
        let cut: String = text.chars().take(max_chars).collect();
        format!(
            "{cut}\n\n[truncated: showing {max_chars} of ~{} chars]",
            text.chars().count()
        )
    } else {
        text
    };
    Ok(truncated)
}

fn html_to_text(html: &str) -> String {
    let mut s = html.to_string();
    // 去掉 script / style / noscript
    for tag in ["script", "style", "noscript"] {
        s = strip_tag_blocks(&s, tag);
    }
    // 常见块级标签换行
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    let mut tag_buf = String::new();
    for ch in s.chars() {
        if ch == '<' {
            in_tag = true;
            tag_buf.clear();
            continue;
        }
        if in_tag {
            if ch == '>' {
                in_tag = false;
                let t = tag_buf.to_ascii_lowercase();
                if matches!(
                    t.trim_start_matches('/').split_whitespace().next().unwrap_or(""),
                    "p" | "div" | "br" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                        | "section" | "article" | "header" | "footer" | "blockquote"
                ) {
                    out.push('\n');
                }
            } else {
                tag_buf.push(ch);
            }
            continue;
        }
        out.push(ch);
    }
    let decoded = decode_basic_entities(&out);
    collapse_ws(&decoded)
}

fn strip_tag_blocks(html: &str, tag: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    let bytes = html.as_bytes();
    let lower_bytes = lower.as_bytes();
    while i < bytes.len() {
        if let Some(rel) = find_substr(&lower_bytes[i..], open.as_bytes()) {
            out.push_str(&html[i..i + rel]);
            let after_open = i + rel + open.len();
            // 找到开标签结束
            let gt = html[after_open..]
                .find('>')
                .map(|n| after_open + n + 1)
                .unwrap_or(html.len());
            if let Some(rel_close) = find_substr(&lower_bytes[gt..], close.as_bytes()) {
                i = gt + rel_close + close.len();
            } else {
                i = gt;
            }
            continue;
        }
        out.push_str(&html[i..]);
        break;
    }
    out
}

fn find_substr(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn decode_basic_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_blank = false;
    for line in s.lines() {
        let t = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if t.is_empty() {
            if !prev_blank && !out.is_empty() {
                out.push('\n');
            }
            prev_blank = true;
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&t);
        prev_blank = false;
    }
    out.trim().to_string()
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
        || (ip.octets()[0] == 100 && (ip.octets()[1] & 0b1100_0000) == 0b0100_0000)
}

fn is_blocked_v6(ip: Ipv6Addr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() {
        return true;
    }
    let segments = ip.segments();
    if (segments[0] & 0xfe00) == 0xfc00 {
        return true;
    }
    if (segments[0] & 0xffc0) == 0xfe80 {
        return true;
    }
    ip.to_ipv4_mapped().map(is_blocked_v4).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_script_and_tags() {
        let html = r#"<html><head><script>evil()</script><style>.x{}</style></head>
            <body><h1>Hello</h1><p>World &amp; friends</p></body></html>"#;
        let text = html_to_text(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("World & friends"));
        assert!(!text.contains("evil"));
        assert!(!text.contains(".x{}"));
    }

    #[test]
    fn blocks_localhost() {
        assert!(assert_public_http_url("http://localhost/a").is_err());
        assert!(assert_public_http_url("http://127.0.0.1/").is_err());
        assert!(assert_public_http_url("http://192.168.1.1/").is_err());
    }

    #[test]
    fn accepts_public_host_shape() {
        let parsed = reqwest::Url::parse("https://example.com/path").unwrap();
        assert!(!is_blocked_host(parsed.host_str().unwrap()));
    }
}
