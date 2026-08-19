use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, watch, Mutex, RwLock};

use crate::{ListenerCommand, ThreadActivity};

pub struct ManagedThread {
    pub runtime: Arc<agent::AstroThread>,
    pub commands: mpsc::UnboundedSender<ListenerCommand>,
    pub activity_rx: watch::Receiver<ThreadActivity>,
    listener: Mutex<Option<tokio::task::JoinHandle<()>>>,
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
        }
    }

    pub async fn stop_listener(&self) {
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
}

#[derive(Clone, Default)]
pub struct ThreadManager {
    entries: Arc<RwLock<HashMap<String, Arc<ManagedThread>>>>,
    creation_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl ThreadManager {
    pub async fn get(&self, thread_id: &str) -> Option<Arc<ManagedThread>> {
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

    pub async fn remove_if_current(
        &self,
        thread_id: &str,
        expected: &Arc<ManagedThread>,
    ) -> Option<Arc<ManagedThread>> {
        let mut entries = self.entries.write().await;
        if entries
            .get(thread_id)
            .is_some_and(|current| Arc::ptr_eq(current, expected))
        {
            entries.remove(thread_id)
        } else {
            None
        }
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
