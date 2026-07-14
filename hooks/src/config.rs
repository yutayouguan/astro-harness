//! 加载 `~/.astro/config.yaml`（或 `ASTRO_MEMORY_DIR`）。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// 委派 / 嵌套子 Agent 配置（与 `config.yaml` 的 `delegation:` 段对应）。
#[derive(Debug, Clone, Deserialize)]
pub struct DelegationConfig {
    /// 允许发起嵌套的最大 caller depth（顶层 0；默认 1 = 仅一层叶子）。
    #[serde(default = "default_max_spawn_depth")]
    pub max_spawn_depth: u32,
    /// 并行子任务上限。
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_children: usize,
    /// 子 Agent 默认最大迭代轮次。
    #[serde(default = "default_child_max_iterations")]
    pub child_max_iterations: usize,
    /// 委派子任务是否自动建 git worktree。
    #[serde(default = "default_true")]
    pub worktree: bool,
    /// 是否允许 `role=orchestrator`（全局开关）。
    #[serde(default = "default_true")]
    pub orchestrator_enabled: bool,
}

fn default_max_spawn_depth() -> u32 {
    1
}
fn default_max_concurrent() -> usize {
    3
}
/// 子 Agent 独立迭代预算默认值（对齐 Hermes `delegation.max_iterations`）。
fn default_child_max_iterations() -> usize {
    50 // 与 agent::DEFAULT_CHILD_MAX_ITERATIONS 保持一致
}
fn default_true() -> bool {
    true
}

impl Default for DelegationConfig {
    fn default() -> Self {
        Self {
            max_spawn_depth: default_max_spawn_depth(),
            max_concurrent_children: default_max_concurrent(),
            child_max_iterations: default_child_max_iterations(),
            worktree: true,
            orchestrator_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AstroConfig {
    #[serde(default)]
    pub hooks: HashMap<String, String>,
    #[serde(default)]
    pub delegation: DelegationConfig,
}

/// 数据根目录：`$ASTRO_MEMORY_DIR` 或 `~/.astro`。
pub fn default_astro_root() -> PathBuf {
    if let Ok(dir) = std::env::var("ASTRO_MEMORY_DIR") {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".astro")
}

pub fn config_path(root: &Path) -> PathBuf {
    root.join("config.yaml")
}

pub fn load_config(root: &Path) -> anyhow::Result<AstroConfig> {
    let path = config_path(root);
    if !path.is_file() {
        return Ok(AstroConfig::default());
    }
    let text = fs::read_to_string(&path)?;
    let cfg: AstroConfig = serde_yaml::from_str(&text)?;
    Ok(cfg)
}

/// 从默认根目录加载配置（失败则返回默认值）。
pub fn load_config_or_default() -> AstroConfig {
    load_config(&default_astro_root()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hooks_block() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.yaml"),
            "hooks:\n  post_tool_call: \"echo hi\"\n  agent:end: \"true\"\n",
        )
        .unwrap();
        let cfg = load_config(dir.path()).unwrap();
        assert_eq!(cfg.hooks.get("post_tool_call").unwrap(), "echo hi");
        assert_eq!(cfg.hooks.get("agent:end").unwrap(), "true");
        assert_eq!(cfg.delegation.max_spawn_depth, 1);
        assert_eq!(cfg.delegation.child_max_iterations, 50);
        assert!(cfg.delegation.worktree);
    }

    #[test]
    fn parse_delegation_block() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.yaml"),
            "delegation:\n  max_spawn_depth: 2\n  child_max_iterations: 30\n  worktree: false\n  orchestrator_enabled: false\n",
        )
        .unwrap();
        let cfg = load_config(dir.path()).unwrap();
        assert_eq!(cfg.delegation.max_spawn_depth, 2);
        assert_eq!(cfg.delegation.child_max_iterations, 30);
        assert!(!cfg.delegation.worktree);
        assert!(!cfg.delegation.orchestrator_enabled);
    }
}
