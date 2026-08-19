use std::sync::Arc;

use agent_protocol::{Event, EventMsg, TurnCompleteEvent, TurnStartedEvent};
use proto::astro_service_server::AstroService;
use server::{
    run_listener_commands, ConnectionRegistry, ListenerCommand, ThreadActivity,
    ThreadHistoryBuilder, ThreadState,
};
use tokio::sync::{mpsc, oneshot, Mutex};

async fn start_listener() -> (ConnectionRegistry, mpsc::UnboundedSender<ListenerCommand>) {
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
        "thread-1".into(),
        state,
        command_rx,
        connections.clone(),
    ));
    (connections, commands)
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
