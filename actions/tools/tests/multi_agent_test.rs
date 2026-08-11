//! pipeline 转调 orchestration 冒烟测试。

use tools::ToolContext;

#[tokio::test]
async fn pipeline_queues_orchestration() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

    let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
    let sessions =
        session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
    let targets = tools::ImageGenTargets::default();
    let creds = tools::ModelCredentials {
        provider: "openai".into(),
        model: "test".into(),
        api_key: "k".into(),
        base_url: String::new(),
    };
    let mut ctx = ToolContext {
        memory: &mut memory,
        sessions: &sessions,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: dir.path().to_path_buf(),
        project_root: None,
        image_gen_targets: &targets,
        session_id: "s".into(),
        turn_id: None,
        credentials: &creds,
        chat_targets: &[],
        execution: None,
        hook_bus: None,
    };

    let out = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "pipeline",
        &serde_json::json!({
            "goal": "ship feature",
            "agents": ["researcher", "writer"]
        }),
        None,
    )
    .await
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(out.text()).unwrap();
    assert_eq!(v["status"], "queued");
    assert_eq!(v["status"], "queued");
    assert!(v["orchestration_id"].as_str().unwrap().len() > 4);
    let id = v["orchestration_id"].as_str().unwrap();
    assert!(!id.is_empty());

    let db = orchestration::OrchestrationDb::open_default().unwrap();
    let orch = db.get(id).unwrap().unwrap();
    assert_eq!(orch.goal, "ship feature");
    let steps = db.list_steps(id).unwrap();
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].role, "researcher");

    std::env::remove_var("ASTRO_MEMORY_DIR");
}

#[tokio::test]
async fn pipeline_rejects_empty_agents() {
    let dir = tempfile::tempdir().unwrap();
    let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
    let sessions =
        session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
    let targets = tools::ImageGenTargets::default();
    let creds = tools::ModelCredentials {
        provider: "openai".into(),
        model: "test".into(),
        api_key: "k".into(),
        base_url: String::new(),
    };
    let mut ctx = ToolContext {
        memory: &mut memory,
        sessions: &sessions,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: dir.path().to_path_buf(),
        project_root: None,
        image_gen_targets: &targets,
        session_id: "s".into(),
        turn_id: None,
        credentials: &creds,
        chat_targets: &[],
        execution: None,
        hook_bus: None,
    };
    let err = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "pipeline",
        &serde_json::json!({"goal": "x", "agents": []}),
        None,
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("agents"));
}
