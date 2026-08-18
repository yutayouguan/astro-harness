mod common;

use agent::{AgentStatus, EventMsg, Op};
use agent_protocol::TurnStartedEvent;
use agent_rollout::{read_rollout, RolloutItem};
use serde_json::json;

use common::new_thread;

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
