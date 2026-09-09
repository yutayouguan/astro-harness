//! Agent defaults in config.toml [desktop.agents.<id>]. No JSON fallback.

use std::path::{Path, PathBuf};

/// 单个 Agent 的运行时配置，持久化于 `config.toml [desktop.agents.<id>]`。
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
    /// 为 null 时使用全局 `desktop.tools`
    #[serde(default)]
    pub tools_enabled: Option<serde_json::Value>,
    /// ISO 8601 创建时间（本地时区 RFC3339）
    #[serde(default)]
    pub created_at: String,
}

impl AgentRuntimeConfig {
    /// Shared configuration file; existence of this file does not imply an agent section exists.
    pub fn path(base: &Path, _agent_id: &str) -> PathBuf {
        crate::settings::path(base)
    }

    pub fn load_optional(base: &Path, agent_id: &str) -> anyhow::Result<Option<Self>> {
        let id = crate::settings::migration::canonical_agent_id(agent_id);
        crate::settings::read::<serde_json::Value>(base, &["desktop", "agents", &id])?
            .map(crate::settings::agent_from_wire)
            .transpose()
    }

    /// 从磁盘加载配置；文件不存在时返回错误
    pub fn load(base: &Path, agent_id: &str) -> anyhow::Result<Self> {
        Self::load_optional(base, agent_id)?.ok_or_else(|| {
            anyhow::anyhow!("Agent configuration section does not exist: {agent_id}")
        })
    }

    pub(crate) fn update_name(base: &Path, agent_id: &str, name: &str) -> anyhow::Result<()> {
        let id = crate::settings::migration::canonical_agent_id(agent_id);
        crate::settings::update(base, |doc| {
            let mut value =
                crate::settings::get::<serde_json::Value>(doc, &["desktop", "agents", &id])?
                    .ok_or_else(|| anyhow::anyhow!("agent configuration is missing"))?;
            value["name"] = serde_json::Value::String(name.into());
            crate::settings::put(doc, &["desktop", "agents", &id], &value)
        })
    }

    /// 将配置写回 `config.toml [desktop.agents.<id>]`（自动创建目录）
    pub fn save(&self, base: &Path) -> anyhow::Result<()> {
        let mut config = self.clone();
        config.id = crate::settings::migration::canonical_agent_id(&self.id);
        crate::settings::write(
            base,
            &["desktop", "agents", &config.id],
            &crate::settings::agent_to_wire(&config)?,
        )
    }
}
