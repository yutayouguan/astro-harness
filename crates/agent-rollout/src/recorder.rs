use std::io;
use std::path::{Path, PathBuf};

use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, oneshot};

use crate::{is_persisted_rollout_item, RolloutItem, ThreadHistoryMode};

enum RecorderCommand {
    Record {
        items: Vec<RolloutItem>,
        reply: oneshot::Sender<io::Result<()>>,
    },
    Flush {
        reply: oneshot::Sender<io::Result<()>>,
    },
    Shutdown {
        reply: oneshot::Sender<io::Result<()>>,
    },
}

#[derive(Clone)]
pub struct RolloutRecorder {
    path: PathBuf,
    mode: ThreadHistoryMode,
    tx: mpsc::UnboundedSender<RecorderCommand>,
}

impl RolloutRecorder {
    pub async fn open(path: PathBuf, mode: ThreadHistoryMode) -> io::Result<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            tokio::fs::create_dir_all(parent).await?;
        }
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        let (tx, mut rx) = mpsc::unbounded_channel();

        tokio::spawn(async move {
            let mut file = file;
            while let Some(command) = rx.recv().await {
                match command {
                    RecorderCommand::Record { items, reply } => {
                        let result = write_items(&mut file, items).await;
                        let _ = reply.send(result);
                    }
                    RecorderCommand::Flush { reply } => {
                        let _ = reply.send(file.flush().await);
                    }
                    RecorderCommand::Shutdown { reply } => {
                        let result = file.flush().await;
                        let _ = reply.send(result);
                        break;
                    }
                }
            }
        });

        Ok(Self { path, mode, tx })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn record(&self, items: Vec<RolloutItem>) -> io::Result<()> {
        let items = items
            .into_iter()
            .filter(|item| is_persisted_rollout_item(item, self.mode))
            .collect();
        let (reply, receiver) = oneshot::channel();
        self.tx
            .send(RecorderCommand::Record { items, reply })
            .map_err(|_| io::Error::other("rollout writer closed"))?;
        receiver
            .await
            .map_err(|_| io::Error::other("rollout writer reply closed"))?
    }

    pub async fn flush(&self) -> io::Result<()> {
        let (reply, receiver) = oneshot::channel();
        self.tx
            .send(RecorderCommand::Flush { reply })
            .map_err(|_| io::Error::other("rollout writer closed"))?;
        receiver
            .await
            .map_err(|_| io::Error::other("rollout writer reply closed"))?
    }

    pub async fn shutdown(&self) -> io::Result<()> {
        let (reply, receiver) = oneshot::channel();
        self.tx
            .send(RecorderCommand::Shutdown { reply })
            .map_err(|_| io::Error::other("rollout writer closed"))?;
        receiver
            .await
            .map_err(|_| io::Error::other("rollout writer reply closed"))?
    }
}

async fn write_items(file: &mut tokio::fs::File, items: Vec<RolloutItem>) -> io::Result<()> {
    for item in items {
        let bytes = serde_json::to_vec(&item).map_err(io::Error::other)?;
        file.write_all(&bytes).await?;
        file.write_all(b"\n").await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use agent_protocol::{event::TurnStartedEvent, EventMsg};
    use serde_json::json;
    use tempfile::TempDir;

    use super::RolloutRecorder;
    use crate::{read_rollout, RolloutItem, ThreadHistoryMode};

    #[tokio::test]
    async fn record_then_flush_preserves_append_order() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("rollout.jsonl");
        let recorder = RolloutRecorder::open(path.clone(), ThreadHistoryMode::Paginated)
            .await
            .unwrap();

        recorder
            .record(vec![
                RolloutItem::SessionMeta(json!({"thread_id": "thread-1"})),
                RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
                    turn_id: "turn-1".into(),
                })),
            ])
            .await
            .unwrap();
        recorder.flush().await.unwrap();

        let items = read_rollout(&path).await.unwrap();
        assert!(matches!(items.first(), Some(RolloutItem::SessionMeta(_))));
        assert!(matches!(
            items.get(1),
            Some(RolloutItem::EventMsg(EventMsg::TurnStarted(_)))
        ));

        recorder.shutdown().await.unwrap();
    }
}
