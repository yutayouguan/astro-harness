//! MCP 连接池与工具调用。
//!
//! 按 Agent 加载配置、维持 RunningService，并向 ToolRegistry 暴露原生命名空间工具。

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{anyhow, Context};
use futures::{stream, StreamExt};
use http::{header::AUTHORIZATION, HeaderName, HeaderValue};
use rmcp::model::{CallToolRequestParams, ContentBlock, ElicitRequestParams, Tool as RmcpTool};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{Peer, RoleClient, ServiceExt};
use serde_json::{json, Value};
use tracing::{info, warn};

use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

use crate::config::{
    load_mcp_servers_layered, merge_discovered, persist_discovered_layered, DiscoveredTool,
    McpHttpAuth, McpServerConfig, McpTransportType,
};
use crate::elicitation::McpElicitationBroker;
use crate::names::{
    is_mcp_tool_name, parse_qualified_name, qualify_tool_name, sanitize_server_id, tool_namespace,
    MCP_TOOLSET,
};

/// 同时启动的 MCP Server 上限。
pub const MAX_PARALLEL_MCP_STARTUPS: usize = 4;
/// 自动重连退避上限。
pub const MAX_MCP_RETRY_DELAY_SECS: u64 = 30;
/// 单个 Server instructions 的内存上限。
pub const MAX_MCP_SERVER_INSTRUCTIONS_CHARS: usize = 16_384;
/// 单个 Agent 所有已连接 Server instructions 的总上限。
pub const MAX_TOTAL_MCP_INSTRUCTIONS_CHARS: usize = 65_536;

/// MCP 连接建立时使用的权限快照。
///
/// 常驻连接只接受 session/profile 生成的稳定策略，不承接单次工具审批授权。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpExecutionContext {
    sandbox_policy: sandbox::SandboxPolicy,
    working_dir: PathBuf,
    allowed_working_roots: Vec<PathBuf>,
    sandbox_audit: Option<sandbox::SandboxAuditMetadata>,
}

impl McpExecutionContext {
    pub fn new(
        sandbox_policy: sandbox::SandboxPolicy,
        working_dir: impl AsRef<Path>,
    ) -> anyhow::Result<Self> {
        let working_dir = working_dir.as_ref().canonicalize().with_context(|| {
            format!(
                "resolve MCP working directory: {}",
                working_dir.as_ref().display()
            )
        })?;
        if !working_dir.is_dir() {
            anyhow::bail!(
                "MCP working directory is not a directory: {}",
                working_dir.display()
            );
        }
        Ok(Self {
            sandbox_policy,
            allowed_working_roots: vec![working_dir.clone()],
            working_dir,
            sandbox_audit: None,
        })
    }

    /// 允许受信任扩展包中的 MCP Server 使用其包目录作为 cwd。
    ///
    /// 这里只扩大 cwd 校验边界，不增加沙箱写权限；扩展目录仍受当前 profile 约束。
    pub fn with_additional_working_roots(
        mut self,
        roots: impl IntoIterator<Item = PathBuf>,
    ) -> anyhow::Result<Self> {
        for root in roots {
            let root = root
                .canonicalize()
                .with_context(|| format!("resolve MCP allowed working root: {}", root.display()))?;
            if !root.is_dir() {
                anyhow::bail!(
                    "MCP allowed working root is not a directory: {}",
                    root.display()
                );
            }
            if !self.allowed_working_roots.contains(&root) {
                self.allowed_working_roots.push(root);
            }
        }
        self.allowed_working_roots.sort();
        Ok(self)
    }

    pub fn with_sandbox_audit(mut self, audit: sandbox::SandboxAuditMetadata) -> Self {
        self.sandbox_audit = Some(audit);
        self
    }

    fn sandbox_audit_for(&self, server_id: &str) -> Option<sandbox::SandboxAuditMetadata> {
        self.sandbox_audit
            .as_ref()
            .map(|audit| audit.with_tool_name(format!("mcp:{server_id}")))
    }

    fn fingerprint(&self) -> String {
        format!(
            "{}|{}|{}",
            self.sandbox_policy.profile_hash_material(),
            self.working_dir.to_string_lossy(),
            self.allowed_working_roots
                .iter()
                .map(|root| root.to_string_lossy())
                .collect::<Vec<_>>()
                .join(";")
        )
    }
}

/// 供 ToolRegistry 注册的精简条目。
#[derive(Debug, Clone)]
pub struct ToolEntrySpec {
    /// `mcp__{server}__{tool}` 限定名。
    pub qualified_name: String,
    /// Responses API 原生命名空间 `mcp__{server}`。
    pub namespace: String,
    /// 服务器 id。
    pub server_id: String,
    /// 原生工具名。
    pub native_name: String,
    /// 描述。
    pub description: String,
    /// 参数 schema。
    pub schema: Value,
    /// 解析 Server 默认与单工具覆盖后的审批模式。
    pub approval_mode: types::McpToolApprovalMode,
    /// Server 声明的非授权性风险提示。
    pub annotations: types::McpToolAnnotations,
    /// 用户/扩展策略解析后的输出 token 预算。
    pub output_token_limit: Option<usize>,
}

/// 已连接 Server 在 initialize 阶段返回的 guidance 快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerInstructions {
    pub server_id: String,
    pub server_name: String,
    pub instructions: String,
}

/// 当前健康连接可显式按需访问的 MCP 内容能力。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpBrokerCapabilities {
    pub resource_servers: Vec<String>,
    pub prompt_servers: Vec<String>,
}

fn normalize_server_instructions(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(
        trimmed
            .chars()
            .take(MAX_MCP_SERVER_INSTRUCTIONS_CHARS)
            .collect(),
    )
}

fn server_instructions_from_info(info: Option<&rmcp::model::ServerInfo>) -> Option<String> {
    info.and_then(|info| {
        info.instructions
            .as_deref()
            .and_then(normalize_server_instructions)
    })
}

fn bound_server_instructions(
    mut entries: Vec<McpServerInstructions>,
) -> Vec<McpServerInstructions> {
    entries.sort_by(|left, right| left.server_id.cmp(&right.server_id));

    let mut remaining = MAX_TOTAL_MCP_INSTRUCTIONS_CHARS;
    let mut bounded = Vec::new();
    for mut entry in entries {
        if remaining == 0 {
            break;
        }
        let count = entry.instructions.chars().count();
        if count > remaining {
            entry.instructions = entry.instructions.chars().take(remaining).collect();
        }
        remaining = remaining.saturating_sub(entry.instructions.chars().count());
        bounded.push(entry);
    }
    bounded
}

fn tool_annotations(tool: &RmcpTool) -> types::McpToolAnnotations {
    let Some(annotations) = tool.annotations.as_ref() else {
        return types::McpToolAnnotations::default();
    };
    types::McpToolAnnotations {
        title: annotations.title.clone(),
        read_only_hint: annotations.read_only_hint,
        destructive_hint: annotations.destructive_hint,
        idempotent_hint: annotations.idempotent_hint,
        open_world_hint: annotations.open_world_hint,
    }
}

/// 单个 MCP 服务器的运行状态快照（供 UI）。
#[derive(Debug, Clone)]
pub struct ServerStatus {
    /// 服务器 id。
    pub id: String,
    /// 显示名。
    pub name: String,
    /// 是否为必需 Server。
    pub required: bool,
    /// 状态字符串（如 connected / error）。
    pub status: String,
    /// 已发现工具名列表。
    pub tools: Vec<String>,
    /// 最近错误。
    pub error: Option<String>,
    /// 最近失败是否允许自动重试。
    pub retryable: bool,
    /// 当前连续失败次数。
    pub retry_attempt: u32,
    /// 下一次允许重试的 Unix 毫秒时间；永久错误为 None。
    pub next_retry_at_unix_ms: Option<u64>,
    /// 此 Server 是否可通过标准 MCP OAuth 登录。
    pub oauth_available: bool,
    /// 当前连接是否使用 Keychain 中的 OAuth 凭证。
    pub authenticated: bool,
}

/// MCP Server 连接生命周期状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpLifecycleState {
    Disabled,
    Connecting,
    Connected,
    Disconnected,
    Backoff,
    AuthRequired,
    Error,
}

impl McpLifecycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Connecting => "connecting",
            Self::Connected => "connected",
            Self::Disconnected => "disconnected",
            Self::Backoff => "backoff",
            Self::AuthRequired => "auth-required",
            Self::Error => "error",
        }
    }
}

/// 单个必需 MCP Server 的启动失败诊断。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpStartupFailure {
    pub server_id: String,
    pub server_name: String,
    pub error: String,
}

/// 一个或多个必需 MCP Server 启动失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequiredMcpServersError {
    pub failures: Vec<McpStartupFailure>,
}

