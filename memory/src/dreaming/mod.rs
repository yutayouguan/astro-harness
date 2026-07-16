//! 入梦提炼：将每日日记凝练进各 Agent 的 `MEMORY.md`。
//!
//! 职责：
//! - 维护全局入梦状态 `dreaming.json`（开关、统计、每 Agent 已处理日期）
//! - 筛选未入梦的日记忆、构建 LLM 提示词与 Extractor 输入
//! - 将模型输出写回 `MEMORY.md` 并更新统计（积分、新增要点数）
//!
//! 不变量：
//! - 单次最多处理 [`MAX_DIARIES_PER_RUN`] 天日记，总字符不超过 [`MAX_DIARY_CHARS`]
//! - 已记入 `dreamed_dates` 的日期不会重复提炼
//! - 实际 LLM 调用由 Tauri / backend 注入；本模块不直接请求模型

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use home::{
    agent_workspace_dir, daily_memory_path, list_agents, list_daily_memory_dates, AgentInfo,
};
use crate::config::load_memory_config;
use crate::{parse_memory_entries, MemoryStore};

/// 入梦全局状态文件：`{base}/dreaming.json`
const STATE_FILE: &str = "dreaming.json";
/// 单次最多处理的日记天数
const MAX_DIARIES_PER_RUN: usize = 7;
/// 送入模型的日记总字符上限（粗略）
const MAX_DIARY_CHARS: usize = 28_000;

/// 入梦功能的全局状态（持久化于 `dreaming.json`）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DreamingState {
    /// 是否启用自动入梦
    #[serde(default)]
    pub enabled: bool,
    /// 最近一次修改开关或配置的时间
    #[serde(default)]
    pub updated_at: Option<String>,
    /// 全局最近一次成功运行时间
    #[serde(default)]
    pub last_run_at: Option<String>,
    /// 全局最近一次错误信息
    #[serde(default)]
    pub last_error: Option<String>,
    /// 是否有入梦任务正在执行
    #[serde(default)]
    pub running: bool,
    /// 累计消耗「积分」（字符量估算）
    #[serde(default)]
    pub total_points: u64,
    /// 累计成功提炼次数
    #[serde(default)]
    pub total_summaries: u64,
    /// 按 Agent id 索引的入梦统计
    #[serde(default)]
    pub agents: HashMap<String, AgentDreamStats>,
}

/// 单个 Agent 的入梦进度与统计
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentDreamStats {
    /// 已完成入梦的日期列表（`YYYY-MM-DD`）
    #[serde(default)]
    pub dreamed_dates: Vec<String>,
    /// 该 Agent 累计积分
    #[serde(default)]
    pub points: u64,
    /// 累计新增记忆要点条数
    #[serde(default)]
    pub new_memories: u64,
    /// 该 Agent 最近一次入梦时间
    #[serde(default)]
    pub last_run_at: Option<String>,
    /// 该 Agent 最近一次错误
    #[serde(default)]
    pub last_error: Option<String>,
}

/// 单个 Agent 的一次入梦任务载荷（含提示词与待处理日记）
#[derive(Debug, Clone)]
pub struct DreamJob {
    pub agent_id: String,
    pub agent_name: String,
    /// 该 Agent 工作区根目录
    pub workspace: PathBuf,
    /// 入梦前的 `MEMORY.md` 原文（用于对比新增条数）
    pub memory_before: String,
    pub diaries: Vec<DreamDiary>,
    /// 发给模型的 system 角色提示
    pub system_prompt: String,
    /// 发给模型的 user 角色提示（含当前 MEMORY 与日记）
    pub user_prompt: String,
}

/// 待提炼的一条日记忆
#[derive(Debug, Clone)]
pub struct DreamDiary {
    /// 日期 `YYYY-MM-DD`
    pub date: String,
    /// 日记正文（已 trim，非空）
    pub content: String,
}

/// 入梦结构化抽取目标（经 Extractor `submit` 提交）。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DreamMemoryUpdate {
    /// 更新后的完整 MEMORY.md 正文（markdown，不要代码围栏）
    pub memory_markdown: String,
    /// 可选：本次提炼的简短说明
    #[serde(default)]
    pub change_summary: Option<String>,
}

