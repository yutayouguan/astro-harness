//! 采样步骤级工具路由快照。

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use providers::types::message::legacy_namespace_function_name;
use tools::{DynToolHandler, ToolRegistry};

#[derive(Clone)]
struct ToolRoute {
    registered_name: String,
    dynamic_handler: Option<DynToolHandler>,
    needs_confirmation: bool,
    stop_after_tool_call: bool,
    exclusive_access: bool,
    sandbox_preference: types::SandboxablePreference,
    mcp_approval: Option<types::McpToolApproval>,
    approval_requirement: types::ExecApprovalRequirement,
}

/// 不可变的注册表投影，与单次模型请求可见的工具规格配对。
///
/// `routes` 是该步骤的可调用集合，是 `model_visible_specs` 的超集：
/// deferred 工具不进入初始可见列表，但仍保留完整路由供搜索后的调用执行。
pub(crate) struct ToolRouter {
    routes: HashMap<String, ToolRoute>,
    model_visible_specs: Arc<[serde_json::Value]>,
}

impl ToolRouter {
    pub(crate) fn from_registry(
        registry: &ToolRegistry,
        callable_specs: &[serde_json::Value],
        model_visible_specs: Vec<serde_json::Value>,
    ) -> Self {
        let mut routes = HashMap::new();
        for spec in callable_specs.iter().chain(model_visible_specs.iter()) {
            for (wire_name, registered_name) in spec_route_names(registry, spec) {
                let Some(entry) = registry.get(&registered_name) else {
                    continue;
                };
                routes.insert(
                    wire_name,
                    ToolRoute {
                        registered_name: registered_name.clone(),
                        dynamic_handler: registry.dynamic_handler(&registered_name),
                        needs_confirmation: entry.needs_confirmation,
                        stop_after_tool_call: entry.stop_after_tool_call,
                        exclusive_access: entry.exclusive_access,
                        sandbox_preference: entry.sandbox_preference,
                        mcp_approval: entry.mcp_approval.clone(),
                        approval_requirement: entry.approval_requirement,
                    },
                );
            }
        }
        Self {
            routes,
            model_visible_specs: model_visible_specs.into(),
        }
    }

    pub(crate) fn model_visible_specs(&self) -> Arc<[serde_json::Value]> {
        Arc::clone(&self.model_visible_specs)
    }

    pub(crate) fn has_tool(&self, name: &str) -> bool {
        self.routes.contains_key(name)
    }

    pub(crate) fn registered_name<'a>(&'a self, wire_name: &'a str) -> &'a str {
        self.routes
            .get(wire_name)
            .map_or(wire_name, |route| route.registered_name.as_str())
    }

    pub(crate) fn dynamic_handler(&self, name: &str) -> Option<DynToolHandler> {
        self.routes
            .get(name)
            .and_then(|route| route.dynamic_handler.clone())
    }

    pub(crate) fn any_needs_confirmation(&self, names: &[&str]) -> bool {
        names.iter().any(|name| {
            self.routes
                .get(*name)
                .is_some_and(|route| route.needs_confirmation)
        })
    }

    pub(crate) fn any_stop_after(&self, names: &[&str]) -> bool {
        names.iter().any(|name| {
            self.routes
                .get(*name)
                .is_some_and(|route| route.stop_after_tool_call)
        })
    }

    pub(crate) fn any_exclusive_access(&self, names: &[&str]) -> bool {
        names.iter().any(|name| {
            self.routes
                .get(*name)
                .is_some_and(|route| route.exclusive_access)
        })
    }

    pub(crate) fn mcp_approval(&self, name: &str) -> Option<types::McpToolApproval> {
        self.routes
            .get(name)
            .and_then(|route| route.mcp_approval.clone())
    }

    #[allow(dead_code)]
    pub(crate) fn approval_requirement(&self, name: &str) -> types::ExecApprovalRequirement {
        self.routes
            .get(name)
            .map_or(types::ExecApprovalRequirement::Skip, |route| {
                route.approval_requirement
            })
    }

    pub(crate) fn any_may_require_approval(&self, names: &[&str]) -> bool {
        names.iter().any(|name| {
            self.routes.get(*name).is_some_and(|route| {
                route.approval_requirement != types::ExecApprovalRequirement::Skip
            })
        })
    }

    pub(crate) fn sandbox_preference(&self, name: &str) -> types::SandboxablePreference {
        self.routes
            .get(name)
            .map_or(types::SandboxablePreference::Forbid, |route| {
                route.sandbox_preference
            })
    }
}

fn spec_route_names(registry: &ToolRegistry, spec: &serde_json::Value) -> Vec<(String, String)> {
    let kind = spec
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("function");
    if kind == "namespace" {
        let Some(namespace) = spec.get("name").and_then(serde_json::Value::as_str) else {
            return Vec::new();
        };
        let mut routes = Vec::new();
        for child in spec
            .get("tools")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(child_name) = child.get("name").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let native_wire_name = format!("{namespace}.{child_name}");
            let legacy_wire_name = legacy_namespace_function_name(namespace, child_name);
            let Some(registered_name) = [
                format!("{namespace}_{child_name}"),
                legacy_wire_name.clone(),
                native_wire_name.clone(),
                child_name.to_string(),
            ]
            .into_iter()
            .find(|candidate| registry.get(candidate).is_some()) else {
                continue;
            };
            routes.push((native_wire_name, registered_name.clone()));
            routes.push((legacy_wire_name, registered_name));
        }
        return routes;
    }

    let name = if kind == "tool_search" {
        Some("tool_search")
    } else {
        spec.get("name")
            .or_else(|| spec.pointer("/function/name"))
            .and_then(serde_json::Value::as_str)
    };
    name.filter(|name| registry.get(name).is_some())
        .map(|name| vec![(name.to_string(), name.to_string())])
        .unwrap_or_default()
}

