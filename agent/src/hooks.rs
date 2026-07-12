//! Prompt Hooks：在 Agent 循环关键节点插入可插拔观察、遥测与协作式取消逻辑。
//!
//! 设计对齐 Rig Prompt Hooks：实现 [`PromptHooks`] 即可在提示词构建、模型回复、工具调用
//! 与轮次结束时获得回调；通过共享的 [`CancelSignal`] 可在任意钩子中请求中止当前循环。
//! 内置 [`NoopHooks`]（默认）、[`RecordingHooks`]（测试/断言）与 [`ChannelHooks`]（gRPC/UI 时间线）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use common::message::Message;

/// 协作式取消信号，可在多线程/多任务间共享。
///
/// Hook 或外部控制面调用 [`cancel`](Self::cancel) 后，Agent 循环应在各检查点调用
/// [`is_cancelled`](Self::is_cancelled) 并尽早退出，同时保留截至取消时刻的对话历史。
#[derive(Clone, Default)]
pub struct CancelSignal {
    /// 底层原子布尔，使用 `SeqCst` 保证跨线程可见性。
    inner: Arc<AtomicBool>,
}

impl CancelSignal {
    /// 创建未触发的新取消信号（与 `Default` 等价）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 请求取消当前 Prompt/Agent 循环；幂等，重复调用无副作用。
    pub fn cancel(&self) {
        self.inner.store(true, Ordering::SeqCst);
    }

    /// 查询是否已收到取消请求。
    pub fn is_cancelled(&self) -> bool {
        self.inner.load(Ordering::SeqCst)
    }
}

/// Prompt 循环因取消而中断时抛出的错误类型。
///
/// 携带取消发生前的完整 [`Message`] 历史，便于上层持久化部分结果或向用户展示上下文。
#[derive(Debug, Clone)]
pub enum PromptCancelled {
    /// 用户操作或 Hook 主动调用 [`CancelSignal::cancel`] 导致的中断。
    Cancelled {
        /// 取消时刻已累积的对话消息（含 system/user/assistant/tool 等）。
        history: Vec<Message>,
    },
}

impl std::fmt::Display for PromptCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled { history } => {
                write!(f, "prompt cancelled (history_len={})", history.len())
            }
        }
    }
}

impl std::error::Error for PromptCancelled {}

/// Agent 主循环的生命周期钩子接口。
///
/// 所有方法均有默认空实现，按需覆盖即可。每个回调均传入 [`CancelSignal`] 引用，
/// 实现方可在耗时观察逻辑中检查或触发取消。实现须为 `Send + Sync` 以跨异步任务共享。
#[async_trait]
pub trait PromptHooks: Send + Sync {
    /// 系统提示词组装完成后调用，可在此记录或修改观测数据（只读场景）。
    ///
    /// # 参数
    ///
    /// - `system_prompt`：即将送入模型的完整 system 字符串。
    /// - `cancel`：共享取消信号。
    async fn on_prompt_build(&self, _system_prompt: &str, _cancel: &CancelSignal) {}

    /// 助手文本回复（非流式聚合结果）生成后调用。
    async fn on_completion(&self, _assistant_text: &str, _cancel: &CancelSignal) {}

    /// 模型发起工具调用时调用，发生在实际执行工具之前。
    async fn on_tool_call(&self, _name: &str, _args: &serde_json::Value, _cancel: &CancelSignal) {}

    /// 工具执行完毕、结果写回会话前调用。
    async fn on_tool_result(&self, _name: &str, _result: &str, _cancel: &CancelSignal) {}

    /// 单轮（一次用户输入的处理周期）结束时调用，`turn` 为从 1 开始的轮次序号。
    async fn on_turn_end(&self, _turn: usize, _cancel: &CancelSignal) {}
}

/// 空操作 Hook，作为默认实现不产生任何副作用。
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopHooks;

#[async_trait]
impl PromptHooks for NoopHooks {}

/// 将 Hook 事件序列化写入内存缓冲，供单元测试或本地调试断言。
///
/// 事件为简短字符串（如 `tool_call:search`），不保留完整 payload 以降低测试耦合。
#[derive(Debug, Default)]
pub struct RecordingHooks {
    /// 按发生顺序记录的事件标签列表；由 Mutex 保护以支持并发回调。
    pub events: std::sync::Mutex<Vec<String>>,
}

