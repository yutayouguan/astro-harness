//! Prompt Hooks：Agent 生命周期可插拔观察与协作取消。
//!
//! 钩子名对齐文档（`pre_llm_call`、`pre_tool_call` 等）。进程内总线见 [`hooks::PluginHookBus`]；
//! 本模块保留 trait 适配层，供 AgentLoop / 测试使用。内置 [`NoopHooks`]、[`RecordingHooks`]、
//! [`ChannelHooks`]（推 UI 时间线）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use common::message::Message;

/// 协作式取消信号。
#[derive(Clone, Default)]
pub struct CancelSignal {
    inner: Arc<AtomicBool>,
}

impl CancelSignal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.inner.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.load(Ordering::SeqCst)
    }
}

/// Prompt 循环因取消而中断。
#[derive(Debug, Clone)]
pub enum PromptCancelled {
    Cancelled { history: Vec<Message> },
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

/// Agent 主循环生命周期钩子（Hermes 同款命名）。
#[async_trait]
pub trait PromptHooks: Send + Sync {
    async fn on_session_start(&self, _session_id: &str, _cancel: &CancelSignal) {}

    /// 每轮 LLM 循环前；可返回注入上下文（由调用方读取 [`pre_llm_call_context`]）。
    async fn pre_llm_call(&self, _system_prompt: &str, _cancel: &CancelSignal) {}

    async fn pre_api_request(&self, _cancel: &CancelSignal) {}

    async fn post_api_request(&self, _error: Option<&str>, _cancel: &CancelSignal) {}

    async fn pre_tool_call(&self, _name: &str, _args: &serde_json::Value, _cancel: &CancelSignal) {}

    async fn post_tool_call(&self, _name: &str, _result: &str, _cancel: &CancelSignal) {}

    async fn post_llm_call(&self, _assistant_text: &str, _cancel: &CancelSignal) {}

    async fn on_session_end(&self, _turn: usize, _cancel: &CancelSignal) {}

    async fn on_session_finalize(&self, _session_id: &str, _cancel: &CancelSignal) {}

    async fn on_session_reset(&self, _session_id: &str, _cancel: &CancelSignal) {}

    async fn subagent_stop(&self, _child_session_id: &str, _summary: &str, _cancel: &CancelSignal) {}
}

/// 空操作 Hook。
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopHooks;

#[async_trait]
impl PromptHooks for NoopHooks {}

/// 测试用事件记录器（标签为 Hermes 钩子名）。
#[derive(Debug, Default)]
pub struct RecordingHooks {
    pub events: std::sync::Mutex<Vec<String>>,
    /// `pre_llm_call` 可选注入（测试用）。
    pub inject_context: std::sync::Mutex<Option<String>>,
}

impl RecordingHooks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> Vec<String> {
        self.events.lock().map(|g| g.clone()).unwrap_or_default()
    }

    fn push(&self, event: impl Into<String>) {
        if let Ok(mut g) = self.events.lock() {
            g.push(event.into());
        }
    }
}

#[async_trait]
impl PromptHooks for RecordingHooks {
    async fn on_session_start(&self, session_id: &str, _cancel: &CancelSignal) {
        self.push(format!("on_session_start:{session_id}"));
    }

    async fn pre_llm_call(&self, system_prompt: &str, _cancel: &CancelSignal) {
        self.push(format!("pre_llm_call:{}", system_prompt.len()));
    }

    async fn pre_api_request(&self, _cancel: &CancelSignal) {
        self.push("pre_api_request");
    }

    async fn post_api_request(&self, error: Option<&str>, _cancel: &CancelSignal) {
        match error {
            Some(e) => self.push(format!("post_api_request:err:{e}")),
            None => self.push("post_api_request"),
        }
    }

    async fn pre_tool_call(&self, name: &str, _args: &serde_json::Value, _cancel: &CancelSignal) {
        self.push(format!("pre_tool_call:{name}"));
    }

    async fn post_tool_call(&self, name: &str, _result: &str, _cancel: &CancelSignal) {
        self.push(format!("post_tool_call:{name}"));
    }

    async fn post_llm_call(&self, assistant_text: &str, _cancel: &CancelSignal) {
        self.push(format!("post_llm_call:{}", assistant_text.len()));
    }

