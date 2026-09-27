//! `astro_service.rs` 的单元测试（原内联 mod tests 拆出）。

use super::*;
use crate::ListenerCommand;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use agent::streaming::{run_multi_turn_events_with_responses_fn, ResponsesOverride};
use providers::CompletionStream;
use tempfile::TempDir;

/// 测试等待预算：轮询 / 通道等待的上限。条件满足即返回，所以给足余量
/// 不会拖慢正常路径，只在并发争用/慢机器上避免误判失败。
const WAIT_BUDGET: Duration = Duration::from_secs(15);

async fn wait_for_agent_thread_extensions(
    managed: &Arc<ManagedThread>,
    memory_dir: &std::path::Path,
    root_thread_id: &str,
    ready: impl Fn(&[agent_protocol::ExtensionItem]) -> bool,
) -> Vec<agent_protocol::ExtensionItem> {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            managed.runtime.flush_rollout().await.unwrap();
            let rollout_root = memory_dir.join("sessions").join("rollouts");
            let extensions = match agent_rollout::find_rollout(&rollout_root, root_thread_id)
                .unwrap()
            {
                Some(path) => agent_rollout::read_rollout(&path)
                    .await
                    .unwrap()
                    .into_iter()
                    .filter_map(|item| match item {
                        agent_rollout::RolloutItem::EventMsg(
                            agent_protocol::EventMsg::ItemCompleted(agent_protocol::ItemEvent {
                                item: agent_protocol::TurnItem::Extension(extension),
                                ..
                            }),
                        ) if extension.namespace == "astro.agent_thread"
                            || extension.namespace == "astro.agent_thread_resync" =>
                        {
                            Some(extension)
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                None => Vec::new(),
            };
            if ready(&extensions) {
                return extensions;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("agent thread extensions were not materialized")
}

#[test]
fn only_successful_run_allows_post_turn_side_effects() {
    assert!(allows_post_turn_side_effects("success"));
    for outcome in ["error", "interrupt", "hitl_waiting", ""] {
        assert!(!allows_post_turn_side_effects(outcome), "outcome={outcome}");
    }
}

#[tokio::test]
async fn agent_thread_projection_carries_unified_stream_generation() {
    let dir = TempDir::new().unwrap();
    let control = subagents::AgentControl::open(
        "projection-root".into(),
        subagents::AgentGraphStore::open(dir.path().join("graph.db"))
            .await
            .unwrap(),
        subagents::Limits {
            max_threads: 8,
            max_depth: 4,
            max_running: 4,
        },
    )
    .await
    .unwrap();
    control
        .reserve_spawn(&subagents::AgentPath::root(), "child")
        .await
        .unwrap()
        .commit()
        .await
        .unwrap();
    let observation = control
        .next_activity_after(
            subagents::ActivityCursor(0),
            std::time::Duration::from_secs(1),
        )
        .await;
    let subagents::ActivityObservation::Activity(activity) = observation else {
        panic!("spawn activity missing");
    };
    let projection = agent_thread_projection(*activity, "generation-2").unwrap();
    assert_eq!(projection["stream_id"], "generation-2");
    assert_eq!(projection["activity_sequence"], 1);
    assert_eq!(projection["canonical_path"], "/root/child");
    assert_eq!(projection["activity_kind"], "spawned");
}

#[tokio::test]
async fn agent_thread_watcher_replays_then_skips_projectionless_activity() {
    let dir = TempDir::new().unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    let root = "agent-thread-replay-root";
    let managed = service.get_or_create_thread(root).await.unwrap();
    let control = Arc::new(
        subagents::AgentControl::open(
            root.into(),
            subagents::AgentGraphStore::open(dir.path().join("agent-graph.db"))
                .await
                .unwrap(),
            subagents::Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 4,
            },
        )
        .await
        .unwrap(),
    );
    let spawn = control
        .reserve_spawn(&subagents::AgentPath::root(), "child")
        .await
        .unwrap();
    let child_thread_id = spawn.thread_id().to_string();
    spawn.commit().await.unwrap();

    service
        .attach_agent_thread_watcher(root, "default", Arc::clone(&control), Arc::clone(&managed))
        .await;
    control.notify_main_steer();
    control
        .record_runner_event(&child_thread_id, subagents::RunnerEvent::RuntimeTerminated)
        .await
        .unwrap();

    let extensions = wait_for_agent_thread_extensions(&managed, dir.path(), root, |items| {
        let Some(stream_id) = items
            .iter()
            .find(|item| item.namespace == "astro.agent_thread")
            .map(|item| &item.payload["stream_id"])
        else {
            return false;
        };
        items
            .iter()
            .filter(|item| {
                item.namespace == "astro.agent_thread" && item.payload["stream_id"] == *stream_id
            })
            .count()
            >= 2
            && items.iter().any(|item| {
                item.namespace == "astro.agent_thread_resync"
                    && item.payload["stream_id"] == *stream_id
            })
    })
    .await;
    let projected = extensions
        .iter()
        .filter(|extension| extension.namespace == "astro.agent_thread")
        .collect::<Vec<_>>();
    assert_eq!(projected.len(), 2, "MainSteer must not create a projection");
    let stream_id = &projected[0].payload["stream_id"];
    let resync = extensions
        .iter()
        .find(|extension| {
            extension.namespace == "astro.agent_thread_resync"
                && extension.payload["stream_id"] == *stream_id
        })
        .unwrap();
    assert_eq!(projected[0].payload["activity_sequence"], 1);
    assert_eq!(projected[0].payload["activity_kind"], "spawned");
    assert_eq!(projected[1].payload["activity_sequence"], 3);
    assert_eq!(projected[1].payload["activity_kind"], "edge_closed");
    assert_eq!(
        projected[0].payload["stream_id"], resync.payload["stream_id"],
        "replayed and live activity must share the watcher generation"
    );
    assert_eq!(
        projected[1].payload["stream_id"],
        resync.payload["stream_id"]
    );
}

#[tokio::test]
async fn agent_thread_watcher_resyncs_retention_gap_before_next_projection() {
    let dir = TempDir::new().unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    let root = "agent-thread-gap-root";
    let managed = service.get_or_create_thread(root).await.unwrap();
    let control = Arc::new(
        subagents::AgentControl::open(
            root.into(),
            subagents::AgentGraphStore::open(dir.path().join("agent-gap-graph.db"))
                .await
                .unwrap(),
            subagents::Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 4,
            },
        )
        .await
        .unwrap(),
    );
    for _ in 0..1_025 {
        control.notify_main_steer();
    }

    service
        .attach_agent_thread_watcher(root, "default", Arc::clone(&control), Arc::clone(&managed))
        .await;
    let gap_extensions = wait_for_agent_thread_extensions(&managed, dir.path(), root, |items| {
        items.iter().any(|item| {
            item.namespace == "astro.agent_thread_resync"
                && item.payload["reason"] == "activity_gap"
        })
    })
    .await;
    let stream_id = gap_extensions
        .iter()
        .find(|item| {
            item.namespace == "astro.agent_thread_resync"
                && item.payload["reason"] == "activity_gap"
        })
        .unwrap()
        .payload["stream_id"]
        .clone();
    control
        .reserve_spawn(&subagents::AgentPath::root(), "after_gap")
        .await
        .unwrap()
        .commit()
        .await
        .unwrap();

    let extensions = wait_for_agent_thread_extensions(&managed, dir.path(), root, |items| {
        ["agent_control_generation_changed", "activity_gap"]
            .into_iter()
            .all(|reason| {
                items.iter().any(|item| {
                    item.namespace == "astro.agent_thread_resync"
                        && item.payload["stream_id"] == stream_id
                        && item.payload["reason"] == reason
                })
            })
            && items.iter().any(|item| {
                item.namespace == "astro.agent_thread" && item.payload["stream_id"] == stream_id
            })
    })
    .await;
    let projection = extensions
        .iter()
        .find(|extension| extension.namespace == "astro.agent_thread")
        .unwrap();
    let stream_id = &projection.payload["stream_id"];
    let resyncs = extensions
        .iter()
        .filter(|extension| {
            extension.namespace == "astro.agent_thread_resync"
                && extension.payload["stream_id"] == *stream_id
        })
        .collect::<Vec<_>>();
    assert_eq!(resyncs.len(), 2);
    assert_eq!(
        resyncs[0].payload["reason"],
        "agent_control_generation_changed"
    );
    assert_eq!(resyncs[1].payload["reason"], "activity_gap");
    assert_eq!(
        resyncs[0].payload["stream_id"],
        resyncs[1].payload["stream_id"]
    );

    assert_eq!(projection.payload["canonical_path"], "/root/after_gap");
    assert_eq!(projection.payload["activity_sequence"], 1_026);
    assert_eq!(
        projection.payload["stream_id"],
        resyncs[0].payload["stream_id"]
    );
}

#[tokio::test]
async fn release_session_runtime_is_idempotent() {
    let dir = TempDir::new().unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    let session_id = "release-session-runtime-idempotent";

    let session = service.get_session(session_id).await.unwrap();
    let gate = HitlGate::new(session_id.to_string());
    let (ui_tx, _ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let pause = service
        .admit_pause_generation(session_id, &session, Arc::clone(&gate), ui_tx)
        .await
        .unwrap();

    let request = Request::new(ChatControlRequest {
        session_id: session_id.to_string(),
        action: ChatControlAction::ReleaseSession as i32,
    });
    service.chat_control(request).await.expect("first release");
    drop(pause);
    drop(gate);

    let request = Request::new(ChatControlRequest {
        session_id: session_id.to_string(),
        action: ChatControlAction::ReleaseSession as i32,
    });
    service.chat_control(request).await.expect("second release");

    assert!(service.sessions.read().await.get(session_id).is_none());
    assert!(
        agent::exec::dispatch::active_root_runtime_material(dir.path(), session_id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(service
        .pause_controls
        .read()
        .expect("pause registry lock poisoned")
        .get(session_id)
        .is_none());
    assert!(service.hitl_registry.get(session_id).await.is_none());
    assert!(!service
        .generation_operations
        .lock()
        .unwrap()
        .contains_key(session_id));
}

#[tokio::test]
async fn cancel_with_stale_pause_does_not_recreate_released_session() {
    let dir = TempDir::new().unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    let session_id = "released-session-with-stale-pause";
    service
        .pause_controls
        .write()
        .expect("pause registry lock poisoned")
        .insert(
            session_id.into(),
            PauseRegistration {
                control: PauseControl::new(),
                session: Weak::new(),
                hitl_gate: HitlGate::new(session_id),
                ui_generation: {
                    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
                    service.hook_runtime.ui_slot.install_tx(session_id, tx)
                },
                operation: service.generation_operation(session_id),
            },
        );
    assert!(service.sessions.read().await.get(session_id).is_none());

    service
        .chat_control(Request::new(ChatControlRequest {
            session_id: session_id.into(),
            action: ChatControlAction::ChatControlCancel as i32,
        }))
        .await
        .expect("stale cancel should remain idempotent");

    assert!(
        service.sessions.read().await.get(session_id).is_none(),
        "cancel must not lazily recreate a released session"
    );
}

async fn spawn_pending_turn(
    session: SessionHandle,
) -> (
    tokio::task::JoinHandle<()>,
    tokio::sync::mpsc::Receiver<anyhow::Result<agent_protocol::Event>>,
) {
    let chat: ResponsesOverride = Arc::new(|_, _, _| {
        Box::pin(async move { Ok(Box::pin(futures::stream::pending()) as CompletionStream) })
    });
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let handle = tokio::spawn(run_multi_turn_events_with_responses_fn(
        session,
        chat,
        ProviderConfig {
            model: "scripted".into(),
            ..ProviderConfig::default()
        },
        "system".into(),
        PauseControl::new(),
        None,
        tx,
    ));
    let started = rx.recv().await.expect("pending turn should start").unwrap();
    assert!(matches!(
        started.msg,
        agent_protocol::EventMsg::TurnStarted(_)
    ));
    (handle, rx)
}

#[tokio::test]
async fn stale_cancel_targets_the_session_generation_that_registered_pause() {
    let dir = TempDir::new().unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    let session_id = "cancel-generation-race";
    let old_session = service.get_session(session_id).await.unwrap();
    let (old_turn, _old_rx) = spawn_pending_turn(Arc::clone(&old_session)).await;
    let old_gate = HitlGate::new(session_id);
    let (old_ui_tx, _old_ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let _old_pause = service
        .admit_pause_generation(session_id, &old_session, old_gate, old_ui_tx)
        .await
        .unwrap();

    service.sessions.write().await.remove(session_id);
    let replacement = service.get_session(session_id).await.unwrap();
    let (replacement_turn, _replacement_rx) = spawn_pending_turn(Arc::clone(&replacement)).await;

    service
        .chat_control(Request::new(ChatControlRequest {
            session_id: session_id.into(),
            action: ChatControlAction::ChatControlCancel as i32,
        }))
        .await
        .expect("stale cancel should remain valid for its exact generation");

    assert!(
        old_session.cancel_signal().is_cancelled(),
        "the pause registration must retain the old session generation"
    );
    assert!(
        !replacement.cancel_signal().is_cancelled(),
        "a same-id replacement session must not be cancelled"
    );

    replacement
        .abort_all_tasks(TurnAbortReason::Replaced)
        .await
        .unwrap();
    old_turn.await.unwrap();
    replacement_turn.await.unwrap();
}

#[tokio::test]
async fn stale_session_arc_cannot_admit_over_its_same_id_replacement() {
    let dir = TempDir::new().unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    let session_id = "stale-admission-session";
    let stale = service.get_session(session_id).await.unwrap();
    service.sessions.write().await.remove(session_id);
    let replacement = service.get_session(session_id).await.unwrap();
    assert!(!Arc::ptr_eq(&stale, &replacement));

    let (stale_ui_tx, _stale_ui_rx) = tokio::sync::mpsc::unbounded_channel();
    assert!(
        service
            .admit_pause_generation(session_id, &stale, HitlGate::new(session_id), stale_ui_tx,)
            .await
            .is_none(),
        "an Arc removed from sessions must not overwrite the replacement generation"
    );
    assert!(service
        .pause_controls
        .read()
        .expect("pause registry lock poisoned")
        .get(session_id)
        .is_none());

    let (replacement_ui_tx, _replacement_ui_rx) = tokio::sync::mpsc::unbounded_channel();
    assert!(service
        .admit_pause_generation(
            session_id,
            &replacement,
            HitlGate::new(session_id),
            replacement_ui_tx,
        )
        .await
        .is_some());
}

#[tokio::test]
async fn concurrent_generation_setup_and_install_keep_request_settings_together() {
    let dir = TempDir::new().unwrap();
    let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
    let session_id = "generation-config-isolation";
    let session = service.get_session(session_id).await.unwrap();
    let (ui_tx, _ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let registration = service
        .admit_pause_generation(session_id, &session, HitlGate::new(session_id), ui_tx)
        .await
        .unwrap();
    let first_setup_started = Arc::new(tokio::sync::Notify::new());
    let release_first_setup = Arc::new(tokio::sync::Notify::new());
    let second_setup_started = Arc::new(AtomicBool::new(false));
    let first = {
        let service = Arc::clone(&service);
        let session = Arc::clone(&session);
        let launch_session = Arc::clone(&session);
        let registration = registration.clone();
        let first_setup_started = Arc::clone(&first_setup_started);
        let release_first_setup = Arc::clone(&release_first_setup);
        tokio::spawn(async move {
            service
                .launch_current_pause_generation_with_setup(
                    session_id,
                    &registration,
                    &session,
                    move |session| async move {
                        session
                            .set_interaction_mode(tools::InteractionMode::Plan)
                            .await;
                        session.set_temperature(0.2);
                        first_setup_started.notify_one();
                        release_first_setup.notified().await;
                    },
                    move || {
                        let session = Arc::clone(&launch_session);
                        async move { (session.interaction_mode().await, session.temperature()) }
                    },
                )
                .await
                .unwrap()
        })
    };
    first_setup_started.notified().await;

    let second = {
        let service = Arc::clone(&service);
        let session = Arc::clone(&session);
        let launch_session = Arc::clone(&session);
        let registration = registration.clone();
        let second_setup_started = Arc::clone(&second_setup_started);
        tokio::spawn(async move {
            service
                .launch_current_pause_generation_with_setup(
                    session_id,
                    &registration,
                    &session,
                    move |session| async move {
                        second_setup_started.store(true, Ordering::SeqCst);
                        session
                            .set_interaction_mode(tools::InteractionMode::Agent)
                            .await;
                        session.set_temperature(1.4);
                    },
                    move || {
                        let session = Arc::clone(&launch_session);
                        async move { (session.interaction_mode().await, session.temperature()) }
                    },
                )
                .await
                .unwrap()
        })
    };
    tokio::task::yield_now().await;
    assert!(
        !second_setup_started.load(Ordering::SeqCst),
        "the second request must not apply settings before the first installs"
    );
    release_first_setup.notify_one();

    let first = first.await.unwrap();
    let second = second.await.unwrap();
    assert_eq!(first, (tools::InteractionMode::Plan, 0.2));
    assert_eq!(second, (tools::InteractionMode::Agent, 1.4));
}

#[tokio::test]
async fn cancelled_generation_launch_cleans_registration_and_restores_settings() {
    let dir = TempDir::new().unwrap();
    let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
    let session_id = "cancelled-generation-launch";
    let session = service.get_session(session_id).await.unwrap();
    let initial_temperature = session.temperature();
    let (ui_tx, mut ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let gate = HitlGate::new(session_id);
    let registration = service
        .admit_pause_generation(session_id, &session, Arc::clone(&gate), ui_tx)
        .await
        .unwrap();
    let setup_applied = Arc::new(tokio::sync::Notify::new());
    let release_launch = Arc::new(tokio::sync::Notify::new());
    let launch_owner = tokio::spawn({
        let service = Arc::clone(&service);
        let session = Arc::clone(&session);
        let setup_applied = Arc::clone(&setup_applied);
        let release_launch = Arc::clone(&release_launch);
        async move {
            service
                .launch_current_pause_generation_with_setup(
                    session_id,
                    &registration,
                    &session,
                    move |session| async move {
                        session.set_temperature(0.2);
                        session
                            .set_interaction_mode(tools::InteractionMode::Plan)
                            .await;
                        setup_applied.notify_one();
                    },
                    move || async move {
                        release_launch.notified().await;
                    },
                )
                .await
        }
    });
    setup_applied.notified().await;
    launch_owner.abort();
    assert!(launch_owner.await.unwrap_err().is_cancelled());
    release_launch.notify_one();

    tokio::time::timeout(WAIT_BUDGET, async {
        loop {
            if service
                .pause_controls
                .read()
                .expect("pause registry lock poisoned")
                .get(session_id)
                .is_none()
                && service.hitl_registry.get(session_id).await.is_none()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("abandoned generation must be cleaned by service-owned work");
    assert!(matches!(
        ui_rx.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Disconnected)
    ));

    let (next_ui_tx, _next_ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let next = service
        .admit_pause_generation(session_id, &session, HitlGate::new(session_id), next_ui_tx)
        .await
        .unwrap();
    let inherited = service
        .launch_current_pause_generation_with_setup(session_id, &next, &session, |_| async {}, {
            let session = Arc::clone(&session);
            move || async move { session.temperature() }
        })
        .await
        .unwrap();
    assert_eq!(inherited, initial_temperature);
}

#[tokio::test]
async fn abandoned_launch_reply_after_send_rolls_back_exact_generation() {
    let dir = TempDir::new().unwrap();
    let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
    let session_id = "abandoned-launch-reply-after-send";
    let session = service.get_session(session_id).await.unwrap();
    let initial_temperature = session.temperature();
    let initial_mode = session.interaction_mode().await;
    let gate = HitlGate::new(session_id);
    let mut gate_wait = gate
        .begin_wait(agent::Interrupt {
            id: "abandoned-launch-reply-gate".into(),
            ..Default::default()
        })
        .await;
    let (ui_tx, mut ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let registration = service
        .admit_pause_generation(session_id, &session, Arc::clone(&gate), ui_tx)
        .await
        .unwrap();
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    let worker = tokio::spawn({
        let service = Arc::clone(&service);
        let worker_session = Arc::clone(&session);
        let launch_session = Arc::clone(&session);
        async move {
            service
                .run_generation_launch_worker(
                    session_id.to_string(),
                    registration,
                    worker_session,
                    move |session| async move {
                        session.set_temperature(0.2);
                        session
                            .set_interaction_mode(tools::InteractionMode::Plan)
                            .await;
                    },
                    move || async move { spawn_pending_turn(launch_session).await },
                    reply_tx,
                )
                .await;
        }
    });

    let guarded_reply = reply_rx
        .await
        .expect("worker must send its launch reply")
        .expect("current generation must launch");
    tokio::task::yield_now().await;
    assert!(
        !worker.is_finished(),
        "a successful reply send must leave the worker waiting for caller acceptance"
    );
    assert_eq!(session.temperature(), 0.2);
    assert_eq!(
        session.interaction_mode().await,
        tools::InteractionMode::Plan
    );
    assert!(!session.cancel_signal().is_cancelled());

    drop(guarded_reply);
    tokio::time::timeout(WAIT_BUDGET, worker)
        .await
        .expect("false acknowledgement must release the launch worker")
        .unwrap();

    assert!(session.cancel_signal().is_cancelled());
    assert_eq!(session.temperature(), initial_temperature);
    assert_eq!(session.interaction_mode().await, initial_mode);
    assert_eq!(
        tokio::time::timeout(WAIT_BUDGET, &mut gate_wait)
            .await
            .expect("false acknowledgement must resolve the exact gate")
            .expect("gate resolution")
            .status,
        "cancelled"
    );
    assert!(service
        .pause_controls
        .read()
        .expect("pause registry lock poisoned")
        .get(session_id)
        .is_none());
    assert!(service.hitl_registry.get(session_id).await.is_none());
    loop {
        match ui_rx.try_recv() {
            Ok(_) => {}
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                panic!("false acknowledgement must detach the exact UI generation")
            }
        }
    }
    assert!(!service
        .generation_operations
        .lock()
        .unwrap()
        .contains_key(session_id));
}

#[tokio::test]
async fn abandoned_prepared_admission_after_send_does_not_commit() {
    let dir = TempDir::new().unwrap();
    let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
    let session_id = "abandoned-admission-reply-after-send";
    let session = service.get_session(session_id).await.unwrap();
    let live_gate = HitlGate::new(session_id);
    let (live_ui_tx, mut live_ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let live = service
        .admit_pause_generation(session_id, &session, Arc::clone(&live_gate), live_ui_tx)
        .await
        .unwrap();
    let abandoned_gate = HitlGate::new(session_id);
    let (abandoned_ui_tx, mut abandoned_ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    let worker = tokio::spawn({
        let service = Arc::clone(&service);
        let session = Arc::clone(&session);
        let abandoned_gate = Arc::clone(&abandoned_gate);
        async move {
            service
                .run_admit_pause_generation_worker(
                    session_id.to_string(),
                    session,
                    abandoned_gate,
                    abandoned_ui_tx,
                    reply_tx,
                )
                .await;
        }
    });

    let prepared = reply_rx
        .await
        .expect("worker must send its admission reply")
        .expect("current session must be admitted");
    tokio::time::timeout(WAIT_BUDGET, worker)
        .await
        .expect("prepare worker must finish after transferring the commit capability")
        .unwrap();
    assert!(service
        .pause_controls
        .read()
        .expect("pause registry lock poisoned")
        .get(session_id)
        .is_some_and(|current| Arc::ptr_eq(&current.control, &live.control)));
    assert!(!live.control.is_cancelled());

    drop(prepared);
    assert!(service
        .pause_controls
        .read()
        .expect("pause registry lock poisoned")
        .get(session_id)
        .is_some_and(|current| Arc::ptr_eq(&current.control, &live.control)));
    assert!(!live.control.is_cancelled());
    assert!(service
        .hitl_registry
        .get(session_id)
        .await
        .is_some_and(|gate| Arc::ptr_eq(&gate, &live_gate)));
    loop {
        match abandoned_ui_rx.try_recv() {
            Ok(_) => {}
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                panic!("false acknowledgement must detach the abandoned UI generation")
            }
        }
    }
    let _ = service.hook_runtime.plugin.fire(
        ::hooks::PRE_LLM_CALL,
        &::hooks::HookPayload {
            session_id: session_id.into(),
            ..Default::default()
        },
    );
    assert_eq!(live_ui_rx.try_recv().unwrap().name, ::hooks::PRE_LLM_CALL);
    assert!(service
        .generation_operations
        .lock()
        .unwrap()
        .contains_key(session_id));

    let replacement_gate = HitlGate::new(session_id);
    let (replacement_ui_tx, mut replacement_ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let replacement = service
        .admit_pause_generation(
            session_id,
            &session,
            Arc::clone(&replacement_gate),
            replacement_ui_tx,
        )
        .await
        .expect("a replacement must admit after abandoned handoff cleanup");
    assert!(!replacement.control.is_cancelled());
    assert!(service
        .hitl_registry
        .get(session_id)
        .await
        .is_some_and(|gate| Arc::ptr_eq(&gate, &replacement_gate)));
    let _ = service.hook_runtime.plugin.fire(
        ::hooks::PRE_LLM_CALL,
        &::hooks::HookPayload {
            session_id: session_id.into(),
            ..Default::default()
        },
    );
    assert_eq!(
        replacement_ui_rx.try_recv().unwrap().name,
        ::hooks::PRE_LLM_CALL
    );
}

#[tokio::test]
async fn abandoned_prepared_admission_without_live_generation_prunes_operation() {
    let dir = TempDir::new().unwrap();
    let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
    let session_id = "abandoned-prepared-admission-prunes-operation";
    let session = service.get_session(session_id).await.unwrap();
    let (ui_tx, _ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    let worker = tokio::spawn({
        let service = Arc::clone(&service);
        let session = Arc::clone(&session);
        async move {
            service
                .run_admit_pause_generation_worker(
                    session_id.to_string(),
                    session,
                    HitlGate::new(session_id),
                    ui_tx,
                    reply_tx,
                )
                .await;
        }
    });

    let prepared = reply_rx
        .await
        .expect("worker must send its prepared admission")
        .expect("current session must prepare admission");
    worker.await.unwrap();
    assert!(service
        .generation_operations
        .lock()
        .unwrap()
        .contains_key(session_id));

    drop(prepared);
    assert!(!service
        .generation_operations
        .lock()
        .unwrap()
        .contains_key(session_id));
}

#[tokio::test]
async fn closed_admission_reply_does_not_replace_live_generation() {
    let dir = TempDir::new().unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    let session_id = "closed-admission-reply-live-generation";
    let session = service.get_session(session_id).await.unwrap();
    let live_gate = HitlGate::new(session_id);
    let (live_ui_tx, mut live_ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let live = service
        .admit_pause_generation(session_id, &session, Arc::clone(&live_gate), live_ui_tx)
        .await
        .unwrap();

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    drop(reply_rx);
    let (abandoned_ui_tx, _abandoned_ui_rx) = tokio::sync::mpsc::unbounded_channel();
    service
        .run_admit_pause_generation_worker(
            session_id.to_string(),
            Arc::clone(&session),
            HitlGate::new(session_id),
            abandoned_ui_tx,
            reply_tx,
        )
        .await;

    assert!(
        !live.control.is_cancelled(),
        "a closed caller must not irreversibly cancel the live generation"
    );
    assert!(service
        .pause_controls
        .read()
        .expect("pause registry lock poisoned")
        .get(session_id)
        .is_some_and(|current| Arc::ptr_eq(&current.control, &live.control)));
    assert!(service
        .hitl_registry
        .get(session_id)
        .await
        .is_some_and(|gate| Arc::ptr_eq(&gate, &live_gate)));
    let _ = service.hook_runtime.plugin.fire(
        ::hooks::PRE_LLM_CALL,
        &::hooks::HookPayload {
            session_id: session_id.into(),
            ..Default::default()
        },
    );
    assert_eq!(live_ui_rx.try_recv().unwrap().name, ::hooks::PRE_LLM_CALL);
}

#[tokio::test]
async fn stale_chat_cleanup_preserves_replacement_pause_gate_and_ui() {
    let dir = TempDir::new().unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    let session_id = "cleanup-generation-race";
    let old_session = service.get_session(session_id).await.unwrap();
    let old_gate = HitlGate::new(session_id);
    let old_gate_rx = old_gate
        .begin_wait(agent::Interrupt {
            id: "old-gate".into(),
            ..Default::default()
        })
        .await;
    service.hitl_registry.insert(Arc::clone(&old_gate)).await;
    let (old_ui_tx, _old_ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let old_ui_generation = service
        .hook_runtime
        .ui_slot
        .install_tx(session_id, old_ui_tx);
    let old_registration = PauseRegistration {
        control: PauseControl::new(),
        session: Arc::downgrade(&old_session),
        hitl_gate: Arc::clone(&old_gate),
        ui_generation: old_ui_generation,
        operation: service.generation_operation(session_id),
    };
    service
        .pause_controls
        .write()
        .expect("pause registry lock poisoned")
        .insert(session_id.into(), old_registration.clone());

    service.sessions.write().await.remove(session_id);
    let replacement = service.get_session(session_id).await.unwrap();
    let replacement_gate = HitlGate::new(session_id);
    let mut replacement_gate_rx = replacement_gate
        .begin_wait(agent::Interrupt {
            id: "replacement-gate".into(),
            ..Default::default()
        })
        .await;
    service
        .hitl_registry
        .insert(Arc::clone(&replacement_gate))
        .await;
    let (replacement_ui_tx, mut replacement_ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let replacement_ui_generation = service
        .hook_runtime
        .ui_slot
        .install_tx(session_id, replacement_ui_tx);
    let replacement_registration = PauseRegistration {
        control: PauseControl::new(),
        session: Arc::downgrade(&replacement),
        hitl_gate: Arc::clone(&replacement_gate),
        ui_generation: replacement_ui_generation,
        operation: service.generation_operation(session_id),
    };
    service
        .pause_controls
        .write()
        .expect("pause registry lock poisoned")
        .insert(session_id.into(), replacement_registration.clone());

    service
        .cleanup_pause_generation(session_id, &old_registration)
        .await;

    assert!(old_registration.control.is_cancelled());
    assert_eq!(old_gate_rx.await.unwrap().status, "cancelled");
    assert!(replacement_gate_rx.try_recv().is_err());
    assert!(service
        .pause_controls
        .read()
        .expect("pause registry lock poisoned")
        .get(session_id)
        .is_some_and(|registration| Arc::ptr_eq(
            &registration.control,
            &replacement_registration.control
        )));
    assert!(service
        .hitl_registry
        .get(session_id)
        .await
        .is_some_and(|gate| Arc::ptr_eq(&gate, &replacement_gate)));

    let _ = service.hook_runtime.plugin.fire(
        ::hooks::PRE_LLM_CALL,
        &::hooks::HookPayload {
            session_id: session_id.into(),
            system_prompt_chars: Some(17),
            ..Default::default()
        },
    );
    assert_eq!(
        replacement_ui_rx.try_recv().unwrap().name,
        ::hooks::PRE_LLM_CALL
    );
}

#[tokio::test]
async fn concurrent_chat_admission_and_stale_cleanup_preserve_one_generation() {
    let dir = TempDir::new().unwrap();
    let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
    let session_id = "concurrent-chat-generation";
    let session = service.get_session(session_id).await.unwrap();
    let gate_a = HitlGate::new(session_id);
    let gate_b = HitlGate::new(session_id);
    let (ui_tx_a, mut ui_rx_a) = tokio::sync::mpsc::unbounded_channel();
    let (ui_tx_b, mut ui_rx_b) = tokio::sync::mpsc::unbounded_channel();
    let barrier = Arc::new(tokio::sync::Barrier::new(3));

    let admission_a = {
        let service = Arc::clone(&service);
        let session = Arc::clone(&session);
        let gate = Arc::clone(&gate_a);
        let barrier = Arc::clone(&barrier);
        tokio::spawn(async move {
            barrier.wait().await;
            service
                .admit_pause_generation(session_id, &session, gate, ui_tx_a)
                .await
                .unwrap()
        })
    };
    let admission_b = {
        let service = Arc::clone(&service);
        let session = Arc::clone(&session);
        let gate = Arc::clone(&gate_b);
        let barrier = Arc::clone(&barrier);
        tokio::spawn(async move {
            barrier.wait().await;
            service
                .admit_pause_generation(session_id, &session, gate, ui_tx_b)
                .await
                .unwrap()
        })
    };
    barrier.wait().await;
    let registration_a = admission_a.await.unwrap();
    let registration_b = admission_b.await.unwrap();

    let current = service
        .pause_controls
        .read()
        .expect("pause registry lock poisoned")
        .get(session_id)
        .cloned()
        .expect("one generation remains registered");
    let (winner, loser, winner_gate, winner_ui_rx, loser_ui_rx) =
        if Arc::ptr_eq(&current.control, &registration_a.control) {
            (
                registration_a,
                registration_b,
                gate_a,
                &mut ui_rx_a,
                &mut ui_rx_b,
            )
        } else {
            (
                registration_b,
                registration_a,
                gate_b,
                &mut ui_rx_b,
                &mut ui_rx_a,
            )
        };
    assert!(service
        .hitl_registry
        .get(session_id)
        .await
        .is_some_and(|gate| Arc::ptr_eq(&gate, &winner_gate)));

    save_interrupt_file(
        &service.memory_dir,
        session_id,
        &[agent::Interrupt {
            id: "winner-interrupt".into(),
            ..Default::default()
        }],
    )
    .unwrap();
    service.cleanup_pause_generation(session_id, &loser).await;

    assert!(service
        .pause_controls
        .read()
        .expect("pause registry lock poisoned")
        .get(session_id)
        .is_some_and(|registration| Arc::ptr_eq(&registration.control, &winner.control)));
    assert!(service
        .hitl_registry
        .get(session_id)
        .await
        .is_some_and(|gate| Arc::ptr_eq(&gate, &winner_gate)));
    let _ = service.hook_runtime.plugin.fire(
        ::hooks::PRE_LLM_CALL,
        &::hooks::HookPayload {
            session_id: session_id.into(),
            ..Default::default()
        },
    );
    assert_eq!(winner_ui_rx.try_recv().unwrap().name, ::hooks::PRE_LLM_CALL);
    assert!(loser_ui_rx.try_recv().is_err());
    let interrupt_path =
        crate::grpc::interrupt_store::interrupt_file_path(&service.memory_dir, session_id);
    let interrupts: Vec<agent::Interrupt> =
        serde_json::from_slice(&std::fs::read(interrupt_path).unwrap()).unwrap();
    assert_eq!(interrupts[0].id, "winner-interrupt");
}

#[tokio::test]
async fn same_session_admission_waits_for_cancel_abort_to_finish() {
    let dir = TempDir::new().unwrap();
    let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
    let session_id = "cancel-admission-generation";
    let session = service.get_session(session_id).await.unwrap();
    let other_session_id = "independent-generation";
    let other_session = service.get_session(other_session_id).await.unwrap();
    let (old_ui_tx, _old_ui_rx) = tokio::sync::mpsc::unbounded_channel();
    service
        .admit_pause_generation(session_id, &session, HitlGate::new(session_id), old_ui_tx)
        .await
        .unwrap();

    let cancel_entered = Arc::new(tokio::sync::Barrier::new(2));
    let allow_abort = Arc::new(tokio::sync::Barrier::new(2));
    let cancel = {
        let service = Arc::clone(&service);
        let cancel_entered = Arc::clone(&cancel_entered);
        let allow_abort = Arc::clone(&allow_abort);
        tokio::spawn(async move {
            service
                .cancel_current_pause_generation_with(session_id, move |session| async move {
                    cancel_entered.wait().await;
                    allow_abort.wait().await;
                    if let Some(session) = session {
                        session
                            .abort_all_tasks(TurnAbortReason::Interrupted)
                            .await?;
                    }
                    Ok(())
                })
                .await
        })
    };
    cancel_entered.wait().await;

    let (new_ui_tx, _new_ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let admission = {
        let service = Arc::clone(&service);
        let session = Arc::clone(&session);
        tokio::spawn(async move {
            service
                .admit_pause_generation(session_id, &session, HitlGate::new(session_id), new_ui_tx)
                .await
        })
    };
    tokio::task::yield_now().await;
    assert!(
        !admission.is_finished(),
        "new generation must wait until old cancel finishes aborting its Session"
    );
    let (other_ui_tx, _other_ui_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        service.admit_pause_generation(
            other_session_id,
            &other_session,
            HitlGate::new(other_session_id),
            other_ui_tx,
        ),
    )
    .await
    .expect("a different session must not wait behind the blocked cancel")
    .unwrap();

    allow_abort.wait().await;
    cancel.await.unwrap().unwrap().unwrap();
    let new_registration = admission.await.unwrap().unwrap();
    assert!(!new_registration.control.is_cancelled());

    let (new_turn, _new_rx) = spawn_pending_turn(Arc::clone(&session)).await;
    tokio::task::yield_now().await;
    assert!(
        !new_turn.is_finished(),
        "the completed old cancel must not abort the newly admitted turn"
    );
    session
        .abort_all_tasks(TurnAbortReason::Replaced)
        .await
        .unwrap();
    new_turn.await.unwrap();
}

#[tokio::test]
async fn cancelled_cancel_owner_still_finishes_gate_cleanup() {
    let dir = TempDir::new().unwrap();
    let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
    let session_id = "cancelled-cancel-owner";
    let session = service.get_session(session_id).await.unwrap();
    let gate = HitlGate::new(session_id);
    let (ui_tx, _ui_rx) = tokio::sync::mpsc::unbounded_channel();
    service
        .admit_pause_generation(session_id, &session, Arc::clone(&gate), ui_tx)
        .await
        .unwrap();
    let mut gate_wait = gate
        .begin_wait(agent::Interrupt {
            id: "cancel-owner-gate".into(),
            ..Default::default()
        })
        .await;
    let abort_entered = Arc::new(tokio::sync::Notify::new());
    let release_abort = Arc::new(tokio::sync::Notify::new());
    let owner = tokio::spawn({
        let service = Arc::clone(&service);
        let abort_entered = Arc::clone(&abort_entered);
        let release_abort = Arc::clone(&release_abort);
        async move {
            service
                .cancel_current_pause_generation_with(session_id, move |_| async move {
                    abort_entered.notify_one();
                    release_abort.notified().await;
                    Ok(())
                })
                .await
        }
    });
    abort_entered.notified().await;
    owner.abort();
    assert!(matches!(owner.await, Err(error) if error.is_cancelled()));
    release_abort.notify_one();

    let resolution = tokio::time::timeout(WAIT_BUDGET, &mut gate_wait)
        .await
        .expect("detached cancel worker must resolve the exact gate")
        .expect("gate resolution");
    assert_eq!(resolution.status, "cancelled");
    tokio::time::timeout(WAIT_BUDGET, async {
        loop {
            if !service
                .generation_operations
                .lock()
                .unwrap()
                .contains_key(session_id)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cancel worker must prune its generation operation");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelled_release_owner_still_finishes_session_cleanup() {
    let dir = TempDir::new().unwrap();
    let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
    let session_id = "cancelled-release-owner";
    let finalize_hits = Arc::new(AtomicUsize::new(0));
    let finalize_counter = Arc::clone(&finalize_hits);
    let (finalize_entered_tx, finalize_entered_rx) = std::sync::mpsc::channel();
    let (release_finalize_tx, release_finalize_rx) = std::sync::mpsc::channel();
    let release_finalize_rx = Arc::new(std::sync::Mutex::new(release_finalize_rx));
    service
        .hook_runtime
        .plugin
        .register(::hooks::SESSION_END, move |_| {
            finalize_counter.fetch_add(1, Ordering::SeqCst);
            let _ = finalize_entered_tx.send(());
            let _ = release_finalize_rx
                .lock()
                .expect("finalize release mutex poisoned")
                .recv();
            ::hooks::HookOutcome::Continue
        });
    let session = service.get_session(session_id).await.unwrap();
    let gate = HitlGate::new(session_id);
    let (ui_tx, _ui_rx) = tokio::sync::mpsc::unbounded_channel();
    service
        .admit_pause_generation(session_id, &session, gate, ui_tx)
        .await
        .unwrap();

    let owner = tokio::spawn({
        let service = Arc::clone(&service);
        async move { service.release_session_runtime(session_id).await }
    });
    tokio::task::spawn_blocking(move || finalize_entered_rx.recv())
        .await
        .unwrap()
        .unwrap();
    owner.abort();
    assert!(matches!(owner.await, Err(error) if error.is_cancelled()));
    release_finalize_tx.send(()).unwrap();
    session.shutdown_runtime().await;

    tokio::time::timeout(WAIT_BUDGET, async {
        loop {
            if !service
                .generation_operations
                .lock()
                .unwrap()
                .contains_key(session_id)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("release worker must prune its generation operation");
    assert_eq!(finalize_hits.load(Ordering::SeqCst), 1);
    assert!(service.sessions.read().await.get(session_id).is_none());
    assert!(service
        .pause_controls
        .read()
        .expect("pause registry lock poisoned")
        .get(session_id)
        .is_none());
    assert!(service.hitl_registry.get(session_id).await.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_new_chat_release_finalizes_session_once() {
    let dir = TempDir::new().unwrap();
    let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
    let session_id = "concurrent-new-chat-finalize-once";
    let finalize_hits = Arc::new(AtomicUsize::new(0));
    let finalize_counter = Arc::clone(&finalize_hits);
    let (finalize_entered_tx, finalize_entered_rx) = std::sync::mpsc::channel();
    let (release_finalize_tx, release_finalize_rx) = std::sync::mpsc::channel();
    let release_finalize_rx = Arc::new(std::sync::Mutex::new(release_finalize_rx));
    service
        .hook_runtime
        .plugin
        .register(::hooks::SESSION_END, move |_| {
            if finalize_counter.fetch_add(1, Ordering::SeqCst) == 0 {
                let _ = finalize_entered_tx.send(());
                let _ = release_finalize_rx
                    .lock()
                    .expect("finalize release mutex poisoned")
                    .recv();
            }
            ::hooks::HookOutcome::Continue
        });
    service.get_session(session_id).await.unwrap();

    let first = tokio::spawn({
        let service = Arc::clone(&service);
        async move { service.release_session_for_new_chat(session_id).await }
    });
    tokio::task::spawn_blocking(move || finalize_entered_rx.recv())
        .await
        .unwrap()
        .unwrap();
    let second = tokio::spawn({
        let service = Arc::clone(&service);
        async move { service.release_session_for_new_chat(session_id).await }
    });
    tokio::task::yield_now().await;
    assert!(
        !second.is_finished(),
        "the concurrent release must share the in-flight completion"
    );
    assert_eq!(finalize_hits.load(Ordering::SeqCst), 1);

    release_finalize_tx.send(()).unwrap();
    first.await.unwrap();
    second.await.unwrap();
    assert_eq!(finalize_hits.load(Ordering::SeqCst), 1);
    assert!(service.release_ownerships.lock().unwrap().is_empty());
}

#[tokio::test]
async fn concurrent_release_callers_share_ownership_then_reclaim_entry() {
    let dir = TempDir::new().unwrap();
    let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
    let session_id = "concurrent-release-shared-ownership";
    let finalize_hits = Arc::new(AtomicUsize::new(0));
    let finalize_counter = Arc::clone(&finalize_hits);
    service
        .hook_runtime
        .plugin
        .register(::hooks::SESSION_END, move |_| {
            finalize_counter.fetch_add(1, Ordering::SeqCst);
            ::hooks::HookOutcome::Continue
        });
    service.get_session(session_id).await.unwrap();

    let acquired = Arc::new(tokio::sync::Barrier::new(3));
    let release = Arc::new(tokio::sync::Barrier::new(3));
    let ownership_ptrs = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut callers = Vec::new();
    for _ in 0..2 {
        callers.push(tokio::spawn({
            let service = Arc::clone(&service);
            let acquired = Arc::clone(&acquired);
            let release = Arc::clone(&release);
            let ownership_ptrs = Arc::clone(&ownership_ptrs);
            async move {
                let ownership = service.release_generation_ownership(session_id);
                ownership_ptrs.lock().unwrap().push(ownership.entry_ptr());
                acquired.wait().await;
                release.wait().await;
                service
                    .release_session_runtime_with_ownership(session_id, ownership)
                    .await
            }
        }));
    }

    acquired.wait().await;
    let ownership_ptrs = ownership_ptrs.lock().unwrap().clone();
    assert_eq!(ownership_ptrs.len(), 2);
    assert_eq!(ownership_ptrs[0], ownership_ptrs[1]);
    assert_eq!(service.release_ownerships.lock().unwrap().len(), 1);
    release.wait().await;
    for caller in callers {
        let result = caller.await.unwrap();
        assert!(!result.should_finalize_without_runtime);
    }
    assert_eq!(finalize_hits.load(Ordering::SeqCst), 1);
    assert!(service.release_ownerships.lock().unwrap().is_empty());
}

#[tokio::test]
async fn cancelled_cold_release_reply_does_not_consume_fallback_claim() {
    let dir = TempDir::new().unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    let session_id = "cancelled-cold-release-fallback-claim";
    let cancelled_ownership = service.release_generation_ownership(session_id);
    let surviving_ownership = service.release_generation_ownership(session_id);
    assert_eq!(
        cancelled_ownership.entry_ptr(),
        surviving_ownership.entry_ptr()
    );
    let finalize_hits = Arc::new(AtomicUsize::new(0));
    let finalize_counter = Arc::clone(&finalize_hits);
    service
        .hook_runtime
        .plugin
        .register(::hooks::SESSION_END, move |_| {
            finalize_counter.fetch_add(1, Ordering::SeqCst);
            ::hooks::HookOutcome::Continue
        });

    let (cancelled_tx, cancelled_rx) = tokio::sync::oneshot::channel();
    drop(cancelled_rx);
    service
        .run_release_session_runtime_worker(
            session_id.to_string(),
            cancelled_ownership,
            cancelled_tx,
        )
        .await;

    let (surviving_tx, surviving_rx) = tokio::sync::oneshot::channel();
    service
        .run_release_session_runtime_worker(
            session_id.to_string(),
            surviving_ownership,
            surviving_tx,
        )
        .await;
    let result = surviving_rx
        .await
        .expect("surviving caller must receive the shared completion")
        .claim_fallback();
    if result.should_finalize_without_runtime {
        let _ = service.hook_runtime.fire_plugin(
            ::hooks::SESSION_END,
            &::hooks::HookPayload {
                session_id: session_id.into(),
                ..Default::default()
            },
        );
    }

    assert!(result.should_finalize_without_runtime);
    assert_eq!(finalize_hits.load(Ordering::SeqCst), 1);
    assert!(service.release_ownerships.lock().unwrap().is_empty());
}

#[tokio::test]
async fn release_only_completion_does_not_consume_new_chat_fallback() {
    let dir = TempDir::new().unwrap();
    let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
    let session_id = "release-only-before-new-chat-fallback";
    let finalize_hits = Arc::new(AtomicUsize::new(0));
    let finalize_counter = Arc::clone(&finalize_hits);
    service
        .hook_runtime
        .plugin
        .register(::hooks::SESSION_END, move |_| {
            finalize_counter.fetch_add(1, Ordering::SeqCst);
            ::hooks::HookOutcome::Continue
        });
    let acquired = Arc::new(tokio::sync::Barrier::new(3));
    let (ownership_tx, mut ownership_rx) = tokio::sync::mpsc::unbounded_channel();
    for is_new_chat in [false, true] {
        tokio::spawn({
            let service = Arc::clone(&service);
            let acquired = Arc::clone(&acquired);
            let ownership_tx = ownership_tx.clone();
            async move {
                let ownership = service.release_generation_ownership(session_id);
                ownership_tx.send((is_new_chat, ownership)).unwrap();
                acquired.wait().await;
            }
        });
    }
    drop(ownership_tx);
    acquired.wait().await;
    let mut release_only = None;
    let mut new_chat = None;
    while let Some((is_new_chat, ownership)) = ownership_rx.recv().await {
        if is_new_chat {
            new_chat = Some(ownership);
        } else {
            release_only = Some(ownership);
        }
    }
    let release_only = release_only.expect("release-only ownership");
    let new_chat = new_chat.expect("new-chat ownership");
    assert_eq!(release_only.entry_ptr(), new_chat.entry_ptr());

    let release_result = service
        .release_session_runtime_with_ownership(session_id, release_only)
        .await;
    assert!(release_result.should_finalize_without_runtime);

    let (new_chat_tx, new_chat_rx) = tokio::sync::oneshot::channel();
    service
        .run_release_session_runtime_worker(session_id.to_string(), new_chat, new_chat_tx)
        .await;
    let new_chat_result = new_chat_rx
        .await
        .expect("new-chat caller must receive the shared completion")
        .claim_fallback();
    assert!(
        new_chat_result.should_finalize_without_runtime,
        "release-only completion must not consume fallback ownership"
    );
    let _ = service.hook_runtime.fire_plugin(
        ::hooks::SESSION_END,
        &::hooks::HookPayload {
            session_id: session_id.into(),
            ..Default::default()
        },
    );
    assert_eq!(finalize_hits.load(Ordering::SeqCst), 1);
    assert!(service.release_ownerships.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unique_release_ownership_entries_are_reclaimed() {
    let dir = TempDir::new().unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    let finalize_hits = Arc::new(AtomicUsize::new(0));
    let finalize_counter = Arc::clone(&finalize_hits);
    service
        .hook_runtime
        .plugin
        .register(::hooks::SESSION_END, move |_| {
            finalize_counter.fetch_add(1, Ordering::SeqCst);
            ::hooks::HookOutcome::Continue
        });

    for index in 0..32 {
        let session_id = format!("unique-release-ownership-{index}");
        service.get_session(&session_id).await.unwrap();
        let result = service.release_session_runtime(&session_id).await;
        assert!(!result.should_finalize_without_runtime);
    }

    assert_eq!(finalize_hits.load(Ordering::SeqCst), 32);
    assert!(service.release_ownerships.lock().unwrap().is_empty());
}

#[tokio::test]
async fn recreated_session_resets_finalize_ownership_generation() {
    let dir = TempDir::new().unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    let session_id = "recreated-session-finalize-ownership";
    let finalize_hits = Arc::new(AtomicUsize::new(0));
    let finalize_counter = Arc::clone(&finalize_hits);
    service
        .hook_runtime
        .plugin
        .register(::hooks::SESSION_END, move |_| {
            finalize_counter.fetch_add(1, Ordering::SeqCst);
            ::hooks::HookOutcome::Continue
        });

    let first = service.get_session(session_id).await.unwrap();
    let first_release = service.release_session_runtime(session_id).await;
    assert!(!first_release.should_finalize_without_runtime);
    assert_eq!(finalize_hits.load(Ordering::SeqCst), 1);
    assert!(service.release_ownerships.lock().unwrap().is_empty());

    let second = service.get_session(session_id).await.unwrap();
    assert!(!Arc::ptr_eq(&first, &second));
    assert!(service.release_ownerships.lock().unwrap().is_empty());

    let second_release = service.release_session_runtime(session_id).await;
    assert!(!second_release.should_finalize_without_runtime);
    assert_eq!(finalize_hits.load(Ordering::SeqCst), 2);
    assert!(service.release_ownerships.lock().unwrap().is_empty());
}

#[tokio::test]
async fn stale_launcher_cannot_replace_a_newly_installed_generation() {
    let dir = TempDir::new().unwrap();
    let service = Arc::new(AstroServiceImpl::new(dir.path().to_path_buf()));
    let session_id = "stale-launcher-generation";
    let session = service.get_session(session_id).await.unwrap();
    let (old_ui_tx, _old_ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let old_registration = service
        .admit_pause_generation(session_id, &session, HitlGate::new(session_id), old_ui_tx)
        .await
        .unwrap();
    let old_ready = Arc::new(tokio::sync::Barrier::new(2));
    let release_old = Arc::new(tokio::sync::Barrier::new(2));
    let old_touched_session = Arc::new(AtomicBool::new(false));
    let old_launcher = {
        let service = Arc::clone(&service);
        let old_ready = Arc::clone(&old_ready);
        let release_old = Arc::clone(&release_old);
        let old_touched_session = Arc::clone(&old_touched_session);
        tokio::spawn(async move {
            old_ready.wait().await;
            release_old.wait().await;
            service
                .launch_current_pause_generation_with(
                    session_id,
                    &old_registration,
                    move || async move {
                        old_touched_session.store(true, Ordering::SeqCst);
                    },
                )
                .await
        })
    };
    old_ready.wait().await;

    service
        .cancel_current_pause_generation(session_id)
        .await
        .unwrap()
        .unwrap();
    let (new_ui_tx, _new_ui_rx) = tokio::sync::mpsc::unbounded_channel();
    let new_registration = service
        .admit_pause_generation(session_id, &session, HitlGate::new(session_id), new_ui_tx)
        .await
        .unwrap();
    let (new_turn, _new_rx) = service
        .launch_current_pause_generation_with(session_id, &new_registration, {
            let session = Arc::clone(&session);
            move || async move { spawn_pending_turn(session).await }
        })
        .await
        .expect("the current generation must install its task");

    release_old.wait().await;
    assert!(old_launcher.await.unwrap().is_none());
    assert!(!old_touched_session.load(Ordering::SeqCst));
    assert!(
        !new_turn.is_finished(),
        "the stale launcher must not replace or abort the new task"
    );
    session
        .abort_all_tasks(TurnAbortReason::Replaced)
        .await
        .unwrap();
    new_turn.await.unwrap();
}

#[tokio::test]
async fn new_chat_preserves_hooks_while_release_session_skips_them() {
    let dir = TempDir::new().unwrap();
    let hook_dir = dir.path().join("hooks").join("audit");
    std::fs::create_dir_all(&hook_dir).unwrap();
    std::fs::write(
        hook_dir.join("HOOK.yaml"),
        "name: audit\nevents:\n  - CommandNewChat\n",
    )
    .unwrap();

    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    let gateway_hits = Arc::new(AtomicUsize::new(0));
    let gateway_counter = Arc::clone(&gateway_hits);
    service
        .hook_runtime
        .gateway
        .register_handler("audit", move |event, _| {
            if event == ::hooks::COMMAND_NEW_CHAT {
                gateway_counter.fetch_add(1, Ordering::SeqCst);
            }
        });

    let reset_hits = Arc::new(AtomicUsize::new(0));
    let reset_counter = Arc::clone(&reset_hits);
    service
        .hook_runtime
        .plugin
        .register(::hooks::SESSION_RESET, move |_| {
            reset_counter.fetch_add(1, Ordering::SeqCst);
            ::hooks::HookOutcome::Continue
        });
    let finalize_hits = Arc::new(AtomicUsize::new(0));
    let finalize_counter = Arc::clone(&finalize_hits);
    service
        .hook_runtime
        .plugin
        .register(::hooks::SESSION_END, move |_| {
            finalize_counter.fetch_add(1, Ordering::SeqCst);
            ::hooks::HookOutcome::Continue
        });

    service
        .chat_control(Request::new(ChatControlRequest {
            session_id: "release-only".into(),
            action: ChatControlAction::ReleaseSession as i32,
        }))
        .await
        .expect("release session");
    assert_eq!(gateway_hits.load(Ordering::SeqCst), 0);
    assert_eq!(reset_hits.load(Ordering::SeqCst), 0);
    assert_eq!(finalize_hits.load(Ordering::SeqCst), 0);

    let session = service.get_session("new-chat").await.unwrap();
    session.set_hook_runtime(Arc::clone(&service.hook_runtime));
    service
        .chat_control(Request::new(ChatControlRequest {
            session_id: "new-chat".into(),
            action: ChatControlAction::ChatControlNewChat as i32,
        }))
        .await
        .expect("new chat");
    assert_eq!(gateway_hits.load(Ordering::SeqCst), 1);
    assert_eq!(reset_hits.load(Ordering::SeqCst), 1);
    assert_eq!(finalize_hits.load(Ordering::SeqCst), 1);
}

#[test]
fn parse_auxiliary_targets_groups_by_task_and_sorts_by_order() {
    let map = crate::grpc::thread_settings::parse_auxiliary_targets(vec![
        proto::AuxiliaryModelTarget {
            task: "compaction".into(),
            provider_id: "p-fb".into(),
            backend_id: "openai".into(),
            model: "gpt-fb".into(),
            api_key: "k-fb".into(),
            base_url: "https://fb".into(),
            order: 1,
        },
        proto::AuxiliaryModelTarget {
            task: "compaction".into(),
            provider_id: "p-pref".into(),
            backend_id: "deepseek".into(),
            model: "gpt-pref".into(),
            api_key: "k-pref".into(),
            base_url: "https://pref".into(),
            order: 0,
        },
        proto::AuxiliaryModelTarget {
            task: "unknown_task".into(),
            provider_id: "x".into(),
            backend_id: "x".into(),
            model: "x".into(),
            api_key: "x".into(),
            base_url: "x".into(),
            order: 0,
        },
    ]);

    let chain = map
        .get(&types::AuxiliaryTask::Compaction)
        .expect("compaction");
    assert_eq!(chain.len(), 2);
    assert_eq!(chain[0].provider_id, "p-pref");
    assert_eq!(chain[1].provider_id, "p-fb");
    assert!(!map.contains_key(&types::AuxiliaryTask::Dreaming));
}

#[tokio::test]
async fn concurrent_thread_creation_installs_exactly_one_listener() {
    let dir = TempDir::new().unwrap();
    memory::ensure_workspace(dir.path()).unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    let first_service = service.clone();
    let second_service = service.clone();
    let (first, second) = tokio::join!(
        first_service.get_or_create_thread("race-thread"),
        second_service.get_or_create_thread("race-thread")
    );
    let first = first.expect("first create");
    let second = second.expect("second create");
    assert!(Arc::ptr_eq(&first, &second));
    assert!(service.threads.contains("race-thread").await);
    first
        .runtime
        .submit(agent_protocol::Op::Shutdown)
        .await
        .unwrap();
    first.runtime.wait_terminated().await;
}

#[tokio::test]
async fn idle_thread_unloads_only_after_thirty_minutes() {
    let dir = TempDir::new().unwrap();
    memory::ensure_workspace(dir.path()).unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    service
        .get_session("idle-thread")
        .await
        .expect("prewarm idle session");
    tokio::time::pause();
    let managed = service
        .get_or_create_thread("idle-thread")
        .await
        .expect("create idle thread");
    let old_runtime = Arc::clone(&managed.runtime);
    drop(managed);
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    tokio::time::advance(std::time::Duration::from_secs(29 * 60)).await;
    tokio::task::yield_now().await;
    assert!(service.threads.contains("idle-thread").await);

    tokio::time::advance(std::time::Duration::from_secs(60)).await;
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    assert!(!service.threads.contains("idle-thread").await);
    assert!(service.thread_states.get("idle-thread").await.is_none());
    tokio::time::resume();
    let replacement = service
        .get_or_create_thread("idle-thread")
        .await
        .expect("idle thread should be reloadable");
    assert!(!Arc::ptr_eq(&old_runtime, &replacement.runtime));
    assert!(!replacement.listener_is_finished().await);
    replacement
        .runtime
        .submit(agent_protocol::Op::Shutdown)
        .await
        .unwrap();
    replacement.runtime.wait_terminated().await;
}

#[tokio::test]
async fn acquired_managed_handle_prevents_idle_unload_until_released() {
    let dir = TempDir::new().unwrap();
    memory::ensure_workspace(dir.path()).unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    let managed = service
        .get_or_create_thread("leased-idle-thread")
        .await
        .expect("create idle thread");
    tokio::time::pause();
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    tokio::time::advance(std::time::Duration::from_secs(30 * 60)).await;
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    let current = service
        .threads
        .get("leased-idle-thread")
        .await
        .expect("leased thread remains current");
    assert!(Arc::ptr_eq(&managed, &current));
    assert!(!managed.listener_is_finished().await);

    drop(current);
    drop(managed);
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    tokio::time::advance(std::time::Duration::from_secs(30 * 60)).await;
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    assert!(!service.threads.contains("leased-idle-thread").await);
}

#[tokio::test]
async fn failed_terminal_unsubscribed_thread_unloads_after_thirty_minutes() {
    let dir = TempDir::new().unwrap();
    memory::ensure_workspace(dir.path()).unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    let managed = service
        .get_or_create_thread("failed-thread")
        .await
        .expect("create thread");
    tokio::time::pause();
    let (_receiver, _cancel, generation) = service
        .connections
        .register("connection-failed".into())
        .await;
    let subscription = generation.key().clone();
    let (resume_reply, resume_rx) = tokio::sync::oneshot::channel();
    managed
        .commands
        .send(ListenerCommand::Resume {
            subscription: subscription.clone(),
            include_turns: true,
            reply: resume_reply,
        })
        .unwrap();
    resume_rx.await.unwrap();
    managed
        .commands
        .send(ListenerCommand::CoreEvent(agent_protocol::Event {
            id: "turn-failed".into(),
            msg: agent_protocol::EventMsg::TurnStarted(agent_protocol::TurnStartedEvent {
                turn_id: "turn-failed".into(),
            }),
        }))
        .unwrap();
    managed
        .commands
        .send(ListenerCommand::CoreEvent(agent_protocol::Event {
            id: "turn-failed".into(),
            msg: agent_protocol::EventMsg::TurnComplete(agent_protocol::TurnCompleteEvent {
                turn_id: "turn-failed".into(),
                last_agent_message: None,
                error: Some(agent_protocol::ErrorEvent {
                    message: "boom".into(),
                    error_type: "provider".into(),
                }),
            }),
        }))
        .unwrap();
    let (unsubscribe_reply, unsubscribe_rx) = tokio::sync::oneshot::channel();
    managed
        .commands
        .send(ListenerCommand::Unsubscribe {
            subscription,
            reply: Some(unsubscribe_reply),
        })
        .unwrap();
    unsubscribe_rx.await.unwrap();
    assert_eq!(managed.activity_rx.borrow().status, "errored");
    assert!(!managed.activity_rx.borrow().has_subscribers);
    drop(managed);
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    tokio::time::advance(std::time::Duration::from_secs(30 * 60)).await;
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }

    assert!(!service.threads.contains("failed-thread").await);
    assert!(service.thread_states.get("failed-thread").await.is_none());
}

#[tokio::test]
async fn never_returning_background_phase_releases_sink_then_allows_idle_unload() {
    let dir = TempDir::new().unwrap();
    memory::ensure_workspace(dir.path()).unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    let managed = service
        .get_or_create_thread("stuck-background-thread")
        .await
        .expect("create thread");
    tokio::time::pause();
    let (_receiver, _cancel, generation) = service
        .connections
        .register("connection-stuck".into())
        .await;
    let subscription = generation.key().clone();
    let (resume_reply, resume_rx) = tokio::sync::oneshot::channel();
    managed
        .commands
        .send(ListenerCommand::Resume {
            subscription: subscription.clone(),
            include_turns: true,
            reply: resume_reply,
        })
        .unwrap();
    resume_rx.await.unwrap();
    managed
        .commands
        .send(ListenerCommand::ObservedCoreEvent(agent_protocol::Event {
            id: "turn-stuck".into(),
            msg: agent_protocol::EventMsg::TurnComplete(agent_protocol::TurnCompleteEvent {
                turn_id: "turn-stuck".into(),
                last_agent_message: Some("done".into()),
                error: None,
            }),
        }))
        .unwrap();
    let (unsubscribe_reply, unsubscribe_rx) = tokio::sync::oneshot::channel();
    managed
        .commands
        .send(ListenerCommand::Unsubscribe {
            subscription,
            reply: Some(unsubscribe_reply),
        })
        .unwrap();
    unsubscribe_rx.await.unwrap();
    assert!(managed.activity_rx.borrow().has_subscribers);
    drop(managed);
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    // Simulate a provider future that never resolves. The background lease must retain
    // the sink through the work timeout plus marker window, then expire independently
    // before the normal thirty-minute idle-unload window.
    tokio::time::advance(crate::BACKGROUND_EXTENSION_SINK_TIMEOUT).await;
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    let current = service
        .threads
        .get("stuck-background-thread")
        .await
        .expect("thread remains during idle window");
    assert!(!current.activity_rx.borrow().has_subscribers);
    drop(current);
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    tokio::time::advance(std::time::Duration::from_secs(30 * 60)).await;
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    assert!(!service.threads.contains("stuck-background-thread").await);
    assert!(service
        .thread_states
        .get("stuck-background-thread")
        .await
        .is_none());
}

#[tokio::test(start_paused = true)]
async fn bounded_background_phase_runs_finalizer_after_provider_timeout() {
    let finalized = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let finalized_for_task = Arc::clone(&finalized);
    let phase = tokio::spawn(run_bounded_post_turn_phase(
        std::future::pending::<Result<(), &'static str>>(),
        async move {
            finalized_for_task.store(true, std::sync::atomic::Ordering::SeqCst);
        },
    ));
    tokio::task::yield_now().await;
    tokio::time::advance(std::time::Duration::from_secs(5 * 60)).await;
    let outcome = phase.await.expect("phase supervisor");
    assert_eq!(outcome, BackgroundPhaseOutcome::TimedOut);
    assert!(finalized.load(std::sync::atomic::Ordering::SeqCst));
}

#[tokio::test]
async fn bounded_background_phase_finalizes_error_panic_and_cancellation() {
    let finalized = Arc::new(AtomicUsize::new(0));

    let error_finalized = Arc::clone(&finalized);
    let error = run_bounded_post_turn_phase(async { Err::<(), _>("provider") }, async move {
        error_finalized.fetch_add(1, Ordering::SeqCst);
    })
    .await;
    assert_eq!(error, BackgroundPhaseOutcome::Failed);

    let panic_finalized = Arc::clone(&finalized);
    let panicked = run_bounded_post_turn_phase(
        async {
            panic!("provider panic");
            #[allow(unreachable_code)]
            Ok::<(), &'static str>(())
        },
        async move {
            panic_finalized.fetch_add(1, Ordering::SeqCst);
        },
    )
    .await;
    assert_eq!(panicked, BackgroundPhaseOutcome::Panicked);

    let cancelled_task = tokio::spawn(std::future::pending::<Result<(), &'static str>>());
    cancelled_task.abort();
    let cancel_finalized = Arc::clone(&finalized);
    let cancelled = finish_bounded_post_turn_task(cancelled_task, async move {
        cancel_finalized.fetch_add(1, Ordering::SeqCst);
    })
    .await;
    assert_eq!(cancelled, BackgroundPhaseOutcome::Cancelled);
    assert_eq!(finalized.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn released_generation_background_finalizer_cannot_resurrect_thread_or_session() {
    let dir = TempDir::new().unwrap();
    memory::ensure_workspace(dir.path()).unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    let old = service
        .get_or_create_thread("released-background")
        .await
        .expect("old generation");
    let lifecycle = old.lifecycle_token();
    let finalized = Arc::new(AtomicBool::new(false));
    let finalized_by_task = Arc::clone(&finalized);
    let commands = old.commands.clone();
    let supervisor = tokio::spawn(async move {
        run_bounded_post_turn_phase_until_cancelled(
            std::future::pending::<Result<(), Status>>(),
            async move {
                let _ = commands.send(crate::ListenerCommand::ExpireBackgroundSink {
                    turn_id: "turn-old".into(),
                });
                finalized_by_task.store(true, Ordering::SeqCst);
            },
            lifecycle,
        )
        .await;
    });
    old.set_side_effect_supervisor(supervisor).await;

    let release_service = service.clone();
    let release = tokio::spawn(async move {
        release_service
            .release_session_runtime("released-background")
            .await
    });
    // 固定 16 次 yield 在并行负载下会误判：这里等释放真正结束，但仍远小于 30s marker 超时。
    let released = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !release.is_finished() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await;
    assert!(
        released.is_ok(),
        "explicit release must cancel work and expire the old sink without waiting for the 30s marker timeout"
    );
    release.await.expect("release task");
    assert!(!service.threads.contains("released-background").await);
    assert!(!service
        .sessions
        .read()
        .await
        .contains_key("released-background"));
    assert!(finalized.load(Ordering::SeqCst));
    assert!(old.side_effect_supervisor_is_finished().await);
    assert!(old.listener_is_finished().await);
}

#[tokio::test]
async fn old_background_finalizer_cannot_write_into_new_same_id_generation() {
    let dir = TempDir::new().unwrap();
    memory::ensure_workspace(dir.path()).unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    let old = service
        .get_or_create_thread("reused-background")
        .await
        .expect("old generation");
    service.release_session_runtime("reused-background").await;
    let current = service
        .get_or_create_thread("reused-background")
        .await
        .expect("new generation");
    assert!(!Arc::ptr_eq(&old, &current));

    let _ = service.emit_background_complete(&old, "turn-old").await;
    let state = service
        .thread_states
        .get("reused-background")
        .await
        .expect("new state");
    let state = state.lock().await;
    assert!(
        state
            .history
            .completed_turns()
            .iter()
            .all(|turn| turn.id != "turn-old"),
        "an old-generation marker must never mutate the replacement generation"
    );
}

#[tokio::test]
async fn cancelled_background_phase_materializes_completion_before_release() {
    let dir = TempDir::new().unwrap();
    memory::ensure_workspace(dir.path()).unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    let managed = service
        .get_or_create_thread("cancelled-background")
        .await
        .expect("thread generation");
    let lifecycle = managed.lifecycle_token();
    lifecycle.cancel();

    service
        .finalize_background_phase(&managed, "turn-cancelled")
        .await;

    let state = service
        .thread_states
        .get("cancelled-background")
        .await
        .expect("thread state");
    let state = state.lock().await;
    let marker = state
        .history
        .completed_turns()
        .iter()
        .find(|turn| turn.id == "turn-cancelled")
        .and_then(|turn| {
            turn.items.iter().find(|item| {
                matches!(
                    &item.item,
                    agent_protocol::TurnItem::Extension(extension)
                        if extension.namespace == "astro.background_complete"
                )
            })
        });
    assert!(
            marker.is_some(),
            "release cancellation must converge desktop background-pending state through the unified extension event"
        );
}

#[tokio::test]
async fn cancelled_blocked_extension_submissions_remove_all_registered_waiters() {
    let connections = ConnectionRegistry::default();
    let (commands, command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (activity_tx, _activity_rx) = tokio::sync::watch::channel(ThreadActivity {
        status: "idle".into(),
        has_subscribers: false,
    });
    let state = Arc::new(Mutex::new(ThreadState {
        status: "idle".into(),
        history: ThreadHistoryBuilder::default(),
        subscribers: Default::default(),
        background_extension_sinks: Default::default(),
        listener_command_tx: commands.clone(),
        activity_tx,
    }));
    tokio::spawn(crate::run_listener_commands(
        "waiter-cleanup".into(),
        state,
        command_rx,
        connections,
    ));

    let (blocked_submission_tx, _blocked_submission_rx) = tokio::sync::mpsc::channel(1);
    blocked_submission_tx
        .send(())
        .await
        .expect("fill bounded submission queue");
    for index in 0..16 {
        let (registration, _materialized) = register_extension_waiter(
            &commands,
            format!("item-{index}"),
            format!("payload-{index}"),
        )
        .expect("register waiter");
        let blocked_submission_tx = blocked_submission_tx.clone();
        let blocked = tokio::spawn(async move {
            let _registration = registration;
            blocked_submission_tx
                .send(())
                .await
                .expect("queue remains open");
        });
        tokio::task::yield_now().await;
        blocked.abort();
        let _ = blocked.await;
    }

    let (count_reply, count_rx) = tokio::sync::oneshot::channel();
    commands
        .send(ListenerCommand::ExtensionWaiterCount { reply: count_reply })
        .expect("inspect waiter count");
    assert_eq!(count_rx.await.expect("waiter count"), 0);
}

#[tokio::test]
async fn completed_snapshot_can_resubscribe_then_unload_after_explicit_unsubscribe() {
    let dir = TempDir::new().unwrap();
    memory::ensure_workspace(dir.path()).unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    let managed = service
        .get_or_create_thread("completed-thread")
        .await
        .expect("create thread");
    tokio::time::pause();
    let (_receiver, _cancel, generation) = service
        .connections
        .register("connection-completed".into())
        .await;
    let subscription = generation.key().clone();
    let (resume_reply, resume_rx) = tokio::sync::oneshot::channel();
    managed
        .commands
        .send(ListenerCommand::Resume {
            subscription: subscription.clone(),
            include_turns: true,
            reply: resume_reply,
        })
        .unwrap();
    resume_rx.await.unwrap();
    for msg in [
        agent_protocol::EventMsg::TurnStarted(agent_protocol::TurnStartedEvent {
            turn_id: "turn-completed".into(),
        }),
        agent_protocol::EventMsg::TurnComplete(agent_protocol::TurnCompleteEvent {
            turn_id: "turn-completed".into(),
            last_agent_message: Some("done".into()),
            error: None,
        }),
    ] {
        managed
            .commands
            .send(ListenerCommand::CoreEvent(agent_protocol::Event {
                id: "turn-completed".into(),
                msg,
            }))
            .unwrap();
    }
    let (unsubscribe_reply, unsubscribe_rx) = tokio::sync::oneshot::channel();
    managed
        .commands
        .send(ListenerCommand::Unsubscribe {
            subscription: subscription.clone(),
            reply: Some(unsubscribe_reply),
        })
        .unwrap();
    unsubscribe_rx.await.unwrap();

    let (resume_reply, resume_rx) = tokio::sync::oneshot::channel();
    managed
        .commands
        .send(ListenerCommand::Resume {
            subscription: subscription.clone(),
            include_turns: true,
            reply: resume_reply,
        })
        .unwrap();
    let snapshot = resume_rx.await.unwrap();
    assert_eq!(snapshot.turns.len(), 1);
    assert_eq!(snapshot.turns[0].status, "completed");
    let (unsubscribe_reply, unsubscribe_rx) = tokio::sync::oneshot::channel();
    managed
        .commands
        .send(ListenerCommand::Unsubscribe {
            subscription,
            reply: Some(unsubscribe_reply),
        })
        .unwrap();
    unsubscribe_rx.await.unwrap();
    drop(managed);
    tokio::time::sleep(std::time::Duration::from_millis(1)).await;

    tokio::time::advance(std::time::Duration::from_secs(30 * 60)).await;
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    assert!(!service.threads.contains("completed-thread").await);
}

#[tokio::test]
async fn reconcile_extensions_rejects_an_empty_session_id_without_side_effects() {
    let dir = TempDir::new().unwrap();
    memory::ensure_workspace(dir.path()).unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());

    let error = AstroService::reconcile_extensions(
        &service,
        Request::new(proto::ReconcileExtensionsRequest {
            session_id: "  ".into(),
        }),
    )
    .await
    .unwrap_err();

    assert_eq!(error.code(), tonic::Code::InvalidArgument);
    assert!(!service.threads.contains("").await);
}

#[tokio::test]
async fn thread_attachment_rpcs_round_trip_and_remove() {
    let dir = TempDir::new().unwrap();
    memory::ensure_workspace(dir.path()).unwrap();
    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    open_sessions(dir.path())
        .await
        .unwrap()
        .ensure_session("thread-attachments", "test")
        .await
        .unwrap();

    let added = AstroService::add_thread_attachment(
        &service,
        Request::new(proto::AddThreadAttachmentRequest {
            thread_id: "thread-attachments".into(),
            attachment_type: "workspace_file".into(),
            identity_key: "notes.md".into(),
            payload_json: serde_json::json!({"path":"notes.md"}).to_string(),
        }),
    )
    .await
    .unwrap()
    .into_inner();
    assert_eq!(added.outcome, "created");

    let listed = AstroService::list_thread_attachments(
        &service,
        Request::new(proto::ListThreadAttachmentsRequest {
            thread_id: "thread-attachments".into(),
            cursor: String::new(),
            limit: 10,
        }),
    )
    .await
    .unwrap()
    .into_inner();
    assert_eq!(listed.data.len(), 1);
    assert_eq!(listed.data[0].identity_key, "notes.md");

    let removed = AstroService::remove_thread_attachment(
        &service,
        Request::new(proto::RemoveThreadAttachmentRequest {
            thread_id: "thread-attachments".into(),
            attachment_type: "workspace_file".into(),
            identity_key: "notes.md".into(),
        }),
    )
    .await
    .unwrap()
    .into_inner();
    assert!(removed.removed);
    assert_eq!(removed.attachment.unwrap().identity_key, "notes.md");
}

#[test]
fn terminal_prefill_only_accepts_a_single_unexecuted_line() {
    assert_eq!(
        terminal_prefill_bytes("cat README.md\n"),
        Some(b"cat README.md".to_vec())
    );
    assert_eq!(terminal_prefill_bytes("  ls  "), Some(b"  ls  ".to_vec()));
    assert_eq!(terminal_prefill_bytes(""), None);
    assert_eq!(terminal_prefill_bytes("\n"), None);
    // 多行脚本不能预填：否则前几行会立刻执行。
    assert_eq!(terminal_prefill_bytes("rm -rf /\necho done"), None);
}
