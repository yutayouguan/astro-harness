//! Responses 原生流式抽象与多轮工具循环。
//!
//! 本模块将 Provider 原始 chunk 流映射为 Agent 语义事件，并实现
//! `StreamingResponses` trait 在 canonical Thread task 中驱动
//! 「LLM 流式 → 工具执行 → 再请求」闭环。
//!
//! 子模块划分：
//! - [`types`]：Provider assistant 内容类型（`StreamedAssistantContent`）
//! - [`traits`]：三层 Streaming trait
//! - [`provider`]：`ProviderStreamer`（trait 实现 + fallback 接入）
//! - [`fallback`]：聊天主模型故障切换（首包前 fallback）
//! - [`hitl_bridge`]：`astro_hitl` 解析与当前会话 park/resume 桥
//! - [`summary`]：迭代预算耗尽后的强制总结轮
//! - [`tools_exec`]：单轮工具调用执行（串行 HITL 路径 / 并发路径）
//! - [`multi_turn`]：多轮工具循环编排（本模块的核心）

/// Responses 主模型故障切换（首包前 fallback）。
pub mod fallback;
/// Astro HITL 桥：`astro_hitl` 解析与当前会话 park/resume。
pub(crate) mod hitl_bridge;
/// 多轮循环生命周期辅助：事件发送、usage 记录、终态收尾。
pub(crate) mod lifecycle;
/// 上下文维护：LLM 前后的压缩/摘要、工具结果记录、hook 集成。
mod maintenance;
/// 多轮工具循环编排。
pub(crate) mod multi_turn;
/// `ProviderStreamer`：Streaming trait 实现 + fallback 接入。
mod provider;
/// 显式 Run 阶段 / requirements。
pub mod run_state;
/// 迭代预算耗尽后的强制总结轮。
mod summary;
/// 单轮工具调用执行：串行（HITL/危险命令）与并发（普通工具）。
mod tools_exec;
/// Agent Responses streaming trait。
mod traits;
/// 流式事件与内容类型。
mod types;

pub use multi_turn::{
    run_multi_turn_events_with_responses_fn, run_multi_turn_stream_with_responses_fn,
    run_thread_turn_events, ThreadTurnEventArgs,
};
pub use provider::{ProviderStreamer, ResponsesOverride, ResponsesOverrideInput};
pub use run_state::{RunPhase, RunRequirements, RunState};
pub use traits::StreamingResponses;
pub use types::{AssistantContentStream, StreamedAssistantContent};
