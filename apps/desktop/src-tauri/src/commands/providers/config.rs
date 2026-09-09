//! 工具启用状态 + MCP 配置，持久化到 ~/.astro（可按 Agent 隔离）。

use proto::astro_service_client::AstroServiceClient;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

use crate::infra::grpc::{default_grpc_address, endpoint_url};

/// 规范化 Agent id：`default` → 默认 id，空 → `None`。
fn normalize_agent_id(agent_id: Option<String>) -> Option<String> {
    agent_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s == "default" {
                home::DEFAULT_AGENT_ID.to_string()
            } else {
                s
            }
        })
}

/// 读取当前 Agent（或全局）的工具集启用表。
#[tauri::command]
pub async fn get_tools_enabled(agent_id: Option<String>) -> Result<HashMap<String, bool>, String> {
    let id = normalize_agent_id(agent_id);
    home::sync_tools_enabled_defaults_for_agent(id.as_deref()).map_err(|e| e.to_string())
}

/// 保存工具集启用表（与磁盘现有项合并，避免 UI 未列的工具集被冲掉）。
#[tauri::command]
pub async fn set_tools_enabled(
    enabled: HashMap<String, bool>,
    agent_id: Option<String>,
) -> Result<(), String> {
    let id = normalize_agent_id(agent_id);
    home::patch_tools_enabled_for_agent(id.as_deref(), &enabled).map_err(|e| e.to_string())
}

/// Persisted global preferences and tool groups safe to configure from Desktop.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolLoadingSettings {
    modes: HashMap<String, home::ToolLoadingMode>,
    adjustable_toolsets: Vec<String>,
}

/// Global loading preferences; enable gates remain independently Agent-scoped.
#[tauri::command]
pub fn get_tool_loading_settings() -> Result<ToolLoadingSettings, String> {
    let mut registry = tools::ToolRegistry::new();
    tools::register_all(&mut registry);
    Ok(ToolLoadingSettings {
        modes: home::load_tool_loading_modes().map_err(|error| error.to_string())?,
        adjustable_toolsets: home::KNOWN_TOOLSET_IDS.iter()
            .filter(|toolset| registry.toolset_loading_adjustable(toolset))
            .map(|toolset| (*toolset).to_string()).collect(),
    })
}

#[tauri::command]
pub fn set_tool_loading_mode(toolset: String, mode: home::ToolLoadingMode) -> Result<ToolLoadingSettings, String> {
    let mut registry = tools::ToolRegistry::new();
    tools::register_all(&mut registry);
    if !registry.toolset_loading_adjustable(&toolset) {
        return Err(format!("toolset loading policy is fixed: {toolset}"));
    }
    home::set_tool_loading_mode(&toolset, mode).map_err(|error| error.to_string())?;
    get_tool_loading_settings()
}

#[cfg(test)]
mod tool_loading_tests {
    use super::*;

    #[test]
    fn tool_loading_commands_round_trip_without_changing_enable_gates() {
        let dir = tempfile::TempDir::new().unwrap();
        let _env = home::test_env::AstroMemoryDirGuard::set(dir.path());
        let initial = get_tool_loading_settings().unwrap();
        assert!(initial.modes.is_empty());
        assert!(initial.adjustable_toolsets.iter().any(|id| id == "browser"));
        assert!(!home::settings::path(dir.path()).exists());
        let saved = set_tool_loading_mode("browser".into(), home::ToolLoadingMode::Always).unwrap();
        assert_eq!(saved.modes["browser"], home::ToolLoadingMode::Always);
        assert!(home::load_tools_enabled().unwrap().is_empty());
        assert!(get_tool_loading_settings().unwrap().modes.contains_key("browser"));
        assert!(set_tool_loading_mode("apply_patch".into(), home::ToolLoadingMode::OnDemand).is_err());
        assert!(set_tool_loading_mode("system".into(), home::ToolLoadingMode::OnDemand).is_err());
        assert!(set_tool_loading_mode("browser".into(), home::ToolLoadingMode::Auto).unwrap().modes.is_empty());
    }
}

/// Builtin default disclosure, not per-sampling effective availability.
#[tauri::command]
pub async fn get_tool_catalog() -> Result<Vec<tools::ToolCatalogItem>, String> {
    let mut registry = tools::ToolRegistry::new();
    tools::register_all(&mut registry);
    tools::register_workflow_tools(&mut registry, &home::default_memory_dir())
        .map_err(|error| error.to_string())?;
    Ok(tools::catalog_for_ui(&registry))
}

/// MCP 发现工具的前端 DTO。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpDiscoveredToolDto {
    /// 原生工具名。
    pub name: String,
    /// 描述。
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
}

/// MCP Server 的真实运行状态；来自 backend 内存中的 Agent Hub。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpRuntimeStatusDto {
    pub id: String,
    pub name: String,
    pub status: String,
    pub tools: Vec<String>,
    pub required: bool,
    pub error: Option<String>,
    pub retryable: bool,
    pub retry_attempt: u32,
    pub next_retry_at_unix_ms: Option<u64>,
    pub oauth_available: bool,
    pub authenticated: bool,
}

