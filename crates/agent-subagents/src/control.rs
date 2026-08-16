use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tokio::sync::{mpsc, Notify};

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

    pub fn register(
        &self,
        thread_id: &str,
    ) -> (
        Arc<AgentThreadControl>,
        mpsc::UnboundedReceiver<AgentThreadCommand>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        let control = Arc::new(AgentThreadControl::default());
        self.inner.lock().unwrap().insert(
            thread_id.to_string(),
            LiveThread {
                tx,
                control: Arc::clone(&control),
            },
        );
        (control, rx)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn follow_up_interrupt_and_close_are_independent_controls() {
        let registry = LiveAgentThreads::default();
        let (control, mut rx) = registry.register("thread");
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
}
