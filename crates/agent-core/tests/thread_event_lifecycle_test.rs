mod common;

use std::sync::Arc;

use agent::streaming::{run_multi_turn_stream_with_chat_fn, ChatOverride};
use agent::{AgentStatus, Config, Event, EventMsg, Op, Session};
use agent_protocol::{
    TurnInput, TurnInputMode, TurnInputRequest, TurnInputSubmission, TurnStartedEvent,
};
use agent_rollout::{read_rollout, RolloutItem, RolloutRecorder, ThreadHistoryMode};
use providers::{CompletionStream, PauseControl, ProviderConfig};
use serde_json::json;
use tokio::sync::Notify;

use common::new_thread;

async fn collect_next_terminal(thread: &agent::AstroThread) -> Vec<Event> {
    let mut events = Vec::new();
    loop {
        let event = thread.next_event().await.unwrap();
        let terminal = event.msg.is_terminal();
        events.push(event);
        if terminal {
            return events;
        }
    }
}

fn provider_error_chat() -> ChatOverride {
    Arc::new(|_messages, _tools, _config| {
        Box::pin(async { Err(anyhow::anyhow!("provider failed")) })
    })
}

#[tokio::test]
async fn durable_event_is_in_rollout_when_receiver_observes_it() {
    let (_dir, session, thread, _recorder, rollout_path) = new_thread().await;
    session
        .send_event(
            "turn-1",
            EventMsg::TurnStarted(TurnStartedEvent {
                turn_id: "turn-1".into(),
            }),
        )
        .await;
    let event = thread.next_event().await.unwrap();
    assert!(matches!(event.msg, EventMsg::TurnStarted(_)));
    assert_eq!(
        thread.status(),
        AgentStatus::Running {
            turn_id: "turn-1".into()
        }
    );
    let rollout = read_rollout(&rollout_path).await.unwrap();
    assert!(rollout.iter().any(|item| matches!(
        item,
        RolloutItem::EventMsg(EventMsg::TurnStarted(event)) if event.turn_id == "turn-1"
    )));
}

#[tokio::test]
async fn closed_rollout_writer_does_not_suppress_live_event() {
    let (_dir, session, thread, recorder, _path) = new_thread().await;
    recorder.shutdown().await.unwrap();
    session
        .send_event(
            "turn-1",
            EventMsg::TurnStarted(TurnStartedEvent {
                turn_id: "turn-1".into(),
            }),
        )
        .await;
    let event = thread.next_event().await.unwrap();
    assert!(matches!(event.msg, EventMsg::TurnStarted(_)));
}

#[tokio::test]
async fn shutdown_closes_rollout_before_live_only_completion() {
    let (_dir, _session, thread, recorder, rollout_path) = new_thread().await;
    let submission_id = thread.submit(Op::Shutdown).await.unwrap();

    let event = thread.next_event().await.unwrap();
    assert_eq!(event.id, submission_id);
    assert!(matches!(event.msg, EventMsg::ShutdownComplete));
    assert_eq!(thread.status(), AgentStatus::Shutdown);
    assert!(recorder
        .record(vec![RolloutItem::SessionMeta(json!({"late": true}))])
        .await
        .is_err());
    let rollout = read_rollout(&rollout_path).await.unwrap();
    assert!(!rollout
        .iter()
        .any(|item| matches!(item, RolloutItem::EventMsg(EventMsg::ShutdownComplete))));
    thread.wait_terminated().await;
}

#[tokio::test]
async fn rollout_shutdown_error_is_live_and_precedes_shutdown_complete() {
    let (_dir, _session, thread, recorder, _path) = new_thread().await;
    recorder.shutdown().await.unwrap();
    let submission_id = thread.submit(Op::Shutdown).await.unwrap();

    let error = thread.next_event().await.unwrap();
    assert_eq!(error.id, submission_id);
    assert!(matches!(
        error.msg,
        EventMsg::Error(ref event) if event.error_type == "rollout_shutdown"
    ));
    let complete = thread.next_event().await.unwrap();
    assert_eq!(complete.id, submission_id);
    assert!(matches!(complete.msg, EventMsg::ShutdownComplete));
    assert_eq!(thread.status(), AgentStatus::Shutdown);
    thread.wait_terminated().await;
}

#[tokio::test]
async fn provider_error_emits_one_error_and_complete_with_error() {
    let (_dir, session, thread, _recorder, _path) = new_thread().await;
    let (legacy_tx, _legacy_rx) = tokio::sync::mpsc::channel(8);
    let run = tokio::spawn(run_multi_turn_stream_with_chat_fn(
        Arc::clone(&session),
        provider_error_chat(),
        ProviderConfig::default(),
        "system".into(),
        PauseControl::new(),
        None,
        legacy_tx,
    ));

    let events = collect_next_terminal(&thread).await;
    run.await.unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.msg, EventMsg::Error(_)))
            .count(),
        1
    );
    assert!(matches!(
        events.last().unwrap().msg,
        EventMsg::TurnComplete(ref event) if event.error.is_some()
    ));
}

#[tokio::test]
async fn pause_control_cancel_emits_only_turn_aborted() {
    let (_dir, session, thread, _recorder, _path) = new_thread().await;
    let pause = PauseControl::new();
    let entered = Arc::new(Notify::new());
    let chat: ChatOverride = {
        let entered = Arc::clone(&entered);
        Arc::new(move |_messages, _tools, _config| {
            let entered = Arc::clone(&entered);
            Box::pin(async move {
                entered.notify_one();
                Ok(Box::pin(futures::stream::pending()) as CompletionStream)
            })
        })
    };
    let (legacy_tx, _legacy_rx) = tokio::sync::mpsc::channel(8);
    let run = tokio::spawn(run_multi_turn_stream_with_chat_fn(
        Arc::clone(&session),
        chat,
        ProviderConfig::default(),
        "system".into(),
        Arc::clone(&pause),
        None,
        legacy_tx,
    ));
    entered.notified().await;
    pause.cancel();

    let events = collect_next_terminal(&thread).await;
    run.await.unwrap();
    assert!(matches!(
        events.last().unwrap().msg,
        EventMsg::TurnAborted(_)
    ));
    assert_eq!(
        events
            .iter()
            .filter(|event| event.msg.is_terminal())
            .count(),
        1
    );
}

#[tokio::test]
async fn prepare_failure_emits_one_error_and_complete_with_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = Config::with_defaults(dir.path().to_path_buf());
    config.max_turns = 0;
    let rollout = RolloutRecorder::open(
        dir.path().join("prepare-failure.jsonl"),
        ThreadHistoryMode::Paginated,
    )
    .await
    .unwrap();
    let session = Arc::new(Session::with_session_id(config, "prepare-failure".into()).unwrap());
    let thread = agent::AstroThread::spawn(session, rollout).unwrap();
    let (_submission_id, submitted) = thread
        .submit_turn(
            TurnInputRequest {
                input: vec![TurnInput {
                    content: "over budget".into(),
                    image_data_urls: Vec::new(),
                }],
            },
            TurnInputMode::StartIfIdle,
        )
        .await
        .unwrap();
    assert!(matches!(submitted, TurnInputSubmission::Started { .. }));

    let events = collect_next_terminal(&thread).await;
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.msg, EventMsg::Error(_)))
            .count(),
        1
    );
    assert!(matches!(
        events.last().unwrap().msg,
        EventMsg::TurnComplete(ref event) if event.error.is_some()
    ));
}
