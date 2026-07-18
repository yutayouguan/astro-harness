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

fn default_unused_days() -> u32 {
    30
}

fn default_complex_threshold() -> usize {
    5
}

/// 运行时学习闭环配置（`config.yaml` 的 `learning:` 段）。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct LearningConfig {
    /// 复杂任务后是否在下一轮注入 nudge。
    #[serde(default = "default_true")]
    pub nudge_enabled: bool,
    /// 上一轮工具次数 ≥ 该值视为复杂任务。
    #[serde(default = "default_complex_threshold")]
    pub complex_task_tool_threshold: usize,
    /// `skills curate` 闲置天数阈值。
    #[serde(default = "default_unused_days")]
    pub unused_skill_days: u32,
}

impl Default for LearningConfig {
    fn default() -> Self {
        Self {
            nudge_enabled: true,
            complex_task_tool_threshold: 5,
            unused_skill_days: 30,
        }
    }
}

fn default_approval_mode() -> String {
    "smart".to_string()
}

/// 危险命令审批配置（`config.yaml` 的 `approvals:` 段）。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ApprovalsConfig {
    /// 审批模式：`smart`（默认）| `manual` | `off`。仅作用于 Ask 级；hardline 永远拦。
    #[serde(default = "default_approval_mode")]
    pub mode: String,
    /// 用户永久放行的命令白名单（精确或 glob，含 `* ? [`）。
    #[serde(default)]
    pub command_allowlist: Vec<String>,
}

impl Default for ApprovalsConfig {
    fn default() -> Self {
        Self {
            mode: default_approval_mode(),
            command_allowlist: Vec::new(),
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

fn default_max_skill_bytes() -> usize {
    15_360
}

fn default_min_judge_score() -> f32 {
    0.6
}

/// 离线进化门禁（`config.yaml` 的 `evolution.gates` 段）。
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct EvolutionGates {
    /// 候选变体须通过测试。
    #[serde(default = "default_true")]
    pub run_tests: bool,
    /// Skill 体积上限（字节，默认 ~15KB）。
    #[serde(default = "default_max_skill_bytes")]
    pub max_skill_bytes: usize,
    /// 只允许开 PR，禁止直接落库。
    #[serde(default = "default_true")]
    pub require_pr: bool,
    /// judge 最低分（0–1）；`<= 0` 表示关闭 judge 评审。
    #[serde(default = "default_min_judge_score")]
    pub min_judge_score: f32,
}

impl Default for EvolutionGates {
    fn default() -> Self {
        Self {
            run_tests: true,
            max_skill_bytes: 15_360,
            require_pr: true,
            min_judge_score: 0.6,
        }
    }
}

/// 离线进化配置（`config.yaml` 的 `evolution:` 段）。
///
/// 与 `auxiliary`（在线便宜辅助）分离：进化为离线批量、可接受慢与贵；
/// `reflection` 应显式指向强模型，`judge` 可省或走中等模型。
fn default_generations() -> u32 {
    2
}

fn default_variants() -> u32 {
    3
}

/// GEPA-lite 遗传搜索参数（`config.yaml` 的 `evolution.search` 段）。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct EvolutionSearch {
    /// 迭代代数。
    #[serde(default = "default_generations")]
    pub generations: u32,
    /// 每代每目标变体数。
    #[serde(default = "default_variants")]
    pub variants: u32,
    /// 是否启用交叉算子（对 Pareto 前沿 top-2 融合出子代）。
    #[serde(default = "default_true")]
    pub crossover: bool,
}

impl Default for EvolutionSearch {
    fn default() -> Self {
        Self {
            generations: 2,
            variants: 3,
            crossover: true,
        }
    }
}

fn default_dspy_timeout() -> u64 {
    600
}

fn default_auto_cooldown_secs() -> u64 {
    3_600
}

fn default_auto_min_new_decisions() -> usize {
    3
}

fn default_auto_max_runs_per_day() -> u32 {
    3
}

/// 自动触发进化（`config.yaml` 的 `evolution.auto` 段）。
///
/// 默认关闭；开启后在 Chat Done 时尝试跑一次便宜的单轮 reflect，
/// 产物只入待审提案，绝不自动写入技能。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct EvolutionAuto {
    /// 是否启用自动触发（仍需 `evolution.enabled`）。
    #[serde(default)]
    pub enabled: bool,
    /// 两次自动运行的最小间隔（秒）。
    #[serde(default = "default_auto_cooldown_secs")]
    pub cooldown_secs: u64,
    /// 距上次成功触发后，至少新增多少条 DecisionLog 才再跑。
    #[serde(default = "default_auto_min_new_decisions")]
    pub min_new_decisions: usize,
    /// 每个 UTC 自然日最多自动运行次数。
    #[serde(default = "default_auto_max_runs_per_day")]
    pub max_runs_per_day: u32,
}

impl Default for EvolutionAuto {
    fn default() -> Self {
        Self {
            enabled: false,
            cooldown_secs: 3_600,
            min_new_decisions: 3,
            max_runs_per_day: 3,
        }
    }
}

/// 外部 Python DSPy 引擎对接配置（`config.yaml` 的 `evolution.dspy` 段）。
///
/// 默认关闭；需用户自备 Python + 依赖（见 `evolution-dspy/`）。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct EvolutionDspy {
    /// 是否启用 DSPy 对接。
    #[serde(default)]
    pub enabled: bool,
    /// Python 可执行路径（空 = 运行时按 venv/系统 python3 解析）。
    #[serde(default)]
    pub python_bin: String,
    /// evolution-dspy 项目路径（空 = 运行时按 resource_dir/仓库解析）。
    #[serde(default)]
    pub project_path: String,
    /// 子进程超时秒数。
    #[serde(default = "default_dspy_timeout")]
    pub timeout_secs: u64,
}

