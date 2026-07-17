//! Astro Agent 内置工具集：注册、分发与解析。
//!
//! 本 crate 提供 Agent 可调用的全部内置工具实现，以及注册表、目录、
//! schema 清理、路径安全与 tool call 解析等基础设施。
//! 入口函数 [`register_all`] 将所有内置工具注册到 [`ToolRegistry`]。
//!
//! 模块分层：
//! - [`core`]：注册表、上下文、分发、解析、schema、目录与路径安全
//! - [`builtins`]：各内置工具实现（按领域分子目录：media / system / memory / hitl / present / agents）

pub mod core;
pub mod builtins;
pub mod approval;

// 保持原有顶层路径，避免破坏下游 crate 引用。
pub use core::{catalog, context, dispatch, parse, registry, schema};
pub use approval::{classify_dangerous_command, ApprovalAction};
pub(crate) use core::path_safe;
pub(crate) use builtins::{
    audio_understand, browser, clarify, code_exec, confirm, context_tools, create_agent, delegate,
    file_ops, image_gen, memory_tools, multi_agent, music_gen, orchestration, present_callout,
    present_metrics, present_result, present_ui, request_user_location, robotics, scheduled,
    skills_tool, task_plan, team, terminal, tts, video_gen, video_understand, vision, web_extract,
    web_search,
};
pub use context_tools::render_pinned_for_prompt;

pub use catalog::{
    builtin_catalog, catalog_for_ui, params_from_schema, ToolCatalogItem, ToolFunctionInfo,
    ToolParamInfo,
};
pub use context::{ImageGenCreds, ImageGenTargets, ToolContext};
pub use path_safe::resolve_safe;
pub use dispatch::dispatch_tool;
pub use parse::{
    extract_tool_calls, resolve_tool_calls, ParsedToolCall, ToolCallAccumulator, ToolCallDelta,
};
pub use registry::{BuiltinToolRegistrar, ToolEntry, ToolRegistry};
pub use schema::{
    sanitize_tool_schema, schema_for_args, schema_has_vendor_hazards,
};

// 宏：`tool_schema!` / `register_tool_schemars!` / `define_tool_args!` / `submit_builtin_tool!`

/// 将本模块的 `register` 函数报名到 inventory，供 [`register_all`] 自动收集。
///
/// ```ignore
/// pub fn register(registry: &mut ToolRegistry) { /* ... */ }
/// submit_builtin_tool!(register);
/// ```
#[macro_export]
macro_rules! submit_builtin_tool {
    ($register_fn:ident) => {
        ::inventory::submit! {
            $crate::registry::BuiltinToolRegistrar {
                register: $register_fn,
            }
        }
    };
}

/// 向注册表一次性注册全部内置工具。
///
/// 通过 [`inventory`] 收集各工具模块的 [`BuiltinToolRegistrar`]；新工具在自身文件
/// `submit_builtin_tool!(register)` 即可，无需改本函数。MCP 工具由 agent 层单独注册。
///
/// 注意：工具模块须通过 `builtins` 与本文件的 `pub(crate) use` 编入 crate，否则 submit
/// 不会进入最终二进制。
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
    fn inventory_registers_all_builtin_modules() {
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
            "clarify",
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
        assert!(
            inventory::iter::<BuiltinToolRegistrar>.into_iter().count() >= 30,
            "expected ~31 tool modules submitted"
        );
    }
}
