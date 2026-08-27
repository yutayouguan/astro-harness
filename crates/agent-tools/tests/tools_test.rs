//! 内置工具注册与分发集成测试。

use tools::{builtin_handler_names, register_all, ToolRegistry};

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
        "cron_add",
        "cron_list",
        "cron_remove",
        "image_gen",
        "video_gen",
        "video_analyze",
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
        "todo",
    ] {
        assert!(names.contains(&expected), "missing {expected}");
    }
}

#[test]
fn builtin_tool_names_are_openai_compatible() {
    let mut registry = ToolRegistry::new();
    register_all(&mut registry);

    for tool in registry.all_tools() {
        assert!(
            !tool.name.is_empty()
                && tool
                    .name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
            "invalid provider-visible tool name: {}",
            tool.name
        );
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

