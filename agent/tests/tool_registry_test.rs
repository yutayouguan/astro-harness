//! 工具注册表启用过滤与 schema 导出测试。

use agent::{ToolEntry, ToolRegistry};

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
        check_fn: Some(Box::new(|| false)),
        icon: "❌",
    });
    let tools = registry.available_tools();
    assert_eq!(tools.len(), 0);
}

#[test]
fn test_enabled_map_filters_toolset() {
    let mut registry = ToolRegistry::new();
    registry.register(ToolEntry {
        name: "memory_add".to_string(),
        toolset: "memory".to_string(),
        description: "add".to_string(),
        schema: serde_json::json!({}),
        check_fn: None,
        icon: "🧠",
    });
    registry.register(ToolEntry {
        name: "cron_list".to_string(),
        toolset: "scheduled".to_string(),
        description: "list".to_string(),
        schema: serde_json::json!({}),
        check_fn: None,
        icon: "⏰",
    });
    let mut enabled = std::collections::HashMap::new();
    enabled.insert("memory".into(), false);
    enabled.insert("scheduled".into(), true);
    registry.set_enabled_map(enabled);

    let tools = registry.available_tools();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "cron_list");
    assert!(!registry.is_tool_allowed("memory_add"));
    assert!(registry.is_tool_allowed("cron_list"));
}
