//! 采样步骤级工具路由快照。

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

use tools::{ToolContext, ToolRegistry};

#[derive(Clone)]
struct ToolRoute {
    registered_name: String,
}

/// 不可变的注册表投影，与单次模型请求可见的工具规格配对。
///
/// `routes` 是该步骤的完整可调用集合，是模型直调集合的超集：除可信
/// `tool_search_output` 激活的 Deferred 工具外，还可包含仅供 QuickJS cell 使用的
/// 嵌套路由。`model_routes` 单独约束模型顶层调用，防止越过披露边界。
pub(crate) struct ToolRouter {
    /// 与本次 sampling request 共享生命周期的工具注册表快照。
    registry: ToolRegistry,
    routes: HashMap<types::ToolName, ToolRoute>,
    /// 模型可以直接发起的路由；Code Mode 嵌套路由不在其中。
    model_routes: HashSet<types::ToolName>,
    model_visible_specs: Arc<[serde_json::Value]>,
}

impl ToolRouter {
    /// 将 Provider 原生 ResponseItem 解析为保留 namespace 的执行调用。
    pub(crate) fn build_tool_call(
        item: &agent_protocol::ResponseItem,
    ) -> Option<types::ParsedToolCall> {
        match item {
            agent_protocol::ResponseItem::FunctionCall {
                id,
                name,
                namespace,
                arguments,
                encrypted_function_args,
                call_id,
                ..
            } => {
                let (arguments, args_parse_error) =
                    match serde_json::from_str::<serde_json::Value>(arguments) {
                        Ok(arguments) => (arguments, false),
                        Err(error) => (
                            serde_json::json!({
                                "_parse_error": format!("invalid function arguments JSON: {error}"),
                                "_raw": arguments,
                            }),
                            true,
                        ),
                    };
                Some(types::ParsedToolCall {
                    item_id: id.as_ref().map(ToString::to_string),
                    id: call_id.clone(),
                    name: name.clone(),
                    namespace: namespace.clone(),
                    arguments,
                    encrypted_arguments: encrypted_function_args.clone(),
                    args_parse_error,
                    signature: None,
                })
            }
            agent_protocol::ResponseItem::CustomToolCall {
                id,
                call_id,
                name,
                namespace,
                input,
                ..
            } => Some(types::ParsedToolCall {
                item_id: id.as_ref().map(ToString::to_string),
                id: call_id.clone(),
                name: name.clone(),
                namespace: namespace.clone(),
                arguments: serde_json::Value::String(input.clone()),
                encrypted_arguments: None,
                args_parse_error: false,
                signature: None,
            }),
            agent_protocol::ResponseItem::ToolSearchCall {
                id,
                call_id: Some(call_id),
                execution,
                arguments,
                ..
            } if execution == "client" => Some(types::ParsedToolCall {
                item_id: id.as_ref().map(ToString::to_string),
                id: call_id.clone(),
                name: "tool_search".into(),
                namespace: None,
                arguments: arguments.clone(),
                encrypted_arguments: None,
                args_parse_error: false,
                signature: None,
            }),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_registry(
        registry: &ToolRegistry,
        additional_callable_specs: &[serde_json::Value],
        model_visible_specs: Vec<serde_json::Value>,
    ) -> Self {
        Self::from_registry_with_nested(
            registry,
            additional_callable_specs,
            model_visible_specs,
            &[],
        )
    }

    #[cfg(test)]
    pub(crate) fn from_registry_with_nested(
        registry: &ToolRegistry,
        additional_callable_specs: &[serde_json::Value],
        model_visible_specs: Vec<serde_json::Value>,
        nested_callable_specs: &[serde_json::Value],
    ) -> Self {
        finalize_tool_router(
            registry.clone(),
            additional_callable_specs,
            model_visible_specs,
            nested_callable_specs,
        )
    }

    fn entry(&self, namespace: Option<&str>, name: &str) -> Option<&types::ToolEntry> {
        let registered_name = self.registered_name(namespace, name)?;
        self.registry.get(registered_name)
    }

    pub(crate) async fn dispatch(
        &self,
        ctx: &mut ToolContext<'_>,
        namespace: Option<&str>,
        name: &str,
        args: &serde_json::Value,
    ) -> anyhow::Result<types::ToolOutput> {
        let registered_name = self
            .registered_name(namespace, name)
            .ok_or_else(|| anyhow::anyhow!("tool is not registered in this StepContext"))?;
        self.registry.dispatch(ctx, registered_name, args).await
    }

    pub(crate) fn model_visible_specs(&self) -> Arc<[serde_json::Value]> {
        Arc::clone(&self.model_visible_specs)
    }

    pub(crate) fn has_tool(&self, namespace: Option<&str>, name: &str) -> bool {
        self.routes
            .contains_key(&types::ToolName::new(namespace, name))
    }

    pub(crate) fn model_can_call(&self, namespace: Option<&str>, name: &str) -> bool {
        self.model_routes
            .contains(&types::ToolName::new(namespace, name))
    }

    pub(crate) fn registered_name(&self, namespace: Option<&str>, name: &str) -> Option<&str> {
        self.routes
            .get(&types::ToolName::new(namespace, name))
            .map(|route| route.registered_name.as_str())
    }

    pub(crate) fn needs_confirmation(&self, namespace: Option<&str>, name: &str) -> bool {
        self.entry(namespace, name)
            .is_some_and(|entry| entry.needs_confirmation)
    }

    pub(crate) fn stop_after(&self, namespace: Option<&str>, name: &str) -> bool {
        self.entry(namespace, name)
            .is_some_and(|entry| entry.stop_after_tool_call)
    }

    pub(crate) fn exclusive_access(&self, namespace: Option<&str>, name: &str) -> bool {
        self.entry(namespace, name)
            .is_some_and(|entry| entry.exclusive_access)
    }

    pub(crate) fn mcp_approval(
        &self,
        namespace: Option<&str>,
        name: &str,
    ) -> Option<types::McpToolApproval> {
        self.entry(namespace, name)
            .and_then(|entry| entry.mcp_approval.clone())
    }

    #[allow(dead_code)]
    pub(crate) fn approval_requirement(
        &self,
        namespace: Option<&str>,
        name: &str,
    ) -> types::ExecApprovalRequirement {
        self.entry(namespace, name)
            .map_or(types::ExecApprovalRequirement::Skip, |entry| {
                entry.approval_requirement
            })
    }

    pub(crate) fn may_require_approval(&self, namespace: Option<&str>, name: &str) -> bool {
        self.entry(namespace, name)
            .is_some_and(|entry| entry.approval_requirement != types::ExecApprovalRequirement::Skip)
    }

    pub(crate) fn sandbox_preference(
        &self,
        namespace: Option<&str>,
        name: &str,
    ) -> types::SandboxablePreference {
        self.entry(namespace, name)
            .map_or(types::SandboxablePreference::Forbid, |entry| {
                entry.sandbox_preference
            })
    }
}

/// 根据当前交互模式和已发现 Deferred 工具构建 Step 工具计划。
pub(crate) fn build_tool_router(
    registry: &ToolRegistry,
    interaction_mode: types::InteractionMode,
    requested_tool_mode: types::ToolMode,
    discovered_deferred: &HashSet<types::ToolName>,
) -> anyhow::Result<ToolRouter> {
    let mut canonical_names = HashMap::<types::ToolName, String>::new();
    for entry in registry.all_tools() {
        let tool_name = entry.tool_name();
        if let Some(existing) = canonical_names.insert(tool_name.clone(), entry.name.clone()) {
            if existing != entry.name {
                anyhow::bail!(
                    "tool identity collision for `{tool_name}` between `{existing}` and `{}`",
                    entry.name
                );
            }
        }
    }
    let (model_visible_specs, discovered_specs, nested_specs) =
        registry.schemas_for_step_with_mode(requested_tool_mode, discovered_deferred)?;
    let model_visible_specs = tools::filter_schemas(interaction_mode, model_visible_specs);
    let discovered_specs = tools::filter_schemas(interaction_mode, discovered_specs);
    let nested_specs = tools::filter_schemas(interaction_mode, nested_specs);
    for spec in discovered_specs
        .iter()
        .chain(model_visible_specs.iter())
        .chain(nested_specs.iter())
    {
        for (_, registered_name) in spec_route_names(registry, spec) {
            if registry.runtime(&registered_name).is_none() {
                anyhow::bail!(
                    "tool `{registered_name}` is model-visible but has no CoreToolRuntime"
                );
            }
        }
    }
    Ok(finalize_tool_router(
        (*registry).clone(),
        &discovered_specs,
        model_visible_specs,
        &nested_specs,
    ))
}

/// 将可见 schema 和执行 Registry 冻结为一个不可变 ToolRouter。
pub(crate) fn finalize_tool_router(
    registry: ToolRegistry,
    additional_callable_specs: &[serde_json::Value],
    model_visible_specs: Vec<serde_json::Value>,
    nested_callable_specs: &[serde_json::Value],
) -> ToolRouter {
    let mut routes = HashMap::new();
    let mut model_routes = HashSet::new();
    for spec in additional_callable_specs
        .iter()
        .chain(model_visible_specs.iter())
    {
        for (tool_name, registered_name) in spec_route_names(&registry, spec) {
            model_routes.insert(tool_name.clone());
            if registry.get(&registered_name).is_none() {
                continue;
            }
            routes.insert(
                tool_name,
                ToolRoute {
                    registered_name: registered_name.clone(),
                },
            );
        }
    }
    for spec in nested_callable_specs {
        for (tool_name, registered_name) in spec_route_names(&registry, spec) {
            if registry.get(&registered_name).is_none() {
                continue;
            }
            let code_mode_name = types::ToolName::plain(super::code_mode::normalize_identifier(
                &tool_name.wire_name(),
            ));
            routes.entry(code_mode_name).or_insert_with(|| ToolRoute {
                registered_name: registered_name.clone(),
            });
        }
    }
    ToolRouter {
        registry,
        routes,
        model_routes,
        model_visible_specs: model_visible_specs.into(),
    }
}

fn spec_route_names(
    registry: &ToolRegistry,
    spec: &serde_json::Value,
) -> Vec<(types::ToolName, String)> {
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
            let tool_name = types::ToolName::namespaced(namespace, child_name);
            let Some(registered_name) = registry
                .all_tools()
                .into_iter()
                .find(|entry| entry.tool_name() == tool_name)
                .map(|entry| entry.name.clone())
            else {
                continue;
            };
            routes.push((tool_name, registered_name));
        }
        return routes;
    }

