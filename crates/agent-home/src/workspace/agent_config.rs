//! Agent 运行时配置结构体——无 SQLite 依赖，纯 JSON 文件 I/O。

use std::fs;
use std::path::{Path, PathBuf};

use super::paths::agent_config_dir;

/// 单个 Agent 的运行时配置，持久化于 `agents/{id}/config.json`。
///
/// 为 null 的字段表示「继承全局默认」或由上层 Builder 回退；工作区 Markdown 不在此文件。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentRuntimeConfig {
    /// 规范化后的 Agent id（与 `workspace-{id}` 目录名对应）
    pub id: String,
    /// 显示名称（UI / 列表用，可与 IDENTITY.md 的 Name 不同步）
    pub name: String,
    /// 继承哪个 Agent 的配置（通常为 `workspace`）；字段为 null 时用全局默认
    #[serde(default)]
    pub inherit_from: Option<String>,
    /// LLM Provider id；null 时用全局 `models.json` 默认
    #[serde(default)]
    pub provider_id: Option<String>,
    /// 模型名；null 时用 Provider 默认
    #[serde(default)]
    pub model: Option<String>,
    /// 采样温度；缺省由 ProviderConfig / AgentBuilder 回退
    #[serde(default)]
    pub temperature: Option<f32>,
    /// 工具循环最大轮次（对齐 Rig multi_turn）
    #[serde(default)]
    pub max_turns: Option<usize>,
    /// Provider 扩展参数（reasoning / vendor extras），对齐 Rig additional_params
    #[serde(default)]
    pub additional_params: Option<serde_json::Value>,
    /// 为 null 时使用全局 `tools/enabled.json`
    #[serde(default)]
    pub tools_enabled: Option<serde_json::Value>,
    /// ISO 8601 创建时间（本地时区 RFC3339）
    #[serde(default)]
    pub created_at: String,
}

impl AgentRuntimeConfig {
    /// 该 Agent 的 `config.json` 绝对路径
    pub fn path(base: &Path, agent_id: &str) -> PathBuf {
        agent_config_dir(base, agent_id).join("config.json")
    }

    /// 从磁盘加载配置；文件不存在时返回错误
    pub fn load(base: &Path, agent_id: &str) -> anyhow::Result<Self> {
        let path = Self::path(base, agent_id);
        if !path.is_file() {
            anyhow::bail!("Agent 配置不存在: {}", path.display());
        }
        let text = fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&text)?)
    }

    /// 将配置写回 `agents/{id}/config.json`（自动创建目录）
    pub fn save(&self, base: &Path) -> anyhow::Result<()> {
        let dir = agent_config_dir(base, &self.id);
        fs::create_dir_all(&dir)?;
        let path = dir.join("config.json");
        fs::write(&path, format!("{}\n", serde_json::to_string_pretty(self)?))?;
        Ok(())
    }
}
