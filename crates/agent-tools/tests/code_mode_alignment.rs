use tools::{register_all, ToolRegistry};

#[test]
fn registry_exposes_codex_code_mode_control_tools() {
    let mut registry = ToolRegistry::new();
    register_all(&mut registry);
    let specs = registry.schemas_for_api_with_mode(types::ToolMode::CodeMode);

    if !specs.iter().any(|spec| spec["name"] == "exec") {
        // Codex also falls back to direct tools when its Code Mode host is unavailable.
        return;
    }

    let exec = specs.iter().find(|spec| spec["name"] == "exec").unwrap();
    assert_eq!(exec["type"], "custom");
    assert_eq!(exec["format"]["syntax"], "lark");
    assert!(exec["format"]["definition"]
        .as_str()
        .unwrap()
        .contains("PRAGMA_LINE"));

    let wait = specs.iter().find(|spec| spec["name"] == "wait").unwrap();
    assert_eq!(wait["type"], "function");
    assert_eq!(
        wait["parameters"]["required"],
        serde_json::json!(["cell_id"])
    );
    assert_eq!(wait["parameters"]["additionalProperties"], false);
}

#[test]
fn registry_applies_codex_tool_modes() {
    let mut registry = tools::ToolRegistry::new();
    tools::register_all(&mut registry);

    let direct = registry.schemas_for_api_with_mode(types::ToolMode::Direct);
    assert!(!direct.iter().any(|spec| spec["name"] == "exec"));
    assert!(direct.iter().any(|spec| spec["name"] == "exec_command"));

    let code_mode = registry.schemas_for_api_with_mode(types::ToolMode::CodeMode);
    if code_mode.iter().any(|spec| spec["name"] == "exec") {
        assert!(code_mode.iter().any(|spec| spec["name"] == "exec_command"));

        let only = registry.schemas_for_api_with_mode(types::ToolMode::CodeModeOnly);
        assert_eq!(
            only.iter()
                .filter_map(|spec| spec["name"].as_str())
                .collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from(["exec", "wait"])
        );
    }
}

#[test]
fn code_mode_falls_back_but_code_mode_only_fails_closed() {
    let mut registry = tools::ToolRegistry::new();
    registry.register(types::ToolEntry {
        name: "ordinary".into(),
        toolset: "test".into(),
        ..types::ToolEntry::lifecycle_defaults()
    });
    registry.register(types::ToolEntry {
        name: "exec".into(),
        toolset: "code_mode".into(),
        check_fn: Some(Box::new(|| false)),
        exposure: types::ToolExposure::DirectModelOnly,
        ..types::ToolEntry::lifecycle_defaults()
    });

    let fallback = registry.schemas_for_api_with_mode(types::ToolMode::CodeMode);
    assert!(fallback.iter().any(|spec| spec["name"] == "ordinary"));
    assert!(!fallback.iter().any(|spec| spec["name"] == "exec"));

    let fail_closed = registry.schemas_for_api_with_mode(types::ToolMode::CodeModeOnly);
    assert!(fail_closed.is_empty());
}
