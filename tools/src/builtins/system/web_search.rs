//! 网页搜索工具：Brave Search API，或免 Key 的 Bing RSS 回退。
//!
//! 优先使用环境变量 `BRAVE_API_KEY` 调用 Brave Search；未配置时使用
//! Bing 公开 RSS（`format=rss`，无需 API Key）。

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

/// Arguments for the `web_search` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct WebSearchArgs {
    /// Search query.
    pub query: String,
    /// Max results (1–10, default 5).
    #[serde(default)]
    pub max_results: Option<u32>,
}

/// 单条网页搜索结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// 标题。
    pub title: String,
    /// 链接。
    pub url: String,
    /// 摘要（可为空）。
    pub snippet: String,
}

/// 向注册表注册 `web_search` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "web_search".to_string(),
        toolset: "web_search".to_string(),
        description: "Search the web. Uses Brave Search when BRAVE_API_KEY is set; otherwise free Bing RSS (no API key)."
            .to_string(),
        schema: schema_for_args::<WebSearchArgs>(),
        check_fn: None,
        icon: "search",
            ..ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["web_search"],
    async_ctx: dispatch,
}

/// 执行网页搜索并返回格式化的结果文本。
///
/// `query` 不能为空；`max_results` 会被 clamp 到 1–10。
pub async fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: WebSearchArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("web_search 参数无效: {e}"))?;
    let query = parsed.query.trim();
    if query.is_empty() {
        anyhow::bail!("web_search 需要 query");
    }
    let max = parsed.max_results.unwrap_or(5).clamp(1, 10) as usize;

    if let Ok(key) = std::env::var("BRAVE_API_KEY") {
        if !key.trim().is_empty() {
            match brave_search(&key, query, max).await {
                Ok(text) => return Ok(text),
                Err(e) => {
                    // Brave 失败时降级到免费 Bing，避免整次工具报错
                    let bing = bing_rss_search(query, max).await.map_err(|bing_err| {
                        anyhow::anyhow!("Brave Search 失败: {e}; Bing 回退也失败: {bing_err}")
                    })?;
                    return Ok(format!("（Brave 失败，已用 Bing 回退）\n\n{bing}"));
                }
            }
        }
    }
    bing_rss_search(query, max).await
}

fn http_client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(20))
        .build()?)
}

/// 调用 Brave Search API 并格式化网页结果列表。
async fn brave_search(api_key: &str, query: &str, max: usize) -> anyhow::Result<String> {
    let client = http_client()?;
    let url = format!(
        "https://api.search.brave.com/res/v1/web/search?q={}&count={}",
        urlencoding::encode(query),
        max
    );
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .header("X-Subscription-Token", api_key.trim())
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Brave 请求失败: {e}"))?;
    let status = resp.status();
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Brave 响应解析失败: {e}"))?;
    if !status.is_success() {
        anyhow::bail!(
            "Brave Search HTTP {}: {}",
            status.as_u16(),
            v.pointer("/message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown")
        );
    }
    let mut hits = Vec::new();
    if let Some(arr) = v.pointer("/web/results").and_then(|r| r.as_array()) {
        for item in arr.iter().take(max) {
            let title = item.get("title").and_then(|t| t.as_str()).unwrap_or("");
            let url = item.get("url").and_then(|t| t.as_str()).unwrap_or("");
            let desc = item
                .get("description")
                .and_then(|t| t.as_str())
                .unwrap_or("");
            if title.is_empty() && url.is_empty() {
                continue;
            }
            hits.push(SearchHit {
                title: title.to_string(),
                url: url.to_string(),
                snippet: desc.to_string(),
            });
        }
    }
    Ok(format_hits(&hits))
}

