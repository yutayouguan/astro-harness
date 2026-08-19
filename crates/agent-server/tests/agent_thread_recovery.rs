use std::collections::BTreeMap;
use std::time::Duration;

use agent::exec::agent_control_directory::AgentControlDirectory;
use futures::{Stream, StreamExt};
use proto::astro_service_server::AstroService;
use proto::{session_event::Payload, ChatRequest, SessionEvent, SubscribeSessionEventsRequest};
use server::grpc::AstroServiceImpl;
use server::{
    AgentThreadChangedPayload, MemoryUpdatedPayload, SessionEventHub, SessionEventMsg,
    SubscribeFilter,
};
use subagents::{AgentPath, AgentStatusV2, AgentTreeSnapshotV2, RunnerEvent};
use tempfile::tempdir;
use tonic::Request;

type Projection = BTreeMap<String, String>;

async fn attach_root(service: &AstroServiceImpl, root: &str) {
    let response = service
        .chat(Request::new(ChatRequest {
            session_id: root.into(),
            content: "attach event watcher".into(),
            provider: "acceptance-invalid-provider".into(),
            ..Default::default()
        }))
        .await
        .expect("chat attaches the root watcher");
    drop(response);
}

async fn subscribe(
    service: &AstroServiceImpl,
    root: &str,
    stream_id: &str,
    after_event_id: u64,
) -> impl Stream<Item = Result<SessionEvent, tonic::Status>> + Unpin {
    service
        .subscribe_session_events(Request::new(SubscribeSessionEventsRequest {
            session_id: root.into(),
            agent_id: "default".into(),
            stream_id: stream_id.into(),
            after_event_id,
        }))
        .await
        .unwrap()
        .into_inner()
}

async fn next_event<S>(stream: &mut S, label: &str) -> SessionEvent
where
    S: Stream<Item = Result<SessionEvent, tonic::Status>> + Unpin,
{
    tokio::time::timeout(Duration::from_secs(3), stream.next())
        .await
        .unwrap_or_else(|_| panic!("{label}: timed out"))
        .expect("event stream ended")
        .expect("event status")
}

async fn next_projection<S>(
    stream: &mut S,
    label: &str,
) -> (SessionEvent, proto::AgentThreadChangedEvent)
where
    S: Stream<Item = Result<SessionEvent, tonic::Status>> + Unpin,
{
    for _ in 0..16 {
        let event = next_event(stream, label).await;
        if let Some(Payload::AgentThreadChanged(projection)) = event.payload.clone() {
            return (event, projection);
        }
    }
    panic!("{label}: no field-13 AgentThreadChanged payload");
}

async fn next_resync<S>(
    stream: &mut S,
    label: &str,
) -> (SessionEvent, proto::SessionResyncRequiredEvent)
where
    S: Stream<Item = Result<SessionEvent, tonic::Status>> + Unpin,
{
    for _ in 0..16 {
        let event = next_event(stream, label).await;
        if let Some(Payload::ResyncRequired(reset)) = event.payload.clone() {
            return (event, reset);
        }
    }
    panic!("{label}: no field-14 SessionResyncRequired payload");
}

fn projection_from_snapshot(snapshot: &AgentTreeSnapshotV2) -> Projection {
    snapshot
        .threads
        .iter()
        .map(|thread| {
            (
                thread.canonical_path.to_string(),
                serde_json::to_string(&thread.status).unwrap(),
            )
        })
        .collect()
}

