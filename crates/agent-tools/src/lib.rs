//! Astro Agent 内置工具集：注册、分发与解析。
//!
//! 本 crate 提供 Agent 可调用的全部内置工具实现，以及注册表、目录、
//! schema 清理、路径安全与 tool call 解析等基础设施。
//! 入口函数 [`register_all`] 将所有内置工具注册到 [`ToolRegistry`]。
//!
//! 模块分层：
//! - [`engine`]：注册表、上下文、分发、解析、schema、目录与路径安全
//! - [`builtin`]：各内置工具实现（按领域分子目录：media / shell / memory / hitl / present / agents）

pub mod approval;
pub mod builtin;
pub mod engine;
pub mod interaction_mode;
pub mod terminal_session;

pub use approval::{
    classify_dangerous_command, command_type_rule_candidate, is_hardline_blocked,
    matches_allowlist, matches_command_type_allowlist, resolve_command_action, ApprovalAction,
    ApprovalMode,
};
pub use builtin::context_tools::render_pinned_for_prompt;
pub(crate) use engine::path_safe;
pub use engine::{catalog, context, dispatch, registry, schema};
pub use interaction_mode::{
    check_tool_call, filter_schemas, tool_visible_in_mode, InteractionMode,
};

pub use builtin::hitl::request_user_input_async::parse_async_user_message;
pub use builtin::shell::browser;
pub use builtin::shell::jobs::{
    shutdown_all_jobs as shutdown_background_jobs,
    shutdown_jobs_for_session as shutdown_background_jobs_for_session,
};
pub use catalog::{
    builtin_catalog, catalog_for_ui, params_from_schema, ToolCatalogItem, ToolFunctionInfo,
    ToolParamInfo,
};
pub use context::{
    image_gen_targets_from_parts, ImageGenCreds, ImageGenParts, ImageGenTargets, ModelCredentials,
    ToolContext,
};
pub use dispatch::{builtin_handler_names, dispatch_tool, tool_requires_in_process_write};
pub use engine::code_mode::render_tool_description as render_code_mode_tool_description;
pub use engine::execution::{
    AgentThreadDispatch, FollowupAgentDispatchRequest, ParentRuntimeMaterial,
    SpawnAgentDispatchRequest,
};
pub use engine::executor::{LegacyToolAdapter, ToolExecutor, ToolExecutorFuture};
pub use path_safe::resolve_safe;
pub use registry::DynToolHandler;
pub use registry::{BuiltinToolHandler, BuiltinToolRegistrar, ToolEntry, ToolRegistry};
pub use sandbox::{SandboxAuditKind, SandboxAuditMetadata};
pub use schema::{sanitize_tool_schema, schema_for_args, schema_has_vendor_hazards};
pub use terminal_session::{
    shared_terminal_sessions, TerminalDimensions, TerminalReadResult, TerminalSessionInfo,
    TerminalSessionManager,
};
pub use types::{ParsedToolCall, ToolCallAccumulator, ToolCallDelta};

// 宏：`tool_schema!` / `register_tool_schemars!` / `define_tool_args!` / `submit_builtin_tool!`

