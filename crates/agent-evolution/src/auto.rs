//! 自动触发进化的成本护栏与运行状态。
//!
//! 状态文件：`{base}/evolution/auto_state.json`。
//! 本模块只做「能不能跑 / 记一次跑完」的纯逻辑；真正调用模型由 Tauri 侧负责。
//! 自动路径**只允许单轮 reflect**，绝不跑遗传搜索 / DSPy，也绝不自动写入技能。

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use memory::EvolutionAuto;
use serde::{Deserialize, Serialize};

/// 跳过原因（可展示给 UI / 日志）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// `evolution.enabled` 关闭。
    EvolutionDisabled,
    /// `evolution.auto.enabled` 关闭。
    AutoDisabled,
    /// 距上次运行不足冷却时间。
    Cooldown { remaining_secs: u64 },
    /// 今日（UTC）已达次数上限。
    DailyLimit { used: u32, max: u32 },
    /// 自上次运行以来新增 DecisionLog 不足。
    NotEnoughDecisions { have: usize, need: usize },
}

impl SkipReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::EvolutionDisabled => "evolution_disabled",
            Self::AutoDisabled => "auto_disabled",
            Self::Cooldown { .. } => "cooldown",
            Self::DailyLimit { .. } => "daily_limit",
            Self::NotEnoughDecisions { .. } => "not_enough_decisions",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::EvolutionDisabled => "离线进化总开关未开启".into(),
            Self::AutoDisabled => "自动触发未开启".into(),
            Self::Cooldown { remaining_secs } => {
                format!("冷却中，还需约 {remaining_secs} 秒")
            }
            Self::DailyLimit { used, max } => {
                format!("今日已自动运行 {used}/{max} 次")
            }
            Self::NotEnoughDecisions { have, need } => {
                format!("新增决策 {have}/{need}，暂不触发")
            }
        }
    }
}

/// 护栏评估结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoGate {
    Allow,
    Skip(SkipReason),
}

/// 持久化的自动触发状态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct AutoState {
    /// 上次成功触发（或尝试记入）的 UTC RFC3339。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<String>,
    /// 上次触发时看到的最新 DecisionLog id（水位）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_decision_id: Option<String>,
    /// 今日（UTC `YYYY-MM-DD`）已运行次数。
    #[serde(default)]
    pub runs_today: u32,
    /// 与 `runs_today` 对应的 UTC 日期。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs_today_date: Option<String>,
}

/// `{base}/evolution/auto_state.json`
pub fn auto_state_path(base: &Path) -> PathBuf {
    home::evolution_dir(base).join("auto_state.json")
}

/// 读取状态；缺失或损坏时返回默认。
pub fn load_auto_state(base: &Path) -> AutoState {
    let path = auto_state_path(base);
    if !path.is_file() {
        return AutoState::default();
    }
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => AutoState::default(),
    }
}

