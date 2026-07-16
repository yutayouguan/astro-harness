//! 从 `{base}/config.yaml` 加载记忆相关配置（`memory:` / `auxiliary:` 段）。
//!
//! 与 hooks 共用同一路径；本模块只反序列化关心的段，忽略其余键。
//! 写回开关时用 [`serde_yaml::Value`] 合并，保留 hooks 等其余键。

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
    /// 收到 live 记忆更新 SessionEvent 时是否自动 `refresh_memory`（P3，默认开）。
    #[serde(default = "default_true")]
    pub auto_refresh_on_update: bool,
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
            auto_refresh_on_update: true,
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
    pub title_generation: AuxiliaryRoute,
    #[serde(default)]
    pub compaction: AuxiliaryRoute,
    #[serde(default)]
    pub smart_approval: AuxiliaryRoute,
    #[serde(default)]
    pub background_review: AuxiliaryRoute,
    #[serde(default)]
    pub dreaming: AuxiliaryRoute,
}

/// 辅助路由用途。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuxiliaryKind {
    /// 生成标题。
    TitleGeneration,
    /// 压缩上下文。
    Compaction,
    /// 智能审批。
    SmartApproval,
    /// 回合后自我改进 review。
    BackgroundReview,
    /// 入梦提炼。
    Dreaming,
}

impl AuxiliaryKind {
    pub const ALL: [Self; 5] = [
        Self::TitleGeneration,
        Self::Compaction,
        Self::SmartApproval,
        Self::Dreaming,
        Self::BackgroundReview,
    ];

    pub const fn config_key(self) -> &'static str {
        match self {
            Self::TitleGeneration => "title_generation",
            Self::Compaction => "compaction",
            Self::SmartApproval => "smart_approval",
            Self::Dreaming => "dreaming",
            Self::BackgroundReview => "background_review",
        }
    }
}

impl AuxiliaryConfig {
    pub fn route(&self, kind: AuxiliaryKind) -> &AuxiliaryRoute {
        match kind {
            AuxiliaryKind::TitleGeneration => &self.title_generation,
            AuxiliaryKind::Compaction => &self.compaction,
            AuxiliaryKind::SmartApproval => &self.smart_approval,
            AuxiliaryKind::Dreaming => &self.dreaming,
            AuxiliaryKind::BackgroundReview => &self.background_review,
        }
    }
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

fn config_yaml_path(base: &Path) -> std::path::PathBuf {
    base.join("config.yaml")
}

/// 读入已有 `config.yaml` 为 Value；缺失则空 Mapping。
fn load_yaml_root(base: &Path) -> anyhow::Result<serde_yaml::Value> {
    let path = config_yaml_path(base);
    if !path.is_file() {
        return Ok(serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
    }
    let text = fs::read_to_string(&path)?;
    if text.trim().is_empty() {
        return Ok(serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
    }
    Ok(serde_yaml::from_str(&text)?)
}

/// 原子写回 `config.yaml`。
fn save_yaml_root(base: &Path, root: &serde_yaml::Value) -> anyhow::Result<()> {
    fs::create_dir_all(base)?;
    let path = config_yaml_path(base);
    let tmp = path.with_extension("yaml.tmp");
    let text = serde_yaml::to_string(root)?;
    fs::write(&tmp, text)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// 确保 `root[seg…]` 为 Mapping，返回最内层可变 Mapping。
fn ensure_mapping_path<'a>(
    root: &'a mut serde_yaml::Value,
    segs: &[&str],
) -> anyhow::Result<&'a mut serde_yaml::Mapping> {
    if !root.is_mapping() {
        *root = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
    }
    let mut cur = root
        .as_mapping_mut()
        .ok_or_else(|| anyhow::anyhow!("config.yaml root must be a mapping"))?;
    for &seg in segs {
        let key = serde_yaml::Value::String(seg.to_string());
        if !cur.contains_key(&key) || !cur.get(&key).is_some_and(|v| v.is_mapping()) {
            cur.insert(key.clone(), serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
        }
        cur = cur
            .get_mut(&key)
            .and_then(|v| v.as_mapping_mut())
            .ok_or_else(|| anyhow::anyhow!("config.yaml segment `{seg}` is not a mapping"))?;
    }
    Ok(cur)
}

/// 设置嵌套布尔键（如 `memory.write_approval`），保留文件中其它键。
fn set_nested_bool(base: &Path, parents: &[&str], key: &str, value: bool) -> anyhow::Result<()> {
    let mut root = load_yaml_root(base)?;
    let map = ensure_mapping_path(&mut root, parents)?;
    map.insert(
        serde_yaml::Value::String(key.to_string()),
        serde_yaml::Value::Bool(value),
    );
    save_yaml_root(base, &root)
}

fn route_to_value(route: &AuxiliaryRoute) -> serde_yaml::Value {
    let mut map = serde_yaml::Mapping::new();
    map.insert(
        serde_yaml::Value::String("provider".to_string()),
        serde_yaml::Value::String(route.provider.clone()),
    );
    map.insert(
        serde_yaml::Value::String("model".to_string()),
        serde_yaml::Value::String(route.model.clone()),
    );
    serde_yaml::Value::Mapping(map)
}

fn set_nested_route(base: &Path, parents: &[&str], key: &str, route: &AuxiliaryRoute) -> anyhow::Result<()> {
    let mut root = load_yaml_root(base)?;
    let map = ensure_mapping_path(&mut root, parents)?;
    map.insert(
        serde_yaml::Value::String(key.to_string()),
        route_to_value(route),
    );
    save_yaml_root(base, &root)
}

/// 设置 `memory.write_approval` 并返回最新配置。
pub fn set_write_approval(base: &Path, enabled: bool) -> anyhow::Result<MemoryConfig> {
    set_nested_bool(base, &["memory"], "write_approval", enabled)?;
    Ok(load_memory_config(base))
}

/// 设置 `memory.auto_refresh_on_update` 并返回最新配置。
pub fn set_auto_refresh_on_update(base: &Path, enabled: bool) -> anyhow::Result<MemoryConfig> {
    set_nested_bool(base, &["memory"], "auto_refresh_on_update", enabled)?;
    Ok(load_memory_config(base))
}

/// 设置 `auxiliary.background_review_enabled` 并返回最新辅助配置。
pub fn set_background_review_enabled(
    base: &Path,
    enabled: bool,
) -> anyhow::Result<AuxiliaryConfig> {
    set_nested_bool(base, &["auxiliary"], "background_review_enabled", enabled)?;
    Ok(load_auxiliary_config(base))
}

/// 设置单个辅助路由并返回最新辅助配置。
pub fn set_auxiliary_route(
    base: &Path,
    kind: AuxiliaryKind,
    route: AuxiliaryRoute,
) -> anyhow::Result<AuxiliaryConfig> {
    set_nested_route(base, &["auxiliary"], kind.config_key(), &route)?;
    Ok(load_auxiliary_config(base))
}

/// 将所有辅助路由重置为 `auto/auto` 并返回最新辅助配置。
pub fn reset_all_auxiliary_routes(base: &Path) -> anyhow::Result<AuxiliaryConfig> {
    let mut root = load_yaml_root(base)?;
    let map = ensure_mapping_path(&mut root, &["auxiliary"])?;
    let default_route = AuxiliaryRoute::default();
    for kind in AuxiliaryKind::ALL {
        map.insert(
            serde_yaml::Value::String(kind.config_key().to_string()),
            route_to_value(&default_route),
        );
    }
    save_yaml_root(base, &root)?;
    Ok(load_auxiliary_config(base))
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
    let route = aux.route(kind);
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
        assert!(cfg.auto_refresh_on_update);
        assert_eq!(cfg.daily_prompt_max_chars, 1024);
        let aux = load_auxiliary_config(dir.path());
        assert_eq!(aux, AuxiliaryConfig::default());
    }

    #[test]
    fn auto_refresh_defaults_true() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_memory_config(dir.path()).auto_refresh_on_update);
    }

