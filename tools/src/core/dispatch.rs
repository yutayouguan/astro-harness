//! 工具统一分发：按名称路由到各内置实现，并处理启用检查与调用记账。
//!
//! 所有 Agent 侧的工具执行均经 [`dispatch_tool`] 入口，确保禁用工具、
//! 调用统计与错误格式保持一致。

use crate::context::ToolContext;

/// 按工具名将调用路由到对应内置实现。
///
/// # 流程
/// 1. 通过 `registry_allows` 闭包检查 toolset 是否启用，禁用时立即返回错误。
/// 2. 调用 `memory::record_tool_call` 写审计日志，并 `record_usage_tool_call` 累加用量。
/// 3. 按 `name` 匹配具体模块的 `dispatch` 函数。
///
/// # 参数
/// - `registry_allows`：通常传入 `registry.is_tool_allowed`，用于读取 `tools-enabled.json` 状态。
/// - `ctx`：可变执行上下文，部分工具（如 `memory`、`create_agent`）会修改其中的 `memory` 或 `workspace_dir`。
///
/// # 约束
/// - 未知工具名返回 `未知工具` 错误；MCP 工具不由本函数处理。
/// - 部分工具为同步实现，部分为 `async`；调用方需 `await` 本函数。
pub async fn dispatch_tool(
    registry_allows: impl Fn(&str) -> bool,
    ctx: &mut ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    if !registry_allows(name) {
        let toolset = memory::tool_name_to_toolset(name);
        anyhow::bail!("工具已禁用（tools-enabled.json → {toolset}=false）: {name}");
    }

    // 先记账再执行：即使失败也计入一次「发起调用」
    let agent_id = ctx.memory.agent_id.clone();
    let _ = memory::record_tool_call(&agent_id, name, args);
    let _ = memory::record_usage_tool_call(
        &agent_id,
        name,
        args,
        Some(ctx.session_id.as_str()),
        ctx.turn_id.as_deref(),
    );

    match name {
        "memory" | "session_search" | "memory_add" | "memory_replace" | "memory_remove" => {
            crate::memory_tools::dispatch(ctx, name, args)
        }
        "cron_add" | "cron_list" | "cron_remove" | "cron_enable" | "cron_disable" | "scheduled" => {
            crate::scheduled::dispatch(name, args)
        }
        "image_gen" => crate::image_gen::dispatch(ctx, args).await,
        "file_ops" => crate::file_ops::dispatch(ctx, args),
        "terminal" => crate::terminal::dispatch(ctx, args).await,
        "web_search" => crate::web_search::dispatch(ctx, args).await,
        "code_exec" => crate::code_exec::dispatch(ctx, args).await,
        "vision" => crate::vision::dispatch(ctx, args).await,
        "tts" => crate::tts::dispatch(ctx, args).await,
        "music" => crate::music::dispatch(ctx, args).await,
        "skills" => crate::skills_tool::dispatch(ctx, args),
        "clarify" => crate::clarify::dispatch(ctx, args),
        "confirm" => crate::confirm::dispatch(ctx, args),
        "request_user_location" => crate::request_user_location::dispatch(ctx, args),
        "present_ui" => crate::present_ui::dispatch(ctx, args),
        "present_metrics" => crate::present_metrics::dispatch(ctx, args),
        "present_callout" => crate::present_callout::dispatch(ctx, args),
        "present_result" => crate::present_result::dispatch(ctx, args),
        "delegate" => crate::delegate::dispatch(ctx, args),
        "delegate_async" => crate::delegate::dispatch_async(ctx, args),
        "delegate_status" => crate::delegate::dispatch_status(args),
        "delegate_collect" => crate::delegate::dispatch_collect(args).await,
        "delegate_cancel" => crate::delegate::dispatch_cancel(args),
        "multi_agent" => crate::multi_agent::dispatch(ctx, args),
        "orchestration_run" => crate::orchestration::dispatch_run(ctx, args),
        "orchestration_status" => crate::orchestration::dispatch_status(args),
        "create_agent" => crate::create_agent::dispatch(ctx, args),
        "task_plan" => crate::task_plan::dispatch(ctx, args),
        "browser" => crate::browser::dispatch(ctx, args).await,
        other => anyhow::bail!("未知工具: {other}"),
    }
}
