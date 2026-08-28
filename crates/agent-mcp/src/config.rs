//! 读写统一的 Astro `config.toml` MCP 配置。
//!
//! 配置只来自 `~/.astro/config.toml` 与可信项目的
//! `<project>/.astro/config.toml`；旧 `mcp.json`、`.codex` 与 Agent 私有
//! `config.toml` 都不是配置输入。

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use agent_config::loader::{load_local_config, LocalConfigOptions};
use anyhow::Context;
use serde::{Deserialize, Serialize};

use home::{default_memory_dir, ensure_default_workspace_dirs};

use crate::names::sanitize_server_id;
use types::{McpToolAnnotations, McpToolApprovalMode};

/// MCP server 启动默认超时（秒）。
pub const DEFAULT_STARTUP_TIMEOUT_SECS: u64 = 10;
/// MCP 工具调用默认超时（秒）。
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

/// Streamable HTTP 认证方式。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum McpHttpAuth {
    /// 标准 MCP OAuth 2.1 Authorization Code + PKCE。
    OAuth,
    /// 第一方 ChatGPT 会话认证；Astro 当前不具备该信任通道。
    Chatgpt,
}

impl McpHttpAuth {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OAuth => "oauth",
            Self::Chatgpt => "chatgpt",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "oauth" => Ok(Self::OAuth),
            "chatgpt" => Ok(Self::Chatgpt),
            other => Err(format!(
                "unsupported MCP HTTP auth {other:?}; expected oauth or chatgpt"
            )),
        }
    }
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
    /// Server 提供的非授权性风险提示。
    #[serde(default, flatten)]
    pub annotations: McpToolAnnotations,
}

/// 单工具配置。布尔值兼容 Astro 旧开关，table 支持 `approval_mode`。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum McpToolConfig {
    Enabled(bool),
    Settings(McpToolSettings),
}

/// MCP 单工具覆盖项。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpToolSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_mode: Option<McpToolApprovalMode>,
}

impl McpToolConfig {
    pub fn is_enabled(&self) -> bool {
        match self {
            Self::Enabled(enabled) => *enabled,
            Self::Settings(settings) => settings.enabled.unwrap_or(true),
        }
    }

    pub fn approval_mode(&self) -> Option<McpToolApprovalMode> {
        match self {
            Self::Enabled(_) => None,
            Self::Settings(settings) => settings.approval_mode,
        }
    }

    pub fn with_enabled(self, enabled: bool) -> Self {
        match self {
            Self::Enabled(_) => Self::Enabled(enabled),
            Self::Settings(mut settings) => {
                settings.enabled = Some(enabled);
                Self::Settings(settings)
            }
        }
    }

    pub fn from_parts(enabled: bool, approval_mode: Option<McpToolApprovalMode>) -> Self {
        match approval_mode {
            Some(approval_mode) => Self::Settings(McpToolSettings {
                enabled: (!enabled).then_some(false),
                approval_mode: Some(approval_mode),
            }),
            None => Self::Enabled(enabled),
        }
    }
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
    /// HTTP 认证方式；缺省时优先匿名连接，401 后提示 OAuth 登录。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<McpHttpAuth>,
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
    /// Server 默认工具审批模式。
    #[serde(default)]
    pub default_tools_approval_mode: McpToolApprovalMode,
    /// 单工具开关及审批覆盖；布尔值旧格式仍可读取。
    #[serde(default)]
    pub tools: HashMap<String, McpToolConfig>,
    /// 最近一次 list_tools 缓存（供 UI）
    #[serde(default)]
    pub discovered: Vec<DiscoveredTool>,
}

/// serde 默认：字段缺省为 `true`。
fn default_true() -> bool {
    true
}

