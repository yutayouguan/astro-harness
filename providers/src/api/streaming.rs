//! Rig 风格流式原语：Usage、PauseControl（watch + AbortHandle，不依赖 rig crate）
//!
//! 对齐 https://docs.rs/rig-core/latest/src/rig_core/streaming.rs.html ：
//! - pause 用 `tokio::sync::watch`，避免 Notify 丢唤醒
//! - pause 时不 poll 上游（背压），cancel 用 `futures::stream::AbortHandle` 中止流

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use futures::stream::AbortHandle;
use tokio::sync::watch;

/// Token 用量（对齐 OpenAI usage / Rig FinalUsage）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_write_tokens: u32,
    pub reasoning_tokens: u32,
    pub request_count: u32,
}

impl Usage {
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
    pub fn add_assign(&mut self, other: Usage) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.cache_read_tokens = self.cache_read_tokens.saturating_add(other.cache_read_tokens);
        self.cache_write_tokens = self.cache_write_tokens.saturating_add(other.cache_write_tokens);
        self.reasoning_tokens = self.reasoning_tokens.saturating_add(other.reasoning_tokens);
        let n = if other.request_count == 0 { 1 } else { other.request_count };
        self.request_count = self.request_count.saturating_add(n);
    }
    pub fn from_parts(input: u32, output: u32) -> Self {
        Self {
            input_tokens: input,
            output_tokens: output,
            request_count: 1,
            ..Default::default()
        }
    }
    pub fn is_empty(&self) -> bool {
        self.input_tokens == 0
            && self.output_tokens == 0
            && self.cache_read_tokens == 0
            && self.cache_write_tokens == 0
            && self.reasoning_tokens == 0
    }
}

/// 暂停 / 恢复 / 取消（Rig 语义：pause 停止 poll；cancel 中止 Abortable 流）。
#[derive(Debug)]
pub struct PauseControl {
    /// 暂停状态广播发送端。
    paused_tx: watch::Sender<bool>,
    /// 保活接收端，避免无 subscriber 时 `send` 失败导致状态未写入。
    _paused_rx: watch::Receiver<bool>,
    /// 取消状态广播发送端。
    cancelled_tx: watch::Sender<bool>,
    /// 取消状态保活接收端。
    _cancelled_rx: watch::Receiver<bool>,
    /// 当前轮 HTTP 流的 abort 句柄（cancel 时触发）。
    abort: StdMutex<Option<AbortHandle>>,
    /// 快速路径，避免每次 clone receiver。
    cancelled_flag: AtomicBool,
}

impl PauseControl {
    /// 创建共享的暂停控制器（通常包裹在 `Arc` 中跨任务使用）。
    pub fn new() -> Arc<Self> {
        let (paused_tx, paused_rx) = watch::channel(false);
        let (cancelled_tx, cancelled_rx) = watch::channel(false);
        Arc::new(Self {
            paused_tx,
            _paused_rx: paused_rx,
            cancelled_tx,
            _cancelled_rx: cancelled_rx,
            abort: StdMutex::new(None),
            cancelled_flag: AtomicBool::new(false),
        })
    }

    /// 暂停上游流式 poll（阻塞在 `wait_if_paused` 处）。
    pub fn pause(&self) {
        let _ = self.paused_tx.send(true);
    }

    /// 恢复上游流式 poll。
    pub fn resume(&self) {
        let _ = self.paused_tx.send(false);
    }

    /// 取消本轮流：置位标志、解除暂停并 abort 已绑定的 HTTP 流。
    pub fn cancel(&self) {
        self.cancelled_flag.store(true, Ordering::SeqCst);
        let _ = self.cancelled_tx.send(true);
        let _ = self.paused_tx.send(false); // 解除 pause 等待
        if let Ok(mut guard) = self.abort.lock() {
            if let Some(handle) = guard.take() {
                handle.abort();
            }
        }
    }

    /// 当前是否处于暂停状态。
    pub fn is_paused(&self) -> bool {
        *self.paused_tx.borrow()
    }

    /// 当前是否已取消。
    pub fn is_cancelled(&self) -> bool {
        self.cancelled_flag.load(Ordering::SeqCst) || *self.cancelled_tx.borrow()
    }

    /// 等待取消信号（用于 `tokio::select!` 与上游 poll 竞速）
    pub async fn wait_cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        let mut rx = self.cancelled_tx.subscribe();
        while !*rx.borrow() {
            if rx.changed().await.is_err() {
                return;
            }
        }
    }

    /// 绑定本轮 provider 流的 AbortHandle（新一轮会替换并 abort 旧句柄）
    pub fn attach_abort(&self, handle: AbortHandle) {
        if let Ok(mut guard) = self.abort.lock() {
            if let Some(old) = guard.replace(handle) {
                old.abort();
            }
        }
    }

    pub fn clear_abort(&self) {
        if let Ok(mut guard) = self.abort.lock() {
            *guard = None;
        }
    }

    /// 若已取消返回 `false`；暂停时阻塞直到 resume / cancel（不丢唤醒）。
    pub async fn wait_if_paused(&self) -> bool {
        let mut paused_rx = self.paused_tx.subscribe();
        let mut cancelled_rx = self.cancelled_tx.subscribe();
        loop {
            if self.is_cancelled() || *cancelled_rx.borrow() {
                return false;
            }
            if !*paused_rx.borrow() {
                return true;
            }
            tokio::select! {
                result = paused_rx.changed() => {
                    if result.is_err() {
                        return false;
                    }
                }
                result = cancelled_rx.changed() => {
                    if result.is_err() || *cancelled_rx.borrow() {
                        return false;
                    }
                }
            }
        }
    }
}

impl Default for PauseControl {
    /// 创建未暂停、未取消的控制器（非 `Arc` 包装）。
    fn default() -> Self {
        let (paused_tx, paused_rx) = watch::channel(false);
        let (cancelled_tx, cancelled_rx) = watch::channel(false);
        Self {
            paused_tx,
            _paused_rx: paused_rx,
            cancelled_tx,
            _cancelled_rx: cancelled_rx,
            abort: StdMutex::new(None),
            cancelled_flag: AtomicBool::new(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn pause_blocks_until_resume() {
        let pc = PauseControl::new();
        pc.pause();
        let pc2 = pc.clone();
        let handle = tokio::spawn(async move { pc2.wait_if_paused().await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(!handle.is_finished());
        pc.resume();
        assert!(handle.await.unwrap());
    }

    #[tokio::test]
    async fn cancel_unblocks_with_false() {
        let pc = PauseControl::new();
        pc.pause();
        let pc2 = pc.clone();
        let handle = tokio::spawn(async move { pc2.wait_if_paused().await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        pc.cancel();
        assert!(!handle.await.unwrap());
        assert!(pc.is_cancelled());
    }

    #[tokio::test]
    async fn resume_during_wait_no_lost_wakeup() {
        // 密集 pause/resume，确保 watch 不错过
        let pc = PauseControl::new();
        for _ in 0..50 {
            pc.pause();
            let pc2 = pc.clone();
            let h = tokio::spawn(async move { pc2.wait_if_paused().await });
            tokio::task::yield_now().await;
            pc.resume();
            assert!(h.await.unwrap());
        }
    }

    #[test]
    fn usage_add_assign() {
        let mut a = Usage::from_parts(10, 5);
        a.add_assign(Usage::from_parts(3, 7));
        assert_eq!(a.prompt_tokens(), 13);
        assert_eq!(a.completion_tokens(), 12);
        assert_eq!(a.total_tokens(), 25);
    }
}
