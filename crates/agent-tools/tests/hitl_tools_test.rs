//! ask_user（question/confirm/location）HITL 工具输出须通过 A2UI catalog 校验。

use tempfile::TempDir;
use tools::{register_all, ToolContext, ToolRegistry};

fn make_ctx(
    dir: &TempDir,
) -> (
    std::sync::RwLock<memory::MemoryManager>,
    session::SessionStore,
    tools::ImageGenTargets,
    std::path::PathBuf,
    tools::ModelCredentials,
) {
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
    let sessions =
        session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
    let targets = tools::ImageGenTargets::default();
    let creds = tools::ModelCredentials::default();
    (
        std::sync::RwLock::new(memory),
        sessions,
        targets,
        workspace,
        creds,
    )
}

#[tokio::test]
async fn confirm_emits_valid_a2ui_hitl() {
    let dir = TempDir::new().unwrap();
    let (memory, sessions, targets, workspace, creds) = make_ctx(&dir);
    let mut ctx = ToolContext {
        memory: &memory,
        sessions: &sessions,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: workspace,
        project_root: None,
        image_gen_targets: &targets,
        session_id: "test".into(),
        turn_id: None,
        credentials: &creds,
        chat_targets: &[],
        execution: None,
        permission_profile: None,
        skill_config_overrides: &[],
        hook_bus: None,
        hook_runtime: None,
        workspace_write_grant: false,
        sandbox_policy: None,
        network_grant: tools::InProcessNetworkGrant::default(),
        managed_network: None,
    };

    let raw = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "ask_user",
        &serde_json::json!({
            "mode": "confirm",
            "title": "Delete file?",
            "body": "report.pdf will be removed"
        }),
        None,
    )
    .await
    .unwrap();

    let v: serde_json::Value = serde_json::from_str(raw.text()).unwrap();
    assert_eq!(v["astro_hitl"], true);
    assert_eq!(v["reason"], "confirmation");
    let ops = v["operations"].as_array().expect("operations array");
    a2ui::validate_operations(ops).unwrap();
}

#[tokio::test]
async fn clarify_emits_valid_a2ui_hitl() {
    let dir = TempDir::new().unwrap();
    let (memory, sessions, targets, workspace, creds) = make_ctx(&dir);
    let mut ctx = ToolContext {
        memory: &memory,
        sessions: &sessions,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: workspace,
        project_root: None,
        image_gen_targets: &targets,

        session_id: "test".into(),
        turn_id: None,
        credentials: &creds,
        chat_targets: &[],
        execution: None,
        permission_profile: None,
        skill_config_overrides: &[],
        hook_bus: None,
        hook_runtime: None,
        workspace_write_grant: false,
        sandbox_policy: None,
        network_grant: tools::InProcessNetworkGrant::default(),
        managed_network: None,
    };

    let raw = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "ask_user",
        &serde_json::json!({
            "questions": [
                {
                    "id": "env",
                    "question": "Which env?",
                    "options": ["staging", "production"]
                }
            ]
        }),
        None,
    )
    .await
    .unwrap();

    let v: serde_json::Value = serde_json::from_str(raw.text()).unwrap();
    assert_eq!(v["astro_hitl"], true);
    assert_eq!(v["reason"], "input_required");
    assert_eq!(v["response_schema"]["required"][0], "answers");
    let ops = v["operations"].as_array().expect("operations array");
    a2ui::validate_operations(ops).unwrap();
    let has_wizard = ops.iter().any(|op| {
        op.pointer("/updateComponents/components")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter()
                    .any(|c| c.get("component").and_then(|n| n.as_str()) == Some("ClarifyWizard"))
            })
            .unwrap_or(false)
    });
    assert!(has_wizard);
}

#[tokio::test]
async fn clarify_free_text_step_allows_empty_options() {
    let dir = TempDir::new().unwrap();
    let (memory, sessions, targets, workspace, creds) = make_ctx(&dir);
    let mut ctx = ToolContext {
        memory: &memory,
        sessions: &sessions,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: workspace,
        project_root: None,
        image_gen_targets: &targets,

        session_id: "test".into(),
        turn_id: None,
        credentials: &creds,
        chat_targets: &[],
        execution: None,
        permission_profile: None,
        skill_config_overrides: &[],
        hook_bus: None,
        hook_runtime: None,
        workspace_write_grant: false,
        sandbox_policy: None,
        network_grant: tools::InProcessNetworkGrant::default(),
        managed_network: None,
    };

    let raw = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "ask_user",
        &serde_json::json!({
            "questions": [
                {
                    "id": "idea",
                    "question": "你想怎么做？",
                    "options": []
                }
            ]
        }),
        None,
    )
    .await
    .unwrap();

    let v: serde_json::Value = serde_json::from_str(raw.text()).unwrap();
    assert_eq!(v["astro_hitl"], true);
    let ops = v["operations"].as_array().expect("operations array");
    a2ui::validate_operations(ops).unwrap();
    let step_options = ops
        .iter()
        .find_map(|op| {
            op.pointer("/updateComponents/components")
                .and_then(|c| c.as_array())
                .and_then(|arr| {
                    arr.iter().find(|c| {
                        c.get("component").and_then(|n| n.as_str()) == Some("ClarifyWizard")
                    })
                })
                .and_then(|w| w.get("steps"))
                .and_then(|s| s.as_array())
                .and_then(|steps| steps.first())
                .and_then(|step| step.get("options"))
                .and_then(|o| o.as_array())
        })
        .expect("wizard step options");
    assert!(step_options.is_empty());
}

