//! Agent 运行时核心 crate：组装对话循环、上下文、钩子与流式输出。
//!
//! 对外暴露构建器、循环控制、消息转换与工具注册等能力，供 Tauri 前端与后端服务复用。
//! 工具注册表实现位于 `tools` crate，本 crate re-export [`ToolRegistry`]。

/// 声明式 Agent 构建与规格导出。
pub mod builder;
/// 聊天主模型故障切换（首包前 fallback）。
pub mod chat_fallback;
/// 控制型运行时能力（HITL / interrupt / schema 校验）。
pub mod control;
/// 执行域聚合模块。
pub mod exec;
/// Agent 运行期事件广播，供 UI 订阅流式输出与工具调用。
pub mod event_bus;
/// Hermes 风格迭代预算（consume / refund）。
pub mod iteration_budget;
/// 多轮对话主循环：工具调用、深度限制与回合结果。
pub mod loop_;
/// 提示词域：上下文、消息转换、hook 与 prompt builder。
pub mod prompt;
/// 流式补全与多轮流式迭代抽象。
pub mod streaming;
/// LLM 用量双写（UsageDb + SessionStore 账单）。
mod usage_record;
/// 助手回合时间线（astro_timeline_v1）。
pub mod timeline;

/// 链式构建可运行的 Agent 实例及其规格。
pub use builder::{AgentBuilder, BuiltAgentSpec};
/// 聊天 fallback 分类与流式尝试入口。
pub use chat_fallback::{
    is_failover_eligible, probe_or_wrap_pre_content, try_stream_completion_with_fallback,
    ActiveTargetMeta,
};
/// 上下文类型 re-export，便于调用方直接 `use agent::StaticContext`。
pub use prompt::context::{DynamicContext, StaticContext};
/// 上下文占用快照 re-export。
pub use prompt::context_usage::{build_snapshot, ContextUsageSegment, ContextUsageSnapshot};
/// 钩子 trait 与常用实现 re-export。
pub use prompt::hooks::{CancelSignal, PromptCancelled};
/// HITL 闸门 re-export。
pub use control::hitl::{
    is_exclusive_tool, is_interactive_tool, HitlGate, HitlRegistry, HitlRequest, HitlResolution,
    HITL_DEFAULT_TIMEOUT_SECS,
};
/// Interrupt 状态机 re-export。
pub use control::interrupt::{Interrupt, InterruptError, InterruptPending, ResumeItem};
/// 对话循环核心类型 re-export。
pub use loop_::{AgentConfig, AgentLoop, MaxDepthError, TurnResult};
/// 回合后记忆 review。
pub use exec::memory_review::{
    job_from_agent, maybe_run_background_review, review_notify_from_applied,
    spawn_background_review_after_turn, BackgroundReviewJob, MemoryReviewNotify,
};
/// 迭代预算 re-export。
pub use iteration_budget::{
    should_refund_tool_round, IterationBudget, DEFAULT_CHILD_MAX_ITERATIONS,
    DEFAULT_MAX_ITERATIONS,
};
/// 消息转换入口 re-export。
pub use prompt::messages::to_provider_messages;
/// Provider 侧用量统计与暂停控制 re-export。
pub use providers::{PauseControl, Usage};
/// 流式 API re-export。
pub use streaming::{
    chat_target_from_provider_config, run_multi_turn_stream, run_multi_turn_stream_from_provider,
    stream_multi_turn, stream_multi_turn_from_provider, stream_multi_turn_with_hitl,
    targets_and_registry_from_primary, MultiTurnStreamItem, ProviderStreamer,
    StreamedAssistantContent, StreamingChat, StreamingCompletion, StreamingPrompt,
};
/// 工具注册表与条目定义（实现位于 `tools` crate）。
pub use tools::{ToolEntry, ToolRegistry};
/// 临时保留旧模块名，确保外部旧路径仍可编译；Task 5 再收紧。
pub use control::{hitl, interrupt, schema_validate, smart_approval};
/// 执行域的过渡根别名；Task 5 再收紧。
pub use exec::cron as cron_exec;
pub use exec::delegate as delegate_exec;
pub use exec::orchestration;
pub use exec::multi_agent;

