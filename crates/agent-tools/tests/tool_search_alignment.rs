use std::collections::HashSet;
use std::sync::RwLock;

use tempfile::TempDir;
use tools::builtin::shell::tool_search::{dispatch, ToolSearchArgs};
use tools::{ToolContext, ToolEntry, ToolRegistry};

#[tokio::test]
async fn search_returns_full_schema_without_mutating_deferred_dynamic_tool() {
    let dir = TempDir::new().unwrap();
    let workspace = dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let memory = RwLock::new(memory::MemoryManager::new(dir.path().to_path_buf()).unwrap());
    let sessions = session::SessionStore::open_sessions_dir(&dir.path().join("data"))
        .await
        .unwrap();
    let targets = tools::ImageGenTargets::default();
    let credentials = tools::ModelCredentials::default();

    let mut registry = ToolRegistry::new();
    registry.register(ToolEntry {
        name: "mcp__calendar__list_events".into(),
        toolset: "mcp".into(),
        namespace: "mcp__calendar".into(),
        description: "List upcoming calendar events".into(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {"days": {"type": "integer"}},
            "required": ["days"]
        }),
        icon: "plug",
        ..ToolEntry::lifecycle_defaults().deferred()
    });
    let registry = RwLock::new(registry);

    let ctx = ToolContext {
        memory: &memory,
        sessions: &sessions,
        memory_dir: dir.path().to_path_buf(),
        workspace_dir: workspace,
        project_root: None,
        workspace_roots: Vec::new(),
        image_gen_targets: &targets,
        session_id: "test".into(),
        turn_id: None,
        credentials: &credentials,
        service_tier: None,
        model_targets: &[],
        execution: None,
        permission_profile: None,
        skill_config_overrides: &[],
        hook_bus: None,
        hook_runtime: None,
        workspace_write_grant: false,
        sandbox_policy: None,
        managed_network: None,
        context_window: None,
        context_tokens_used: None,
        tool_registry: Some(&registry),
    };

    let output = dispatch(
        &ctx,
        &ToolSearchArgs {
            query: "calendar events".into(),
            limit: 5,
        },
    )
    .await
    .unwrap();
    let specs: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(specs[0]["type"], "namespace");
    assert_eq!(specs[0]["name"], "mcp__calendar");
    assert_eq!(specs[0]["tools"][0]["name"], "list_events");
    assert_eq!(specs[0]["tools"][0]["defer_loading"], true);
    assert_eq!(
        specs[0]["tools"][0]["parameters"]["properties"]["days"]["type"],
        "integer"
    );

    let visible = registry.read().unwrap().schemas_for_api();
    assert!(!visible.iter().any(|spec| {
        spec.get("name")
            .or_else(|| spec.pointer("/function/name"))
            .and_then(|name| name.as_str())
            == Some("mcp__calendar__list_events")
    }));
    let discovered = HashSet::from([types::ToolName::namespaced("mcp__calendar", "list_events")]);
    let routable_deferred = registry.read().unwrap().schemas_for_step(&discovered).1;
    assert!(routable_deferred.iter().any(|spec| {
        spec["type"] == "namespace"
            && spec["name"] == "mcp__calendar"
            && spec["tools"][0]["name"] == "list_events"
    }));
}

#[test]
fn deferred_tool_remains_deferred_after_registry_reregistration() {
    let mut registry = ToolRegistry::new();
    let deferred = || ToolEntry {
        name: "mcp__calendar__list_events".into(),
        toolset: "mcp".into(),
        namespace: "mcp__calendar".into(),
        description: "List upcoming calendar events".into(),
        icon: "plug",
        ..ToolEntry::lifecycle_defaults().deferred()
    };

    registry.register(deferred());
    registry.unregister_toolset("mcp");
    registry.register(deferred());

    assert_eq!(
        registry.get("mcp__calendar__list_events").unwrap().exposure,
        types::ToolExposure::Deferred
    );
}

#[test]
fn registry_emits_native_responses_tool_shapes() {
    let mut registry = ToolRegistry::new();
    tools::register_all(&mut registry);
    let specs = registry.schemas_for_api();

    assert!(specs.iter().any(|spec| spec["type"] == "tool_search"));
    let apply_patch = specs
        .iter()
        .find(|spec| spec["name"] == "apply_patch")
        .expect("apply_patch custom tool");
    assert_eq!(apply_patch["type"], "custom");
    assert_eq!(apply_patch["format"]["syntax"], "lark");

    let cron = specs
        .iter()
        .find(|spec| spec["type"] == "namespace" && spec["name"] == "cron")
        .expect("cron namespace");
    let names: Vec<_> = cron["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert!(names.contains(&"add"));
    assert!(names.contains(&"list"));
}
