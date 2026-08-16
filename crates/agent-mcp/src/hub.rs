//! MCP 连接池与工具调用。
//!
//! 按 Agent 加载配置、维持 RunningService，并向 ToolRegistry 暴露限定名工具。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{anyhow, Context};
use http::{HeaderName, HeaderValue};
use rmcp::model::{CallToolRequestParams, ContentBlock, Tool as RmcpTool};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{Peer, RoleClient, ServiceExt};
use serde_json::{json, Value};
use tracing::{info, warn};

use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

use crate::config::{
    load_for_active_agent, load_mcp_servers, merge_discovered, persist_discovered,
    save_mcp_servers, DiscoveredTool, McpServerConfig, McpTransportType,
};
use crate::names::{
    is_mcp_tool_name, parse_qualified_name, qualify_tool_name, sanitize_server_id, MCP_TOOLSET,
};

/// MCP 连接建立时使用的权限快照。
///
/// 常驻连接只接受 session/profile 生成的稳定策略，不承接单次工具审批授权。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpExecutionContext {
    sandbox_policy: sandbox::SandboxPolicy,
    working_dir: PathBuf,
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
            working_dir,
        })
    }

    fn fingerprint(&self) -> String {
        format!(
            "{}|{}",
            self.sandbox_policy.profile_hash_material(),
            self.working_dir.to_string_lossy()
        )
    }
}

/// 供 ToolRegistry 注册的精简条目。
#[derive(Debug, Clone)]
pub struct ToolEntrySpec {
    /// `mcp__{server}__{tool}` 限定名。
    pub qualified_name: String,
    /// 服务器 id。
    pub server_id: String,
    /// 原生工具名。
    pub native_name: String,
    /// 描述。
    pub description: String,
    /// 参数 schema。
    pub schema: Value,
}

/// 单个 MCP 服务器的运行状态快照（供 UI）。
#[derive(Debug, Clone)]
pub struct ServerStatus {
    /// 服务器 id。
    pub id: String,
    /// 显示名。
    pub name: String,
    /// 状态字符串（如 connected / error）。
    pub status: String,
    /// 已发现工具名列表。
    pub tools: Vec<String>,
    /// 最近错误。
    pub error: Option<String>,
}

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
}

impl rmcp::handler::client::ClientHandler for ToolChangeHandler {
    fn get_info(&self) -> rmcp::model::ClientInfo {
        rmcp::model::ClientInfo::default()
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
    /// 当前 session/profile 对 MCP 连接施加的不可变权限快照。
    execution_context: Option<McpExecutionContext>,
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
        Self {
            agent_id: None,
            servers: HashMap::new(),
            configs: Vec::new(),
            last_connect_errors: HashMap::new(),
            execution_context: None,
        }
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
        self.execution_context = context;
    }

    /// 从磁盘重载：保留未变连接，重连变更项，断开已删除/禁用项
    pub async fn reload_from_disk(&mut self, agent_id: Option<&str>) -> anyhow::Result<()> {
        let id = agent_id
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        self.agent_id = id.clone();
        let configs = if let Some(ref a) = id {
            load_mcp_servers(Some(a))?
        } else {
            load_for_active_agent()?
        };
        self.reload_with_configs(configs).await
    }

    /// 按给定配置表重连：保留指纹未变的连接，其余重连或断开。
    pub async fn reload_with_configs(
        &mut self,
        configs: Vec<McpServerConfig>,
    ) -> anyhow::Result<()> {
        self.configs = configs;
        let enabled: Vec<_> = self.configs.iter().filter(|c| c.enabled).cloned().collect();

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
            self.servers.remove(&id);
        }

        let mut discovered_updates: Vec<(String, Vec<DiscoveredTool>)> = Vec::new();
        let mut connect_errors: HashMap<String, String> = HashMap::new();

