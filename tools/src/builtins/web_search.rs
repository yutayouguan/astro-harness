//! 网页搜索工具：Brave Search API 或 DuckDuckGo Instant Answer 回退。
//!
//! 优先使用环境变量 `BRAVE_API_KEY` 调用 Brave Search；未配置时回退到
//! DuckDuckGo Instant Answer API（结果较简略）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `web_search` 工具的参数结构。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct WebSearchArgs {
    /// 搜索关键词。
    pub query: String,
    /// 最大结果数，范围 1–10，默认 5。
    #[serde(default)]
    pub max_results: Option<u32>,
}

/// 向注册表注册 `web_search` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "web_search".to_string(),
        toolset: "web_search".to_string(),
        description: "Search the web. Uses Brave Search API if BRAVE_API_KEY is set, otherwise DuckDuckGo Instant Answer."
            .to_string(),
        schema: schema_for_args::<WebSearchArgs>(),
        check_fn: None,
        icon: "search",
    });
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
            return brave_search(&key, query, max).await;
        }
    }
    duckduckgo_instant(query).await
}

/// 调用 Brave Search API 并格式化网页结果列表。
async fn brave_search(api_key: &str, query: &str, max: usize) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
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
        .await?;
    let status = resp.status();
    let v: serde_json::Value = resp.json().await?;
    if !status.is_success() {
        anyhow::bail!(
            "Brave Search 失败: {}",
            v.pointer("/message").and_then(|m| m.as_str()).unwrap_or("unknown")
        );
    }
    let mut lines = Vec::new();
    if let Some(arr) = v.pointer("/web/results").and_then(|r| r.as_array()) {
        for (i, item) in arr.iter().take(max).enumerate() {
            let title = item.get("title").and_then(|t| t.as_str()).unwrap_or("");
            let url = item.get("url").and_then(|t| t.as_str()).unwrap_or("");
            let desc = item
                .get("description")
                .and_then(|t| t.as_str())
                .unwrap_or("");
            lines.push(format!("{}. {title}\n   {url}\n   {desc}", i + 1));
        }
    }
    if lines.is_empty() {
        Ok("未找到结果".to_string())
    } else {
        Ok(lines.join("\n\n"))
    }
}

/// 调用 DuckDuckGo Instant Answer API 作为无 Brave Key 时的回退方案。
async fn duckduckgo_instant(query: &str) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let url = format!(
        "https://api.duckduckgo.com/?q={}&format=json&no_html=1&skip_disambig=1",
        urlencoding::encode(query)
    );
    let v: serde_json::Value = client.get(&url).send().await?.json().await?;
    let mut parts = Vec::new();
    if let Some(heading) = v.get("Heading").and_then(|h| h.as_str()).filter(|s| !s.is_empty()) {
        parts.push(format!("# {heading}"));
    }
    if let Some(abs) = v
        .get("AbstractText")
        .and_then(|a| a.as_str())
        .filter(|s| !s.is_empty())
    {
        parts.push(abs.to_string());
    }
    if let Some(abs_url) = v
        .get("AbstractURL")
        .and_then(|a| a.as_str())
        .filter(|s| !s.is_empty())
    {
        parts.push(format!("来源: {abs_url}"));
    }
    if let Some(arr) = v.get("RelatedTopics").and_then(|r| r.as_array()) {
        for (i, item) in arr.iter().take(5).enumerate() {
            let text = item
                .get("Text")
                .and_then(|t| t.as_str())
                .or_else(|| item.pointer("/Topics/0/Text").and_then(|t| t.as_str()))
                .unwrap_or("");
            let u = item
                .get("FirstURL")
                .and_then(|t| t.as_str())
                .or_else(|| item.pointer("/Topics/0/FirstURL").and_then(|t| t.as_str()))
                .unwrap_or("");
            if !text.is_empty() {
                parts.push(format!("{}. {text}\n   {u}", i + 1));
            }
        }
    }
    if parts.is_empty() {
        Ok(format!("DuckDuckGo 未返回摘要。可设置 BRAVE_API_KEY 获得更好的搜索结果。查询: {query}"))
    } else {
        Ok(parts.join("\n\n"))
    }
}
