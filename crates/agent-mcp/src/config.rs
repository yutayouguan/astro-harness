//! 读写全局、可信项目与 Agent 私有的 `config.toml` MCP 配置。
//!
//! 旧 `mcp.json` 仅用于一次性只读迁移；运行时不再写回 JSON。

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use home::{
    active_agent_id, agent_config_dir, default_memory_dir, ensure_default_workspace_dirs,
    DEFAULT_AGENT_ID,
};

use crate::names::sanitize_server_id;

/// MCP server 启动默认超时（秒），与 Codex 默认值一致。
pub const DEFAULT_STARTUP_TIMEOUT_SECS: u64 = 10;
/// MCP 工具调用默认超时（秒），与 Codex 默认值一致。
pub const DEFAULT_TOOL_TIMEOUT_SECS: u64 = 60;
/// MCP server 启动超时允许范围。
pub const STARTUP_TIMEOUT_SECS_RANGE: std::ops::RangeInclusive<u64> = 1..=120;
/// MCP 工具调用超时允许范围。
pub const TOOL_TIMEOUT_SECS_RANGE: std::ops::RangeInclusive<u64> = 1..=3600;

/// MCP 传输方式。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum McpTransportType {
    /// 本地子进程 stdio。
    Stdio,
    /// Streamable HTTP。
    #[serde(rename = "streamableHttp", alias = "streamable_http")]
    StreamableHttp,
}

impl Default for McpTransportType {
    /// 默认使用 stdio 本地进程。
    fn default() -> Self {
        Self::Stdio
    }
}

impl McpTransportType {
    /// 序列化为配置字符串。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::StreamableHttp => "streamableHttp",
        }
    }

    /// 解析配置字符串；旧 SSE 与未知传输必须显式报错，禁止静默回退。
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "stdio" => Ok(Self::Stdio),
            "streamableHttp" | "streamable_http" => Ok(Self::StreamableHttp),
            "sse" => Err(legacy_sse_error()),
            other => Err(format!(
                "unsupported MCP transport {other:?}; expected stdio or streamableHttp"
            )),
        }
    }
}

fn legacy_sse_error() -> String {
    "legacy SSE transport is not supported; configure the server's Streamable HTTP /mcp endpoint instead (an /sse URL cannot be converted automatically)".into()
}

/// 连接时发现的工具摘要（写入配置缓存）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredTool {
    /// 原生工具名。
    pub name: String,
    /// 描述。
    #[serde(default)]
    pub description: String,
}

/// 单个 MCP 服务器配置条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    /// 服务器 id（限定工具名前缀）。
    pub id: String,
    /// 显示名。
    pub name: String,
    /// 说明。
    #[serde(default)]
    pub description: String,
    /// 传输类型。
    /// stdio | streamableHttp
    #[serde(default, alias = "transport", deserialize_with = "de_type")]
    pub r#type: McpTransportType,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// 从 Astro 本地进程环境按名称转发给 STDIO Server，不在配置中持久化值。
    #[serde(default, alias = "env_vars", alias = "envVars")]
    pub env_vars: Vec<String>,
    #[serde(default)]
    pub url: String,
    #[serde(default, alias = "http_headers", alias = "httpHeaders")]
    pub headers: HashMap<String, String>,
    /// HTTP Bearer Token 所在的环境变量名。
    #[serde(
        default,
        alias = "bearer_token_env_var",
        alias = "bearerTokenEnvVar",
        skip_serializing_if = "Option::is_none"
    )]
    pub bearer_token_env_var: Option<String>,
    /// HTTP Header 名到本地环境变量名的映射。
    #[serde(default, alias = "env_http_headers", alias = "envHttpHeaders")]
    pub env_http_headers: HashMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 连接失败时是否阻止 Agent 进入首次 LLM 调用。
    #[serde(default)]
    pub required: bool,
    /// STDIO Server 工作目录；必须位于当前执行根内。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// 启动超时（秒）；覆盖建连、初始化与首次工具发现，缺失默认 10s。
    #[serde(
        default,
        alias = "startup_timeout_sec",
        alias = "startup_timeout_secs",
        alias = "startupTimeoutSec",
        skip_serializing_if = "Option::is_none"
    )]
    pub startup_timeout_secs: Option<u64>,
    /// 工具调用超时（秒）；缺失默认 60s。
    #[serde(
        default,
        alias = "tool_timeout_sec",
        alias = "tool_timeout_secs",
        alias = "toolTimeoutSec",
        skip_serializing_if = "Option::is_none"
    )]
    pub tool_timeout_secs: Option<u64>,
    /// 显式工具 allow list；`Some([])` 表示不暴露任何工具。
    #[serde(
        default,
        alias = "enabled_tools",
        alias = "enabledTools",
        skip_serializing_if = "Option::is_none"
    )]
    pub enabled_tools: Option<Vec<String>>,
    /// 工具 deny list，在 allow list 之后应用。
    #[serde(default, alias = "disabled_tools", alias = "disabledTools")]
    pub disabled_tools: Vec<String>,
    /// 单工具开关；缺失视为 true
    #[serde(default)]
    pub tools: HashMap<String, bool>,
    /// 最近一次 list_tools 缓存（供 UI）
    #[serde(default)]
    pub discovered: Vec<DiscoveredTool>,
}

