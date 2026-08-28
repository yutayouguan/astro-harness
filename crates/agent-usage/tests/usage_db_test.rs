use std::sync::Mutex;
use tempfile::TempDir;
use usage::db::{
    usage_db_path, NewUsageEvent, UsageDb, UsageInsightsQuery, UsagePeriod, USAGE_SCHEMA_VERSION,
};

use agent_db::sqlx;
use agent_db::{AstroDb, DbSpec};

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct ZeroBillingEvent<'a> {
    ts: &'a str,
    kind: &'a str,
    name: &'a str,
    agent_id: &'a str,
    session_id: Option<String>,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    cost_usd: f64,
    meta_json: Option<String>,
}

fn zero_billing_event(e: ZeroBillingEvent<'_>) -> NewUsageEvent {
    NewUsageEvent {
        ts: e.ts.into(),
        kind: e.kind.into(),
        name: e.name.into(),
        agent_id: e.agent_id.into(),
        session_id: e.session_id,
        input_tokens: e.input_tokens,
        output_tokens: e.output_tokens,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        total_tokens: e.total_tokens,
        cost_usd: e.cost_usd,
        cost_status: None,
        cost_source: None,
        pricing_version: None,
        billing_provider: None,
        billing_base_url: None,
        billing_mode: None,
        meta_json: e.meta_json,
        turn_id: None,
    }
}

