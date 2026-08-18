use async_channel::{Receiver, Sender};
use tokio::sync::watch;
use uuid::Uuid;

use agent_protocol::{Event, Op, Submission};

pub(crate) const SUBMISSION_CHANNEL_CAPACITY: usize = 512;

pub(crate) fn submission_channel() -> (Sender<Submission>, Receiver<Submission>) {
    async_channel::bounded(SUBMISSION_CHANNEL_CAPACITY)
}

pub(crate) fn event_channel() -> (Sender<Event>, Receiver<Event>) {
    async_channel::unbounded()
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // Task 6 owns the first status transitions.
pub enum AgentStatus {
    Idle,
    Running { turn_id: String },
    Errored(String),
    Shutdown,
}

pub struct SessionIo {
    pub(crate) tx_sub: Sender<Submission>,
    pub(crate) rx_event: Receiver<Event>,
    #[allow(dead_code)] // Exposed once production callers migrate in a later task.
    pub(crate) status_rx: watch::Receiver<AgentStatus>,
    #[allow(dead_code)] // Exposed once production callers migrate in a later task.
    pub(crate) termination_rx: watch::Receiver<bool>,
}

impl SessionIo {
    pub(crate) fn new() -> (
        Self,
        Receiver<Submission>,
        Sender<Event>,
        watch::Sender<AgentStatus>,
        watch::Sender<bool>,
    ) {
        let (tx_sub, rx_sub) = submission_channel();
        let (event_tx, rx_event) = event_channel();
        let (status_tx, status_rx) = watch::channel(AgentStatus::Idle);
        let (termination_tx, termination_rx) = watch::channel(false);
        (
            Self {
                tx_sub,
                rx_event,
                status_rx,
                termination_rx,
            },
            rx_sub,
            event_tx,
            status_tx,
            termination_tx,
        )
    }

    pub async fn submit(&self, op: Op) -> Result<String, async_channel::SendError<Submission>> {
        let id = Uuid::new_v4().to_string();
        self.tx_sub.send(Submission { id: id.clone(), op }).await?;
        Ok(id)
    }

    pub async fn next_event(&self) -> Result<Event, async_channel::RecvError> {
        self.rx_event.recv().await
    }

    #[allow(dead_code)] // The stable thread handle is introduced before callers migrate.
    pub fn status(&self) -> AgentStatus {
        self.status_rx.borrow().clone()
    }

    #[allow(dead_code)] // The stable thread handle is introduced before callers migrate.
    pub fn subscribe_status(&self) -> watch::Receiver<AgentStatus> {
        self.status_rx.clone()
    }

    #[allow(dead_code)] // The stable thread handle is introduced before callers migrate.
    pub async fn wait_terminated(&self) {
        let mut termination_rx = self.termination_rx.clone();
        loop {
            if *termination_rx.borrow() {
                return;
            }
            if termination_rx.changed().await.is_err() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use agent_protocol::{event::TurnStartedEvent, Event, EventMsg, Op, Submission};

    use super::{event_channel, submission_channel, SUBMISSION_CHANNEL_CAPACITY};

    #[test]
    fn submission_channel_is_bounded_at_512() {
        let (tx, _rx) = submission_channel();
        for index in 0..SUBMISSION_CHANNEL_CAPACITY {
            tx.try_send(Submission {
                id: index.to_string(),
                op: Op::Interrupt,
            })
            .unwrap();
        }

        assert!(matches!(
            tx.try_send(Submission {
                id: "overflow".into(),
                op: Op::Interrupt,
            }),
            Err(async_channel::TrySendError::Full(_))
        ));
    }

    #[tokio::test]
    async fn event_receiver_preserves_send_order() {
        let (tx, rx) = event_channel();
        for id in ["one", "two"] {
            tx.send(Event {
                id: id.into(),
                msg: EventMsg::TurnStarted(TurnStartedEvent { turn_id: id.into() }),
            })
            .await
            .unwrap();
        }

        assert_eq!(rx.recv().await.unwrap().id, "one");
        assert_eq!(rx.recv().await.unwrap().id, "two");
    }
}
