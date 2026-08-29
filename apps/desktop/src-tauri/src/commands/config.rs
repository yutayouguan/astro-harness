//! 工具启用状态 + MCP 配置，持久化到 ~/.astro（可按 Agent 隔离）。

use proto::astro_service_client::AstroServiceClient;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    let mut merged = home::load_tools_enabled_for_agent(id.as_deref());
    for (k, v) in enabled {
        merged.insert(k, v);
    }
    for known in home::KNOWN_TOOLSET_IDS {
        merged.entry((*known).to_string()).or_insert(true);
    }
    home::save_tools_enabled_for_agent(id.as_deref(), &merged).map_err(|e| e.to_string())
}

/// 内置工具目录（schemars 派生参数），供前端 Tools 面板展示
#[tauri::command]
pub async fn get_tool_catalog() -> Result<Vec<tools::ToolCatalogItem>, String> {
    Ok(tools::builtin_catalog())
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
    McpServerDto {
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
    Ok(servers
        .into_iter()
        .map(|server| dto_from_config_scoped(server, scope))
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

/// 短连 list_tools，写回 discovered，并合并默认工具开关
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
    /// `month` | `quarter` | `year`
    pub period: String,
    /// 可选截止时间（ISO8601）；默认 now
    pub as_of: Option<String>,
    /// 可选 Agent 筛选；`None` 表示全部
    pub agent_id: Option<String>,
}

/// Tauri 命令：按 period / agent 聚合用量洞察。
#[tauri::command]
pub async fn get_usage_insights(args: UsageInsightsArgs) -> Result<usage::UsageInsights, String> {
    let period = match args.period.to_lowercase().as_str() {
        "month" => usage::UsagePeriod::Month,
        "quarter" => usage::UsagePeriod::Quarter,
        "year" => usage::UsagePeriod::Year,
        other => return Err(format!("invalid period: {other}")),
    };
    let agent_id = normalize_agent_id(args.agent_id);
    let db = usage::UsageDb::open_default()
        .await
        .map_err(|e| e.to_string())?;
    db.query_insights(usage::UsageInsightsQuery {
        period,
        as_of: args.as_of,
        agent_id,
    })
    .await
    .map_err(|e| e.to_string())
}

/// `get_trace_insights` 请求参数。
#[derive(Debug, Deserialize)]
pub struct TraceInsightsArgs {
    /// `month` | `quarter` | `year`
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

/// `query_agent_logs` 请求参数。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryAgentLogsArgs {
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub min_level: Option<String>,
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
        lines: args.lines.unwrap_or(50),
        source,
    };
    home::query_agent_logs(q).map_err(|e| e.to_string())
}

#[cfg(test)]
mod mcp_config_tests {
    use super::*;

    fn server_dto(transport: &str) -> McpServerDto {
        McpServerDto {
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
        assert!(!config.is_tool_enabled("publish"));

        let dto = dto_from_config(config);
        assert_eq!(dto.default_tools_approval_mode, "writes");
        assert_eq!(
            dto.tool_approval_modes.get("publish").map(String::as_str),
            Some("prompt")
        );
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
