//! usage.db 事件写入与聚合查询测试。

use memory::usage_db::{
    usage_db_path, NewUsageEvent, UsageDb, UsageInsightsQuery, UsagePeriod,
};
use std::sync::Mutex;
use tempfile::TempDir;

/// 串行化依赖 `ASTRO_MEMORY_DIR` 的用例，避免并行污染。
static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn usage_db_path_under_memory_dir() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = TempDir::new().unwrap();
    std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
    assert_eq!(usage_db_path(), dir.path().join("usage.db"));
    std::env::remove_var("ASTRO_MEMORY_DIR");
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

#[test]
fn offset_timestamp_normalized_and_counted_in_month() {
    let dir = TempDir::new().unwrap();
    let db = UsageDb::new(dir.path().join("usage.db")).unwrap();
    // `+00:00` 若不规范化为 `…Z`，会因字典序落在 `2026-07-01T00:00:00Z` 之前而被排除
    db.insert(NewUsageEvent {
        ts: "2026-07-01T00:00:00+00:00".into(),
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
    let insights = db
        .query_insights(UsageInsightsQuery {
            period: UsagePeriod::Month,
            as_of: Some("2026-07-15T00:00:00Z".into()),
            agent_id: None,
        })
        .unwrap();
    assert_eq!(insights.kpis.calls, 1);
}

#[test]
fn estimate_usage_cost_official_snapshot_and_unknown() {
    use memory::{estimate_usage_cost, CostStatus, UsageTokens};
    let usage = UsageTokens {
        input_tokens: 1_000_000,
        output_tokens: 1_000_000,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        request_count: 1,
    };
    let r = estimate_usage_cost(
        "gpt-4o-mini",
        &usage,
        Some("openai"),
        None,
        None,
    );
    assert_eq!(r.status, CostStatus::Estimated);
    assert!(r.amount_usd.unwrap() > 0.0);
    let unk = estimate_usage_cost(
        "totally-unknown-model-xyz",
        &usage,
        Some("custom"),
        Some("http://localhost:9"),
        None,
    );
    assert_eq!(unk.status, CostStatus::Unknown);
    assert!(unk.amount_usd.is_none() || unk.amount_usd == Some(0.0));
}