impl Default for EvolutionDspy {
    fn default() -> Self {
        Self {
            enabled: false,
            python_bin: String::new(),
            project_path: String::new(),
            timeout_secs: 600,
        }
    }
}

/// 离线进化配置（`config.yaml` 的 `evolution:` 段）。
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
pub struct EvolutionConfig {
    /// 离线进化总开关（默认关）。
    #[serde(default)]
    pub enabled: bool,
    /// 反思/变异路由：读 trace 诊断失败、提出改写。建议强模型。
    #[serde(default)]
    pub reflection: AuxiliaryRoute,
    /// 评测路由：对候选判分（可省或中等模型）。
    #[serde(default)]
    pub judge: AuxiliaryRoute,
    /// 门禁。
    #[serde(default)]
    pub gates: EvolutionGates,
    /// GEPA-lite 遗传搜索参数。
    #[serde(default)]
    pub search: EvolutionSearch,
    /// 外部 Python DSPy 对接。
    #[serde(default)]
    pub dspy: EvolutionDspy,
    /// 自动触发（默认关 + 成本护栏）。
    #[serde(default)]
    pub auto: EvolutionAuto,
}

/// 进化路由用途。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvolutionRouteKind {
    /// 反思/变异。
    Reflection,
    /// 评测。
    Judge,
}

impl EvolutionRouteKind {
    pub const ALL: [Self; 2] = [Self::Reflection, Self::Judge];

    pub const fn config_key(self) -> &'static str {
        match self {
            Self::Reflection => "reflection",
            Self::Judge => "judge",
        }
    }
}