fn runtime_status_dto(server: proto::McpServerInfo) -> McpRuntimeStatusDto {
    McpRuntimeStatusDto {
        id: server.id,
        name: server.name,
        status: server.status,
        tools: server.tools,
        required: server.required,
        error: (!server.error.is_empty()).then_some(server.error),
        retryable: server.retryable,
        retry_attempt: server.retry_attempt,
        next_retry_at_unix_ms: (server.next_retry_at_unix_ms > 0)
            .then_some(server.next_retry_at_unix_ms),
        oauth_available: server.oauth_available,
        authenticated: server.authenticated,
    }
}

/// MCP 服务器配置的前端 DTO。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerDto {
    #[serde(default, rename = "sourcePath")]
    pub source_path: Option<String>,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// stdio | streamableHttp
    #[serde(default = "default_mcp_type", alias = "transport")]
    pub r#type: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// 仅保存环境变量名，运行时从本地环境转发值。
    #[serde(default, rename = "envVars", alias = "env_vars")]
    pub env_vars: Vec<String>,
    #[serde(default)]
    pub url: String,
    #[serde(default, alias = "http_headers", alias = "httpHeaders")]
    pub headers: HashMap<String, String>,
    #[serde(default, rename = "bearerTokenEnvVar", alias = "bearer_token_env_var")]
    pub bearer_token_env_var: Option<String>,
    #[serde(default, rename = "envHttpHeaders", alias = "env_http_headers")]
    pub env_http_headers: HashMap<String, String>,
    /// oauth | chatgpt；缺省表示标准 OAuth 可用但先尝试匿名连接。
    #[serde(default)]
    pub auth: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub cwd: Option<String>,
    /// 启动超时（秒），覆盖建连、初始化与首次工具发现。
    #[serde(
        default,
        rename = "startupTimeoutSecs",
        alias = "startup_timeout_sec",
        alias = "startup_timeout_secs",
        alias = "startupTimeoutSec"
    )]
    pub startup_timeout_secs: Option<u64>,
    /// 单次工具调用超时（秒）。
    #[serde(
        default,
        rename = "toolTimeoutSecs",
        alias = "tool_timeout_sec",
        alias = "tool_timeout_secs",
        alias = "toolTimeoutSec"
    )]
    pub tool_timeout_secs: Option<u64>,
    #[serde(
        default,
        rename = "enabledTools",
        alias = "enabled_tools",
        alias = "enabled-tools"
    )]
    pub enabled_tools: Option<Vec<String>>,
    #[serde(
        default,
        rename = "disabledTools",
        alias = "disabled_tools",
        alias = "disabled-tools"
    )]
    pub disabled_tools: Vec<String>,
    /// Server 默认审批模式：auto | prompt | writes | approve。
    #[serde(
        default = "default_mcp_approval_mode",
        rename = "defaultToolsApprovalMode"
    )]
    pub default_tools_approval_mode: String,
    /// 单工具开关；缺失视为启用
    #[serde(default)]
    pub tools: HashMap<String, bool>,
    /// 单工具审批模式覆盖。
    #[serde(default, rename = "toolApprovalModes")]
    pub tool_approval_modes: HashMap<String, String>,
    /// 单工具文本输出 token 上限。
    #[serde(default, rename = "toolOutputTokenLimits")]
    pub tool_output_token_limits: HashMap<String, u64>,
    /// 最近一次 list_tools 缓存
    #[serde(default)]
    pub discovered: Vec<McpDiscoveredToolDto>,
    /// 配置作用域：global / builtin / project。
    #[serde(default = "default_global_scope")]
    pub scope: String,
    /// 配置来源层，用于 UI 解释覆盖关系。
    #[serde(default = "default_user_provenance")]
    pub provenance: String,
    /// packaged/builtin 配置为只读。
    #[serde(default = "default_true")]
    pub editable: bool,
}

/// serde 默认传输类型：`stdio`。
fn default_mcp_type() -> String {
    "stdio".into()
}

/// serde 默认 `enabled = true`。
fn default_true() -> bool {
    true
}

fn default_global_scope() -> String {
    "global".into()
}

fn default_user_provenance() -> String {
    "user".into()
}

fn default_mcp_approval_mode() -> String {
    "auto".into()
}

/// `McpServerConfig` → 前端 DTO。
fn dto_from_config(c: mcp::McpServerConfig) -> McpServerDto {
    dto_from_config_scoped(c, "global")
}