/// 使用 Bing 公开 RSS 端点搜索（无需 API Key）。
async fn bing_rss_search(query: &str, max: usize) -> anyhow::Result<String> {
    let client = http_client()?;
    let url = format!(
        "https://www.bing.com/search?q={}&format=rss",
        urlencoding::encode(query)
    );
    let mut last_err = None;
    for attempt in 0..2 {
        match client
            .get(&url)
            .header(
                "Accept",
                "application/rss+xml, application/xml, text/xml, */*",
            )
            .send()
            .await
        {
            Ok(resp) => {
                let status = resp.status();
                let body = resp
                    .text()
                    .await
                    .map_err(|e| anyhow::anyhow!("Bing 响应读取失败: {e}"))?;
                if !status.is_success() {
                    last_err = Some(anyhow::anyhow!(
                        "Bing RSS HTTP {} (attempt {})",
                        status.as_u16(),
                        attempt + 1
                    ));
                    continue;
                }
                let hits = parse_bing_rss(&body, max);
                if hits.is_empty() {
                    return Ok("未找到结果".to_string());
                }
                return Ok(format_hits(&hits));
            }
            Err(e) => {
                last_err = Some(anyhow::anyhow!("Bing 请求失败: {e}"));
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Bing 搜索失败")))
}

/// 解析 Bing RSS XML，提取 title / link / description。
pub fn parse_bing_rss(xml: &str, max: usize) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    for item in xml.split("<item>").skip(1) {
        let end = item.find("</item>").unwrap_or(item.len());
        let block = &item[..end];
        let title = xml_tag_text(block, "title");
        let url = xml_tag_text(block, "link");
        let snippet = strip_html(&xml_tag_text(block, "description"));
        if title.is_empty() && url.is_empty() {
            continue;
        }
        hits.push(SearchHit {
            title: decode_xml_entities(&title),
            url,
            snippet: decode_xml_entities(&snippet),
        });
        if hits.len() >= max {
            break;
        }
    }
    hits
}

fn xml_tag_text(block: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let Some(start) = block.find(&open) else {
        return String::new();
    };
    let rest = &block[start + open.len()..];
    let Some(end) = rest.find(&close) else {
        return String::new();
    };
    rest[..end].trim().to_string()
}

fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_xml_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

fn format_hits(hits: &[SearchHit]) -> String {
    if hits.is_empty() {
        return "未找到结果".to_string();
    }
    hits.iter()
        .enumerate()
        .map(|(i, h)| {
            if h.snippet.is_empty() {
                format!("{}. {}\n   {}", i + 1, h.title, h.url)
            } else {
                format!("{}. {}\n   {}\n   {}", i + 1, h.title, h.url, h.snippet)
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bing_rss_extracts_items() {
        let xml = r#"<?xml version="1.0" encoding="utf-8" ?>
<rss version="2.0"><channel>
<title>Bing: Rust</title>
<item>
  <title>Rust Programming Language</title>
  <link>https://www.rust-lang.org/</link>
  <description>Rust is blazingly fast &amp; memory-efficient.</description>
</item>
<item>
  <title>Tokio</title>
  <link>https://tokio.rs/</link>
  <description>An async <b>runtime</b> for Rust.</description>
</item>
<item>
  <title>Ignored when max=2</title>
  <link>https://example.com/</link>
  <description>x</description>
</item>
</channel></rss>"#;
        let hits = parse_bing_rss(xml, 2);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].title, "Rust Programming Language");
        assert_eq!(hits[0].url, "https://www.rust-lang.org/");
        assert_eq!(
            hits[0].snippet,
            "Rust is blazingly fast & memory-efficient."
        );
        assert_eq!(hits[1].title, "Tokio");
        assert_eq!(hits[1].url, "https://tokio.rs/");
        assert_eq!(hits[1].snippet, "An async runtime for Rust.");
    }

    #[test]
    fn parse_bing_rss_empty_and_malformed() {
        assert!(parse_bing_rss("", 5).is_empty());
        assert!(parse_bing_rss("<rss></rss>", 5).is_empty());
        let one = parse_bing_rss(
            "<item><title>Only Title</title><link></link><description></description></item>",
            5,
        );
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].title, "Only Title");
    }

    #[test]
    fn format_hits_omits_empty_snippet_line() {
        let text = format_hits(&[SearchHit {
            title: "A".into(),
            url: "https://a.test".into(),
            snippet: String::new(),
        }]);
        assert_eq!(text, "1. A\n   https://a.test");
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    #[tokio::test]
    #[ignore = "network"]
    async fn bing_rss_live_returns_hits() {
        let text = bing_rss_search("OpenAI API", 3).await.expect("bing");
        assert!(text.contains("http"), "{text}");
        assert!(!text.contains("未找到结果"), "{text}");
        println!("{text}");
    }
}
