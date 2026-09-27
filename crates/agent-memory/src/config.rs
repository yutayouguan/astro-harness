//! 从 `{base}/config.toml` 加载记忆相关配置（`memory:` / `auxiliary:` 段）。
//!
//! 与 hooks 共用同一路径；本模块只反序列化关心的段，忽略其余键。
//! 写回开关时用 [`serde_yaml::Value`] 合并，保留 hooks 等其余键。

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::warn;
use types::{
    ApprovalPolicy, ApprovalsReviewer, PermissionPreset, PermissionsConfig, SessionPermissions,
};

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

/// 记忆子系统配置（`config.toml` 的 `memory:` 段）。
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

/// 运行时学习闭环配置（`config.toml` 的 `learning:` 段）。
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

/// 命令审批规则（`config.toml` 的 `command_approvals:` 段）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommandTypeRule {
    /// 可执行程序族，例如 `curl`。不包含参数，匹配时忽略大小写。
    pub command_family: String,
    /// 仅对同一风险分类生效，避免把某个程序的高风险用法一并放行。
    pub risk: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct CommandApprovalConfig {
    /// 用户永久放行的命令白名单（精确或 glob，含 `* ? [`）。
    #[serde(default)]
    pub command_allowlist: Vec<String>,
    /// 用户永久放行的低风险命令类型；同时匹配程序族与风险分类。
    #[serde(default)]
    pub command_type_allowlist: Vec<CommandTypeRule>,
}

/// 命令网络代理开关。它与 profile 的 `network.enabled` 是两个独立维度。
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
struct NetworkProxyConfig {
    #[serde(default)]
    enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionConfigSource {
    Default,
    ExplicitProfiles,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDiagnosticCode {
    InvalidProfileConfig,
    DomainRulesWithoutProxy,
    DroppedWriteRoot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PermissionConfigDiagnostic {
    pub code: PermissionDiagnosticCode,
    pub message: String,
}

/// 启动时解析出的安全权限配置。发生歧义或无效配置时返回安全默认并附带诊断。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoadedPermissionSettings {
    pub permissions: PermissionsConfig,
    pub selection: SessionPermissions,
    pub network_proxy_enabled: bool,
    pub command_allowlist: Vec<String>,
    pub command_type_allowlist: Vec<CommandTypeRule>,
    pub source: PermissionConfigSource,
    pub diagnostics: Vec<PermissionConfigDiagnostic>,
}

impl Default for LoadedPermissionSettings {
    fn default() -> Self {
        Self {
            permissions: PermissionsConfig::default(),
            selection: SessionPermissions::ask_for_approval(),
            network_proxy_enabled: false,
            command_allowlist: Vec::new(),
            command_type_allowlist: Vec::new(),
            source: PermissionConfigSource::Default,
            diagnostics: Vec::new(),
        }
    }
}

fn default_true_compression() -> bool {
    true
}
fn default_soft_ratio() -> f32 {
    0.40
}
fn default_medium_ratio() -> f32 {
    0.60
}
fn default_hard_ratio() -> f32 {
    0.80
}
fn default_soft_max_chars() -> usize {
    2_400
}
fn default_soft_head_chars() -> usize {
    1_600
}
fn default_soft_tail_chars() -> usize {
    600
}
fn default_medium_max_chars() -> usize {
    1_800
}
fn default_medium_head_chars() -> usize {
    1_100
}
fn default_medium_tail_chars() -> usize {
    500
}
fn default_hard_max_chars() -> usize {
    900
}
fn default_hard_head_chars() -> usize {
    600
}
fn default_hard_tail_chars() -> usize {
    200
}
fn default_tool_results_limit() -> usize {
    12
}
fn default_mid_run_summary_ratio() -> f32 {
    0.80
}
fn default_recommend_compact_ratio() -> f32 {
    0.85
}
fn default_protect_last_n() -> usize {
    20
}
fn default_protect_first_messages() -> usize {
    4
}
fn default_thrashing_min_gain_ratio() -> f32 {
    0.05
}
fn default_thrashing_max_consecutive() -> u32 {
    3
}
fn default_keep_tail_bubbles() -> usize {
    3
}

/// 上下文卫生配置（`config.toml` 的 `compression:` 段）。
///
/// 驱动 Run 内 Soft/Medium/Hard、mid-run、Gateway recommend、会话 `/compact` keep_tail。
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CompressionConfig {
    #[serde(default = "default_true_compression")]
    pub enabled: bool,
    #[serde(default = "default_soft_ratio")]
    pub soft_ratio: f32,
    #[serde(default = "default_medium_ratio")]
    pub medium_ratio: f32,
    #[serde(default = "default_hard_ratio")]
    pub hard_ratio: f32,
    #[serde(default = "default_soft_max_chars")]
    pub soft_max_chars: usize,
    #[serde(default = "default_soft_head_chars")]
    pub soft_head_chars: usize,
    #[serde(default = "default_soft_tail_chars")]
    pub soft_tail_chars: usize,
    #[serde(default = "default_medium_max_chars")]
    pub medium_max_chars: usize,
    #[serde(default = "default_medium_head_chars")]
    pub medium_head_chars: usize,
    #[serde(default = "default_medium_tail_chars")]
    pub medium_tail_chars: usize,
    #[serde(default = "default_hard_max_chars")]
    pub hard_max_chars: usize,
    #[serde(default = "default_hard_head_chars")]
    pub hard_head_chars: usize,
    #[serde(default = "default_hard_tail_chars")]
    pub hard_tail_chars: usize,
    #[serde(default = "default_tool_results_limit")]
    pub tool_results_limit: usize,
    #[serde(default = "default_mid_run_summary_ratio")]
    pub mid_run_summary_ratio: f32,
    #[serde(default = "default_recommend_compact_ratio")]
    pub recommend_compact_ratio: f32,
    #[serde(default = "default_protect_last_n")]
    pub protect_last_n: usize,
    #[serde(default = "default_protect_first_messages")]
    pub protect_first_messages: usize,
    #[serde(default = "default_thrashing_min_gain_ratio")]
    pub thrashing_min_gain_ratio: f32,
    #[serde(default = "default_thrashing_max_consecutive")]
    pub thrashing_max_consecutive: u32,
    #[serde(default = "default_keep_tail_bubbles")]
    pub keep_tail_bubbles: usize,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            soft_ratio: 0.40,
            medium_ratio: 0.60,
            hard_ratio: 0.80,
            soft_max_chars: 2_400,
            soft_head_chars: 1_600,
            soft_tail_chars: 600,
            medium_max_chars: 1_800,
            medium_head_chars: 1_100,
            medium_tail_chars: 500,
            hard_max_chars: 900,
            hard_head_chars: 600,
            hard_tail_chars: 200,
            tool_results_limit: 12,
            mid_run_summary_ratio: 0.80,
            recommend_compact_ratio: 0.85,
            protect_last_n: 20,
            protect_first_messages: 4,
            thrashing_min_gain_ratio: 0.05,
            thrashing_max_consecutive: 3,
            keep_tail_bubbles: 3,
        }
    }
}

/// 辅助模型配置（`config.toml` 的 `auxiliary:` 段）。
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
    #[serde(default)]
    pub workflow_ai_polish: AuxiliaryRoute,
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
    /// 工作流 AI 辅助（✨ 润色/生成）。
    WorkflowAiPolish,
}

impl AuxiliaryKind {
    pub const ALL: [Self; 6] = [
        Self::TitleGeneration,
        Self::Compaction,
        Self::SmartApproval,
        Self::Dreaming,
        Self::BackgroundReview,
        Self::WorkflowAiPolish,
    ];