fn dto_from_config_scoped(c: mcp::McpServerConfig, scope: &str) -> McpServerDto {
    let tools = c
        .tools
        .iter()
        .map(|(name, config)| (name.clone(), config.is_enabled()))
        .collect();
    let tool_approval_modes = c
        .tools
        .iter()
        .filter_map(|(name, config)| {
            config
                .approval_mode()
                .map(|mode| (name.clone(), mode.as_str().to_string()))
        })
        .collect();
    let tool_output_token_limits = c
        .tools
        .iter()
        .filter_map(|(name, config)| {
            let limit = match config {
                mcp::McpToolConfig::Enabled(_) => None,
                mcp::McpToolConfig::Settings(settings) => settings.output_token_limit,
            }?;
            Some((name.clone(), u64::try_from(limit.get()).ok()?))
        })
        .collect();
    McpServerDto {
        source_path: None,
        id: c.id,
        name: c.name,
        description: c.description,
        r#type: c.r#type.as_str().to_string(),
        command: c.command,
        args: c.args,
        env: c.env,
        env_vars: c.env_vars,
        url: c.url,
        headers: c.headers,
        bearer_token_env_var: c.bearer_token_env_var,
        env_http_headers: c.env_http_headers,
        auth: c.auth.map(|auth| auth.as_str().to_string()),
        enabled: c.enabled,
        required: c.required,
        cwd: c.cwd,
        startup_timeout_secs: c.startup_timeout_secs,
        tool_timeout_secs: c.tool_timeout_secs,
        enabled_tools: c.enabled_tools,
        disabled_tools: c.disabled_tools,
        default_tools_approval_mode: c.default_tools_approval_mode.as_str().to_string(),
        tools,
        tool_approval_modes,
        tool_output_token_limits,
        discovered: c
            .discovered
            .into_iter()
            .map(|d| McpDiscoveredToolDto {
                name: d.name,
                description: d.description,
                title: d.annotations.title,
                read_only_hint: d.annotations.read_only_hint,
                destructive_hint: d.annotations.destructive_hint,
                idempotent_hint: d.annotations.idempotent_hint,
                open_world_hint: d.annotations.open_world_hint,
            })
            .collect(),
        scope: scope.to_string(),
        provenance: if scope == "builtin" {
            "packaged".to_string()
        } else {
            scope.to_string()
        },
        editable: scope != "builtin",
    }
}

