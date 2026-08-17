use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tokio::sync::{mpsc, Notify};

/// 防止不同会话通过保留已完成线程无限占用 blocking worker。
pub const MAX_LIVE_AGENT_THREADS: usize = 32;

#[derive(Debug)]
pub enum AgentThreadCommand {
    FollowUp(String),
    Close,
}

#[derive(Debug, Default)]
pub struct AgentThreadControl {
    interrupted: AtomicBool,
    closed: AtomicBool,
    notify: Notify,
}

impl AgentThreadControl {
    pub fn interrupt(&self) {
        self.interrupted.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn begin_turn(&self) {
        if !self.closed.load(Ordering::SeqCst) {
            self.interrupted.store(false, Ordering::SeqCst);
        }
    }

    pub fn is_interrupted(&self) -> bool {
        self.interrupted.load(Ordering::SeqCst)
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    pub async fn cancelled(&self) {
        loop {
            if self.is_interrupted() || self.is_closed() {
                return;
            }
            self.notify.notified().await;
        }
    }
}

#[derive(Clone)]
struct LiveThread {
    tx: mpsc::UnboundedSender<AgentThreadCommand>,
    control: Arc<AgentThreadControl>,
    parent_session_id: String,
}

#[derive(Default)]
pub struct LiveAgentThreads {
    inner: Mutex<HashMap<String, LiveThread>>,
}

impl LiveAgentThreads {
    pub fn global() -> &'static Self {
        static REGISTRY: OnceLock<LiveAgentThreads> = OnceLock::new();
        REGISTRY.get_or_init(Self::default)
    }

    pub fn ensure_capacity(
        &self,
        parent_session_id: &str,
        max_per_session: usize,
    ) -> anyhow::Result<()> {
        let live = self.inner.lock().unwrap();
        check_capacity(&live, parent_session_id, max_per_session)
    }

    /// 原子检查并登记存活线程。已完成但仍可追问的线程也保留在此表中，
    /// 因此会继续占用会话级与全局资源预算，直到显式关闭或进程退出。
    pub fn register_bounded(
        &self,
        thread_id: &str,
        parent_session_id: &str,
        max_per_session: usize,
    ) -> anyhow::Result<(
        Arc<AgentThreadControl>,
        mpsc::UnboundedReceiver<AgentThreadCommand>,
    )> {
        let (tx, rx) = mpsc::unbounded_channel();
        let control = Arc::new(AgentThreadControl::default());
        let mut live = self.inner.lock().unwrap();
        if live.contains_key(thread_id) {
            anyhow::bail!("agent thread is already live: {thread_id}");
        }
        check_capacity(&live, parent_session_id, max_per_session)?;
        live.insert(
            thread_id.to_string(),
            LiveThread {
                tx,
                control: Arc::clone(&control),
                parent_session_id: parent_session_id.to_string(),
            },
        );
        Ok((control, rx))
    }

    pub fn send_follow_up(&self, thread_id: &str, message: String) -> anyhow::Result<()> {
        let live = self
            .inner
            .lock()
            .unwrap()
            .get(thread_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("agent thread is not live: {thread_id}"))?;
        live.tx
            .send(AgentThreadCommand::FollowUp(message))
            .map_err(|_| anyhow::anyhow!("agent thread command channel closed: {thread_id}"))
    }

    pub fn interrupt(&self, thread_id: &str) -> anyhow::Result<()> {
        let live = self
            .inner
            .lock()
            .unwrap()
            .get(thread_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("agent thread is not live: {thread_id}"))?;
        live.control.interrupt();
        Ok(())
    }

    pub fn close(&self, thread_id: &str) -> anyhow::Result<()> {
        let live = self
            .inner
            .lock()
            .unwrap()
            .get(thread_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("agent thread is not live: {thread_id}"))?;
        live.control.close();
        let _ = live.tx.send(AgentThreadCommand::Close);
        Ok(())
    }

    pub fn remove(&self, thread_id: &str) {
        self.inner.lock().unwrap().remove(thread_id);
    }

    pub fn is_live(&self, thread_id: &str) -> bool {
        self.inner.lock().unwrap().contains_key(thread_id)
    }
}

fn check_capacity(
    live: &HashMap<String, LiveThread>,
    parent_session_id: &str,
    max_per_session: usize,
) -> anyhow::Result<()> {
    let session_count = live
        .values()
        .filter(|thread| thread.parent_session_id == parent_session_id)
        .count();
    if session_count >= max_per_session {
        anyhow::bail!(
            "subagent live-thread limit reached for session ({session_count}/{max_per_session})"
        );
    }
    if live.len() >= MAX_LIVE_AGENT_THREADS {
        anyhow::bail!(
            "global subagent live-thread limit reached ({}/{MAX_LIVE_AGENT_THREADS})",
            live.len()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn follow_up_interrupt_and_close_are_independent_controls() {
        let registry = LiveAgentThreads::default();
        let (control, mut rx) = registry.register_bounded("thread", "parent", 1).unwrap();
        registry.send_follow_up("thread", "next".into()).unwrap();
        assert!(matches!(
            rx.recv().await,
            Some(AgentThreadCommand::FollowUp(message)) if message == "next"
        ));

        registry.interrupt("thread").unwrap();
        control.cancelled().await;
        assert!(control.is_interrupted());
        assert!(!control.is_closed());

        control.begin_turn();
        assert!(!control.is_interrupted());
        registry.close("thread").unwrap();
        assert!(control.is_closed());
        assert!(matches!(rx.recv().await, Some(AgentThreadCommand::Close)));
    }

    #[test]
    fn completed_live_threads_still_consume_session_budget() {
        let registry = LiveAgentThreads::default();
        let (_first, _first_rx) = registry.register_bounded("first", "parent", 1).unwrap();
        let error = registry
            .register_bounded("second", "parent", 1)
            .unwrap_err();
        assert!(error.to_string().contains("live-thread limit"));

        registry.remove("first");
        assert!(registry.register_bounded("second", "parent", 1).is_ok());
    }

    #[test]
    fn live_thread_limit_is_scoped_per_parent_session() {
        let registry = LiveAgentThreads::default();
        let (_first, _first_rx) = registry.register_bounded("first", "parent-a", 1).unwrap();
        assert!(registry.register_bounded("second", "parent-b", 1).is_ok());
    }

    #[test]
    fn global_live_thread_limit_bounds_all_sessions() {
        let registry = LiveAgentThreads::default();
        for index in 0..MAX_LIVE_AGENT_THREADS {
            registry
                .register_bounded(&format!("thread-{index}"), &format!("parent-{index}"), 1)
                .unwrap();
        }
        let error = registry
            .register_bounded("overflow", "overflow-parent", 1)
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("global subagent live-thread limit"));
    }
}
