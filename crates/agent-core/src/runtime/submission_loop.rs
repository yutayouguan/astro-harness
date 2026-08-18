use std::sync::Arc;

use agent_protocol::{Op, Submission};
use async_channel::Receiver;

use super::Session;

pub(crate) async fn submission_loop(_session: Arc<Session>, rx_sub: Receiver<Submission>) {
    while let Ok(submission) = rx_sub.recv().await {
        if matches!(submission.op, Op::Shutdown) {
            break;
        }
    }
}
