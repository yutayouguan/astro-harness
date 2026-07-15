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
    browser, clarify, code_exec, confirm, create_agent, delegate, file_ops, image_gen, memory_tools,
    multi_agent, music, orchestration, present_callout, present_metrics, present_result, present_ui,
    request_user_location, robotics, scheduled, skills_tool, task_plan, terminal, tts, video_gen, vision,
    web_search,
};

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
pub use registry::{ToolEntry, ToolRegistry};
pub use schema::{
    sanitize_tool_schema, schema_for_args, schema_has_vendor_hazards,
};

// 宏：`tool_schema!` / `register_tool_schemars!` / `define_tool_args!`（见 schema.rs）

/// 向注册表一次性注册全部内置工具。
///
/// 按领域顺序调用各模块的 `register`；MCP 工具由 agent 层单独注册。
/// 通常在应用启动或测试初始化时调用一次。
pub fn register_all(registry: &mut ToolRegistry) {
    // memory
    memory_tools::register(registry);
    scheduled::register(registry);
    skills_tool::register(registry);
    task_plan::register(registry);
    // media
    image_gen::register(registry);
    video_gen::register(registry);
    vision::register(registry);
    robotics::register(registry);
    tts::register(registry);
    music::register(registry);
    // system
    file_ops::register(registry);
    terminal::register(registry);
    code_exec::register(registry);
    browser::register(registry);
    web_search::register(registry);
    // hitl
    clarify::register(registry);
    confirm::register(registry);
    request_user_location::register(registry);
    // present
    present_ui::register(registry);
    present_metrics::register(registry);
    present_callout::register(registry);
    present_result::register(registry);
    // agents
    delegate::register(registry);
    multi_agent::register(registry);
    orchestration::register(registry);
    create_agent::register(registry);
}
