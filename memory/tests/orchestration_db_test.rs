//! orchestration.db 测试

use memory::orchestration_db::{
    NewOrchestration, NewOrchestrationStep, OrchestrationDb, OrchestrationStatus, StepStatus,
};
use tempfile::TempDir;

#[test]
fn create_and_list_steps_in_order() {
    let dir = TempDir::new().unwrap();
    let db = OrchestrationDb::new(dir.path().join("orchestration.db")).unwrap();
    let id = db
        .create(NewOrchestration {
            parent_agent_id: "workspace".into(),
            session_id: Some("s1".into()),
            goal: "写周报".into(),
            steps: vec![
                NewOrchestrationStep {
                    role: "researcher".into(),
                    agent_id: None,
                    prompt: "收集素材".into(),
                },
                NewOrchestrationStep {
                    role: "writer".into(),
                    agent_id: Some("workspace".into()),
                    prompt: "起草".into(),
                },
            ],
        })
        .unwrap();
    let orch = db.get(&id).unwrap().unwrap();
    assert_eq!(orch.status, OrchestrationStatus::Queued.as_str());
    let steps = db.list_steps(&id).unwrap();
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].seq, 0);
    assert_eq!(steps[1].seq, 1);
    assert_eq!(steps[0].status, StepStatus::Pending.as_str());
}

#[test]
fn mark_step_failed_stops_semantics_helpers() {
    let dir = TempDir::new().unwrap();
    let db = OrchestrationDb::new(dir.path().join("o.db")).unwrap();
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
    let steps = db.list_steps(&id).unwrap();
    db.set_orchestration_status(&id, OrchestrationStatus::Running, None, None)
        .unwrap();
    db.set_step_running(&steps[0].id).unwrap();
    db.set_step_failed(&steps[0].id, "boom").unwrap();
    db.set_orchestration_status(&id, OrchestrationStatus::Failed, Some("boom"), None)
        .unwrap();
    let orch = db.get(&id).unwrap().unwrap();
    assert_eq!(orch.status, "failed");
}

#[test]
fn try_claim_running_cas_only_queued() {
    let dir = TempDir::new().unwrap();
    let db = OrchestrationDb::new(dir.path().join("o.db")).unwrap();
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

    assert!(db.try_claim_running(&id).unwrap());
    let orch = db.get(&id).unwrap().unwrap();
    assert_eq!(orch.status, OrchestrationStatus::Running.as_str());

    assert!(!db.try_claim_running(&id).unwrap());
    let orch = db.get(&id).unwrap().unwrap();
    assert_eq!(orch.status, OrchestrationStatus::Running.as_str());
}
