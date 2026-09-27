//! 工具注册表启用过滤与 schema 导出测试。

use std::collections::HashMap;

use agent::{ToolEntry, ToolRegistry};
use tempfile::TempDir;

#[test]
fn test_tool_registration_and_dispatch() {
    let mut registry = ToolRegistry::new();
    registry.register(ToolEntry {
        name: "web_search".to_string(),
        toolset: "web".to_string(),
        description: "搜索互联网".to_string(),
        schema: serde_json::json!({"type": "object", "properties": {"query": {"type": "string"}}}),
        check_fn: None,
        icon: "🔍",
        ..ToolEntry::lifecycle_defaults()
    });
    let tools = registry.available_tools();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "web_search");
}

#[test]
fn test_check_fn_filters_unavailable() {
    let mut registry = ToolRegistry::new();
    registry.register(ToolEntry {
        name: "disabled_tool".to_string(),
        toolset: "test".to_string(),
        description: "不可用工具".to_string(),
        schema: serde_json::json!({}),
        check_fn: Some(std::sync::Arc::new(|| false)),
        icon: "❌",
        ..ToolEntry::lifecycle_defaults()
    });
    let tools = registry.available_tools();
    assert_eq!(tools.len(), 0);
}

#[test]
fn test_enabled_map_filters_toolset() {
    let mut registry = ToolRegistry::new();
    registry.register(ToolEntry {
        name: "memory".to_string(),
        toolset: "memory".to_string(),
        description: "add".to_string(),
        schema: serde_json::json!({}),
        check_fn: None,
        icon: "🧠",
        ..ToolEntry::lifecycle_defaults()
    });
    registry.register(ToolEntry {
        name: "cron".to_string(),
        toolset: "cron".to_string(),
        description: "list".to_string(),
        schema: serde_json::json!({}),
        check_fn: None,
        icon: "⏰",
        ..ToolEntry::lifecycle_defaults()
    });
    let mut enabled = std::collections::HashMap::new();
    enabled.insert("memory".into(), false);
    enabled.insert("cron".into(), true);
    registry.set_enabled_map(enabled);

    let tools = registry.available_tools();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "cron");
    assert!(!registry.is_tool_allowed("memory"));
    assert!(registry.is_tool_allowed("cron"));
}

#[test]
fn mcp_disabled_tools_not_in_schemas_for_api() {
    // 对齐 attach_mcp_tools：仅注册 Hub 过滤后的条目
    let mut server = mcp::McpServerConfig {
        id: "demo".into(),
        name: "demo".into(),
        description: String::new(),
        r#type: mcp::McpTransportType::Stdio,
        command: "npx".into(),
        args: vec![],
        env: HashMap::new(),
        env_vars: vec![],
        url: String::new(),
        headers: HashMap::new(),
        bearer_token_env_var: None,
        env_http_headers: HashMap::new(),
        auth: None,
        enabled: true,
        required: false,
        cwd: None,
        enabled_tools: None,
        disabled_tools: vec![],
        default_tools_approval_mode: types::McpToolApprovalMode::Auto,
        tools: HashMap::from([
            ("keep".into(), mcp::McpToolConfig::Enabled(true)),
            ("drop".into(), mcp::McpToolConfig::Enabled(false)),
        ]),
        discovered: vec![],
        startup_timeout_secs: None,
        tool_timeout_secs: None,
    };
    let discovered = vec!["keep".into(), "drop".into(), "unset".into()];
    let qualified = mcp::filter_enabled_tool_names(&server, &discovered);

    let mut reg = ToolRegistry::new();
    for name in &qualified {
        reg.register(ToolEntry {
            name: name.clone(),
            toolset: mcp::MCP_TOOLSET.to_string(),
            namespace: mcp::tool_namespace("demo"),
            description: "mcp".into(),
            schema: serde_json::json!({"type": "object", "properties": {}}),
            check_fn: None,
            icon: "plug",
            ..ToolEntry::lifecycle_defaults()
        });
    }

    let specs = reg.schemas_for_api();
    let namespace = specs
        .iter()
        .find(|spec| spec["type"] == "namespace" && spec["name"] == "mcp__demo")
        .expect("native MCP namespace");
    let names: Vec<_> = namespace["tools"]
        .as_array()
        .expect("namespace tools")
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert!(names.contains(&"keep"));
    assert!(names.contains(&"unset"));
    assert!(!names.contains(&"drop"));
    assert!(!reg.is_tool_allowed("mcp__demo__drop"));

    server.enabled = false;
    assert!(mcp::filter_enabled_tool_names(&server, &discovered).is_empty());
}

#[test]
fn reload_uses_agent_specific_tools_enabled() {
    let dir = TempDir::new().unwrap();
    // 进程级 env 覆盖必须走共享 guard：串行化 + Drop 还原。
    let _env = home::test_env::AstroMemoryDirGuard::set(dir.path());

    let mut global = HashMap::new();
    global.insert("memory".into(), true);
    home::save_tools_enabled(&global).unwrap();

    let mut custom = HashMap::new();
    custom.insert("memory".into(), false);
    custom.insert("cron".into(), true);
    home::save_tools_enabled_for_agent(Some("custom-bot"), &custom).unwrap();

    let mut reg = ToolRegistry::new();
    reg.register(ToolEntry {
        name: "memory".into(),
        toolset: "memory".into(),
        description: "add".into(),
        schema: serde_json::json!({"type": "object"}),
        check_fn: None,
        icon: "brain",
        ..ToolEntry::lifecycle_defaults()
    });
    reg.reload_enabled_from_disk(Some("custom-bot"));
    assert!(!reg.is_tool_allowed("memory"));
    assert!(reg.schemas_for_api().iter().all(|s| {
        s.get("name")
            .or_else(|| s.pointer("/function/name"))
            .and_then(|n| n.as_str())
            != Some("memory")
    }));
}
