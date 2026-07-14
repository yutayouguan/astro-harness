//! multi_agent 转调 orchestration 冒烟测试。

use tools::ToolContext;

#[tokio::test]
async fn multi_agent_queues_orchestration() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

    let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
    let providers = providers::registry::ProviderRegistry::new();
    let targets = tools::ImageGenTargets::default();
    let mut ctx = ToolContext {
        memory: &mut memory,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: dir.path().to_path_buf(),
        project_root: None,
        image_gen_targets: &targets,
        providers: &providers,
        session_id: "s".into(),
        chat_api_key: "k".into(),
        chat_base_url: String::new(),
        chat_provider: "openai".into(),
        chat_model: "test".into(),
        chat_targets: vec![],
    delegate_runner: None,
    async_spawner: None,
    orchestration_spawner: None,
    };

    let out = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "multi_agent",
        &serde_json::json!({
            "goal": "ship feature",
            "agents": ["researcher", "writer"]
        }),
    )
    .await
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["status"], "queued");
    assert_eq!(v["via"], "multi_agent");
    let id = v["orchestration_id"].as_str().unwrap();
    assert!(!id.is_empty());

    let db = memory::OrchestrationDb::open_default().unwrap();
    let orch = db.get(id).unwrap().unwrap();
    assert_eq!(orch.goal, "ship feature");
    let steps = db.list_steps(id).unwrap();
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].role, "researcher");

    std::env::remove_var("ASTRO_MEMORY_DIR");
}

#[tokio::test]
async fn multi_agent_rejects_empty_agents() {
    let dir = tempfile::tempdir().unwrap();
    let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
    let providers = providers::registry::ProviderRegistry::new();
    let targets = tools::ImageGenTargets::default();
    let mut ctx = ToolContext {
        memory: &mut memory,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: dir.path().to_path_buf(),
        project_root: None,
        image_gen_targets: &targets,
        providers: &providers,
        session_id: "s".into(),
        chat_api_key: "k".into(),
        chat_base_url: String::new(),
        chat_provider: "openai".into(),
        chat_model: "test".into(),
        chat_targets: vec![],
    delegate_runner: None,
    async_spawner: None,
    orchestration_spawner: None,
    };
    let err = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "multi_agent",
        &serde_json::json!({"goal": "x", "agents": []}),
    )
    .await
    .unwrap_err();
    assert!(err.to_string().contains("agents"));
}
