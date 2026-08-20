//! 加载 `~/.astro/config.yaml`（或 `ASTRO_MEMORY_DIR`）。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AstroConfig {
    #[serde(default)]
    pub hooks: HashMap<String, String>,
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
            "hooks:\n  PostToolUse: \"echo hi\"\n  AgentEnd: \"true\"\n",
        )
        .unwrap();
        let cfg = load_config(dir.path()).unwrap();
        assert_eq!(cfg.hooks.get("PostToolUse").unwrap(), "echo hi");
        assert_eq!(cfg.hooks.get("AgentEnd").unwrap(), "true");
    }
}