impl std::fmt::Display for RequiredMcpServersError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let details = self
            .failures
            .iter()
            .map(|failure| {
                format!(
                    "{} ({}): {}",
                    failure.server_name, failure.server_id, failure.error
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        write!(formatter, "required MCP server startup failed: {details}")
    }
}

impl std::error::Error for RequiredMcpServersError {}

/// 通知处理器与 RunningServer 共享的工具状态。
struct SharedToolState {
    tools: Vec<RmcpTool>,
    generation: u64,
}

/// `tools/list_changed` 通知处理器 — 自动重新拉取工具列表。
#[derive(Clone)]
struct ToolChangeHandler {
    state: Arc<TokioMutex<SharedToolState>>,
    server_id: String,
    /// 单调递增序列号，确保旧响应不覆盖新响应。
    next_seq: Arc<std::sync::atomic::AtomicU64>,
    elicitation: Option<Arc<McpElicitationBroker>>,
}

impl rmcp::handler::client::ClientHandler for ToolChangeHandler {
    fn get_info(&self) -> rmcp::model::ClientInfo {
        rmcp::model::ClientInfo::default()
    }

    async fn create_elicitation(
        &self,
        request: ElicitRequestParams,
        context: rmcp::service::RequestContext<rmcp::service::RoleClient>,
    ) -> Result<rmcp::model::ElicitResult, rmcp::model::ErrorData> {
        let request_id = serde_json::to_value(&context.id)
            .map(|value| match value {
                Value::String(value) => value,
                value => value.to_string(),
            })
            .map_err(|error| rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
        let params = serde_json::to_value(request)
            .map_err(|error| rmcp::model::ErrorData::internal_error(error.to_string(), None))?;
        let Some(elicitation) = &self.elicitation else {
            return Ok(rmcp::model::ElicitResult::new(
                rmcp::model::ElicitationAction::Decline,
            ));
        };
        elicitation
            .request(self.server_id.clone(), request_id, params, context.ct)
            .await
    }

    async fn on_tool_list_changed(
        &self,
        context: rmcp::service::NotificationContext<rmcp::service::RoleClient>,
    ) {
        let seq = self
            .next_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        info!(server = %self.server_id, seq, "MCP tools/list_changed, refreshing");
        match context.peer.list_all_tools().await {
            Ok(new_tools) => {
                let count = new_tools.len();
                let mut guard = self.state.lock().await;
                // 只接受比当前更新的序列号，防止旧响应覆盖新响应
                if seq > guard.generation {
                    guard.tools = new_tools;
                    guard.generation = seq;
                    info!(server = %self.server_id, tool_count = count, seq, "MCP tool list refreshed");
                } else {
                    info!(server = %self.server_id, seq, current = guard.generation, "skipping stale refresh");
                }
            }
            Err(e) => {
                warn!(server = %self.server_id, error = %e, "Failed to refresh tool list on notification");
            }
        }
    }
}

/// 已建立连接的服务器运行时状态。
struct RunningServer {
    /// 配置指纹，变更时需重连。
    fingerprint: String,
    /// MCP peer。
    peer: Peer<RoleClient>,
    /// 持有连接生命周期（泛型擦除为 trait object 以兼容不同 handler 类型）。
    _service: Box<dyn std::any::Any + Send>,
    /// 原生工具列表（通知驱动更新时通过 shared_state 同步）。
    tools: Vec<RmcpTool>,
    /// 上次同步的 generation。
    synced_generation: u64,
    /// 与通知处理器共享的工具状态。
    shared_state: Arc<TokioMutex<SharedToolState>>,
    /// 对应配置。
    config: McpServerConfig,
    /// 状态文案。
    status: String,
    /// 错误信息。
    error: Option<String>,
    /// 是否由持久化 OAuth 凭证建立连接。
    authenticated: bool,
    /// initialize 返回的 Server guidance（非配置持久化数据）。
    instructions: Option<String>,
}

/// 单个 Server 的连续失败与退避状态。
#[derive(Debug)]
struct RetryState {
    fingerprint: String,
    attempt: u32,
    retryable: bool,
    next_retry_at: Option<std::time::Instant>,
    next_retry_at_unix_ms: Option<u64>,
}

/// MCP Hub：按 Agent 管理多服务器连接与工具调用。
pub struct McpHub {
    /// 绑定的 Agent id（`None` 表示全局/当前约定）。
    agent_id: Option<String>,
    /// id → 运行中服务器。
    servers: HashMap<String, RunningServer>,
    /// 盘上配置快照（含未连接 / 禁用 server）。
    configs: Vec<McpServerConfig>,
    /// 最近一次 reload 的连接错误（server_id → message）。
    last_connect_errors: HashMap<String, String>,
    /// Server 生命周期状态。
    states: HashMap<String, McpLifecycleState>,
    /// Server 自动重连退避状态。
    retries: HashMap<String, RetryState>,
    /// 当前 session/profile 对 MCP 连接施加的不可变权限快照。
    execution_context: Option<McpExecutionContext>,
    elicitation: Arc<McpElicitationBroker>,
    event_streams: Arc<crate::McpEventStreamManager>,
    event_stream_updates: Option<tokio::sync::mpsc::Receiver<crate::McpEventStreamUpdate>>,
    event_stream_access_generation: tokio::sync::watch::Sender<u64>,
}

impl Default for McpHub {
    /// 等价于 [`McpHub::new`]。
    fn default() -> Self {
        Self::new()
    }
}

impl McpHub {
    /// 创建未绑定 Agent、无连接的空 Hub。
    pub fn new() -> Self {
        let (event_streams, event_stream_updates) = crate::McpEventStreamManager::new();
        let (event_stream_access_generation, _) = tokio::sync::watch::channel(0_u64);
        Self {
            agent_id: None,
            servers: HashMap::new(),
            configs: Vec::new(),
            last_connect_errors: HashMap::new(),
            states: HashMap::new(),
            retries: HashMap::new(),
            execution_context: None,
            elicitation: Arc::new(McpElicitationBroker::new()),
            event_streams: Arc::new(event_streams),
            event_stream_updates: Some(event_stream_updates),
            event_stream_access_generation,
        }
    }

    pub fn event_stream_manager(&self) -> Arc<crate::McpEventStreamManager> {
        Arc::clone(&self.event_streams)
    }

    pub fn event_stream_access_generation(&self) -> tokio::sync::watch::Receiver<u64> {
        self.event_stream_access_generation.subscribe()
    }

    pub fn take_event_stream_updates(
        &mut self,
    ) -> Option<tokio::sync::mpsc::Receiver<crate::McpEventStreamUpdate>> {
        self.event_stream_updates.take()
    }

    pub fn elicitation_broker(&self) -> Arc<McpElicitationBroker> {
        Arc::clone(&self.elicitation)
    }

    pub fn set_elicitation_broker(&mut self, broker: Arc<McpElicitationBroker>) {
        self.elicitation = broker;
    }

    /// 当前绑定的 Agent id（若有）。
    pub fn agent_id(&self) -> Option<&str> {
        self.agent_id.as_deref()
    }

    /// 设置绑定的 Agent id（不立即重连）。
    pub fn set_agent_id(&mut self, agent_id: Option<String>) {
        self.agent_id = agent_id;
    }

    /// 设置后续连接/重连使用的权限快照。
    ///
    /// 传 `None` 表示策略解析失败或尚未配置；所有连接都会 fail closed。
    pub fn set_execution_context(&mut self, context: Option<McpExecutionContext>) {
        let changed = self
            .execution_context
            .as_ref()
            .map(McpExecutionContext::fingerprint)
            != context.as_ref().map(McpExecutionContext::fingerprint);
        self.execution_context = context;
        if changed {
            let next = self.event_stream_access_generation.borrow().wrapping_add(1);
            self.event_stream_access_generation.send_replace(next);
        }
    }

    /// 从磁盘重载：保留未变连接，重连变更项，断开已删除/禁用项
    pub async fn reload_from_disk(&mut self, agent_id: Option<&str>) -> anyhow::Result<()> {
        let id = agent_id
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        self.agent_id = id.clone();
        let project_root = self
            .execution_context
            .as_ref()
            .map(|context| context.working_dir.as_path());
        let configs = load_mcp_servers_layered(project_root)?;
        self.reload_with_configs(configs).await
    }

    /// 按给定配置表重连：保留指纹未变的连接，其余重连或断开。
    pub async fn reload_with_configs(
        &mut self,
        configs: Vec<McpServerConfig>,
    ) -> anyhow::Result<()> {
        self.configs = configs;
        let enabled: Vec<_> = self.configs.iter().filter(|c| c.enabled).cloned().collect();
        let configured_ids: std::collections::HashSet<String> = self
            .configs
            .iter()
            .map(|config| sanitize_server_id(&config.id))
            .collect();
        self.states.retain(|id, _| configured_ids.contains(id));
        self.retries.retain(|id, _| configured_ids.contains(id));
        self.last_connect_errors
            .retain(|id, _| configured_ids.contains(id));
        for config in self.configs.iter().filter(|config| !config.enabled) {
            let sid = sanitize_server_id(&config.id);
            self.states.insert(sid.clone(), McpLifecycleState::Disabled);
            self.retries.remove(&sid);
            self.last_connect_errors.remove(&sid);
        }

        let keep_ids: std::collections::HashSet<String> =
            enabled.iter().map(|c| sanitize_server_id(&c.id)).collect();

        // 断开不再需要的
        let drop_ids: Vec<String> = self
            .servers
            .keys()
            .filter(|id| !keep_ids.contains(*id))
            .cloned()
            .collect();
        for id in drop_ids {
            self.event_streams.abort_server(&id);
            self.servers.remove(&id);
        }

        let mut discovered_updates: Vec<(String, Vec<DiscoveredTool>)> = Vec::new();
        let mut connect_errors = self.last_connect_errors.clone();
        let mut pending = Vec::new();
        let mut required_failures = Vec::new();
        let now = std::time::Instant::now();

        for cfg in enabled {
            let sid = sanitize_server_id(&cfg.id);
            let fp = effective_connection_fingerprint(&cfg, self.execution_context.as_ref());
            if let Some(existing) = self.servers.get(&sid) {
                if existing.fingerprint == fp
                    && existing.status == "connected"
                    && !existing.peer.is_transport_closed()
                {
                    if let Some(rs) = self.servers.get_mut(&sid) {
                        rs.config = cfg;
                        rs.error = None;
                    }
                    self.states
                        .insert(sid.clone(), McpLifecycleState::Connected);
                    self.retries.remove(&sid);
                    connect_errors.remove(&sid);
                    continue;
                }
            }

            if self
                .retries
                .get(&sid)
                .is_some_and(|retry| retry.fingerprint != fp)
            {
                self.retries.remove(&sid);
                connect_errors.remove(&sid);
            }
            if let Some(retry) = self.retries.get(&sid) {
                let waiting = retry
                    .next_retry_at
                    .is_some_and(|next_retry_at| next_retry_at > now);
                if !retry.retryable || waiting {
                    self.states.insert(
                        sid.clone(),
                        if retry.retryable {
                            McpLifecycleState::Backoff
                        } else if self.states.get(&sid) == Some(&McpLifecycleState::AuthRequired) {
                            McpLifecycleState::AuthRequired
                        } else {
                            McpLifecycleState::Error
                        },
                    );
                    if cfg.required {
                        required_failures.push(McpStartupFailure {
                            server_id: sid.clone(),
                            server_name: cfg.name,
                            error: connect_errors
                                .get(&sid)
                                .cloned()
                                .unwrap_or_else(|| "MCP server is waiting for reconnect".into()),
                        });
                    }
                    continue;
                }
            }
            self.servers.remove(&sid);
            self.event_streams.abort_server(&sid);
            self.states.insert(sid, McpLifecycleState::Connecting);
            connect_errors.remove(&sanitize_server_id(&cfg.id));
            pending.push(cfg);
        }

        let execution_context = self.execution_context.clone();
        let elicitation = Arc::clone(&self.elicitation);
        let futures = pending
            .into_iter()
            .map(|cfg| {
                let execution_context = execution_context.clone();
                let elicitation = Arc::clone(&elicitation);
                async move {
                    let result =
                        connect_server(&cfg, execution_context.as_ref(), Some(elicitation)).await;
                    (cfg, result)
                }
            })
            .collect();
        let connection_results = collect_bounded(futures, MAX_PARALLEL_MCP_STARTUPS).await;
        for (cfg, result) in connection_results {
            let sid = sanitize_server_id(&cfg.id);
            match result {
                Ok(mut running) => {
                    running.config = cfg.clone();
                    let discovered: Vec<DiscoveredTool> = running
                        .tools
                        .iter()
                        .map(|t| DiscoveredTool {
                            name: t.name.to_string(),
                            description: t
                                .description
                                .as_ref()
                                .map(|d| d.to_string())
                                .unwrap_or_default(),
                            annotations: tool_annotations(t),
                        })
                        .collect();
                    if let Some(slot) = self
                        .configs
                        .iter_mut()
                        .find(|c| sanitize_server_id(&c.id) == sid)
                    {
                        merge_discovered(slot, discovered.clone());
                    }
                    discovered_updates.push((sid.clone(), discovered));
                    self.states
                        .insert(sid.clone(), McpLifecycleState::Connected);
                    self.retries.remove(&sid);
                    connect_errors.remove(&sid);
                    self.servers.insert(sid, running);
                }
                Err(e) => {
                    warn!(server = %sid, error = %e, "MCP server connect failed");
                    let error = e.to_string();
                    let retryable = is_retryable_connect_error(&e);
                    let attempt = self
                        .retries
                        .get(&sid)
                        .filter(|retry| {
                            retry.fingerprint
                                == effective_connection_fingerprint(
                                    &cfg,
                                    self.execution_context.as_ref(),
                                )
                        })
                        .map_or(1, |retry| retry.attempt.saturating_add(1));
                    let (next_retry_at, next_retry_at_unix_ms) = if retryable {
                        let delay = retry_delay(&sid, attempt);
                        (
                            Some(std::time::Instant::now() + delay),
                            Some(unix_time_ms().saturating_add(delay.as_millis() as u64)),
                        )
                    } else {
                        (None, None)
                    };
                    self.retries.insert(
                        sid.clone(),
                        RetryState {
                            fingerprint: effective_connection_fingerprint(
                                &cfg,
                                self.execution_context.as_ref(),
                            ),
                            attempt,
                            retryable,
                            next_retry_at,
                            next_retry_at_unix_ms,
                        },
                    );
                    let auth_required = is_auth_required_error(&cfg, &e);
                    self.states.insert(
                        sid.clone(),
                        if auth_required {
                            McpLifecycleState::AuthRequired
                        } else if retryable {
                            McpLifecycleState::Backoff
                        } else {
                            McpLifecycleState::Error
                        },
                    );
                    connect_errors.insert(sid.clone(), error.clone());
                    if cfg.required {
                        required_failures.push(McpStartupFailure {
                            server_id: sid,
                            server_name: cfg.name,
                            error,
                        });
                    }
                }
            }
        }

        // 只向当前可写层已显式定义的 Server 补丁 discovered，不摊平继承配置。
        if !discovered_updates.is_empty() {
            let project_root = self
                .execution_context
                .as_ref()
                .map(|context| context.working_dir.as_path());
            if let Err(e) = persist_discovered_layered(project_root, &discovered_updates) {
                warn!(error = %e, "persist MCP discovered failed");
            } else if let Ok(fresh) = load_mcp_servers_layered(project_root) {
                for cfg in fresh {
                    let sid = sanitize_server_id(&cfg.id);
                    if let Some(slot) = self
                        .configs
                        .iter_mut()
                        .find(|c| sanitize_server_id(&c.id) == sid)
                    {
                        slot.tools = cfg.tools.clone();
                        slot.enabled_tools = cfg.enabled_tools.clone();
                        slot.disabled_tools = cfg.disabled_tools.clone();
                        slot.default_tools_approval_mode = cfg.default_tools_approval_mode;
                        slot.enabled = cfg.enabled;
                    }
                    if let Some(rs) = self.servers.get_mut(&sid) {
                        rs.config.tools = cfg.tools;
                        rs.config.enabled_tools = cfg.enabled_tools;
                        rs.config.disabled_tools = cfg.disabled_tools;
                        rs.config.default_tools_approval_mode = cfg.default_tools_approval_mode;
                        if let Some(slot) = self
                            .configs
                            .iter()
                            .find(|c| sanitize_server_id(&c.id) == sid)
                        {
                            rs.config.discovered = slot.discovered.clone();
                        }
                        rs.config.enabled = cfg.enabled;
                    }
                }
            }
        }

        // 把连接错误挂到 configs 状态（无 RunningServer 时 server_status 可读）
        self.last_connect_errors = connect_errors;
        if required_failures.is_empty() {
            Ok(())
        } else {
            Err(RequiredMcpServersError {
                failures: required_failures,
            }
            .into())
        }
    }

    /// 清除指定 Server 的退避并丢弃现有连接，使下一次 reload 立即重连。
    pub fn force_reconnect(&mut self, server_id: &str) -> anyhow::Result<()> {
        let sid = sanitize_server_id(server_id);
        if !self
            .configs
            .iter()
            .any(|config| sanitize_server_id(&config.id) == sid)
        {
            anyhow::bail!("unknown MCP server: {sid}");
        }
        self.retries.remove(&sid);
        self.last_connect_errors.remove(&sid);
        self.servers.remove(&sid);
        self.event_streams.abort_server(&sid);
        self.states.insert(sid, McpLifecycleState::Disconnected);
        Ok(())
    }

    /// 仅从磁盘同步 enabled / tools 开关，不重连（供工具调用路径）
    pub fn sync_enablement_from_disk(&mut self) -> anyhow::Result<()> {
        let project_root = self
            .execution_context
            .as_ref()
            .map(|context| context.working_dir.as_path());
        let fresh = load_mcp_servers_layered(project_root)?;
        let mut seen = std::collections::HashSet::new();
        for cfg in &fresh {
            let sid = sanitize_server_id(&cfg.id);
            seen.insert(sid.clone());
            if let Some(slot) = self
                .configs
                .iter_mut()
                .find(|c| sanitize_server_id(&c.id) == sid)
            {
                slot.enabled = cfg.enabled;
                slot.tools = cfg.tools.clone();
                slot.enabled_tools = cfg.enabled_tools.clone();
                slot.disabled_tools = cfg.disabled_tools.clone();
                slot.default_tools_approval_mode = cfg.default_tools_approval_mode;
            } else {
                self.configs.push(cfg.clone());
            }
            if let Some(rs) = self.servers.get_mut(&sid) {
                rs.config.enabled = cfg.enabled;
                rs.config.tools = cfg.tools.clone();
                rs.config.enabled_tools = cfg.enabled_tools.clone();
                rs.config.disabled_tools = cfg.disabled_tools.clone();
                rs.config.default_tools_approval_mode = cfg.default_tools_approval_mode;
            }
            if !cfg.enabled {
                self.states.insert(sid, McpLifecycleState::Disabled);
            }
        }
        // 磁盘上已删除的配置：从内存 configs 去掉（连接由完整 reload 清理）
        self.configs
            .retain(|c| seen.contains(&sanitize_server_id(&c.id)));
        Ok(())
    }

    /// 仅暴露：server.enabled && tool.enabled && 已连接
    pub fn enabled_tool_entries(&mut self) -> Vec<ToolEntrySpec> {
        // 同步通知驱动的工具列表更新（基于 generation 精确检测变更）
        for rs in self.servers.values_mut() {
            if let Ok(state) = rs.shared_state.try_lock() {
                if state.generation > rs.synced_generation {
                    rs.tools = state.tools.clone();
                    rs.synced_generation = state.generation;
                }
            }
        }

        let mut out = Vec::new();
        for (sid, rs) in &self.servers {
            if rs.status != "connected" || !rs.config.enabled {
                continue;
            }
            // 活性检测：transport 已关闭的 server 跳过
            if rs.peer.is_transport_closed() {
                tracing::debug!(server = %sid, "MCP server transport closed, skipping tools");
                continue;
            }
            for tool in &rs.tools {
                let native = tool.name.as_ref();
                if !rs.config.is_tool_enabled(native) {
                    continue;
                }
                let schema = Value::Object(tool.input_schema.as_ref().clone());
                out.push(ToolEntrySpec {
                    qualified_name: qualify_tool_name(sid, native),
                    namespace: tool_namespace(sid),
                    server_id: sid.clone(),
                    native_name: native.to_string(),
                    description: tool
                        .description
                        .as_ref()
                        .map(|d| d.to_string())
                        .unwrap_or_else(|| format!("MCP tool {native} via {}", rs.config.name)),
                    schema,
                    approval_mode: rs.config.tool_approval_mode(native),
                    annotations: tool_annotations(tool),
                    output_token_limit: rs.config.tool_output_token_limit(native),
                });
            }
        }
        out
    }

    /// 当前健康连接提供的 Server instructions，按稳定 Server id 排序并施加总上限。
    pub fn server_instructions(&self) -> Vec<McpServerInstructions> {
        let entries: Vec<_> = self
            .servers
            .iter()
            .filter(|(_, server)| {
                server.status == "connected"
                    && server.config.enabled
                    && !server.peer.is_transport_closed()
            })
            .filter_map(|(server_id, server)| {
                server
                    .instructions
                    .as_ref()
                    .map(|instructions| McpServerInstructions {
                        server_id: server_id.clone(),
                        server_name: server.config.name.clone(),
                        instructions: instructions.clone(),
                    })
            })
            .collect();
        bound_server_instructions(entries)
    }

    /// 仅统计健康、启用且 initialize 明确宣告的内容能力。
    pub fn broker_capabilities(&self) -> McpBrokerCapabilities {
        let mut capabilities = McpBrokerCapabilities::default();
        for (server_id, server) in self.servers.iter().filter(|(_, server)| {
            server.status == "connected"
                && server.config.enabled
                && !server.peer.is_transport_closed()
        }) {
            let Some(info) = server.peer.peer_info() else {
                continue;
            };
            if info.capabilities.resources.is_some() {
                capabilities.resource_servers.push(server_id.clone());
            }
            if info.capabilities.prompts.is_some() {
                capabilities.prompt_servers.push(server_id.clone());
            }
        }
        capabilities.resource_servers.sort();
        capabilities.resource_servers.dedup();
        capabilities.prompt_servers.sort();
        capabilities.prompt_servers.dedup();
        capabilities
    }

    /// 查询某服务器连接状态摘要。
    pub fn server_status(&self) -> Vec<ServerStatus> {
        let mut out = Vec::new();
        for cfg in &self.configs {
            let sid = sanitize_server_id(&cfg.id);
            if let Some(rs) = self.servers.get(&sid) {
                let state = if rs.peer.is_transport_closed() {
                    McpLifecycleState::Disconnected
                } else {
                    self.states
                        .get(&sid)
                        .copied()
                        .unwrap_or(McpLifecycleState::Connected)
                };
                out.push(ServerStatus {
                    id: sid,
                    name: cfg.name.clone(),
                    required: cfg.required,
                    status: state.as_str().into(),
                    tools: rs.tools.iter().map(|t| t.name.to_string()).collect(),
                    error: rs.error.clone(),
                    retryable: false,
                    retry_attempt: 0,
                    next_retry_at_unix_ms: None,
                    oauth_available: crate::auth::is_oauth_available(cfg),
                    authenticated: rs.authenticated,
                });
            } else {
                let err = self.last_connect_errors.get(&sid).cloned();
                let retry = self.retries.get(&sid);
                out.push(ServerStatus {
                    id: sid,
                    name: cfg.name.clone(),
                    required: cfg.required,
                    status: self
                        .states
                        .get(&sanitize_server_id(&cfg.id))
                        .copied()
                        .unwrap_or(if !cfg.enabled {
                            McpLifecycleState::Disabled
                        } else if err.is_some() {
                            McpLifecycleState::Error
                        } else {
                            McpLifecycleState::Disconnected
                        })
                        .as_str()
                        .into(),
                    tools: cfg.discovered.iter().map(|d| d.name.clone()).collect(),
                    error: err,
                    retryable: retry.is_some_and(|retry| retry.retryable),
                    retry_attempt: retry.map_or(0, |retry| retry.attempt),
                    next_retry_at_unix_ms: retry.and_then(|retry| retry.next_retry_at_unix_ms),
                    oauth_available: crate::auth::is_oauth_available(cfg),
                    authenticated: false,
                });
            }
        }
        out
    }

    /// 当前内存中的服务器配置列表。
    pub fn configs(&self) -> &[McpServerConfig] {
        &self.configs
    }

    /// 校验工具调用权限并返回 peer 克隆 + 超时配置，供 lock 外异步调用。
    ///
    /// 成功时返回 `(peer, native_tool_name, timeout_secs, output_token_limit)`；调用方在释放
    /// `MutexGuard` 后再执行 `call_tool_with_peer`。
    pub fn resolve_tool_peer(
        &self,
        qualified_name: &str,
    ) -> anyhow::Result<(Peer<RoleClient>, String, u64, Option<usize>)> {
        if !is_mcp_tool_name(qualified_name) {
            anyhow::bail!("不是 MCP 工具: {qualified_name}");
        }
        let (server_id, sanitized_native) = parse_qualified_name(qualified_name)
            .ok_or_else(|| anyhow!("无效 MCP 工具名: {qualified_name}"))?;

        let rs = self
            .servers
            .get(server_id)
            .ok_or_else(|| anyhow!("MCP server 未连接: {server_id}"))?;

        if !rs.config.enabled {
            anyhow::bail!("MCP server 已禁用: {server_id}");
        }

        // sanitized name → 原始 native name 反查
        let native = rs
            .tools
            .iter()
            .find(|t| qualify_tool_name(server_id, t.name.as_ref()) == qualified_name)
            .map(|t| t.name.to_string())
            .unwrap_or_else(|| sanitized_native.to_string());

        if !rs.config.is_tool_enabled(&native) {
            anyhow::bail!("MCP 工具已禁用: {qualified_name}");
        }

        let timeout_secs = rs.config.effective_tool_timeout_secs();
        let output_token_limit = rs.config.tool_output_token_limit(&native);
        Ok((rs.peer.clone(), native, timeout_secs, output_token_limit))
    }

    fn resolve_broker_peer(
        &self,
        server_id: &str,
        capability: &str,
        supports: impl FnOnce(&rmcp::model::ServerCapabilities) -> bool,
    ) -> anyhow::Result<(Peer<RoleClient>, String, u64)> {
        let requested = server_id.trim();
        if requested.is_empty() {
            anyhow::bail!("server_id 不能为空");
        }
        let server_id = sanitize_server_id(requested);
        let server = self
            .servers
            .get(&server_id)
            .ok_or_else(|| anyhow!("MCP server 未连接: {server_id}"))?;
        if server.status != "connected" || !server.config.enabled {
            anyhow::bail!("MCP server 不可用: {server_id}");
        }
        if server.peer.is_transport_closed() {
            anyhow::bail!("MCP server 连接已关闭: {server_id}");
        }
        let info = server
            .peer
            .peer_info()
            .ok_or_else(|| anyhow!("MCP server 缺少 initialize 结果: {server_id}"))?;
        if !supports(&info.capabilities) {
            anyhow::bail!("MCP server {server_id} 未声明 {capability} capability");
        }
        Ok((
            server.peer.clone(),
            server_id,
            server.config.effective_tool_timeout_secs(),
        ))
    }

    pub(crate) fn resolve_resource_peer(
        &self,
        server_id: &str,
    ) -> anyhow::Result<(Peer<RoleClient>, String, u64)> {
        self.resolve_broker_peer(server_id, "resources", |caps| caps.resources.is_some())
    }

    pub(crate) fn resolve_prompt_peer(
        &self,
        server_id: &str,
    ) -> anyhow::Result<(Peer<RoleClient>, String, u64)> {
        self.resolve_broker_peer(server_id, "prompts", |caps| caps.prompts.is_some())
    }

    /// 调用已连接 MCP 工具（按服务器与工具名）。
    pub fn call_tool(
        &self,
        qualified_name: &str,
        args: &Value,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<types::ToolOutput>> + Send + 'static>,
    > {
        let resolved = self.resolve_tool_peer(qualified_name);
        let qualified_name = qualified_name.to_string();
        let args = args.clone();
        Box::pin(async move {
            let (peer, native, timeout_secs, output_token_limit) = resolved?;
            call_tool_with_peer(
                &peer,
                &qualified_name,
                &native,
                &args,
                timeout_secs,
                output_token_limit,
            )
            .await
        })
    }

    /// 短连刷新某 server 的 discovered（供 UI refresh_mcp_tools）
    pub async fn refresh_discovered(
        server_id: Option<&str>,
        execution_context: &McpExecutionContext,
    ) -> anyhow::Result<Vec<McpServerConfig>> {
        let mut configs = load_mcp_servers_layered(Some(&execution_context.working_dir))?;
        let mut discovered_updates = Vec::new();
        for cfg in configs.iter_mut() {
            if let Some(want) = server_id {
                if sanitize_server_id(&cfg.id) != sanitize_server_id(want) {
                    continue;
                }
            }
            if !cfg.enabled && server_id.is_none() {
                continue;
            }
            match connect_server(cfg, Some(execution_context), None).await {
                Ok(running) => {
                    let discovered: Vec<DiscoveredTool> = running
                        .tools
                        .iter()
                        .map(|t| DiscoveredTool {
                            name: t.name.to_string(),
                            description: t
                                .description
                                .as_ref()
                                .map(|d| d.to_string())
                                .unwrap_or_default(),
                            annotations: tool_annotations(t),
                        })
                        .collect();
                    merge_discovered(cfg, discovered.clone());
                    discovered_updates.push((sanitize_server_id(&cfg.id), discovered));
                    // drop running → 关闭短连
                }
                Err(e) => {
                    warn!(server = %cfg.id, error = %e, "refresh discovered failed");
                }
            }
        }
        persist_discovered_layered(Some(&execution_context.working_dir), &discovered_updates)?;
        Ok(configs)
    }
}

/// 单次工具调用返回的最大媒体资产数（防止 MCP server 返回过多大 blob）。
const MAX_MEDIA_ASSETS: usize = 20;

/// 将 MCP content blocks 转为结构化 ToolOutput（保留 image/media 信息）。
fn content_to_tool_output(
    blocks: &[ContentBlock],
    output_token_limit: Option<usize>,
) -> types::ToolOutput {
    let mut text_parts = Vec::new();
    let mut media_assets = Vec::new();

    for b in blocks {
        match b {
            ContentBlock::Text(t) => {
                text_parts.push(t.text.clone());
            }
            ContentBlock::Image(img) if media_assets.len() < MAX_MEDIA_ASSETS => {
                let mime = img.mime_type.clone();
                let data_url = format!("data:{};base64,{}", mime, img.data);
                media_assets.push(types::MediaAsset {
                    kind: types::MediaKind::Image,
                    mime_type: mime,
                    reference: types::MediaRef::DataUrl(data_url),
                    label: None,
                    id: None,
                });
                text_parts.push("[image from MCP tool]".to_string());
            }
            ContentBlock::Audio(audio) if media_assets.len() < MAX_MEDIA_ASSETS => {
                let mime = audio.mime_type.clone();
                let data_url = format!("data:{};base64,{}", mime, audio.data);
                media_assets.push(types::MediaAsset {
                    kind: types::MediaKind::Audio,
                    mime_type: mime,
                    reference: types::MediaRef::DataUrl(data_url),
                    label: None,
                    id: None,
                });
                text_parts.push("[audio from MCP tool]".to_string());
            }
            ContentBlock::Image(_) | ContentBlock::Audio(_) => {
                text_parts.push("[media skipped: asset limit reached]".to_string());
            }
            ContentBlock::Resource(res) => {
                // 简化处理：序列化为 JSON 保留结构信息
                text_parts.push(json!(res).to_string());
            }
            _ => {
                text_parts.push(json!(b).to_string());
            }
        }
    }

    let text = if text_parts.is_empty() {
        "{}".to_string()
    } else {
        text_parts.join("\n")
    };
    let max_bytes = output_token_limit
        .map(|tokens| tokens.saturating_mul(24).div_ceil(5))
        .unwrap_or(types::MAX_TOOL_RESULT_BYTES)
        .min(types::MAX_TOOL_RESULT_BYTES);
    let text = types::truncate_tool_result(&text, max_bytes);

    if media_assets.is_empty() {
        types::ToolOutput::Text(text)
    } else {
        types::ToolOutput::Media {
            text,
            assets: media_assets,
        }
    }
}

/// 使用已解析的 `Peer` 执行 MCP 工具调用（lock-free，供 `Arc<Mutex<McpHub>>` 场景使用）。
///
/// `qualified_name` 仅用于错误消息；`native` 是原生工具名（不含 `mcp__` 前缀）。
pub async fn call_tool_with_peer(
    peer: &Peer<RoleClient>,
    qualified_name: &str,
    native: &str,
    args: &Value,
    timeout_secs: u64,
    output_token_limit: Option<usize>,
) -> anyhow::Result<types::ToolOutput> {
    let arguments = mcp_tool_arguments(args)?;

    let mut params = CallToolRequestParams::new(native.to_string());
    if let Some(args_map) = arguments {
        params = params.with_arguments(args_map);
    }

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        peer.call_tool(params),
    )
    .await
    .map_err(|_| anyhow!("MCP tool 调用超时 ({timeout_secs}s): {qualified_name}"))?
    .with_context(|| format!("call_tool {qualified_name}"))?;

    if result.is_error == Some(true) {
        let msg = content_to_tool_output(&result.content, output_token_limit).into_text();
        anyhow::bail!("MCP tool error: {msg}");
    }

    if let Some(structured) = result.structured_content {
        let max_bytes = output_token_limit
            .map(|tokens| tokens.saturating_mul(24).div_ceil(5))
            .unwrap_or(types::MAX_TOOL_RESULT_BYTES)
            .min(types::MAX_TOOL_RESULT_BYTES);
        return Ok(types::ToolOutput::Text(types::truncate_tool_result(
            &structured.to_string(),
            max_bytes,
        )));
    }
    Ok(content_to_tool_output(&result.content, output_token_limit))
}

/// 按配置建立 MCP 连接并拉取工具列表。
///
/// 使用 [`ToolChangeHandler`] 作为客户端 handler，自动处理
/// `tools/list_changed` 通知，实时同步工具列表。
async fn connect_server(
    cfg: &McpServerConfig,
    execution_context: Option<&McpExecutionContext>,
    elicitation: Option<Arc<McpElicitationBroker>>,
) -> anyhow::Result<RunningServer> {
    let sid = sanitize_server_id(&cfg.id);
    let timeout_secs = cfg.effective_startup_timeout_secs();
    with_startup_timeout(
        &sid,
        timeout_secs,
        connect_server_inner(cfg, execution_context, elicitation),
    )
    .await
}

async fn with_startup_timeout<T>(
    server_id: &str,
    timeout_secs: u64,
    future: impl std::future::Future<Output = anyhow::Result<T>>,
) -> anyhow::Result<T> {
    tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), future)
        .await
        .map_err(|_| anyhow!("MCP server 启动超时 ({timeout_secs}s): {server_id}"))?
}

