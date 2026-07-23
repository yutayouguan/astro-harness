//! 流式响应分片。

use std::pin::Pin;

use futures::Stream;
use serde::{Deserialize, Serialize};

/// Token 用量。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_write_tokens: u32,
    pub reasoning_tokens: u32,
    pub request_count: u32,
}

impl Usage {
    pub fn from_parts(input: u32, output: u32) -> Self {
        Self {
            input_tokens: input,
            output_tokens: output,
            request_count: 1,
            ..Default::default()
        }
    }

    pub fn prompt_tokens(&self) -> u32 {
        self.input_tokens
            .saturating_add(self.cache_read_tokens)
            .saturating_add(self.cache_write_tokens)
    }

    pub fn completion_tokens(&self) -> u32 {
        self.output_tokens
    }

    pub fn total_tokens(&self) -> u32 {
        self.prompt_tokens().saturating_add(self.completion_tokens())
    }

    pub fn is_empty(&self) -> bool {
        self.input_tokens == 0
            && self.output_tokens == 0
            && self.cache_read_tokens == 0
            && self.cache_write_tokens == 0
            && self.reasoning_tokens == 0
    }

    pub fn add_assign(&mut self, other: Self) {
        if other.is_empty() {
            return;
        }
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.cache_read_tokens = self.cache_read_tokens.saturating_add(other.cache_read_tokens);
        self.cache_write_tokens = self.cache_write_tokens.saturating_add(other.cache_write_tokens);
        self.reasoning_tokens = self.reasoning_tokens.saturating_add(other.reasoning_tokens);
        self.request_count = self.request_count.saturating_add(if other.request_count > 0 {
            other.request_count
        } else {
            1
        });
    }
}

/// 流式分片（所有厂商统一输出）。
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// 正文 token。
    Text(String),
    /// 推理/思考内容。
    Thinking(String),
    /// 思考签名（多轮连续性）。
    ThoughtSignature(String),
    /// 工具调用开始。
    ToolCallStart {
        index: u32,
        id: String,
        name: String,
    },
    /// 工具调用参数增量。
    ToolCallDelta {
        index: u32,
        arguments: String,
    },
    /// Token 用量。
    Usage(Usage),
    /// 引用信息。
    Citation(serde_json::Value),
    /// 流结束。
    Done {
        finish_reason: String,
    },
    /// 错误。
    Error(String),
    /// Google Interactions：interaction id。
    InteractionId(String),
}

/// 流式补全响应。
pub type CompletionStream = Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>;

/// 暂停/恢复/取消控制。
#[derive(Debug, Default)]
pub struct PauseControl {
    paused: std::sync::atomic::AtomicBool,
    aborted: std::sync::atomic::AtomicBool,
}

impl PauseControl {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pause(&self) {
        self.paused
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn resume(&self) {
        self.paused
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn abort(&self) {
        self.aborted
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn is_aborted(&self) -> bool {
        self.aborted.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn clear_abort(&self) {
        self.aborted
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_arithmetic() {
        let mut a = Usage::from_parts(10, 5);
        a.add_assign(Usage::from_parts(3, 7));
        assert_eq!(a.input_tokens, 13);
        assert_eq!(a.output_tokens, 12);
        assert_eq!(a.total_tokens(), 25);
        assert_eq!(a.request_count, 2);
    }

    #[test]
    fn usage_empty_noop() {
        let mut a = Usage::from_parts(10, 5);
        a.add_assign(Usage::default());
        assert_eq!(a.input_tokens, 10);
        assert_eq!(a.request_count, 1);
    }
}