impl EvolutionConfig {
    pub fn route(&self, kind: EvolutionRouteKind) -> &AuxiliaryRoute {
        match kind {
            EvolutionRouteKind::Reflection => &self.reflection,
            EvolutionRouteKind::Judge => &self.judge,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    #[serde(default)]
    memory: Option<MemoryConfig>,
    #[serde(default)]
    auxiliary: Option<AuxiliaryConfig>,
    #[serde(default)]
    learning: Option<LearningConfig>,
    #[serde(default)]
    evolution: Option<EvolutionConfig>,
    #[serde(default)]
    approvals: Option<ApprovalsConfig>,
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

/// 从 `{base}/config.yaml` 加载学习闭环配置。
pub fn load_learning_config(base: &Path) -> LearningConfig {
    read_file_config(base).learning.unwrap_or_default()
}

/// 从 `{base}/config.yaml` 加载离线进化配置。
pub fn load_evolution_config(base: &Path) -> EvolutionConfig {
    read_file_config(base).evolution.unwrap_or_default()
}

/// 从 `{base}/config.yaml` 加载危险命令审批配置。
pub fn load_approvals_config(base: &Path) -> ApprovalsConfig {
    read_file_config(base).approvals.unwrap_or_default()
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
            cur.insert(
                key.clone(),
                serde_yaml::Value::Mapping(serde_yaml::Mapping::new()),
            );
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

fn set_nested_string(base: &Path, parents: &[&str], key: &str, value: &str) -> anyhow::Result<()> {
    let mut root = load_yaml_root(base)?;
    let map = ensure_mapping_path(&mut root, parents)?;
    map.insert(
        serde_yaml::Value::String(key.to_string()),
        serde_yaml::Value::String(value.to_string()),
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

fn set_nested_route(
    base: &Path,
    parents: &[&str],
    key: &str,
    route: &AuxiliaryRoute,
) -> anyhow::Result<()> {
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

/// 设置 `approvals.mode`（smart|manual|off）并返回最新审批配置。
pub fn set_approval_mode(base: &Path, mode: &str) -> anyhow::Result<ApprovalsConfig> {
    set_nested_string(base, &["approvals"], "mode", mode)?;
    Ok(load_approvals_config(base))
}

/// 向 `approvals.command_allowlist` 追加一条（去重，保留其余键），返回最新审批配置。
pub fn add_command_to_allowlist(base: &Path, entry: &str) -> anyhow::Result<ApprovalsConfig> {
    let entry = entry.trim();
    if entry.is_empty() {
        return Ok(load_approvals_config(base));
    }
    let mut root = load_yaml_root(base)?;
    let map = ensure_mapping_path(&mut root, &["approvals"])?;
    let key = serde_yaml::Value::String("command_allowlist".to_string());
    let list = match map.get_mut(&key).and_then(|v| v.as_sequence_mut()) {
        Some(seq) => seq,
        None => {
            map.insert(key.clone(), serde_yaml::Value::Sequence(Vec::new()));
            map.get_mut(&key).unwrap().as_sequence_mut().unwrap()
        }
    };
    let exists = list
        .iter()
        .any(|v| v.as_str().map(|s| s == entry).unwrap_or(false));
    if !exists {
        list.push(serde_yaml::Value::String(entry.to_string()));
        save_yaml_root(base, &root)?;
    }
    Ok(load_approvals_config(base))
}

/// 从 `approvals.command_allowlist` 移除一条（按精确文本），返回最新审批配置。
pub fn remove_command_from_allowlist(base: &Path, entry: &str) -> anyhow::Result<ApprovalsConfig> {
    let entry = entry.trim();
    let mut root = load_yaml_root(base)?;
    let map = ensure_mapping_path(&mut root, &["approvals"])?;
    let key = serde_yaml::Value::String("command_allowlist".to_string());
    if let Some(list) = map.get_mut(&key).and_then(|v| v.as_sequence_mut()) {
        let before = list.len();
        list.retain(|v| v.as_str().map(|s| s != entry).unwrap_or(true));
        if list.len() != before {
            save_yaml_root(base, &root)?;
        }
    }
    Ok(load_approvals_config(base))
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

/// 设置嵌套浮点键（如 `evolution.gates.min_judge_score`）。
fn set_nested_f64(base: &Path, parents: &[&str], key: &str, value: f64) -> anyhow::Result<()> {
    let mut root = load_yaml_root(base)?;
    let map = ensure_mapping_path(&mut root, parents)?;
    map.insert(
        serde_yaml::Value::String(key.to_string()),
        serde_yaml::Value::Number(serde_yaml::Number::from(value)),
    );
    save_yaml_root(base, &root)
}

/// 设置嵌套无符号整数键（如 `evolution.gates.max_skill_bytes`）。
fn set_nested_usize(base: &Path, parents: &[&str], key: &str, value: usize) -> anyhow::Result<()> {
    let mut root = load_yaml_root(base)?;
    let map = ensure_mapping_path(&mut root, parents)?;
    map.insert(
        serde_yaml::Value::String(key.to_string()),
        serde_yaml::Value::Number(serde_yaml::Number::from(value as u64)),
    );
    save_yaml_root(base, &root)
}

/// 设置 `evolution.enabled` 并返回最新配置。
pub fn set_evolution_enabled(base: &Path, enabled: bool) -> anyhow::Result<EvolutionConfig> {
    set_nested_bool(base, &["evolution"], "enabled", enabled)?;
    Ok(load_evolution_config(base))
}

/// 设置单个进化路由（`reflection` / `judge`）并返回最新配置。
pub fn set_evolution_route(
    base: &Path,
    kind: EvolutionRouteKind,
    route: AuxiliaryRoute,
) -> anyhow::Result<EvolutionConfig> {
    set_nested_route(base, &["evolution"], kind.config_key(), &route)?;
    Ok(load_evolution_config(base))
}

/// 将两条进化路由重置为 `auto/auto` 并返回最新配置。
pub fn reset_all_evolution_routes(base: &Path) -> anyhow::Result<EvolutionConfig> {
    let mut root = load_yaml_root(base)?;
    let map = ensure_mapping_path(&mut root, &["evolution"])?;
    let default_route = AuxiliaryRoute::default();
    for kind in EvolutionRouteKind::ALL {
        map.insert(
            serde_yaml::Value::String(kind.config_key().to_string()),
            route_to_value(&default_route),
        );
    }
    save_yaml_root(base, &root)?;
    Ok(load_evolution_config(base))
}

/// 设置 DSPy 对接配置并返回最新配置。
pub fn set_evolution_dspy(base: &Path, dspy: &EvolutionDspy) -> anyhow::Result<EvolutionConfig> {
    set_nested_bool(base, &["evolution", "dspy"], "enabled", dspy.enabled)?;
    set_nested_string(base, &["evolution", "dspy"], "python_bin", &dspy.python_bin)?;
    set_nested_string(base, &["evolution", "dspy"], "project_path", &dspy.project_path)?;
    set_nested_usize(
        base,
        &["evolution", "dspy"],
        "timeout_secs",
        dspy.timeout_secs as usize,
    )?;
    Ok(load_evolution_config(base))
}

/// 设置遗传搜索参数并返回最新配置。
pub fn set_evolution_search(
    base: &Path,
    search: &EvolutionSearch,
) -> anyhow::Result<EvolutionConfig> {
    set_nested_usize(
        base,
        &["evolution", "search"],
        "generations",
        search.generations as usize,
    )?;
    set_nested_usize(
        base,
        &["evolution", "search"],
        "variants",
        search.variants as usize,
    )?;
    set_nested_bool(base, &["evolution", "search"], "crossover", search.crossover)?;
    Ok(load_evolution_config(base))
}

/// 设置自动触发参数并返回最新配置。
pub fn set_evolution_auto(base: &Path, auto: &EvolutionAuto) -> anyhow::Result<EvolutionConfig> {
    set_nested_bool(base, &["evolution", "auto"], "enabled", auto.enabled)?;
    set_nested_usize(
        base,
        &["evolution", "auto"],
        "cooldown_secs",
        auto.cooldown_secs as usize,
    )?;
    set_nested_usize(
        base,
        &["evolution", "auto"],
        "min_new_decisions",
        auto.min_new_decisions,
    )?;
    set_nested_usize(
        base,
        &["evolution", "auto"],
        "max_runs_per_day",
        auto.max_runs_per_day as usize,
    )?;
    Ok(load_evolution_config(base))
}

/// 设置进化门禁并返回最新配置。
pub fn set_evolution_gates(
    base: &Path,
    gates: &EvolutionGates,
) -> anyhow::Result<EvolutionConfig> {
    set_nested_bool(base, &["evolution", "gates"], "run_tests", gates.run_tests)?;
    set_nested_bool(base, &["evolution", "gates"], "require_pr", gates.require_pr)?;
    set_nested_usize(
        base,
        &["evolution", "gates"],
        "max_skill_bytes",
        gates.max_skill_bytes,
    )?;
    set_nested_f64(
        base,
        &["evolution", "gates"],
        "min_judge_score",
        gates.min_judge_score as f64,
    )?;
    Ok(load_evolution_config(base))
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
    let provider =
        if route.provider.trim().is_empty() || route.provider.eq_ignore_ascii_case("auto") {
            session_provider.to_string()
        } else {
            route.provider.trim().to_string()
        };
    let model = if route.model.trim().is_empty() || route.model.eq_ignore_ascii_case("auto") {
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
        let learning = load_learning_config(dir.path());
        assert_eq!(learning, LearningConfig::default());
        assert!(learning.nudge_enabled);
        assert_eq!(learning.complex_task_tool_threshold, 5);
        assert_eq!(learning.unused_skill_days, 30);
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
    fn evolution_defaults_and_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = load_evolution_config(dir.path());
        assert_eq!(cfg, EvolutionConfig::default());
        assert!(!cfg.enabled);
        assert_eq!(cfg.reflection, AuxiliaryRoute::default());
        assert_eq!(cfg.judge, AuxiliaryRoute::default());
        assert!(cfg.gates.run_tests);
        assert!(cfg.gates.require_pr);
        assert_eq!(cfg.gates.max_skill_bytes, 15_360);
        assert!((cfg.gates.min_judge_score - 0.6).abs() < 1e-6);
        assert_eq!(cfg.search.generations, 2);
        assert_eq!(cfg.search.variants, 3);
        assert!(cfg.search.crossover);
        assert!(!cfg.dspy.enabled);
        assert_eq!(cfg.dspy.timeout_secs, 600);
        assert!(!cfg.auto.enabled);
        assert_eq!(cfg.auto.cooldown_secs, 3_600);
        assert_eq!(cfg.auto.min_new_decisions, 3);
        assert_eq!(cfg.auto.max_runs_per_day, 3);
    }

    #[test]
    fn evolution_set_auto_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = set_evolution_auto(
            dir.path(),
            &EvolutionAuto {
                enabled: true,
                cooldown_secs: 7200,
                min_new_decisions: 5,
                max_runs_per_day: 2,
            },
        )
        .unwrap();
        assert!(cfg.auto.enabled);
        assert_eq!(cfg.auto.cooldown_secs, 7200);
        assert_eq!(cfg.auto.min_new_decisions, 5);
        assert_eq!(cfg.auto.max_runs_per_day, 2);
    }

    #[test]
    fn evolution_set_dspy_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = set_evolution_dspy(
            dir.path(),
            &EvolutionDspy {
                enabled: true,
                python_bin: "/x/py".into(),
                project_path: "evolution-dspy".into(),
                timeout_secs: 300,
            },
        )
        .unwrap();
        assert!(cfg.dspy.enabled);
        assert_eq!(cfg.dspy.python_bin, "/x/py");
        assert_eq!(cfg.dspy.project_path, "evolution-dspy");
        assert_eq!(cfg.dspy.timeout_secs, 300);
    }

    #[test]
    fn evolution_set_search_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = set_evolution_search(
            dir.path(),
            &EvolutionSearch {
                generations: 4,
                variants: 5,
                crossover: false,
            },
        )
        .unwrap();
        assert_eq!(cfg.search.generations, 4);
        assert_eq!(cfg.search.variants, 5);
        assert!(!cfg.search.crossover);
    }

    #[test]
    fn evolution_set_enabled_route_and_gates_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("config.yaml"), "hooks:\n  enabled: true\n").unwrap();

        let cfg = set_evolution_enabled(dir.path(), true).unwrap();
        assert!(cfg.enabled);

        set_evolution_route(
            dir.path(),
            EvolutionRouteKind::Reflection,
            AuxiliaryRoute {
                provider: "prov-strong".into(),
                model: "big".into(),
            },
        )
        .unwrap();
        let cfg = load_evolution_config(dir.path());
        assert_eq!(cfg.reflection.provider, "prov-strong");
        assert_eq!(cfg.reflection.model, "big");
        assert_eq!(cfg.judge, AuxiliaryRoute::default());

        let cfg = set_evolution_gates(
            dir.path(),
            &EvolutionGates {
                run_tests: false,
                max_skill_bytes: 8192,
                require_pr: false,
                min_judge_score: 0.75,
            },
        )
        .unwrap();
        assert!(!cfg.gates.run_tests);
        assert!(!cfg.gates.require_pr);
        assert_eq!(cfg.gates.max_skill_bytes, 8192);
        assert!((cfg.gates.min_judge_score - 0.75).abs() < 1e-6);

        // 保留无关键
        let text = fs::read_to_string(dir.path().join("config.yaml")).unwrap();
        assert!(text.contains("enabled: true"));

        let cfg = reset_all_evolution_routes(dir.path()).unwrap();
        assert_eq!(cfg.reflection, AuxiliaryRoute::default());
        assert_eq!(cfg.judge, AuxiliaryRoute::default());
        // gates 不受重置路由影响
        assert!(!cfg.gates.run_tests);
    }

    #[test]
    fn learning_yaml_overrides() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.yaml"),
            r#"
learning:
  nudge_enabled: false
  complex_task_tool_threshold: 8
  unused_skill_days: 14
"#,
        )
        .unwrap();
        let cfg = load_learning_config(dir.path());
        assert!(!cfg.nudge_enabled);
        assert_eq!(cfg.complex_task_tool_threshold, 8);
        assert_eq!(cfg.unused_skill_days, 14);
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
    fn approvals_defaults_to_smart_empty() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = load_approvals_config(dir.path());
        assert_eq!(cfg.mode, "smart");
        assert!(cfg.command_allowlist.is_empty());
    }

    #[test]
    fn set_mode_and_append_allowlist_dedups_and_preserves() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.yaml"),
            "memory:\n  memory_char_limit: 42\n",
        )
        .unwrap();
        let cfg = set_approval_mode(dir.path(), "manual").unwrap();
        assert_eq!(cfg.mode, "manual");

        add_command_to_allowlist(dir.path(), "rm -rf /tmp/x").unwrap();
        let cfg = add_command_to_allowlist(dir.path(), "rm -rf /tmp/x").unwrap(); // 去重
        assert_eq!(cfg.command_allowlist, vec!["rm -rf /tmp/x".to_string()]);
        let cfg = add_command_to_allowlist(dir.path(), "git push --force*").unwrap();
        assert_eq!(cfg.command_allowlist.len(), 2);

        // 移除一条
        let cfg = remove_command_from_allowlist(dir.path(), "rm -rf /tmp/x").unwrap();
        assert_eq!(cfg.command_allowlist, vec!["git push --force*".to_string()]);
        // 移除不存在的不报错、不改动
        let cfg = remove_command_from_allowlist(dir.path(), "nope").unwrap();
        assert_eq!(cfg.command_allowlist.len(), 1);

        // 保留无关键
        assert_eq!(load_memory_config(dir.path()).memory_char_limit, 42);
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
