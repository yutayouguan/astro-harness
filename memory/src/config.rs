//! 从 `{base}/config.yaml` 加载记忆相关配置（`memory:` 段）。
//!
//! 与 hooks 共用同一路径；本模块只反序列化 `memory:`，忽略其余键。

use std::fs;
use std::path::Path;

use serde::Deserialize;

fn default_true() -> bool {
    true
}

fn default_mem_limit() -> usize {
    2200
}

fn default_user_limit() -> usize {
    1375
}

fn default_daily_max() -> usize {
    1024
}

/// 记忆子系统配置（`config.yaml` 的 `memory:` 段）。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct MemoryConfig {
    /// 是否向 prompt 注入长期记忆快照。
    #[serde(default = "default_true")]
    pub memory_enabled: bool,
    /// 是否向 prompt 注入用户档案快照。
    #[serde(default = "default_true")]
    pub user_profile_enabled: bool,
    /// `MEMORY.md` 字符上限。
    #[serde(default = "default_mem_limit")]
    pub memory_char_limit: usize,
    /// `USER.md` 字符上限。
    #[serde(default = "default_user_limit")]
    pub user_char_limit: usize,
    /// 写入审批开关（P2；P1 仅解析保留）。
    #[serde(default)]
    pub write_approval: bool,
    /// 注入 prompt 时今日日记的最大字符数。
    #[serde(default = "default_daily_max")]
    pub daily_prompt_max_chars: usize,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            memory_enabled: true,
            user_profile_enabled: true,
            memory_char_limit: 2200,
            user_char_limit: 1375,
            write_approval: false,
            daily_prompt_max_chars: 1024,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    #[serde(default)]
    memory: Option<MemoryConfig>,
}

/// 从 `{base}/config.yaml` 加载记忆配置；文件缺失或无 `memory:` 段时返回默认值。
pub fn load_memory_config(base: &Path) -> MemoryConfig {
    let path = base.join("config.yaml");
    if !path.is_file() {
        return MemoryConfig::default();
    }
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return MemoryConfig::default(),
    };
    match serde_yaml::from_str::<FileConfig>(&text) {
        Ok(file) => file.memory.unwrap_or_default(),
        Err(_) => MemoryConfig::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = load_memory_config(dir.path());
        assert_eq!(cfg, MemoryConfig::default());
        assert!(cfg.memory_enabled);
        assert!(cfg.user_profile_enabled);
        assert_eq!(cfg.memory_char_limit, 2200);
        assert_eq!(cfg.user_char_limit, 1375);
        assert!(!cfg.write_approval);
        assert_eq!(cfg.daily_prompt_max_chars, 1024);
    }

    #[test]
    fn yaml_overrides_and_ignores_hooks() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.yaml"),
            r#"
hooks:
  post_tool_call: "echo hi"
memory:
  memory_enabled: false
  memory_char_limit: 100
  user_char_limit: 50
  write_approval: true
  daily_prompt_max_chars: 32
  unknown_key: ignored
"#,
        )
        .unwrap();
        let cfg = load_memory_config(dir.path());
        assert!(!cfg.memory_enabled);
        assert!(cfg.user_profile_enabled); // default
        assert_eq!(cfg.memory_char_limit, 100);
        assert_eq!(cfg.user_char_limit, 50);
        assert!(cfg.write_approval);
        assert_eq!(cfg.daily_prompt_max_chars, 32);
    }
}