    pub const fn config_key(self) -> &'static str {
        match self {
            Self::TitleGeneration => "title_generation",
            Self::Compaction => "compaction",
            Self::SmartApproval => "smart_approval",
            Self::Dreaming => "dreaming",
            Self::BackgroundReview => "background_review",
            Self::WorkflowAiPolish => "workflow_ai_polish",
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
            AuxiliaryKind::WorkflowAiPolish => &self.workflow_ai_polish,
        }
    }
}

fn default_max_skill_bytes() -> usize {
    15_360
}

fn default_min_judge_score() -> f32 {
    0.6
}

/// 离线进化门禁（`config.toml` 的 `evolution.gates` 段）。
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct EvolutionGates {
    /// 候选变体须通过测试。
    #[serde(default = "default_true")]
    pub run_tests: bool,
    /// Skill 体积上限（字节，默认 ~15KB）。
    #[serde(default = "default_max_skill_bytes")]
    pub max_skill_bytes: usize,
    /// 始终人工审批（产品不变量）；配置项保留兼容，读写时强制为 `true`。
    #[serde(default = "default_true")]
    pub require_pr: bool,
    /// judge 最低分（0–1）；`<= 0` 表示关闭 judge 评审。
    #[serde(default = "default_min_judge_score")]
    pub min_judge_score: f32,
    /// 沙箱模式：`"tempdir"`（默认）| `"docker"`（容器隔离）。
    /// 对标 GEPA gskill 的 Docker harness。
    #[serde(default = "default_sandbox_mode")]
    pub sandbox_mode: String,
    /// Docker 沙箱镜像名（sandbox_mode="docker" 时使用）。
    #[serde(default = "default_sandbox_docker_image")]
    pub sandbox_docker_image: String,
}

fn default_sandbox_mode() -> String {
    "tempdir".to_string()
}

fn default_sandbox_docker_image() -> String {
    "astro-skill-test:latest".to_string()
}

impl Default for EvolutionGates {
    fn default() -> Self {
        Self {
            run_tests: true,
            max_skill_bytes: 15_360,
            require_pr: true,
            min_judge_score: 0.6,
            sandbox_mode: "tempdir".to_string(),
            sandbox_docker_image: "astro-skill-test:latest".to_string(),
        }
    }
}

/// 离线进化配置（`config.toml` 的 `evolution:` 段）。
///
/// 与 `auxiliary`（在线便宜辅助）分离：进化为离线批量、可接受慢与贵；
/// `reflection` 应显式指向强模型，`judge` 可省或走中等模型。
fn default_generations() -> u32 {
    2
}

fn default_variants() -> u32 {
    3
}

fn default_max_eval_examples() -> usize {
    5
}

fn default_population_size() -> u32 {
    3
}

fn default_max_llm_calls() -> u32 {
    40
}

fn default_post_approval_cooldown_secs() -> u64 {
    172_800 // 48h
}

/// GEPA-lite 遗传搜索参数（`config.toml` 的 `evolution.search` 段）。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct EvolutionSearch {
    /// 迭代代数。
    #[serde(default = "default_generations")]
    pub generations: u32,
    /// 每代总变体目标（不按父代展开，控制成本）。
    #[serde(default = "default_variants")]
    pub variants: u32,
    /// 是否启用交叉算子（对 Pareto 前沿 top-2 融合出子代）。
    #[serde(default = "default_true")]
    pub crossover: bool,
    /// 每技能最多参与 grounded 评分的评测例数（0 = 不限）。
    #[serde(default = "default_max_eval_examples")]
    pub max_eval_examples: usize,
    /// 单次遗传搜索的 LLM 调用总上限（0 = 不限；默认 40）。
    #[serde(default = "default_max_llm_calls")]
    pub max_llm_calls: u32,
    /// 每代保留的种群大小（Pareto 选择后保留的最多个体数）。
    #[serde(default = "default_population_size")]
    pub population_size: u32,
    /// 自定义变异 system prompt（空/None = 使用内置 MUTATION_SYSTEM_PROMPT）。
    /// 对标 GEPA ProposalFn：让用户注入领域知识到变异过程。
    #[serde(default)]
    pub mutation_system_prompt: Option<String>,
    /// 自定义交叉 system prompt（空/None = 使用内置 CROSSOVER_SYSTEM_PROMPT）。
    #[serde(default)]
    pub crossover_system_prompt: Option<String>,
    /// 评测例采样模式：`"fixed"`（默认，Fail 优先截断）| `"shuffle"`（每代随机采样）。
    /// 对标 GEPA batch_sampler / epoch_shuffled。
    #[serde(default = "default_eval_sampling")]
    pub eval_sampling: String,
    /// [P0] 技能批准后的冷却期（秒）。在此期间内对同一技能再次发起搜索会被提前返回。
    /// 0 = 禁用。对标 AgentArk 72h 稳定观测窗口。
    #[serde(default = "default_post_approval_cooldown_secs")]
    pub post_approval_cooldown_secs: u64,
}

fn default_eval_sampling() -> String {
    "fixed".to_string()
}

impl Default for EvolutionSearch {
    fn default() -> Self {
        Self {
            generations: 2,
            variants: 3,
            crossover: true,
            max_eval_examples: 5,
            max_llm_calls: 40,
            population_size: 3,
            mutation_system_prompt: None,
            crossover_system_prompt: None,
            eval_sampling: "fixed".to_string(),
            post_approval_cooldown_secs: 172_800,
        }
    }
}

fn default_min_skill_failure_signals() -> usize {
    3
}

fn default_signal_window_days() -> u32 {
    7
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

/// 自动触发进化（`config.toml` 的 `evolution.auto` 段）。
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
    /// [P2] 触发定向进化所需的最少 skill 失败信号数（ToolFailure / UserCorrection）。
    /// 0 = 禁用定向触发。
    #[serde(default = "default_min_skill_failure_signals")]
    pub min_skill_failure_signals: usize,
    /// [P2] 统计失败信号的时间窗口（天）。
    #[serde(default = "default_signal_window_days")]
    pub signal_window_days: u32,
}

impl Default for EvolutionAuto {
    fn default() -> Self {
        Self {
            enabled: false,
            cooldown_secs: 3_600,
            min_new_decisions: 3,
            max_runs_per_day: 3,
            min_skill_failure_signals: 3,
            signal_window_days: 7,
        }
    }
}

fn default_curator_interval_days() -> u32 {
    7
}

fn default_curator_max_enqueue() -> usize {
    5
}

/// 技能策展（`config.toml` 的 `evolution.curator` 段）。
///
/// 默认关闭；手动可随时跑。开启后仅表示「允许按 interval 提示/自动报告」，
/// **默认不自动入队提案**（入队需显式 enqueue）。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct EvolutionCurator {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_curator_interval_days")]
    pub interval_days: u32,
    /// 单次入队上限。
    #[serde(default = "default_curator_max_enqueue")]
    pub max_enqueue: usize,
    /// LLM 辅助诊断开关（默认关）。
    #[serde(default)]
    pub llm_diagnose: bool,
    /// 单次策展最多 LLM 诊断调用数（默认 3）。
    #[serde(default = "default_curator_max_llm_calls")]
    pub max_llm_calls: u32,
}

fn default_curator_max_llm_calls() -> u32 {
    3
}

impl Default for EvolutionCurator {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_days: 7,
            max_enqueue: 5,
            llm_diagnose: false,
            max_llm_calls: 3,
        }
    }
}

/// 外部 Python DSPy 引擎对接配置（`config.toml` 的 `evolution.dspy` 段）。
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