/// 原子写回状态。
pub fn save_auto_state(base: &Path, state: &AutoState) -> anyhow::Result<()> {
    let path = auto_state_path(base);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(state)?;
    fs::write(&tmp, text)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

fn utc_today() -> String {
    Utc::now().format("%Y-%m-%d").to_string()
}

/// 将状态中的「今日计数」滚到当前 UTC 日。
pub fn normalize_day(state: &mut AutoState) {
    let today = utc_today();
    if state.runs_today_date.as_deref() != Some(today.as_str()) {
        state.runs_today = 0;
        state.runs_today_date = Some(today);
    }
}

/// 统计 DecisionLog 中位于水位之后的条目数。
///
/// `watermark_id` 为 `None` 时，全部算「新」。
pub fn count_new_decisions(
    decisions: &[memory::DecisionEntry],
    watermark_id: Option<&str>,
) -> usize {
    let Some(wid) = watermark_id.filter(|s| !s.is_empty()) else {
        return decisions.len();
    };
    if let Some(pos) = decisions.iter().position(|d| d.id == wid) {
        decisions.len().saturating_sub(pos + 1)
    } else {
        // 水位 id 已不在近期窗口：保守地按「全部都新」计，避免永远卡住。
        decisions.len()
    }
}

/// 评估是否允许自动跑一次。
///
/// `evolution_enabled` = `evolution.enabled`；`auto` = `evolution.auto`；
/// `decisions` 建议传近期窗口（如最近 50 条）。
pub fn evaluate_auto_gate(
    evolution_enabled: bool,
    auto: &EvolutionAuto,
    state: &AutoState,
    decisions: &[memory::DecisionEntry],
    now: DateTime<Utc>,
) -> AutoGate {
    if !evolution_enabled {
        return AutoGate::Skip(SkipReason::EvolutionDisabled);
    }
    if !auto.enabled {
        return AutoGate::Skip(SkipReason::AutoDisabled);
    }

    let mut state = state.clone();
    // 用传入 now 对应的日期滚日（测试可注入）。
    let today = now.format("%Y-%m-%d").to_string();
    if state.runs_today_date.as_deref() != Some(today.as_str()) {
        state.runs_today = 0;
        state.runs_today_date = Some(today);
    }

    let max_runs = auto.max_runs_per_day.max(1);
    if state.runs_today >= max_runs {
        return AutoGate::Skip(SkipReason::DailyLimit {
            used: state.runs_today,
            max: max_runs,
        });
    }

    if let Some(ref last) = state.last_run_at {
        if let Ok(ts) = DateTime::parse_from_rfc3339(last) {
            let elapsed = (now - ts.with_timezone(&Utc)).num_seconds().max(0) as u64;
            let need = auto.cooldown_secs;
            if elapsed < need {
                return AutoGate::Skip(SkipReason::Cooldown {
                    remaining_secs: need - elapsed,
                });
            }
        }
    }

    let have = count_new_decisions(decisions, state.last_decision_id.as_deref());
    let need = auto.min_new_decisions.max(1);
    if have < need {
        return AutoGate::Skip(SkipReason::NotEnoughDecisions { have, need });
    }

    AutoGate::Allow
}

/// 标记一次自动运行完成（成功调用模型后调用；失败也可记，避免热重试烧钱）。
pub fn mark_auto_run(
    state: &mut AutoState,
    now: DateTime<Utc>,
    latest_decision_id: Option<String>,
) {
    let today = now.format("%Y-%m-%d").to_string();
    if state.runs_today_date.as_deref() != Some(today.as_str()) {
        state.runs_today = 0;
        state.runs_today_date = Some(today);
    }
    state.runs_today = state.runs_today.saturating_add(1);
    state.last_run_at = Some(now.to_rfc3339());
    if let Some(id) = latest_decision_id.filter(|s| !s.is_empty()) {
        state.last_decision_id = Some(id);
    }
}

/// UI / 命令侧方便读取的快照。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoStatus {
    pub enabled: bool,
    pub cooldown_secs: u64,
    pub min_new_decisions: usize,
    pub max_runs_per_day: u32,
    pub state: AutoState,
    pub new_decisions: usize,
    pub would_run: bool,
    pub skip_reason: Option<String>,
    pub skip_message: Option<String>,
}

