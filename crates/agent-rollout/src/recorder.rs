use std::io;
use std::path::{Path, PathBuf};

use tokio::io::{AsyncWrite, AsyncWriteExt};
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
        let (tx, rx) = mpsc::unbounded_channel();

        tokio::spawn(run_writer_loop(file, rx));

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

async fn run_writer_loop<W>(mut writer: W, mut rx: mpsc::UnboundedReceiver<RecorderCommand>)
where
    W: AsyncWrite + Unpin,
{
    while let Some(command) = rx.recv().await {
        match command {
            RecorderCommand::Record { items, reply } => {
                let result = write_items(&mut writer, items).await;
                let failed = result.is_err();
                let _ = reply.send(result);
                if failed {
                    return;
                }
            }
            RecorderCommand::Flush { reply } => {
                let result = writer.flush().await;
                let failed = result.is_err();
                let _ = reply.send(result);
                if failed {
                    return;
                }
            }
            RecorderCommand::Shutdown { reply } => {
                let _ = reply.send(writer.flush().await);
                return;
            }
        }
    }
}

async fn write_items<W>(writer: &mut W, items: Vec<RolloutItem>) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let mut bytes = Vec::new();
    for item in items {
        let item = serde_json::to_vec(&item).map_err(io::Error::other)?;
        bytes.extend_from_slice(&item);
        bytes.push(b'\n');
    }
    writer.write_all(&bytes).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use agent_protocol::{event::TurnStartedEvent, EventMsg};
    use serde_json::json;
    use tempfile::TempDir;
    use tokio::io::AsyncWrite;
    use tokio::sync::{mpsc, oneshot};

    use super::{run_writer_loop, RecorderCommand, RolloutRecorder};
    use crate::{read_rollout, RolloutItem, ThreadHistoryMode};

    struct FailAfter {
        remaining: usize,
    }

    impl AsyncWrite for FailAfter {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            if self.remaining == 0 {
                return Poll::Ready(Err(io::Error::other("simulated write failure")));
            }
            let written = self.remaining.min(buf.len());
            self.remaining -= written;
            Poll::Ready(Ok(written))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

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

    #[tokio::test]
    async fn write_failure_terminates_writer_and_drops_queued_replies() {
        let (tx, rx) = mpsc::unbounded_channel();
        let writer = tokio::spawn(run_writer_loop(FailAfter { remaining: 1 }, rx));
        let (first_reply, first_result) = oneshot::channel();
        let (second_reply, second_result) = oneshot::channel();
        let item = RolloutItem::SessionMeta(json!({"thread_id": "thread-1"}));

        tx.send(RecorderCommand::Record {
            items: vec![item.clone()],
            reply: first_reply,
        })
        .unwrap();
        tx.send(RecorderCommand::Record {
            items: vec![item],
            reply: second_reply,
        })
        .unwrap();

        assert!(first_result.await.unwrap().is_err());
        assert!(second_result.await.is_err());
        writer.await.unwrap();

        let (late_reply, _) = oneshot::channel();
        assert!(tx
            .send(RecorderCommand::Flush { reply: late_reply })
            .is_err());
    }

    #[tokio::test]
    async fn transient_events_are_filtered_before_writing() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("rollout.jsonl");
        let recorder = RolloutRecorder::open(path.clone(), ThreadHistoryMode::Paginated)
            .await
            .unwrap();

        recorder
            .record(vec![RolloutItem::EventMsg(EventMsg::Warning(
                agent_protocol::event::ErrorEvent {
                    message: "temporary".into(),
                    error_type: "notice".into(),
                },
            ))])
            .await
            .unwrap();
        recorder.flush().await.unwrap();

        assert!(read_rollout(&path).await.unwrap().is_empty());
        recorder.shutdown().await.unwrap();
    }
}