/// 前端 DTO → `McpServerConfig`（会 sanitize server id）。
fn config_from_dto(d: McpServerDto) -> Result<mcp::McpServerConfig, String> {
    let transport = mcp::McpTransportType::parse(&d.r#type)?;
    let default_tools_approval_mode =
        types::McpToolApprovalMode::parse(&d.default_tools_approval_mode)?;
    match transport {
        mcp::McpTransportType::Stdio
            if !d.headers.is_empty()
                || d.bearer_token_env_var.is_some()
                || !d.env_http_headers.is_empty()
                || d.auth.is_some() =>
        {
            return Err("STDIO MCP server cannot define HTTP authentication or headers".into());
        }
        mcp::McpTransportType::StreamableHttp
            if d.cwd.as_deref().is_some_and(|cwd| !cwd.trim().is_empty()) =>
        {
            return Err("HTTP MCP server cannot define cwd".into());
        }
        mcp::McpTransportType::StreamableHttp
            if !d.args.is_empty() || !d.env.is_empty() || !d.env_vars.is_empty() =>
        {
            return Err("HTTP MCP server cannot define args, env, or env_vars".into());
        }
        _ => {}
    }
    let mut tools = d
        .tools
        .into_iter()
        .map(|(name, enabled)| {
            let approval_mode = d
                .tool_approval_modes
                .get(&name)
                .map(|mode| types::McpToolApprovalMode::parse(mode))
                .transpose()?;
            Ok((name, mcp::McpToolConfig::from_parts(enabled, approval_mode)))
        })
        .collect::<Result<HashMap<_, _>, String>>()?;
    for (name, mode) in d.tool_approval_modes {
        if tools.contains_key(&name) {
            continue;
        }
        tools.insert(
            name,
            mcp::McpToolConfig::from_parts(true, Some(types::McpToolApprovalMode::parse(&mode)?)),
        );
    }
    for (name, raw_limit) in d.tool_output_token_limits {
        let limit = usize::try_from(raw_limit)
            .ok()
            .and_then(std::num::NonZeroUsize::new)
            .ok_or_else(|| format!("tool output token limit for {name:?} must be positive"))?;
        let entry = tools
            .entry(name)
            .or_insert(mcp::McpToolConfig::Enabled(true));
        *entry = entry.clone().with_output_token_limit(Some(limit));
    }
    Ok(mcp::McpServerConfig {
        id: mcp::sanitize_server_id(&d.id),
        name: d.name,
        description: d.description,
        r#type: transport,
        command: d.command,
        args: d.args,
        env: d.env,
        env_vars: d.env_vars,
        url: d.url,
        headers: d.headers,
        bearer_token_env_var: d
            .bearer_token_env_var
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
        env_http_headers: d.env_http_headers,
        auth: d.auth.as_deref().map(mcp::McpHttpAuth::parse).transpose()?,
        enabled: d.enabled,
        required: d.required,
        cwd: d.cwd,
        startup_timeout_secs: d.startup_timeout_secs,
        tool_timeout_secs: d.tool_timeout_secs,
        enabled_tools: d.enabled_tools,
        disabled_tools: d.disabled_tools,
        default_tools_approval_mode,
        tools,
        discovered: d
            .discovered
            .into_iter()
            .map(|x| mcp::DiscoveredTool {
                name: x.name,
                description: x.description,
                annotations: types::McpToolAnnotations {
                    title: x.title,
                    read_only_hint: x.read_only_hint,
                    destructive_hint: x.destructive_hint,
                    idempotent_hint: x.idempotent_hint,
                    open_world_hint: x.open_world_hint,
                },
            })
            .collect(),
    })
}

/// Tauri 命令：get_mcp_servers。
#[tauri::command]
pub async fn get_mcp_servers(
    scope: Option<String>,
    project_root: Option<String>,
) -> Result<Vec<McpServerDto>, String> {
    let scope = scope.as_deref().unwrap_or("global");
    let explicit_root = project_root
        .as_deref()
        .map(str::trim)
        .filter(|root| !root.is_empty())
        .map(std::path::PathBuf::from);
    let resolved_root = explicit_root.or_else(|| agent::git_worktree::resolve_project_root(None));
    let servers =
        mcp::load_mcp_servers_scoped(scope, resolved_root.as_deref()).map_err(|e| e.to_string())?;
    let sources = mcp::source_paths(scope, resolved_root.as_deref()).map_err(|e| e.to_string())?;
    Ok(servers
        .into_iter()
        .map(|server| {
            let source = sources.get(&server.id).cloned();
            let mut dto = dto_from_config_scoped(server, scope);
            dto.source_path = source;
            dto
        })
        .collect())
}

/// 读取指定 Agent 的 MCP Hub 真实连接状态；无活跃 Hub 时返回分层配置状态。
#[tauri::command]
pub async fn get_mcp_server_statuses(
    agent_id: Option<String>,
) -> Result<Vec<McpRuntimeStatusDto>, String> {
    let memory_root = home::default_memory_dir();
    let id = normalize_agent_id(agent_id).unwrap_or_else(|| home::active_agent_id(&memory_root));
    let project_root = agent::git_worktree::resolve_project_root(None)
        .map(|root| root.to_string_lossy().into_owned())
        .unwrap_or_default();
    let endpoint = endpoint_url(&default_grpc_address());
    let mut client = AstroServiceClient::connect(endpoint)
        .await
        .map_err(|error| error.to_string())?;
    let response = client
        .list_mcp_servers(proto::McpServerListRequest {
            agent_id: id,
            project_root,
        })
        .await
        .map_err(|error| error.to_string())?
        .into_inner();

    Ok(response
        .servers
        .into_iter()
        .map(runtime_status_dto)
        .collect())
}

/// 清除指定 Server 的退避并让对应 Agent Hub 立即重连。
#[tauri::command]
pub async fn reconnect_mcp_server(
    agent_id: Option<String>,
    server_id: String,
) -> Result<Vec<McpRuntimeStatusDto>, String> {
    let memory_root = home::default_memory_dir();
    let id = normalize_agent_id(agent_id).unwrap_or_else(|| home::active_agent_id(&memory_root));
    let endpoint = endpoint_url(&default_grpc_address());
    let mut client = AstroServiceClient::connect(endpoint)
        .await
        .map_err(|error| error.to_string())?;
    let response = client
        .reconnect_mcp_server(proto::McpReconnectRequest {
            agent_id: id,
            server_id,
        })
        .await
        .map_err(|error| error.to_string())?
        .into_inner();
    Ok(response
        .servers
        .into_iter()
        .map(runtime_status_dto)
        .collect())
}

/// Tauri 命令：set_mcp_servers。
#[tauri::command]
pub async fn set_mcp_servers(
    servers: Vec<McpServerDto>,
    scope: Option<String>,
    project_root: Option<String>,
) -> Result<(), String> {
    let scope = scope.as_deref().unwrap_or("global");
    let explicit_root = project_root
        .as_deref()
        .map(str::trim)
        .filter(|root| !root.is_empty())
        .map(std::path::PathBuf::from);
    let resolved_root = explicit_root.or_else(|| agent::git_worktree::resolve_project_root(None));
    let configs: Vec<_> = servers
        .into_iter()
        .map(config_from_dto)
        .collect::<Result<_, _>>()?;
    mcp::save_mcp_servers_scoped(scope, resolved_root.as_deref(), &configs)
        .map_err(|e| e.to_string())
}

/// 实时刷新工具发现；仅缓存元数据，不改写配置或显式工具开关。
#[tauri::command]
pub async fn refresh_mcp_tools(
    agent_id: Option<String>,
    server_id: Option<String>,
) -> Result<Vec<McpServerDto>, String> {
    let id = normalize_agent_id(agent_id);
    let memory_root = home::default_memory_dir();
    let effective_agent_id = id
        .clone()
        .unwrap_or_else(|| home::active_agent_id(&memory_root));
    let execution_root = agent::git_worktree::resolve_project_root(None)
        .unwrap_or_else(|| home::agent_workspace_dir(&memory_root, &effective_agent_id));
    let profile_id = memory::load_permission_settings(&memory_root)
        .selection
        .profile_id;
    let sandbox_audit = tools::SandboxAuditMetadata::new(
        memory_root.clone(),
        None,
        None,
        "mcp_refresh",
        profile_id,
    );
    let policy = tools::context::build_command_sandbox_policy(
        &memory_root,
        &execution_root,
        None,
        false,
        None,
    )
    .map_err(|error| {
        sandbox_audit.record(
            tools::SandboxAuditKind::Denied,
            None,
            "mcp",
            "policy_resolution_failed",
            None,
        );
        error.to_string()
    })?;
    let execution_context = mcp::McpExecutionContext::new(policy, &execution_root)
        .map(|context| context.with_sandbox_audit(sandbox_audit))
        .map_err(|e| e.to_string())?;
    let configs = mcp::McpHub::refresh_discovered(server_id.as_deref(), &execution_context)
        .await
        .map_err(|e| e.to_string())?;
    Ok(configs.into_iter().map(dto_from_config).collect())
}

/// Tauri 命令：get_agent_usage_stats。
#[tauri::command]
pub async fn get_agent_usage_stats(
    agent_id: Option<String>,
) -> Result<usage::AgentUsageSummary, String> {
    let id = normalize_agent_id(agent_id);
    Ok(usage::get_usage_summary(id.as_deref()))
}

/// `get_usage_insights` 请求参数。
#[derive(Debug, Deserialize)]
pub struct UsageInsightsArgs {
    /// `month` | `quarter` | `year` | `days30` | `days90` | `days365`
    pub period: String,
    /// 可选截止时间（ISO8601）；默认 now
    pub as_of: Option<String>,
    /// 可选 Agent 筛选；`None` 表示全部
    pub agent_id: Option<String>,
    /// 可选序列粒度：`day` | `week` | `month`；缺省时按 period 自适应。
    #[serde(default)]
    pub granularity: Option<String>,
}

/// Tauri 命令：按 period / agent 聚合用量洞察。
#[tauri::command]
pub async fn get_usage_insights(args: UsageInsightsArgs) -> Result<usage::UsageInsights, String> {
    let period = match args.period.to_lowercase().as_str() {
        "month" => usage::UsagePeriod::Month,
        "quarter" => usage::UsagePeriod::Quarter,
        "year" => usage::UsagePeriod::Year,
        "days30" => usage::UsagePeriod::Days30,
        "days90" => usage::UsagePeriod::Days90,
        "days365" => usage::UsagePeriod::Days365,
        other => return Err(format!("invalid period: {other}")),
    };
    let agent_id = normalize_agent_id(args.agent_id);
    let granularity = match args.granularity.as_deref().map(str::to_lowercase) {
        None => None,
        Some(value) if value == "day" => Some(usage::UsageGranularity::Day),
        Some(value) if value == "week" => Some(usage::UsageGranularity::Week),
        Some(value) if value == "month" => Some(usage::UsageGranularity::Month),
        Some(value) => return Err(format!("invalid granularity: {value}")),
    };
    let db = usage::UsageDb::open_default()
        .await
        .map_err(|e| e.to_string())?;
    let query = usage::UsageInsightsQuery {
        period,
        as_of: args.as_of,
        agent_id,
    };
    match granularity {
        Some(granularity) => db.query_insights_with_granularity(query, granularity).await,
        None => db.query_insights(query).await,
    }
    .map_err(|e| e.to_string())
}

/// `get_trace_insights` 请求参数。
#[derive(Debug, Deserialize)]
pub struct TraceInsightsArgs {
    /// `month` | `quarter` | `year` | `days30` | `days90` | `days365`
    pub period: String,
    /// 可选截止时间（ISO8601）；默认 now
    pub as_of: Option<String>,
    /// 可选 Agent 筛选；`None` 表示全部
    pub agent_id: Option<String>,
}

/// Tauri 命令：按 period / agent 聚合 Agent 调用链 Tracing。
#[tauri::command]
pub async fn get_trace_insights(args: TraceInsightsArgs) -> Result<usage::TraceInsights, String> {
    let period = match args.period.to_lowercase().as_str() {
        "month" => usage::UsagePeriod::Month,
        "quarter" => usage::UsagePeriod::Quarter,
        "year" => usage::UsagePeriod::Year,
        "days30" => usage::UsagePeriod::Days30,
        "days90" => usage::UsagePeriod::Days90,
        "days365" => usage::UsagePeriod::Days365,
        other => return Err(format!("invalid period: {other}")),
    };
    let agent_id = normalize_agent_id(args.agent_id);
    usage::query_trace_insights(usage::TraceInsightsQuery {
        period,
        as_of: args.as_of,
        agent_id,
    })
    .await
    .map_err(|e| e.to_string())
}

/// 诊断页的实时组件状态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsStatusDto {
    pub backend_healthy: bool,
    pub backend_endpoint: String,
    pub backend_error: Option<String>,
    pub provider_enabled: usize,
    pub provider_total: usize,
    pub active_provider_id: Option<String>,
    pub active_provider_name: Option<String>,
    pub provider_error: Option<String>,
    pub mcp_connected: usize,
    pub mcp_total: usize,
    pub mcp_retrying: usize,
    pub mcp_error: Option<String>,
    pub database_healthy: bool,
    pub database_journal_mode: String,
    pub database_schema_version: Option<i32>,
    pub database_error: Option<String>,
}

