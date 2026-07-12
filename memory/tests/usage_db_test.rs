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
