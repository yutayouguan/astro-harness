//! orchestration.db 测试

use orchestration::db::{
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
            provider: String::new(),
            model: String::new(),
            api_key: String::new(),
            base_url: String::new(),
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
            provider: String::new(),
            model: String::new(),
            api_key: String::new(),
            base_url: String::new(),
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
            provider: String::new(),
            model: String::new(),
            api_key: String::new(),
            base_url: String::new(),
        })
        .unwrap();

    assert!(db.try_claim_running(&id).unwrap());
    let orch = db.get(&id).unwrap().unwrap();
    assert_eq!(orch.status, OrchestrationStatus::Running.as_str());

    assert!(!db.try_claim_running(&id).unwrap());
    let orch = db.get(&id).unwrap().unwrap();
    assert_eq!(orch.status, OrchestrationStatus::Running.as_str());
}

#[test]
fn list_in_period_filters_by_created_at_and_agent() {
    let dir = TempDir::new().unwrap();
    let db = OrchestrationDb::new(dir.path().join("orchestration.db")).unwrap();

    let id_in = db
        .create(NewOrchestration {
            parent_agent_id: "alice".into(),
            session_id: None,
            goal: "in-window".into(),
            steps: vec![NewOrchestrationStep {
                role: "r".into(),
                agent_id: None,
                prompt: "p".into(),
            }],
            provider: String::new(),
            model: String::new(),
            api_key: String::new(),
            base_url: String::new(),
        })
        .unwrap();
    let id_out = db
        .create(NewOrchestration {
            parent_agent_id: "bob".into(),
            session_id: None,
            goal: "out".into(),
            steps: vec![NewOrchestrationStep {
                role: "r".into(),
                agent_id: None,
                prompt: "p".into(),
            }],
            provider: String::new(),
            model: String::new(),
            api_key: String::new(),
            base_url: String::new(),
        })
        .unwrap();

    // 将 id_in 钉在窗内、id_out 钉在窗外（秒精度，与 period_window 一致）
    db.set_created_at_for_test(&id_in, "2026-07-10T12:00:00Z")
        .unwrap();
    db.set_created_at_for_test(&id_out, "2026-05-01T12:00:00Z")
        .unwrap();

    let rows = db
        .list_in_period(
            "2026-07-01T00:00:00Z",
            "2026-08-01T00:00:00Z",
            Some("alice"),
            50,
        )
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, id_in);
    let steps = db.list_steps(&rows[0].id).unwrap();
    assert_eq!(steps.len(), 1);
}

#[test]
fn create_writes_second_precision_timestamps() {
    let dir = TempDir::new().unwrap();
    let db = OrchestrationDb::new(dir.path().join("orchestration.db")).unwrap();
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
            provider: String::new(),
            model: String::new(),
            api_key: String::new(),
            base_url: String::new(),
        })
        .unwrap();
    let orch = db.get(&id).unwrap().unwrap();
    // 秒精度 RFC3339：`…00Z`，不含小数点
    assert!(
        !orch.created_at.contains('.'),
        "created_at should be second precision, got {}",
        orch.created_at
    );
    assert!(
        orch.created_at.ends_with('Z'),
        "created_at should end with Z, got {}",
        orch.created_at
    );
}

#[test]
fn list_in_period_month_boundary_with_second_precision() {
    let dir = TempDir::new().unwrap();
    let db = OrchestrationDb::new(dir.path().join("orchestration.db")).unwrap();

    let id_last = db
        .create(NewOrchestration {
            parent_agent_id: "alice".into(),
            session_id: None,
            goal: "july-last".into(),
            steps: vec![NewOrchestrationStep {
                role: "r".into(),
                agent_id: None,
                prompt: "p".into(),
            }],
            provider: String::new(),
            model: String::new(),
            api_key: String::new(),
            base_url: String::new(),
        })
        .unwrap();
    let id_first = db
        .create(NewOrchestration {
            parent_agent_id: "alice".into(),
            session_id: None,
            goal: "aug-first".into(),
            steps: vec![NewOrchestrationStep {
                role: "r".into(),
                agent_id: None,
                prompt: "p".into(),
            }],
            provider: String::new(),
            model: String::new(),
            api_key: String::new(),
            base_url: String::new(),
        })
        .unwrap();

    // 月末最后一秒应落入 7 月窗；月初边界秒属半开区间 [start, end) 的 end，不入 7 月
    db.set_created_at_for_test(&id_last, "2026-07-31T23:59:59Z")
        .unwrap();
    db.set_created_at_for_test(&id_first, "2026-08-01T00:00:00Z")
        .unwrap();

    let july = db
        .list_in_period(
            "2026-07-01T00:00:00Z",
            "2026-08-01T00:00:00Z",
            Some("alice"),
            50,
        )
        .unwrap();
    assert_eq!(july.len(), 1);
    assert_eq!(july[0].id, id_last);

    let aug = db
        .list_in_period(
            "2026-08-01T00:00:00Z",
            "2026-09-01T00:00:00Z",
            Some("alice"),
            50,
        )
        .unwrap();
    assert_eq!(aug.len(), 1);
    assert_eq!(aug[0].id, id_first);
}

#[test]
fn skip_pending_steps_after_marks_later_pending() {
    let dir = TempDir::new().unwrap();
    let db = OrchestrationDb::new(dir.path().join("orchestration.db")).unwrap();
    let id = db
        .create(NewOrchestration {
            parent_agent_id: "workspace".into(),
            session_id: None,
            goal: "g".into(),
            steps: vec![
                NewOrchestrationStep {
                    role: "a".into(),
                    agent_id: None,
                    prompt: "p0".into(),
                },
                NewOrchestrationStep {
                    role: "b".into(),
                    agent_id: None,
                    prompt: "p1".into(),
                },
                NewOrchestrationStep {
                    role: "c".into(),
                    agent_id: None,
                    prompt: "p2".into(),
                },
            ],
            provider: String::new(),
            model: String::new(),
            api_key: String::new(),
            base_url: String::new(),
        })
        .unwrap();
    let steps = db.list_steps(&id).unwrap();
    db.set_step_running(&steps[0].id).unwrap();
    db.set_step_failed(&steps[0].id, "boom").unwrap();
    let n = db.skip_pending_steps_after(&id, steps[0].seq).unwrap();
    assert_eq!(n, 2);
    let steps = db.list_steps(&id).unwrap();
    assert_eq!(steps[0].status, StepStatus::Failed.as_str());
    assert_eq!(steps[1].status, StepStatus::Skipped.as_str());
    assert_eq!(steps[2].status, StepStatus::Skipped.as_str());
}

#[test]
fn orchestration_db_impls_sqlite_store() {
    use types::SqliteStore;
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("orchestration.db");
    let db = OrchestrationDb::new(path.clone()).unwrap();
    assert_eq!(SqliteStore::path(&db), path.as_path());
    db.migrate().unwrap();
}
