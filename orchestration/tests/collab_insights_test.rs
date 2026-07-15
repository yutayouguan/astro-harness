//! 协作洞察查询测试

use std::sync::Mutex;

use orchestration::{query_collaboration_insights, CollaborationInsightsQuery};
use tempfile::TempDir;
use usage::{NewUsageEvent, UsageDb, UsageInsightsQuery, UsagePeriod};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn empty_db_returns_empty_collab() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = TempDir::new().unwrap();
    std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

    let insights = query_collaboration_insights(CollaborationInsightsQuery {
        period: UsagePeriod::Month,
        as_of: Some("2026-07-13T12:00:00Z".into()),
        agent_id: None,
    })
    .unwrap();
    assert!(insights.orchestrations.is_empty());
    assert!(insights.graph.nodes.is_empty());
    assert!(insights.graph.edges.is_empty());
    std::env::remove_var("ASTRO_MEMORY_DIR");
}

#[test]
fn graph_counts_end_phase_only_and_filters_agent() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = TempDir::new().unwrap();
    std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

    let usage = UsageDb::open_default().unwrap();
    let meta_end = serde_json::json!({
        "from": "alice", "to": "role:r", "phase": "end", "ok": true
    })
    .to_string();
    let meta_start = serde_json::json!({
        "from": "alice", "to": "role:r", "phase": "start"
    })
    .to_string();
    let meta_other = serde_json::json!({
        "from": "bob", "to": "role:x", "phase": "end"
    })
    .to_string();

    for (ts, meta) in [
        ("2026-07-10T12:00:00Z", meta_end),
        ("2026-07-10T12:01:00Z", meta_start),
        ("2026-07-10T12:02:00Z", meta_other),
    ] {
        usage
            .insert(NewUsageEvent {
                ts: ts.into(),
                kind: "orchestration".into(),
                name: "orchestration_step".into(),
                agent_id: "alice".into(),
                session_id: None,
                turn_id: None,
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
                meta_json: Some(meta),
            })
            .unwrap();
    }

    let insights = query_collaboration_insights(CollaborationInsightsQuery {
        period: UsagePeriod::Month,
        as_of: Some("2026-07-13T12:00:00Z".into()),
        agent_id: Some("alice".into()),
    })
    .unwrap();
    assert_eq!(insights.graph.edges.len(), 1);
    assert_eq!(insights.graph.edges[0].weight, 1);
    assert_eq!(insights.graph.edges[0].from, "alice");
    assert_eq!(insights.graph.edges[0].to, "role:r");
    assert!(insights
        .graph
        .nodes
        .iter()
        .any(|n| n.id == "role:r" && n.kind == "role" && n.label == "r"));

    let usage_insights = usage
        .query_insights(UsageInsightsQuery {
            period: UsagePeriod::Month,
            as_of: Some("2026-07-13T12:00:00Z".into()),
            agent_id: None,
        })
        .unwrap();
    assert_eq!(usage_insights.kpis.calls, 0);

    std::env::remove_var("ASTRO_MEMORY_DIR");
}