/// 读取 Backend / Provider / MCP / Session DB 的诊断概览。
#[tauri::command]
pub async fn get_diagnostics_status() -> Result<DiagnosticsStatusDto, String> {
    let endpoint = endpoint_url(&default_grpc_address());
    let (
        provider_enabled,
        provider_total,
        active_provider_id,
        active_provider_name,
        provider_error,
    ) = match super::core::get_providers_state() {
        Ok(state) => {
            let total = state
                .providers
                .iter()
                .filter(|provider| provider.supports_responses_api)
                .count();
            let enabled = state
                .providers
                .iter()
                .filter(|provider| provider.enabled && provider.supports_responses_api)
                .count();
            let active_provider_name = state.active_provider_id.as_ref().and_then(|active_id| {
                state
                    .providers
                    .iter()
                    .find(|provider| &provider.id == active_id)
                    .map(|provider| provider.display_name.clone())
            });
            (
                enabled,
                total,
                state.active_provider_id,
                active_provider_name,
                None,
            )
        }
        Err(error) => (0, 0, None, None, Some(error)),
    };

    let mcp_result = get_mcp_server_statuses(None).await;
    let backend_healthy = mcp_result.is_ok();
    let backend_error = mcp_result.as_ref().err().cloned();
    let (mcp_connected, mcp_total, mcp_retrying, mcp_error) = match mcp_result {
        Ok(statuses) => {
            let connected = statuses
                .iter()
                .filter(|status| status.status == "connected")
                .count();
            let retrying = statuses
                .iter()
                .filter(|status| matches!(status.status.as_str(), "connecting" | "backoff"))
                .count();
            (connected, statuses.len(), retrying, None)
        }
        Err(error) => (0, 0, 0, Some(error)),
    };

    let (database_healthy, database_schema_version, database_error) =
        match crate::commands::common::open_sessions().await {
            Ok(store) => match store.schema_version().await {
                Ok(version) => (true, Some(version), None),
                Err(error) => (false, None, Some(error.to_string())),
            },
            Err(error) => (false, None, Some(error)),
        };

    Ok(DiagnosticsStatusDto {
        backend_healthy,
        backend_endpoint: endpoint,
        backend_error,
        provider_enabled,
        provider_total,
        active_provider_id,
        active_provider_name,
        provider_error,
        mcp_connected,
        mcp_total,
        mcp_retrying,
        mcp_error,
        database_healthy,
        database_journal_mode: if database_healthy { "WAL" } else { "—" }.to_string(),
        database_schema_version,
        database_error,
    })
}