impl RecordingHooks {
    /// 创建空的事件记录器。
    pub fn new() -> Self {
        Self::default()
    }

    /// 获取当前已记录事件的快照副本，不消费或清空缓冲区。
    pub fn snapshot(&self) -> Vec<String> {
        self.events.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// 追加一条事件；若 Mutex 中毒则静默丢弃（测试场景下视为可接受）。
    fn push(&self, event: impl Into<String>) {
        if let Ok(mut g) = self.events.lock() {
            g.push(event.into());
        }
    }
}

#[async_trait]
impl PromptHooks for RecordingHooks {
    /// 记录提示词构建事件（长度）。
    async fn on_prompt_build(&self, system_prompt: &str, _cancel: &CancelSignal) {
        self.push(format!("prompt_build:{}", system_prompt.len()));
    }

    /// 记录助手回复事件（长度）。
    async fn on_completion(&self, assistant_text: &str, _cancel: &CancelSignal) {
        self.push(format!("completion:{}", assistant_text.len()));
    }

    /// 记录工具调用事件。
    async fn on_tool_call(&self, name: &str, _args: &serde_json::Value, _cancel: &CancelSignal) {
        self.push(format!("tool_call:{name}"));
    }

    /// 记录工具结果事件。
    async fn on_tool_result(&self, name: &str, _result: &str, _cancel: &CancelSignal) {
        self.push(format!("tool_result:{name}"));
    }

    /// 记录轮次结束事件。
    async fn on_turn_end(&self, turn: usize, _cancel: &CancelSignal) {
        self.push(format!("turn_end:{turn}"));
    }
}

/// 通过 channel 向外推送的结构化 Hook 事件，供 gRPC 流或 UI 时间线消费。
#[derive(Debug, Clone)]
pub struct HookEvent {
    /// 事件类型标识，如 `hook:tool_call`、`hook:completion`。
    pub kind: String,
    /// 人类可读的摘要或预览文本（工具结果可能被截断）。
    pub detail: String,
}

/// 将 Hook 回调转为无界 channel 消息的生产者实现。
///
/// 发送失败（接收端已关闭）时静默忽略，避免打断 Agent 主循环。
pub struct ChannelHooks {
    /// 事件出站通道发送端。
    tx: tokio::sync::mpsc::UnboundedSender<HookEvent>,
}

impl ChannelHooks {
    /// 绑定既有 `UnboundedSender`，由调用方负责创建接收端与背压策略。
    pub fn new(tx: tokio::sync::mpsc::UnboundedSender<HookEvent>) -> Self {
        Self { tx }
    }

    /// 构造并发送一条 [`HookEvent`]；接收端关闭时不报错。
    fn emit(&self, kind: impl Into<String>, detail: impl Into<String>) {
        let _ = self.tx.send(HookEvent {
            kind: kind.into(),
            detail: detail.into(),
        });
    }
}

#[async_trait]
impl PromptHooks for ChannelHooks {
    /// 推送提示词构建 Hook 事件。
    async fn on_prompt_build(&self, system_prompt: &str, _cancel: &CancelSignal) {
        self.emit(
            "hook:prompt_build",
            format!("system_prompt_chars={}", system_prompt.len()),
        );
    }

    /// 推送助手回复 Hook 事件。
    async fn on_completion(&self, assistant_text: &str, _cancel: &CancelSignal) {
        self.emit(
            "hook:completion",
            format!("assistant_chars={}", assistant_text.len()),
        );
    }

    /// 推送工具调用 Hook 事件。
    async fn on_tool_call(&self, name: &str, args: &serde_json::Value, _cancel: &CancelSignal) {
        self.emit("hook:tool_call", format!("{name} {args}"));
    }

    /// 推送工具结果 Hook 事件（正文截断预览）。
    async fn on_tool_result(&self, name: &str, result: &str, _cancel: &CancelSignal) {
        let preview: String = result.chars().take(200).collect();
        self.emit("hook:tool_result", format!("{name} → {preview}"));
    }

    /// 推送轮次结束 Hook 事件。
    async fn on_turn_end(&self, turn: usize, _cancel: &CancelSignal) {
        self.emit("hook:turn_end", format!("turn={turn}"));
    }
}
