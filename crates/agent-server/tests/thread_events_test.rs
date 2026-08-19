use std::sync::Arc;

use agent_protocol::{
    Event, EventMsg, ExtensionItem, ItemEvent, TurnCompleteEvent, TurnItem, TurnStartedEvent,
};
use futures::StreamExt;
use proto::astro_service_server::AstroService;
use server::{
    run_listener_commands, ConnectionRegistry, ListenerCommand, ThreadActivity,
    ThreadHistoryBuilder, ThreadState, WORKSPACE_EVENT_THREAD_ID,
};
use tokio::sync::{mpsc, oneshot, Mutex};

async fn start_listener() -> (ConnectionRegistry, mpsc::UnboundedSender<ListenerCommand>) {
    start_listener_for("thread-1").await
}

async fn start_listener_for(
    thread_id: &str,
) -> (ConnectionRegistry, mpsc::UnboundedSender<ListenerCommand>) {
    let connections = ConnectionRegistry::default();
    let (commands, command_rx) = mpsc::unbounded_channel();
    let (activity_tx, _activity_rx) = tokio::sync::watch::channel(ThreadActivity {
        status: "idle".into(),
        has_subscribers: false,
    });
    let state = Arc::new(Mutex::new(ThreadState {
        status: "idle".into(),
        history: ThreadHistoryBuilder::default(),
        subscribers: Default::default(),
        listener_command_tx: commands.clone(),
        activity_tx,
    }));
    tokio::spawn(run_listener_commands(
        thread_id.into(),
        state,
        command_rx,
        connections.clone(),
    ));
    (connections, commands)
}

async fn subscribe_and_resume(
    service: &server::grpc::AstroServiceImpl,
    connection_id: &str,
    thread_id: &str,
) -> std::pin::Pin<Box<dyn futures::Stream<Item = Result<proto::ThreadEvent, tonic::Status>> + Send>>
{
    let stream = AstroService::subscribe_thread_events(
        service,
        tonic::Request::new(proto::SubscribeThreadEventsRequest {
            connection_id: connection_id.into(),
        }),
    )
    .await
    .expect("subscribe")
    .into_inner();
    AstroService::resume_thread(
        service,
        tonic::Request::new(proto::ResumeThreadRequest {
            connection_id: connection_id.into(),
            thread_id: thread_id.into(),
            include_turns: true,
        }),
    )
    .await
    .expect("resume");
    stream
}

async fn recv_extension(
    stream: &mut std::pin::Pin<
        Box<dyn futures::Stream<Item = Result<proto::ThreadEvent, tonic::Status>> + Send>,
    >,
) -> proto::ThreadExtension {
    loop {
        let event = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
            .await
            .expect("thread extension timeout")
            .expect("thread event stream")
            .expect("thread event");
        if let Some(proto::thread_event::Payload::Extension(extension)) = event.payload {
            return extension;
        }
    }
}

async fn assert_rollout_contains_extension(
    memory_dir: &std::path::Path,
    thread_id: &str,
    namespace: &str,
) {
    let rollout_root = memory_dir.join("sessions").join("rollouts");
    let path = agent_rollout::find_rollout(&rollout_root, thread_id)
        .expect("find rollout")
        .expect("rollout path");
    let items = agent_rollout::read_rollout(&path)
        .await
        .expect("read rollout");
    assert!(items.into_iter().any(|item| matches!(
        item,
        agent_rollout::RolloutItem::EventMsg(EventMsg::ItemCompleted(agent_protocol::ItemEvent {
            item: agent_protocol::TurnItem::Extension(extension),
            ..
        })) if extension.namespace == namespace
    )));
}

async fn resume(
    connections: &ConnectionRegistry,
    commands: &mpsc::UnboundedSender<ListenerCommand>,
    connection_id: &str,
    include_turns: bool,
) -> server::ThreadSnapshot {
    let (reply, recv) = oneshot::channel();
    let subscription = connections
        .current_generation_key(connection_id)
        .await
        .expect("connection generation");
    commands
        .send(ListenerCommand::Resume {
            subscription,
            include_turns,
            reply,
        })
        .expect("listener should accept resume");
    recv.await.expect("listener should reply")
}

