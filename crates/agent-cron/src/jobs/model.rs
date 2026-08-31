//! 定时任务数据模型与抽取辅助。

use chrono::Local;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::schedule::compute_next_run;

/// 定时任务定义，持久化在 `~/.astro/cron/jobs.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    /// 唯一 id（UUID 字符串）
    pub id: String,
    /// 调度表达式：`every:5m` / `every:1h` / 五段 cron（分 时 日 月 周）
    pub schedule: String,
    /// 到期时交给 Agent 执行的完整指令
    pub task: String,
    /// 短标题（UI 展示；缺省由 task 首行截取）
    #[serde(default)]
    pub title: String,
    /// 执行该任务的 Agent id
    #[serde(default = "default_agent_id")]
    pub agent_id: String,
    /// 可选：覆盖 Agent 默认 Provider
    #[serde(default)]
    pub provider_id: Option<String>,
    /// 可选：覆盖 Agent 默认模型
    #[serde(default)]
    pub model: Option<String>,
    /// 是否参与调度扫描
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// ISO 8601 创建时间
    pub created_at: String,
    /// 上次实际触发时间（手动 touch 或 claim_due）
    #[serde(default)]
    pub last_run_at: Option<String>,
    /// 下次计划触发时间
    #[serde(default)]
    pub next_run_at: Option<String>,
    /// 是否在聊天时间线展示本次执行
    #[serde(default)]
    pub show_in_chat: bool,
}

/// 创建或更新定时任务的输入（供 Tauri / 上层调用）
#[derive(Debug, Clone)]
pub struct NewCronJob {
    /// 调度表达式（同 [`CronJob::schedule`]）
    pub schedule: String,
    /// 到期执行的指令
    pub task: String,
    /// 短标题
    pub title: String,
    /// 目标 Agent id
    pub agent_id: String,
    /// 可选 Provider 覆盖
    pub provider_id: Option<String>,
    /// 可选模型覆盖
    pub model: Option<String>,
    /// 是否在聊天中展示
    pub show_in_chat: bool,
}

/// 自然语言 → 定时任务的结构化抽取目标（Extractor `submit`）。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CronJobExtract {
    /// `every:5m` / `every:1h` / 五段 cron
    pub schedule: String,
    /// 到期时交给 Agent 执行的指令
    pub task: String,
    /// 可选短标题；缺省由 task 首行截取
    #[serde(default)]
    pub title: Option<String>,
}

/// Extractor preamble：约束 schedule 语法。
pub fn cron_extract_preamble() -> &'static str {
    r#"从用户自然语言提炼一条定时任务。
规则：
1. schedule 必须是下列之一：
   - every:Nm / every:Nh / every:Nd（N 为正整数；可选 ;wd=1,2,3 限定周几，0=周日）
   - 五段 cron：分 时 日 月 周（例如每天 09:00 → 0 9 * * *）
2. task 是到期时要执行的完整指令，保留用户意图，不要空。
3. title 可选，简短中文标题；不确定时可省略。
4. 不要编造用户没说的调度细节；缺省时间可用每天 09:00。"#
}

/// 校验并规范化抽取结果（trim、补 title、验证 schedule 可解析）。
pub fn normalize_cron_extract(mut draft: CronJobExtract) -> anyhow::Result<CronJobExtract> {
    draft.schedule = draft.schedule.trim().to_string();
    draft.task = draft.task.trim().to_string();
    if let Some(t) = draft.title.as_mut() {
        *t = t.trim().to_string();
        if t.is_empty() {
            draft.title = None;
        }
    }
    if draft.task.is_empty() {
        anyhow::bail!("task 为空");
    }
    if draft.schedule.is_empty() {
        anyhow::bail!("schedule 为空");
    }
    let _ = compute_next_run(&draft.schedule, Local::now())?;
    if draft.title.is_none() {
        draft.title = Some(title_from_task(&draft.task));
    }
    Ok(draft)
}

pub(crate) fn default_true() -> bool {
    true
}

/// 缺省 agent_id（默认工作区）。
pub(crate) fn default_agent_id() -> String {
    home::DEFAULT_AGENT_ID.to_string()
}

/// 规范化 cron 任务的 agent_id。
///
/// - 空 / `"default"` → [`home::DEFAULT_AGENT_ID`]（`workspace`）
/// - 其余走 [`home::normalize_agent_id`]
///
/// 旧版 `jobs.json` 与部分工具路径曾写入 `"default"`；执行侧必须映射到真实默认工作区，
/// 否则会落到 `workspace-default/` 并静默建仓。
pub fn normalize_cron_agent_id(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("default") {
        home::DEFAULT_AGENT_ID.to_string()
    } else {
        home::normalize_agent_id(t)
    }
}

/// 从 task 首行截取最多 40 字符作为默认标题
pub(crate) fn title_from_task(task: &str) -> String {
    task.lines()
        .next()
        .unwrap_or(task)
        .chars()
        .take(40)
        .collect()
}