/// 离线进化配置（`config.toml` 的 `evolution:` 段）。
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
    /// Curator（技能库健康维护）。
    #[serde(default)]
    pub curator: EvolutionCurator,
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
    command_approvals: Option<CommandApprovalConfig>,
    #[serde(default)]
    permissions: Option<PermissionsConfig>,
    #[serde(default)]
    approval_policy: Option<ApprovalPolicy>,
    #[serde(default)]
    approvals_reviewer: Option<ApprovalsReviewer>,
    #[serde(default)]
    network_proxy: Option<NetworkProxyConfig>,
    #[serde(default)]
    compression: Option<CompressionConfig>,
}

fn read_file_config(base: &Path) -> FileConfig {
    let path = base.join("config.toml");
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
    match toml::from_str::<FileConfig>(&text) {
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

/// 从 `{base}/config.toml` 加载记忆配置；文件缺失或无 `memory:` 段时返回默认值。
pub fn load_memory_config(base: &Path) -> MemoryConfig {
    read_file_config(base).memory.unwrap_or_default()
}

/// 从 `{base}/config.toml` 加载辅助模型配置。
pub fn load_auxiliary_config(base: &Path) -> AuxiliaryConfig {
    read_file_config(base).auxiliary.unwrap_or_default()
}

/// 从 `{base}/config.toml` 加载学习闭环配置。
pub fn load_learning_config(base: &Path) -> LearningConfig {
    read_file_config(base).learning.unwrap_or_default()
}

/// 从 `{base}/config.toml` 加载离线进化配置。
pub fn load_evolution_config(base: &Path) -> EvolutionConfig {
    let mut cfg = read_file_config(base).evolution.unwrap_or_default();
    // 产品不变量：进化产物只经人工审批，忽略 yaml 中的 false。
    cfg.gates.require_pr = true;
    cfg
}

/// 从 `{base}/config.toml` 加载命令审批白名单。
pub fn load_command_approval_config(base: &Path) -> CommandApprovalConfig {
    read_file_config(base).command_approvals.unwrap_or_default()
}

/// 加载权限 profile、审批策略与审查者。
pub fn load_permission_settings(base: &Path) -> LoadedPermissionSettings {
    let file = read_file_config(base);
    if let Some(permissions) = file.permissions.clone() {
        return load_explicit_permissions(file, permissions, base);
    }
    let rules = file.command_approvals.unwrap_or_default();
    let mut loaded = LoadedPermissionSettings::default();
    loaded.selection.approval_policy = file.approval_policy.unwrap_or_default();
    loaded.selection.approvals_reviewer = file.approvals_reviewer.unwrap_or_default();
    loaded.command_allowlist = rules.command_allowlist;
    loaded.command_type_allowlist = rules.command_type_allowlist;
    loaded
}

/// 净化配置文件带来的可写根（用户级 + 各 profile 自带）：只保留通过
/// `permission_audit::sanitize_write_root` 的条目，并把它们规范化成沙箱真正使用的路径。
///
/// 返回被丢弃的原始条目（用于诊断）。这是"模型不能给自己授权"的最后一道防线：
/// 即使有人手写 config.toml，`~/.ssh`、Astro 自身目录这类路径也不会生效。
fn sanitize_loaded_write_roots(permissions: &mut PermissionsConfig, base: &Path) -> Vec<String> {
    let mut dropped = Vec::new();
    let sanitize = |roots: &mut Vec<String>, dropped: &mut Vec<String>| {
        let mut kept: Vec<String> = Vec::new();
        for root in roots.drain(..) {
            match crate::permission_audit::sanitize_write_root(&root, base) {
                Ok(path) => {
                    let rendered = path.display().to_string();
                    if !kept.contains(&rendered) {
                        kept.push(rendered);
                    }
                }
                Err(reason) => {
                    tracing::warn!(root = %root, %reason, "ignoring unauthorized writable root");
                    dropped.push(reason);
                }
            }
        }
        *roots = kept;
    };
    sanitize(&mut permissions.extra_writable_roots, &mut dropped);
    for profile in permissions.profiles.values_mut() {
        sanitize(&mut profile.extra_writable_roots, &mut dropped);
    }
    dropped
}

/// 覆写用户级永久可写目录（权限设置页维护，`permissions.extra_writable_roots`）。
///
/// 与选中哪个 profile 无关，因此内置组合也能用；每个路径都按
/// `permission_audit::sanitize_write_root` 净化，空列表删除该键。
pub fn set_extra_write_roots(
    base: &Path,
    roots: &[String],
) -> anyhow::Result<LoadedPermissionSettings> {
    let mut normalized = Vec::with_capacity(roots.len());
    for root in roots {
        let path = crate::permission_audit::sanitize_write_root(root, base)
            .map_err(|reason| anyhow::anyhow!("{reason}"))?;
        let rendered = path.display().to_string();
        if !normalized.contains(&rendered) {
            normalized.push(rendered);
        }
    }

    let _guard = home::config_file::lock_config_file(&home::config_path(base))?;
    let mut root = load_config_root(base)?;
    let permissions = ensure_mapping_path(&mut root, &["permissions"])?;
    let key = serde_yaml::Value::String("extra_writable_roots".into());
    if normalized.is_empty() {
        permissions.remove(&key);
    } else {
        permissions.insert(
            key,
            serde_yaml::Value::Sequence(
                normalized
                    .into_iter()
                    .map(serde_yaml::Value::String)
                    .collect(),
            ),
        );
    }
    save_config_root(base, &root)?;
    Ok(load_permission_settings(base))
}

/// 激活桌面端内置权限组合，同时保留自定义 profiles 和其它配置段。
pub fn set_permission_preset(
    base: &Path,
    preset: PermissionPreset,
) -> anyhow::Result<LoadedPermissionSettings> {
    let selection = preset.selection();
    let _guard = home::config_file::lock_config_file(&home::config_path(base))?;
    let mut root = load_config_root(base)?;
    ensure_mapping_path(&mut root, &["permissions"])?.insert(
        serde_yaml::Value::String("default_profile".into()),
        serde_yaml::Value::String(selection.profile_id.clone()),
    );
    {
        let top = ensure_mapping_path(&mut root, &[])?;
        top.insert(
            serde_yaml::Value::String("approval_policy".into()),
            serde_yaml::Value::String(
                match selection.approval_policy {
                    ApprovalPolicy::OnRequest => "on-request",
                    ApprovalPolicy::Never => "never",
                }
                .into(),
            ),
        );
        top.insert(
            serde_yaml::Value::String("approvals_reviewer".into()),
            serde_yaml::Value::String(
                match selection.approvals_reviewer {
                    ApprovalsReviewer::User => "user",
                    ApprovalsReviewer::AutoReview => "auto_review",
                }
                .into(),
            ),
        );
    }
    save_config_root(base, &root)?;
    Ok(load_permission_settings(base))
}

fn load_explicit_permissions(
    file: FileConfig,
    mut permissions: PermissionsConfig,
    base: &Path,
) -> LoadedPermissionSettings {
    let mut diagnostics = Vec::new();
    if let Err(error) = permissions.validate() {
        diagnostics.push(PermissionConfigDiagnostic {
            code: PermissionDiagnosticCode::InvalidProfileConfig,
            message: error.to_string(),
        });
        return LoadedPermissionSettings {
            diagnostics,
            ..LoadedPermissionSettings::default()
        };
    }
    // 配置文件里的可写根同样要过净化：手写（或在极端配置下由模型写入）的
    // `extra_writable_roots` 不能绕过 Astro 自身目录与敏感目录的护栏。
    let dropped_write_roots = sanitize_loaded_write_roots(&mut permissions, base);
    if !dropped_write_roots.is_empty() {
        diagnostics.push(PermissionConfigDiagnostic {
            code: PermissionDiagnosticCode::DroppedWriteRoot,
            message: format!(
                "已忽略不可授权的可写目录：{}",
                dropped_write_roots.join("；")
            ),
        });
    }

    let network_proxy_enabled = file.network_proxy.unwrap_or_default().enabled;
    let has_domain_rules = permissions
        .profiles
        .values()
        .any(|profile| !profile.network.domains.is_empty());
    if has_domain_rules && !network_proxy_enabled {
        diagnostics.push(PermissionConfigDiagnostic {
            code: PermissionDiagnosticCode::DomainRulesWithoutProxy,
            message: "permission profile domain rules require network_proxy.enabled=true"
                .to_string(),
        });
        return LoadedPermissionSettings {
            diagnostics,
            ..LoadedPermissionSettings::default()
        };
    }

    let selection = SessionPermissions {
        profile_id: permissions.default_profile.clone(),
        approval_policy: file.approval_policy.unwrap_or_default(),
        approvals_reviewer: file.approvals_reviewer.unwrap_or_default(),
    };
    let (command_allowlist, command_type_allowlist) = file
        .command_approvals
        .map(|rules| (rules.command_allowlist, rules.command_type_allowlist))
        .unwrap_or_default();
    LoadedPermissionSettings {
        permissions,
        selection,
        network_proxy_enabled,
        command_allowlist,
        command_type_allowlist,
        source: PermissionConfigSource::ExplicitProfiles,
        diagnostics,
    }
}

/// 从 `{base}/config.toml` 加载上下文卫生配置。
pub fn load_compression_config(base: &Path) -> CompressionConfig {
    read_file_config(base).compression.unwrap_or_default()
}

fn insert_yaml_f64(map: &mut serde_yaml::Mapping, key: &str, value: f64) {
    map.insert(
        serde_yaml::Value::String(key.to_string()),
        serde_yaml::Value::Number(serde_yaml::Number::from(value)),
    );
}

fn insert_yaml_usize(map: &mut serde_yaml::Mapping, key: &str, value: usize) {
    map.insert(
        serde_yaml::Value::String(key.to_string()),
        serde_yaml::Value::Number(serde_yaml::Number::from(value as u64)),
    );
}

fn insert_yaml_u32(map: &mut serde_yaml::Mapping, key: &str, value: u32) {
    map.insert(
        serde_yaml::Value::String(key.to_string()),
        serde_yaml::Value::Number(serde_yaml::Number::from(value)),
    );
}

fn insert_yaml_bool(map: &mut serde_yaml::Mapping, key: &str, value: bool) {
    map.insert(
        serde_yaml::Value::String(key.to_string()),
        serde_yaml::Value::Bool(value),
    );
}

fn write_compression_mapping(map: &mut serde_yaml::Mapping, cfg: &CompressionConfig) {
    insert_yaml_bool(map, "enabled", cfg.enabled);
    insert_yaml_f64(map, "soft_ratio", cfg.soft_ratio as f64);
    insert_yaml_f64(map, "medium_ratio", cfg.medium_ratio as f64);
    insert_yaml_f64(map, "hard_ratio", cfg.hard_ratio as f64);
    insert_yaml_usize(map, "soft_max_chars", cfg.soft_max_chars);
    insert_yaml_usize(map, "soft_head_chars", cfg.soft_head_chars);
    insert_yaml_usize(map, "soft_tail_chars", cfg.soft_tail_chars);
    insert_yaml_usize(map, "medium_max_chars", cfg.medium_max_chars);
    insert_yaml_usize(map, "medium_head_chars", cfg.medium_head_chars);
    insert_yaml_usize(map, "medium_tail_chars", cfg.medium_tail_chars);
    insert_yaml_usize(map, "hard_max_chars", cfg.hard_max_chars);
    insert_yaml_usize(map, "hard_head_chars", cfg.hard_head_chars);
    insert_yaml_usize(map, "hard_tail_chars", cfg.hard_tail_chars);
    insert_yaml_usize(map, "tool_results_limit", cfg.tool_results_limit);
    insert_yaml_f64(
        map,
        "mid_run_summary_ratio",
        cfg.mid_run_summary_ratio as f64,
    );
    insert_yaml_f64(
        map,
        "recommend_compact_ratio",
        cfg.recommend_compact_ratio as f64,
    );
    insert_yaml_usize(map, "protect_last_n", cfg.protect_last_n);
    insert_yaml_usize(map, "protect_first_messages", cfg.protect_first_messages);
    insert_yaml_f64(
        map,
        "thrashing_min_gain_ratio",
        cfg.thrashing_min_gain_ratio as f64,
    );
    insert_yaml_u32(
        map,
        "thrashing_max_consecutive",
        cfg.thrashing_max_consecutive,
    );
    insert_yaml_usize(map, "keep_tail_bubbles", cfg.keep_tail_bubbles);
}

/// 整包写入 `compression:` 段并返回最新配置。
pub fn set_compression_config(
    base: &Path,
    cfg: &CompressionConfig,
) -> anyhow::Result<CompressionConfig> {
    let _guard = home::config_file::lock_config_file(&home::config_path(base))?;
    let mut root = load_config_root(base)?;
    let map = ensure_mapping_path(&mut root, &["compression"])?;
    write_compression_mapping(map, cfg);
    save_config_root(base, &root)?;
    Ok(load_compression_config(base))
}

/// 将 `compression:` 重置为默认值并返回最新配置。
pub fn reset_compression_config(base: &Path) -> anyhow::Result<CompressionConfig> {
    set_compression_config(base, &CompressionConfig::default())
}

fn config_toml_path(base: &Path) -> std::path::PathBuf {
    home::config_path(base)
}

/// 读入已有 `config.toml` 为 Value；缺失则空 Mapping。
fn load_config_root(base: &Path) -> anyhow::Result<serde_yaml::Value> {
    let path = config_toml_path(base);
    if !path.is_file() {
        return Ok(serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
    }
    let text = fs::read_to_string(&path)?;
    if text.trim().is_empty() {
        return Ok(serde_yaml::Value::Mapping(serde_yaml::Mapping::new()));
    }
    Ok(serde_yaml::to_value(toml::from_str::<toml::Value>(&text)?)?)
}

/// 原子写回 `config.toml`。
fn save_config_root(base: &Path, root: &serde_yaml::Value) -> anyhow::Result<()> {
    fs::create_dir_all(base)?;
    let path = config_toml_path(base);
    let previous = load_config_root(base)?;
    let mut document = home::settings::read_document(&path)?;
    home::settings::replace_changed_root(&mut document, &previous, root)?;
    home::settings::write_document(&path, &document)
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
        .ok_or_else(|| anyhow::anyhow!("config.toml root must be a mapping"))?;
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
            .ok_or_else(|| anyhow::anyhow!("config.toml segment `{seg}` is not a mapping"))?;
    }
    Ok(cur)
}

/// 设置嵌套布尔键（如 `memory.write_approval`），保留文件中其它键。
fn set_nested_bool(base: &Path, parents: &[&str], key: &str, value: bool) -> anyhow::Result<()> {
    let _guard = home::config_file::lock_config_file(&home::config_path(base))?;
    let mut root = load_config_root(base)?;
    let map = ensure_mapping_path(&mut root, parents)?;
    map.insert(
        serde_yaml::Value::String(key.to_string()),
        serde_yaml::Value::Bool(value),
    );
    save_config_root(base, &root)
}

fn set_nested_string(base: &Path, parents: &[&str], key: &str, value: &str) -> anyhow::Result<()> {
    let _guard = home::config_file::lock_config_file(&home::config_path(base))?;
    let mut root = load_config_root(base)?;
    let map = ensure_mapping_path(&mut root, parents)?;
    map.insert(
        serde_yaml::Value::String(key.to_string()),
        serde_yaml::Value::String(value.to_string()),
    );
    save_config_root(base, &root)
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
    let _guard = home::config_file::lock_config_file(&home::config_path(base))?;
    let mut root = load_config_root(base)?;
    let map = ensure_mapping_path(&mut root, parents)?;
    map.insert(
        serde_yaml::Value::String(key.to_string()),
        route_to_value(route),
    );
    save_config_root(base, &root)
}

/// 设置 `memory.write_approval` 并返回最新配置。
pub fn set_write_approval(base: &Path, enabled: bool) -> anyhow::Result<MemoryConfig> {
    set_nested_bool(base, &["memory"], "write_approval", enabled)?;
    Ok(load_memory_config(base))
}

/// 向 `command_approvals.command_allowlist` 追加一条（去重，保留其余键）。
pub fn add_command_to_allowlist(base: &Path, entry: &str) -> anyhow::Result<CommandApprovalConfig> {
    let entry = entry.trim();
    if entry.is_empty() {
        return Ok(load_command_approval_config(base));
    }
    let _guard = home::config_file::lock_config_file(&home::config_path(base))?;
    let mut root = load_config_root(base)?;
    let map = ensure_mapping_path(&mut root, &["command_approvals"])?;
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
        save_config_root(base, &root)?;
    }
    Ok(load_command_approval_config(base))
}

/// 从 `command_approvals.command_allowlist` 移除一条（按精确文本）。
pub fn remove_command_from_allowlist(
    base: &Path,
    entry: &str,
) -> anyhow::Result<CommandApprovalConfig> {
    let entry = entry.trim();
    let _guard = home::config_file::lock_config_file(&home::config_path(base))?;
    let mut root = load_config_root(base)?;
    let map = ensure_mapping_path(&mut root, &["command_approvals"])?;
    let key = serde_yaml::Value::String("command_allowlist".to_string());
    if let Some(list) = map.get_mut(&key).and_then(|v| v.as_sequence_mut()) {
        let before = list.len();
        list.retain(|v| v.as_str().map(|s| s != entry).unwrap_or(true));
        if list.len() != before {
            save_config_root(base, &root)?;
        }
    }
    Ok(load_command_approval_config(base))
}

/// 向 `approvals.command_type_allowlist` 追加一条低风险命令类型规则。
pub fn add_command_type_to_allowlist(
    base: &Path,
    rule: &CommandTypeRule,
) -> anyhow::Result<CommandApprovalConfig> {
    let family = rule.command_family.trim().to_ascii_lowercase();
    let risk = rule.risk.trim();
    if family.is_empty() || risk.is_empty() {
        return Ok(load_command_approval_config(base));
    }
    let normalized = CommandTypeRule {
        command_family: family,
        risk: risk.to_string(),
    };
    let _guard = home::config_file::lock_config_file(&home::config_path(base))?;
    let mut root = load_config_root(base)?;
    let map = ensure_mapping_path(&mut root, &["command_approvals"])?;
    let key = serde_yaml::Value::String("command_type_allowlist".to_string());
    let list = match map.get_mut(&key).and_then(|value| value.as_sequence_mut()) {
        Some(sequence) => sequence,
        None => {
            map.insert(key.clone(), serde_yaml::Value::Sequence(Vec::new()));
            map.get_mut(&key).unwrap().as_sequence_mut().unwrap()
        }
    };
    let value = serde_yaml::to_value(&normalized)?;
    if !list.contains(&value) {
        list.push(value);
        save_config_root(base, &root)?;
    }
    Ok(load_command_approval_config(base))
}

/// 从 `approvals.command_type_allowlist` 移除一条规则。
pub fn remove_command_type_from_allowlist(
    base: &Path,
    rule: &CommandTypeRule,
) -> anyhow::Result<CommandApprovalConfig> {
    let family = rule.command_family.trim();
    let risk = rule.risk.trim();
    let _guard = home::config_file::lock_config_file(&home::config_path(base))?;
    let mut root = load_config_root(base)?;
    let map = ensure_mapping_path(&mut root, &["command_approvals"])?;
    let key = serde_yaml::Value::String("command_type_allowlist".to_string());
    if let Some(list) = map.get_mut(&key).and_then(|value| value.as_sequence_mut()) {
        let before = list.len();
        list.retain(|value| {
            serde_yaml::from_value::<CommandTypeRule>(value.clone())
                .map(|stored| {
                    !stored.command_family.eq_ignore_ascii_case(family) || stored.risk != risk
                })
                .unwrap_or(true)
        });
        if list.len() != before {
            save_config_root(base, &root)?;
        }
    }
    Ok(load_command_approval_config(base))
}

/// Atomically write a single exact-host domain rule into a custom permission profile.
///
/// Only custom leaf profiles may be amended — builtin profiles (`:read-only`,
/// `:workspace`, `:danger-full-access`) are rejected.  The host is stored
/// exactly as given (no wildcard expansion).  Other config keys are preserved.
pub fn amend_network_domain(
    base: &Path,
    profile_id: &str,
    host: &str,
    action: types::NetworkPolicyRuleAction,
) -> anyhow::Result<()> {
    use types::{is_builtin_profile, NetworkPolicyRuleAction};

    let host = host.trim().to_lowercase();
    anyhow::ensure!(!host.is_empty(), "host must not be empty");
    anyhow::ensure!(
        !host.contains('*'),
        "wildcard hosts cannot be persisted as exact amendments"
    );
    anyhow::ensure!(
        !is_builtin_profile(profile_id),
        "builtin profile {profile_id} cannot be amended"
    );

    let _guard = home::config_file::lock_config_file(&home::config_path(base))?;
    let mut root = load_config_root(base)?;
    let profile_map = ensure_mapping_path(
        &mut root,
        &["permissions", "profiles", profile_id, "network"],
    )?;
    let domains_key = serde_yaml::Value::String("domains".into());
    let domains = match profile_map
        .get_mut(&domains_key)
        .and_then(|v| v.as_mapping_mut())
    {
        Some(m) => m,
        None => {
            profile_map.insert(
                domains_key.clone(),
                serde_yaml::Value::Mapping(serde_yaml::Mapping::new()),
            );
            profile_map
                .get_mut(&domains_key)
                .unwrap()
                .as_mapping_mut()
                .unwrap()
        }
    };
    let action_str = match action {
        NetworkPolicyRuleAction::Allow => "allow",
        NetworkPolicyRuleAction::Deny => "deny",
    };
    domains.insert(
        serde_yaml::Value::String(host),
        serde_yaml::Value::String(action_str.into()),
    );
    save_config_root(base, &root)
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
    let _guard = home::config_file::lock_config_file(&home::config_path(base))?;
    let mut root = load_config_root(base)?;
    let map = ensure_mapping_path(&mut root, &["auxiliary"])?;
    let default_route = AuxiliaryRoute::default();
    for kind in AuxiliaryKind::ALL {
        map.insert(
            serde_yaml::Value::String(kind.config_key().to_string()),
            route_to_value(&default_route),
        );
    }
    save_config_root(base, &root)?;
    Ok(load_auxiliary_config(base))
}

/// 设置嵌套浮点键（如 `evolution.gates.min_judge_score`）。
fn set_nested_f64(base: &Path, parents: &[&str], key: &str, value: f64) -> anyhow::Result<()> {
    let _guard = home::config_file::lock_config_file(&home::config_path(base))?;
    let mut root = load_config_root(base)?;
    let map = ensure_mapping_path(&mut root, parents)?;
    map.insert(
        serde_yaml::Value::String(key.to_string()),
        serde_yaml::Value::Number(serde_yaml::Number::from(value)),
    );
    save_config_root(base, &root)
}

/// 设置嵌套无符号整数键（如 `evolution.gates.max_skill_bytes`）。
fn set_nested_usize(base: &Path, parents: &[&str], key: &str, value: usize) -> anyhow::Result<()> {
    let _guard = home::config_file::lock_config_file(&home::config_path(base))?;
    let mut root = load_config_root(base)?;
    let map = ensure_mapping_path(&mut root, parents)?;
    map.insert(
        serde_yaml::Value::String(key.to_string()),
        serde_yaml::Value::Number(serde_yaml::Number::from(value as u64)),
    );
    save_config_root(base, &root)
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
    let _guard = home::config_file::lock_config_file(&home::config_path(base))?;
    let mut root = load_config_root(base)?;
    let map = ensure_mapping_path(&mut root, &["evolution"])?;
    let default_route = AuxiliaryRoute::default();
    for kind in EvolutionRouteKind::ALL {
        map.insert(
            serde_yaml::Value::String(kind.config_key().to_string()),
            route_to_value(&default_route),
        );
    }
    save_config_root(base, &root)?;
    Ok(load_evolution_config(base))
}

/// 设置 DSPy 对接配置并返回最新配置。
pub fn set_evolution_dspy(base: &Path, dspy: &EvolutionDspy) -> anyhow::Result<EvolutionConfig> {
    set_nested_bool(base, &["evolution", "dspy"], "enabled", dspy.enabled)?;
    set_nested_string(base, &["evolution", "dspy"], "python_bin", &dspy.python_bin)?;
    set_nested_string(
        base,
        &["evolution", "dspy"],
        "project_path",
        &dspy.project_path,
    )?;
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
    set_nested_bool(
        base,
        &["evolution", "search"],
        "crossover",
        search.crossover,
    )?;
    set_nested_usize(
        base,
        &["evolution", "search"],
        "max_eval_examples",
        search.max_eval_examples,
    )?;
    set_nested_usize(
        base,
        &["evolution", "search"],
        "max_llm_calls",
        search.max_llm_calls as usize,
    )?;
    set_nested_usize(
        base,
        &["evolution", "search"],
        "population_size",
        search.population_size as usize,
    )?;
    if let Some(ref p) = search.mutation_system_prompt {
        set_nested_string(base, &["evolution", "search"], "mutation_system_prompt", p)?;
    }
    if let Some(ref p) = search.crossover_system_prompt {
        set_nested_string(base, &["evolution", "search"], "crossover_system_prompt", p)?;
    }
    set_nested_string(
        base,
        &["evolution", "search"],
        "eval_sampling",
        &search.eval_sampling,
    )?;
    set_nested_usize(
        base,
        &["evolution", "search"],
        "post_approval_cooldown_secs",
        search.post_approval_cooldown_secs as usize,
    )?;
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
    set_nested_usize(
        base,
        &["evolution", "auto"],
        "min_skill_failure_signals",
        auto.min_skill_failure_signals,
    )?;
    set_nested_usize(
        base,
        &["evolution", "auto"],
        "signal_window_days",
        auto.signal_window_days as usize,
    )?;
    Ok(load_evolution_config(base))
}

/// 设置策展参数并返回最新配置。
pub fn set_evolution_curator(
    base: &Path,
    curator: &EvolutionCurator,
) -> anyhow::Result<EvolutionConfig> {
    set_nested_bool(base, &["evolution", "curator"], "enabled", curator.enabled)?;
    set_nested_usize(
        base,
        &["evolution", "curator"],
        "interval_days",
        curator.interval_days as usize,
    )?;
    set_nested_usize(
        base,
        &["evolution", "curator"],
        "max_enqueue",
        curator.max_enqueue,
    )?;
    set_nested_bool(
        base,
        &["evolution", "curator"],
        "llm_diagnose",
        curator.llm_diagnose,
    )?;
    set_nested_usize(
        base,
        &["evolution", "curator"],
        "max_llm_calls",
        curator.max_llm_calls as usize,
    )?;
    Ok(load_evolution_config(base))
}

/// 设置进化门禁并返回最新配置。
pub fn set_evolution_gates(base: &Path, gates: &EvolutionGates) -> anyhow::Result<EvolutionConfig> {
    set_nested_bool(base, &["evolution", "gates"], "run_tests", gates.run_tests)?;
    // 始终强制人审；忽略调用方传入的 false。
    set_nested_bool(base, &["evolution", "gates"], "require_pr", true)?;
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
    set_nested_string(
        base,
        &["evolution", "gates"],
        "sandbox_mode",
        &gates.sandbox_mode,
    )?;
    set_nested_string(
        base,
        &["evolution", "gates"],
        "sandbox_docker_image",
        &gates.sandbox_docker_image,
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
    fn shared_toml_updates_preserve_unrelated_domains() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            home::config_path(dir.path()),
            "[mcp_servers.demo]\ncommand = 'server'\n[custom]\nrevision = 7\n",
        )
        .unwrap();
        std::thread::scope(|scope| {
            scope.spawn(|| set_write_approval(dir.path(), true).unwrap());
            scope.spawn(|| set_evolution_enabled(dir.path(), true).unwrap());
        });
        let root: toml::Value =
            toml::from_str(&fs::read_to_string(home::config_path(dir.path())).unwrap()).unwrap();
        assert_eq!(root["memory"]["write_approval"].as_bool(), Some(true));
        assert_eq!(root["evolution"]["enabled"].as_bool(), Some(true));
        assert_eq!(
            root["mcp_servers"]["demo"]["command"].as_str(),
            Some("server")
        );
        assert_eq!(root["custom"]["revision"].as_integer(), Some(7));
        assert!(!dir.path().join("config.yaml").exists());
    }

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
        let compression = load_compression_config(dir.path());
        assert_eq!(compression, CompressionConfig::default());
        assert!(compression.enabled);
        assert!((compression.soft_ratio - 0.40).abs() < 1e-6);
        assert_eq!(compression.tool_results_limit, 12);
        assert_eq!(compression.keep_tail_bubbles, 3);
    }

    #[test]
    fn compression_toml_overrides_and_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            r#""hooks" = { "enabled" = true }