    async fn on_session_end(&self, turn: usize, _cancel: &CancelSignal) {
        self.push(format!("on_session_end:{turn}"));
    }

    async fn on_session_finalize(&self, session_id: &str, _cancel: &CancelSignal) {
        self.push(format!("on_session_finalize:{session_id}"));
    }

    async fn on_session_reset(&self, session_id: &str, _cancel: &CancelSignal) {
        self.push(format!("on_session_reset:{session_id}"));
    }

    async fn subagent_stop(&self, child_session_id: &str, _summary: &str, _cancel: &CancelSignal) {
        self.push(format!("subagent_stop:{child_session_id}"));
    }
}

/// 推送到 UI / gRPC 的结构化事件。
#[derive(Debug, Clone)]
pub struct HookEvent {
    /// 钩子名，如 `pre_llm_call`。
    pub kind: String,
    pub detail: String,
    /// 可选 outcome 摘要（Continue / Block / …）。
    pub outcome: String,
}

/// Channel 生产者：实现 [`PromptHooks`] 并推送 Hermes 名事件。
pub struct ChannelHooks {
    tx: tokio::sync::mpsc::UnboundedSender<HookEvent>,
}

impl ChannelHooks {
    pub fn new(tx: tokio::sync::mpsc::UnboundedSender<HookEvent>) -> Self {
        Self { tx }
    }

    fn emit(&self, kind: impl Into<String>, detail: impl Into<String>) {
        let _ = self.tx.send(HookEvent {
            kind: kind.into(),
            detail: detail.into(),
            outcome: String::new(),
        });
    }
}

#[async_trait]
impl PromptHooks for ChannelHooks {
    async fn on_session_start(&self, session_id: &str, _cancel: &CancelSignal) {
        self.emit("on_session_start", format!("session={session_id}"));
    }

    async fn pre_llm_call(&self, system_prompt: &str, _cancel: &CancelSignal) {
        self.emit(
            "pre_llm_call",
            format!("system_prompt_chars={}", system_prompt.len()),
        );
    }

    async fn pre_api_request(&self, _cancel: &CancelSignal) {
        self.emit("pre_api_request", "");
    }

    async fn post_api_request(&self, error: Option<&str>, _cancel: &CancelSignal) {
        self.emit(
            "post_api_request",
            error.map(|e| format!("error={e}")).unwrap_or_default(),
        );
    }

    async fn pre_tool_call(&self, name: &str, args: &serde_json::Value, _cancel: &CancelSignal) {
        self.emit("pre_tool_call", format!("{name} {args}"));
    }

    async fn post_tool_call(&self, name: &str, result: &str, _cancel: &CancelSignal) {
        let preview: String = result.chars().take(200).collect();
        self.emit("post_tool_call", format!("{name} → {preview}"));
    }

    async fn post_llm_call(&self, assistant_text: &str, _cancel: &CancelSignal) {
        self.emit(
            "post_llm_call",
            format!("assistant_chars={}", assistant_text.len()),
        );
    }

    async fn on_session_end(&self, turn: usize, _cancel: &CancelSignal) {
        self.emit("on_session_end", format!("turn={turn}"));
    }

    async fn on_session_finalize(&self, session_id: &str, _cancel: &CancelSignal) {
        self.emit("on_session_finalize", format!("session={session_id}"));
    }

    async fn on_session_reset(&self, session_id: &str, _cancel: &CancelSignal) {
        self.emit("on_session_reset", format!("session={session_id}"));
    }

    async fn subagent_stop(&self, child_session_id: &str, summary: &str, _cancel: &CancelSignal) {
        let preview: String = summary.chars().take(120).collect();
        self.emit(
            "subagent_stop",
            format!("child={child_session_id} {preview}"),
        );
    }
}

/// 组合：先走 [`hooks::PluginHookBus`]，再走 trait hooks（观察推送）。
pub struct CompositeHooks {
    pub bus: Arc<::hooks::PluginHookBus>,
    pub inner: Arc<dyn PromptHooks>,
    pub session_started: AtomicBool,
}