    #[test]
    fn set_auto_refresh_false() {
        let dir = tempfile::tempdir().unwrap();
        set_auto_refresh_on_update(dir.path(), false).unwrap();
        assert!(!load_memory_config(dir.path()).auto_refresh_on_update);
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

    #[test]
    fn set_write_approval_preserves_other_keys() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.yaml"),
            "hooks:\n  post_tool_call: \"echo hi\"\nmemory:\n  memory_char_limit: 99\n",
        )
        .unwrap();
        let cfg = set_write_approval(dir.path(), true).unwrap();
        assert!(cfg.write_approval);
        assert_eq!(cfg.memory_char_limit, 99);
        let text = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(text.contains("post_tool_call"));
        assert!(text.contains("write_approval: true"));
        assert!(text.contains("99"));
    }

    #[test]
    fn set_background_review_enabled_creates_auxiliary() {
        let dir = tempfile::tempdir().unwrap();
        let aux = set_background_review_enabled(dir.path(), true).unwrap();
        assert!(aux.background_review_enabled);
        let text = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(text.contains("background_review_enabled: true"));
    }

    #[test]
    fn auxiliary_defaults_cover_all_five_tasks() {
        let cfg = AuxiliaryConfig::default();
        for kind in AuxiliaryKind::ALL {
            assert_eq!(cfg.route(kind), &AuxiliaryRoute::default());
        }
    }

    #[test]
    fn set_route_preserves_unrelated_yaml() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.yaml"),
            "hooks:\n  enabled: true\nmemory:\n  write_approval: true\n",
        )
        .unwrap();
        set_auxiliary_route(
            dir.path(),
            AuxiliaryKind::Compaction,
            AuxiliaryRoute {
                provider: "provider-1".into(),
                model: "small".into(),
            },
        )
        .unwrap();
        let text = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(text.contains("enabled: true"));
        assert!(text.contains("write_approval: true"));
        assert_eq!(load_auxiliary_config(dir.path()).compaction.model, "small");
    }

    #[test]
    fn reset_all_routes_keeps_background_review_enabled() {
        let dir = tempfile::tempdir().unwrap();
        set_background_review_enabled(dir.path(), true).unwrap();
        set_auxiliary_route(
            dir.path(),
            AuxiliaryKind::Dreaming,
            AuxiliaryRoute {
                provider: "p".into(),
                model: "m".into(),
            },
        )
        .unwrap();
        let cfg = reset_all_auxiliary_routes(dir.path()).unwrap();
        assert!(cfg.background_review_enabled);
        assert_eq!(cfg.dreaming, AuxiliaryRoute::default());
    }
}

