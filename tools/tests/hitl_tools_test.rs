//! confirm / clarify / request_user_location HITL 工具输出须通过 A2UI catalog 校验。

use tempfile::TempDir;
use tools::{register_all, ToolContext, ToolRegistry};

fn make_ctx(
    dir: &TempDir,
) -> (
    memory::MemoryManager,
    session::SessionStore,
    providers::registry::ProviderRegistry,
    tools::ImageGenTargets,
    std::path::PathBuf,
) {
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
    let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
    let providers = providers::registry::ProviderRegistry::new();
    let targets = tools::ImageGenTargets::default();
    (memory, sessions, providers, targets, workspace)
}

#[tokio::test]
async fn confirm_emits_valid_a2ui_hitl() {
    let dir = TempDir::new().unwrap();
    let (mut memory, sessions, providers, targets, workspace) = make_ctx(&dir);
    let mut ctx = ToolContext {
        memory: &mut memory,
        sessions: &sessions,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: workspace,
        project_root: None,
        image_gen_targets: &targets,
        providers: &providers,
        session_id: "test".into(),
        turn_id: None,
        chat_api_key: String::new(),
        chat_base_url: String::new(),
        chat_provider: String::new(),
        chat_model: String::new(),
        chat_targets: vec![],
        delegate_runner: None,
        async_spawner: None,
        orchestration_spawner: None,
        hook_bus: None,
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
    let (mut memory, sessions, providers, targets, workspace) = make_ctx(&dir);
    let mut ctx = ToolContext {
        memory: &mut memory,
        sessions: &sessions,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: workspace,
        project_root: None,
        image_gen_targets: &targets,
        providers: &providers,
        session_id: "test".into(),
        turn_id: None,
        chat_api_key: String::new(),
        chat_base_url: String::new(),
        chat_provider: String::new(),
        chat_model: String::new(),
        chat_targets: vec![],
        delegate_runner: None,
        async_spawner: None,
        orchestration_spawner: None,
        hook_bus: None,
    };

    let raw = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "clarify",
        &serde_json::json!({
            "questions": [
                {
                    "id": "env",
                    "question": "Which env?",
                    "options": ["staging", "production"]
                }
            ]
        }),
    )
    .await
    .unwrap();

    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["astro_hitl"], true);
    assert_eq!(v["reason"], "input_required");
    assert_eq!(v["response_schema"]["required"][0], "answers");
    let ops = v["operations"].as_array().expect("operations array");
    a2ui::validate_operations(ops).unwrap();
    let has_wizard = ops.iter().any(|op| {
        op.pointer("/updateComponents/components")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter().any(|c| {
                    c.get("component").and_then(|n| n.as_str()) == Some("ClarifyWizard")
                })
            })
            .unwrap_or(false)
    });
    assert!(has_wizard);
}

#[tokio::test]
async fn clarify_multi_emits_wizard_hitl() {
    let dir = TempDir::new().unwrap();
    let (mut memory, sessions, providers, targets, workspace) = make_ctx(&dir);
    let mut ctx = ToolContext {
        memory: &mut memory,
        sessions: &sessions,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: workspace,
        project_root: None,
        image_gen_targets: &targets,
        providers: &providers,
        session_id: "test".into(),
        turn_id: None,
        chat_api_key: String::new(),
        chat_base_url: String::new(),
        chat_provider: String::new(),
        chat_model: String::new(),
        chat_targets: vec![],
        delegate_runner: None,
        async_spawner: None,
        orchestration_spawner: None,
        hook_bus: None,
    };

    let raw = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "clarify",
        &serde_json::json!({
            "title": "开干前确认",
            "questions": [
                {
                    "id": "style",
                    "question": "风格偏好？",
                    "options": ["民谣", "电子", "雷鬼"]
                },
                {
                    "id": "lyrics",
                    "question": "歌词？",
                    "options": ["你写", "纯音乐"]
                },
                {
                    "id": "mood",
                    "question": "氛围？",
                    "options": ["欢快洗脑", "优美自然"]
                }
            ]
        }),
    )
    .await
    .unwrap();

    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["astro_hitl"], true);
    assert_eq!(v["reason"], "input_required");
    assert_eq!(v["response_schema"]["required"][0], "answers");
    let ops = v["operations"].as_array().expect("operations array");
    a2ui::validate_operations(ops).unwrap();
    let has_wizard = ops.iter().any(|op| {
        op.pointer("/updateComponents/components")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter().any(|c| {
                    c.get("component").and_then(|n| n.as_str()) == Some("ClarifyWizard")
                })
            })
            .unwrap_or(false)
    });
    assert!(has_wizard);
}

#[tokio::test]
async fn request_user_location_emits_valid_a2ui_hitl() {
    let dir = TempDir::new().unwrap();
    let (mut memory, sessions, providers, targets, workspace) = make_ctx(&dir);
    let mut ctx = ToolContext {
        memory: &mut memory,
        sessions: &sessions,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: workspace,
        project_root: None,
        image_gen_targets: &targets,
        providers: &providers,
        session_id: "test".into(),
        turn_id: None,
        chat_api_key: String::new(),
        chat_base_url: String::new(),
        chat_provider: String::new(),
        chat_model: String::new(),
        chat_targets: vec![],
        delegate_runner: None,
        async_spawner: None,
        orchestration_spawner: None,
        hook_bus: None,
    };

    let raw = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "request_user_location",
        &serde_json::json!({
            "message": "Need location for weather"
        }),
    )
    .await
    .unwrap();

    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["astro_hitl"], true);
    assert_eq!(v["reason"], "location_required");
    let ops = v["operations"].as_array().expect("operations array");
    a2ui::validate_operations(ops).unwrap();
}

#[tokio::test]
async fn present_ui_emits_valid_astro_ui() {
    let dir = TempDir::new().unwrap();
    let (mut memory, sessions, providers, targets, workspace) = make_ctx(&dir);
    let mut ctx = ToolContext {
        memory: &mut memory,
        sessions: &sessions,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: workspace,
        project_root: None,
        image_gen_targets: &targets,
        providers: &providers,
        session_id: "test".into(),
        turn_id: None,
        chat_api_key: String::new(),
        chat_base_url: String::new(),
        chat_provider: String::new(),
        chat_model: String::new(),
        chat_targets: vec![],
        delegate_runner: None,
        async_spawner: None,
        orchestration_spawner: None,
        hook_bus: None,
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
    assert!(names.contains(&"request_user_location"));
    assert!(names.contains(&"present_ui"));
}
