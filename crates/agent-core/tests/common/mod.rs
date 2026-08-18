use std::{path::PathBuf, sync::Arc};

use agent::{AstroThread, Config, Session};
use agent_rollout::{RolloutRecorder, ThreadHistoryMode};
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
    let recorder = RolloutRecorder::open(path.clone(), ThreadHistoryMode::Paginated)
        .await
        .unwrap();
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
