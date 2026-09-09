//! 加载 `~/.astro/config.toml`（或 `ASTRO_MEMORY_DIR`）。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AstroConfig {
    #[serde(default, rename = "shell_hooks")]
    pub hooks: HashMap<String, String>,
}

/// 数据根目录：`$ASTRO_MEMORY_DIR` 或 `~/.astro`。
pub fn default_astro_root() -> PathBuf {
    home::default_memory_dir()
}

pub fn config_path(root: &Path) -> PathBuf {
    home::config_path(root)
}

pub fn load_config(root: &Path) -> anyhow::Result<AstroConfig> {
    let path = config_path(root);
    if !path.is_file() {
        return Ok(AstroConfig::default());
    }
    let text = fs::read_to_string(&path)?;
    let value: toml::Value = text.parse()?;
    anyhow::ensure!(
        !value
            .get("hooks")
            .and_then(toml::Value::as_table)
            .is_some_and(|hooks| hooks.values().any(toml::Value::is_str)),
        "legacy string hooks must move to [shell_hooks]"
    );
    let cfg: AstroConfig = toml::from_str(&text)?;
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
            dir.path().join("config.toml"),
            r#""shell_hooks" = { "PostToolUse" = "echo hi", "AgentEnd" = "true" }
"#,
        )
        .unwrap();
        let cfg = load_config(dir.path()).unwrap();
        assert_eq!(cfg.hooks.get("PostToolUse").unwrap(), "echo hi");
        assert_eq!(cfg.hooks.get("AgentEnd").unwrap(), "true");
    }
}
