//! Agent 运行时核心 crate：组装对话循环、上下文、钩子与流式输出。
//!
//! 对外暴露构建器、循环控制、消息转换与工具注册等能力，供 Tauri 前端与后端服务复用。
//! `ToolRegistry` 已迁移至 `tools` crate，此处保留 re-export 以兼容旧导入路径。

/// 声明式 Agent 构建与规格导出。
pub mod builder;
/// 静态/动态上下文分层，用于 system prompt 组装。
pub mod context;
/// 定时任务执行逻辑（与 `cron_tools` 注册的工具配合）。
pub mod cron_exec;
/// Agent 运行期事件广播，供 UI 订阅流式输出与工具调用。
pub mod event_bus;
/// 提示词生命周期钩子（取消、录制、渠道回调等）。
pub mod hooks;
/// AG-UI 风格 interrupt 挂起与 resume 校验。
pub mod interrupt;
/// 多轮对话主循环：工具调用、深度限制与回合结果。
pub mod loop_;
/// 会话消息到 Provider API 消息的格式转换。
pub mod messages;
/// 多子 Agent 并行编排（任务拆分与结果汇总）。
pub mod multi_agent;
/// 将静态/动态上下文等层叠为完整 system prompt。
pub mod prompt_builder;
/// 流式补全与多轮流式迭代抽象。
pub mod streaming;

// 兼容旧路径：ToolRegistry 现位于 tools crate
/// 链式构建可运行的 Agent 实例及其规格。
pub use builder::{AgentBuilder, BuiltAgentSpec};
/// 上下文类型 re-export，便于调用方直接 `use agent::StaticContext`。
pub use context::{DynamicContext, StaticContext};
/// 钩子 trait 与常用实现 re-export。
pub use hooks::{
    CancelSignal, ChannelHooks, HookEvent, NoopHooks, PromptCancelled, PromptHooks, RecordingHooks,
};
/// Interrupt 状态机 re-export。
pub use interrupt::{Interrupt, InterruptError, InterruptPending, ResumeItem};
/// 对话循环核心类型 re-export。
pub use loop_::{AgentConfig, AgentLoop, MaxDepthError, TurnResult};
/// 消息转换入口 re-export。
pub use messages::to_provider_messages;
/// Provider 侧用量统计与暂停控制 re-export。
pub use providers::{PauseControl, Usage};
/// 流式 API re-export。
pub use streaming::{
    stream_multi_turn, MultiTurnStreamItem, ProviderStreamer, StreamedAssistantContent,
    StreamingChat, StreamingCompletion, StreamingPrompt,
};
/// 工具注册表与条目定义（实现位于 `tools` crate）。
pub use tools::{ToolEntry, ToolRegistry};