/// serde 默认：字段缺省为 `true`。
fn default_true() -> bool {
    true
}

/// 反序列化传输类型（兼容 `streamable_http` 别名）。
fn de_type<'de, D>(deserializer: D) -> Result<McpTransportType, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    McpTransportType::parse(&s).map_err(serde::de::Error::custom)
}

impl McpServerConfig {
    /// 工具是否启用（未登记视为启用）。
    pub fn is_tool_enabled(&self, tool_name: &str) -> bool {
        if self
            .enabled_tools
            .as_ref()
            .is_some_and(|allowed| !allowed.iter().any(|name| name == tool_name))
        {
            return false;
        }
        if self.disabled_tools.iter().any(|name| name == tool_name) {
            return false;
        }
        self.tools.get(tool_name).copied().unwrap_or(true)
    }

    /// 应用默认值与安全边界后的启动超时。
    pub fn effective_startup_timeout_secs(&self) -> u64 {
        self.startup_timeout_secs
            .unwrap_or(DEFAULT_STARTUP_TIMEOUT_SECS)
            .clamp(
                *STARTUP_TIMEOUT_SECS_RANGE.start(),
                *STARTUP_TIMEOUT_SECS_RANGE.end(),
            )
    }

    /// 应用默认值与安全边界后的工具调用超时。
    pub fn effective_tool_timeout_secs(&self) -> u64 {
        self.tool_timeout_secs
            .unwrap_or(DEFAULT_TOOL_TIMEOUT_SECS)
            .clamp(
                *TOOL_TIMEOUT_SECS_RANGE.start(),
                *TOOL_TIMEOUT_SECS_RANGE.end(),
            )
    }

    /// 连接身份指纹（仅返回哈希，避免将静态凭证写入运行状态或日志）。
    ///
    /// 不含 tools/discovered/tool timeout，避免非连接项变化触发重连。
    pub fn connection_fingerprint(&self) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        sanitize_server_id(&self.id).hash(&mut hasher);
        self.r#type.as_str().hash(&mut hasher);
        self.command.hash(&mut hasher);
        self.args.hash(&mut hasher);
        hash_string_map(&self.env, &mut hasher);
        self.env_vars.hash(&mut hasher);
        self.cwd.hash(&mut hasher);
        self.url.hash(&mut hasher);
        hash_string_map(&self.headers, &mut hasher);
        self.bearer_token_env_var.hash(&mut hasher);
        hash_string_map(&self.env_http_headers, &mut hasher);
        self.effective_startup_timeout_secs().hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}

fn hash_string_map(map: &HashMap<String, String>, hasher: &mut impl Hasher) {
    for (key, value) in map.iter().collect::<BTreeMap<_, _>>() {
        key.hash(hasher);
        value.hash(hasher);
    }
}

/// 旧版磁盘 `mcp.json` 结构，仅供只读迁移。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct LegacyMcpFile {
    #[serde(default)]
    servers: Vec<McpServerConfig>,
}

/// TOML 中的单个 MCP Server；传输由 command/url 互斥推断。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct TomlMcpServer {
    #[serde(
        default,
        rename = "type",
        alias = "transport",
        skip_serializing_if = "Option::is_none"
    )]
    legacy_transport: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    env: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    env_vars: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(
        default,
        rename = "http_headers",
        alias = "headers",
        skip_serializing_if = "HashMap::is_empty"
    )]
    headers: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bearer_token_env_var: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    env_http_headers: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    required: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    startup_timeout_sec: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tool_timeout_sec: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    enabled_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    disabled_tools: Vec<String>,
    /// Astro 当前的逐工具开关，后续迁移到 enabled_tools/disabled_tools。
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    tools: HashMap<String, bool>,
    /// UI 使用的发现缓存；后续可迁移到独立 runtime state。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    discovered: Vec<DiscoveredTool>,
}

