//! usage.db 事件写入与聚合查询测试。

use memory::usage_db::{
    usage_db_path, NewUsageEvent, UsageDb, UsageInsightsQuery, UsagePeriod,
};
use tempfile::TempDir;

#[test]
fn usage_db_path_under_memory_dir() {
    let dir = TempDir::new().unwrap();
    std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
    assert_eq!(usage_db_path(), dir.path().join("usage.db"));
}

#[test]
fn insert_and_count_events() {
    let dir = TempDir::new().unwrap();
    let db = UsageDb::new(dir.path().join("usage.db")).unwrap();
    db.insert(NewUsageEvent {
        ts: "2026-07-13T02:00:00Z".into(),
        kind: "tool".into(),
        name: "terminal".into(),
        agent_id: "workspace".into(),
        session_id: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        cost_usd: 0.0,
        meta_json: None,
    })
    .unwrap();
    db.insert(NewUsageEvent {
        ts: "2026-07-13T03:00:00Z".into(),
        kind: "llm".into(),
        name: "gpt-4o-mini".into(),
        agent_id: "workspace".into(),
        session_id: Some("s1".into()),
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
        cost_usd: 0.001,
        meta_json: None,
    })
    .unwrap();
    let insights = db
        .query_insights(UsageInsightsQuery {
            period: UsagePeriod::Month,
            as_of: Some("2026-07-13T12:00:00Z".into()),
            agent_id: None,
        })
        .unwrap();
    assert_eq!(insights.kpis.calls, 2);
    assert_eq!(insights.kpis.tokens, 150);
    assert!((insights.kpis.cost_usd - 0.001).abs() < 1e-9);
    assert_eq!(insights.kpis.active_agents, 1);
}

#[test]
fn filters_by_agent_and_excludes_out_of_range() {
    let dir = TempDir::new().unwrap();
    let db = UsageDb::new(dir.path().join("usage.db")).unwrap();
    for (ts, agent, kind, name) in [
        ("2026-07-01T10:00:00Z", "workspace", "tool", "terminal"),
        ("2026-07-02T10:00:00Z", "research", "tool", "web_search"),
        ("2026-06-01T10:00:00Z", "workspace", "tool", "terminal"), // 上月
    ] {
        db.insert(NewUsageEvent {
            ts: ts.into(),
            kind: kind.into(),
            name: name.into(),
            agent_id: agent.into(),
            session_id: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            cost_usd: 0.0,
            meta_json: None,
        })
        .unwrap();
    }
    let all = db
        .query_insights(UsageInsightsQuery {
            period: UsagePeriod::Month,
            as_of: Some("2026-07-15T00:00:00Z".into()),
            agent_id: None,
        })
        .unwrap();
    assert_eq!(all.kpis.calls, 2);
    assert_eq!(all.kpis.active_agents, 2);

    let one = db
        .query_insights(UsageInsightsQuery {
            period: UsagePeriod::Month,
            as_of: Some("2026-07-15T00:00:00Z".into()),
            agent_id: Some("workspace".into()),
        })
        .unwrap();
    assert_eq!(one.kpis.calls, 1);
    assert_eq!(one.rankings.by_kind[0].name, "terminal");
}

#[test]
fn skill_events_do_not_inflate_kpi_calls() {
    let dir = TempDir::new().unwrap();
    let db = UsageDb::new(dir.path().join("usage.db")).unwrap();
    db.insert(NewUsageEvent {
        ts: "2026-07-13T01:00:00Z".into(),
        kind: "tool".into(),
        name: "skills".into(),
        agent_id: "workspace".into(),
        session_id: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        cost_usd: 0.0,
        meta_json: None,
    })
    .unwrap();
    db.insert(NewUsageEvent {
        ts: "2026-07-13T01:00:01Z".into(),
        kind: "skill".into(),
        name: "demo".into(),
        agent_id: "workspace".into(),
        session_id: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        cost_usd: 0.0,
        meta_json: None,
    })
    .unwrap();
    let insights = db
        .query_insights(UsageInsightsQuery {
            period: UsagePeriod::Month,
            as_of: Some("2026-07-13T12:00:00Z".into()),
            agent_id: None,
        })
        .unwrap();
    assert_eq!(insights.kpis.calls, 1);
    assert!(insights
        .rankings
        .by_kind
        .iter()
        .any(|r| r.kind == "skill" && r.name == "demo"));
}