fn is_retryable_connect_error(error: &anyhow::Error) -> bool {
    let message = format!("{error:#}").to_ascii_lowercase();
    let permanent_markers = [
        "execution policy is unavailable",
        "network access denied",
        "permission denied",
        "working directory",
        "escapes the execution root",
        "stdio command",
        "缺少 url",
        "invalid http header",
        "invalid environment variable name",
        "unauthorized",
        "forbidden",
        "status: 400",
        "status 400",
        "status: 401",
        "status 401",
        "status: 403",
        "status 403",
        "status: 404",
        "status 404",
    ];
    !permanent_markers
        .iter()
        .any(|marker| message.contains(marker))
}

fn is_auth_required_error(config: &McpServerConfig, error: &anyhow::Error) -> bool {
    if !crate::auth::is_oauth_available(config) || has_configured_authorization(config) {
        return false;
    }
    let message = format!("{error:#}").to_ascii_lowercase();
    message.contains("unauthorized")
        || message.contains("oauth authorization required")
        || message.contains("status: 401")
        || message.contains("status 401")
}

fn has_configured_authorization(config: &McpServerConfig) -> bool {
    config.bearer_token_env_var.is_some()
        || config
            .headers
            .keys()
            .chain(config.env_http_headers.keys())
            .any(|name| name.eq_ignore_ascii_case("authorization"))
}