impl fmt::Debug for ToolRouter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolRouter")
            .field("route_count", &self.routes.len())
            .field("model_visible_specs", &self.model_visible_specs)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_sandbox_preference_from_registry() {
        let mut registry = ToolRegistry::new();
        registry.register(types::ToolEntry {
            name: "sandboxed".into(),
            toolset: "core".into(),
            description: "sandboxed process".into(),
            schema: serde_json::json!({"type": "object", "properties": {}}),
            check_fn: None,
            icon: "terminal",
            ..types::ToolEntry::lifecycle_defaults().sandboxable()
        });
        let specs = vec![serde_json::json!({
            "type": "function",
            "function": {"name": "sandboxed", "parameters": {}}
        })];
        let router = ToolRouter::from_registry(&registry, &specs, specs.clone());
        registry.register(types::ToolEntry {
            name: "sandboxed".into(),
            toolset: "core".into(),
            description: "replacement".into(),
            schema: serde_json::json!({"type": "object", "properties": {}}),
            check_fn: None,
            icon: "terminal",
            ..types::ToolEntry::lifecycle_defaults()
        });

        assert_eq!(
            router.sandbox_preference("sandboxed"),
            types::SandboxablePreference::Auto
        );
        assert_eq!(
            router.sandbox_preference("missing"),
            types::SandboxablePreference::Forbid
        );
    }

    #[test]
    fn deferred_tools_are_callable_without_being_advertised() {
        let mut registry = ToolRegistry::new();
        registry.register(types::ToolEntry {
            name: "direct_tool".into(),
            toolset: "core".into(),
            description: "always advertised".into(),
            ..types::ToolEntry::lifecycle_defaults()
        });
        registry.register(types::ToolEntry {
            name: "deferred_tool".into(),
            toolset: "core".into(),
            description: "found through tool_search".into(),
            ..types::ToolEntry::lifecycle_defaults().deferred()
        });
        registry.register(types::ToolEntry {
            name: "hidden_tool".into(),
            toolset: "core".into(),
            description: "internal only".into(),
            ..types::ToolEntry::lifecycle_defaults().hidden()
        });

        let visible = registry.schemas_for_api();
        let router =
            ToolRouter::from_registry(&registry, &registry.all_callable_tool_schemas(), visible);

        assert!(router.has_tool("direct_tool"));
        assert!(router.has_tool("deferred_tool"));
        assert!(!router.has_tool("hidden_tool"));
        let specs = router.model_visible_specs();
        let advertised: Vec<&str> = specs
            .iter()
            .filter_map(|spec| {
                spec.get("name")
                    .or_else(|| spec.pointer("/function/name"))?
                    .as_str()
            })
            .collect();
        assert_eq!(advertised, vec!["direct_tool"]);
    }

    #[test]
    fn namespace_wire_name_resolves_to_registered_handler_name() {
        let mut registry = ToolRegistry::new();
        registry.register(types::ToolEntry {
            name: "cron_list".into(),
            toolset: "cron".into(),
            namespace: "cron".into(),
            description: "list jobs".into(),
            ..types::ToolEntry::lifecycle_defaults()
        });
        let visible = registry.schemas_for_api();
        let router =
            ToolRouter::from_registry(&registry, &registry.all_callable_tool_schemas(), visible);

        assert!(router.has_tool("cron.list"));
        assert_eq!(router.registered_name("cron.list"), "cron_list");
        assert!(router.has_tool("cron__list"));
        assert_eq!(router.registered_name("cron__list"), "cron_list");
    }

    #[test]
    fn snapshots_approval_requirement_from_registry() {
        let mut registry = ToolRegistry::new();
        registry.register(types::ToolEntry {
            name: "needs_approval".into(),
            toolset: "core".into(),
            description: "requires approval".into(),
            approval_requirement: types::ExecApprovalRequirement::NeedsApproval,
            ..types::ToolEntry::lifecycle_defaults()
        });
        registry.register(types::ToolEntry {
            name: "auto_skip".into(),
            toolset: "core".into(),
            description: "auto skip".into(),
            ..types::ToolEntry::lifecycle_defaults()
        });
        let specs = vec![
            serde_json::json!({"type": "function", "function": {"name": "needs_approval", "parameters": {}}}),
            serde_json::json!({"type": "function", "function": {"name": "auto_skip", "parameters": {}}}),
        ];
        let router = ToolRouter::from_registry(&registry, &specs, specs.clone());

        assert_eq!(
            router.approval_requirement("needs_approval"),
            types::ExecApprovalRequirement::NeedsApproval
        );
        assert_eq!(
            router.approval_requirement("auto_skip"),
            types::ExecApprovalRequirement::Skip
        );
        assert!(router.any_may_require_approval(&["needs_approval", "auto_skip"]));
        assert!(!router.any_may_require_approval(&["auto_skip"]));
        assert!(!router.any_may_require_approval(&["unknown"]));
    }
}
