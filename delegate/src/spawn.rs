//! 由 agent 在启动时注入；tools 的 `delegate` 同步调用并等待结果。

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// 委派子 Agent 角色：叶子不可再派；编排者在深度允许时可再派。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DelegateRole {
    #[default]
    Leaf,
    Orchestrator,
}

impl DelegateRole {
    /// 解析字符串；未知值视为 leaf。
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "orchestrator" => Self::Orchestrator,
            _ => Self::Leaf,
        }
    }
}

/// 单个委派子任务。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegateTaskSpec {
    pub goal: String,
    pub context: String,
    /// 默认 leaf：不可嵌套委派。
    #[serde(default)]
    pub role: DelegateRole,
    /// 可选工具集白名单（如 `terminal`、`file`/`file_ops`、`web`/`web_search`）。
    /// 空/缺省 = 父集减去嵌套剥离后的全部工具。
    #[serde(default)]
    pub toolsets: Option<Vec<String>>,
    /// 子 Agent 最大工具跟随轮次；缺省读配置 `child_max_iterations`（默认 50）。
    #[serde(default)]
    pub max_iterations: Option<usize>,
}

impl DelegateTaskSpec {
    pub fn new(goal: impl Into<String>, context: impl Into<String>) -> Self {
        Self {
            goal: goal.into(),
            context: context.into(),
            role: DelegateRole::Leaf,
            toolsets: None,
            max_iterations: None,
        }
    }
}

/// 同步委派请求（父 Agent 凭据 + 任务列表）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegateRunRequest {
    pub parent_agent_id: String,
    pub parent_session_id: String,
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
    /// 含 primary 的聊天 fallback 链；空则子 Agent 由四字段合成单目标。
    #[serde(default)]
    pub chat_targets: Vec<common::ChatTarget>,
    pub tasks: Vec<DelegateTaskSpec>,
    /// 并行上限（至少 1）。
    pub max_concurrent: usize,
    /// 发起方当前嵌套深度（顶层 0）。
    pub caller_depth: u32,
    /// 允许发起嵌套的最大 caller depth（默认 1）。
    pub max_spawn_depth: u32,
    /// 可选：代码仓根（显式或环境）；缺省由执行器解析。
    #[serde(default)]
    pub project_root: Option<std::path::PathBuf>,
    /// 父 Agent 插件钩子总线（由 `ToolContext` 注入，供 `subagent_start` 使用）；
    /// 跨进程持久化（异步委派恢复）时不可序列化，恢复后为 `None`（观察型钩子静默跳过）。
    #[serde(skip)]
    pub hook_bus: Option<Arc<hooks::PluginHookBus>>,
}

pub type DelegateRunner =
    Arc<dyn Fn(DelegateRunRequest) -> anyhow::Result<String> + Send + Sync + 'static>;
