//! 统一网页抓取工具：合并原 `web_extract`（HTML→文本）与 `http_fetch`（原始 body）。
//!
//! `mode=text`（默认）：HTML 粗过滤为可读文本，支持多 URL、max_chars 截断。
//! `mode=raw`：返回原始 HTTP body（不跟随重定向），单 URL。
//! SSRF 防护对齐 [`super::browser`]。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::engine::network::{assert_public_http_url, public_redirect_policy};
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) ",
    "AppleWebKit/537.36 (KHTML, like Gecko) ",
    "Chrome/122.0.0.0 Safari/537.36"
);

const DEFAULT_MAX_CHARS: usize = 12_000;
const HARD_MAX_CHARS: usize = 48_000;
const RAW_MAX_BYTES: usize = 12_000;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum WebFetchMode {
    /// Extract readable text from HTML (default).
    #[default]
    Text,
    /// Return raw HTTP body as-is (no HTML stripping, no redirects).
    Raw,
}

/// Arguments for the `web_fetch` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct WebFetchArgs {
    /// Single URL (mutually exclusive with `urls`; `urls` wins if both set).
    #[serde(default)]
    pub url: Option<String>,
    /// Multiple URLs (max 5, text mode only).
    #[serde(default)]
    pub urls: Option<Vec<String>>,
    /// Max characters of body text per URL (default 12000, hard cap 48000, text mode only).
    #[serde(default)]
    pub max_chars: Option<usize>,
    /// Fetch mode: "text" (default, HTML stripped) or "raw" (original body, no redirects).
    #[serde(default)]
    pub mode: WebFetchMode,
}

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "web_fetch".to_string(),
        toolset: "web_search".to_string(),
        description:
            "Fetch public http(s) URLs. mode=text (default): extract readable text (HTML stripped), \
             supports multiple URLs, each capped by max_chars (default 12000). \
             mode=raw: return raw HTTP body (no redirects, no HTML stripping, single URL, ~12KB cap). \
             Use after web_search when you need page body, not just snippets. \
             Rejects localhost/private IPs."
                .to_string(),
        schema: schema_for_args::<WebFetchArgs>(),
        check_fn: None,
        icon: "file-text",
        ..ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["web_fetch"],
    async_ctx: dispatch,
}

pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: WebFetchArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("web_fetch 参数无效: {e}"))?;
    let grant = ctx.effective_in_process_network_grant();

    match parsed.mode {
        WebFetchMode::Text => dispatch_text(parsed, grant).await,
        WebFetchMode::Raw => dispatch_raw(parsed, grant).await,
    }
}

// ── mode=text（原 web_extract 逻辑）──────────────────────────────

async fn dispatch_text(
    parsed: WebFetchArgs,
    grant: crate::InProcessNetworkGrant,
) -> anyhow::Result<String> {
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
        if let Some(u) = parsed
            .url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            targets.push(u.to_string());
        }
    }
    if targets.is_empty() {
        anyhow::bail!("web_fetch 需要 url 或 urls");
    }
    if targets.len() > 5 {
        anyhow::bail!("web_fetch 一次最多 5 个 URL");
    }

    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(25))
        .redirect(public_redirect_policy(5, grant.clone()))
        .build()?;

    let mut sections = Vec::with_capacity(targets.len());
    for (i, url) in targets.iter().enumerate() {
        assert_public_http_url(url, &grant)?;
        match fetch_and_extract(&client, url, max_chars).await {
            Ok(body) => sections.push(format!(
                "### [{}/{}] {}\n\n{}",
                i + 1,
                targets.len(),
                url,
                body
            )),
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

// ── mode=raw（原 http_fetch 逻辑）──────────────────────────────

async fn dispatch_raw(
    parsed: WebFetchArgs,
    grant: crate::InProcessNetworkGrant,
) -> anyhow::Result<String> {
    let url = parsed
        .url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("web_fetch mode=raw 需要 url"))?;
    assert_public_http_url(url, &grant)?;

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
    let truncated = if body.len() > RAW_MAX_BYTES {
        let kept = types::truncate_utf8(&body, RAW_MAX_BYTES);
        format!(
            "{kept}…\n\n[已截断，返回 {}/{} 字节（上限 {RAW_MAX_BYTES}）]",
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

// ── 共享：HTML 解析、SSRF 防护 ──────────────────────────────────

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
    for tag in ["script", "style", "noscript"] {
        s = strip_tag_blocks(&s, tag);
    }
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
                    t.trim_start_matches('/')
                        .split_whitespace()
                        .next()
                        .unwrap_or(""),
                    "p" | "div"
                        | "br"
                        | "li"
                        | "tr"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "h4"
                        | "h5"
                        | "h6"
                        | "section"
                        | "article"
                        | "header"
                        | "footer"
                        | "blockquote"
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
    fn truncate_raw_respects_utf8() {
        let body = "你好".repeat(5000);
        let kept = types::truncate_utf8(&body, RAW_MAX_BYTES);
        assert!(kept.len() <= RAW_MAX_BYTES);
        assert!(std::str::from_utf8(kept.as_bytes()).is_ok());
    }
}