fn apply(projection: &mut Projection, event: &proto::AgentThreadChangedEvent) {
    projection.insert(
        event.canonical_path.clone(),
        event.status_payload_json.clone(),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnect_replay_and_targeted_resync_converge_to_the_durable_tree() {
    let memory = tempdir().unwrap();
    let service = AstroServiceImpl::new(memory.path().to_path_buf());
    let root = "recovery-root";
    let graph = memory.path().join("subagents-v2.db");
    attach_root(&service, root).await;

    // Listener first, then snapshot. The field-14 marker supplies the event
    // watermark used as the reconnect cursor.
    let mut disconnected = subscribe(&service, root, "", 0).await;
    let (initial_reset, reset) = next_resync(&mut disconnected, "initial cursor").await;
    assert_eq!(reset.reason, "stream_generation_changed");
    let control = AgentControlDirectory::global()
        .get_at(root, &graph)
        .expect("attached root control");
    let initial_snapshot = control.snapshot().unwrap();
    let mut interrupted_projection = projection_from_snapshot(&initial_snapshot);
    let mut uninterrupted = subscribe(
        &service,
        root,
        &initial_reset.stream_id,
        initial_reset.event_id,
    )
    .await;

    let research = control
        .reserve_spawn_typed(&AgentPath::root(), "research", "researcher")
        .unwrap();
    let research_id = research.thread_id().to_string();
    research.commit().unwrap();
    control
        .record_runner_event(
            &research_id,
            RunnerEvent::TurnStarted {
                turn_id: "research-turn".into(),
            },
        )
        .unwrap();
    control
        .record_runner_event(
            &research_id,
            RunnerEvent::TurnCompleted {
                turn_id: "research-turn".into(),
                last_message: "sources".into(),
            },
        )
        .unwrap();

    let mut disconnect_cursor = initial_reset.event_id;
    for index in 0..3 {
        let (event, changed) = next_projection(&mut disconnected, "research projection").await;
        assert_eq!(changed.root_thread_id, root);
        assert_eq!(changed.thread_id, research_id);
        assert_eq!(changed.parent_thread_id, root);
        assert_eq!(changed.canonical_path, "/root/research");
        assert_eq!(changed.task_name, "research");
        assert_eq!(changed.agent_type, "researcher");
        assert!(!changed.session_id.is_empty());
        assert_eq!(changed.activity_sequence, (index + 1) as u64);
        assert!(matches!(
            changed.activity_kind.as_str(),
            "spawned" | "status_changed"
        ));
        apply(&mut interrupted_projection, &changed);
        disconnect_cursor = event.event_id;
    }
    let mut uninterrupted_projection = projection_from_snapshot(&initial_snapshot);
    for _ in 0..3 {
        let (_, changed) = next_projection(&mut uninterrupted, "uninterrupted research").await;
        apply(&mut uninterrupted_projection, &changed);
    }
    drop(disconnected);

    // All citation events occur while the first client is disconnected.
    let research_path = AgentPath::parse("/root/research").unwrap();
    let citations = control
        .reserve_spawn_typed(&research_path, "citations", "reviewer")
        .unwrap();
    let citations_id = citations.thread_id().to_string();
    citations.commit().unwrap();
    control
        .record_runner_event(
            &citations_id,
            RunnerEvent::TurnStarted {
                turn_id: "citations-turn".into(),
            },
        )
        .unwrap();
    control
        .record_runner_event(
            &citations_id,
            RunnerEvent::TurnCompleted {
                turn_id: "citations-turn".into(),
                last_message: "checked".into(),
            },
        )
        .unwrap();

    let mut reconnected =
        subscribe(&service, root, &initial_reset.stream_id, disconnect_cursor).await;
    let mut replay_ids = Vec::new();
    for _ in 0..3 {
        let (event, changed) = next_projection(&mut reconnected, "citation replay").await;
        assert_eq!(event.stream_id, initial_reset.stream_id);
        assert!(event.event_id > disconnect_cursor);
        replay_ids.push(event.event_id);
        apply(&mut interrupted_projection, &changed);
    }
    assert!(replay_ids.windows(2).all(|pair| pair[0] < pair[1]));
    for _ in 0..3 {
        let (_, changed) = next_projection(&mut uninterrupted, "live citation").await;
        apply(&mut uninterrupted_projection, &changed);
    }
    let durable = projection_from_snapshot(&control.snapshot().unwrap());
    assert_eq!(interrupted_projection, uninterrupted_projection);
    assert_eq!(interrupted_projection, durable);

    // Overflow field-13 history while disconnected. A same-stream field-14
    // marker requires a fresh snapshot instead of partial replay.
    let stale_cursor = *replay_ids.last().unwrap();
    drop(reconnected);
    for generation in 0..36 {
        let turn_id = format!("overflow-{generation}");
        control
            .record_runner_event(
                &citations_id,
                RunnerEvent::TurnStarted {
                    turn_id: turn_id.clone(),
                },
            )
            .unwrap();
        control
            .record_runner_event(
                &citations_id,
                RunnerEvent::TurnCompleted {
                    turn_id,
                    last_message: format!("overflow result {generation}"),
                },
            )
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut overflow = subscribe(&service, root, &initial_reset.stream_id, stale_cursor).await;
    let (overflow_reset, reset) = next_resync(&mut overflow, "history overflow").await;
    assert_eq!(overflow_reset.stream_id, initial_reset.stream_id);
    assert_eq!(reset.reason, "replay_gap");
    let refreshed = control.snapshot().unwrap();
    uninterrupted_projection.insert(
        "/root/research/citations".into(),
        serde_json::to_string(&AgentStatusV2::Completed {
            last_message: "overflow result 35".into(),
        })
        .unwrap(),
    );
    assert_eq!(
        projection_from_snapshot(&refreshed),
        uninterrupted_projection
    );

    // Another root's field-13 events must not leak through the recovery root
    // filter after the refreshed cursor.
    let mut filtered = subscribe(
        &service,
        root,
        &overflow_reset.stream_id,
        overflow_reset.event_id,
    )
    .await;
    let other = "other-root";
    attach_root(&service, other).await;
    let other_control = AgentControlDirectory::global()
        .get_at(other, &graph)
        .expect("other root control");
    other_control
        .reserve_spawn(&AgentPath::root(), "must_not_leak")
        .unwrap()
        .commit()
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(200), filtered.next())
            .await
            .is_err()
    );

    // An ActivityBus retention gap also uses field 14 in the same service
    // stream. A fresh snapshot followed by a new field-13 event converges.
    let gap_root = "gap-root";
    let mut gap_stream = subscribe(&service, gap_root, "", 0).await;
    let (gap_initial, _) = next_resync(&mut gap_stream, "gap initial cursor").await;
    let gap_control = AgentControlDirectory::global()
        .open_root_at(gap_root, &graph)
        .unwrap();
    for _ in 0..1025 {
        gap_control.notify_main_steer();
    }
    attach_root(&service, gap_root).await;
    let (gap_reset, _) = loop {
        let next = next_resync(&mut gap_stream, "activity gap").await;
        if next.1.reason == "activity_gap" {
            break next;
        }
    };
    assert_eq!(gap_reset.stream_id, gap_initial.stream_id);
    let gap_snapshot = gap_control.snapshot().unwrap();
    assert_eq!(gap_snapshot.activity_sequence, 1025);
    gap_control
        .reserve_spawn(&AgentPath::root(), "after_gap")
        .unwrap()
        .commit()
        .unwrap();
    let (_, after_gap) = next_projection(&mut gap_stream, "projection after fresh snapshot").await;
    assert_eq!(after_gap.activity_sequence, 1026);
    let mut gap_projection = projection_from_snapshot(&gap_snapshot);
    apply(&mut gap_projection, &after_gap);
    assert_eq!(
        gap_projection,
        projection_from_snapshot(&gap_control.snapshot().unwrap())
    );
}

#[tokio::test]
async fn root_filter_rejects_other_root_memory_events() {
    let hub = SessionEventHub::new(8);
    let mut receiver = hub.subscribe(
        SubscribeFilter {
            session_id: Some("root-a".into()),
            agent_id: Some("default".into()),
        },
        &hub.stream_id(),
        0,
    );
    hub.publish(SessionEventMsg {
        session_id: Some("root-b".into()),
        agent_id: "default".into(),
        memory_updated: Some(MemoryUpdatedPayload {
            source: "review".into(),
            target: "memory".into(),
            summary: "other root".into(),
            live_written: true,
        }),
        pending_changed: None,
        session_metadata_changed: None,
        agent_thread_changed: None,
        resync_required: None,
    });
    hub.publish(SessionEventMsg {
        session_id: Some("root-b".into()),
        agent_id: "default".into(),
        memory_updated: None,
        pending_changed: None,
        session_metadata_changed: None,
        agent_thread_changed: Some(AgentThreadChangedPayload {
            activity_sequence: 1,
            root_thread_id: "root-b".into(),
            thread_id: "thread-b".into(),
            parent_thread_id: "root-b".into(),
            canonical_path: "/root/worker".into(),
            task_name: "worker".into(),
            agent_type: "default".into(),
            session_id: "thread-b".into(),
            status_kind: "running".into(),
            status_payload_json: r#"{"kind":"running"}"#.into(),
            activity_kind: "spawned".into(),
        }),
        resync_required: None,
    });
    assert!(
        tokio::time::timeout(Duration::from_millis(100), receiver.recv())
            .await
            .is_err()
    );
}
