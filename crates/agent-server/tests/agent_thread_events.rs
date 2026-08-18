use std::time::Duration;

use agent::exec::agent_control_directory::AgentControlDirectory;
use futures::StreamExt;
use proto::astro_service_server::AstroService;
use proto::{ChatRequest, SubscribeSessionEventsRequest};
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
    let control = AgentControlDirectory::global()
        .open_root_at(root, &graph)
        .unwrap();
    let reservation = control
        .reserve_spawn_typed(&AgentPath::root(), "worker", "reviewer")
        .unwrap();
    let thread_id = reservation.thread_id().to_string();
    reservation.commit().unwrap();

    let first = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("watcher event timeout")
        .unwrap()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    control
        .record_runner_event(
            &thread_id,
            RunnerEvent::TurnStarted {
                turn_id: "turn-1".into(),
            },
        )
        .unwrap();

    let second = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("watcher status event timeout")
        .unwrap()
        .unwrap();
    assert_eq!(first.agent_id, "default");
    assert_eq!(second.agent_id, "default");
    let projections = [first, second]
        .into_iter()
        .map(|event| match event.payload.unwrap() {
            proto::session_event::Payload::AgentThreadChanged(event) => event,
            other => panic!("unexpected event: {other:?}"),
        })
        .collect::<Vec<_>>();
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
    let control = AgentControlDirectory::global()
        .open_root_at(root, &graph)
        .unwrap();
    let reservation = control
        .reserve_spawn(&AgentPath::root(), "early_worker")
        .unwrap();
    reservation.commit().unwrap();

    attach_root(&service, root).await;
    let mut stream = service
        .subscribe_session_events(Request::new(SubscribeSessionEventsRequest {
            session_id: root.into(),
            ..Default::default()
        }))
        .await
        .unwrap()
        .into_inner();

    let event = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("pre-attach activity replay timeout")
        .unwrap()
        .unwrap();
    let projection = match event.payload.unwrap() {
        proto::session_event::Payload::AgentThreadChanged(event) => event,
        other => panic!("unexpected event: {other:?}"),
    };
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
        let event = tokio::time::timeout(Duration::from_secs(2), stream.next())
            .await
            .expect("closed projection timeout")
            .unwrap()
            .unwrap();
        let projection = match event.payload.unwrap() {
            proto::session_event::Payload::AgentThreadChanged(event) => event,
            other => panic!("unexpected event: {other:?}"),
        };
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
    let control = AgentControlDirectory::global()
        .open_root_at(root, &graph)
        .unwrap();
    control.notify_main_steer();
    let reservation = control
        .reserve_spawn(&AgentPath::root(), "after_steer")
        .unwrap();
    reservation.commit().unwrap();

    let event = tokio::time::timeout(Duration::from_secs(2), stream.next())
        .await
        .expect("projection after steer timeout")
        .unwrap()
        .unwrap();
    let projection = match event.payload.unwrap() {
        proto::session_event::Payload::AgentThreadChanged(event) => event,
        other => panic!("unexpected event: {other:?}"),
    };
    assert_eq!(projection.canonical_path, "/root/after_steer");
    assert_eq!(projection.activity_kind, "spawned");
}
