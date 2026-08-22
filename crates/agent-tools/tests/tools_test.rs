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
        "context_search",
        "pin_context",
        "cron.add",
        "cron.list",
        "cron.remove",
        "image_gen",
        "video_gen",
        "video_analyze",
        "file_ops",
        "terminal",
        "web_search",
        "web_fetch",
        "code_exec",
        "image_analyze",
        "robotics",
        "speech_gen",
        "skills",
        "ask_user",
        "spawn_agent",
        "list_agents",
        "followup_task",
        "send_message",
        "wait_agent",
        "interrupt_agent",
        "persona_create",
        "todo",
    ] {
        assert!(names.contains(&expected), "missing {expected}");
    }
}

#[test]
fn registers_only_codex_v2_agent_tools() {
    assert_eq!(
        tools::builtin::subagent::CODEX_V2_AGENT_TOOL_NAMES,
        [
            "spawn_agent",
            "list_agents",
            "send_message",
            "followup_task",
            "wait_agent",
            "interrupt_agent",
        ]
    );
    let mut registry = ToolRegistry::new();
    register_all(&mut registry);
    let mut names = registry
        .all_tools()
        .into_iter()
        .filter(|entry| entry.toolset == "subagents")
        .map(|entry| entry.name.as_str())
        .collect::<Vec<_>>();
    names.sort_unstable();

    let mut expected = vec![
        "spawn_agent",
        "list_agents",
        "send_message",
        "followup_task",
        "wait_agent",
        "interrupt_agent",
    ];
    expected.sort_unstable();
    assert_eq!(names, expected);

    for removed in [
        "read_agent",
        "close_agent",
        "send_message_to_agent",
        "wait_agents",
        "subagent",
        "delegate",
        "run_delegate",
        "direct_agent",
    ] {
        assert!(
            registry.get(removed).is_none(),
            "legacy tool remains: {removed}"
        );
        assert_ne!(
            home::tool_name_to_toolset(removed),
            "subagents",
            "legacy tool still routes through the subagents toolset: {removed}"
        );
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
    let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
    let sessions =
        session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
    let memory = std::sync::RwLock::new(memory);
    let targets = tools::ImageGenTargets::default();
    let creds = tools::ModelCredentials::default();
    let mut ctx = ToolContext {
        memory: &memory,
        sessions: &sessions,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: workspace.clone(),
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
        context_window: None,
        context_tokens_used: None,
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
        None,
    )
    .await
    .unwrap();
    assert!(w.text().contains("已写入"));
    assert!(w.text().contains("notes/hello.txt"));
    assert!(!Path::new(w.text().trim_start_matches("已写入 ").trim()).is_absolute());

    let r = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "file_ops",
        &serde_json::json!({
            "path": "notes/hello.txt",
            "operation": "read"
        }),
        None,
    )
    .await
    .unwrap();
    assert_eq!(r.text(), "hello tools");

    let missing = tools::dispatch_tool(
        |_| true,
        &mut ctx,
        "file_ops",
        &serde_json::json!({
            "path": "notes/x.txt",
            "operation": "write"
        }),
        None,
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
        None,
    )
    .await;
    assert!(del_root.unwrap_err().to_string().contains("根目录"));
}