"compression" = { "enabled" = false, "soft_ratio" = 0.35, "medium_ratio" = 0.55, "hard_ratio" = 0.75, "tool_results_limit" = 8, "keep_tail_bubbles" = 5 }
"#,
        )
        .unwrap();
        let cfg = load_compression_config(dir.path());
        assert!(!cfg.enabled);
        assert!((cfg.soft_ratio - 0.35).abs() < 1e-6);
        assert!((cfg.medium_ratio - 0.55).abs() < 1e-6);
        assert!((cfg.hard_ratio - 0.75).abs() < 1e-6);
        assert_eq!(cfg.tool_results_limit, 8);
        assert_eq!(cfg.keep_tail_bubbles, 5);
        // 未覆盖字段仍用默认
        assert_eq!(cfg.soft_max_chars, 2_400);
        assert_eq!(cfg.protect_last_n, 20);

        let mut next = cfg.clone();
        next.enabled = true;
        next.recommend_compact_ratio = 0.90;
        next.soft_head_chars = 1_200;
        let saved = set_compression_config(dir.path(), &next).unwrap();
        assert!(saved.enabled);
        assert!((saved.recommend_compact_ratio - 0.90).abs() < 1e-6);
        assert_eq!(saved.soft_head_chars, 1_200);
        assert_eq!(saved.keep_tail_bubbles, 5);

        let text = fs::read_to_string(dir.path().join("config.toml")).unwrap();
        let parsed: toml::Value = text.parse().unwrap();
        assert_eq!(parsed["hooks"]["enabled"].as_bool(), Some(true));
        assert!(text.contains(r#""hooks" = { "enabled" = true }"#));

        let reset = reset_compression_config(dir.path()).unwrap();
        assert_eq!(reset, CompressionConfig::default());
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
                min_skill_failure_signals: 3,
                signal_window_days: 7,
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
                max_eval_examples: 8,
                max_llm_calls: 20,
                population_size: 2,
                ..EvolutionSearch::default()
            },
        )
        .unwrap();
        assert_eq!(cfg.search.generations, 4);
        assert_eq!(cfg.search.variants, 5);
        assert!(!cfg.search.crossover);
        assert_eq!(cfg.search.max_eval_examples, 8);
        assert_eq!(cfg.search.max_llm_calls, 20);
        assert_eq!(cfg.search.population_size, 2);
    }

    #[test]
    fn evolution_set_enabled_route_and_gates_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            r#""hooks" = { "enabled" = true }