fn retry_delay(server_id: &str, attempt: u32) -> std::time::Duration {
    let exponent = attempt.saturating_sub(1).min(5);
    let base_ms = (1_u64 << exponent) * 1_000;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    server_id.hash(&mut hasher);
    attempt.hash(&mut hasher);
    let jitter_per_mille = 800 + (hasher.finish() % 401);
    let delay_ms =
        (base_ms * jitter_per_mille / 1_000).min(MAX_MCP_RETRY_DELAY_SECS.saturating_mul(1_000));
    std::time::Duration::from_millis(delay_ms)
}

fn unix_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

async fn collect_bounded<F, T>(futures: Vec<F>, limit: usize) -> Vec<T>
where
    F: std::future::Future<Output = T>,
{
    stream::iter(futures)
        .buffer_unordered(limit.max(1))
        .collect()
        .await
}

/// 实际建连流程；由 [`connect_server`] 对完整启动阶段施加统一超时。
async fn connect_server_inner(
    cfg: &McpServerConfig,
    execution_context: Option<&McpExecutionContext>,
    elicitation: Option<Arc<McpElicitationBroker>>,
) -> anyhow::Result<RunningServer> {
    let sid = sanitize_server_id(&cfg.id);
    let execution_context = execution_context.ok_or_else(|| {
        anyhow!("MCP execution policy is unavailable; refusing to connect server {sid}")
    })?;
    let fingerprint = effective_connection_fingerprint(cfg, Some(execution_context));

    let shared_state = Arc::new(TokioMutex::new(SharedToolState {
        tools: Vec::new(),
        generation: 0,
    }));
    let handler = ToolChangeHandler {
        state: Arc::clone(&shared_state),
        server_id: sid.clone(),
        next_seq: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        elicitation,
    };

    let mut authenticated = false;
    let service = match cfg.r#type {
        McpTransportType::Stdio => {
            let cmd = build_stdio_command(cfg, execution_context)?;
            let audit = execution_context.sandbox_audit_for(&sid);
            let spawn_started = std::time::Instant::now();
            let transport = match TokioChildProcess::new(cmd) {
                Ok(transport) => {
                    if let Some(audit) = &audit {
                        audit.record(
                            sandbox::SandboxAuditKind::Spawned,
                            Some(&execution_context.sandbox_policy),
                            "stdio",
                            "spawned",
                            Some(spawn_started.elapsed().as_millis() as u64),
                        );
                    }
                    transport
                }
                Err(error) => {
                    if let Some(audit) = &audit {
                        audit.record(
                            sandbox::SandboxAuditKind::Denied,
                            Some(&execution_context.sandbox_policy),
                            "stdio",
                            "spawn_failed",
                            Some(spawn_started.elapsed().as_millis() as u64),
                        );
                    }
                    return Err(error.into());
                }
            };
            handler.serve(transport).await.context("stdio serve")?
        }
        McpTransportType::StreamableHttp => {
            if !execution_context.sandbox_policy.network_access {
                anyhow::bail!(
                    "network access denied by the active permission profile for MCP server {sid}"
                );
            }
            if cfg.url.trim().is_empty() {
                anyhow::bail!("HTTP MCP server 缺少 url");
            }
            if cfg.auth == Some(McpHttpAuth::Chatgpt) {
                anyhow::bail!(
                    "ChatGPT session authentication is only available to trusted first-party integrations"
                );
            }
            let map = resolve_http_headers(cfg)?;
            let mut config = StreamableHttpClientTransportConfig::with_uri(cfg.url.as_str());
            if !map.is_empty() {
                let has_static_authorization = map.contains_key(&AUTHORIZATION);
                config = config.custom_headers(map);
                let (client, used_oauth) =
                    crate::auth::http_client_for(cfg, !has_static_authorization)
                        .await
                        .context("load MCP OAuth credentials")?;
                authenticated = used_oauth;
                let transport = StreamableHttpClientTransport::with_client(client, config);
                handler.serve(transport).await.context("http serve")?
            } else {
                let (client, used_oauth) = crate::auth::http_client_for(cfg, true)
                    .await
                    .context("load MCP OAuth credentials")?;
                authenticated = used_oauth;
                let transport = StreamableHttpClientTransport::with_client(client, config);
                handler.serve(transport).await.context("http serve")?
            }
        }
    };

    let peer = service.peer().clone();
    let peer_info = peer.peer_info();
    let instructions = server_instructions_from_info(peer_info.as_deref());
    let tools = peer.list_all_tools().await.context("list_all_tools")?;
    {
        let mut state = shared_state.lock().await;
        state.tools = tools.clone();
        state.generation = 1;
    }
    info!(
        server = %sid,
        tool_count = tools.len(),
        "MCP server connected"
    );

    Ok(RunningServer {
        fingerprint,
        peer,
        _service: Box::new(service),
        tools,
        synced_generation: 1,
        shared_state,
        config: cfg.clone(),
        status: "connected".into(),
        error: None,
        authenticated,
        instructions,
    })
}