#[tokio::test]
async fn clarify_multi_emits_wizard_hitl() {
    let dir = TempDir::new().unwrap();
    let (memory, sessions, targets, workspace, creds) = make_ctx(&dir);
    let mut ctx = ToolContext {
        memory: &memory,
        sessions: &sessions,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: workspace,
        project_root: None,
        image_gen_targets: &targets,

        session_id: "test".into(),
        turn_id: None,
        credentials: &creds,
        chat_targets: &[],
        execution: None,
        permission_profile: None,
        skill_config_overrides: &[],
        hook_bus: None,
        hook_runtime: None,
        workspace_write_grant: false,
        sandbox_policy: None,
        network_grant: tools::InProcessNetworkGrant::default(),
        managed_network: None,
    };

    let raw = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "ask_user",
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
        None,
    )
    .await
    .unwrap();

    let v: serde_json::Value = serde_json::from_str(raw.text()).unwrap();
    assert_eq!(v["astro_hitl"], true);
    assert_eq!(v["reason"], "input_required");
    assert_eq!(v["response_schema"]["required"][0], "answers");
    let ops = v["operations"].as_array().expect("operations array");
    a2ui::validate_operations(ops).unwrap();
    let has_wizard = ops.iter().any(|op| {
        op.pointer("/updateComponents/components")
            .and_then(|c| c.as_array())
            .map(|arr| {
                arr.iter()
                    .any(|c| c.get("component").and_then(|n| n.as_str()) == Some("ClarifyWizard"))
            })
            .unwrap_or(false)
    });
    assert!(has_wizard);
}

#[tokio::test]
async fn ask_user_location_mode_emits_valid_a2ui_hitl() {
    let dir = TempDir::new().unwrap();
    let (memory, sessions, targets, workspace, creds) = make_ctx(&dir);
    let mut ctx = ToolContext {
        memory: &memory,
        sessions: &sessions,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: workspace,
        project_root: None,
        image_gen_targets: &targets,

        session_id: "test".into(),
        turn_id: None,
        credentials: &creds,
        chat_targets: &[],
        execution: None,
        permission_profile: None,
        skill_config_overrides: &[],
        hook_bus: None,
        hook_runtime: None,
        workspace_write_grant: false,
        sandbox_policy: None,
        network_grant: tools::InProcessNetworkGrant::default(),
        managed_network: None,
    };

    let raw = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "ask_user",
        &serde_json::json!({
            "mode": "location",
            "message": "Need location for weather"
        }),
        None,
    )
    .await
    .unwrap();

    let v: serde_json::Value = serde_json::from_str(raw.text()).unwrap();
    assert_eq!(v["astro_hitl"], true);
    assert_eq!(v["reason"], "location_required");
    let ops = v["operations"].as_array().expect("operations array");
    a2ui::validate_operations(ops).unwrap();
}

#[tokio::test]
async fn present_emits_valid_astro_ui() {
    let dir = TempDir::new().unwrap();
    let (memory, sessions, targets, workspace, creds) = make_ctx(&dir);
    let mut ctx = ToolContext {
        memory: &memory,
        sessions: &sessions,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: workspace,
        project_root: None,
        image_gen_targets: &targets,

        session_id: "test".into(),
        turn_id: None,
        credentials: &creds,
        chat_targets: &[],
        execution: None,
        permission_profile: None,
        skill_config_overrides: &[],
        hook_bus: None,
        hook_runtime: None,
        workspace_write_grant: false,
        sandbox_policy: None,
        network_grant: tools::InProcessNetworkGrant::default(),
        managed_network: None,
    };

    let raw = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "present",
        &serde_json::json!({
            "title": "Weather",
            "body": "Sunny, 26°C",
            "image_url": "https://example.com/wx.png"
        }),
        None,
    )
    .await
    .unwrap();

    let v: serde_json::Value = serde_json::from_str(raw.text()).unwrap();
    assert_eq!(v["astro_ui"], true);
    assert!(v.get("astro_hitl").is_none());
    assert_eq!(v["summary"], "Weather");
    let ops = v["operations"].as_array().expect("operations array");
    a2ui::validate_operations(ops).unwrap();
}

#[tokio::test]
async fn ask_user_rejects_mixed_questions_and_body() {
    let dir = TempDir::new().unwrap();
    let (memory, sessions, targets, workspace, creds) = make_ctx(&dir);
    let mut ctx = ToolContext {
        memory: &memory,
        sessions: &sessions,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: workspace,
        project_root: None,
        image_gen_targets: &targets,

        session_id: "test".into(),
        turn_id: None,
        credentials: &creds,
        chat_targets: &[],
        execution: None,
        permission_profile: None,
        skill_config_overrides: &[],
        hook_bus: None,
        hook_runtime: None,
        workspace_write_grant: false,
        sandbox_policy: None,
        network_grant: tools::InProcessNetworkGrant::default(),
        managed_network: None,
    };

    let err = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "ask_user",
        &serde_json::json!({
            "questions": [{ "question": "Which?" }],
            "body": "approve?"
        }),
        None,
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains("mix") || err.to_string().contains("mode="),
        "{err}"
    );
}

#[tokio::test]
async fn register_all_includes_ask_user() {
    let mut registry = ToolRegistry::new();
    register_all(&mut registry);
    let names: Vec<_> = registry
        .available_tools()
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert!(names.contains(&"ask_user"));
    assert!(!names.contains(&"ask"));
    assert!(!names.contains(&"confirm"));
    assert!(!names.contains(&"clarify"));
    assert!(!names.contains(&"request_user_location"));
    assert!(names.contains(&"present"));
}
