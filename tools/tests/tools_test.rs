//! 内置工具注册与分发集成测试。

use std::path::Path;
use tempfile::TempDir;
use tools::{builtin_handler_names, register_all, ToolContext, ToolRegistry};

#[tokio::test]
async fn register_all_includes_panel_tools() {
    let mut registry = ToolRegistry::new();
    register_all(&mut registry);
    let names: Vec<_> = registry
        .available_tools()
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    for expected in [
        "memory",
        "session_search",
        "search_context",
        "pin_context",
        "cron_add",
        "image_gen",
        "video_gen",
        "video_understand",
        "file_ops",
        "terminal",
        "web_search",
        "web_extract",
        "browser",
        "code_exec",
        "vision",
        "robotics",
        "tts",
        "skills",
        "clarify",
        "confirm",
        "request_user_location",
        "delegate",
        "delegate_async",
        "delegate_status",
        "delegate_collect",
        "delegate_cancel",
        "multi_agent",
        "task_plan",
    ] {
        assert!(names.contains(&expected), "missing {expected}");
    }
}

#[tokio::test]
async fn metadata_tools_have_handlers_without_legacy_memory_aliases() {
    let mut registry = ToolRegistry::new();
    register_all(&mut registry);
    let handlers = builtin_handler_names();
    for entry in registry.all_tools() {
        assert!(
            handlers.binary_search(&entry.name.as_str()).is_ok(),
            "metadata tool `{}` missing handler",
            entry.name
        );
    }
    for legacy in ["memory_add", "memory_replace", "memory_remove"] {
        assert!(
            handlers.binary_search(&legacy).is_err(),
            "legacy tool name still has handler: {legacy}"
        );
    }
}

#[tokio::test]
async fn file_ops_write_and_read() {
    let dir = TempDir::new().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
    let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
    let providers = providers::registry::ProviderRegistry::new();
    let targets = tools::ImageGenTargets::default();
    let mut ctx = ToolContext {
        memory: &mut memory,
        sessions: &sessions,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: workspace.clone(),
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

    let w = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "file_ops",
        &serde_json::json!({
            "path": "notes/hello.txt",
            "operation": "write",
            "content": "hello tools"
        }),
    )
    .await
    .unwrap();
    assert!(w.contains("已写入"));
    assert!(w.contains("notes/hello.txt"));
    assert!(!Path::new(w.trim_start_matches("已写入 ").trim()).is_absolute());

    let r = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "file_ops",
        &serde_json::json!({
            "path": "notes/hello.txt",
            "operation": "read"
        }),
    )
    .await
    .unwrap();
    assert_eq!(r, "hello tools");

    let missing = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "file_ops",
        &serde_json::json!({
            "path": "notes/x.txt",
            "operation": "write"
        }),
    )
    .await;
    assert!(missing.unwrap_err().to_string().contains("content"));

    let del_root = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "file_ops",
        &serde_json::json!({
            "path": ".",
            "operation": "delete"
        }),
    )
    .await;
    assert!(del_root.unwrap_err().to_string().contains("根目录"));
}