        for cfg in enabled {
            let sid = sanitize_server_id(&cfg.id);
            let fp = effective_connection_fingerprint(&cfg, self.execution_context.as_ref());
            if let Some(existing) = self.servers.get(&sid) {
                if existing.fingerprint == fp && existing.status == "connected" {
                    if let Some(rs) = self.servers.get_mut(&sid) {
                        rs.config = cfg;
                        rs.error = None;
                    }
                    continue;
                }
            }
            self.servers.remove(&sid);
            match connect_server(&cfg, self.execution_context.as_ref()).await {
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
                    self.servers.insert(sid, running);
                }
                Err(e) => {
                    warn!(server = %sid, error = %e, "MCP server connect failed");
                    connect_errors.insert(sid, e.to_string());
                }
            }
        }

        // 只补丁写回 discovered，与磁盘最新 tools 合并，避免覆盖 UI 开关
        if !discovered_updates.is_empty() {
            if let Err(e) = persist_discovered(self.agent_id.as_deref(), &discovered_updates) {
                warn!(error = %e, "persist MCP discovered failed");
            } else if let Ok(fresh) = load_mcp_servers(self.agent_id.as_deref()) {
                for cfg in fresh {
                    let sid = sanitize_server_id(&cfg.id);
                    if let Some(slot) = self
                        .configs
                        .iter_mut()
                        .find(|c| sanitize_server_id(&c.id) == sid)
                    {
                        slot.tools = cfg.tools.clone();
                        slot.discovered = cfg.discovered.clone();
                        slot.enabled = cfg.enabled;
                    }
                    if let Some(rs) = self.servers.get_mut(&sid) {
                        rs.config.tools = cfg.tools;
                        rs.config.discovered = cfg.discovered;
                        rs.config.enabled = cfg.enabled;
                    }
                }
            }
        }

        // 把连接错误挂到 configs 状态（无 RunningServer 时 server_status 可读）
        self.last_connect_errors = connect_errors;
        Ok(())
    }

    /// 仅从磁盘同步 enabled / tools 开关，不重连（供工具调用路径）
    pub fn sync_enablement_from_disk(&mut self) -> anyhow::Result<()> {
        let fresh = load_mcp_servers(self.agent_id.as_deref())?;
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
            } else {
                self.configs.push(cfg.clone());
            }
            if let Some(rs) = self.servers.get_mut(&sid) {
                rs.config.enabled = cfg.enabled;
                rs.config.tools = cfg.tools.clone();
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
                    server_id: sid.clone(),
                    native_name: native.to_string(),
                    description: tool
                        .description
                        .as_ref()
                        .map(|d| d.to_string())
                        .unwrap_or_else(|| format!("MCP tool {native} via {}", rs.config.name)),
                    schema,
                });
            }
        }
        out
    }

    /// 查询某服务器连接状态摘要。
    pub fn server_status(&self) -> Vec<ServerStatus> {
        let mut out = Vec::new();
        for cfg in &self.configs {
            let sid = sanitize_server_id(&cfg.id);
            if let Some(rs) = self.servers.get(&sid) {
                out.push(ServerStatus {
                    id: sid,
                    name: cfg.name.clone(),
                    status: rs.status.clone(),
                    tools: rs.tools.iter().map(|t| t.name.to_string()).collect(),
                    error: rs.error.clone(),
                });
            } else {
                let err = self.last_connect_errors.get(&sid).cloned();
                out.push(ServerStatus {
                    id: sid,
                    name: cfg.name.clone(),
                    status: if !cfg.enabled {
                        "disabled".into()
                    } else if err.is_some() {
                        "error".into()
                    } else {
                        "disconnected".into()
                    },
                    tools: cfg.discovered.iter().map(|d| d.name.clone()).collect(),
                    error: err,
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
    /// 成功时返回 `(peer, native_tool_name, timeout_secs)`；调用方在释放
    /// `MutexGuard` 后再执行 `call_tool_with_peer`。
    pub fn resolve_tool_peer(
        &self,
        qualified_name: &str,
    ) -> anyhow::Result<(Peer<RoleClient>, String, u64)> {
        if !is_mcp_tool_name(qualified_name) {
            anyhow::bail!("不是 MCP 工具: {qualified_name}");
        }
        let (server_id, native) = parse_qualified_name(qualified_name)
            .ok_or_else(|| anyhow!("无效 MCP 工具名: {qualified_name}"))?;

        let rs = self
            .servers
            .get(server_id)
            .ok_or_else(|| anyhow!("MCP server 未连接: {server_id}"))?;

        if !rs.config.enabled {
            anyhow::bail!("MCP server 已禁用: {server_id}");
        }
        if !rs.config.is_tool_enabled(native) {
            anyhow::bail!("MCP 工具已禁用: {qualified_name}");
        }

        let timeout_secs = rs.config.tool_timeout_secs.unwrap_or(300);
        Ok((rs.peer.clone(), native.to_string(), timeout_secs))
    }

    /// 调用已连接 MCP 工具（按服务器与工具名）。
    pub async fn call_tool(
        &self,
        qualified_name: &str,
        args: &Value,
    ) -> anyhow::Result<types::ToolOutput> {
        let (peer, native, timeout_secs) = self.resolve_tool_peer(qualified_name)?;
        call_tool_with_peer(&peer, qualified_name, &native, args, timeout_secs).await
    }

    /// 短连刷新某 server 的 discovered（供 UI refresh_mcp_tools）
    pub async fn refresh_discovered(
        agent_id: Option<&str>,
        server_id: Option<&str>,
        execution_context: &McpExecutionContext,
    ) -> anyhow::Result<Vec<McpServerConfig>> {
        let mut configs = load_mcp_servers(agent_id)?;
        for cfg in configs.iter_mut() {
            if let Some(want) = server_id {
                if sanitize_server_id(&cfg.id) != sanitize_server_id(want) {
                    continue;
                }
            }
            if !cfg.enabled && server_id.is_none() {
                continue;
            }
            match connect_server(cfg, Some(execution_context)).await {
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
                        })
                        .collect();
                    merge_discovered(cfg, discovered);
                    // drop running → 关闭短连
                }
                Err(e) => {
                    warn!(server = %cfg.id, error = %e, "refresh discovered failed");
                }
            }
        }
        save_mcp_servers(agent_id, &configs)?;
        Ok(configs)
    }
}

/// 单次工具调用返回的最大媒体资产数（防止 MCP server 返回过多大 blob）。
const MAX_MEDIA_ASSETS: usize = 20;

/// 将 MCP content blocks 转为结构化 ToolOutput（保留 image/media 信息）。
fn content_to_tool_output(blocks: &[ContentBlock]) -> types::ToolOutput {
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
    let text = types::truncate_tool_result(&text, types::MAX_TOOL_RESULT_BYTES);

    if media_assets.is_empty() {
        types::ToolOutput::Text(text)
    } else {
        types::ToolOutput::Media {
            text,
            assets: media_assets,
        }
    }
}

/// 向下兼容的纯文本格式化（保持旧接口）。
fn format_content(blocks: &[ContentBlock]) -> String {
    content_to_tool_output(blocks).into_text()
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
        let msg = format_content(&result.content);
        anyhow::bail!(
            "MCP tool error: {}",
            types::truncate_tool_result(&msg, types::MAX_TOOL_RESULT_BYTES)
        );
    }

    if let Some(structured) = result.structured_content {
        return Ok(types::ToolOutput::Text(types::truncate_tool_result(
            &structured.to_string(),
            types::MAX_TOOL_RESULT_BYTES,
        )));
    }
    Ok(content_to_tool_output(&result.content))
}

