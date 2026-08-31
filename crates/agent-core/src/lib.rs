//! Agent 运行时核心 crate：组装对话循环、上下文、钩子与流式输出。
//!
//! 对外暴露构建器、循环控制、消息转换与工具注册等能力，供 Tauri 前端与后端服务复用。
//! 工具注册表实现位于 `tools` crate，本 crate re-export [`ToolRegistry`]。
//! 领域细分能力（上下文、hook、cron/Agent Thread 执行等）请通过 `prompt::` /
//! `control::` / `exec::` 子模块路径访问，根导出仅保留最常用的顶层类型。

/// 声明式 Agent 构建与规格导出。
pub mod builder;
/// 工具结果压缩：保留原始 tool content，给 provider 发送压缩视图。
pub mod compression;
/// 控制型运行时能力（HITL / interrupt / schema 校验）。
pub mod control;
/// 执行域聚合模块（cron / subagents / memory_review）。
pub mod exec;
/// Git worktree 隔离与项目根解析。
pub mod git_worktree;
/// 提示词域：上下文、消息转换、hook 与 prompt builder。
pub mod prompt;
/// Agent 运行时核心。
pub mod runtime;
/// 流式补全与多轮流式迭代抽象。
pub mod streaming;
/// Session 任务生命周期。
pub(crate) mod tasks;
/// 助手回合时间线（astro_timeline_v1）。
pub mod timeline;

pub use agent_protocol::{Event, EventMsg, Op, TurnAbortReason, TurnInput};
/// 链式构建可运行的 Agent 实例及其规格。
pub use builder::{AgentBuilder, BuiltAgentSpec};
/// HITL 闸门 re-export。
#[allow(deprecated)]
pub use control::hitl::{
    is_exclusive_tool, is_interactive_tool, HitlGate, HitlRegistry, HitlRequest, HitlResolution,
    HITL_DEFAULT_TIMEOUT_SECS,
};
/// Interrupt 状态机 re-export。
pub use control::interrupt::{Interrupt, InterruptError, InterruptPending, ResumeItem};
/// 对话循环核心类型 re-export。
pub use runtime::{
    AgentConfig, AgentLoop, AgentStatus, AstroThread, Config, MaxDepthError, RuntimeIoBindError,
    Session, ToolCallError, TurnResult,
};
/// 流式 API re-export。
pub use streaming::{
    run_multi_turn_events_with_chat_fn, run_multi_turn_stream_with_chat_fn, run_thread_turn_events,
    ChatOverride, ProviderStreamer, StreamedAssistantContent, StreamingResponses,
    ThreadTurnEventArgs,
};
/// 工具注册表（实现位于 `tools` crate）。
pub use tools::ToolRegistry;
/// 工具元数据（定义位于 `common`）。
pub use types::ToolEntry;
/// Agno 风格模型声明（实现位于 `common`）。
pub use types::{ModelRole, ModelSpec};