/// 一次入梦批次的汇总报告（多 Agent）
#[derive(Debug, Clone, Serialize)]
pub struct DreamRunReport {
    /// 是否全部 Agent 均成功
    pub ok: bool,
    pub agents_processed: usize,
    pub diaries_processed: usize,
    pub total_summaries: u64,
    pub total_points: u64,
    pub last_error: Option<String>,
    pub agents: Vec<DreamAgentReport>,
}

/// 单个 Agent 入梦结果摘要
#[derive(Debug, Clone, Serialize)]
pub struct DreamAgentReport {
    pub agent_id: String,
    pub agent_name: String,
    /// 本次处理的日记条数
    pub diaries: usize,
    /// 本次估算新增记忆要点数
    pub new_memories: u64,
    /// 本次消耗积分
    pub points: u64,
    pub error: Option<String>,
}

/// `dreaming.json` 的绝对路径
pub fn dreaming_state_path(base: &Path) -> PathBuf {
    base.join(STATE_FILE)
}

/// 加载入梦状态；文件缺失或解析失败返回默认值
pub fn load_dreaming_state(base: &Path) -> DreamingState {
    let path = dreaming_state_path(base);
    let Ok(raw) = fs::read_to_string(&path) else {
        return DreamingState::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// 原子写入入梦状态（`.json.tmp` → rename）
pub fn save_dreaming_state(base: &Path, state: &DreamingState) -> anyhow::Result<()> {
    fs::create_dir_all(base)?;
    let path = dreaming_state_path(base);
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(state)?;
    fs::write(&tmp, raw)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// 切换入梦开关；关闭时同时清除 `running` 标记
pub fn set_dreaming_enabled(base: &Path, enabled: bool) -> anyhow::Result<DreamingState> {
    let mut state = load_dreaming_state(base);
    state.enabled = enabled;
    state.updated_at = Some(Utc::now().to_rfc3339());
    if !enabled {
        state.running = false;
    }
    save_dreaming_state(base, &state)?;
    Ok(state)
}

/// 读取文本文件，失败返回空串
fn read_text(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

/// 选出尚未入梦的日记（新→旧，受数量与字符上限约束）
pub fn select_undreamed_diaries(
    workspace: &Path,
    stats: &AgentDreamStats,
) -> Vec<DreamDiary> {
    let dreamed: std::collections::HashSet<&str> =
        stats.dreamed_dates.iter().map(|s| s.as_str()).collect();
    let mut out = Vec::new();
    let mut chars = 0usize;
    for date in list_daily_memory_dates(workspace) {
        if dreamed.contains(date.as_str()) {
            continue;
        }
        let path = daily_memory_path(workspace, &date);
        let content = read_text(&path);
        let trimmed = content.trim();
        if trimmed.is_empty() {
            continue;
        }
        if out.len() >= MAX_DIARIES_PER_RUN {
            break;
        }
        if chars + trimmed.len() > MAX_DIARY_CHARS && !out.is_empty() {
            break;
        }
        chars += trimmed.len();
        out.push(DreamDiary {
            date,
            content: trimmed.to_string(),
        });
    }
    out
}

/// 构建完整对话用的 system / user 提示词（非 Extractor 路径）
pub fn build_dream_prompts(agent_name: &str, memory: &str, diaries: &[DreamDiary]) -> (String, String) {
    let system = format!(
        r#"你是 Astro 的「入梦」记忆提炼助手，正在为专家「{agent_name}」整理长期记忆。

任务：根据现有 MEMORY.md 与近期日记，输出一份更新后的完整 MEMORY.md。

规则：
1. 只保留长期有用的事实、偏好、决策、项目结论、人际关系要点。
2. 合并日记中的新信息；去掉闲聊、临时状态、重复内容。
3. 使用简洁中文（或原文语言）要点列表，每条以 "- " 开头。
4. 可按主题分组（用 ## 小标题），但不要写开场白或解释。
5. 不要编造日记中没有的信息。
6. 只输出 MEMORY.md 正文本身，不要用 markdown 代码围栏包裹。"#
    );

    let mut user = String::new();
    user.push_str("# 当前 MEMORY.md\n\n");
    if memory.trim().is_empty() {
        user.push_str("（空）\n\n");
    } else {
        user.push_str(memory.trim());
        user.push_str("\n\n");
    }
    user.push_str("# 待提炼日记\n\n");
    for d in diaries {
        user.push_str(&format!("## {}\n\n{}\n\n", d.date, d.content));
    }
    user.push_str("请输出更新后的完整 MEMORY.md：\n");
    (system, user)
}

/// Extractor 用的 preamble（规则）与待抽取原文。
pub fn build_dream_extract_inputs(
    agent_name: &str,
    memory: &str,
    diaries: &[DreamDiary],
) -> (String, String) {
    let preamble = format!(
        r#"为专家「{agent_name}」提炼长期记忆。
规则：
1. 只保留长期有用的事实、偏好、决策、项目结论、人际关系要点。
2. 合并日记中的新信息；去掉闲聊、临时状态、重复内容。
3. memory_markdown 使用简洁要点列表，每条以 "- " 开头；可按主题用 ## 分组。
4. 不要编造日记中没有的信息。
5. memory_markdown 不要用 markdown 代码围栏包裹。
6. change_summary 可选，一句话说明本次变更。"#
    );

    let mut text = String::new();
    text.push_str("# 当前 MEMORY.md\n\n");
    if memory.trim().is_empty() {
        text.push_str("（空）\n\n");
    } else {
        text.push_str(memory.trim());
        text.push_str("\n\n");
    }
    text.push_str("# 待提炼日记\n\n");
    for d in diaries {
        text.push_str(&format!("## {}\n\n{}\n\n", d.date, d.content));
    }
    (preamble, text)
}

/// 去掉模型偶发包裹的 ``` 围栏
pub fn sanitize_memory_output(raw: &str) -> String {
    let mut s = raw.trim().to_string();
    if s.starts_with("```") {
        if let Some(rest) = s.strip_prefix("```") {
            let rest = rest.strip_prefix("markdown").or_else(|| rest.strip_prefix("md")).unwrap_or(rest);
            let rest = rest.trim_start_matches('\n');
            if let Some(end) = rest.rfind("```") {
                s = rest[..end].trim().to_string();
            } else {
                s = rest.trim().to_string();
            }
        }
    }
    s
}

/// 粗略估算「积分」：按输入+输出字符 / 1000
pub fn estimate_points(input_chars: usize, output_chars: usize) -> u64 {
    ((input_chars + output_chars) as u64).div_ceil(1000).max(1)
}

/// 统计 MEMORY 要点条数；兼容 `§` 分隔与遗留 `- `/`* ` 列表。
pub fn count_memory_bullets(content: &str) -> u64 {
    parse_memory_entries(content).len() as u64
}

/// 为单个 Agent 准备入梦任务；无新日记则返回 None
pub fn prepare_dream_job(base: &Path, agent: &AgentInfo, state: &DreamingState) -> Option<DreamJob> {
    let ws = agent_workspace_dir(base, &agent.id);
    let stats = state.agents.get(&agent.id).cloned().unwrap_or_default();
    let diaries = select_undreamed_diaries(&ws, &stats);
    if diaries.is_empty() {
        return None;
    }
    let memory_path = ws.join("MEMORY.md");
    let memory_before = read_text(&memory_path);
    let (system_prompt, user_prompt) =
        build_dream_prompts(&agent.name, &memory_before, &diaries);
    Some(DreamJob {
        agent_id: agent.id.clone(),
        agent_name: agent.name.clone(),
        workspace: ws,
        memory_before,
        diaries,
        system_prompt,
        user_prompt,
    })
}

/// 为所有 Agent 收集有待处理日记的入梦任务
pub fn prepare_all_dream_jobs(base: &Path, state: &DreamingState) -> Vec<DreamJob> {
    list_agents(base)
        .into_iter()
        .filter_map(|a| prepare_dream_job(base, &a, state))
        .collect()
}

/// 将模型结果写回 MEMORY.md，并更新该 Agent 的入梦统计
pub fn finalize_dream_job(
    state: &mut DreamingState,
    job: &DreamJob,
    model_output: &str,
) -> anyhow::Result<DreamAgentReport> {
    let cleaned = sanitize_memory_output(model_output);
    finalize_dream_job_with_memory(state, job, &cleaned)
}

/// 将结构化抽取结果写回 MEMORY.md
pub fn finalize_dream_job_from_update(
    state: &mut DreamingState,
    job: &DreamJob,
    update: &DreamMemoryUpdate,
) -> anyhow::Result<DreamAgentReport> {
    let cleaned = sanitize_memory_output(&update.memory_markdown);
    finalize_dream_job_with_memory(state, job, &cleaned)
}

/// 将清洗后的 MEMORY 经 MemoryStore 写盘并更新全局/Agent 统计（内部共用）。
///
/// 超限 / 扫描失败直接返回 Err，不做静默截断。不触碰任何 AgentLoop snapshot。
///
/// 当 `write_approval` 开启时：扫描通过后入 pending（`action=replace_all`），**不**改 live MEMORY；
/// 仍更新 dreamed_dates / 统计，避免重复入梦。
fn finalize_dream_job_with_memory(
    state: &mut DreamingState,
    job: &DreamJob,
    cleaned: &str,
) -> anyhow::Result<DreamAgentReport> {
    if cleaned.trim().is_empty() {
        anyhow::bail!("模型返回空内容");
    }
    let entries = parse_memory_entries(cleaned);
    if entries.is_empty() {
        anyhow::bail!("未能从 MEMORY 输出中解析出任何条目");
    }

    let base = job
        .workspace
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| job.workspace.clone());
    let cfg = load_memory_config(&base);
    let memory_path = job.workspace.join("MEMORY.md");

    if cfg.write_approval {
        // 超限也要在 finalize 失败（与直写路径一致）；扫描在 enqueue 内完成
        let used = entries
            .join(crate::agent::store::ENTRY_DELIMITER)
            .chars()
            .count();
        if used > cfg.memory_char_limit {
            anyhow::bail!(
                "记忆内容超过字符上限（{used}/{}）；无法入队审批",
                cfg.memory_char_limit
            );
        }
        crate::pending::enqueue(
            &base,
            crate::pending::PendingMemoryWrite {
                id: String::new(),
                agent_id: job.agent_id.clone(),
                target: crate::MemoryTarget::Memory,
                action: "replace_all".into(),
                content: Some(cleaned.to_string()),
                old_text: None,
                source: "dreaming".into(),
                created_at: String::new(),
            },
        )?;
    } else {
        let mut store = MemoryStore::open(memory_path, cfg.memory_char_limit)?;
        store.replace_all_entries(entries)?;
    }

    let before_n = count_memory_bullets(&job.memory_before);
    let after_n = count_memory_bullets(cleaned);
    let new_memories = after_n.saturating_sub(before_n).max(1);
    let input_chars = job.system_prompt.len() + job.user_prompt.len();
    let points = estimate_points(input_chars, cleaned.len());

    let entry = state.agents.entry(job.agent_id.clone()).or_default();
    for d in &job.diaries {
        if !entry.dreamed_dates.iter().any(|x| x == &d.date) {
            entry.dreamed_dates.push(d.date.clone());
        }
    }
    entry.dreamed_dates.sort();
    entry.dreamed_dates.dedup();
    entry.dreamed_dates.sort_by(|a, b| b.cmp(a));
    entry.points = entry.points.saturating_add(points);
    entry.new_memories = entry.new_memories.saturating_add(new_memories);
    entry.last_run_at = Some(Utc::now().to_rfc3339());
    entry.last_error = None;

    state.total_points = state.total_points.saturating_add(points);
    state.total_summaries = state.total_summaries.saturating_add(1);

    Ok(DreamAgentReport {
        agent_id: job.agent_id.clone(),
        agent_name: job.agent_name.clone(),
        diaries: job.diaries.len(),
        new_memories,
        points,
        error: None,
    })
}

/// 记录某 Agent 入梦失败到状态（不抛错）
pub fn mark_agent_dream_error(state: &mut DreamingState, agent_id: &str, err: &str) {
    let entry = state.agents.entry(agent_id.to_string()).or_default();
    entry.last_error = Some(err.to_string());
    entry.last_run_at = Some(Utc::now().to_rfc3339());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::ensure_workspace;
    use home::{agent_workspace_dir, create_agent, daily_memory_path};
    use tempfile::tempdir;

    #[test]
    fn sanitize_strips_fences() {
        let raw = "```markdown\n- a\n- b\n```";
        assert_eq!(sanitize_memory_output(raw), "- a\n- b");
    }

    #[test]
    fn select_skips_dreamed_and_empty() {
        let dir = tempdir().unwrap();
        ensure_workspace(dir.path()).unwrap();
        let agent = create_agent(dir.path(), "Tester").unwrap();
        let ws = agent_workspace_dir(dir.path(), &agent.id);
        let d1 = daily_memory_path(&ws, "2026-07-10");
        let d2 = daily_memory_path(&ws, "2026-07-11");
        fs::create_dir_all(d1.parent().unwrap()).unwrap();
        fs::write(&d1, "- day1 note\n").unwrap();
        fs::write(&d2, "   \n").unwrap();
        let mut stats = AgentDreamStats::default();
        let all = select_undreamed_diaries(&ws, &stats);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].date, "2026-07-10");
        stats.dreamed_dates.push("2026-07-10".into());
        assert!(select_undreamed_diaries(&ws, &stats).is_empty());
    }

    #[test]
    fn finalize_writes_memory_and_marks_dates() {
        let dir = tempdir().unwrap();
        ensure_workspace(dir.path()).unwrap();
        let agent = create_agent(dir.path(), "Bot").unwrap();
        let ws = agent_workspace_dir(dir.path(), &agent.id);
        let diary = daily_memory_path(&ws, "2026-07-09");
        fs::create_dir_all(diary.parent().unwrap()).unwrap();
        fs::write(&diary, "- met Alice\n").unwrap();
        let mut state = DreamingState::default();
        let job = prepare_dream_job(dir.path(), &agent, &state).unwrap();
        let report = finalize_dream_job(&mut state, &job, "- Alice is a collaborator\n").unwrap();
        assert_eq!(report.diaries, 1);
        let mem = fs::read_to_string(ws.join("MEMORY.md")).unwrap();
        assert!(mem.contains("Alice"));
        assert!(
            mem.contains('§') || mem == "Alice is a collaborator",
            "dreaming writeback should go through MemoryStore § format; got: {mem}"
        );
        assert!(state.agents[&agent.id]
            .dreamed_dates
            .iter()
            .any(|d| d == "2026-07-09"));
    }

    #[test]
    fn finalize_from_structured_update() {
        let dir = tempdir().unwrap();
        ensure_workspace(dir.path()).unwrap();
        let agent = create_agent(dir.path(), "Bot").unwrap();
        let ws = agent_workspace_dir(dir.path(), &agent.id);
        let diary = daily_memory_path(&ws, "2026-07-08");
        fs::create_dir_all(diary.parent().unwrap()).unwrap();
        fs::write(&diary, "- prefer dark mode\n").unwrap();
        let mut state = DreamingState::default();
        let job = prepare_dream_job(dir.path(), &agent, &state).unwrap();
        let update = DreamMemoryUpdate {
            memory_markdown: "## Prefs\n- prefers dark mode\n".into(),
            change_summary: Some("added preference".into()),
        };
        let report = finalize_dream_job_from_update(&mut state, &job, &update).unwrap();
        assert_eq!(report.diaries, 1);
        let mem = fs::read_to_string(ws.join("MEMORY.md")).unwrap();
        assert!(mem.contains("dark mode"));
        assert!(state.agents[&agent.id]
            .dreamed_dates
            .iter()
            .any(|d| d == "2026-07-08"));
    }

    #[test]
    fn finalize_over_limit_fails_without_silent_truncate() {
        let dir = tempdir().unwrap();
        ensure_workspace(dir.path()).unwrap();
        // Tiny MEMORY limit so finalize must fail instead of truncating.
        fs::write(
            dir.path().join("config.yaml"),
            "memory:\n  memory_char_limit: 30\n",
        )
        .unwrap();
        let agent = create_agent(dir.path(), "Bot").unwrap();
        let ws = agent_workspace_dir(dir.path(), &agent.id);
        let diary = daily_memory_path(&ws, "2026-07-07");
        fs::create_dir_all(diary.parent().unwrap()).unwrap();
        fs::write(&diary, "- seed\n").unwrap();
        fs::write(ws.join("MEMORY.md"), "seed note").unwrap();
        let before = fs::read_to_string(ws.join("MEMORY.md")).unwrap();
        let mut state = DreamingState::default();
        let job = prepare_dream_job(dir.path(), &agent, &state).unwrap();
        let err = finalize_dream_job(
            &mut state,
            &job,
            "- this drafted memory entry is intentionally far too long for the tiny limit\n",
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("limit") || err.to_string().contains("上限"),
            "unexpected error: {err}"
        );
        let after = fs::read_to_string(ws.join("MEMORY.md")).unwrap();
        assert_eq!(after, before);
        assert!(!state.agents.contains_key(&agent.id) || state.agents[&agent.id].dreamed_dates.is_empty());
    }

    #[test]
    fn count_memory_bullets_counts_section_entries() {
        assert_eq!(count_memory_bullets("- a\n- b\n"), 2);
        assert_eq!(count_memory_bullets("a\n§\nb\n§\nc"), 3);
    }
}
