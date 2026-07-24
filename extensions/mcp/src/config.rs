//! 读写 `mcp.json`（全局或按 Agent）。
//!
//! 负责传输类型、发现工具合并，以及启用状态持久化。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use home::{
    active_agent_id, agent_config_dir, default_memory_dir, ensure_default_workspace_dirs,
    DEFAULT_AGENT_ID,
};

use crate::names::sanitize_server_id;

/// MCP 传输方式。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum McpTransportType {
    /// 本地子进程 stdio。
    Stdio,
    /// Server-Sent Events。
    Sse,
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
            Self::Sse => "sse",
            Self::StreamableHttp => "streamableHttp",
        }
    }

    /// 解析配置字符串；未知值回退为 [`Stdio`](Self::Stdio)。
    pub fn parse(s: &str) -> Self {
        match s {
            "sse" => Self::Sse,
            "streamableHttp" | "streamable_http" => Self::StreamableHttp,
            _ => Self::Stdio,
        }
    }
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
    /// stdio | sse | streamableHttp
    #[serde(default, alias = "transport", deserialize_with = "de_type")]
    pub r#type: McpTransportType,
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
    /// 工具调用超时（秒）；缺失默认 300s。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_timeout_secs: Option<u64>,
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
    match s.as_str() {
        "stdio" => Ok(McpTransportType::Stdio),
        "sse" => Ok(McpTransportType::Sse),
        "streamableHttp" | "streamable_http" => Ok(McpTransportType::StreamableHttp),
        other => Err(serde::de::Error::unknown_variant(
            other,
            &["stdio", "sse", "streamableHttp"],
        )),
    }
}

impl McpServerConfig {
    /// 工具是否启用（未登记视为启用）。
    pub fn is_tool_enabled(&self, tool_name: &str) -> bool {
        self.tools.get(tool_name).copied().unwrap_or(true)
    }

    /// 连接身份指纹（不含 tools/discovered，避免开关变化触发重连）
    pub fn connection_fingerprint(&self) -> String {
        format!(
            "{}|{}|{}|{:?}|{:?}|{}|{:?}",
            sanitize_server_id(&self.id),
            self.r#type.as_str(),
            self.command,
            self.args,
            self.env,
            self.url,
            self.headers
        )
    }
}

/// 磁盘上的 mcp.json 结构。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct McpFile {
    #[serde(default)]
    servers: Vec<McpServerConfig>,
}

/// 全局 `~/.astro/mcp.json` 路径。
pub fn mcp_path_global() -> PathBuf {
    default_memory_dir().join("mcp.json")
}

/// 指定 Agent 的 `mcp.json` 路径（落在 agent 配置目录）。
pub fn mcp_path_for_agent(agent_id: Option<&str>) -> PathBuf {
    let base = default_memory_dir();
    match agent_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => {
            let id = if id == "default" {
                DEFAULT_AGENT_ID
            } else {
                id
            };
            agent_config_dir(&base, id).join("mcp.json")
        }
        None => mcp_path_global(),
    }
}

/// 读取 mcp.json；缺失或空文件返回默认。
fn read_file(path: &Path) -> anyhow::Result<McpFile> {
    if !path.exists() {
        return Ok(McpFile::default());
    }
    let raw = fs::read_to_string(path)?;
    if raw.trim().is_empty() {
        return Ok(McpFile::default());
    }
    Ok(serde_json::from_str(&raw)?)
}

/// 原子写入 mcp.json（先写临时文件再 rename）。
fn write_file(path: &Path, file: &McpFile) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(file)?)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// 读取 Agent MCP 配置；专属文件不存在时回退全局
pub fn load_mcp_servers(agent_id: Option<&str>) -> anyhow::Result<Vec<McpServerConfig>> {
    let _ = ensure_default_workspace_dirs();
    let path = mcp_path_for_agent(agent_id);
    let file = if path.exists() {
        read_file(&path)?
    } else if agent_id.is_some() {
        read_file(&mcp_path_global())?
    } else {
        read_file(&path)?
    };
    Ok(file
        .servers
        .into_iter()
        .map(|mut s| {
            s.id = sanitize_server_id(&s.id);
            s
        })
        .collect())
}

/// 将服务器列表写回当前 Agent（或全局）的 `mcp.json`。
pub fn save_mcp_servers(agent_id: Option<&str>, servers: &[McpServerConfig]) -> anyhow::Result<()> {
    ensure_default_workspace_dirs()?;
    let id = agent_id.map(str::trim).filter(|s| !s.is_empty()).map(|s| {
        if s == "default" {
            DEFAULT_AGENT_ID.to_string()
        } else {
            s.to_string()
        }
    });
    let path = mcp_path_for_agent(id.as_deref());
    let file = McpFile {
        servers: servers.to_vec(),
    };
    write_file(&path, &file)?;
    if id.as_deref() == Some(DEFAULT_AGENT_ID) {
        write_file(&mcp_path_global(), &file)?;
    }
    Ok(())
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
    if updates.is_empty() {
        return Ok(());
    }
    let mut configs = load_mcp_servers(agent_id)?;
    for (sid, discovered) in updates {
        let sid = sanitize_server_id(sid);
        if let Some(slot) = configs
            .iter_mut()
            .find(|c| sanitize_server_id(&c.id) == sid)
        {
            merge_discovered(slot, discovered.clone());
        }
    }
    save_mcp_servers(agent_id, &configs)
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
            url: String::new(),
            headers: HashMap::new(),
            enabled: true,
            tools: HashMap::from([("a".into(), false)]),
            discovered: vec![],
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
            url: String::new(),
            headers: HashMap::new(),
            enabled: true,
            tools: HashMap::from([("a".into(), false)]),
            discovered: vec![],
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
}
