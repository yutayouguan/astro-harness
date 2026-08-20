use std::{path::PathBuf, sync::Arc};

use agent::{AstroThread, Config, Session};
use agent_rollout::RolloutRecorder;
use tempfile::TempDir;

pub(crate) async fn new_thread() -> (
    TempDir,
    Arc<Session>,
    Arc<AstroThread>,
    RolloutRecorder,
    PathBuf,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rollout.jsonl");
    let recorder = RolloutRecorder::open(path.clone()).await.unwrap();
    let recorder_control = recorder.clone();
    let session = Arc::new(
        Session::with_session_id(
            Config::with_defaults(dir.path().to_path_buf()),
            "thread-event-test".into(),
        )
        .unwrap(),
    );
    let thread = AstroThread::spawn(Arc::clone(&session), recorder).unwrap();
    (dir, session, thread, recorder_control, path)
}

#[allow(dead_code)]
pub(crate) async fn collect_through_terminal(
    thread: &AstroThread,
    turn_id: &str,
) -> Vec<agent_protocol::Event> {
    let mut events = Vec::new();
    loop {
        let event = thread.next_event().await.unwrap();
        if event.id != turn_id {
            continue;
        }
        let terminal = event.msg.is_terminal();
        events.push(event);
        if terminal {
            return events;
        }
    }
}
