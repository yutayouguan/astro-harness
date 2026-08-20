use std::time::Duration;

use agent::exec::agent_control_directory::AgentControlDirectory;
use futures::{Stream, StreamExt};
use proto::astro_service_server::AstroService;
use proto::{
    session_event::Payload, ChatControlAction, ChatControlRequest, ChatRequest, SessionEvent,
    SubscribeSessionEventsRequest,
};
use server::grpc::AstroServiceImpl;
use subagents::{AgentPath, RunnerEvent};
use tempfile::tempdir;
use tonic::Request;

async fn attach_root(service: &AstroServiceImpl, root: &str) {
    let response = service
        .chat(Request::new(ChatRequest {
            session_id: root.into(),
            content: "attach watcher".into(),
            provider: "watcher-test-invalid-provider".into(),
            ..Default::default()
        }))
        .await
        .expect("chat creates the root session");
    drop(response);
}

async fn next_event<S>(stream: &mut S, label: &str) -> SessionEvent
where
    S: Stream<Item = Result<SessionEvent, tonic::Status>> + Unpin,
{
    tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .unwrap_or_else(|_| panic!("{label} timeout"))
        .expect("session event stream ended")
        .expect("session event status")
}

async fn next_thread_projection<S>(
    stream: &mut S,
    label: &str,
) -> (SessionEvent, proto::AgentThreadChangedEvent)
where
    S: Stream<Item = Result<SessionEvent, tonic::Status>> + Unpin,
{
    for _ in 0..8 {
        let event = next_event(stream, label).await;
        if let Some(Payload::AgentThreadChanged(projection)) = event.payload.clone() {
            return (event, projection);
        }
    }
    panic!("{label}: no AgentThreadChanged event");
}

async fn next_resync<S>(
    stream: &mut S,
    label: &str,
) -> (SessionEvent, proto::SessionResyncRequiredEvent)
where
    S: Stream<Item = Result<SessionEvent, tonic::Status>> + Unpin,
{
    for _ in 0..8 {
        let event = next_event(stream, label).await;
        if let Some(Payload::ResyncRequired(reset)) = event.payload.clone() {
            return (event, reset);
        }
    }
    panic!("{label}: no ResyncRequired event");
}

