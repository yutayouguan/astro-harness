use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, watch, Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use crate::{ListenerCommand, ThreadActivity};

pub struct ManagedThread {
    pub runtime: Arc<agent::AstroThread>,
    pub commands: mpsc::UnboundedSender<ListenerCommand>,
    pub activity_rx: watch::Receiver<ThreadActivity>,
    listener: Mutex<Option<tokio::task::JoinHandle<()>>>,
    lifecycle: CancellationToken,
    side_effect_supervisor: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl ManagedThread {
    pub fn new(
        runtime: Arc<agent::AstroThread>,
        commands: mpsc::UnboundedSender<ListenerCommand>,
        activity_rx: watch::Receiver<ThreadActivity>,
        listener: tokio::task::JoinHandle<()>,
    ) -> Self {
        Self {
            runtime,
            commands,
            activity_rx,
            listener: Mutex::new(Some(listener)),
            lifecycle: CancellationToken::new(),
            side_effect_supervisor: Mutex::new(None),
        }
    }

    pub fn lifecycle_token(&self) -> CancellationToken {
        self.lifecycle.clone()
    }

    pub async fn set_side_effect_supervisor(&self, supervisor: tokio::task::JoinHandle<()>) {
        let mut slot = self.side_effect_supervisor.lock().await;
        if self.lifecycle.is_cancelled() {
            drop(slot);
            supervisor.abort();
            let _ = supervisor.await;
            return;
        }
        let replaced = slot.replace(supervisor);
        drop(slot);
        if let Some(replaced) = replaced {
            replaced.abort();
            let _ = replaced.await;
        }
    }

    pub async fn stop_side_effects(&self) {
        self.lifecycle.cancel();
        let supervisor = self.side_effect_supervisor.lock().await.take();
        if let Some(supervisor) = supervisor {
            let _ = supervisor.await;
        }
    }

    pub async fn stop_listener(&self) {
        self.stop_side_effects().await;
        let _ = self.commands.send(ListenerCommand::Stop);
        let listener = self.listener.lock().await.take();
        if let Some(listener) = listener {
            let _ = listener.await;
        }
    }

    pub async fn listener_is_finished(&self) -> bool {
        self.listener
            .lock()
            .await
            .as_ref()
            .is_none_or(tokio::task::JoinHandle::is_finished)
    }

    #[cfg(test)]
    pub async fn side_effect_supervisor_is_finished(&self) -> bool {
        self.side_effect_supervisor
            .lock()
            .await
            .as_ref()
            .is_none_or(tokio::task::JoinHandle::is_finished)
    }
}

pub enum RemoveCurrentThread {
    Removed(Arc<ManagedThread>),
    Leased,
    NotCurrent,
}

#[derive(Clone, Default)]
pub struct ThreadManager {
    entries: Arc<RwLock<HashMap<String, Arc<ManagedThread>>>>,
    creation_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl ThreadManager {
    pub async fn get(&self, thread_id: &str) -> Option<Arc<ManagedThread>> {
        let creation_lock = self.creation_lock(thread_id).await;
        let _operation = creation_lock.lock().await;
        self.get_locked(thread_id).await
    }

    /// 在调用方持有该 thread 创建锁时读取当前条目。
    pub(crate) async fn get_locked(&self, thread_id: &str) -> Option<Arc<ManagedThread>> {
        self.entries.read().await.get(thread_id).cloned()
    }

    pub async fn insert_if_absent(
        &self,
        thread_id: String,
        candidate: Arc<ManagedThread>,
    ) -> Result<(), Arc<ManagedThread>> {
        let mut entries = self.entries.write().await;
        if let Some(existing) = entries.get(&thread_id) {
            return Err(existing.clone());
        }
        entries.insert(thread_id, candidate);
        Ok(())
    }

    pub async fn remove(&self, thread_id: &str) -> Option<Arc<ManagedThread>> {
        self.entries.write().await.remove(thread_id)
    }

    pub async fn remove_if_current_and_unleased(
        &self,
        thread_id: &str,
        expected: &Arc<ManagedThread>,
    ) -> RemoveCurrentThread {
        let mut entries = self.entries.write().await;
        if !entries
            .get(thread_id)
            .is_some_and(|current| Arc::ptr_eq(current, expected))
        {
            return RemoveCurrentThread::NotCurrent;
        }
        // manager 条目和 idle-unload 任务是两个持有引用。
        // 额外的引用代表进行中的操作，必须先完成。
        if Arc::strong_count(expected) > 2 {
            return RemoveCurrentThread::Leased;
        }
        RemoveCurrentThread::Removed(
            entries
                .remove(thread_id)
                .expect("current thread disappeared while entries are write-locked"),
        )
    }

    pub async fn contains(&self, thread_id: &str) -> bool {
        self.entries.read().await.contains_key(thread_id)
    }

    pub async fn creation_lock(&self, thread_id: &str) -> Arc<Mutex<()>> {
        let mut locks = self.creation_locks.lock().await;
        locks.retain(|_, lock| Arc::strong_count(lock) > 1);
        locks
            .entry(thread_id.into())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}