    let name = if kind == "tool_search" {
        Some("tool_search")
    } else {
        spec.get("name").and_then(serde_json::Value::as_str)
    };
    name.filter(|name| registry.get(name).is_some())
        .map(|name| vec![(types::ToolName::plain(name), name.to_string())])
        .unwrap_or_default()
}

impl fmt::Debug for ToolRouter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolRouter")
            .field("route_count", &self.routes.len())
            .field("model_route_count", &self.model_routes.len())
            .field("model_visible_specs", &self.model_visible_specs)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_tool_call_rejects_invalid_json_before_dispatch() {
        let call = ToolRouter::build_tool_call(&agent_protocol::ResponseItem::FunctionCall {
            id: Some("item-invalid".into()),
            name: "lookup".into(),
            namespace: Some("crm".into()),
            arguments: "{invalid".into(),
            encrypted_function_args: None,
            call_id: "call-invalid".into(),
            internal_chat_message_metadata_passthrough: None,
        })
        .expect("function call");

        assert!(call.args_parse_error);
        assert_eq!(call.namespace.as_deref(), Some("crm"));
        assert_eq!(call.arguments["_raw"], "{invalid");
    }

    #[test]
    fn build_tool_call_ignores_server_executed_tool_search() {
        let item = agent_protocol::ResponseItem::ToolSearchCall {
            id: Some("search-1".into()),
            call_id: Some("call-search-1".into()),
            status: Some("completed".into()),
            execution: "server".into(),
            arguments: serde_json::json!({"query": "calendar"}),
            internal_chat_message_metadata_passthrough: None,
        };

        assert!(ToolRouter::build_tool_call(&item).is_none());
    }

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
            "name": "sandboxed",
            "parameters": {}
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
            router.sandbox_preference(None, "sandboxed"),
            types::SandboxablePreference::Auto
        );
        assert_eq!(
            router.sandbox_preference(None, "missing"),
            types::SandboxablePreference::Forbid
        );
    }

    #[test]
    fn build_tool_router_rejects_visible_metadata_without_runtime() {
        let mut registry = ToolRegistry::new();
        registry.register(types::ToolEntry {
            name: "metadata_only".into(),
            toolset: "core".into(),
            description: "missing runtime".into(),
            ..types::ToolEntry::lifecycle_defaults()
        });

        let error = build_tool_router(
            &registry,
            types::InteractionMode::Agent,
            types::ToolMode::Direct,
            &HashSet::new(),
        )
        .unwrap_err();

        assert!(error.to_string().contains("has no CoreToolRuntime"));
    }

    #[test]
    fn build_tool_router_rejects_duplicate_canonical_identity() {
        let mut registry = ToolRegistry::new();
        for registered_name in ["cron_list", "cron__list"] {
            registry.register(types::ToolEntry {
                name: registered_name.into(),
                namespace: "cron".into(),
                toolset: "cron".into(),
                description: "duplicate native identity".into(),
                ..types::ToolEntry::lifecycle_defaults()
            });
        }

        let error = build_tool_router(
            &registry,
            types::InteractionMode::Agent,
            types::ToolMode::Direct,
            &HashSet::new(),
        )
        .unwrap_err();

        assert!(error.to_string().contains("tool identity collision"));
    }

    #[test]
    fn only_discovered_deferred_tools_are_callable_without_being_advertised() {
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

        registry.register(types::ToolEntry {
            name: "unseen_deferred_tool".into(),
            toolset: "core".into(),
            description: "not returned by tool_search".into(),
            ..types::ToolEntry::lifecycle_defaults().deferred()
        });

        let discovered = std::collections::HashSet::from([types::ToolName::plain("deferred_tool")]);
        let (visible, routable_deferred) = registry.schemas_for_step(&discovered);
        let router = ToolRouter::from_registry(&registry, &routable_deferred, visible);

        assert!(router.has_tool(None, "direct_tool"));
        assert!(router.has_tool(Some("functions"), "direct_tool"));
        assert!(router.has_tool(None, "deferred_tool"));
        assert!(!router.has_tool(None, "unseen_deferred_tool"));
        assert!(!router.has_tool(None, "hidden_tool"));
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
    fn code_mode_nested_routes_do_not_expand_model_permissions() {
        let mut registry = ToolRegistry::new();
        for name in ["exec", "business_tool"] {
            registry.register(types::ToolEntry {
                name: name.into(),
                toolset: "core".into(),
                description: name.into(),
                ..types::ToolEntry::lifecycle_defaults()
            });
        }
        let visible = vec![serde_json::json!({
            "type": "function",
            "name": "exec",
            "parameters": {}
        })];
        let nested = vec![serde_json::json!({
            "type": "function",
            "name": "business_tool",
            "parameters": {}
        })];
        let router = ToolRouter::from_registry_with_nested(&registry, &[], visible, &nested);

        assert!(router.model_can_call(None, "exec"));
        assert!(!router.model_can_call(None, "business_tool"));
        assert!(router.has_tool(None, "business_tool"));
    }

    #[test]
    fn native_namespace_identity_resolves_without_flattened_aliases() {
        let mut registry = ToolRegistry::new();
        registry.register(types::ToolEntry {
            name: "cron_list".into(),
            toolset: "cron".into(),
            namespace: "cron".into(),
            description: "list jobs".into(),
            ..types::ToolEntry::lifecycle_defaults()
        });
        let (visible, routable_deferred) =
            registry.schemas_for_step(&std::collections::HashSet::new());
        let router = ToolRouter::from_registry(&registry, &routable_deferred, visible);

        assert!(router.has_tool(Some("cron"), "list"));
        assert_eq!(
            router.registered_name(Some("cron"), "list"),
            Some("cron_list")
        );
        assert!(!router.has_tool(None, "cron.list"));
        assert!(!router.has_tool(None, "cron__list"));
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
            serde_json::json!({"type": "function", "name": "needs_approval", "parameters": {}}),
            serde_json::json!({"type": "function", "name": "auto_skip", "parameters": {}}),
        ];
        let router = ToolRouter::from_registry(&registry, &specs, specs.clone());

        assert_eq!(
            router.approval_requirement(None, "needs_approval"),
            types::ExecApprovalRequirement::NeedsApproval
        );
        assert_eq!(
            router.approval_requirement(None, "auto_skip"),
            types::ExecApprovalRequirement::Skip
        );
        assert!(router.may_require_approval(None, "needs_approval"));
        assert!(!router.may_require_approval(None, "auto_skip"));
        assert!(!router.may_require_approval(None, "unknown"));
    }
}