#[tokio::test]
async fn one_root_watcher_publishes_runner_status_changes_without_duplicates() {
    let memory = tempdir().unwrap();
    let service = AstroServiceImpl::new(memory.path().to_path_buf());
    let root = "root-watcher";
    let graph = memory.path().join("subagents-v2.db");

    attach_root(&service, root).await;
    attach_root(&service, root).await;

    let mut stream = service
        .subscribe_session_events(Request::new(SubscribeSessionEventsRequest {
            session_id: root.into(),
            agent_id: "default".into(),
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();
    let _ = next_resync(&mut stream, "initial watcher generation").await;
    let control = AgentControlDirectory::global()
        .open_root_at(root, &graph)
        .unwrap();
    let reservation = control
        .reserve_spawn_typed(&AgentPath::root(), "worker", "reviewer")
        .unwrap();
    let thread_id = reservation.thread_id().to_string();
    reservation.commit().unwrap();

    let (first_event, first) = next_thread_projection(&mut stream, "watcher event").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    control
        .record_runner_event(
            &thread_id,
            RunnerEvent::TurnStarted {
                turn_id: "turn-1".into(),
            },
        )
        .unwrap();

    let (second_event, second) = next_thread_projection(&mut stream, "watcher status event").await;
    assert_eq!(first_event.agent_id, "default");
    assert_eq!(second_event.agent_id, "default");
    let projections = [first, second];
    assert_eq!(projections.len(), 2);
    assert_eq!(projections[0].activity_kind, "spawned");
    assert_eq!(projections[1].activity_kind, "status_changed");
    assert_eq!(projections[1].canonical_path, "/root/worker");
    assert_eq!(projections[1].status_kind, "running");
    assert!(projections[0].activity_sequence < projections[1].activity_sequence);

    assert!(
        tokio::time::timeout(Duration::from_millis(150), stream.next())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn watcher_replays_activity_published_before_attach() {
    let memory = tempdir().unwrap();
    let service = AstroServiceImpl::new(memory.path().to_path_buf());
    let root = "root_attach_race";
    let graph = memory.path().join("subagents-v2.db");
    let mut stream = service
        .subscribe_session_events(Request::new(SubscribeSessionEventsRequest {
            session_id: root.into(),
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();
    let _ = next_resync(&mut stream, "initial empty generation").await;
    let control = AgentControlDirectory::global()
        .open_root_at(root, &graph)
        .unwrap();
    let reservation = control
        .reserve_spawn(&AgentPath::root(), "early_worker")
        .unwrap();
    reservation.commit().unwrap();

    attach_root(&service, root).await;

    let (_, projection) = next_thread_projection(&mut stream, "pre-attach activity replay").await;
    assert_eq!(projection.canonical_path, "/root/early_worker");
    assert_eq!(projection.activity_sequence, 1);
}

#[tokio::test]
async fn watcher_publishes_closed_thread_projection_from_edge_activity() {
    let memory = tempdir().unwrap();
    let service = AstroServiceImpl::new(memory.path().to_path_buf());
    let root = "root_closed_projection";
    let graph = memory.path().join("subagents-v2.db");
    attach_root(&service, root).await;
    let mut stream = service
        .subscribe_session_events(Request::new(SubscribeSessionEventsRequest {
            session_id: root.into(),
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();
    let _ = next_resync(&mut stream, "initial closed generation").await;
    let control = AgentControlDirectory::global()
        .open_root_at(root, &graph)
        .unwrap();
    let reservation = control
        .reserve_spawn(&AgentPath::root(), "closing_worker")
        .unwrap();
    let thread_id = reservation.thread_id().to_string();
    reservation.commit().unwrap();
    control
        .record_runner_event(
            &thread_id,
            RunnerEvent::TurnStarted {
                turn_id: "turn-close".into(),
            },
        )
        .unwrap();
    control
        .record_runner_event(&thread_id, RunnerEvent::RuntimeTerminated)
        .unwrap();

    let mut closed = None;
    for _ in 0..3 {
        let (_, projection) = next_thread_projection(&mut stream, "closed projection").await;
        if projection.activity_kind == "edge_closed" {
            closed = Some(projection);
        }
    }
    let closed = closed.expect("edge_closed projection");
    assert_eq!(closed.status_kind, "shutdown");
    assert_eq!(closed.canonical_path, "/root/closing_worker");
    assert_eq!(closed.status_payload_json, r#"{"kind":"shutdown"}"#);
}

#[tokio::test]
async fn watcher_skips_projectionless_activity_without_losing_the_next_projection() {
    let memory = tempdir().unwrap();
    let service = AstroServiceImpl::new(memory.path().to_path_buf());
    let root = "root-projectionless";
    let graph = memory.path().join("subagents-v2.db");
    attach_root(&service, root).await;

    let mut stream = service
        .subscribe_session_events(Request::new(SubscribeSessionEventsRequest {
            session_id: root.into(),
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();
    let _ = next_resync(&mut stream, "initial projectionless generation").await;
    let control = AgentControlDirectory::global()
        .open_root_at(root, &graph)
        .unwrap();
    control.notify_main_steer();
    let reservation = control
        .reserve_spawn(&AgentPath::root(), "after_steer")
        .unwrap();
    reservation.commit().unwrap();

    let (_, projection) = next_thread_projection(&mut stream, "projection after steer").await;
    assert_eq!(projection.canonical_path, "/root/after_steer");
    assert_eq!(projection.activity_kind, "spawned");
}

#[tokio::test]
async fn release_and_reopen_resyncs_same_stream_before_sequence_restarts() {
    let memory = tempdir().unwrap();
    let service = AstroServiceImpl::new(memory.path().to_path_buf());
    let root = "root-release-reopen";
    let graph = memory.path().join("subagents-v2.db");

    attach_root(&service, root).await;
    let mut stream = service
        .subscribe_session_events(Request::new(SubscribeSessionEventsRequest {
            session_id: root.into(),
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();
    let (initial_reset, _) = next_resync(&mut stream, "initial generation").await;
    let first_control = AgentControlDirectory::global()
        .get_at(root, &graph)
        .expect("first root control");
    let first = first_control
        .reserve_spawn(&AgentPath::root(), "first_worker")
        .unwrap();
    first.commit().unwrap();
    let (_, first_projection) = next_thread_projection(&mut stream, "first activity").await;
    assert_eq!(first_projection.activity_sequence, 1);

    drop(first_control);
    service
        .chat_control(Request::new(ChatControlRequest {
            session_id: root.into(),
            action: ChatControlAction::ReleaseSession as i32,
        }))
        .await
        .unwrap();
    attach_root(&service, root).await;

    let (reopen_reset, reset) = next_resync(&mut stream, "reopened generation").await;
    assert_eq!(reset.reason, "agent_control_generation_changed");
    assert_eq!(reopen_reset.stream_id, initial_reset.stream_id);
    assert!(reopen_reset.event_id > initial_reset.event_id);

    let reopened = AgentControlDirectory::global()
        .get_at(root, &graph)
        .expect("reopened root control");
    let snapshot = reopened.snapshot().unwrap();
    assert_eq!(snapshot.activity_sequence, 0);
    let second = reopened
        .reserve_spawn(&AgentPath::root(), "second_worker")
        .unwrap();
    second.commit().unwrap();
    let (second_event, second_projection) =
        next_thread_projection(&mut stream, "activity after reopen").await;
    assert_eq!(second_event.stream_id, reopen_reset.stream_id);
    assert_eq!(second_projection.activity_sequence, 1);
    assert!(second_projection.activity_sequence > snapshot.activity_sequence);
}

#[tokio::test]
async fn activity_retention_gap_resyncs_same_stream_and_snapshot_precedes_next_projection() {
    let memory = tempdir().unwrap();
    let service = AstroServiceImpl::new(memory.path().to_path_buf());
    let root = "root-activity-gap";
    let graph = memory.path().join("subagents-v2.db");
    let mut stream = service
        .subscribe_session_events(Request::new(SubscribeSessionEventsRequest {
            session_id: root.into(),
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();
    let _ = next_resync(&mut stream, "initial generation").await;

    let control = AgentControlDirectory::global()
        .open_root_at(root, &graph)
        .unwrap();
    for _ in 0..1025 {
        control.notify_main_steer();
    }
    attach_root(&service, root).await;

    let mut gap_reset = None;
    for _ in 0..4 {
        let (event, reset) = next_resync(&mut stream, "activity gap reset").await;
        if reset.reason == "activity_gap" {
            gap_reset = Some(event);
            break;
        }
    }
    let gap_reset = gap_reset.expect("explicit activity_gap reset");
    let snapshot = control.snapshot().unwrap();
    assert_eq!(snapshot.activity_sequence, 1025);

    let reservation = control
        .reserve_spawn(&AgentPath::root(), "after_gap")
        .unwrap();
    reservation.commit().unwrap();
    let (event, projection) = next_thread_projection(&mut stream, "projection after gap").await;
    assert_eq!(event.stream_id, gap_reset.stream_id);
    assert_eq!(projection.activity_sequence, 1026);
    assert!(projection.activity_sequence > snapshot.activity_sequence);
}
