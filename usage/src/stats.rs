//! 按 Agent 聚合的工具集与技能调用计数。
//!
//! 统计数据持久化于 `~/.astro/agents/{id}/usage-stats.json`，供前端展示用量摘要。
//! 工具名经 [`tool_name_to_toolset`](home::tool_name_to_toolset) 归并为工具集计数；
//! `skills` 工具若携带 `skill_id` 参数则额外累加技能维度。

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use home::tool_name_to_toolset;
use home::{
    agent_config_dir, default_memory_dir, ensure_default_workspace_dirs, normalize_agent_id,
    DEFAULT_AGENT_ID,
};

/// 单 Agent 的工具集与技能调用计数快照（可序列化）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentUsageStats {
    /// 工具集 id → 调用次数（与前端 `AGENT_TOOLS` / `tools-enabled` 对齐）。
    #[serde(default)]
    pub tools: HashMap<String, u64>,
    /// 技能名 → 调用次数（仅 `skills` 工具且带 `skill_id` 时累加）。
    #[serde(default)]
    pub skills: HashMap<String, u64>,
}

/// 面向 API 的用量摘要，含合计字段与 Agent 标识。
#[derive(Debug, Clone, Serialize)]
pub struct AgentUsageSummary {
    /// 目标 Agent 标识（已规范化）。
    pub agent_id: String,
    /// 全部工具集调用次数之和。
    pub tool_total: u64,
    /// 全部技能调用次数之和。
    pub skill_total: u64,
    /// 各工具集明细计数。
    pub tools: HashMap<String, u64>,
    /// 各技能明细计数。
    pub skills: HashMap<String, u64>,
}

impl AgentUsageStats {
    /// 返回 `tools` 映射中所有计数的总和。
    pub fn tool_total(&self) -> u64 {
        self.tools.values().copied().sum()
    }

    /// 返回 `skills` 映射中所有计数的总和。
    pub fn skill_total(&self) -> u64 {
        self.skills.values().copied().sum()
    }

    /// 转换为带合计字段的 [`AgentUsageSummary`]，消耗 `self`。
    pub fn into_summary(self, agent_id: String) -> AgentUsageSummary {
        AgentUsageSummary {
            tool_total: self.tool_total(),
            skill_total: self.skill_total(),
            tools: self.tools,
            skills: self.skills,
            agent_id,
        }
    }
}

/// 返回指定 Agent 的 `usage-stats.json` 路径。
fn usage_path(agent_id: &str) -> PathBuf {
    let base = default_memory_dir();
    let id = normalize_agent_id(agent_id);
    agent_config_dir(&base, &id).join("usage-stats.json")
}

/// 规范化 Agent 键：`None`、空白、`"default"` 均映射为 [`DEFAULT_AGENT_ID`]。
fn normalize_key(agent_id: Option<&str>) -> String {
    match agent_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) if id == "default" => DEFAULT_AGENT_ID.to_string(),
        Some(id) => normalize_agent_id(id),
        None => DEFAULT_AGENT_ID.to_string(),
    }
}