#[tokio::test]
async fn two_connections_receive_the_same_thread_event_order() {
    let (connections, commands) = start_listener().await;
    let (mut first, _, _) = connections.register("first".into()).await;
    let (mut second, _, _) = connections.register("second".into()).await;
    resume(&connections, &commands, "first", false).await;
    resume(&connections, &commands, "second", false).await;
    for msg in [
        EventMsg::TurnStarted(TurnStartedEvent {
            turn_id: "turn-1".into(),
        }),
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-1".into(),
            last_agent_message: Some("hello".into()),
            error: None,
        }),
    ] {
        commands
            .send(ListenerCommand::CoreEvent(Event {
                id: "turn-1".into(),
                msg,
            }))
            .expect("listener should accept event");
    }
    for _ in 0..2 {
        assert_eq!(first.recv().await, second.recv().await);
    }
}

#[tokio::test]
async fn running_resume_has_no_snapshot_to_live_gap() {
    let (connections, commands) = start_listener().await;
    let (mut connection, _, _) = connections.register("resume".into()).await;
    commands
        .send(ListenerCommand::CoreEvent(Event {
            id: "turn-1".into(),
            msg: EventMsg::TurnStarted(TurnStartedEvent {
                turn_id: "turn-1".into(),
            }),
        }))
        .expect("listener should accept start");
    let snapshot = resume(&connections, &commands, "resume", true).await;
    assert_eq!(snapshot.active_turn.expect("active turn").id, "turn-1");
    commands
        .send(ListenerCommand::CoreEvent(Event {
            id: "turn-1".into(),
            msg: EventMsg::TurnComplete(TurnCompleteEvent {
                turn_id: "turn-1".into(),
                last_agent_message: Some("done".into()),
                error: None,
            }),
        }))
        .expect("listener should accept completion");
    let live = connection.recv().await.expect("completion should be live");
    assert!(matches!(
        live.payload,
        Some(proto::thread_event::Payload::TurnComplete(_))
    ));
}

#[tokio::test]
async fn background_review_extension_preserves_memory_namespace_and_payload() {
    let (connections, commands) = start_listener().await;
    let (mut connection, _, _) = connections.register("desktop".into()).await;
    resume(&connections, &commands, "desktop", false).await;
    commands
        .send(ListenerCommand::CoreEvent(Event {
            id: "background-review".into(),
            msg: EventMsg::ItemCompleted(ItemEvent {
                turn_id: "background-review".into(),
                item: TurnItem::Extension(ExtensionItem {
                    id: "memory-1".into(),
                    namespace: "astro.memory".into(),
                    payload: serde_json::json!({
                        "source":"review",
                        "target":"memory",
                        "summary":"memory updated",
                        "live_written":true
                    }),
                }),
            }),
        }))
        .expect("listener should accept extension");
    let event = connection.recv().await.expect("memory extension");
    let Some(proto::thread_event::Payload::Extension(extension)) = event.payload else {
        panic!("expected extension");
    };
    let payload: serde_json::Value =
        serde_json::from_str(&extension.payload_json).expect("memory payload");
    assert_eq!(extension.namespace, "astro.memory");
    assert_eq!(payload["summary"], "memory updated");
}

#[tokio::test]
async fn workspace_pending_extension_uses_workspace_thread_namespace_and_payload() {
    let (connections, commands) = start_listener_for(WORKSPACE_EVENT_THREAD_ID).await;
    let (mut connection, _, _) = connections.register("desktop".into()).await;
    resume(&connections, &commands, "desktop", false).await;
    commands
        .send(ListenerCommand::CoreEvent(Event {
            id: "pending-1".into(),
            msg: EventMsg::ItemCompleted(ItemEvent {
                turn_id: "pending-1".into(),
                item: TurnItem::Extension(ExtensionItem {
                    id: "pending-1".into(),
                    namespace: "astro.pending".into(),
                    payload: serde_json::json!({"pending_count":2,"reason":"enqueued"}),
                }),
            }),
        }))
        .expect("listener should accept extension");
    let event = connection.recv().await.expect("pending extension");
    assert_eq!(event.thread_id, WORKSPACE_EVENT_THREAD_ID);
    let Some(proto::thread_event::Payload::Extension(extension)) = event.payload else {
        panic!("expected extension");
    };
    let payload: serde_json::Value =
        serde_json::from_str(&extension.payload_json).expect("pending payload");
    assert_eq!(extension.namespace, "astro.pending");
    assert_eq!(payload["pending_count"], 2);
}

