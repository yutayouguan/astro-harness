//! Process-owned MCP event streams, independent from one Agent turn runtime.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::{timeout_at, Instant};

const UPDATE_CAPACITY: usize = 64;
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpEventNotification {
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[async_trait]
pub trait McpEventStreamOpener: Send + Sync {
    async fn open(
        &self,
        event_name: &str,
        arguments: &Value,
    ) -> Result<mpsc::Receiver<McpEventNotification>>;
}

#[derive(Debug)]
pub enum McpEventStreamUpdate {
    Notification {
        thread_id: String,
        server_id: String,
        subscription_id: String,
        stream_attempt_id: u64,
        notification: McpEventNotification,
    },
    Ended {
        thread_id: String,
        server_id: String,
        subscription_id: String,
        stream_attempt_id: u64,
        error: Option<String>,
    },
}

struct ManagedEventStream {
    server_id: String,
    stream_attempt_id: u64,
    active: Arc<AtomicBool>,
    worker: JoinHandle<()>,
}

pub struct McpEventStreamManager {
    streams: Mutex<HashMap<(String, String), ManagedEventStream>>,
    next_stream_attempt_id: AtomicU64,
    updates: mpsc::Sender<McpEventStreamUpdate>,
    shutdown: AtomicBool,
}

impl McpEventStreamManager {
    pub fn new() -> (Self, mpsc::Receiver<McpEventStreamUpdate>) {
        let (updates, receiver) = mpsc::channel(UPDATE_CAPACITY);
        (
            Self {
                streams: Mutex::default(),
                next_stream_attempt_id: AtomicU64::new(1),
                updates,
                shutdown: AtomicBool::new(false),
            },
            receiver,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn start(
        &self,
        thread_id: String,
        server_id: String,
        subscription_id: String,
        opener: Arc<dyn McpEventStreamOpener>,
        event_name: String,
        arguments: Value,
        mut access_generation: watch::Receiver<u64>,
    ) -> Result<u64> {
        let key = (thread_id.clone(), subscription_id.clone());
        let (ready_tx, ready_rx) = oneshot::channel();
        {
            let mut streams = self.streams.lock().unwrap_or_else(PoisonError::into_inner);
            if self.shutdown.load(Ordering::Acquire) {
                return Err(anyhow!("MCP event stream manager is shut down"));
            }
            streams.retain(|_, stream| !stream.worker.is_finished());
            if let Some(stream) = streams.get(&key) {
                if stream.active.load(Ordering::Acquire) {
                    return Ok(stream.stream_attempt_id);
                }
                return Err(anyhow!("MCP event subscription is already starting"));
            }
            let stream_attempt_id = self.next_stream_attempt_id.fetch_add(1, Ordering::Relaxed);
            let active = Arc::new(AtomicBool::new(false));
            let worker_active = Arc::clone(&active);
            let updates = self.updates.clone();
            let worker_thread_id = thread_id.clone();
            let worker_server_id = server_id.clone();
            let worker_subscription_id = subscription_id.clone();
            let worker = tokio::spawn(async move {
                let deadline = Instant::now() + ACTIVATION_TIMEOUT;
                let result = async {
                    let mut receiver = timeout_at(deadline, opener.open(&event_name, &arguments))
                        .await
                        .map_err(|_| anyhow!("MCP event stream activation timed out"))??;
                    let first = timeout_at(deadline, receiver.recv())
                        .await
                        .map_err(|_| anyhow!("MCP event stream activation timed out"))?
                        .ok_or_else(|| anyhow!("MCP event stream ended before activation"))?;
                    if first.method != "notifications/events/active" {
                        return Err(anyhow!("MCP event stream did not confirm activation"));
                    }
                    worker_active.store(true, Ordering::Release);
                    let _ = ready_tx.send(Ok(stream_attempt_id));
                    updates
                        .send(McpEventStreamUpdate::Notification {
                            thread_id: worker_thread_id.clone(),
                            server_id: worker_server_id.clone(),
                            subscription_id: worker_subscription_id.clone(),
                            stream_attempt_id,
                            notification: first,
                        })
                        .await
                        .map_err(|_| anyhow!("MCP event stream receiver was dropped"))?;
                    loop {
                        tokio::select! {
                            changed = access_generation.changed() => {
                                changed.map_err(|_| anyhow!("MCP event stream access changed"))?;
                                return Err(anyhow!("MCP event stream access changed"));
                            }
                            notification = receiver.recv() => {
                                let Some(notification) = notification else { return Ok(()); };
                                let terminated = notification.method == "notifications/events/terminated";
                                updates.send(McpEventStreamUpdate::Notification {
                                    thread_id: worker_thread_id.clone(),
                                    server_id: worker_server_id.clone(),
                                    subscription_id: worker_subscription_id.clone(),
                                    stream_attempt_id,
                                    notification,
                                }).await.map_err(|_| anyhow!("MCP event stream receiver was dropped"))?;
                                if terminated { return Ok(()); }
                            }
                            () = updates.closed() => return Err(anyhow!("MCP event stream receiver was dropped")),
                        }
                    }
                }
                .await;
                worker_active.store(false, Ordering::Release);
                let _ = updates
                    .send(McpEventStreamUpdate::Ended {
                        thread_id: worker_thread_id,
                        server_id: worker_server_id,
                        subscription_id: worker_subscription_id,
                        stream_attempt_id,
                        error: result.err().map(|error| error.to_string()),
                    })
                    .await;
            });
            streams.insert(
                key,
                ManagedEventStream {
                    server_id,
                    stream_attempt_id,
                    active,
                    worker,
                },
            );
        }
        ready_rx
            .await
            .map_err(|_| anyhow!("MCP event stream ended before activation"))?
    }

    pub async fn cancel(&self, thread_id: &str, subscription_id: &str) {
        let stream = self
            .streams
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&(thread_id.to_string(), subscription_id.to_string()));
        if let Some(stream) = stream {
            stream.worker.abort();
            let _ = stream.worker.await;
        }
    }

    pub async fn cancel_server(&self, server_id: &str) {
        let streams = self.take_server_streams(server_id);
        for stream in streams {
            stream.worker.abort();
            let _ = stream.worker.await;
        }
    }

    pub fn abort_server(&self, server_id: &str) {
        for stream in self.take_server_streams(server_id) {
            stream.worker.abort();
        }
    }

    fn take_server_streams(&self, server_id: &str) -> Vec<ManagedEventStream> {
        {
            let mut current = self.streams.lock().unwrap_or_else(PoisonError::into_inner);
            let keys = current
                .iter()
                .filter(|(_, stream)| stream.server_id == server_id)
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            keys.into_iter()
                .filter_map(|key| current.remove(&key))
                .collect::<Vec<_>>()
        }
    }

    pub async fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
        let streams = self
            .streams
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain()
            .map(|(_, stream)| stream)
            .collect::<Vec<_>>();
        for stream in &streams {
            stream.worker.abort();
        }
        for stream in streams {
            let _ = stream.worker.await;
        }
    }
}

impl Drop for McpEventStreamManager {
    fn drop(&mut self) {
        for (_, stream) in self
            .streams
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .drain()
        {
            stream.worker.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::Mutex as TokioMutex;

    struct TestOpener {
        opened: TokioMutex<Option<mpsc::Receiver<McpEventNotification>>>,
    }

    #[async_trait]
    impl McpEventStreamOpener for TestOpener {
        async fn open(
            &self,
            _event_name: &str,
            _arguments: &Value,
        ) -> Result<mpsc::Receiver<McpEventNotification>> {
            self.opened
                .lock()
                .await
                .take()
                .ok_or_else(|| anyhow!("stream already opened"))
        }
    }

    fn notification(method: &str) -> McpEventNotification {
        McpEventNotification {
            method: method.into(),
            params: Value::Null,
        }
    }

    #[tokio::test]
    async fn stream_activates_once_and_keeps_attempt_identity() {
        let (tx, rx) = mpsc::channel(8);
        let opener = Arc::new(TestOpener {
            opened: TokioMutex::new(Some(rx)),
        });
        let (_access_tx, access_rx) = watch::channel(0_u64);
        let (manager, mut updates) = McpEventStreamManager::new();
        let start = manager.start(
            "thread-1".into(),
            "server-1".into(),
            "sub-1".into(),
            opener,
            "event".into(),
            Value::Null,
            access_rx,
        );
        tokio::pin!(start);
        tokio::task::yield_now().await;
        tx.send(notification("notifications/events/active"))
            .await
            .unwrap();
        let attempt = start.await.unwrap();
        assert_eq!(attempt, 1);
        assert_eq!(
            manager
                .start(
                    "thread-1".into(),
                    "server-1".into(),
                    "sub-1".into(),
                    Arc::new(TestOpener {
                        opened: TokioMutex::new(None),
                    }),
                    "event".into(),
                    Value::Null,
                    watch::channel(0_u64).1,
                )
                .await
                .unwrap(),
            attempt
        );
        let McpEventStreamUpdate::Notification {
            stream_attempt_id, ..
        } = updates.recv().await.unwrap()
        else {
            panic!("activation notification expected");
        };
        assert_eq!(stream_attempt_id, attempt);
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn access_change_and_server_removal_stop_owned_streams() {
        let (tx, rx) = mpsc::channel(8);
        let opener = Arc::new(TestOpener {
            opened: TokioMutex::new(Some(rx)),
        });
        let (access_tx, access_rx) = watch::channel(0_u64);
        let (manager, mut updates) = McpEventStreamManager::new();
        let start = manager.start(
            "thread-1".into(),
            "server-1".into(),
            "sub-1".into(),
            opener,
            "event".into(),
            Value::Null,
            access_rx,
        );
        tokio::pin!(start);
        tokio::task::yield_now().await;
        tx.send(notification("notifications/events/active"))
            .await
            .unwrap();
        start.await.unwrap();
        updates.recv().await.unwrap();
        access_tx.send(1).unwrap();
        let McpEventStreamUpdate::Ended { error, .. } = updates.recv().await.unwrap() else {
            panic!("ended update expected");
        };
        assert!(error.is_some_and(|error| error.contains("access changed")));
        manager.cancel_server("server-1").await;
        manager.shutdown().await;
    }
}
