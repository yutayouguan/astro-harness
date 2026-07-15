//! Agent 侧钩子辅助：协作取消与（可选）遗留事件类型。
//!
//! 进程内生命周期总线见 [`hooks::PluginHookBus`]；UI 时间线由
//! [`hooks::UiTimelineSlot`] 挂在共享 bus 上，不再走本模块的 trait 适配层。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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
