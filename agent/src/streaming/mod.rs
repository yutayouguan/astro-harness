//! Rig 风格流式分层与多轮工具循环。
//!
//! 本模块将 Provider 原始 chunk 流映射为 Agent 语义事件，并实现
//! `StreamingCompletion` / `StreamingChat` / `StreamingPrompt` 三层 trait，
//! 最终在 [`run_multi_turn_stream`] 中驱动「LLM 流式 → 工具执行 → 再请求」闭环。
//!
//! 子模块划分：
//! - [`types`]：事件与内容类型（`StreamedAssistantContent` / `MultiTurnStreamItem` 等）
//! - [`traits`]：三层 Streaming trait
//! - [`provider`]：`ProviderStreamer`（trait 实现 + fallback 接入）
//! - [`fallback`]：聊天主模型故障切换（首包前 fallback）
//! - [`hitl_bridge`]：`astro_hitl` 解析与父子会话间 park/resume 桥
//! - [`summary`]：迭代预算耗尽后的强制总结轮
//! - [`tools_exec`]：单轮工具调用执行（串行 HITL 路径 / 并发路径）
//! - [`multi_turn`]：多轮工具循环编排（本模块的核心）

/// 聊天主模型故障切换（首包前 fallback）。
pub mod fallback;
/// Astro HITL 桥：`astro_hitl` 解析与父子会话间 park/resume。
pub(crate) mod hitl_bridge;
/// 多轮工具循环编排。
mod multi_turn;
/// `ProviderStreamer`：Streaming trait 实现 + fallback 接入。
mod provider;
/// 显式 Run 阶段 / requirements。
pub mod run_state;
/// 迭代预算耗尽后的强制总结轮。
mod summary;
/// 单轮工具调用执行：串行（HITL/危险命令）与并发（普通工具）。
mod tools_exec;
/// 三层 Streaming trait。
mod traits;
/// 流式事件与内容类型。
mod types;

/// 供 `exec::delegate` 使用（子 Agent park 到父会话 HITL）。
pub(crate) use hitl_bridge::{parse_astro_hitl, try_park_parent_hitl};
pub use multi_turn::{
    run_multi_turn_stream, run_multi_turn_stream_from_provider, stream_multi_turn,
    stream_multi_turn_from_provider, stream_multi_turn_with_hitl,
};
pub use provider::{
    chat_target_from_provider_config, targets_and_registry_from_primary, ProviderStreamer,
};
pub use run_state::{RunPhase, RunRequirements, RunState};
pub use traits::{StreamingChat, StreamingCompletion, StreamingPrompt};
pub use types::{
    AssistantContentStream, MultiTurnStream, MultiTurnStreamItem, StreamedAssistantContent,
};