"#,
        )
        .unwrap();

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
                require_pr: false, // 调用方传 false 也应被强制为 true
                min_judge_score: 0.75,
                ..EvolutionGates::default()
            },
        )
        .unwrap();
        assert!(!cfg.gates.run_tests);
        assert!(cfg.gates.require_pr);
        assert_eq!(cfg.gates.max_skill_bytes, 8192);
        assert!((cfg.gates.min_judge_score - 0.75).abs() < 1e-6);

        // 保留无关键
        let text = fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(text.contains("enabled = true"));

        let cfg = reset_all_evolution_routes(dir.path()).unwrap();
        assert_eq!(cfg.reflection, AuxiliaryRoute::default());
        assert_eq!(cfg.judge, AuxiliaryRoute::default());
        // gates 不受重置路由影响
        assert!(!cfg.gates.run_tests);
    }

    #[test]
    fn learning_toml_overrides() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            r#""learning" = { "nudge_enabled" = false, "complex_task_tool_threshold" = 8, "unused_skill_days" = 14 }
"#,
        )
        .unwrap();
        let cfg = load_learning_config(dir.path());
        assert!(!cfg.nudge_enabled);
        assert_eq!(cfg.complex_task_tool_threshold, 8);
        assert_eq!(cfg.unused_skill_days, 14);
    }

    #[test]
    fn toml_overrides_and_ignores_hooks() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            r#""hooks" = { "PostToolUse" = "echo hi" }