/// 将本模块的元数据 `register` 与执行 handler 一并报名到 inventory。
///
/// 变体：
/// - `async_ctx`：`async fn(&ToolContext, &Value)`
/// - `sync_ctx`：`fn(&ToolContext|&mut ToolContext, &Value)`
/// - `sync_named`：`fn(&ToolContext|&mut ToolContext, &str, &Value)`
/// - `async_named`：`async fn(&mut ToolContext, &str, &Value)`
/// - `custom`：已符合 [`BuiltinToolHandler`] 签名的 fn
///
/// ```ignore
/// submit_builtin_tool! {
///     register: register,
///     names: ["web_search"],
///     async_ctx: dispatch,
/// }
/// ```
#[macro_export]
macro_rules! submit_builtin_tool {
    // ── raw args 变体（dispatch 接收 &Value）──────────────
    (register: $r:ident, names: [$($n:literal),+ $(,)?], async_ctx: $d:ident $(,)?) => {
        $crate::submit_builtin_tool!(@impl $r, [$($n),+], ctx, _name, args, {
            $d(ctx, args).await.map(::types::ToolOutput::from)
        });
    };
    (register: $r:ident, names: [$($n:literal),+ $(,)?], sync_ctx: $d:ident $(,)?) => {
        $crate::submit_builtin_tool!(@impl $r, [$($n),+], ctx, _name, args, {
            $d(ctx, args).map(::types::ToolOutput::from)
        });
    };
    (register: $r:ident, names: [$($n:literal),+ $(,)?], async_named: $d:ident $(,)?) => {
        $crate::submit_builtin_tool!(@impl $r, [$($n),+], ctx, name, args, {
            $d(ctx, name, args).await.map(::types::ToolOutput::from)
        });
    };
    (register: $r:ident, names: [$($n:literal),+ $(,)?], sync_named: $d:ident $(,)?) => {
        $crate::submit_builtin_tool!(@impl $r, [$($n),+], ctx, name, args, {
            $d(ctx, name, args).map(::types::ToolOutput::from)
        });
    };
    // ── typed args 变体（宏自动反序列化）──────────────────
    (register: $r:ident, names: [$($n:literal),+ $(,)?], async_ctx: $d:ident, args: $t:ty $(,)?) => {
        $crate::submit_builtin_tool!(@impl $r, [$($n),+], ctx, _name, args, {
            let typed: $t = ::serde_json::from_value(args.clone())
                .map_err(|e| ::anyhow::anyhow!(concat!(stringify!($t), " 参数无效: {}"), e))?;
            $d(ctx, &typed).await.map(::types::ToolOutput::from)
        });
    };
    (register: $r:ident, names: [$($n:literal),+ $(,)?], sync_ctx: $d:ident, args: $t:ty $(,)?) => {
        $crate::submit_builtin_tool!(@impl $r, [$($n),+], ctx, _name, args, {
            let typed: $t = ::serde_json::from_value(args.clone())
                .map_err(|e| ::anyhow::anyhow!(concat!(stringify!($t), " 参数无效: {}"), e))?;
            $d(ctx, &typed).map(::types::ToolOutput::from)
        });
    };
    (register: $r:ident, names: [$($n:literal),+ $(,)?], async_named: $d:ident, args: $t:ty $(,)?) => {
        $crate::submit_builtin_tool!(@impl $r, [$($n),+], ctx, name, args, {
            let typed: $t = ::serde_json::from_value(args.clone())
                .map_err(|e| ::anyhow::anyhow!(concat!(stringify!($t), " 参数无效: {}"), e))?;
            $d(ctx, name, &typed).await.map(::types::ToolOutput::from)
        });
    };
    (register: $r:ident, names: [$($n:literal),+ $(,)?], sync_named: $d:ident, args: $t:ty $(,)?) => {
        $crate::submit_builtin_tool!(@impl $r, [$($n),+], ctx, name, args, {
            let typed: $t = ::serde_json::from_value(args.clone())
                .map_err(|e| ::anyhow::anyhow!(concat!(stringify!($t), " 参数无效: {}"), e))?;
            $d(ctx, name, &typed).map(::types::ToolOutput::from)
        });
    };
    // ── custom：handler 已符合 BuiltinToolHandler 签名 ───
    (register: $r:ident, names: [$($n:literal),+ $(,)?], custom: $h:ident $(,)?) => {
        ::inventory::submit! {
            $crate::registry::BuiltinToolRegistrar {
                register: $r,
                names: &[$($n),+],
                handler: $h,
            }
        }
    };
    // ── 内部实现：统一生成 handler + inventory::submit ────
    (@impl $register_fn:ident, [$($name:literal),+], $ctx:ident, $nm:ident, $args:ident, $body:block) => {
        fn __astro_builtin_tool_handler<'a, 'b>(
            $ctx: &'a mut $crate::context::ToolContext<'b>,
            $nm: &'a str,
            $args: &'a ::serde_json::Value,
        ) -> ::std::pin::Pin<
            ::std::boxed::Box<
                dyn ::std::future::Future<Output = ::anyhow::Result<::types::ToolOutput>>
                    + 'a,
            >,
        > {
            ::std::boxed::Box::pin(async move $body)
        }
        ::inventory::submit! {
            $crate::registry::BuiltinToolRegistrar {
                register: $register_fn,
                names: &[$($name),+],
                handler: __astro_builtin_tool_handler,
            }
        }
    };
}

/// 向注册表一次性注册全部内置工具。
///
/// 通过 [`inventory`] 收集各工具模块的 [`BuiltinToolRegistrar`]；新工具在自身文件
/// `submit_builtin_tool! { register, names, … }` 即可，无需改本函数。MCP 工具由 agent 层单独注册。
///
/// 注意：工具模块须通过 `pub mod builtin` 编入 crate，否则 submit 不会进入最终二进制。
/// 通常在应用启动或测试初始化时调用一次。
pub fn register_all(registry: &mut ToolRegistry) {
    for hook in inventory::iter::<BuiltinToolRegistrar> {
        (hook.register)(registry);
    }
}

#[cfg(test)]
mod inventory_register_tests {
    use super::*;

    #[test]
    fn inventory_registers_sample_builtin_tools() {
        let mut registry = ToolRegistry::new();
        register_all(&mut registry);
        let names: Vec<_> = registry
            .available_tools()
            .iter()
            .map(|t| t.name.as_str())
            .collect();
        for expected in [
            "memory",
            "music_gen",
            "image_gen",
            "video_gen",
            "speech_gen",
            "ask_user",
            "request_user_input_async",
            "spawn_agent",
            "list_agents",
            "send_message",
            "followup_task",
            "wait_agent",
            "interrupt_agent",
            "present",
            "web_search",
        ] {
            assert!(
                names.contains(&expected),
                "missing {expected}; got {names:?}"
            );
        }
        let api_names = registry
            .schemas_for_api()
            .into_iter()
            .filter_map(|schema| schema["name"].as_str().map(str::to_string))
            .collect::<Vec<_>>();
        assert!(api_names.contains(&"request_user_input_async".to_string()));
        assert!(!api_names.contains(&"send_user_message_async".to_string()));
        assert!(registry.get("send_user_message_async").is_none());
    }

    #[test]
    fn every_registered_metadata_tool_has_handler() {
        let mut registry = ToolRegistry::new();
        register_all(&mut registry);
        let handlers = builtin_handler_names();
        for entry in registry.all_tools() {
            assert!(
                handlers.binary_search(&entry.name.as_str()).is_ok(),
                "metadata tool `{}` has no dispatch handler",
                entry.name
            );
        }
    }

    #[test]
    fn legacy_memory_tool_names_are_not_registered() {
        let handlers = builtin_handler_names();
        for legacy in ["memory_add", "memory_replace", "memory_remove"] {
            assert!(
                handlers.binary_search(&legacy).is_err(),
                "legacy tool name still has handler: {legacy}"
            );
        }
    }
}