fn redact_diagnostic_log_line(raw: &str) -> String {
    fn redact_marker(input: &str, marker: &str) -> String {
        let mut output = input.to_string();
        let marker_lower = marker.to_ascii_lowercase();
        let mut search_from = 0;
        loop {
            let lower = output.to_ascii_lowercase();
            let Some(offset) = lower[search_from..].find(&marker_lower) else {
                break;
            };
            let index = search_from + offset;
            let start = index + marker.len();
            let quote = output[start..]
                .chars()
                .next()
                .filter(|character| matches!(character, '"' | '\''));
            let value_start = start + quote.map_or(0, char::len_utf8);
            let end = output[value_start..]
                .find(|character: char| {
                    quote.map_or_else(
                        || character.is_whitespace() || matches!(character, ',' | ';'),
                        |quote| character == quote,
                    )
                })
                .map_or(output.len(), |offset| value_start + offset);
            if value_start == end || &output[value_start..end] == "[REDACTED]" {
                search_from = end;
                continue;
            }
            output.replace_range(value_start..end, "[REDACTED]");
            search_from = value_start + "[REDACTED]".len();
        }
        output
    }

    [
        "api_key=",
        "api-key=",
        "apikey=",
        "?key=",
        "&key=",
        "access_token=",
        "refresh_token=",
        "client_secret=",
        "\"api_key\":\"",
        "\"api_key\": \"",
        "\"apiKey\":\"",
        "\"apiKey\": \"",
        "bearer ",
    ]
    .into_iter()
    .fold(raw.to_string(), |line, marker| redact_marker(&line, marker))
}

