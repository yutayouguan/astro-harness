//! 工具启用状态 + MCP 配置，持久化到 ~/.astro（可按 Agent 隔离）。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 规范化 Agent id：`default` → 默认 id，空 → `None`。
fn normalize_agent_id(agent_id: Option<String>) -> Option<String> {
    agent_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s == "default" {
                memory::DEFAULT_AGENT_ID.to_string()
            } else {
                s
            }
        })
}

/// 读取当前 Agent（或全局）的工具集启用表。
#[tauri::command]
pub async fn get_tools_enabled(
    agent_id: Option<String>,
) -> Result<HashMap<String, bool>, String> {
    let id = normalize_agent_id(agent_id);
    memory::sync_tools_enabled_defaults_for_agent(id.as_deref()).map_err(|e| e.to_string())
}

/// 保存工具集启用表。
#[tauri::command]
pub async fn set_tools_enabled(
    enabled: HashMap<String, bool>,
    agent_id: Option<String>,
) -> Result<(), String> {
    let id = normalize_agent_id(agent_id);
    memory::save_tools_enabled_for_agent(id.as_deref(), &enabled).map_err(|e| e.to_string())
}

/// 内置工具目录（schemars 派生参数），供前端 Tools 面板展示
#[tauri::command]
pub async fn get_tool_catalog() -> Result<Vec<tools::ToolCatalogItem>, String> {
    Ok(tools::builtin_catalog())
}

/// MCP 发现工具的前端 DTO。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpDiscoveredToolDto {
    /// 原生工具名。
    pub name: String,
    /// 描述。
    #[serde(default)]
    pub description: String,
}

/// MCP 服务器配置的前端 DTO。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerDto {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// stdio | sse | streamableHttp
    #[serde(default = "default_mcp_type", alias = "transport")]
    pub r#type: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 单工具开关；缺失视为启用
    #[serde(default)]
    pub tools: HashMap<String, bool>,
    /// 最近一次 list_tools 缓存
    #[serde(default)]
    pub discovered: Vec<McpDiscoveredToolDto>,
}

/// serde 默认传输类型：`stdio`。
fn default_mcp_type() -> String {
    "stdio".into()
}

/// serde 默认 `enabled = true`。
fn default_true() -> bool {
    true
}

/// `McpServerConfig` → 前端 DTO。
fn dto_from_config(c: mcp::McpServerConfig) -> McpServerDto {
    McpServerDto {
        id: c.id,
        name: c.name,
        description: c.description,
        r#type: c.r#type.as_str().to_string(),
        command: c.command,
        args: c.args,
        env: c.env,
        url: c.url,
        headers: c.headers,
        enabled: c.enabled,
        tools: c.tools,
        discovered: c
            .discovered
            .into_iter()
            .map(|d| McpDiscoveredToolDto {
                name: d.name,
                description: d.description,
            })
            .collect(),
    }
}

/// 前端 DTO → `McpServerConfig`（会 sanitize server id）。
fn config_from_dto(d: McpServerDto) -> mcp::McpServerConfig {
    mcp::McpServerConfig {
        id: mcp::sanitize_server_id(&d.id),
        name: d.name,
        description: d.description,
        r#type: mcp::McpTransportType::parse(&d.r#type),
        command: d.command,
        args: d.args,
        env: d.env,
        url: d.url,
        headers: d.headers,
        enabled: d.enabled,
        tools: d.tools,
        discovered: d
            .discovered
            .into_iter()
            .map(|x| mcp::DiscoveredTool {
                name: x.name,
                description: x.description,
            })
            .collect(),
    }
}

/// Tauri 命令：get_mcp_servers。
#[tauri::command]
pub async fn get_mcp_servers(agent_id: Option<String>) -> Result<Vec<McpServerDto>, String> {
    let id = normalize_agent_id(agent_id);
    let servers = mcp::load_mcp_servers(id.as_deref()).map_err(|e| e.to_string())?;
    Ok(servers.into_iter().map(dto_from_config).collect())
}

/// Tauri 命令：set_mcp_servers。
#[tauri::command]
pub async fn set_mcp_servers(
    servers: Vec<McpServerDto>,
    agent_id: Option<String>,
) -> Result<(), String> {
    let id = normalize_agent_id(agent_id);
    let configs: Vec<_> = servers.into_iter().map(config_from_dto).collect();
    mcp::save_mcp_servers(id.as_deref(), &configs).map_err(|e| e.to_string())
}

/// 短连 list_tools，写回 discovered，并合并默认工具开关
#[tauri::command]
pub async fn refresh_mcp_tools(
    agent_id: Option<String>,
    server_id: Option<String>,
) -> Result<Vec<McpServerDto>, String> {
    let id = normalize_agent_id(agent_id);
    let configs = mcp::McpHub::refresh_discovered(id.as_deref(), server_id.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    Ok(configs.into_iter().map(dto_from_config).collect())
}

/// Tauri 命令：get_agent_usage_stats。
#[tauri::command]
pub async fn get_agent_usage_stats(
    agent_id: Option<String>,
) -> Result<memory::AgentUsageSummary, String> {
    let id = normalize_agent_id(agent_id);
    Ok(memory::get_usage_summary(id.as_deref()))
}

/// `get_usage_insights` 请求参数。
#[derive(Debug, Deserialize)]
pub struct UsageInsightsArgs {
    /// `month` | `quarter` | `year`
    pub period: String,
    /// 可选截止时间（ISO8601）；默认 now
    pub as_of: Option<String>,
    /// 可选 Agent 筛选；`None` 表示全部
    pub agent_id: Option<String>,
}

/// Tauri 命令：按 period / agent 聚合用量洞察。
#[tauri::command]
pub async fn get_usage_insights(
    args: UsageInsightsArgs,
) -> Result<memory::UsageInsights, String> {
    let period = match args.period.to_lowercase().as_str() {
        "month" => memory::UsagePeriod::Month,
        "quarter" => memory::UsagePeriod::Quarter,
        "year" => memory::UsagePeriod::Year,
        other => return Err(format!("invalid period: {other}")),
    };
    let agent_id = normalize_agent_id(args.agent_id);
    let db = memory::UsageDb::open_default().map_err(|e| e.to_string())?;
    db.query_insights(memory::UsageInsightsQuery {
        period,
        as_of: args.as_of,
        agent_id,
    })
    .map_err(|e| e.to_string())
}
