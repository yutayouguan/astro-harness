//! 编排执行器单测（无真实 LLM）。

use orchestration::{
    NewOrchestration, NewOrchestrationStep, OrchestrationDb, OrchestrationSpawnRequest,
    OrchestrationStatus,
};
use tempfile::TempDir;
use tokio::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::const_new(());

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn second_claim_is_noop_without_llm() {
    let _guard = ENV_LOCK.lock().await;
    let dir = TempDir::new().unwrap();
    std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

    let db = OrchestrationDb::open_default().unwrap();
    let id = db
        .create(NewOrchestration {
            parent_agent_id: "workspace".into(),
            session_id: None,
            goal: "g".into(),
            steps: vec![NewOrchestrationStep {
                role: "a".into(),
                agent_id: None,
                prompt: "p".into(),
            }],
            provider: "openai".into(),
            model: "gpt-4o-mini".into(),
            api_key: "sk-test".into(),
            base_url: String::new(),
        })
        .unwrap();

    assert!(db.try_claim_running(&id).unwrap());

    let req = OrchestrationSpawnRequest {
        orchestration_id: id.clone(),
        parent_agent_id: "workspace".into(),
        provider: "openai".into(),
        model: "gpt-4o-mini".into(),
        api_key: "sk-test".into(),
        base_url: String::new(),
        chat_targets: vec![],
        caller_depth: 0,
        max_spawn_depth: home::DEFAULT_MAX_SPAWN_DEPTH,
        allow_reclaim: false,
    };
    agent::exec::orchestration::run_orchestration(req).await.unwrap();

    let orch = db.get(&id).unwrap().unwrap();
    assert_eq!(orch.status, OrchestrationStatus::Running.as_str());
    std::env::remove_var("ASTRO_MEMORY_DIR");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn empty_api_key_marks_failed_and_emits_telemetry() {
    let _guard = ENV_LOCK.lock().await;
    let dir = TempDir::new().unwrap();
    std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

    let db = OrchestrationDb::open_default().unwrap();
    let id = db
        .create(NewOrchestration {
            parent_agent_id: "workspace".into(),
            session_id: None,
            goal: "g".into(),
            steps: vec![
                NewOrchestrationStep {
                    role: "researcher".into(),
                    agent_id: None,
                    prompt: "collect".into(),
                },
                NewOrchestrationStep {
                    role: "writer".into(),
                    agent_id: None,
                    prompt: "draft".into(),
                },
            ],
            provider: "openai".into(),
            model: "gpt-4o-mini".into(),
            api_key: String::new(),
            base_url: String::new(),
        })
        .unwrap();

    let req = OrchestrationSpawnRequest {
        orchestration_id: id.clone(),
        parent_agent_id: "workspace".into(),
        provider: "openai".into(),
        model: "gpt-4o-mini".into(),
        api_key: String::new(),
        base_url: String::new(),
        chat_targets: vec![],
        caller_depth: 0,
        max_spawn_depth: home::DEFAULT_MAX_SPAWN_DEPTH,
        allow_reclaim: false,
    };
    agent::exec::orchestration::run_orchestration(req).await.unwrap();

    let orch = db.get(&id).unwrap().unwrap();
    assert_eq!(orch.status, OrchestrationStatus::Failed.as_str());
    let steps = db.list_steps(&id).unwrap();
    assert_eq!(steps[0].status, "failed");
    assert!(steps[0]
        .error
        .as_deref()
        .unwrap_or("")
        .contains("API Key"));
    assert_eq!(
        steps[1].status, "skipped",
        "remaining pending steps should be skipped on failure"
    );

    let usage = usage::UsageDb::open_default().unwrap();
    let insights = usage
        .query_insights(usage::UsageInsightsQuery {
            period: usage::UsagePeriod::Year,
            as_of: None,
            agent_id: None,
        })
        .unwrap();
    assert_eq!(
        insights.kpis.calls, 0,
        "orchestration kind must not inflate calls KPI"
    );
    let conn = rusqlite::Connection::open(usage::usage_db_path()).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM usage_events WHERE kind = 'orchestration'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(count >= 2, "expected start+end edges, got {count}");
    std::env::remove_var("ASTRO_MEMORY_DIR");
}