/// 导出隐私裁剪后的诊断 JSON；不包含 Provider 密钥或完整配置。
#[tauri::command]
pub async fn export_diagnostics_bundle(app: AppHandle) -> Result<Option<String>, String> {
    let file_name = format!(
        "astro-diagnostics-{}.json",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    );
    let Some(file_path) = app
        .dialog()
        .file()
        .add_filter("JSON", &["json"])
        .set_file_name(file_name)
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = file_path.into_path().map_err(|error| error.to_string())?;
    let status = get_diagnostics_status().await?;
    let logs = home::query_agent_logs(home::AgentLogQuery {
        logs_dir: home::logs_dir(),
        session_id: None,
        turn_id: None,
        min_level: None,
        since_ms: None,
        until_ms: None,
        lines: 500,
        source: home::LogSource::Both,
    })
    .map_err(|error| error.to_string())?
    .into_iter()
    .map(|line| {
        serde_json::json!({
            "source": line.source,
            "raw": redact_diagnostic_log_line(&line.raw),
        })
    })
    .collect::<Vec<_>>();
    let bundle = serde_json::json!({
        "formatVersion": 1,
        "generatedAt": chrono::Utc::now().to_rfc3339(),
        "application": {
            "version": env!("CARGO_PKG_VERSION"),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
        },
        "status": {
            "backendHealthy": status.backend_healthy,
            "providerEnabled": status.provider_enabled,
            "providerTotal": status.provider_total,
            "activeProviderId": status.active_provider_id,
            "mcpConnected": status.mcp_connected,
            "mcpTotal": status.mcp_total,
            "mcpRetrying": status.mcp_retrying,
            "databaseHealthy": status.database_healthy,
            "databaseJournalMode": status.database_journal_mode,
            "databaseSchemaVersion": status.database_schema_version,
        },
        "recentLogs": logs,
    });
    let encoded = serde_json::to_string_pretty(&bundle).map_err(|error| error.to_string())?;
    let redacted = redact_diagnostic_log_line(&encoded);
    std::fs::write(&path, redacted).map_err(|error| error.to_string())?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// `query_agent_logs` 请求参数。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryAgentLogsArgs {
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub min_level: Option<String>,
    pub since_ms: Option<i64>,
    pub until_ms: Option<i64>,
    pub lines: Option<usize>,
    /// "agent" | "errors" | "both"
    pub source: Option<String>,
}

