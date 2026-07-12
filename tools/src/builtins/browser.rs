//! 浏览器工具：通过 HTTP GET 抓取 URL 文本内容。
//!
//! 提供轻量级网页抓取能力；完整浏览器自动化（点击、填表等）应使用 MCP browser 工具。
//! `selector` 与 `action` 字段为预留，当前仅执行简单 GET 请求。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `browser` 工具的参数结构。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct BrowserArgs {
    /// 目标 URL，须以 `http://` 或 `https://` 开头。
    pub url: String,
    /// 操作类型：`get` | `fetch`（默认 `get`）；当前均走 HTTP GET。
    #[serde(default)]
    pub action: Option<String>,
    /// 预留字段，供 MCP 浏览器自动化使用 CSS 选择器。
    #[serde(default)]
    pub selector: Option<String>,
}

/// 向注册表注册 `browser` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "browser".to_string(),
        toolset: "browser".to_string(),
        description: "Fetch a URL and return text content (simple HTTP get). For full browser automation, use MCP browser tools."
            .to_string(),
        schema: schema_for_args::<BrowserArgs>(),
        check_fn: None,
        icon: "globe-2",
    });
}

/// 执行 HTTP GET 请求并返回响应状态、Content-Type 与正文。
///
/// 正文超过 12_000 字符时截断；请求超时 30 秒。
pub async fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: BrowserArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("browser 参数无效: {e}"))?;
    let url = parsed.url.trim();
    if url.is_empty() {
        anyhow::bail!("browser 需要 url");
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        anyhow::bail!("仅支持 http(s) URL");
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
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
    let truncated = if body.len() > 12_000 {
        format!("{}…\n\n[已截断，共 {} 字符]", &body[..12_000], body.len())
    } else {
        body
    };
    let _ = parsed.action;
    let _ = parsed.selector; // 预留：完整浏览器自动化走 MCP
    Ok(format!(
        "status={status}\ncontent-type={content_type}\n\n{truncated}"
    ))
}