/// 按配置建立 MCP 连接并拉取工具列表。
///
/// 使用 [`ToolChangeHandler`] 作为客户端 handler，自动处理
/// `tools/list_changed` 通知，实时同步工具列表。
async fn connect_server(
    cfg: &McpServerConfig,
    execution_context: Option<&McpExecutionContext>,
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
    };

    let service = match cfg.r#type {
        McpTransportType::Stdio => {
            let cmd = build_stdio_command(cfg, execution_context)?;
            let transport = TokioChildProcess::new(cmd)?;
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
            let mut map = HashMap::new();
            for (k, v) in &cfg.headers {
                if let (Ok(name), Ok(val)) = (
                    HeaderName::try_from(k.as_str()),
                    HeaderValue::try_from(v.as_str()),
                ) {
                    map.insert(name, val);
                }
            }
            let mut config = StreamableHttpClientTransportConfig::with_uri(cfg.url.as_str());
            if !map.is_empty() {
                config = config.custom_headers(map);
            }
            let transport = StreamableHttpClientTransport::from_config(config);
            handler.serve(transport).await.context("http serve")?
        }
    };

    let peer = service.peer().clone();
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
    format!("{}|{policy}", cfg.connection_fingerprint())
}

fn build_stdio_command(
    cfg: &McpServerConfig,
    execution_context: &McpExecutionContext,
) -> anyhow::Result<tokio::process::Command> {
    validate_stdio_command(&cfg.command)?;
    let mut cmd = sandbox::SandboxRunner
        .tokio_command(&execution_context.sandbox_policy, &cfg.command)
        .context("prepare sandboxed MCP stdio command")?;
    cmd.args(&cfg.args)
        .current_dir(&execution_context.working_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .env_clear()
        .envs(scrubbed_parent_env(std::env::vars()))
        .envs(&cfg.env);
    Ok(cmd)
}

/// stdio command 最小约束：不经 shell；禁止 `..` 与危险字符；允许绝对路径或简单命令名（如 npx）
///
/// `mcp.json` 仍视为受信任的本地配置；此处只挡明显的路径穿越 / 注入形态。
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

    fn stdio_server(command: &str) -> McpServerConfig {
        McpServerConfig {
            id: "s1".into(),
            name: "demo".into(),
            description: String::new(),
            r#type: McpTransportType::Stdio,
            command: command.into(),
            args: vec![],
            env: HashMap::new(),
            url: String::new(),
            headers: HashMap::new(),
            enabled: true,
            tools: HashMap::new(),
            discovered: vec![],
            tool_timeout_secs: None,
        }
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
            url: String::new(),
            headers: HashMap::new(),
            enabled: true,
            tools: HashMap::from([("a".into(), true), ("b".into(), false)]),
            discovered: vec![],
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
        let error = connect_server(&stdio_server("echo"), None)
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

        let error = connect_server(&server, Some(&context))
            .await
            .err()
            .expect("restricted network must fail")
            .to_string();
        assert!(error.contains("network access denied"), "{error}");
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