impl TomlMcpServer {
    fn into_config(self, id: String) -> anyhow::Result<McpServerConfig> {
        if let Some(transport) = self.legacy_transport.as_deref() {
            if transport.eq_ignore_ascii_case("sse") {
                anyhow::bail!(legacy_sse_error());
            }
            anyhow::bail!(
                "MCP server {id:?} must omit type/transport; transport is inferred from command or url"
            );
        }
        let command = self.command.unwrap_or_default().trim().to_string();
        let url = self.url.unwrap_or_default().trim().to_string();
        let r#type = match (command.is_empty(), url.is_empty()) {
            (false, true) => McpTransportType::Stdio,
            (true, false) => McpTransportType::StreamableHttp,
            (false, false) => anyhow::bail!("MCP server {id:?} cannot define both command and url"),
            (true, true) => {
                anyhow::bail!("MCP server {id:?} must define exactly one of command or url")
            }
        };
        match r#type {
            McpTransportType::Stdio
                if !self.headers.is_empty()
                    || self.bearer_token_env_var.is_some()
                    || !self.env_http_headers.is_empty() =>
            {
                anyhow::bail!(
                    "STDIO MCP server {id:?} cannot define HTTP authentication or headers"
                )
            }
            McpTransportType::StreamableHttp if self.cwd.is_some() => {
                anyhow::bail!("HTTP MCP server {id:?} cannot define cwd")
            }
            McpTransportType::StreamableHttp
                if !self.args.is_empty() || !self.env.is_empty() || !self.env_vars.is_empty() =>
            {
                anyhow::bail!("HTTP MCP server {id:?} cannot define args, env, or env_vars")
            }
            _ => {}
        }
        let id = sanitize_server_id(&id);
        Ok(McpServerConfig {
            name: self
                .name
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| id.clone()),
            id,
            description: self.description.unwrap_or_default(),
            r#type,
            command,
            args: self.args,
            env: self.env,
            env_vars: self.env_vars,
            url,
            headers: self.headers,
            bearer_token_env_var: self
                .bearer_token_env_var
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            env_http_headers: self.env_http_headers,
            enabled: self.enabled.unwrap_or(true),
            required: self.required.unwrap_or(false),
            cwd: self.cwd.filter(|value| !value.trim().is_empty()),
            startup_timeout_secs: self.startup_timeout_sec,
            tool_timeout_secs: self.tool_timeout_sec,
            enabled_tools: self.enabled_tools,
            disabled_tools: self.disabled_tools,
            tools: self.tools,
            discovered: self.discovered,
        })
    }

    fn from_config(config: &McpServerConfig) -> Self {
        Self {
            legacy_transport: None,
            name: (config.name != config.id).then(|| config.name.clone()),
            description: (!config.description.is_empty()).then(|| config.description.clone()),
            command: (config.r#type == McpTransportType::Stdio).then(|| config.command.clone()),
            args: (config.r#type == McpTransportType::Stdio)
                .then(|| config.args.clone())
                .unwrap_or_default(),
            env: (config.r#type == McpTransportType::Stdio)
                .then(|| config.env.clone())
                .unwrap_or_default(),
            env_vars: (config.r#type == McpTransportType::Stdio)
                .then(|| config.env_vars.clone())
                .unwrap_or_default(),
            cwd: (config.r#type == McpTransportType::Stdio)
                .then(|| config.cwd.clone())
                .flatten(),
            url: (config.r#type == McpTransportType::StreamableHttp).then(|| config.url.clone()),
            headers: (config.r#type == McpTransportType::StreamableHttp)
                .then(|| config.headers.clone())
                .unwrap_or_default(),
            bearer_token_env_var: (config.r#type == McpTransportType::StreamableHttp)
                .then(|| config.bearer_token_env_var.clone())
                .flatten(),
            env_http_headers: (config.r#type == McpTransportType::StreamableHttp)
                .then(|| config.env_http_headers.clone())
                .unwrap_or_default(),
            enabled: (!config.enabled).then_some(false),
            required: config.required.then_some(true),
            startup_timeout_sec: config.startup_timeout_secs,
            tool_timeout_sec: config.tool_timeout_secs,
            enabled_tools: config.enabled_tools.clone(),
            disabled_tools: config.disabled_tools.clone(),
            tools: config.tools.clone(),
            discovered: config.discovered.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ProjectTrust {
    #[serde(default)]
    trust_level: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct McpTomlRoot {
    #[serde(default)]
    mcp_servers: BTreeMap<String, TomlMcpServer>,
    #[serde(default)]
    projects: BTreeMap<String, ProjectTrust>,
}

/// 全局 `~/.astro/config.toml` 路径。
pub fn mcp_config_path_global() -> PathBuf {
    default_memory_dir().join("config.toml")
}

/// Agent 私有 `~/.astro/agents/<id>/config.toml` 路径。
pub fn mcp_config_path_for_agent(agent_id: &str) -> PathBuf {
    let base = default_memory_dir();
    agent_config_dir(&base, normalize_agent_id(agent_id)).join("config.toml")
}

/// 可信项目 `<project>/.astro/config.toml` 路径。
pub fn mcp_config_path_for_project(project_root: &Path) -> PathBuf {
    project_root.join(".astro").join("config.toml")
}

fn normalize_agent_id(agent_id: &str) -> &str {
    let agent_id = agent_id.trim();
    if agent_id == "default" {
        DEFAULT_AGENT_ID
    } else {
        agent_id
    }
}

fn legacy_mcp_path_global() -> PathBuf {
    default_memory_dir().join("mcp.json")
}

fn legacy_mcp_path_for_agent(agent_id: &str) -> PathBuf {
    let base = default_memory_dir();
    agent_config_dir(&base, normalize_agent_id(agent_id)).join("mcp.json")
}

fn read_legacy_file(path: &Path) -> anyhow::Result<LegacyMcpFile> {
    if !path.exists() {
        return Ok(LegacyMcpFile::default());
    }
    let raw = fs::read_to_string(path)?;
    if raw.trim().is_empty() {
        return Ok(LegacyMcpFile::default());
    }
    Ok(serde_json::from_str(&raw)?)
}

fn read_toml_value(path: &Path) -> anyhow::Result<toml::Value> {
    if !path.exists() {
        return Ok(toml::Value::Table(Default::default()));
    }
    let raw = fs::read_to_string(path)?;
    if raw.trim().is_empty() {
        return Ok(toml::Value::Table(Default::default()));
    }
    raw.parse::<toml::Value>()
        .with_context(|| format!("parse MCP config {}", path.display()))
}

fn write_toml_value(path: &Path, value: &toml::Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("toml.tmp");
    let rendered = toml::to_string_pretty(value)?;
    fs::write(&tmp, format!("{rendered}\n"))?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn backup_path_for_legacy(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("mcp.json");
    path.with_file_name(format!("{name}.migrated.bak"))
}

/// 只读导入旧 JSON；TOML 已有 mcp_servers 时不覆盖。
fn migrate_legacy_json(legacy_path: &Path, toml_path: &Path) -> anyhow::Result<bool> {
    if !legacy_path.is_file() {
        return Ok(false);
    }
    let mut root = read_toml_value(toml_path)?;
    let table = root
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("MCP config root must be a TOML table"))?;
    if table.contains_key("mcp_servers") {
        return Ok(false);
    }
    let legacy = read_legacy_file(legacy_path)?;
    let servers: BTreeMap<String, TomlMcpServer> = legacy
        .servers
        .into_iter()
        .map(|mut config| {
            config.id = sanitize_server_id(&config.id);
            (config.id.clone(), TomlMcpServer::from_config(&config))
        })
        .collect();
    let backup = backup_path_for_legacy(legacy_path);
    if !backup.exists() {
        fs::copy(legacy_path, &backup)?;
    }
    table.insert("mcp_servers".into(), toml::Value::try_from(servers)?);
    write_toml_value(toml_path, &root)?;
    Ok(true)
}

fn read_toml_root(path: &Path) -> anyhow::Result<McpTomlRoot> {
    read_toml_value(path)?
        .try_into()
        .with_context(|| format!("decode MCP config {}", path.display()))
}

fn merge_servers(
    merged: &mut BTreeMap<String, McpServerConfig>,
    servers: BTreeMap<String, TomlMcpServer>,
) -> anyhow::Result<()> {
    for (id, server) in servers {
        let config = server.into_config(id)?;
        merged.insert(config.id.clone(), config);
    }
    Ok(())
}

fn project_is_trusted(global: &McpTomlRoot, project_root: &Path) -> bool {
    let Ok(project_root) = project_root.canonicalize() else {
        return false;
    };
    global.projects.iter().any(|(configured_path, trust)| {
        if !trust.trust_level.eq_ignore_ascii_case("trusted") {
            return false;
        }
        let configured_path = Path::new(configured_path);
        configured_path.is_absolute()
            && configured_path
                .canonicalize()
                .is_ok_and(|configured| configured == project_root)
    })
}

/// 按 `全局 → 可信项目 → Agent` 读取并以 Server 为单位整体覆盖。
pub fn load_mcp_servers_layered(
    agent_id: Option<&str>,
    project_root: Option<&Path>,
) -> anyhow::Result<Vec<McpServerConfig>> {
    ensure_default_workspace_dirs()?;
    let global_path = mcp_config_path_global();
    migrate_legacy_json(&legacy_mcp_path_global(), &global_path)?;
    if let Some(agent_id) = agent_id.map(str::trim).filter(|value| !value.is_empty()) {
        migrate_legacy_json(
            &legacy_mcp_path_for_agent(agent_id),
            &mcp_config_path_for_agent(agent_id),
        )?;
    }

    let global = read_toml_root(&global_path)?;
    let mut merged = BTreeMap::new();
    merge_servers(&mut merged, global.mcp_servers.clone())?;
    if let Some(project_root) = project_root.filter(|root| project_is_trusted(&global, root)) {
        let project = read_toml_root(&mcp_config_path_for_project(project_root))?;
        merge_servers(&mut merged, project.mcp_servers)?;
    }
    if let Some(agent_id) = agent_id.map(str::trim).filter(|value| !value.is_empty()) {
        let agent = read_toml_root(&mcp_config_path_for_agent(agent_id))?;
        merge_servers(&mut merged, agent.mcp_servers)?;
    }
    Ok(merged.into_values().collect())
}

/// 无项目上下文时读取全局与 Agent 配置。
pub fn load_mcp_servers(agent_id: Option<&str>) -> anyhow::Result<Vec<McpServerConfig>> {
    load_mcp_servers_layered(agent_id, None)
}

/// 将服务器列表写入 Agent 私有层；未指定 Agent 时写入全局层。
pub fn save_mcp_servers(agent_id: Option<&str>, servers: &[McpServerConfig]) -> anyhow::Result<()> {
    ensure_default_workspace_dirs()?;
    let agent_id = agent_id.map(str::trim).filter(|value| !value.is_empty());
    let (path, legacy_path) = if let Some(agent_id) = agent_id {
        (
            mcp_config_path_for_agent(agent_id),
            legacy_mcp_path_for_agent(agent_id),
        )
    } else {
        (mcp_config_path_global(), legacy_mcp_path_global())
    };
    migrate_legacy_json(&legacy_path, &path)?;
    let mut root = read_toml_value(&path)?;
    let table = root
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("MCP config root must be a TOML table"))?;
    let mapped: BTreeMap<String, TomlMcpServer> = servers
        .iter()
        .map(|config| {
            let id = sanitize_server_id(&config.id);
            (id, TomlMcpServer::from_config(config))
        })
        .collect();
    table.insert("mcp_servers".into(), toml::Value::try_from(mapped)?);
    write_toml_value(&path, &root)
}

/// 加载当前活跃 Agent 的 MCP 服务器配置。
pub fn load_for_active_agent() -> anyhow::Result<Vec<McpServerConfig>> {
    let base = default_memory_dir();
    let id = active_agent_id(&base);
    load_mcp_servers(Some(&id))
}

/// 合并 discovered：保留已有 tools 开关；新工具默认 true
pub fn merge_discovered(server: &mut McpServerConfig, discovered: Vec<DiscoveredTool>) {
    for d in &discovered {
        server.tools.entry(d.name.clone()).or_insert(true);
    }
    server.discovered = discovered;
}

/// 只把 discovered 写回磁盘，并与磁盘上最新的 tools 开关合并，避免覆盖 UI 刚保存的开关
pub fn persist_discovered(
    agent_id: Option<&str>,
    updates: &[(String, Vec<DiscoveredTool>)],
) -> anyhow::Result<()> {
    persist_discovered_layered(agent_id, None, updates)
}

/// 只向当前可写层中已存在的 Server 回写发现缓存。
///
/// 继承自全局或项目层的 Server 不会被自动复制到 Agent 层，避免一次
/// `tools/list` 将分层配置意外摊平。这个缓存后续应迁移到独立运行时状态。
pub fn persist_discovered_layered(
    agent_id: Option<&str>,
    _project_root: Option<&Path>,
    updates: &[(String, Vec<DiscoveredTool>)],
) -> anyhow::Result<()> {
    if updates.is_empty() {
        return Ok(());
    }
    ensure_default_workspace_dirs()?;
    let agent_id = agent_id.map(str::trim).filter(|value| !value.is_empty());
    let (path, legacy_path) = if let Some(agent_id) = agent_id {
        (
            mcp_config_path_for_agent(agent_id),
            legacy_mcp_path_for_agent(agent_id),
        )
    } else {
        (mcp_config_path_global(), legacy_mcp_path_global())
    };
    migrate_legacy_json(&legacy_path, &path)?;

    let mut root = read_toml_value(&path)?;
    let root_table = root
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("MCP config root must be a TOML table"))?;
    let Some(server_table) = root_table
        .get_mut("mcp_servers")
        .and_then(toml::Value::as_table_mut)
    else {
        return Ok(());
    };

    let mut changed = false;
    for (sid, discovered) in updates {
        let sid = sanitize_server_id(sid);
        let Some((_, server)) = server_table
            .iter_mut()
            .find(|(configured_id, _)| sanitize_server_id(configured_id) == sid)
        else {
            continue;
        };
        let server = server
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("MCP server {sid:?} must be a TOML table"))?;
        let tools = server
            .entry("tools")
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("MCP server {sid:?} tools must be a TOML table"))?;
        for tool in discovered {
            tools
                .entry(tool.name.clone())
                .or_insert(toml::Value::Boolean(true));
        }
        server.insert(
            "discovered".into(),
            toml::Value::try_from(discovered.clone())?,
        );
        changed = true;
    }
    if changed {
        write_toml_value(&path, &root)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::TempDir;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn roundtrip_and_defaults() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        let _ = ensure_default_workspace_dirs();
        let servers = vec![McpServerConfig {
            id: "s1".into(),
            name: "demo".into(),
            description: String::new(),
            r#type: McpTransportType::Stdio,
            command: "echo".into(),
            args: vec![],
            env: HashMap::new(),
            env_vars: vec![],
            url: String::new(),
            headers: HashMap::new(),
            bearer_token_env_var: None,
            env_http_headers: HashMap::new(),
            enabled: true,
            required: false,
            cwd: None,
            enabled_tools: None,
            disabled_tools: vec![],
            tools: HashMap::from([("a".into(), false)]),
            discovered: vec![],
            startup_timeout_secs: None,
            tool_timeout_secs: None,
        }];
        save_mcp_servers(None, &servers).unwrap();
        let loaded = load_mcp_servers(None).unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(!loaded[0].is_tool_enabled("a"));
        assert!(loaded[0].is_tool_enabled("missing"));
    }

    #[test]
    fn persist_discovered_preserves_disk_tool_gates() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        let _ = ensure_default_workspace_dirs();
        let servers = vec![McpServerConfig {
            id: "s1".into(),
            name: "demo".into(),
            description: String::new(),
            r#type: McpTransportType::Stdio,
            command: "echo".into(),
            args: vec![],
            env: HashMap::new(),
            env_vars: vec![],
            url: String::new(),
            headers: HashMap::new(),
            bearer_token_env_var: None,
            env_http_headers: HashMap::new(),
            enabled: true,
            required: false,
            cwd: None,
            enabled_tools: None,
            disabled_tools: vec![],
            tools: HashMap::from([("a".into(), false)]),
            discovered: vec![],
            startup_timeout_secs: None,
            tool_timeout_secs: None,
        }];
        save_mcp_servers(None, &servers).unwrap();

        persist_discovered(
            None,
            &[(
                "s1".into(),
                vec![
                    DiscoveredTool {
                        name: "a".into(),
                        description: "A".into(),
                    },
                    DiscoveredTool {
                        name: "b".into(),
                        description: "B".into(),
                    },
                ],
            )],
        )
        .unwrap();

        let loaded = load_mcp_servers(None).unwrap();
        assert!(!loaded[0].is_tool_enabled("a"));
        assert!(loaded[0].is_tool_enabled("b"));
        assert_eq!(loaded[0].discovered.len(), 2);
    }

    #[test]
    fn persist_discovered_does_not_flatten_inherited_server_into_agent_layer() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        ensure_default_workspace_dirs().unwrap();
        fs::write(
            mcp_config_path_global(),
            r#"[mcp_servers.inherited]
command = "global-command"
"#,
        )
        .unwrap();
        let agent_path = mcp_config_path_for_agent("worker");
        fs::create_dir_all(agent_path.parent().unwrap()).unwrap();
        fs::write(&agent_path, "[agent]\nenabled = true\n").unwrap();

        persist_discovered_layered(
            Some("worker"),
            None,
            &[(
                "inherited".into(),
                vec![DiscoveredTool {
                    name: "read".into(),
                    description: "Read".into(),
                }],
            )],
        )
        .unwrap();

        let agent_config = fs::read_to_string(agent_path).unwrap();
        assert!(agent_config.contains("[agent]"));
        assert!(!agent_config.contains("mcp_servers"));
        let loaded = load_mcp_servers_layered(Some("worker"), None).unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(loaded[0].discovered.is_empty());
    }

    #[test]
    fn de_type_rejects_unknown_transport() {
        let err = serde_json::from_str::<McpServerConfig>(
            r#"{
                "id": "x",
                "name": "x",
                "type": "websocket"
            }"#,
        );
        assert!(err.is_err(), "unknown transport must fail deserialize");
    }

    #[test]
    fn legacy_sse_is_rejected_with_migration_guidance() {
        let err = serde_json::from_str::<McpServerConfig>(
            r#"{
                "id": "legacy",
                "name": "legacy",
                "type": "sse",
                "url": "http://localhost:3000/sse"
            }"#,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("legacy SSE transport is not supported"));
        assert!(err.contains("Streamable HTTP /mcp"));
    }

    #[test]
    fn de_type_accepts_known_aliases() {
        let cfg: McpServerConfig = serde_json::from_str(
            r#"{
                "id": "x",
                "name": "x",
                "type": "streamable_http"
            }"#,
        )
        .unwrap();
        assert_eq!(cfg.r#type.as_str(), "streamableHttp");
    }

    #[test]
    fn timeout_defaults_and_bounds_match_runtime_contract() {
        let mut cfg: McpServerConfig = serde_json::from_str(
            r#"{
                "id": "x",
                "name": "x",
                "type": "stdio"
            }"#,
        )
        .unwrap();
        assert_eq!(cfg.effective_startup_timeout_secs(), 10);
        assert_eq!(cfg.effective_tool_timeout_secs(), 60);

        cfg.startup_timeout_secs = Some(0);
        cfg.tool_timeout_secs = Some(9_999);
        assert_eq!(cfg.effective_startup_timeout_secs(), 1);
        assert_eq!(cfg.effective_tool_timeout_secs(), 3_600);
    }

    #[test]
    fn codex_timeout_aliases_are_preserved() {
        let cfg: McpServerConfig = serde_json::from_str(
            r#"{
                "id": "x",
                "name": "x",
                "type": "stdio",
                "startup_timeout_sec": 17,
                "tool_timeout_sec": 91
            }"#,
        )
        .unwrap();
        assert_eq!(cfg.startup_timeout_secs, Some(17));
        assert_eq!(cfg.tool_timeout_secs, Some(91));

        let serialized = serde_json::to_value(cfg).unwrap();
        assert_eq!(serialized["startupTimeoutSecs"], 17);
        assert_eq!(serialized["toolTimeoutSecs"], 91);
    }

    #[test]
    fn codex_environment_reference_fields_roundtrip_in_toml() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        ensure_default_workspace_dirs().unwrap();
        fs::write(
            mcp_config_path_global(),
            r#"[mcp_servers.local]
command = "npx"
env_vars = ["LOCAL_TOKEN"]

[mcp_servers.remote]
url = "https://example.com/mcp"
bearer_token_env_var = "MCP_ACCESS_TOKEN"
http_headers = { X-Region = "us-east-1" }
env_http_headers = { X-API-Key = "MCP_API_KEY" }
"#,
        )
        .unwrap();

        let loaded = load_mcp_servers(None).unwrap();
        let local = loaded.iter().find(|server| server.id == "local").unwrap();
        assert_eq!(local.env_vars, vec!["LOCAL_TOKEN"]);
        let remote = loaded.iter().find(|server| server.id == "remote").unwrap();
        assert_eq!(
            remote.bearer_token_env_var.as_deref(),
            Some("MCP_ACCESS_TOKEN")
        );
        assert_eq!(remote.headers["X-Region"], "us-east-1");
        assert_eq!(remote.env_http_headers["X-API-Key"], "MCP_API_KEY");

        save_mcp_servers(None, &loaded).unwrap();
        let persisted = fs::read_to_string(mcp_config_path_global()).unwrap();
        assert!(persisted.contains("env_vars"));
        assert!(persisted.contains("bearer_token_env_var"));
        assert!(persisted.contains("http_headers"));
        assert!(persisted.contains("env_http_headers"));
        assert!(!persisted.contains("secret-token"));
    }

    #[test]
    fn layered_toml_uses_trusted_project_and_whole_server_overrides() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        ensure_default_workspace_dirs().unwrap();
        let project = dir.path().join("project");
        fs::create_dir_all(project.join(".astro")).unwrap();
        let agent_path = mcp_config_path_for_agent("worker");
        fs::create_dir_all(agent_path.parent().unwrap()).unwrap();

        fs::write(
            mcp_config_path_global(),
            format!(
                r#"[projects."{}"]
trust_level = "trusted"

[mcp_servers.shared]
command = "global-command"
args = ["from-global"]

[mcp_servers.global-only]
command = "global-only"
"#,
                project.display()
            ),
        )
        .unwrap();
        fs::write(
            mcp_config_path_for_project(&project),
            r#"[mcp_servers.shared]
command = "project-command"

[mcp_servers.project-only]
url = "https://example.com/mcp"
"#,
        )
        .unwrap();
        fs::write(
            agent_path,
            r#"[mcp_servers.shared]
command = "agent-command"
cwd = "packages/server"
required = true
tool_timeout_sec = 91
enabled_tools = ["read", "search"]
disabled_tools = ["search"]
"#,
        )
        .unwrap();

        let loaded = load_mcp_servers_layered(Some("worker"), Some(&project)).unwrap();
        assert_eq!(loaded.len(), 3);
        let shared = loaded.iter().find(|server| server.id == "shared").unwrap();
        assert_eq!(shared.command, "agent-command");
        assert!(
            shared.args.is_empty(),
            "override must replace the full server"
        );
        assert_eq!(shared.tool_timeout_secs, Some(91));
        assert!(shared.required);
        assert_eq!(shared.cwd.as_deref(), Some("packages/server"));
        assert!(shared.is_tool_enabled("read"));
        assert!(!shared.is_tool_enabled("search"));
        assert!(!shared.is_tool_enabled("unknown"));
        assert!(loaded.iter().any(|server| server.id == "global-only"));
        assert!(loaded.iter().any(|server| server.id == "project-only"));
    }

    #[test]
    fn untrusted_project_config_is_ignored() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        ensure_default_workspace_dirs().unwrap();
        let project = dir.path().join("project");
        fs::create_dir_all(project.join(".astro")).unwrap();
        fs::write(
            mcp_config_path_global(),
            r#"[mcp_servers.global]
command = "global-command"
"#,
        )
        .unwrap();
        fs::write(
            mcp_config_path_for_project(&project),
            r#"[mcp_servers.project]
command = "must-not-load"
"#,
        )
        .unwrap();

        let loaded = load_mcp_servers_layered(None, Some(&project)).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "global");
    }

    #[test]
    fn toml_rejects_legacy_sse_transport_field() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        ensure_default_workspace_dirs().unwrap();
        fs::write(
            mcp_config_path_global(),
            r#"[mcp_servers.legacy]
type = "sse"
url = "http://localhost:3000/sse"
"#,
        )
        .unwrap();

        let error = load_mcp_servers(None).unwrap_err().to_string();
        assert!(error.contains("legacy SSE transport is not supported"));
    }

    #[test]
    fn http_server_rejects_stdio_only_cwd() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        ensure_default_workspace_dirs().unwrap();
        fs::write(
            mcp_config_path_global(),
            r#"[mcp_servers.remote]
url = "https://example.com/mcp"
cwd = "packages/server"
"#,
        )
        .unwrap();

        let error = load_mcp_servers(None).unwrap_err().to_string();
        assert!(error.contains("cannot define cwd"), "{error}");
    }

    #[test]
    fn legacy_json_migrates_once_and_preserves_other_toml_sections() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        ensure_default_workspace_dirs().unwrap();
        let legacy_path = legacy_mcp_path_global();
        fs::write(
            &legacy_path,
            r#"{
  "servers": [{
    "id": "legacy",
    "name": "Legacy",
    "type": "stdio",
    "command": "legacy-command"
  }]
}"#,
        )
        .unwrap();
        fs::write(
            mcp_config_path_global(),
            r#"[agents]
enabled = false
"#,
        )
        .unwrap();

        let loaded = load_mcp_servers(None).unwrap();
        assert_eq!(loaded[0].command, "legacy-command");
        assert!(backup_path_for_legacy(&legacy_path).is_file());
        let migrated = fs::read_to_string(mcp_config_path_global()).unwrap();
        assert!(migrated.contains("[agents]"));
        assert!(migrated.contains("[mcp_servers.legacy]"));

        fs::write(
            &legacy_path,
            r#"{"servers":[{"id":"changed","name":"Changed","type":"stdio","command":"changed"}]}"#,
        )
        .unwrap();
        let loaded_again = load_mcp_servers(None).unwrap();
        assert_eq!(loaded_again.len(), 1);
        assert_eq!(loaded_again[0].id, "legacy");
    }
}
