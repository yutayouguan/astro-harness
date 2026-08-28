use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use home::tool_name_to_toolset;
use home::{
    agent_config_dir, default_memory_dir, ensure_default_workspace_dirs, normalize_agent_id,
    DEFAULT_AGENT_ID,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentUsageStats {
    #[serde(default)]
    pub tools: HashMap<String, u64>,
    #[serde(default)]
    pub skills: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentUsageSummary {
    pub agent_id: String,
    pub tool_total: u64,
    pub skill_total: u64,
    pub tools: HashMap<String, u64>,
    pub skills: HashMap<String, u64>,
}

impl AgentUsageStats {
    pub fn tool_total(&self) -> u64 {
        self.tools.values().copied().sum()
    }

    pub fn skill_total(&self) -> u64 {
        self.skills.values().copied().sum()
    }

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

fn usage_path(agent_id: &str) -> PathBuf {
    let base = default_memory_dir();
    let id = normalize_agent_id(agent_id);
    agent_config_dir(&base, &id).join("usage-stats.json")
}

fn normalize_key(agent_id: Option<&str>) -> String {
    match agent_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some("default") => DEFAULT_AGENT_ID.to_string(),
        Some(id) => normalize_agent_id(id),
        None => DEFAULT_AGENT_ID.to_string(),
    }
}

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

pub fn save_usage_stats(agent_id: Option<&str>, stats: &AgentUsageStats) -> anyhow::Result<()> {
    ensure_default_workspace_dirs()?;
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

pub fn get_usage_summary(agent_id: Option<&str>) -> AgentUsageSummary {
    let id = normalize_key(agent_id);
    load_usage_stats(Some(&id)).into_summary(id)
}

pub async fn record_tool_call(
    agent_id: &str,
    tool_name: &str,
    args: &serde_json::Value,
    session_id: Option<&str>,
    turn_id: Option<&str>,
) -> anyhow::Result<()> {
    static LOCK: Mutex<()> = Mutex::const_new(());
    let _guard = LOCK.lock().await;

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
        })
        .await;
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
                })
                .await;
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

    #[tokio::test]
    async fn records_toolset_and_skill_counts() {
        let dir = tempfile::tempdir().unwrap();
        let _env = AstroMemoryDirGuard::set(dir.path());

        record_tool_call(
            "workspace",
            "skills",
            &json!({ "skill_id": "demo-skill", "input": {} }),
            Some("sess-1"),
            None,
        )
        .await
        .unwrap();
        record_tool_call(
            "workspace",
            "skills",
            &json!({ "skill_id": "demo-skill" }),
            Some("sess-1"),
            None,
        )
        .await
        .unwrap();
        record_tool_call(
            "workspace",
            "web_search",
            &json!({ "query": "hi" }),
            Some("sess-1"),
            None,
        )
        .await
        .unwrap();

        let summary = get_usage_summary(Some("workspace"));
        assert_eq!(summary.tools.get("skills").copied().unwrap_or(0), 2);
        assert_eq!(summary.tools.get("web_search").copied().unwrap_or(0), 1);
        assert_eq!(summary.skills.get("demo-skill").copied().unwrap_or(0), 2);
        assert_eq!(summary.skill_total, 2);
        assert_eq!(summary.tool_total, 3);

        let insights = crate::UsageDb::open_default()
            .await
            .unwrap()
            .query_insights(crate::UsageInsightsQuery {
                period: crate::UsagePeriod::Month,
                as_of: None,
                agent_id: Some(home::DEFAULT_AGENT_ID.into()),
            })
            .await
            .unwrap();
        assert_eq!(insights.kpis.calls, 3);
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

    #[tokio::test]
    async fn skills_without_skill_id_only_bumps_toolset() {
        let dir = tempfile::tempdir().unwrap();
        let _env = AstroMemoryDirGuard::set(dir.path());

        record_tool_call(
            "workspace",
            "skills",
            &json!({ "skill_id": "  " }),
            None,
            None,
        )
        .await
        .unwrap();
        let summary = get_usage_summary(Some("workspace"));
        assert_eq!(summary.tools.get("skills").copied().unwrap_or(0), 1);
        assert!(summary.skills.is_empty());
        assert_eq!(summary.skill_total, 0);
    }

    #[tokio::test]
    async fn records_tool_event_with_turn_id() {
        let dir = tempfile::tempdir().unwrap();
        let _env = AstroMemoryDirGuard::set(dir.path());

        record_tool_call(
            "workspace",
            "web_search",
            &json!({ "query": "hi" }),
            Some("sess-turn"),
            Some("turn-42"),
        )
        .await
        .unwrap();

        let db = crate::UsageDb::open_default().await.unwrap();
        let (turn,): (Option<String>,) = agent_db::sqlx::query_as(
            "SELECT turn_id FROM usage_events WHERE kind = 'tool' AND session_id = 'sess-turn'",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(turn.as_deref(), Some("turn-42"));
    }
}