/// Tauri 命令：按 session/turn/level 查询 agent/errors 日志尾部。
#[tauri::command]
pub async fn query_agent_logs(args: QueryAgentLogsArgs) -> Result<Vec<home::AgentLogLine>, String> {
    let source = match args.source.as_deref() {
        Some("agent") => home::LogSource::Agent,
        Some("errors") => home::LogSource::Errors,
        _ => home::LogSource::Both,
    };
    let q = home::AgentLogQuery {
        logs_dir: home::logs_dir(),
        session_id: args.session_id,
        turn_id: args.turn_id,
        min_level: args.min_level,
        since_ms: args.since_ms,
        until_ms: args.until_ms,
        lines: args.lines.unwrap_or(50),
        source,
    };
    tokio::task::spawn_blocking(move || home::query_agent_logs(q))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod diagnostics_tests {
    use super::*;

    #[test]
    fn diagnostic_export_redacts_common_credential_shapes() {
        let raw = concat!(
            "api_key=secret-one ",
            "OPENAI_API_KEY='secret-two' ",
            "{\"apiKey\":\"secret-three\"} ",
            "{\"api_key\": \"secret-spaced\"} ",
            "Authorization: Bearer secret-four ",
            "https://example.com?key=secret-five&x=1 ",
            "access_token=secret-six refresh_token=secret-seven ",
            "client_secret=secret-eight"
        );
        let redacted = redact_diagnostic_log_line(raw);

        assert!(!redacted.contains("secret-one"));
        assert!(!redacted.contains("secret-two"));
        assert!(!redacted.contains("secret-three"));
        assert!(!redacted.contains("secret-spaced"));
        assert!(!redacted.contains("secret-four"));
        assert!(!redacted.contains("secret-five"));
        assert!(!redacted.contains("secret-six"));
        assert!(!redacted.contains("secret-seven"));
        assert!(!redacted.contains("secret-eight"));
        assert_eq!(redacted.matches("[REDACTED]").count(), 9);
    }
}

#[cfg(test)]
mod mcp_config_tests {
    use super::*;

    fn server_dto(transport: &str) -> McpServerDto {
        McpServerDto {
            source_path: None,
            id: "demo".into(),
            name: "Demo".into(),
            description: String::new(),
            r#type: transport.into(),
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            env_vars: Vec::new(),
            url: "http://localhost:3000/mcp".into(),
            headers: HashMap::new(),
            bearer_token_env_var: None,
            env_http_headers: HashMap::new(),
            auth: None,
            enabled: true,
            required: false,
            cwd: None,
            startup_timeout_secs: None,
            tool_timeout_secs: None,
            enabled_tools: None,
            disabled_tools: Vec::new(),
            default_tools_approval_mode: "auto".into(),
            tools: HashMap::new(),
            tool_approval_modes: HashMap::new(),
            tool_output_token_limits: HashMap::new(),
            discovered: Vec::new(),
            scope: default_global_scope(),
            provenance: default_user_provenance(),
            editable: true,
        }
    }

    #[test]
    fn mcp_dto_rejects_legacy_sse_instead_of_falling_back() {
        let error = config_from_dto(server_dto("sse")).unwrap_err();
        assert!(error.contains("legacy SSE transport is not supported"));
    }

    #[test]
    fn mcp_dto_accepts_streamable_http() {
        let config = config_from_dto(server_dto("streamableHttp")).unwrap();
        assert_eq!(config.r#type, mcp::McpTransportType::StreamableHttp);
    }

    #[test]
    fn mcp_dto_preserves_timeout_configuration() {
        let mut dto = server_dto("stdio");
        dto.startup_timeout_secs = Some(17);
        dto.tool_timeout_secs = Some(91);

        let config = config_from_dto(dto).unwrap();
        assert_eq!(config.startup_timeout_secs, Some(17));
        assert_eq!(config.tool_timeout_secs, Some(91));

        let roundtrip = dto_from_config(config);
        assert_eq!(roundtrip.startup_timeout_secs, Some(17));
        assert_eq!(roundtrip.tool_timeout_secs, Some(91));
    }

    #[test]
    fn mcp_dto_preserves_environment_credential_references() {
        let mut stdio = server_dto("stdio");
        stdio.url.clear();
        stdio.command = "npx".into();
        stdio.env_vars = vec!["MCP_TOKEN".into()];
        let stdio_roundtrip = dto_from_config(config_from_dto(stdio).unwrap());
        assert_eq!(stdio_roundtrip.env_vars, vec!["MCP_TOKEN"]);

        let mut http = server_dto("streamableHttp");
        http.bearer_token_env_var = Some("MCP_ACCESS_TOKEN".into());
        http.env_http_headers
            .insert("X-API-Key".into(), "MCP_API_KEY".into());
        let http_roundtrip = dto_from_config(config_from_dto(http).unwrap());
        assert_eq!(
            http_roundtrip.bearer_token_env_var.as_deref(),
            Some("MCP_ACCESS_TOKEN")
        );
        assert_eq!(http_roundtrip.env_http_headers["X-API-Key"], "MCP_API_KEY");
    }

    #[test]
    fn mcp_dto_preserves_lifecycle_and_tool_policy() {
        let mut dto = server_dto("stdio");
        dto.required = true;
        dto.cwd = Some("packages/server".into());
        dto.enabled_tools = Some(vec!["read".into(), "search".into()]);
        dto.disabled_tools = vec!["search".into()];

        let config = config_from_dto(dto).unwrap();
        assert!(config.required);
        assert_eq!(config.cwd.as_deref(), Some("packages/server"));
        assert!(config.is_tool_enabled("read"));
        assert!(!config.is_tool_enabled("search"));
        assert!(!config.is_tool_enabled("write"));

        let roundtrip = dto_from_config(config);
        assert!(roundtrip.required);
        assert_eq!(roundtrip.cwd.as_deref(), Some("packages/server"));
        assert_eq!(roundtrip.enabled_tools.unwrap().len(), 2);
        assert_eq!(roundtrip.disabled_tools, vec!["search"]);
    }

    #[test]
    fn mcp_dto_roundtrips_approval_modes_and_annotations() {
        let mut dto = server_dto("streamableHttp");
        dto.default_tools_approval_mode = "writes".into();
        dto.tools.insert("publish".into(), false);
        dto.tool_approval_modes
            .insert("publish".into(), "prompt".into());
        dto.tool_output_token_limits.insert("publish".into(), 512);
        dto.discovered.push(McpDiscoveredToolDto {
            name: "publish".into(),
            description: "Publish a document".into(),
            title: Some("Publish".into()),
            read_only_hint: Some(false),
            destructive_hint: Some(true),
            idempotent_hint: Some(false),
            open_world_hint: Some(true),
        });

        let config = config_from_dto(dto).unwrap();
        assert_eq!(
            config.default_tools_approval_mode,
            types::McpToolApprovalMode::Writes
        );
        assert_eq!(
            config.tool_approval_mode("publish"),
            types::McpToolApprovalMode::Prompt
        );
        assert_eq!(config.tool_output_token_limit("publish"), Some(512));
        assert!(!config.is_tool_enabled("publish"));

        let dto = dto_from_config(config);
        assert_eq!(dto.default_tools_approval_mode, "writes");
        assert_eq!(
            dto.tool_approval_modes.get("publish").map(String::as_str),
            Some("prompt")
        );
        assert_eq!(dto.tool_output_token_limits.get("publish"), Some(&512));
        assert_eq!(dto.discovered[0].destructive_hint, Some(true));
    }

    #[test]
    fn mcp_dto_rejects_http_cwd() {
        let mut dto = server_dto("streamableHttp");
        dto.cwd = Some("packages/server".into());
        let error = config_from_dto(dto).unwrap_err();
        assert!(error.contains("cannot define cwd"));
    }

    #[test]
    fn runtime_status_dto_preserves_retry_metadata() {
        let dto = runtime_status_dto(proto::McpServerInfo {
            id: "docs".into(),
            name: "Docs".into(),
            status: "backoff".into(),
            tools: vec!["search".into()],
            required: true,
            error: "connection reset".into(),
            retryable: true,
            retry_attempt: 3,
            next_retry_at_unix_ms: 1_700_000_000_000,
            oauth_available: true,
            authenticated: false,
        });
        assert_eq!(dto.id, "docs");
        assert!(dto.retryable);
        assert_eq!(dto.retry_attempt, 3);
        assert_eq!(dto.next_retry_at_unix_ms, Some(1_700_000_000_000));
    }
}