impl CompositeHooks {
    pub fn new(bus: Arc<::hooks::PluginHookBus>, inner: Arc<dyn PromptHooks>) -> Self {
        Self {
            bus,
            inner,
            session_started: AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl PromptHooks for CompositeHooks {
    async fn on_session_start(&self, session_id: &str, cancel: &CancelSignal) {
        if self
            .session_started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let _ = self.bus.fire(
                ::hooks::ON_SESSION_START,
                &::hooks::HookPayload {
                    session_id: session_id.into(),
                    ..Default::default()
                },
            );
            self.inner.on_session_start(session_id, cancel).await;
        }
    }

    async fn pre_llm_call(&self, system_prompt: &str, cancel: &CancelSignal) {
        let _ = self.bus.fire(
            ::hooks::PRE_LLM_CALL,
            &::hooks::HookPayload {
                system_prompt_chars: Some(system_prompt.len()),
                detail: format!("system_prompt_chars={}", system_prompt.len()),
                ..Default::default()
            },
        );
        self.inner.pre_llm_call(system_prompt, cancel).await;
    }

    async fn pre_api_request(&self, cancel: &CancelSignal) {
        let _ = self
            .bus
            .fire(::hooks::PRE_API_REQUEST, &::hooks::HookPayload::default());
        self.inner.pre_api_request(cancel).await;
    }

    async fn post_api_request(&self, error: Option<&str>, cancel: &CancelSignal) {
        let _ = self.bus.fire(
            ::hooks::POST_API_REQUEST,
            &::hooks::HookPayload {
                error: error.map(|s| s.to_string()),
                detail: error.unwrap_or("").into(),
                ..Default::default()
            },
        );
        self.inner.post_api_request(error, cancel).await;
    }

    async fn pre_tool_call(&self, name: &str, args: &serde_json::Value, cancel: &CancelSignal) {
        let _ = self.bus.fire(
            ::hooks::PRE_TOOL_CALL,
            &::hooks::HookPayload {
                tool_name: Some(name.into()),
                tool_args: Some(args.clone()),
                detail: format!("{name} {args}"),
                ..Default::default()
            },
        );
        self.inner.pre_tool_call(name, args, cancel).await;
    }

    async fn post_tool_call(&self, name: &str, result: &str, cancel: &CancelSignal) {
        let preview: String = result.chars().take(200).collect();
        let _ = self.bus.fire(
            ::hooks::POST_TOOL_CALL,
            &::hooks::HookPayload {
                tool_name: Some(name.into()),
                tool_result: Some(result.into()),
                detail: format!("{name} → {preview}"),
                ..Default::default()
            },
        );
        self.inner.post_tool_call(name, result, cancel).await;
    }

    async fn post_llm_call(&self, assistant_text: &str, cancel: &CancelSignal) {
        let _ = self.bus.fire(
            ::hooks::POST_LLM_CALL,
            &::hooks::HookPayload {
                assistant_chars: Some(assistant_text.len()),
                detail: format!("assistant_chars={}", assistant_text.len()),
                ..Default::default()
            },
        );
        self.inner.post_llm_call(assistant_text, cancel).await;
    }

    async fn on_session_end(&self, turn: usize, cancel: &CancelSignal) {
        let _ = self.bus.fire(
            ::hooks::ON_SESSION_END,
            &::hooks::HookPayload {
                turn: Some(turn),
                detail: format!("turn={turn}"),
                ..Default::default()
            },
        );
        self.inner.on_session_end(turn, cancel).await;
    }

    async fn on_session_finalize(&self, session_id: &str, cancel: &CancelSignal) {
        let _ = self.bus.fire(
            ::hooks::ON_SESSION_FINALIZE,
            &::hooks::HookPayload {
                session_id: session_id.into(),
                ..Default::default()
            },
        );
        self.inner.on_session_finalize(session_id, cancel).await;
    }

    async fn on_session_reset(&self, session_id: &str, cancel: &CancelSignal) {
        let _ = self.bus.fire(
            ::hooks::ON_SESSION_RESET,
            &::hooks::HookPayload {
                session_id: session_id.into(),
                ..Default::default()
            },
        );
        self.inner.on_session_reset(session_id, cancel).await;
    }

    async fn subagent_stop(&self, child_session_id: &str, summary: &str, cancel: &CancelSignal) {
        let _ = self.bus.fire(
            ::hooks::SUBAGENT_STOP,
            &::hooks::HookPayload {
                session_id: child_session_id.into(),
                detail: summary.chars().take(200).collect(),
                ..Default::default()
            },
        );
        self.inner
            .subagent_stop(child_session_id, summary, cancel)
            .await;
    }
}