#[tokio::test]
async fn migrate_v3_to_v4_keeps_rows_and_adds_turn_id() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("usage.db");

    {
        let spec = DbSpec::new("setup", "usage.db");
        let db = AstroDb::new(dir.path());
        let pool = db.open_pool(&spec).await.unwrap();
        sqlx::query(
            r#"
            CREATE TABLE usage_events (
                id TEXT PRIMARY KEY,
                ts TEXT NOT NULL,
                kind TEXT NOT NULL,
                name TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                session_id TEXT,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                cache_write_tokens INTEGER NOT NULL DEFAULT 0,
                reasoning_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                cost_usd REAL NOT NULL DEFAULT 0,
                cost_status TEXT,
                cost_source TEXT,
                pricing_version TEXT,
                billing_provider TEXT,
                billing_base_url TEXT,
                billing_mode TEXT,
                meta_json TEXT
            );
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("PRAGMA user_version = 3")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO usage_events (id, ts, kind, name, agent_id, total_tokens, cost_usd)
             VALUES ('old1', '2026-01-01T00:00:00Z', 'llm', 'm', 'a', 10, 0.0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
    }

    let db = usage::UsageDb::new(path.clone()).await.unwrap();

    let _db2 = usage::UsageDb::new(path.clone()).await.unwrap();
    let _db3 = usage::UsageDb::new(path.clone()).await.unwrap();

    let (ver,): (i32,) = sqlx::query_as("PRAGMA user_version")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(ver, USAGE_SCHEMA_VERSION);

    let (old_turn,): (Option<String>,) =
        sqlx::query_as("SELECT turn_id FROM usage_events WHERE id = 'old1'")
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert_eq!(old_turn, None);

    let id = db
        .insert(usage::NewUsageEvent {
            ts: "2026-07-14T00:00:00Z".into(),
            kind: "llm".into(),
            name: "m2".into(),
            agent_id: "a".into(),
            session_id: Some("s1".into()),
            turn_id: Some("turn-abc".into()),
            input_tokens: 1,
            output_tokens: 2,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 3,
            cost_usd: 0.0,
            cost_status: None,
            cost_source: None,
            pricing_version: None,
            billing_provider: None,
            billing_base_url: None,
            billing_mode: None,
            meta_json: None,
        })
        .await
        .unwrap();
    assert!(!id.is_empty());

    let (ver2,): (i32,) = sqlx::query_as("PRAGMA user_version")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(ver2, USAGE_SCHEMA_VERSION);

    let (turn,): (Option<String>,) =
        sqlx::query_as("SELECT turn_id FROM usage_events WHERE id = ?1")
            .bind(&id)
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert_eq!(turn.as_deref(), Some("turn-abc"));

    let (old_ok,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM usage_events WHERE id = 'old1'")
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert_eq!(old_ok, 1);
}

#[tokio::test]
async fn usage_db_rejects_newer_schema_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("usage.db");
    {
        let spec = DbSpec::new("setup", "usage.db");
        let db = AstroDb::new(dir.path());
        let pool = db.open_pool(&spec).await.unwrap();
        sqlx::query(
            r#"
            CREATE TABLE usage_events (
                id TEXT PRIMARY KEY,
                ts TEXT NOT NULL,
                kind TEXT NOT NULL,
                name TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                session_id TEXT,
                turn_id TEXT
            );
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("PRAGMA user_version = 5")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }
    let err = usage::UsageDb::new(path).await.err().expect("expected Err");
    assert!(err.to_string().contains("newer than supported"));
}

#[test]
fn usage_db_path_under_memory_dir() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = TempDir::new().unwrap();
    std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
    assert_eq!(usage_db_path(), dir.path().join("usage.db"));
    std::env::remove_var("ASTRO_MEMORY_DIR");
}

#[tokio::test]
async fn insert_and_count_events() {
    let dir = TempDir::new().unwrap();
    let db = UsageDb::new(dir.path().join("usage.db")).await.unwrap();
    db.insert(zero_billing_event(ZeroBillingEvent {
        ts: "2026-07-13T02:00:00Z",
        kind: "tool",
        name: "terminal",
        agent_id: "default",
        session_id: None,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
        cost_usd: 0.0,
        meta_json: None,
    }))
    .await
    .unwrap();
    db.insert(zero_billing_event(ZeroBillingEvent {
        ts: "2026-07-13T03:00:00Z",
        kind: "llm",
        name: "gpt-4o-mini",
        agent_id: "default",
        session_id: Some("s1".into()),
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
        cost_usd: 0.001,
        meta_json: None,
    }))
    .await
    .unwrap();
    let insights = db
        .query_insights(UsageInsightsQuery {
            period: UsagePeriod::Month,
            as_of: Some("2026-07-13T12:00:00Z".into()),
            agent_id: None,
        })
        .await
        .unwrap();
    assert_eq!(insights.kpis.calls, 2);
    assert_eq!(insights.kpis.tokens, 150);
    assert!((insights.kpis.cost_usd - 0.001).abs() < 1e-9);
    assert_eq!(insights.kpis.active_agents, 1);
}

#[tokio::test]
async fn filters_by_agent_and_excludes_out_of_range() {
    let dir = TempDir::new().unwrap();
    let db = UsageDb::new(dir.path().join("usage.db")).await.unwrap();
    for (ts, agent, kind, name) in [
        ("2026-07-01T10:00:00Z", "default", "tool", "terminal"),
        ("2026-07-02T10:00:00Z", "research", "tool", "web_search"),
        ("2026-06-01T10:00:00Z", "default", "tool", "terminal"),
    ] {
        db.insert(zero_billing_event(ZeroBillingEvent {
            ts,
            kind,
            name,
            agent_id: agent,
            session_id: None,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            cost_usd: 0.0,
            meta_json: None,
        }))
        .await
        .unwrap();
    }
    let all = db
        .query_insights(UsageInsightsQuery {
            period: UsagePeriod::Month,
            as_of: Some("2026-07-15T00:00:00Z".into()),
            agent_id: None,
        })
        .await
        .unwrap();
    assert_eq!(all.kpis.calls, 2);
    assert_eq!(all.kpis.active_agents, 2);

    let one = db
        .query_insights(UsageInsightsQuery {
            period: UsagePeriod::Month,
            as_of: Some("2026-07-15T00:00:00Z".into()),
            agent_id: Some("default".into()),
        })
        .await
        .unwrap();
    assert_eq!(one.kpis.calls, 1);
    assert_eq!(one.rankings.by_kind[0].name, "terminal");
}

#[tokio::test]
async fn skill_events_do_not_inflate_kpi_calls() {
    let dir = TempDir::new().unwrap();
    let db = UsageDb::new(dir.path().join("usage.db")).await.unwrap();
    db.insert(zero_billing_event(ZeroBillingEvent {
        ts: "2026-07-13T01:00:00Z",
        kind: "tool",
        name: "skills",
        agent_id: "default",
        session_id: None,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
        cost_usd: 0.0,
        meta_json: None,
    }))
    .await
    .unwrap();
    db.insert(zero_billing_event(ZeroBillingEvent {
        ts: "2026-07-13T01:00:01Z",
        kind: "skill",
        name: "demo",
        agent_id: "default",
        session_id: None,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
        cost_usd: 0.0,
        meta_json: None,
    }))
    .await
    .unwrap();
    let insights = db
        .query_insights(UsageInsightsQuery {
            period: UsagePeriod::Month,
            as_of: Some("2026-07-13T12:00:00Z".into()),
            agent_id: None,
        })
        .await
        .unwrap();
    assert_eq!(insights.kpis.calls, 1);
    assert!(insights
        .rankings
        .by_kind
        .iter()
        .any(|r| r.kind == "skill" && r.name == "demo"));
}

#[tokio::test]
async fn offset_timestamp_normalized_and_counted_in_month() {
    let dir = TempDir::new().unwrap();
    let db = UsageDb::new(dir.path().join("usage.db")).await.unwrap();
    db.insert(zero_billing_event(ZeroBillingEvent {
        ts: "2026-07-01T00:00:00+00:00",
        kind: "tool",
        name: "terminal",
        agent_id: "default",
        session_id: None,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
        cost_usd: 0.0,
        meta_json: None,
    }))
    .await
    .unwrap();
    let insights = db
        .query_insights(UsageInsightsQuery {
            period: UsagePeriod::Month,
            as_of: Some("2026-07-15T00:00:00Z".into()),
            agent_id: None,
        })
        .await
        .unwrap();
    assert_eq!(insights.kpis.calls, 1);
}

#[tokio::test]
async fn usage_db_rebuilds_incompatible_schema_and_ignores_unknown_cost() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("usage.db");
    {
        let spec = DbSpec::new("setup", "usage.db");
        let db = AstroDb::new(dir.path());
        let pool = db.open_pool(&spec).await.unwrap();
        sqlx::query(
            "CREATE TABLE usage_events (
                id TEXT PRIMARY KEY, ts TEXT, kind TEXT, name TEXT, agent_id TEXT,
                session_id TEXT, prompt_tokens INTEGER, completion_tokens INTEGER,
                total_tokens INTEGER, cost_usd REAL, meta_json TEXT
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO usage_events VALUES ('1','2026-07-01T00:00:00Z','llm','m','a',NULL,1,1,2,9.9,NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
    }
    let db = usage::UsageDb::new(path.clone()).await.unwrap();
    let q = usage::UsageInsightsQuery {
        period: usage::UsagePeriod::Year,
        as_of: Some("2026-07-13T00:00:00Z".into()),
        agent_id: None,
    };
    let insights = db.query_insights(q.clone()).await.unwrap();
    assert_eq!(insights.kpis.calls, 0);

    db.insert(usage::NewUsageEvent {
        ts: "2026-07-10T12:00:00Z".into(),
        kind: "llm".into(),
        name: "m".into(),
        agent_id: "a".into(),
        session_id: None,
        turn_id: None,
        input_tokens: 10,
        output_tokens: 5,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        total_tokens: 15,
        cost_usd: 0.0,
        cost_status: Some("unknown".into()),
        cost_source: Some("none".into()),
        pricing_version: None,
        billing_provider: None,
        billing_base_url: None,
        billing_mode: None,
        meta_json: None,
    })
    .await
    .unwrap();
    db.insert(usage::NewUsageEvent {
        ts: "2026-07-10T13:00:00Z".into(),
        kind: "llm".into(),
        name: "m2".into(),
        agent_id: "a".into(),
        session_id: None,
        turn_id: None,
        input_tokens: 10,
        output_tokens: 5,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        total_tokens: 15,
        cost_usd: 1.25,
        cost_status: Some("estimated".into()),
        cost_source: Some("official_docs_snapshot".into()),
        pricing_version: Some("test".into()),
        billing_provider: Some("openai".into()),
        billing_base_url: None,
        billing_mode: None,
        meta_json: None,
    })
    .await
    .unwrap();
    let insights = db.query_insights(q).await.unwrap();
    assert!((insights.kpis.cost_usd - 1.25).abs() < 1e-9);
    assert_eq!(insights.kpis.tokens, 30);
    assert_eq!(insights.unpriced_llm_events, 1);
}

#[test]
fn estimate_usage_cost_official_snapshot_and_unknown() {
    use usage::{estimate_usage_cost, CostStatus, UsageTokens};
    let usage = UsageTokens {
        input_tokens: 1_000_000,
        output_tokens: 1_000_000,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        request_count: 1,
    };
    let r = estimate_usage_cost("gpt-4o-mini", &usage, Some("openai"), None, None);
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

#[test]
fn estimate_usage_cost_reads_openrouter_cache_file() {
    let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    use usage::{estimate_usage_cost, CostStatus, UsageTokens};
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
    let cache_dir = dir.path().join("cache");
    std::fs::create_dir_all(&cache_dir).unwrap();
    let cache = cache_dir.join("openrouter-model-pricing.json");
    std::fs::write(
        &cache,
        r#"{"fetched_at":"2099-01-01T00:00:00Z","models":{"test/or-model":{"prompt":0.000001,"completion":0.000002}}}"#,
    )
    .unwrap();
    let usage = UsageTokens {
        input_tokens: 1_000_000,
        output_tokens: 1_000_000,
        request_count: 1,
        ..Default::default()
    };
    let r = estimate_usage_cost(
        "test/or-model",
        &usage,
        Some("openrouter"),
        Some("https://openrouter.ai/api/v1"),
        None,
    );
    assert_eq!(r.status, CostStatus::Estimated);
    assert!((r.amount_usd.unwrap() - 3.0).abs() < 1e-6);
    std::env::remove_var("ASTRO_MEMORY_DIR");
}