const SAFE_PARENT_ENV_KEYS: &[&str] = &[
    "PATH", "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "LC_CTYPE", "TERM", "TMPDIR", "TMP",
    "TEMP", "SHELL",
];

fn scrubbed_parent_env(
    parent: impl IntoIterator<Item = (impl AsRef<str>, impl AsRef<str>)>,
) -> HashMap<String, String> {
    parent
        .into_iter()
        .filter_map(|(key, value)| {
            let key = key.as_ref();
            SAFE_PARENT_ENV_KEYS
                .contains(&key)
                .then(|| (key.to_string(), value.as_ref().to_string()))
        })
        .collect()
}

fn effective_connection_fingerprint(
    cfg: &McpServerConfig,
    execution_context: Option<&McpExecutionContext>,
) -> String {
    let policy = execution_context
        .map(McpExecutionContext::fingerprint)
        .unwrap_or_else(|| "unconfigured".to_string());
    let credential_hash = credential_fingerprint_with(cfg, |name| std::env::var(name).ok());
    format!(
        "{}|{policy}|{credential_hash:016x}",
        cfg.connection_fingerprint()
    )
}

fn is_valid_env_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('_' | 'a'..='z' | 'A'..='Z'))
        && chars.all(|ch| matches!(ch, '_' | 'a'..='z' | 'A'..='Z' | '0'..='9'))
}