"memory" = { "memory_enabled" = false, "memory_char_limit" = 100, "user_char_limit" = 50, "write_approval" = true, "daily_prompt_max_chars" = 32, "unknown_key" = "ignored" }
"auxiliary" = { "dreaming" = { "provider" = "openai", "model" = "gpt-4o-mini" }, "background_review" = { "provider" = "auto", "model" = "cheap-review" }, "background_review_enabled" = true }
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
            dir.path().join("config.toml"),
            r#""hooks" = { "PostToolUse" = "echo hi" }
"memory" = { "memory_char_limit" = 99 }
"#,
        )
        .unwrap();
        let cfg = set_write_approval(dir.path(), true).unwrap();
        assert!(cfg.write_approval);
        assert_eq!(cfg.memory_char_limit, 99);
        let text = fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(text.contains("PostToolUse"));
        assert!(text.contains("write_approval = true"));
        assert!(text.contains("99"));
    }

    #[test]
    fn set_background_review_enabled_creates_auxiliary() {
        let dir = tempfile::tempdir().unwrap();
        let aux = set_background_review_enabled(dir.path(), true).unwrap();
        assert!(aux.background_review_enabled);
        let text = fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(text.contains("background_review_enabled = true"));
    }

    #[test]
    fn command_approval_rules_default_to_empty() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = load_command_approval_config(dir.path());
        assert!(cfg.command_allowlist.is_empty());
        assert!(cfg.command_type_allowlist.is_empty());
    }

    #[test]
    fn command_type_allowlist_roundtrips_and_can_be_removed() {
        let dir = tempfile::tempdir().unwrap();
        let rule = CommandTypeRule {
            command_family: "CURL".to_string(),
            risk: "dynamic shell expansion".to_string(),
        };
        let cfg = add_command_type_to_allowlist(dir.path(), &rule).unwrap();
        assert_eq!(cfg.command_type_allowlist.len(), 1);
        assert_eq!(cfg.command_type_allowlist[0].command_family, "curl");

        let loaded = load_permission_settings(dir.path());
        assert_eq!(loaded.command_type_allowlist, cfg.command_type_allowlist);

        let cfg = remove_command_type_from_allowlist(dir.path(), &rule).unwrap();
        assert!(cfg.command_type_allowlist.is_empty());
    }

    #[test]
    fn permission_settings_default_to_workspace_user_approval() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load_permission_settings(dir.path());
        assert_eq!(loaded.source, PermissionConfigSource::Default);
        assert_eq!(loaded.selection, SessionPermissions::ask_for_approval());
        assert!(loaded.diagnostics.is_empty());
    }

    #[test]
    fn canonical_approval_fields_apply_without_custom_profiles() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            r#""approval_policy" = "on-request"
