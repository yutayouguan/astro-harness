use tools::{register_all, ToolRegistry};
use types::ToolMode;

fn names(specs: &[serde_json::Value]) -> Vec<String> {
    specs
        .iter()
        .flat_map(|spec| {
            if spec["type"] == "namespace" {
                let namespace = spec["name"].as_str().unwrap_or_default();
                return spec["tools"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|tool| tool["name"].as_str())
                    .map(|name| format!("{namespace}.{name}"))
                    .collect::<Vec<_>>();
            }
            spec["name"]
                .as_str()
                .map(str::to_string)
                .into_iter()
                .collect()
        })
        .collect()
}

#[test]
fn direct_only_exposes_regular_tools() {
    let mut registry = ToolRegistry::new();
    register_all(&mut registry);
    let names = names(
        &registry
            .schemas_for_api_with_mode(ToolMode::Direct)
            .unwrap(),
    );
    assert!(names.contains(&"exec_command".to_string()));
    assert!(!names.contains(&"exec".to_string()));
    assert!(!names.contains(&"wait".to_string()));
}

#[test]
fn code_mode_exposes_regular_and_control_tools() {
    let mut registry = ToolRegistry::new();
    register_all(&mut registry);
    let names = names(
        &registry
            .schemas_for_api_with_mode(ToolMode::CodeMode)
            .unwrap(),
    );
    assert!(names.contains(&"exec_command".to_string()));
    assert!(names.contains(&"exec".to_string()));
    assert!(names.contains(&"wait".to_string()));
}

#[test]
fn code_mode_only_exposes_only_control_tools() {
    let mut registry = ToolRegistry::new();
    register_all(&mut registry);
    let names = names(
        &registry
            .schemas_for_api_with_mode(ToolMode::CodeModeOnly)
            .unwrap(),
    );
    assert_eq!(names, vec!["exec", "wait"]);
}

#[test]
fn unavailable_runtime_only_downgrades_hybrid_mode() {
    let mut registry = ToolRegistry::new();
    register_all(&mut registry);
    registry.unregister("wait");

    assert_eq!(
        registry.effective_tool_mode(ToolMode::CodeMode).unwrap(),
        ToolMode::Direct
    );
    assert!(registry
        .effective_tool_mode(ToolMode::CodeModeOnly)
        .is_err());
}

#[test]
fn code_mode_controls_are_not_user_catalog_entries() {
    assert!(!tools::builtin_catalog()
        .iter()
        .any(|toolset| toolset.id == "code_mode"));
}
