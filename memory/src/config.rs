//! 从 `{base}/config.yaml` 加载记忆相关配置（`memory:` / `auxiliary:` 段）。
//!
//! 与 hooks 共用同一路径；本模块只反序列化关心的段，忽略其余键。

use std::fs;
use std::path::Path;

use serde::Deserialize;
use tracing::warn;

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

fn default_auto() -> String {
    "auto".to_string()
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
    /// 写入审批开关（P2）。
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

/// 单个辅助模型路由：`auto` 表示跟随当前会话主模型。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AuxiliaryRoute {
    #[serde(default = "default_auto")]
    pub provider: String,
    #[serde(default = "default_auto")]
    pub model: String,
}

impl Default for AuxiliaryRoute {
    fn default() -> Self {
        Self {
            provider: default_auto(),
            model: default_auto(),
        }
    }
}

/// 辅助模型配置（`config.yaml` 的 `auxiliary:` 段）。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, Default)]
pub struct AuxiliaryConfig {
    /// 回合结束后是否自动跑 memory background review（默认关闭，避免意外产生费用）。
    #[serde(default)]
    pub background_review_enabled: bool,
    #[serde(default)]
    pub background_review: AuxiliaryRoute,
    #[serde(default)]
    pub dreaming: AuxiliaryRoute,
}

/// 辅助路由用途。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuxiliaryKind {
    /// 回合后自我改进 review。
    BackgroundReview,
    /// 入梦提炼。
    Dreaming,
}

#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    #[serde(default)]
    memory: Option<MemoryConfig>,
    #[serde(default)]
    auxiliary: Option<AuxiliaryConfig>,
}

fn read_file_config(base: &Path) -> FileConfig {
    let path = base.join("config.yaml");
    if !path.is_file() {
        return FileConfig::default();
    }
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            warn!(
                path = %path.display(),
                error = %e,
                "falling back to config defaults"
            );
            return FileConfig::default();
        }
    };
    match serde_yaml::from_str::<FileConfig>(&text) {
        Ok(file) => file,
        Err(e) => {
            warn!(
                path = %path.display(),
                error = %e,
                "falling back to config defaults"
            );
            FileConfig::default()
        }
    }
}

/// 从 `{base}/config.yaml` 加载记忆配置；文件缺失或无 `memory:` 段时返回默认值。
pub fn load_memory_config(base: &Path) -> MemoryConfig {
    read_file_config(base).memory.unwrap_or_default()
}

/// 从 `{base}/config.yaml` 加载辅助模型配置。
pub fn load_auxiliary_config(base: &Path) -> AuxiliaryConfig {
    read_file_config(base).auxiliary.unwrap_or_default()
}

/// 将辅助路由解析为具体 `(provider, model)`。
///
/// `provider`/`model` 为 `auto`（忽略大小写）或空白时，回退到会话主模型。
pub fn resolve_auxiliary(
    kind: AuxiliaryKind,
    aux: &AuxiliaryConfig,
    session_provider: &str,
    session_model: &str,
) -> (String, String) {
    let route = match kind {
        AuxiliaryKind::BackgroundReview => &aux.background_review,
        AuxiliaryKind::Dreaming => &aux.dreaming,
    };
    let provider = if route.provider.trim().is_empty()
        || route.provider.eq_ignore_ascii_case("auto")
    {
        session_provider.to_string()
    } else {
        route.provider.trim().to_string()
    };
    let model = if route.model.trim().is_empty() || route.model.eq_ignore_ascii_case("auto")
    {
        session_model.to_string()
    } else {
        route.model.trim().to_string()
    };
    (provider, model)
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
        let aux = load_auxiliary_config(dir.path());
        assert_eq!(aux, AuxiliaryConfig::default());
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
auxiliary:
  dreaming:
    provider: openai
    model: gpt-4o-mini
  background_review:
    provider: auto
    model: cheap-review
  background_review_enabled: true
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

        let aux = load_auxiliary_config(dir.path());
        assert!(aux.background_review_enabled);
        assert_eq!(aux.dreaming.provider, "openai");
        assert_eq!(aux.dreaming.model, "gpt-4o-mini");
        assert_eq!(aux.background_review.provider, "auto");
        assert_eq!(aux.background_review.model, "cheap-review");

        let (p, m) = resolve_auxiliary(
            AuxiliaryKind::Dreaming,
            &aux,
            "session-prov",
            "session-model",
        );
        assert_eq!(p, "openai");
        assert_eq!(m, "gpt-4o-mini");

        let (p2, m2) = resolve_auxiliary(
            AuxiliaryKind::BackgroundReview,
            &aux,
            "session-prov",
            "session-model",
        );
        assert_eq!(p2, "session-prov");
        assert_eq!(m2, "cheap-review");
    }
}

