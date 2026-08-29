use tools::{register_all, ToolRegistry};

#[test]
fn registry_exposes_codex_code_mode_control_tools() {
    let mut registry = ToolRegistry::new();
    register_all(&mut registry);
    let specs = registry.schemas_for_api();

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