"approvals_reviewer" = "auto_review"
"#,
        )
        .unwrap();

        let loaded = load_permission_settings(dir.path());

        assert_eq!(loaded.selection.profile_id, types::WORKSPACE_PROFILE);
        assert_eq!(loaded.selection.approval_policy, ApprovalPolicy::OnRequest);
        assert_eq!(
            loaded.selection.approvals_reviewer,
            ApprovalsReviewer::AutoReview
        );
    }

    #[test]
    fn permission_preset_roundtrips_and_preserves_unrelated_config() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            r#""memory" = { "memory_char_limit" = 41 }
"permissions" = { "profiles" = {  } }
"#,
        )
        .unwrap();
        let loaded = set_permission_preset(dir.path(), PermissionPreset::ApproveForMe).unwrap();
        assert_eq!(loaded.selection, SessionPermissions::approve_for_me());
        assert_eq!(load_memory_config(dir.path()).memory_char_limit, 41);

        let loaded = set_permission_preset(dir.path(), PermissionPreset::FullAccess).unwrap();
        assert_eq!(loaded.selection, SessionPermissions::full_access());
    }

    #[test]
    fn loaded_write_roots_drop_paths_the_settings_page_would_reject() {
        let dir = tempfile::tempdir().unwrap();
        let inside_astro = dir.path().join("workspace-out");
        std::fs::create_dir_all(&inside_astro).unwrap();
        let mut rejected = format!("\"{}\"", inside_astro.display());
        if let Some(home) = std::env::var_os("HOME") {
            rejected.push_str(&format!(
                ", \"{}\"",
                std::path::Path::new(&home).join(".ssh").display()
            ));
        }
        fs::write(
            dir.path().join("config.toml"),
            format!(
                r#""permissions" = {{ "default_profile" = ":workspace", "extra_writable_roots" = [{rejected}], "profiles" = {{ "writer" = {{ "extends" = ":workspace", "extra_writable_roots" = ["/tmp/astro-kept-out"] }} }} }}
"#
            ),
        )
        .unwrap();

        let loaded = load_permission_settings(dir.path());
        // 手写配置不能绕过护栏：Astro 自身目录 / 敏感目录在加载时就被丢掉。
        assert!(loaded.permissions.extra_writable_roots.is_empty());
        assert!(loaded
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == PermissionDiagnosticCode::DroppedWriteRoot));
        // 合法条目（用户级 + profile 自带）继续生效。
        assert_eq!(
            loaded.permissions.extra_writable_roots_for("writer"),
            vec![std::path::PathBuf::from("/tmp/astro-kept-out")]
        );
        // 只影响运行时有效范围，不改写用户的配置文件。
        let raw = fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(raw.contains("workspace-out"), "{raw}");
    }

    #[test]
    fn extra_write_roots_roundtrip_and_sanitize() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            r#""permissions" = { "default_profile" = ":workspace" }