fn validate_env_var_name(name: &str) -> anyhow::Result<&str> {
    let name = name.trim();
    if !is_valid_env_var_name(name) {
        anyhow::bail!("invalid environment variable name: {name:?}");
    }
    Ok(name)
}

fn resolve_forwarded_env_with(
    cfg: &McpServerConfig,
    mut lookup: impl FnMut(&str) -> Option<String>,
) -> anyhow::Result<HashMap<String, String>> {
    let mut resolved = HashMap::new();
    for configured_name in &cfg.env_vars {
        let name = validate_env_var_name(configured_name)?;
        if let Some(value) = lookup(name) {
            resolved.insert(name.to_string(), value);
        }
    }
    Ok(resolved)
}

fn resolve_http_headers_with(
    cfg: &McpServerConfig,
    mut lookup: impl FnMut(&str) -> Option<String>,
) -> anyhow::Result<HashMap<HeaderName, HeaderValue>> {
    let mut resolved = HashMap::new();
    for (configured_name, configured_value) in &cfg.headers {
        let name = HeaderName::try_from(configured_name.as_str())
            .with_context(|| format!("invalid HTTP header name: {configured_name}"))?;
        let value = HeaderValue::try_from(configured_value.as_str())
            .with_context(|| format!("invalid HTTP header value for {configured_name}"))?;
        resolved.insert(name, value);
    }
    for (configured_header, configured_env_var) in &cfg.env_http_headers {
        let name = HeaderName::try_from(configured_header.as_str())
            .with_context(|| format!("invalid HTTP header name: {configured_header}"))?;
        let env_var = validate_env_var_name(configured_env_var)?;
        let Some(raw_value) = lookup(env_var) else {
            continue;
        };
        let value = HeaderValue::try_from(raw_value.as_str()).with_context(|| {
            format!("invalid HTTP header value from environment variable {env_var}")
        })?;
        resolved.insert(name, value);
    }
    if let Some(configured_env_var) = cfg.bearer_token_env_var.as_deref() {
        let env_var = validate_env_var_name(configured_env_var)?;
        if let Some(token) = lookup(env_var).filter(|value| !value.is_empty()) {
            let value = HeaderValue::try_from(format!("Bearer {token}")).with_context(|| {
                format!("invalid bearer token from environment variable {env_var}")
            })?;
            resolved.entry(AUTHORIZATION).or_insert(value);
        }
    }
    Ok(resolved)
}

fn resolve_http_headers(cfg: &McpServerConfig) -> anyhow::Result<HashMap<HeaderName, HeaderValue>> {
    resolve_http_headers_with(cfg, |name| std::env::var(name).ok())
}

fn credential_fingerprint_with(
    cfg: &McpServerConfig,
    mut lookup: impl FnMut(&str) -> Option<String>,
) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for name in &cfg.env_vars {
        name.hash(&mut hasher);
        lookup(name.trim()).hash(&mut hasher);
    }
    if let Some(name) = cfg.bearer_token_env_var.as_deref() {
        name.hash(&mut hasher);
        lookup(name.trim()).hash(&mut hasher);
    }
    let mut env_headers = cfg.env_http_headers.iter().collect::<Vec<_>>();
    env_headers.sort_unstable_by(|left, right| left.0.cmp(right.0));
    for (header, name) in env_headers {
        header.hash(&mut hasher);
        name.hash(&mut hasher);
        lookup(name.trim()).hash(&mut hasher);
    }
    hasher.finish()
}