/// 组装状态快照（不触发运行）。
pub fn build_auto_status(
    evolution_enabled: bool,
    auto: &EvolutionAuto,
    state: &AutoState,
    decisions: &[memory::DecisionEntry],
) -> AutoStatus {
    let mut norm = state.clone();
    normalize_day(&mut norm);
    let gate = evaluate_auto_gate(evolution_enabled, auto, &norm, decisions, Utc::now());
    let new_decisions = count_new_decisions(decisions, norm.last_decision_id.as_deref());
    let (would_run, skip_reason, skip_message) = match gate {
        AutoGate::Allow => (true, None, None),
        AutoGate::Skip(r) => (false, Some(r.as_str().to_string()), Some(r.message())),
    };
    AutoStatus {
        enabled: auto.enabled,
        cooldown_secs: auto.cooldown_secs,
        min_new_decisions: auto.min_new_decisions,
        max_runs_per_day: auto.max_runs_per_day,
        state: norm,
        new_decisions,
        would_run,
        skip_reason,
        skip_message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use memory::{DecisionEntry, DecisionKind};

    fn dec(id: &str) -> DecisionEntry {
        DecisionEntry {
            id: id.into(),
            kind: DecisionKind::ToolFailure,
            summary: "x".into(),
            session_id: None,
            tool_name: None,
            created_at: Utc::now().to_rfc3339(),
        }
    }

    fn auto_on() -> EvolutionAuto {
        EvolutionAuto {
            enabled: true,
            cooldown_secs: 3600,
            min_new_decisions: 3,
            max_runs_per_day: 3,
            ..EvolutionAuto::default()
        }
    }

    #[test]
    fn defaults_skip_when_disabled() {
        let gate = evaluate_auto_gate(
            true,
            &EvolutionAuto::default(),
            &AutoState::default(),
            &[dec("a"), dec("b"), dec("c")],
            Utc::now(),
        );
        assert_eq!(gate, AutoGate::Skip(SkipReason::AutoDisabled));
    }

    #[test]
    fn skips_when_evolution_master_off() {
        let gate = evaluate_auto_gate(
            false,
            &auto_on(),
            &AutoState::default(),
            &[dec("a"), dec("b"), dec("c")],
            Utc::now(),
        );
        assert_eq!(gate, AutoGate::Skip(SkipReason::EvolutionDisabled));
    }

    #[test]
    fn allows_when_enough_new_decisions() {
        let decisions = vec![dec("1"), dec("2"), dec("3")];
        let gate = evaluate_auto_gate(
            true,
            &auto_on(),
            &AutoState::default(),
            &decisions,
            Utc::now(),
        );
        assert_eq!(gate, AutoGate::Allow);
    }

    #[test]
    fn respects_cooldown() {
        let now = Utc::now();
        let state = AutoState {
            last_run_at: Some((now - chrono::Duration::seconds(100)).to_rfc3339()),
            last_decision_id: Some("1".into()),
            runs_today: 0,
            runs_today_date: Some(now.format("%Y-%m-%d").to_string()),
        };
        let decisions = vec![dec("1"), dec("2"), dec("3"), dec("4")];
        let gate = evaluate_auto_gate(true, &auto_on(), &state, &decisions, now);
        match gate {
            AutoGate::Skip(SkipReason::Cooldown { remaining_secs }) => {
                assert!(remaining_secs > 0);
            }
            other => panic!("expected cooldown, got {other:?}"),
        }
    }

    #[test]
    fn respects_daily_limit() {
        let now = Utc::now();
        let state = AutoState {
            last_run_at: Some((now - chrono::Duration::hours(2)).to_rfc3339()),
            last_decision_id: None,
            runs_today: 3,
            runs_today_date: Some(now.format("%Y-%m-%d").to_string()),
        };
        let decisions = vec![dec("1"), dec("2"), dec("3")];
        let gate = evaluate_auto_gate(true, &auto_on(), &state, &decisions, now);
        assert_eq!(
            gate,
            AutoGate::Skip(SkipReason::DailyLimit { used: 3, max: 3 })
        );
    }

    #[test]
    fn counts_only_after_watermark() {
        let decisions = vec![dec("a"), dec("b"), dec("c"), dec("d")];
        assert_eq!(count_new_decisions(&decisions, Some("b")), 2);
        assert_eq!(count_new_decisions(&decisions, None), 4);
        assert_eq!(count_new_decisions(&decisions, Some("gone")), 4);
    }

    #[test]
    fn not_enough_decisions_after_watermark() {
        let now = Utc::now();
        let state = AutoState {
            last_run_at: Some((now - chrono::Duration::hours(2)).to_rfc3339()),
            last_decision_id: Some("c".into()),
            runs_today: 0,
            runs_today_date: Some(now.format("%Y-%m-%d").to_string()),
        };
        // 水位后只有 1 条，需要 3
        let decisions = vec![dec("a"), dec("b"), dec("c"), dec("d")];
        let gate = evaluate_auto_gate(true, &auto_on(), &state, &decisions, now);
        assert_eq!(
            gate,
            AutoGate::Skip(SkipReason::NotEnoughDecisions { have: 1, need: 3 })
        );
    }

    #[test]
    fn mark_and_persist_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = AutoState::default();
        let now = Utc::now();
        mark_auto_run(&mut state, now, Some("z".into()));
        save_auto_state(dir.path(), &state).unwrap();
        let loaded = load_auto_state(dir.path());
        assert_eq!(loaded.runs_today, 1);
        assert_eq!(loaded.last_decision_id.as_deref(), Some("z"));
        assert!(loaded.last_run_at.is_some());
    }

    #[test]
    fn day_rollover_resets_counter() {
        let mut state = AutoState {
            runs_today: 3,
            runs_today_date: Some("2000-01-01".into()),
            ..Default::default()
        };
        normalize_day(&mut state);
        assert_eq!(state.runs_today, 0);
        assert_eq!(state.runs_today_date.as_deref(), Some(utc_today().as_str()));
    }
}