fn is_auto_approval_mode(mode: &McpToolApprovalMode) -> bool {
    *mode == McpToolApprovalMode::Auto
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
        self.tools
            .get(tool_name)
            .map(McpToolConfig::is_enabled)
            .unwrap_or(true)
    }

    /// 单工具覆盖优先，否则使用 Server 默认审批模式。
    pub fn tool_approval_mode(&self, tool_name: &str) -> McpToolApprovalMode {
        self.tools
            .get(tool_name)
            .and_then(McpToolConfig::approval_mode)
            .unwrap_or(self.default_tools_approval_mode)
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
        self.auth.hash(&mut hasher);
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
    auth: Option<McpHttpAuth>,
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
    #[serde(default, skip_serializing_if = "is_auto_approval_mode")]
    default_tools_approval_mode: McpToolApprovalMode,
    /// Astro 逐工具开关兼容旧 bool；table 同时承载 approval_mode。
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    tools: HashMap<String, McpToolConfig>,
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
                    || !self.env_http_headers.is_empty()
                    || self.auth.is_some() =>
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
            auth: self.auth,
            enabled: self.enabled.unwrap_or(true),
            required: self.required.unwrap_or(false),
            cwd: self.cwd.filter(|value| !value.trim().is_empty()),
            startup_timeout_secs: self.startup_timeout_sec,
            tool_timeout_secs: self.tool_timeout_sec,
            enabled_tools: self.enabled_tools,
            disabled_tools: self.disabled_tools,
            default_tools_approval_mode: self.default_tools_approval_mode,
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
            args: if config.r#type == McpTransportType::Stdio {
                config.args.clone()
            } else {
                Vec::new()
            },
            env: if config.r#type == McpTransportType::Stdio {
                config.env.clone()
            } else {
                HashMap::new()
            },
            env_vars: if config.r#type == McpTransportType::Stdio {
                config.env_vars.clone()
            } else {
                Vec::new()
            },
            cwd: (config.r#type == McpTransportType::Stdio)
                .then(|| config.cwd.clone())
                .flatten(),
            url: (config.r#type == McpTransportType::StreamableHttp).then(|| config.url.clone()),
            headers: if config.r#type == McpTransportType::StreamableHttp {
                config.headers.clone()
            } else {
                HashMap::new()
            },
            bearer_token_env_var: (config.r#type == McpTransportType::StreamableHttp)
                .then(|| config.bearer_token_env_var.clone())
                .flatten(),
            env_http_headers: if config.r#type == McpTransportType::StreamableHttp {
                config.env_http_headers.clone()
            } else {
                HashMap::new()
            },
            auth: (config.r#type == McpTransportType::StreamableHttp)
                .then_some(config.auth)
                .flatten(),
            enabled: (!config.enabled).then_some(false),
            required: config.required.then_some(true),
            startup_timeout_sec: config.startup_timeout_secs,
            tool_timeout_sec: config.tool_timeout_secs,
            enabled_tools: config.enabled_tools.clone(),
            disabled_tools: config.disabled_tools.clone(),
            default_tools_approval_mode: config.default_tools_approval_mode,
            tools: config.tools.clone(),
            discovered: config.discovered.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
struct McpTomlRoot {
    #[serde(default)]
    mcp_servers: BTreeMap<String, TomlMcpServer>,
}

/// 全局 `~/.astro/config.toml` 路径。
pub fn mcp_config_path_global() -> PathBuf {
    default_memory_dir().join("config.toml")
}

/// 可信项目 `<project>/.astro/config.toml` 路径。
pub fn mcp_config_path_for_project(project_root: &Path) -> PathBuf {
    project_root.join(".astro").join("config.toml")
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

/// Decode an inline `mcp_servers` table from a custom agent file.
/// The resulting configs are an overlay; callers decide which inherited layer
/// they replace.
pub fn decode_inline_mcp_servers(
    servers: &BTreeMap<String, toml::Value>,
) -> anyhow::Result<Vec<McpServerConfig>> {
    let mut decoded = BTreeMap::new();
    for (id, value) in servers {
        let server: TomlMcpServer = value
            .clone()
            .try_into()
            .with_context(|| format!("decode inline MCP server {id}"))?;
        let config = server.into_config(id.clone())?;
        decoded.insert(config.id.clone(), config);
    }
    Ok(decoded.into_values().collect())
}

/// 按 `系统 → 全局 → 可信项目` 读取并以 Server 为单位整体覆盖。
pub fn load_mcp_servers_layered(
    project_root: Option<&Path>,
) -> anyhow::Result<Vec<McpServerConfig>> {
    ensure_default_workspace_dirs()?;
    let astro_home = default_memory_dir();
    let cwd = project_root.unwrap_or(&astro_home);
    let loaded = load_local_config(&LocalConfigOptions::new(&astro_home, cwd))?;
    let mut merged = BTreeMap::new();
    for layer in loaded
        .layers
        .layers_low_to_high()
        .filter(|layer| layer.is_enabled())
    {
        let root: McpTomlRoot = layer
            .config
            .clone()
            .try_into()
            .with_context(|| format!("decode MCP config layer {:?}", layer.source))?;
        merge_servers(&mut merged, root.mcp_servers)?;
    }
    Ok(merged.into_values().collect())
}

/// 无项目上下文时读取统一的全局配置。
pub fn load_mcp_servers() -> anyhow::Result<Vec<McpServerConfig>> {
    load_mcp_servers_layered(None)
}

/// 读取单一配置层，不进行跨层合并。
pub fn load_mcp_servers_scoped(
    scope: &str,
    project_root: Option<&Path>,
) -> anyhow::Result<Vec<McpServerConfig>> {
    if scope == "builtin" {
        return Ok(Vec::new());
    }
    let path = match scope {
        "project" => {
            let root = project_root
                .ok_or_else(|| anyhow::anyhow!("project scope requires a project root"))?;
            mcp_config_path_for_project(root)
        }
        _ => mcp_config_path_global(),
    };
    let root: McpTomlRoot = read_toml_value(&path)?
        .try_into()
        .with_context(|| format!("decode MCP config {}", path.display()))?;
    let mut merged = BTreeMap::new();
    merge_servers(&mut merged, root.mcp_servers)?;
    Ok(merged.into_values().collect())
}

/// 将服务器列表写入统一的 `~/.astro/config.toml`。
pub fn save_mcp_servers(servers: &[McpServerConfig]) -> anyhow::Result<()> {
    save_mcp_servers_scoped("global", None, servers)
}

/// 将服务器列表写回指定可编辑层；builtin 永远只读。
pub fn save_mcp_servers_scoped(
    scope: &str,
    project_root: Option<&Path>,
    servers: &[McpServerConfig],
) -> anyhow::Result<()> {
    ensure_default_workspace_dirs()?;
    let path = match scope {
        "builtin" => anyhow::bail!("builtin MCP servers are read-only"),
        "project" => {
            let root = project_root
                .ok_or_else(|| anyhow::anyhow!("project scope requires a project root"))?;
            mcp_config_path_for_project(root)
        }
        _ => mcp_config_path_global(),
    };
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

/// 合并 discovered：保留已有 tools 开关；新工具默认 true
pub fn merge_discovered(server: &mut McpServerConfig, discovered: Vec<DiscoveredTool>) {
    for d in &discovered {
        server
            .tools
            .entry(d.name.clone())
            .or_insert(McpToolConfig::Enabled(true));
    }
    server.discovered = discovered;
}

/// 只把 discovered 写回磁盘，并与磁盘上最新的 tools 开关合并，避免覆盖 UI 刚保存的开关
pub fn persist_discovered(updates: &[(String, Vec<DiscoveredTool>)]) -> anyhow::Result<()> {
    persist_discovered_layered(None, updates)
}

/// 只向当前可写层中已存在的 Server 回写发现缓存。
///
/// 继承自系统或项目层的 Server 不会被自动复制到全局层，避免一次
/// `tools/list` 将分层配置意外摊平。这个缓存后续应迁移到独立运行时状态。
pub fn persist_discovered_layered(
    _project_root: Option<&Path>,
    updates: &[(String, Vec<DiscoveredTool>)],
) -> anyhow::Result<()> {
    if updates.is_empty() {
        return Ok(());
    }
    ensure_default_workspace_dirs()?;
    let path = mcp_config_path_global();

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
    fn decodes_inline_agent_mcp_servers() {
        let value: toml::Value = r#"
[mcp_servers.docs]
url = "https://example.invalid/mcp"
startup_timeout_sec = 20
enabled_tools = ["search"]
"#
        .parse()
        .unwrap();
        let servers = value
            .get("mcp_servers")
            .and_then(toml::Value::as_table)
            .unwrap()
            .iter()
            .map(|(id, value)| (id.clone(), value.clone()))
            .collect();
        let decoded = decode_inline_mcp_servers(&servers).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].id, "docs");
        assert_eq!(decoded[0].url, "https://example.invalid/mcp");
        assert_eq!(decoded[0].startup_timeout_secs, Some(20));
        assert_eq!(
            decoded[0].enabled_tools.as_ref().unwrap(),
            &vec!["search".to_string()]
        );
    }

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
            auth: None,
            enabled: true,
            required: false,
            cwd: None,
            enabled_tools: None,
            disabled_tools: vec![],
            default_tools_approval_mode: McpToolApprovalMode::Auto,
            tools: HashMap::from([("a".into(), McpToolConfig::Enabled(false))]),
            discovered: vec![],
            startup_timeout_secs: None,
            tool_timeout_secs: None,
        }];
        save_mcp_servers(&servers).unwrap();
        let loaded = load_mcp_servers().unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(!loaded[0].is_tool_enabled("a"));
        assert!(loaded[0].is_tool_enabled("missing"));
    }

    #[test]
    fn save_updates_only_mcp_table_in_unified_config() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        ensure_default_workspace_dirs().unwrap();
        fs::write(
            mcp_config_path_global(),
            "[agents]\nenabled = false\n\n[custom]\nvalue = 'keep'\n",
        )
        .unwrap();
        let server: McpServerConfig = serde_json::from_value(serde_json::json!({
            "id": "docs",
            "name": "docs",
            "type": "stdio",
            "command": "docs-server"
        }))
        .unwrap();

        save_mcp_servers(&[server]).unwrap();

        let saved = fs::read_to_string(mcp_config_path_global()).unwrap();
        assert!(saved.contains("[agents]"));
        assert!(saved.contains("enabled = false"));
        assert!(saved.contains("[custom]"));
        assert!(saved.contains("value = \"keep\""));
        assert!(saved.contains("[mcp_servers.docs]"));
    }

    #[test]
    fn approval_modes_and_legacy_boolean_tools_coexist() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        ensure_default_workspace_dirs().unwrap();
        fs::write(
            mcp_config_path_global(),
            r#"[mcp_servers.docs]
command = "docs-server"
default_tools_approval_mode = "writes"

[mcp_servers.docs.tools.read]
approval_mode = "approve"

[mcp_servers.docs.tools.publish]
enabled = false
approval_mode = "prompt"

[mcp_servers.legacy]
command = "legacy-server"
tools = { read = true, write = false }
"#,
        )
        .unwrap();

        let loaded = load_mcp_servers().unwrap();
        let docs = loaded.iter().find(|server| server.id == "docs").unwrap();
        assert_eq!(
            docs.default_tools_approval_mode,
            McpToolApprovalMode::Writes
        );
        assert_eq!(
            docs.tool_approval_mode("read"),
            McpToolApprovalMode::Approve
        );
        assert_eq!(
            docs.tool_approval_mode("publish"),
            McpToolApprovalMode::Prompt
        );
        assert!(!docs.is_tool_enabled("publish"));

        let legacy = loaded.iter().find(|server| server.id == "legacy").unwrap();
        assert!(legacy.is_tool_enabled("read"));
        assert!(!legacy.is_tool_enabled("write"));

        save_mcp_servers(&loaded).unwrap();
        let reloaded = load_mcp_servers().unwrap();
        let docs = reloaded.iter().find(|server| server.id == "docs").unwrap();
        assert_eq!(
            docs.tool_approval_mode("read"),
            McpToolApprovalMode::Approve
        );
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
            auth: None,
            enabled: true,
            required: false,
            cwd: None,
            enabled_tools: None,
            disabled_tools: vec![],
            default_tools_approval_mode: McpToolApprovalMode::Auto,
            tools: HashMap::from([("a".into(), McpToolConfig::Enabled(false))]),
            discovered: vec![],
            startup_timeout_secs: None,
            tool_timeout_secs: None,
        }];
        save_mcp_servers(&servers).unwrap();

        persist_discovered(&[(
            "s1".into(),
            vec![
                DiscoveredTool {
                    name: "a".into(),
                    description: "A".into(),
                    annotations: Default::default(),
                },
                DiscoveredTool {
                    name: "b".into(),
                    description: "B".into(),
                    annotations: Default::default(),
                },
            ],
        )])
        .unwrap();

        let loaded = load_mcp_servers().unwrap();
        assert!(!loaded[0].is_tool_enabled("a"));
        assert!(loaded[0].is_tool_enabled("b"));
        assert_eq!(loaded[0].discovered.len(), 2);
    }

    #[test]
    fn persist_discovered_does_not_flatten_project_server_into_global_layer() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        ensure_default_workspace_dirs().unwrap();
        let project = dir.path().join("project");
        fs::create_dir_all(project.join(".astro")).unwrap();
        fs::write(
            mcp_config_path_global(),
            format!(
                r#"[projects."{}"]
trust_level = "trusted"
"#,
                project.display()
            ),
        )
        .unwrap();
        fs::write(
            mcp_config_path_for_project(&project),
            r#"[mcp_servers.inherited]
command = "project-command"
"#,
        )
        .unwrap();

        persist_discovered_layered(
            Some(&project),
            &[(
                "inherited".into(),
                vec![DiscoveredTool {
                    name: "read".into(),
                    description: "Read".into(),
                    annotations: Default::default(),
                }],
            )],
        )
        .unwrap();

        let global_config = fs::read_to_string(mcp_config_path_global()).unwrap();
        assert!(global_config.contains("[projects."));
        assert!(!global_config.contains("mcp_servers"));
        let loaded = load_mcp_servers_layered(Some(&project)).unwrap();
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
    fn timeout_aliases_are_preserved() {
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
    fn environment_reference_fields_roundtrip_in_toml() {
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
auth = "oauth"
bearer_token_env_var = "MCP_ACCESS_TOKEN"
http_headers = { X-Region = "us-east-1" }
env_http_headers = { X-API-Key = "MCP_API_KEY" }
"#,
        )
        .unwrap();

        let loaded = load_mcp_servers().unwrap();
        let local = loaded.iter().find(|server| server.id == "local").unwrap();
        assert_eq!(local.env_vars, vec!["LOCAL_TOKEN"]);
        let remote = loaded.iter().find(|server| server.id == "remote").unwrap();
        assert_eq!(
            remote.bearer_token_env_var.as_deref(),
            Some("MCP_ACCESS_TOKEN")
        );
        assert_eq!(remote.headers["X-Region"], "us-east-1");
        assert_eq!(remote.env_http_headers["X-API-Key"], "MCP_API_KEY");
        assert_eq!(remote.auth, Some(McpHttpAuth::OAuth));

        save_mcp_servers(&loaded).unwrap();
        let persisted = fs::read_to_string(mcp_config_path_global()).unwrap();
        assert!(persisted.contains("env_vars"));
        assert!(persisted.contains("bearer_token_env_var"));
        assert!(persisted.contains("http_headers"));
        assert!(persisted.contains("env_http_headers"));
        assert!(persisted.contains("auth = \"oauth\""));
        assert!(!persisted.contains("secret-token"));
    }

    #[test]
    fn toml_projection_keeps_only_transport_specific_collection_fields() {
        let stdio: McpServerConfig = serde_json::from_value(serde_json::json!({
            "id": "local",
            "name": "local",
            "type": "stdio",
            "command": "npx",
            "args": ["-y", "server"],
            "env": { "LOG_LEVEL": "info" },
            "envVars": ["LOCAL_TOKEN"]
        }))
        .unwrap();
        let stdio_toml = TomlMcpServer::from_config(&stdio);
        assert_eq!(stdio_toml.args, vec!["-y", "server"]);
        assert_eq!(stdio_toml.env["LOG_LEVEL"], "info");
        assert_eq!(stdio_toml.env_vars, vec!["LOCAL_TOKEN"]);
        assert!(stdio_toml.headers.is_empty());
        assert!(stdio_toml.env_http_headers.is_empty());

        let http: McpServerConfig = serde_json::from_value(serde_json::json!({
            "id": "remote",
            "name": "remote",
            "type": "streamableHttp",
            "url": "https://example.com/mcp",
            "headers": { "X-Region": "us-east-1" },
            "envHttpHeaders": { "X-API-Key": "MCP_API_KEY" }
        }))
        .unwrap();
        let http_toml = TomlMcpServer::from_config(&http);
        assert!(http_toml.args.is_empty());
        assert!(http_toml.env.is_empty());
        assert!(http_toml.env_vars.is_empty());
        assert_eq!(http_toml.headers["X-Region"], "us-east-1");
        assert_eq!(http_toml.env_http_headers["X-API-Key"], "MCP_API_KEY");
    }

    #[test]
    fn layered_toml_uses_trusted_project_and_whole_server_overrides() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        ensure_default_workspace_dirs().unwrap();
        let project = dir.path().join("project");
        fs::create_dir_all(project.join(".astro")).unwrap();
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
        let loaded = load_mcp_servers_layered(Some(&project)).unwrap();
        assert_eq!(loaded.len(), 3);
        let shared = loaded.iter().find(|server| server.id == "shared").unwrap();
        assert_eq!(shared.command, "project-command");
        assert!(
            shared.args.is_empty(),
            "override must replace the full server"
        );
        assert_eq!(shared.tool_timeout_secs, None);
        assert!(!shared.required);
        assert_eq!(shared.cwd, None);
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

        let loaded = load_mcp_servers_layered(Some(&project)).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "global");
    }

    #[test]
    fn dotcodex_and_agent_private_config_paths_are_not_inputs() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        ensure_default_workspace_dirs().unwrap();
        let project = dir.path().join("project");
        fs::create_dir_all(project.join(".codex")).unwrap();
        fs::write(
            mcp_config_path_global(),
            r#"[mcp_servers.global]
command = "global-command"
"#,
        )
        .unwrap();
        fs::write(
            project.join(".codex/config.toml"),
            r#"[mcp_servers.codex]
command = "must-not-load"
"#,
        )
        .unwrap();
        let private = default_memory_dir().join("agents/worker/config.toml");
        fs::create_dir_all(private.parent().unwrap()).unwrap();
        fs::write(
            private,
            r#"[mcp_servers.private]
command = "must-not-load"
"#,
        )
        .unwrap();

        let loaded = load_mcp_servers_layered(Some(&project)).unwrap();
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

        let error = load_mcp_servers().unwrap_err().to_string();
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

        let error = load_mcp_servers().unwrap_err().to_string();
        assert!(error.contains("cannot define cwd"), "{error}");
    }

    #[test]
    fn legacy_json_is_not_a_configuration_input() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = TempDir::new().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        ensure_default_workspace_dirs().unwrap();
        let legacy_path = default_memory_dir().join("mcp.json");
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

        let loaded = load_mcp_servers().unwrap();
        assert!(loaded.is_empty());
        let unchanged = fs::read_to_string(mcp_config_path_global()).unwrap();
        assert!(unchanged.contains("[agents]"));
        assert!(!unchanged.contains("mcp_servers"));
    }
}