/// 从磁盘加载用量统计；文件不存在或解析失败时返回默认值。
pub fn load_usage_stats(agent_id: Option<&str>) -> AgentUsageStats {
    let id = normalize_key(agent_id);
    let path = usage_path(&id);
    if !path.exists() {
        return AgentUsageStats::default();
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// 原子写入用量统计（临时文件 + `rename`）。
pub fn save_usage_stats(agent_id: Option<&str>, stats: &AgentUsageStats) -> anyhow::Result<()> {
    let _ = ensure_default_workspace_dirs()?;
    let id = normalize_key(agent_id);
    let path = usage_path(&id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(stats)?)?;
    fs::rename(&tmp, &path)?;
    Ok(())
}

/// 加载并转换为 [`AgentUsageSummary`]，供前端/API 直接返回。
pub fn get_usage_summary(agent_id: Option<&str>) -> AgentUsageSummary {
    let id = normalize_key(agent_id);
    load_usage_stats(Some(&id)).into_summary(id)
}

/// 记录一次工具调用并写盘；进程内通过互斥锁串行化，避免并发丢计数。
///
/// 工具名归并为工具集后 `tools` 计数 +1；若为 `skills` 且 `args.skill_id` 非空，
/// 则对应 `skills` 条目亦 +1。
///
/// 成功写 JSON 后双写 `usage.db`：`kind=tool`（工具集 id）；skills 再写 `kind=skill`。
/// MCP 工具（`mcp__` 前缀）只更新 JSON，事件由 Agent loop 写 `kind=mcp`。
///
/// `session_id` / `turn_id` 写入 usage 事件，便于 Tracing 按会话与 turn 串联工具调用。
pub fn record_tool_call(
    agent_id: &str,
    tool_name: &str,
    args: &serde_json::Value,
    session_id: Option<&str>,
    turn_id: Option<&str>,
) -> anyhow::Result<()> {
    // 串行化写盘，避免并发丢计数
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let id = normalize_key(Some(agent_id));
    let mut stats = load_usage_stats(Some(&id));
    let toolset = tool_name_to_toolset(tool_name).to_string();
    *stats.tools.entry(toolset.clone()).or_insert(0) += 1;

    if tool_name == "skills" {
        if let Some(skill_id) = args
            .get("skill_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            *stats.skills.entry(skill_id.to_string()).or_insert(0) += 1;
        }
    }

    save_usage_stats(Some(&id), &stats)?;

    let is_mcp = tool_name.starts_with("mcp__");
    if !is_mcp {
        use crate::db::{NewUsageEvent, UsageDb};

        let sid = session_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let tid = turn_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let ts = chrono::Utc::now().to_rfc3339();
        UsageDb::try_record(NewUsageEvent {
            ts: ts.clone(),
            kind: "tool".into(),
            name: toolset,
            agent_id: id.clone(),
            session_id: sid.clone(),
            turn_id: tid.clone(),
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 0,
            cost_usd: 0.0,
            cost_status: None,
            cost_source: None,
            pricing_version: None,
            billing_provider: None,
            billing_base_url: None,
            billing_mode: None,
            meta_json: Some(serde_json::json!({ "tool": tool_name }).to_string()),
        });
        if tool_name == "skills" {
            if let Some(skill_id) = args
                .get("skill_id")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                UsageDb::try_record(NewUsageEvent {
                    ts,
                    kind: "skill".into(),
                    name: skill_id.to_string(),
                    agent_id: id,
                    session_id: sid,
                    turn_id: tid,
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                    reasoning_tokens: 0,
                    total_tokens: 0,
                    cost_usd: 0.0,
                    cost_status: None,
                    cost_source: None,
                    pricing_version: None,
                    billing_provider: None,
                    billing_base_url: None,
                    billing_mode: None,
                    meta_json: None,
                });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    use home::test_env::AstroMemoryDirGuard;

    #[test]
    fn records_toolset_and_skill_counts() {
        let dir = tempfile::tempdir().unwrap();
        let _env = AstroMemoryDirGuard::set(dir.path());

        record_tool_call(
            "workspace",
            "skills",
            &json!({ "skill_id": "demo-skill", "input": {} }),
            Some("sess-1"),
            None,
        )
        .unwrap();
        record_tool_call(
            "workspace",
            "skills",
            &json!({ "skill_id": "demo-skill" }),
            Some("sess-1"),
            None,
        )
        .unwrap();
        record_tool_call(
            "workspace",
            "web_search",
            &json!({ "query": "hi" }),
            Some("sess-1"),
            None,
        )
        .unwrap();

        let summary = get_usage_summary(Some("workspace"));
        assert_eq!(summary.tools.get("skills").copied().unwrap_or(0), 2);
        assert_eq!(summary.tools.get("web_search").copied().unwrap_or(0), 1);
        assert_eq!(summary.skills.get("demo-skill").copied().unwrap_or(0), 2);
        assert_eq!(summary.skill_total, 2);
        assert_eq!(summary.tool_total, 3);

        let insights = crate::UsageDb::open_default()
            .unwrap()
            .query_insights(crate::UsageInsightsQuery {
                period: crate::UsagePeriod::Month,
                as_of: None,
                agent_id: Some("workspace".into()),
            })
            .unwrap();
        assert!(insights.kpis.calls >= 3);
        assert!(insights
            .rankings
            .by_kind
            .iter()
            .any(|r| r.kind == "tool" && r.name == "skills"));
        assert!(insights
            .rankings
            .by_kind
            .iter()
            .any(|r| r.kind == "skill" && r.name == "demo-skill"));
        assert!(insights
            .rankings
            .by_kind
            .iter()
            .any(|r| r.kind == "tool" && r.name == "web_search"));
    }

    #[test]
    fn skills_without_skill_id_only_bumps_toolset() {
        let dir = tempfile::tempdir().unwrap();
        let _env = AstroMemoryDirGuard::set(dir.path());

        record_tool_call(
            "workspace",
            "skills",
            &json!({ "skill_id": "  " }),
            None,
            None,
        )
        .unwrap();
        let summary = get_usage_summary(Some("workspace"));
        assert_eq!(summary.tools.get("skills").copied().unwrap_or(0), 1);
        assert!(summary.skills.is_empty());
        assert_eq!(summary.skill_total, 0);
    }

    #[test]
    fn records_tool_event_with_turn_id() {
        let dir = tempfile::tempdir().unwrap();
        let _env = AstroMemoryDirGuard::set(dir.path());

        record_tool_call(
            "workspace",
            "web_search",
            &json!({ "query": "hi" }),
            Some("sess-turn"),
            Some("turn-42"),
        )
        .unwrap();

        let path = crate::db::usage_db_path();
        let conn = rusqlite::Connection::open(&path).unwrap();
        let turn: Option<String> = conn
            .query_row(
                "SELECT turn_id FROM usage_events WHERE kind = 'tool' AND session_id = 'sess-turn'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(turn.as_deref(), Some("turn-42"));
    }
}
