use tools::{register_all, ToolRegistry};

#[test]
fn registry_exposes_direct_tools_and_tool_search_without_code_mode_controls() {
    let mut registry = ToolRegistry::new();
    register_all(&mut registry);
    let specs = registry.schemas_for_api();
    assert!(!specs.iter().any(|spec| spec["name"] == "exec"));
    assert!(!specs.iter().any(|spec| spec["name"] == "wait"));
    assert!(specs.iter().any(|spec| spec["name"] == "exec_command"));
    assert!(specs.iter().any(|spec| spec["type"] == "tool_search"));
}