#[tokio::test]
async fn thread_rpcs_subscribe_resume_unsubscribe_and_validate_submit_connection() {
    let dir = tempfile::tempdir().expect("tempdir");
    memory::ensure_workspace(dir.path()).expect("workspace");
    let service = server::grpc::AstroServiceImpl::new(dir.path().to_path_buf());
    let stream = AstroService::subscribe_thread_events(
        &service,
        tonic::Request::new(proto::SubscribeThreadEventsRequest {
            connection_id: "connection-1".into(),
        }),
    )
    .await
    .expect("subscribe")
    .into_inner();

    let resumed = AstroService::resume_thread(
        &service,
        tonic::Request::new(proto::ResumeThreadRequest {
            connection_id: "connection-1".into(),
            thread_id: "thread-rpc".into(),
            include_turns: true,
        }),
    )
    .await
    .expect("resume")
    .into_inner()
    .thread
    .expect("snapshot");
    assert_eq!(resumed.thread_id, "thread-rpc");
    assert_eq!(resumed.status, "idle");

    AstroService::unsubscribe_thread(
        &service,
        tonic::Request::new(proto::UnsubscribeThreadRequest {
            connection_id: "connection-1".into(),
            thread_id: "thread-rpc".into(),
        }),
    )
    .await
    .expect("unsubscribe");

    let submit_error = AstroService::submit_turn(
        &service,
        tonic::Request::new(proto::SubmitTurnRequest {
            connection_id: "missing".into(),
            chat: Some(proto::ChatRequest {
                session_id: "thread-rpc".into(),
                content: "hello".into(),
                use_memory: true,
                ..Default::default()
            }),
            mode: "start_if_idle".into(),
            expected_turn_id: String::new(),
        }),
    )
    .await
    .expect_err("unknown connection must be rejected");
    assert_eq!(submit_error.code(), tonic::Code::FailedPrecondition);
    drop(stream);
}

#[tokio::test]
async fn background_review_emitter_uses_durable_memory_extension() {
    let dir = tempfile::tempdir().expect("tempdir");
    memory::ensure_workspace(dir.path()).expect("workspace");
    let service = server::grpc::AstroServiceImpl::new(dir.path().to_path_buf());
    let mut stream = subscribe_and_resume(&service, "desktop-review", "review-thread").await;

    service
        .emit_background_review_extension("review-thread", "记忆已更新（1）· durable review", true)
        .await
        .expect("background review extension");

    let extension = recv_extension(&mut stream).await;
    assert_eq!(extension.namespace, "astro.memory");
    let payload: serde_json::Value =
        serde_json::from_str(&extension.payload_json).expect("memory payload");
    assert_eq!(payload["source"], "review");
    assert_eq!(payload["target"], "memory");
    assert_eq!(payload["summary"], "记忆已更新（1）· durable review");
    assert_eq!(payload["live_written"], true);
    assert_rollout_contains_extension(dir.path(), "review-thread", "astro.memory").await;
}

#[tokio::test]
async fn title_emitter_uses_durable_session_metadata_extension() {
    let dir = tempfile::tempdir().expect("tempdir");
    memory::ensure_workspace(dir.path()).expect("workspace");
    let service = server::grpc::AstroServiceImpl::new(dir.path().to_path_buf());
    let mut stream = subscribe_and_resume(&service, "desktop-title", "title-thread").await;

    service
        .emit_session_metadata_extension("title-thread", "Durable title")
        .await
        .expect("title extension");

    let extension = recv_extension(&mut stream).await;
    assert_eq!(extension.namespace, "astro.session_metadata");
    let payload: serde_json::Value =
        serde_json::from_str(&extension.payload_json).expect("title payload");
    assert_eq!(payload, serde_json::json!({"title":"Durable title"}));
    assert_rollout_contains_extension(dir.path(), "title-thread", "astro.session_metadata").await;
}

#[tokio::test]
async fn global_pending_emitter_uses_durable_workspace_extension() {
    let dir = tempfile::tempdir().expect("tempdir");
    memory::ensure_workspace(dir.path()).expect("workspace");
    let service = server::grpc::AstroServiceImpl::new(dir.path().to_path_buf());
    let mut stream =
        subscribe_and_resume(&service, "desktop-pending", WORKSPACE_EVENT_THREAD_ID).await;

    service
        .emit_pending_extension(2, "enqueued")
        .await
        .expect("pending extension");

    let extension = recv_extension(&mut stream).await;
    assert_eq!(extension.namespace, "astro.pending");
    let payload: serde_json::Value =
        serde_json::from_str(&extension.payload_json).expect("pending payload");
    assert_eq!(
        payload,
        serde_json::json!({"pending_count":2,"reason":"enqueued"})
    );
    assert_rollout_contains_extension(dir.path(), WORKSPACE_EVENT_THREAD_ID, "astro.pending").await;
}