fn build_stdio_command(
    cfg: &McpServerConfig,
    execution_context: &McpExecutionContext,
) -> anyhow::Result<tokio::process::Command> {
    let audit = execution_context.sandbox_audit_for(&sanitize_server_id(&cfg.id));
    validate_stdio_command(&cfg.command).inspect_err(|_error| {
        if let Some(audit) = &audit {
            audit.record(
                sandbox::SandboxAuditKind::Denied,
                Some(&execution_context.sandbox_policy),
                "stdio",
                "command_validation_failed",
                None,
            );
        }
    })?;
    let working_dir = resolve_server_working_dir(cfg, execution_context).inspect_err(|_error| {
        if let Some(audit) = &audit {
            audit.record(
                sandbox::SandboxAuditKind::Denied,
                Some(&execution_context.sandbox_policy),
                "stdio",
                "working_directory_denied",
                None,
            );
        }
    })?;
    let forwarded_env = resolve_forwarded_env_with(cfg, |name| std::env::var(name).ok())?;
    let prepare_started = std::time::Instant::now();
    let mut cmd = match sandbox::SandboxRunner
        .tokio_command(&execution_context.sandbox_policy, &cfg.command)
    {
        Ok(command) => command,
        Err(error) => {
            if let Some(audit) = &audit {
                audit.record_prepare_error(
                    Some(&execution_context.sandbox_policy),
                    "stdio",
                    &error,
                    Some(prepare_started.elapsed().as_millis() as u64),
                );
            }
            return Err(error).context("prepare sandboxed MCP stdio command");
        }
    };
    cmd.args(&cfg.args)
        .current_dir(working_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .env_clear()
        .envs(scrubbed_parent_env(std::env::vars()))
        .envs(forwarded_env)
        .envs(&cfg.env);
    Ok(cmd)
}

fn resolve_server_working_dir(
    cfg: &McpServerConfig,
    execution_context: &McpExecutionContext,
) -> anyhow::Result<PathBuf> {
    let Some(configured) = cfg
        .cwd
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(execution_context.working_dir.clone());
    };
    let requested = Path::new(configured);
    let candidate = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        execution_context.working_dir.join(requested)
    };
    let candidate = candidate
        .canonicalize()
        .with_context(|| format!("resolve MCP server cwd: {}", candidate.display()))?;
    if !candidate.is_dir() {
        anyhow::bail!("MCP server cwd is not a directory: {}", candidate.display());
    }
    if !execution_context
        .allowed_working_roots
        .iter()
        .any(|root| candidate.starts_with(root))
    {
        anyhow::bail!(
            "MCP server cwd escapes the execution root and allowed extension roots: {} (roots: {})",
            candidate.display(),
            execution_context
                .allowed_working_roots
                .iter()
                .map(|root| root.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(candidate)
}

/// stdio command 最小约束：不经 shell；禁止 `..` 与危险字符；允许绝对路径或简单命令名（如 npx）
///
/// TOML 配置已通过全局/可信项目/Agent 分层校验；此处再挡明显的路径穿越 / 注入形态。
pub fn validate_stdio_command(command: &str) -> anyhow::Result<()> {
    let cmd = command.trim();
    if cmd.is_empty() {
        anyhow::bail!("stdio MCP server 缺少 command");
    }
    if cmd.contains('\0') {
        anyhow::bail!("stdio command 含非法空字符");
    }
    if cmd.contains("..") {
        anyhow::bail!("stdio command 不允许包含 '..'");
    }
    // 禁止 shell 元字符（我们不用 shell，但仍拒绝配置里的注入形态）
    const FORBIDDEN: &[char] = &[
        ';', '|', '&', '$', '`', '(', ')', '{', '}', '<', '>', '\n', '\r',
    ];
    if cmd.chars().any(|c| FORBIDDEN.contains(&c)) {
        anyhow::bail!("stdio command 含非法字符（禁止 shell 元字符）");
    }

    let is_abs = {
        #[cfg(windows)]
        {
            cmd.starts_with('/')
                || cmd.starts_with('\\')
                || (cmd.len() >= 3 && cmd.as_bytes().get(1) == Some(&b':'))
        }
        #[cfg(not(windows))]
        {
            cmd.starts_with('/')
        }
    };

    if is_abs {
        return Ok(());
    }

    // PATH 上的简单命令名：仅允许 [A-Za-z0-9._+-]
    if !cmd
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'))
    {
        anyhow::bail!("stdio command 必须是绝对路径，或简单命令名（如 npx / uvx）；当前: {cmd}");
    }
    Ok(())
}

/// 纯函数：根据配置过滤应暴露的工具名（单测用，不依赖真实连接）
pub fn filter_enabled_tool_names(
    server: &McpServerConfig,
    discovered_names: &[String],
) -> Vec<String> {
    if !server.enabled {
        return Vec::new();
    }
    let sid = sanitize_server_id(&server.id);
    discovered_names
        .iter()
        .filter(|n| server.is_tool_enabled(n))
        .map(|n| qualify_tool_name(&sid, n))
        .collect()
}

/// MCP 工具在注册表中的 toolset id（[`MCP_TOOLSET`]）。
pub fn toolset_name() -> &'static str {
    MCP_TOOLSET
}

/// 将 JSON 参数转为 MCP CallTool arguments map
fn mcp_tool_arguments(args: &Value) -> anyhow::Result<Option<serde_json::Map<String, Value>>> {
    match args {
        Value::Null => Ok(None),
        Value::Object(map) => Ok(Some(map.clone())),
        other => anyhow::bail!("MCP 工具参数必须是 JSON object，收到: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_tool_output_limit_truncates_mcp_text() {
        let output = content_to_tool_output(
            &[ContentBlock::Text(rmcp::model::TextContent::new(
                "x".repeat(100),
            ))],
            Some(1),
        );
        assert!(output.text().starts_with("xxxxx"));
        assert!(output.text().contains("[truncated]"));
        assert!(content_to_tool_output(
            &[ContentBlock::Text(rmcp::model::TextContent::new("short"))],
            Some(10),
        )
        .text()
        .eq("short"));
    }

    fn stdio_server(command: &str) -> McpServerConfig {
        McpServerConfig {
            id: "s1".into(),
            name: "demo".into(),
            description: String::new(),
            r#type: McpTransportType::Stdio,
            command: command.into(),
            args: vec![],
            env: HashMap::new(),
            env_vars: vec![],
            url: String::new(),
            headers: HashMap::new(),
            bearer_token_env_var: None,
            env_http_headers: HashMap::new(),
            auth: None,
            enabled: true,
            required: false,
            cwd: None,
            enabled_tools: None,
            disabled_tools: vec![],
            default_tools_approval_mode: types::McpToolApprovalMode::Auto,
            tools: HashMap::new(),
            discovered: vec![],
            startup_timeout_secs: None,
            tool_timeout_secs: None,
        }
    }

    #[test]
    fn server_instructions_are_trimmed_sanitized_and_head_bounded() {
        let raw = format!("  important\0 guidance\n{}  ", "x".repeat(20_000));
        let normalized = normalize_server_instructions(&raw).unwrap();
        assert!(normalized.starts_with("important guidance\n"));
        assert!(!normalized.contains('\0'));
        assert_eq!(
            normalized.chars().count(),
            MAX_MCP_SERVER_INSTRUCTIONS_CHARS
        );
    }

    #[test]
    fn blank_server_instructions_are_ignored() {
        assert_eq!(normalize_server_instructions(" \n\t\0 "), None);
    }

    #[test]
    fn initialize_server_info_exposes_normalized_instructions() {
        let info = rmcp::model::InitializeResult::new(Default::default())
            .with_instructions("  use search before fetch\0  ");
        assert_eq!(
            server_instructions_from_info(Some(&info)).as_deref(),
            Some("use search before fetch")
        );
        assert_eq!(server_instructions_from_info(None), None);
    }

    #[test]
    fn server_instructions_have_stable_order_and_total_bound() {
        let entries = vec![
            McpServerInstructions {
                server_id: "z".into(),
                server_name: "Z".into(),
                instructions: "z".repeat(MAX_TOTAL_MCP_INSTRUCTIONS_CHARS),
            },
            McpServerInstructions {
                server_id: "a".into(),
                server_name: "A".into(),
                instructions: "a".repeat(10),
            },
        ];
        let bounded = bound_server_instructions(entries);
        assert_eq!(bounded[0].server_id, "a");
        assert_eq!(bounded[0].instructions.chars().count(), 10);
        assert_eq!(
            bounded[1].instructions.chars().count(),
            MAX_TOTAL_MCP_INSTRUCTIONS_CHARS - 10
        );
        assert_eq!(
            bounded
                .iter()
                .map(|entry| entry.instructions.chars().count())
                .sum::<usize>(),
            MAX_TOTAL_MCP_INSTRUCTIONS_CHARS
        );
    }

    #[test]
    fn filter_respects_server_and_tool_flags() {
        let mut server = McpServerConfig {
            id: "s1".into(),
            name: "demo".into(),
            description: String::new(),
            r#type: McpTransportType::Stdio,
            command: "x".into(),
            args: vec![],
            env: HashMap::new(),
            env_vars: vec![],
            url: String::new(),
            headers: HashMap::new(),
            bearer_token_env_var: None,
            env_http_headers: HashMap::new(),
            auth: None,
            enabled: true,
            required: false,
            cwd: None,
            enabled_tools: None,
            disabled_tools: vec![],
            default_tools_approval_mode: types::McpToolApprovalMode::Auto,
            tools: HashMap::from([
                ("a".into(), crate::config::McpToolConfig::Enabled(true)),
                ("b".into(), crate::config::McpToolConfig::Enabled(false)),
            ]),
            discovered: vec![],
            startup_timeout_secs: None,
            tool_timeout_secs: None,
        };
        let names = vec!["a".into(), "b".into(), "c".into()];
        let enabled = filter_enabled_tool_names(&server, &names);
        assert_eq!(
            enabled,
            vec![
                "mcp__s1__a".to_string(),
                "mcp__s1__c".to_string() // c 缺省 true
            ]
        );
        server.enabled = false;
        assert!(filter_enabled_tool_names(&server, &names).is_empty());
    }

    #[test]
    fn allow_list_then_deny_list_then_legacy_gate() {
        let mut server = stdio_server("echo");
        server.enabled_tools = Some(vec!["read".into(), "search".into(), "legacy".into()]);
        server.disabled_tools = vec!["search".into()];
        server.tools.insert(
            "legacy".into(),
            crate::config::McpToolConfig::Enabled(false),
        );
        let names = vec![
            "read".into(),
            "search".into(),
            "legacy".into(),
            "unknown".into(),
        ];

        assert_eq!(
            filter_enabled_tool_names(&server, &names),
            vec!["mcp__s1__read"]
        );
    }

    #[test]
    fn stdio_command_validation() {
        assert!(validate_stdio_command("npx").is_ok());
        assert!(validate_stdio_command("uvx").is_ok());
        assert!(validate_stdio_command("/usr/local/bin/mcp-server").is_ok());
        assert!(validate_stdio_command("").is_err());
        assert!(validate_stdio_command("../evil").is_err());
        assert!(validate_stdio_command("npx;rm").is_err());
        assert!(validate_stdio_command("foo/bar").is_err());
    }

    #[test]
    fn parent_environment_is_allowlisted() {
        let env = scrubbed_parent_env([
            ("PATH", "/usr/bin"),
            ("HOME", "/tmp/home"),
            ("OPENAI_API_KEY", "secret"),
            ("GITHUB_TOKEN", "secret"),
            ("CUSTOM", "value"),
        ]);
        assert_eq!(env.get("PATH").map(String::as_str), Some("/usr/bin"));
        assert_eq!(env.get("HOME").map(String::as_str), Some("/tmp/home"));
        assert!(!env.contains_key("OPENAI_API_KEY"));
        assert!(!env.contains_key("GITHUB_TOKEN"));
        assert!(!env.contains_key("CUSTOM"));
    }

    #[test]
    fn configured_environment_references_only_forward_available_values() {
        let mut server = stdio_server("echo");
        server.env_vars = vec!["MCP_TOKEN".into(), "MISSING_TOKEN".into()];

        let resolved = resolve_forwarded_env_with(&server, |name| {
            (name == "MCP_TOKEN").then(|| "secret-value".to_string())
        })
        .unwrap();

        assert_eq!(
            resolved.get("MCP_TOKEN").map(String::as_str),
            Some("secret-value")
        );
        assert!(!resolved.contains_key("MISSING_TOKEN"));
        server.env_vars = vec!["NOT-VALID".into()];
        assert!(resolve_forwarded_env_with(&server, |_| None).is_err());
    }

    #[test]
    fn http_environment_headers_and_bearer_token_follow_precedence() {
        let mut server = stdio_server("echo");
        server.r#type = McpTransportType::StreamableHttp;
        server.command.clear();
        server.url = "https://example.com/mcp".into();
        server
            .headers
            .insert("X-Region".into(), "static-region".into());
        server
            .env_http_headers
            .insert("X-Region".into(), "MCP_REGION".into());
        server.bearer_token_env_var = Some("MCP_TOKEN".into());

        let resolved = resolve_http_headers_with(&server, |name| match name {
            "MCP_REGION" => Some("environment-region".into()),
            "MCP_TOKEN" => Some("secret-token".into()),
            _ => None,
        })
        .unwrap();

        assert_eq!(
            resolved[&HeaderName::from_static("x-region")],
            "environment-region"
        );
        assert_eq!(resolved[&AUTHORIZATION], "Bearer secret-token");

        server
            .headers
            .insert("Authorization".into(), "Static credential".into());
        let explicit = resolve_http_headers_with(&server, |name| {
            (name == "MCP_TOKEN").then(|| "secret-token".into())
        })
        .unwrap();
        assert_eq!(explicit[&AUTHORIZATION], "Static credential");
    }

    #[test]
    fn unresolved_http_environment_credentials_are_omitted() {
        let mut server = stdio_server("echo");
        server.r#type = McpTransportType::StreamableHttp;
        server.command.clear();
        server.bearer_token_env_var = Some("MCP_TOKEN".into());
        server
            .env_http_headers
            .insert("X-API-Key".into(), "MCP_API_KEY".into());

        let resolved = resolve_http_headers_with(&server, |_| None).unwrap();
        assert!(resolved.is_empty());
    }

    #[test]
    fn credential_fingerprint_changes_without_exposing_secret_values() {
        let mut server = stdio_server("echo");
        server.env_vars = vec!["MCP_TOKEN".into()];
        let first = credential_fingerprint_with(&server, |_| Some("first-secret".into()));
        let second = credential_fingerprint_with(&server, |_| Some("second-secret".into()));
        assert_ne!(first, second);
        assert!(
            !format!("{}|{first:016x}", server.connection_fingerprint()).contains("first-secret")
        );
    }

    #[test]
    fn full_access_stdio_command_uses_working_dir_and_explicit_env() {
        let dir = tempfile::tempdir().unwrap();
        let policy = sandbox::SandboxPolicy::new(
            types::SandboxMode::DangerFullAccess,
            dir.path(),
            Vec::new(),
            false,
        )
        .unwrap();
        let context = McpExecutionContext::new(policy, dir.path()).unwrap();
        let mut server = stdio_server("echo");
        server.args = vec!["hello".into()];
        server
            .env
            .insert("MCP_EXPLICIT_TOKEN".into(), "configured".into());

        let command = build_stdio_command(&server, &context).unwrap();
        let command = command.as_std();
        assert_eq!(command.get_program(), "echo");
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["hello"]);
        assert_eq!(
            command.get_current_dir(),
            Some(context.working_dir.as_path())
        );
        let env = command
            .get_envs()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect::<HashMap<_, _>>();
        assert_eq!(
            env.get("MCP_EXPLICIT_TOKEN").and_then(Option::as_deref),
            Some("configured")
        );
    }

    #[test]
    fn rejected_stdio_command_is_audited_without_command_text() {
        let dir = tempfile::tempdir().unwrap();
        let policy = sandbox::SandboxPolicy::new(
            types::SandboxMode::DangerFullAccess,
            dir.path(),
            Vec::new(),
            false,
        )
        .unwrap();
        let audit = sandbox::SandboxAuditMetadata::new(
            dir.path(),
            None,
            None,
            "mcp",
            types::DANGER_FULL_ACCESS_PROFILE,
        );
        let context = McpExecutionContext::new(policy, dir.path())
            .unwrap()
            .with_sandbox_audit(audit);
        let server = stdio_server("echo; secret-command");

        assert!(build_stdio_command(&server, &context).is_err());
        let events = sandbox::list_recent_sandbox_audits(dir.path(), 10).unwrap();
        assert_eq!(events[0].event, sandbox::SandboxAuditKind::Denied);
        let raw = std::fs::read_to_string(sandbox::sandbox_audit_path(dir.path())).unwrap();
        assert!(!raw.contains("secret-command"));
    }

    #[tokio::test]
    async fn stdio_process_spawn_is_audited_before_handshake_failure() {
        let dir = tempfile::tempdir().unwrap();
        let policy = sandbox::SandboxPolicy::new(
            types::SandboxMode::DangerFullAccess,
            dir.path(),
            Vec::new(),
            false,
        )
        .unwrap();
        let audit = sandbox::SandboxAuditMetadata::new(
            dir.path(),
            Some("session-1".into()),
            None,
            "mcp",
            types::DANGER_FULL_ACCESS_PROFILE,
        );
        let context = McpExecutionContext::new(policy, dir.path())
            .unwrap()
            .with_sandbox_audit(audit);
        let server = stdio_server("echo");

        assert!(connect_server_inner(&server, Some(&context), None,)
            .await
            .is_err());
        let events = sandbox::list_recent_sandbox_audits(dir.path(), 10).unwrap();
        assert!(events.iter().any(|event| {
            event.event == sandbox::SandboxAuditKind::Spawned
                && event.tool_name == "mcp:s1"
                && event.target == "stdio"
        }));
    }

    #[test]
    fn stdio_cwd_must_resolve_inside_execution_root() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("packages/server");
        std::fs::create_dir_all(&nested).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let policy = sandbox::SandboxPolicy::new(
            types::SandboxMode::DangerFullAccess,
            root.path(),
            Vec::new(),
            false,
        )
        .unwrap();
        let context = McpExecutionContext::new(policy, root.path()).unwrap();
        let mut server = stdio_server("echo");
        server.cwd = Some("packages/server".into());
        assert_eq!(
            resolve_server_working_dir(&server, &context).unwrap(),
            nested.canonicalize().unwrap()
        );

        server.cwd = Some(outside.path().to_string_lossy().into_owned());
        let error = resolve_server_working_dir(&server, &context)
            .unwrap_err()
            .to_string();
        assert!(error.contains("escapes the execution root"), "{error}");

        let context = context
            .with_additional_working_roots([outside.path().to_path_buf()])
            .unwrap();
        assert_eq!(
            resolve_server_working_dir(&server, &context).unwrap(),
            outside.path().canonicalize().unwrap()
        );
    }

    #[tokio::test]
    async fn startup_collection_obeys_global_parallel_limit() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let current = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let futures = (0..12)
            .map(|_| {
                let current = Arc::clone(&current);
                let peak = Arc::clone(&peak);
                async move {
                    let active = current.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(active, Ordering::SeqCst);
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    current.fetch_sub(1, Ordering::SeqCst);
                }
            })
            .collect();

        collect_bounded(futures, MAX_PARALLEL_MCP_STARTUPS).await;
        assert_eq!(peak.load(Ordering::SeqCst), MAX_PARALLEL_MCP_STARTUPS);
    }

    #[tokio::test]
    async fn required_failure_blocks_while_optional_failure_only_degrades() {
        let mut optional_hub = McpHub::new();
        optional_hub
            .reload_with_configs(vec![stdio_server("echo")])
            .await
            .expect("optional startup failure must not block");
        let optional = optional_hub.server_status();
        assert_eq!(optional[0].status, "error");
        assert!(!optional[0].required);

        let mut required = stdio_server("echo");
        required.required = true;
        let mut required_hub = McpHub::new();
        let error = required_hub
            .reload_with_configs(vec![required])
            .await
            .unwrap_err();
        let typed = error
            .downcast_ref::<RequiredMcpServersError>()
            .expect("required failure must retain structured diagnostics");
        assert_eq!(typed.failures.len(), 1);
        assert_eq!(typed.failures[0].server_id, "s1");
        let status = required_hub.server_status();
        assert_eq!(status[0].status, "error");
        assert!(status[0].required);
    }

    #[test]
    fn retry_classification_and_delay_follow_policy() {
        assert!(!is_retryable_connect_error(&anyhow!(
            "network access denied by active profile"
        )));
        assert!(!is_retryable_connect_error(&anyhow!(
            "HTTP status: 401 Unauthorized"
        )));
        assert!(is_retryable_connect_error(&anyhow!(
            "connection reset by peer"
        )));

        let first = retry_delay("server-a", 1);
        assert!(first >= std::time::Duration::from_millis(800));
        assert!(first <= std::time::Duration::from_millis(1_200));
        assert!(retry_delay("server-a", 20) <= std::time::Duration::from_secs(30));
    }

    #[test]
    fn oauth_eligible_401_requires_auth_but_static_auth_does_not() {
        let mut server = stdio_server("");
        server.r#type = McpTransportType::StreamableHttp;
        server.url = "https://example.invalid/mcp".into();
        let unauthorized = anyhow!("HTTP status client error (401 Unauthorized)");
        assert!(is_auth_required_error(&server, &unauthorized));

        server.bearer_token_env_var = Some("MCP_TOKEN".into());
        assert!(!is_auth_required_error(&server, &unauthorized));

        server.bearer_token_env_var = None;
        server.auth = Some(McpHttpAuth::Chatgpt);
        assert!(!is_auth_required_error(&server, &unauthorized));
    }

    #[tokio::test]
    async fn retryable_failure_backs_off_until_manual_reconnect() {
        let dir = tempfile::tempdir().unwrap();
        let policy = sandbox::SandboxPolicy::new(
            types::SandboxMode::DangerFullAccess,
            dir.path(),
            Vec::new(),
            false,
        )
        .unwrap();
        let context = McpExecutionContext::new(policy, dir.path()).unwrap();
        let config = stdio_server("echo");
        let mut hub = McpHub::new();
        hub.set_execution_context(Some(context));

        hub.reload_with_configs(vec![config.clone()]).await.unwrap();
        let first = hub.server_status().remove(0);
        assert_eq!(first.status, "backoff");
        assert!(first.retryable);
        assert_eq!(first.retry_attempt, 1);
        assert!(first.next_retry_at_unix_ms.is_some());

        hub.reload_with_configs(vec![config.clone()]).await.unwrap();
        let skipped = hub.server_status().remove(0);
        assert_eq!(
            skipped.retry_attempt, 1,
            "reload during backoff must not retry"
        );

        hub.force_reconnect("s1").unwrap();
        let reset = hub.server_status().remove(0);
        assert_eq!(reset.status, "disconnected");
        assert_eq!(reset.retry_attempt, 0);

        hub.reload_with_configs(vec![config]).await.unwrap();
        assert_eq!(hub.server_status()[0].retry_attempt, 1);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn restricted_stdio_command_is_wrapped_by_seatbelt_without_network() {
        let dir = tempfile::tempdir().unwrap();
        let policy = sandbox::SandboxPolicy::new(
            types::SandboxMode::ReadOnly,
            dir.path(),
            Vec::new(),
            false,
        )
        .unwrap();
        let context = McpExecutionContext::new(policy, dir.path()).unwrap();

        let command = build_stdio_command(&stdio_server("echo"), &context).unwrap();
        let command = command.as_std();
        assert_eq!(command.get_program(), "/usr/bin/sandbox-exec");
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(args.first().map(String::as_str), Some("-p"));
        assert_eq!(args.last().map(String::as_str), Some("echo"));
        assert!(!args[1].contains("(allow network*)"));
    }

    #[tokio::test]
    async fn missing_execution_context_fails_closed() {
        let error = connect_server(&stdio_server("echo"), None, None)
            .await
            .err()
            .expect("missing policy must fail")
            .to_string();
        assert!(error.contains("execution policy is unavailable"), "{error}");
    }

    #[tokio::test]
    async fn restricted_profile_denies_remote_mcp_before_connecting() {
        let dir = tempfile::tempdir().unwrap();
        let policy = sandbox::SandboxPolicy::new(
            types::SandboxMode::ReadOnly,
            dir.path(),
            Vec::new(),
            false,
        )
        .unwrap();
        let context = McpExecutionContext::new(policy, dir.path()).unwrap();
        let mut server = stdio_server("");
        server.r#type = McpTransportType::StreamableHttp;
        server.url = "https://example.invalid/mcp".into();

        let error = connect_server(&server, Some(&context), None)
            .await
            .err()
            .expect("restricted network must fail")
            .to_string();
        assert!(error.contains("network access denied"), "{error}");
    }

    #[tokio::test]
    async fn startup_timeout_bounds_the_entire_connection_future() {
        let error = with_startup_timeout("slow", 1, std::future::pending::<anyhow::Result<()>>())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("启动超时 (1s): slow"), "{error}");
    }

    #[test]
    fn call_tool_args_must_be_object_or_null() {
        assert!(mcp_tool_arguments(&serde_json::json!({"a": 1})).is_ok());
        assert!(mcp_tool_arguments(&serde_json::Value::Null)
            .unwrap()
            .is_none());
        assert!(mcp_tool_arguments(&serde_json::json!("str")).is_err());
        assert!(mcp_tool_arguments(&serde_json::json!([1, 2])).is_err());
    }
}
