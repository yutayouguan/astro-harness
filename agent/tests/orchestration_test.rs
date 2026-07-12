//! 编排执行器单测（无真实 LLM）。

use memory::{
    NewOrchestration, NewOrchestrationStep, OrchestrationDb, OrchestrationSpawnRequest,
    OrchestrationStatus,
};
use tempfile::TempDir;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn second_claim_is_noop_without_llm() {
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
        })
        .unwrap();

    // 先手动 claim，模拟已有执行器在跑
    assert!(db.try_claim_running(&id).unwrap());

    let req = OrchestrationSpawnRequest {
        orchestration_id: id.clone(),
        parent_agent_id: "workspace".into(),
        provider: "openai".into(),
        model: "gpt-4o-mini".into(),
        api_key: "sk-test".into(),
        base_url: String::new(),
    };
    agent::orchestration::run_orchestration(req).await.unwrap();

    let orch = db.get(&id).unwrap().unwrap();
    // 仍为 running（未重新执行），未变成 failed/done
    assert_eq!(orch.status, OrchestrationStatus::Running.as_str());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn empty_api_key_marks_failed() {
    let dir = TempDir::new().unwrap();
    std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

    let db = OrchestrationDb::open_default().unwrap();
    let id = db
        .create(NewOrchestration {
            parent_agent_id: "workspace".into(),
            session_id: None,
            goal: "g".into(),
            steps: vec![NewOrchestrationStep {
                role: "researcher".into(),
                agent_id: None,
                prompt: "collect".into(),
            }],
        })
        .unwrap();

    let req = OrchestrationSpawnRequest {
        orchestration_id: id.clone(),
        parent_agent_id: "workspace".into(),
        provider: "openai".into(),
        model: "gpt-4o-mini".into(),
        api_key: String::new(),
        base_url: String::new(),
    };
    agent::orchestration::run_orchestration(req).await.unwrap();

    let orch = db.get(&id).unwrap().unwrap();
    assert_eq!(orch.status, OrchestrationStatus::Failed.as_str());
    let steps = db.list_steps(&id).unwrap();
    assert_eq!(steps[0].status, "failed");
    assert!(steps[0]
        .error
        .as_deref()
        .unwrap_or("")
        .contains("API Key"));
}