"#,
        )
        .unwrap();

        let loaded =
            set_extra_write_roots(dir.path(), &["/tmp/astro-writer-out".to_string()]).unwrap();
        assert_eq!(
            loaded.permissions.extra_writable_roots_for(":workspace"),
            vec![std::path::PathBuf::from("/tmp/astro-writer-out")]
        );
        // 内置组合（default_profile 未变）也被写入用户级列表。
        assert_eq!(
            loaded.permissions.default_profile, ":workspace",
            "不应改动 default_profile"
        );

        // 相对路径 / Astro 自身目录一律拒绝。
        assert!(set_extra_write_roots(dir.path(), &["relative/out".into()]).is_err());
        let inside_home = dir.path().join("cache").display().to_string();
        assert!(set_extra_write_roots(dir.path(), &[inside_home]).is_err());

        // 清空后字段被移除，回到 profile 自身边界。
        let cleared = set_extra_write_roots(dir.path(), &[]).unwrap();
        assert!(cleared.permissions.extra_writable_roots.is_empty());
        assert!(
            cleared
                .permissions
                .extra_writable_roots_for(":workspace")
                .is_empty()
        );
        // 失败写入不能把之前的值留在文件里。
        assert!(!fs::read_to_string(dir.path().join("config.toml"))
            .unwrap()
            .contains("extra_writable_roots"));
    }

    #[test]
    fn permission_preset_changes_are_visible_on_the_next_load() {
        let dir = tempfile::tempdir().unwrap();
        set_permission_preset(dir.path(), PermissionPreset::ApproveForMe).unwrap();
        assert_eq!(
            load_permission_settings(dir.path()).selection,
            SessionPermissions::approve_for_me()
        );

        set_permission_preset(dir.path(), PermissionPreset::AskForApproval).unwrap();
        assert_eq!(
            load_permission_settings(dir.path()).selection,
            SessionPermissions::ask_for_approval()
        );
    }

    #[test]
    fn removed_approval_mode_is_not_migrated() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            r#""approvals" = { "mode" = "smart", "command_allowlist" = ["git status"] }
"#,
        )
        .unwrap();
        let loaded = load_permission_settings(dir.path());
        assert_eq!(loaded.source, PermissionConfigSource::Default);
        assert_eq!(loaded.selection, SessionPermissions::ask_for_approval());
        assert!(loaded.command_allowlist.is_empty());
    }

    #[test]
    fn explicit_profiles_keep_approval_dimensions_separate() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            r#""permissions" = { "default_profile" = "project-edit", "profiles" = { "project-edit" = { "extends" = ":workspace", "filesystem" = { "workspace_roots" = { "**/*.env" = "deny" } } } } }
"approval_policy" = "on-request"
"approvals_reviewer" = "auto_review"
"network_proxy" = { "enabled" = false }
"#,
        )
        .unwrap();
        let loaded = load_permission_settings(dir.path());
        assert_eq!(loaded.source, PermissionConfigSource::ExplicitProfiles);
        assert_eq!(loaded.selection.profile_id, "project-edit");
        assert_eq!(loaded.selection.approval_policy, ApprovalPolicy::OnRequest);
        assert_eq!(
            loaded.selection.approvals_reviewer,
            ApprovalsReviewer::AutoReview
        );
        assert!(loaded.diagnostics.is_empty());
    }

    #[test]
    fn invalid_profiles_and_unenforced_domain_rules_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            r#""permissions" = { "default_profile" = "a", "profiles" = { "a" = { "extends" = "b" }, "b" = { "extends" = "a" } } }
"#,
        )
        .unwrap();
        let invalid = load_permission_settings(dir.path());
        assert_eq!(invalid.source, PermissionConfigSource::Default);
        assert_eq!(invalid.selection, SessionPermissions::ask_for_approval());
        assert!(invalid
            .diagnostics
            .iter()
            .any(|item| { item.code == PermissionDiagnosticCode::InvalidProfileConfig }));

        fs::write(
            dir.path().join("config.toml"),
            r#""permissions" = { "default_profile" = "project-net", "profiles" = { "project-net" = { "extends" = ":workspace", "network" = { "enabled" = true, "domains" = { "api.openai.com" = "allow" } } } } }
"network_proxy" = { "enabled" = false }
"#,
        )
        .unwrap();
        let no_proxy = load_permission_settings(dir.path());
        assert_eq!(no_proxy.source, PermissionConfigSource::Default);
        assert!(no_proxy
            .diagnostics
            .iter()
            .any(|item| { item.code == PermissionDiagnosticCode::DomainRulesWithoutProxy }));
    }

    #[test]
    fn network_header_injections_parse_without_exposing_values_in_debug() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            r#""permissions" = { "default_profile" = "project-net", "profiles" = { "project-net" = { "extends" = ":workspace", "network" = { "enabled" = true, "domains" = { "api.example.com" = "allow" }, "header_injections" = [{ "host" = "api.example.com", "methods" = ["POST"], "path_prefixes" = ["/console/v1"], "headers" = { "x-managed-source" = "secret-value" } }] } } } }
"network_proxy" = { "enabled" = true }
"#,
        )
        .unwrap();

        let loaded = load_permission_settings(dir.path());
        let rules = &loaded.permissions.profiles["project-net"]
            .network
            .header_injections;
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].host, "api.example.com");
        assert_eq!(rules[0].methods, ["POST"]);
        assert_eq!(rules[0].path_prefixes, ["/console/v1"]);
        assert_eq!(rules[0].headers["x-managed-source"], "secret-value");
        let debug = format!("{:?}", rules[0]);
        assert!(debug.contains("x-managed-source"));
        assert!(!debug.contains("secret-value"));
    }

    #[test]
    fn removed_sandbox_keys_do_not_change_explicit_profiles() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            r#""permissions" = { "default_profile" = ":read-only" }
"sandbox_mode" = "danger-full-access"
"#,
        )
        .unwrap();
        let loaded = load_permission_settings(dir.path());
        assert_eq!(loaded.selection, SessionPermissions::read_only());
        assert!(loaded.diagnostics.is_empty());
    }

    #[test]
    fn append_allowlist_dedups_and_preserves() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            r#""memory" = { "memory_char_limit" = 42 }
"#,
        )
        .unwrap();
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
    fn set_route_preserves_unrelated_toml() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            r#""hooks" = { "enabled" = true }
"memory" = { "write_approval" = true }
"#,
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
        let text = fs::read_to_string(dir.path().join("config.toml")).unwrap();
        let parsed: toml::Value = text.parse().unwrap();
        assert_eq!(parsed["hooks"]["enabled"].as_bool(), Some(true));
        assert_eq!(parsed["memory"]["write_approval"].as_bool(), Some(true));
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
