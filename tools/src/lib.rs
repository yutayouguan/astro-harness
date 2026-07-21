//! Astro Agent 内置工具集：注册、分发与解析。
//!
//! 本 crate 提供 Agent 可调用的全部内置工具实现，以及注册表、目录、
//! schema 清理、路径安全与 tool call 解析等基础设施。
//! 入口函数 [`register_all`] 将所有内置工具注册到 [`ToolRegistry`]。
//!
//! 模块分层：
//! - [`core`]：注册表、上下文、分发、解析、schema、目录与路径安全
//! - [`builtins`]：各内置工具实现（按领域分子目录：media / system / memory / hitl / present / agents）

pub mod approval;
pub mod builtins;
pub mod core;
pub mod interaction_mode;

// 保持原有顶层路径，避免破坏下游 crate 引用。
pub use approval::{
    classify_dangerous_command, is_hardline_blocked, matches_allowlist, resolve_command_action,
    ApprovalAction, ApprovalMode,
};
pub use builtins::context_tools::render_pinned_for_prompt;
pub(crate) use core::path_safe;
pub use core::{catalog, context, dispatch, parse, registry, schema};
pub use interaction_mode::{
    check_tool_call, filter_schemas, tool_visible_in_mode, InteractionMode,
};

pub use builtins::system::jobs::shutdown_all_jobs as shutdown_background_jobs;
pub use catalog::{
    builtin_catalog, catalog_for_ui, params_from_schema, ToolCatalogItem, ToolFunctionInfo,
    ToolParamInfo,
};
pub use context::{ImageGenCreds, ImageGenParts, ImageGenTargets, ToolContext};
pub use dispatch::{builtin_handler_names, dispatch_tool};
pub use parse::{
    extract_tool_calls, resolve_tool_calls, ParsedToolCall, ToolCallAccumulator, ToolCallDelta,
};
pub use path_safe::resolve_safe;
pub use registry::{BuiltinToolHandler, BuiltinToolRegistrar, ToolEntry, ToolRegistry};
pub use schema::{sanitize_tool_schema, schema_for_args, schema_has_vendor_hazards};

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
    (
        register: $register_fn:ident,
        names: [$($name:literal),+ $(,)?],
        async_ctx: $dispatch_fn:ident $(,)?
    ) => {
        fn __astro_builtin_tool_handler<'a, 'b>(
            ctx: &'a mut $crate::context::ToolContext<'b>,
            _name: &'a str,
            args: &'a ::serde_json::Value,
        ) -> ::std::pin::Pin<
            ::std::boxed::Box<
                dyn ::std::future::Future<Output = ::anyhow::Result<::std::string::String>>
                    + 'a,
            >,
        > {
            ::std::boxed::Box::pin(async move { $dispatch_fn(ctx, args).await })
        }
        ::inventory::submit! {
            $crate::registry::BuiltinToolRegistrar {
                register: $register_fn,
                names: &[$($name),+],
                handler: __astro_builtin_tool_handler,
            }
        }
    };
    (
        register: $register_fn:ident,
        names: [$($name:literal),+ $(,)?],
        sync_ctx: $dispatch_fn:ident $(,)?
    ) => {
        fn __astro_builtin_tool_handler<'a, 'b>(
            ctx: &'a mut $crate::context::ToolContext<'b>,
            _name: &'a str,
            args: &'a ::serde_json::Value,
        ) -> ::std::pin::Pin<
            ::std::boxed::Box<
                dyn ::std::future::Future<Output = ::anyhow::Result<::std::string::String>>
                    + 'a,
            >,
        > {
            ::std::boxed::Box::pin(async move { $dispatch_fn(ctx, args) })
        }
        ::inventory::submit! {
            $crate::registry::BuiltinToolRegistrar {
                register: $register_fn,
                names: &[$($name),+],
                handler: __astro_builtin_tool_handler,
            }
        }
    };
    (
        register: $register_fn:ident,
        names: [$($name:literal),+ $(,)?],
        sync_named: $dispatch_fn:ident $(,)?
    ) => {
        fn __astro_builtin_tool_handler<'a, 'b>(
            ctx: &'a mut $crate::context::ToolContext<'b>,
            name: &'a str,
            args: &'a ::serde_json::Value,
        ) -> ::std::pin::Pin<
            ::std::boxed::Box<
                dyn ::std::future::Future<Output = ::anyhow::Result<::std::string::String>>
                    + 'a,
            >,
        > {
            ::std::boxed::Box::pin(async move { $dispatch_fn(ctx, name, args) })
        }
        ::inventory::submit! {
            $crate::registry::BuiltinToolRegistrar {
                register: $register_fn,
                names: &[$($name),+],
                handler: __astro_builtin_tool_handler,
            }
        }
    };
    (
        register: $register_fn:ident,
        names: [$($name:literal),+ $(,)?],
        async_named: $dispatch_fn:ident $(,)?
    ) => {
        fn __astro_builtin_tool_handler<'a, 'b>(
            ctx: &'a mut $crate::context::ToolContext<'b>,
            name: &'a str,
            args: &'a ::serde_json::Value,
        ) -> ::std::pin::Pin<
            ::std::boxed::Box<
                dyn ::std::future::Future<Output = ::anyhow::Result<::std::string::String>>
                    + 'a,
            >,
        > {
            ::std::boxed::Box::pin(async move { $dispatch_fn(ctx, name, args).await })
        }
        ::inventory::submit! {
            $crate::registry::BuiltinToolRegistrar {
                register: $register_fn,
                names: &[$($name),+],
                handler: __astro_builtin_tool_handler,
            }
        }
    };
    (
        register: $register_fn:ident,
        names: [$($name:literal),+ $(,)?],
        custom: $handler_fn:ident $(,)?
    ) => {
        ::inventory::submit! {
            $crate::registry::BuiltinToolRegistrar {
                register: $register_fn,
                names: &[$($name),+],
                handler: $handler_fn,
            }
        }
    };
}

/// 向注册表一次性注册全部内置工具。
///
/// 通过 [`inventory`] 收集各工具模块的 [`BuiltinToolRegistrar`]；新工具在自身文件
/// `submit_builtin_tool! { register, names, … }` 即可，无需改本函数。MCP 工具由 agent 层单独注册。
///
/// 注意：工具模块须通过 `pub mod builtins` 编入 crate，否则 submit 不会进入最终二进制。
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
            "tts",
            "ask_user",
            "delegate",
            "present_ui",
            "file_ops",
            "web_search",
        ] {
            assert!(
                names.contains(&expected),
                "missing {expected}; got {names:?}"
            );
        }
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
