//! 内置工具注册与分发集成测试。

use tempfile::TempDir;
use tools::{register_all, ToolContext, ToolRegistry};

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
        "memory_add",
        "session_search",
        "cron_add",
        "image_gen",
        "file_ops",
        "terminal",
        "web_search",
        "browser",
        "code_exec",
        "vision",
        "tts",
        "skills",
        "clarify",
        "delegate",
        "multi_agent",
        "task_plan",
    ] {
        assert!(names.contains(&expected), "missing {expected}");
    }
}

#[tokio::test]
async fn file_ops_write_and_read() {
    let dir = TempDir::new().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
    let providers = providers::registry::ProviderRegistry::new();
    let targets = tools::ImageGenTargets::default();
    let mut ctx = ToolContext {
        memory: &mut memory,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: workspace.clone(),
        image_gen_targets: &targets,
        providers: &providers,
        session_id: "test".into(),
        chat_api_key: String::new(),
        chat_base_url: String::new(),
        chat_provider: String::new(),
        chat_model: String::new(),
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
}
