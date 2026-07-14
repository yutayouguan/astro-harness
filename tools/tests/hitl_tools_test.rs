//! confirm / clarify HITL 工具输出须通过 A2UI catalog 校验。

use tempfile::TempDir;
use tools::{register_all, ToolContext, ToolRegistry};

fn make_ctx(
    dir: &TempDir,
) -> (
    memory::MemoryManager,
    providers::registry::ProviderRegistry,
    tools::ImageGenTargets,
    std::path::PathBuf,
) {
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
    let providers = providers::registry::ProviderRegistry::new();
    let targets = tools::ImageGenTargets::default();
    (memory, providers, targets, workspace)
}

#[tokio::test]
async fn confirm_emits_valid_a2ui_hitl() {
    let dir = TempDir::new().unwrap();
    let (mut memory, providers, targets, workspace) = make_ctx(&dir);
    let mut ctx = ToolContext {
        memory: &mut memory,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: workspace,
        project_root: None,
        image_gen_targets: &targets,
        providers: &providers,
        session_id: "test".into(),
        chat_api_key: String::new(),
        chat_base_url: String::new(),
        chat_provider: String::new(),
        chat_model: String::new(),
        chat_targets: vec![],
    delegate_runner: None,
    async_spawner: None,
    orchestration_spawner: None,
    };

    let raw = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "confirm",
        &serde_json::json!({
            "title": "Delete file?",
            "body": "report.pdf will be removed"
        }),
    )
    .await
    .unwrap();

    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["astro_hitl"], true);
    assert_eq!(v["reason"], "confirmation");
    let ops = v["operations"].as_array().expect("operations array");
    a2ui::validate_operations(ops).unwrap();
}

#[tokio::test]
async fn clarify_emits_valid_a2ui_hitl() {
    let dir = TempDir::new().unwrap();
    let (mut memory, providers, targets, workspace) = make_ctx(&dir);
    let mut ctx = ToolContext {
        memory: &mut memory,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: workspace,
        project_root: None,
        image_gen_targets: &targets,
        providers: &providers,
        session_id: "test".into(),
        chat_api_key: String::new(),
        chat_base_url: String::new(),
        chat_provider: String::new(),
        chat_model: String::new(),
        chat_targets: vec![],
    delegate_runner: None,
    async_spawner: None,
    orchestration_spawner: None,
    };

    let raw = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "clarify",
        &serde_json::json!({
            "question": "Which env?",
            "options": ["staging", "production"]
        }),
    )
    .await
    .unwrap();

    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["astro_hitl"], true);
    assert_eq!(v["reason"], "input_required");
    let ops = v["operations"].as_array().expect("operations array");
    a2ui::validate_operations(ops).unwrap();
}

#[tokio::test]
async fn present_ui_emits_valid_astro_ui() {
    let dir = TempDir::new().unwrap();
    let (mut memory, providers, targets, workspace) = make_ctx(&dir);
    let mut ctx = ToolContext {
        memory: &mut memory,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: workspace,
        project_root: None,
        image_gen_targets: &targets,
        providers: &providers,
        session_id: "test".into(),
        chat_api_key: String::new(),
        chat_base_url: String::new(),
        chat_provider: String::new(),
        chat_model: String::new(),
        chat_targets: vec![],
    delegate_runner: None,
    async_spawner: None,
    orchestration_spawner: None,
    };

    let raw = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "present_ui",
        &serde_json::json!({
            "title": "Weather",
            "body": "Sunny, 26°C",
            "image_url": "https://example.com/wx.png"
        }),
    )
    .await
    .unwrap();

    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["astro_ui"], true);
    assert!(v.get("astro_hitl").is_none());
    assert_eq!(v["summary"], "Weather");
    let ops = v["operations"].as_array().expect("operations array");
    a2ui::validate_operations(ops).unwrap();
}

#[tokio::test]
async fn register_all_includes_confirm() {
    let mut registry = ToolRegistry::new();
    register_all(&mut registry);
    let names: Vec<_> = registry
        .available_tools()
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert!(names.contains(&"confirm"));
    assert!(names.contains(&"clarify"));
    assert!(names.contains(&"present_ui"));
}
