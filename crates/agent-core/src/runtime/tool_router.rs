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
    mcp_approval: Option<types::McpToolApproval>,
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
                        mcp_approval: entry.mcp_approval.clone(),
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
}

impl fmt::Debug for ToolRouter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolRouter")
            .field("route_count", &self.routes.len())
            .field("model_visible_specs", &self.model_visible_specs)
            .finish()
    }
}
