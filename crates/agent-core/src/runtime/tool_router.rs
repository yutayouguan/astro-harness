//! Sampling-step tool routing snapshot.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use tools::{DynToolHandler, ToolRegistry};

#[derive(Clone)]
struct ToolRoute {
    dynamic_handler: Option<DynToolHandler>,
    needs_confirmation: bool,
    stop_after_tool_call: bool,
    exclusive_access: bool,
    sandbox_preference: types::SandboxablePreference,
    mcp_approval: Option<types::McpToolApproval>,
    approval_requirement: types::ExecApprovalRequirement,
}

/// Immutable registry projection paired with the specs visible to one model request.
pub(crate) struct ToolRouter {
    routes: HashMap<String, ToolRoute>,
    model_visible_specs: Arc<[serde_json::Value]>,
}

impl ToolRouter {
    pub(crate) fn from_registry(
        registry: &ToolRegistry,
        model_visible_specs: Vec<serde_json::Value>,
    ) -> Self {
        let routes = model_visible_specs
            .iter()
            .filter_map(|spec| {
                let name = spec.pointer("/function/name")?.as_str()?;
                let entry = registry.get(name)?;
                Some((
                    name.to_string(),
                    ToolRoute {
                        dynamic_handler: registry.dynamic_handler(name),
                        needs_confirmation: entry.needs_confirmation,
                        stop_after_tool_call: entry.stop_after_tool_call,
                        exclusive_access: entry.exclusive_access,
                        sandbox_preference: entry.sandbox_preference,
                        mcp_approval: entry.mcp_approval.clone(),
                        approval_requirement: entry.approval_requirement,
                    },
                ))
            })
            .collect();
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
        let router = ToolRouter::from_registry(
            &registry,
            vec![serde_json::json!({
                "type": "function",
                "function": {"name": "sandboxed", "parameters": {}}
            })],
        );
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
        let router = ToolRouter::from_registry(&registry, specs);

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
