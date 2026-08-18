//! First-class subagent thread runner.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use session::ConversationStore;
use subagents::{
    AgentThreadCommand, AgentThreadControl, AgentThreadStatus, AgentThreadStore, LiveAgentThreads,
    SpawnAgentRequest,
};
use tokio::sync::{mpsc, Mutex};

use crate::runtime::{Config, Session};
use crate::streaming::ChatOverride;
use crate::tasks::TurnInput;

pub(crate) const MAILBOX_FINISH_PREFIX: &str = "agent-mailbox-through:";
const MAIN_STEER_PREFIX: &str = "astro-main-steer-v1:";

#[derive(serde::Deserialize, serde::Serialize)]
struct DurableSteerInput {
    content: String,
    image_data_urls: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MailboxDrainOutcome {
    pub(crate) delivered: usize,
    pub(crate) delivered_steer_ids: Vec<String>,
    pub(crate) deferred: bool,
}

pub(crate) fn encode_main_steer_input(input: &TurnInput) -> anyhow::Result<String> {
    let TurnInput::UserInput {
        content,
        image_data_urls,
    } = input;
    Ok(format!(
        "{MAIN_STEER_PREFIX}{}",
        serde_json::to_string(&DurableSteerInput {
            content: content.clone(),
            image_data_urls: image_data_urls.clone(),
        })?
    ))
}

/// Drain durable mailbox input only at a sampling boundary. A batch is
/// acknowledged after its structured user message has been accepted by
/// SessionStore and the in-memory history.
pub(crate) async fn drain_mailbox_at_safe_boundary(
    session: &mut Session,
) -> anyhow::Result<MailboxDrainOutcome> {
    if session.cancel_signal().is_cancelled() {
        anyhow::bail!("agent turn interrupted before mailbox drain");
    }
    let control = Arc::clone(&session.services.agent_control);
    let path = session.services.agent_path.clone();
    let messages = control.drain_mailbox(&path)?;
    if messages.is_empty() {
        return Ok(MailboxDrainOutcome::default());
    }
    if session.cancel_signal().is_cancelled() {
        anyhow::bail!("agent turn interrupted during mailbox drain");
    }
    let first_sequence = messages.first().expect("non-empty mailbox").sequence;
    let last_sequence = messages.last().expect("non-empty mailbox").sequence;
    let persisted_through = session
        .services
        .sessions
        .get_messages(session.session_id())?
        .iter()
        .filter_map(|message| {
            message
                .finish_reason
                .as_deref()?
                .strip_prefix(MAILBOX_FINISH_PREFIX)?
                .parse::<i64>()
                .ok()
        })
        .max();
    let runtime_history = session.clone_history().await;
    let in_memory_through = runtime_history
        .iter()
        .filter_map(|message| {
            message
                .compressed_content
                .as_deref()?
                .strip_prefix(MAILBOX_FINISH_PREFIX)?
                .parse::<i64>()
                .ok()
        })
        .max();
    let through_sequence = persisted_through
        .into_iter()
        .chain(in_memory_through)
        .filter(|value| *value >= first_sequence && *value <= last_sequence)
        .max()
        .unwrap_or(last_sequence);
    let delivered = messages
        .iter()
        .take_while(|message| message.sequence <= through_sequence)
        .count();
    let marker = format!("{MAILBOX_FINISH_PREFIX}{through_sequence}");
    let durable = session.ensure_durable_turn_input_marker(&marker)?;
    let in_memory = runtime_history
        .iter()
        .any(|message| message.compressed_content.as_deref() == Some(marker.as_str()));
    if !in_memory
        && runtime_history
            .last()
            .is_some_and(|message| message.role == types::message::Role::User)
    {
        return Ok(MailboxDrainOutcome {
            deferred: true,
            ..MailboxDrainOutcome::default()
        });
    }
    let mut contents = Vec::with_capacity(delivered);
    let mut image_data_urls = Vec::new();
    let mut delivered_steer_ids = Vec::new();
    for message in messages.into_iter().take(delivered) {
        if message.sender_thread_id == message.recipient_thread_id {
            if let Some(encoded) = message.payload.strip_prefix(MAIN_STEER_PREFIX) {
                let input: DurableSteerInput = serde_json::from_str(encoded)?;
                contents.push(input.content);
                image_data_urls.extend(input.image_data_urls);
                delivered_steer_ids.push(message.message_id);
                continue;
            }
        }
        contents.push(message.payload);
    }
    let content = contents.join("\n\n");
    let input = TurnInput::UserInput {
        content,
        image_data_urls,
    };
    if !durable {
        session.persist_turn_input(&input, Some(&marker), Some(&marker))?;
    }
    if session.cancel_signal().is_cancelled() {
        anyhow::bail!("agent turn interrupted after durable mailbox history write");
    }
    if !in_memory {
        session
            .record_turn_input_in_memory(&input, Some(&marker))
            .await;
    }
    if session.cancel_signal().is_cancelled() {
        anyhow::bail!("agent turn interrupted after in-memory mailbox history write");
    }
    control.ack_mailbox(&path, through_sequence)?;
    Ok(MailboxDrainOutcome {
        delivered,
        delivered_steer_ids,
        deferred: false,
    })
}

pub async fn run_agent_thread(
    thread_id: String,
    request: SpawnAgentRequest,
    control: Arc<AgentThreadControl>,
    commands: mpsc::UnboundedReceiver<AgentThreadCommand>,
) -> anyhow::Result<()> {
    let store = AgentThreadStore::open_default()?;
    run_agent_thread_inner(
        thread_id,
        request,
        control,
        commands,
        store,
        home::default_memory_dir(),
        None,
    )
    .await
}

async fn run_agent_thread_inner(
    thread_id: String,
    request: SpawnAgentRequest,
    control: Arc<AgentThreadControl>,
    mut commands: mpsc::UnboundedReceiver<AgentThreadCommand>,
    store: AgentThreadStore,
    memory_dir: PathBuf,
    chat_override: Option<ChatOverride>,
) -> anyhow::Result<()> {
    let agent = Arc::new(Mutex::new(build_agent(&thread_id, &request, &memory_dir)?));
    let targets = agent.lock().await.chat_targets().to_vec();
    if targets.is_empty() {
        anyhow::bail!("subagent thread has no chat target");
    }

    if let Some(bus) = request.hook_bus.as_ref() {
        let _ = bus.fire(
            hooks::SUBAGENT_START,
            &hooks::HookPayload {
                session_id: thread_id.clone(),
                detail: format!(
                    "parent_session_id={} agent={} task={}",
                    request.parent_session_id,
                    request.agent_name,
                    types::truncate_chars(&request.task, 200)
                ),
                ..Default::default()
            },
        );
    }

    let mut next_message = Some(initial_message(&request));
    loop {
        if control.is_closed() {
            break;
        }
        if let Some(message) = next_message.take() {
            control.begin_turn();
            store.set_status(&thread_id, AgentThreadStatus::Running, None, None)?;
            let result = run_turn(
                &agent,
                &targets,
                message,
                Arc::clone(&control),
                chat_override.clone(),
            )
            .await;
            if control.is_closed() {
                break;
            }
            if control.is_interrupted() {
                if request.interrupt_message {
                    let _ = agent.lock().await.record_user_message(
                        "[astro:system]\nThe previous agent turn was interrupted by the parent.",
                    )
                    .await;
                }
                store.set_status(
                    &thread_id,
                    AgentThreadStatus::Interrupted,
                    None,
                    Some("interrupted by parent"),
                )?;
            } else {
                match result {
                    Ok(output) => {
                        store.append_message(&thread_id, "assistant", &output)?;
                        store.set_status(
                            &thread_id,
                            AgentThreadStatus::Completed,
                            Some(&types::truncate_chars(&output, 8_000)),
                            None,
                        )?;
                    }
                    Err(error) => {
                        store.set_status(
                            &thread_id,
                            AgentThreadStatus::Failed,
                            None,
                            Some(&error.to_string()),
                        )?;
                    }
                }
            }
        }

        match commands.recv().await {
            Some(AgentThreadCommand::FollowUp(message)) => {
                record_follow_up(&store, &thread_id, &message)?;
                next_message = Some(message);
            }
            Some(AgentThreadCommand::Close) | None => break,
        }
    }

    store.set_status(&thread_id, AgentThreadStatus::Closed, None, None)?;
    LiveAgentThreads::global().remove(&thread_id);
    if let Some(bus) = request.hook_bus.as_ref() {
        let _ = bus.fire(
            hooks::SUBAGENT_STOP,
            &hooks::HookPayload {
                session_id: thread_id,
                detail: "agent thread closed".into(),
                ..Default::default()
            },
        );
    }
    Ok(())
}

pub(super) fn record_follow_up(
    store: &AgentThreadStore,
    thread_id: &str,
    message: &str,
) -> anyhow::Result<()> {
    store.append_message(thread_id, "user", message)
}

fn build_agent(
    thread_id: &str,
    request: &SpawnAgentRequest,
    memory_dir: &Path,
) -> anyhow::Result<Session> {
    let mut config = Config::with_defaults(memory_dir.to_path_buf());
    config.soul = format!(
        "{}\n\n## Subagent developer instructions\n{}",
        config.soul, request.developer_instructions
    );
    if let Some(effort) = request.model_reasoning_effort.as_deref() {
        config.additional_params = serde_json::json!({ "reasoning_effort": effort });
    }
    let mut session = Session::with_session_id_for_agent(
        config,
        thread_id.to_string(),
        &request.parent_agent_id,
    )?;
    session.set_project_root(request.project_root.clone());
    session.set_permission_profile(sandbox_profile(request.sandbox_mode.as_deref()));
    session.set_mcp_config_override(mcp::decode_inline_mcp_servers(&request.mcp_servers)?);
    session.set_skill_config_overrides(
        request
            .skills_config
            .iter()
            .map(|entry| (entry.path.clone(), entry.enabled))
            .collect(),
    );
    if let Some(bus) = request.hook_bus.as_ref() {
        session.set_hook_bus(Arc::clone(bus));
    }

    let mut targets = request.chat_targets.clone();
    if let Some(model) = request.model.as_deref() {
        match types::ModelSpec::parse(model) {
            Ok(spec) => {
                if let Some(primary) = targets.first_mut() {
                    *primary = spec.apply_to(primary);
                }
            }
            Err(error) => tracing::warn!(%error, model, "invalid subagent model override"),
        }
    }
    session.set_chat_targets(targets);
    Ok(session)
}

fn sandbox_profile(mode: Option<&str>) -> Option<String> {
    mode.map(|value| match value.trim().to_ascii_lowercase().as_str() {
        "read-only" | "read_only" => types::READ_ONLY_PROFILE.to_string(),
        "workspace-write" | "workspace_write" => types::WORKSPACE_PROFILE.to_string(),
        "danger-full-access" | "danger_full_access" => {
            types::DANGER_FULL_ACCESS_PROFILE.to_string()
        }
        other => other.to_string(),
    })
}

fn initial_message(request: &SpawnAgentRequest) -> String {
    if request.context_snapshot.trim().is_empty() {
        format!(
            "You are a subagent thread. Complete the delegated task and return a concise result to the parent.\n\n## Task\n{}",
            request.task
        )
    } else {
        format!(
            "You are a subagent thread. Complete the delegated task and return a concise result to the parent.\n\n## Task\n{}\n\n## Forked parent context\n{}",
            request.task, request.context_snapshot
        )
    }
}

async fn run_turn(
    session: &Arc<Mutex<Session>>,
    targets: &[types::ChatTarget],
    message: String,
    control: Arc<AgentThreadControl>,
    chat_override: Option<ChatOverride>,
) -> anyhow::Result<String> {
    let (output, _) = crate::exec::background::run_background_multi_turn_controlled_with_chat(
        Arc::clone(session),
        targets.to_vec(),
        vec![TurnInput::UserInput {
            content: message,
            image_data_urls: Vec::new(),
        }],
        Some(control),
        chat_override,
    )
    .await?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex as StdMutex;
    use std::time::Duration;

    use futures::stream;
    use providers::types::stream::StreamChunk;
    use providers::CompletionStream;

    use super::*;

    fn spawn_request(project_root: PathBuf) -> SpawnAgentRequest {
        SpawnAgentRequest {
            parent_session_id: "parent-session".into(),
            parent_agent_id: "parent-agent".into(),
            task: "inspect the runner".into(),
            agent_name: "runner-test".into(),
            developer_instructions: "Return the scripted result.".into(),
            context_snapshot: String::new(),
            model: None,
            model_reasoning_effort: None,
            sandbox_mode: Some("read-only".into()),
            mcp_servers: Default::default(),
            skills_config: Vec::new(),
            chat_targets: vec![types::ChatTarget {
                provider_id: "test".into(),
                backend_id: "openai".into(),
                model: "test-model".into(),
                api_key: "test-key".into(),
                base_url: "http://127.0.0.1.invalid".into(),
            }],
            project_root: Some(project_root),
            hook_bus: None,
            interrupt_message: true,
        }
    }

    fn scripted_chat(replies: &[&str]) -> ChatOverride {
        let replies = Arc::new(StdMutex::new(
            replies
                .iter()
                .map(|reply| (*reply).to_string())
                .collect::<VecDeque<_>>(),
        ));
        Arc::new(move |_messages, _tools, _config| {
            let reply = replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected extra provider call");
            Box::pin(async move {
                Ok(Box::pin(stream::iter(vec![
                    Ok(StreamChunk::Text(reply)),
                    Ok(StreamChunk::Done {
                        finish_reason: "stop".into(),
                    }),
                ])) as CompletionStream)
            })
        })
    }

    async fn wait_for_message_count(store: &AgentThreadStore, thread_id: &str, expected: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if store.messages(thread_id).unwrap().len() >= expected {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("runner did not persist messages before timeout");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runner_completes_follow_up_and_close_lifecycle() {
        let temp = tempfile::tempdir().unwrap();
        let store = AgentThreadStore::new(temp.path().join("subagents.db")).unwrap();
        let request = spawn_request(temp.path().to_path_buf());
        let thread = store.create(&request).unwrap();
        let registry = LiveAgentThreads::global();
        let (control, commands) = registry
            .register_bounded(&thread.id, &request.parent_session_id, 1)
            .unwrap();
        let thread_id = thread.id.clone();
        let runner_store = store.clone();
        let memory_dir = temp.path().join("agent-home");

        tokio::task::LocalSet::new()
            .run_until(async move {
                let runner = tokio::task::spawn_local(run_agent_thread_inner(
                    thread_id.clone(),
                    request,
                    control,
                    commands,
                    runner_store,
                    memory_dir,
                    Some(scripted_chat(&["first result", "follow-up result"])),
                ));

                wait_for_message_count(&store, &thread_id, 2).await;
                let completed = store.get(&thread_id).unwrap().unwrap();
                assert_eq!(completed.status, AgentThreadStatus::Completed);
                assert_eq!(completed.summary.as_deref(), Some("first result"));

                registry
                    .send_follow_up(&thread_id, "check the tests".into())
                    .unwrap();
                wait_for_message_count(&store, &thread_id, 4).await;
                let messages = store.messages(&thread_id).unwrap();
                let transcript = messages
                    .iter()
                    .map(|message| (message.role.as_str(), message.content.as_str()))
                    .collect::<Vec<_>>();
                assert_eq!(
                    transcript,
                    vec![
                        ("user", "inspect the runner"),
                        ("assistant", "first result"),
                        ("user", "check the tests"),
                        ("assistant", "follow-up result"),
                    ]
                );
                let completed = store.get(&thread_id).unwrap().unwrap();
                assert_eq!(completed.status, AgentThreadStatus::Completed);
                assert_eq!(completed.summary.as_deref(), Some("follow-up result"));

                registry.close(&thread_id).unwrap();
                tokio::time::timeout(Duration::from_secs(5), runner)
                    .await
                    .expect("runner did not close before timeout")
                    .expect("runner task panicked")
                    .expect("runner returned an error");

                assert!(!registry.is_live(&thread_id));
                assert_eq!(
                    store.get(&thread_id).unwrap().unwrap().status,
                    AgentThreadStatus::Closed
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mailbox_is_acked_only_after_safe_boundary_history_acceptance() {
        let temp = tempfile::tempdir().unwrap();
        let graph = subagents::AgentGraphStore::open(temp.path().join("agents-v2.db")).unwrap();
        let root = subagents::AgentControl::open(
            "root-v2".into(),
            graph.clone(),
            subagents::Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .unwrap();
        let reservation = root
            .reserve_spawn(&subagents::AgentPath::root(), "worker")
            .unwrap();
        let thread = reservation.thread().clone();
        reservation.commit().unwrap();
        root.enqueue_message(
            &subagents::AgentPath::root(),
            subagents::MessageAgentV2Request {
                target: "worker".into(),
                message: "follow up safely".into(),
            },
            true,
        )
        .unwrap();
        root.enqueue_message(
            &subagents::AgentPath::root(),
            subagents::MessageAgentV2Request {
                target: "worker".into(),
                message: "and keep tool rows".into(),
            },
            true,
        )
        .unwrap();

        let memory_dir = temp.path().join("memory");
        let interrupted = Session::with_session_id_for_agent_thread(
            Config::with_defaults(memory_dir.clone()),
            thread.session_id.clone(),
            home::DEFAULT_AGENT_ID,
            Arc::clone(&root),
            thread.canonical_path.clone(),
        )
        .unwrap();
        interrupted.cancel_signal().cancel();
        assert!(drain_mailbox_at_safe_boundary(&mut { interrupted })
            .await
            .is_err());
        assert_eq!(graph.pending_for(&thread.thread_id, 0).unwrap().len(), 2);

        let mut retry = Session::with_session_id_for_agent_thread(
            Config::with_defaults(memory_dir),
            thread.session_id.clone(),
            home::DEFAULT_AGENT_ID,
            Arc::clone(&root),
            thread.canonical_path,
        )
        .unwrap();
        assert_eq!(
            drain_mailbox_at_safe_boundary(&mut retry)
                .await
                .unwrap()
                .delivered,
            2
        );
        assert!(graph.pending_for(&thread.thread_id, 0).unwrap().is_empty());
        assert_eq!(
            drain_mailbox_at_safe_boundary(&mut retry)
                .await
                .unwrap()
                .delivered,
            0
        );
        let history = retry.clone_history().await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].role, types::message::Role::User);
        assert_eq!(
            history[0].content_str(),
            "follow up safely\n\nand keep tool rows"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mailbox_atomic_history_write_survives_cancel_and_restart() {
        let temp = tempfile::tempdir().unwrap();
        let graph = subagents::AgentGraphStore::open(temp.path().join("agents-v2.db")).unwrap();
        let root = subagents::AgentControl::open(
            "root-v2".into(),
            graph.clone(),
            subagents::Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .unwrap();
        let reservation = root
            .reserve_spawn(&subagents::AgentPath::root(), "worker")
            .unwrap();
        let thread = reservation.thread().clone();
        reservation.commit().unwrap();
        root.enqueue_message(
            &subagents::AgentPath::root(),
            subagents::MessageAgentV2Request {
                target: "worker".into(),
                message: "persisted before ack".into(),
            },
            true,
        )
        .unwrap();
        let sequence = graph.pending_for(&thread.thread_id, 0).unwrap()[0].sequence;
        let marker = format!("{MAILBOX_FINISH_PREFIX}{sequence}");
        let memory_dir = temp.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions
            .create_session(&thread.session_id, "tauri", None, None, None)
            .unwrap();
        let mut retry = Session::with_session_id_for_agent_thread(
            Config::with_defaults(memory_dir.clone()),
            thread.session_id.clone(),
            home::DEFAULT_AGENT_ID,
            Arc::clone(&root),
            thread.canonical_path.clone(),
        )
        .unwrap();
        let cancel = retry.cancel_signal();
        retry.set_turn_input_after_db_write_hook(Some(Arc::new(move || {
            cancel.cancel();
            anyhow::bail!("failpoint after durable mailbox write")
        })));

        assert!(drain_mailbox_at_safe_boundary(&mut retry).await.is_err());
        let stored = sessions.get_messages(&thread.session_id).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].finish_reason.as_deref(), Some(marker.as_str()));
        assert_eq!(
            stored[0].compressed_content.as_deref(),
            Some(marker.as_str())
        );
        assert!(retry.clone_history().await.is_empty());
        assert_eq!(graph.pending_for(&thread.thread_id, 0).unwrap().len(), 1);
        drop(retry);

        let mut retry = Session::with_session_id_for_agent_thread(
            Config::with_defaults(memory_dir),
            thread.session_id.clone(),
            home::DEFAULT_AGENT_ID,
            Arc::clone(&root),
            thread.canonical_path,
        )
        .unwrap();
        assert_eq!(retry.clone_history().await.len(), 1);

        assert_eq!(
            drain_mailbox_at_safe_boundary(&mut retry)
                .await
                .unwrap()
                .delivered,
            1
        );
        assert!(graph.pending_for(&thread.thread_id, 0).unwrap().is_empty());
        assert_eq!(sessions.get_messages(&thread.session_id).unwrap().len(), 1);
        assert_eq!(retry.clone_history().await.len(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mailbox_recovers_a_crash_after_user_insert_before_marker_update() {
        let temp = tempfile::tempdir().unwrap();
        let graph = subagents::AgentGraphStore::open(temp.path().join("agents-v2.db")).unwrap();
        let root = subagents::AgentControl::open(
            "root-v2".into(),
            graph.clone(),
            subagents::Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .unwrap();
        let reservation = root
            .reserve_spawn(&subagents::AgentPath::root(), "worker")
            .unwrap();
        let thread = reservation.thread().clone();
        reservation.commit().unwrap();
        root.enqueue_message(
            &subagents::AgentPath::root(),
            subagents::MessageAgentV2Request {
                target: "worker".into(),
                message: "committed before marker update".into(),
            },
            true,
        )
        .unwrap();
        let sequence = graph.pending_for(&thread.thread_id, 0).unwrap()[0].sequence;
        let marker = format!("{MAILBOX_FINISH_PREFIX}{sequence}");
        let memory_dir = temp.path().join("memory");
        {
            let sessions =
                session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
            sessions
                .create_session(&thread.session_id, "tauri", None, None, None)
                .unwrap();
            sessions
                .append_message(session::NewMessage {
                    content: Some("committed before marker update"),
                    finish_reason: Some(&marker),
                    ..session::NewMessage::empty(&thread.session_id, "user")
                })
                .unwrap();
            let stored = sessions.get_messages(&thread.session_id).unwrap();
            assert_eq!(stored.len(), 1);
            assert!(stored[0].compressed_content.is_none());
        }

        let mut retry = Session::with_session_id_for_agent_thread(
            Config::with_defaults(memory_dir.clone()),
            thread.session_id.clone(),
            home::DEFAULT_AGENT_ID,
            Arc::clone(&root),
            thread.canonical_path,
        )
        .unwrap();
        assert_eq!(retry.clone_history().await.len(), 1);

        assert_eq!(
            drain_mailbox_at_safe_boundary(&mut retry)
                .await
                .unwrap()
                .delivered,
            1
        );
        assert!(graph.pending_for(&thread.thread_id, 0).unwrap().is_empty());
        assert_eq!(
            drain_mailbox_at_safe_boundary(&mut retry)
                .await
                .unwrap()
                .delivered,
            0
        );
        let stored = session::SessionStore::open_sessions_dir(&memory_dir.join("sessions"))
            .unwrap()
            .get_messages(&thread.session_id)
            .unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(
            stored[0].compressed_content.as_deref(),
            Some(marker.as_str())
        );
        let history = retry.clone_history().await;
        assert_eq!(history.len(), 1);
        assert_eq!(
            history[0].compressed_content.as_deref(),
            Some(marker.as_str())
        );
        let provider = crate::prompt::messages::to_provider_messages("", &history);
        assert_eq!(
            serde_json::to_string(&provider)
                .unwrap()
                .matches("committed before marker update")
                .count(),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn main_steer_recovers_a_finish_only_row_with_media() {
        let temp = tempfile::tempdir().unwrap();
        let graph = subagents::AgentGraphStore::open(temp.path().join("agents-v2.db")).unwrap();
        let root = subagents::AgentControl::open(
            "root-v2".into(),
            graph.clone(),
            subagents::Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .unwrap();
        let image = "data:image/png;base64,bGVnYWN5LXN0ZWVy";
        root.persist_main_steer(
            &subagents::AgentPath::root(),
            encode_main_steer_input(&TurnInput::UserInput {
                content: "legacy steer".into(),
                image_data_urls: vec![image.into()],
            })
            .unwrap(),
        )
        .unwrap();
        let sequence = graph.pending_for("root-v2", 0).unwrap()[0].sequence;
        let marker = format!("{MAILBOX_FINISH_PREFIX}{sequence}");
        let media_json = serde_json::to_string(&vec![types::MediaAsset::data_url(
            types::MediaKind::Image,
            image,
            "image/png",
        )])
        .unwrap();
        let memory_dir = temp.path().join("memory");
        {
            let sessions =
                session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
            sessions
                .create_session("root-v2", "tauri", None, None, None)
                .unwrap();
            sessions
                .append_message(session::NewMessage {
                    content: Some("legacy steer"),
                    finish_reason: Some(&marker),
                    media_json: Some(&media_json),
                    ..session::NewMessage::empty("root-v2", "user")
                })
                .unwrap();
        }
        let mut retry = Session::with_session_id_for_agent_thread(
            Config::with_defaults(memory_dir.clone()),
            "root-v2".into(),
            home::DEFAULT_AGENT_ID,
            Arc::clone(&root),
            subagents::AgentPath::root(),
        )
        .unwrap();

        assert_eq!(
            drain_mailbox_at_safe_boundary(&mut retry)
                .await
                .unwrap()
                .delivered,
            1
        );
        assert!(graph.pending_for("root-v2", 0).unwrap().is_empty());
        assert_eq!(
            drain_mailbox_at_safe_boundary(&mut retry)
                .await
                .unwrap()
                .delivered,
            0
        );
        let stored = session::SessionStore::open_sessions_dir(&memory_dir.join("sessions"))
            .unwrap()
            .get_messages("root-v2")
            .unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(
            stored[0].compressed_content.as_deref(),
            Some(marker.as_str())
        );
        let history = retry.clone_history().await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content_str(), "legacy steer");
        assert_eq!(history[0].media.len(), 1);
        assert_eq!(
            history[0].compressed_content.as_deref(),
            Some(marker.as_str())
        );
        let provider = crate::prompt::messages::to_provider_messages("", &history);
        assert_eq!(
            serde_json::to_string(&provider)
                .unwrap()
                .matches("legacy steer")
                .count(),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mailbox_retry_repairs_a_memory_only_delivery_before_ack() {
        let temp = tempfile::tempdir().unwrap();
        let graph = subagents::AgentGraphStore::open(temp.path().join("agents-v2.db")).unwrap();
        let root = subagents::AgentControl::open(
            "root-v2".into(),
            graph.clone(),
            subagents::Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .unwrap();
        let reservation = root
            .reserve_spawn(&subagents::AgentPath::root(), "worker")
            .unwrap();
        let thread = reservation.thread().clone();
        reservation.commit().unwrap();
        root.enqueue_message(
            &subagents::AgentPath::root(),
            subagents::MessageAgentV2Request {
                target: "worker".into(),
                message: "memory only follow up".into(),
            },
            true,
        )
        .unwrap();
        let sequence = graph.pending_for(&thread.thread_id, 0).unwrap()[0].sequence;
        let marker = format!("agent-mailbox-through:{sequence}");
        let memory_dir = temp.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions
            .create_session(&thread.session_id, "tauri", None, None, None)
            .unwrap();
        let mut retry = Session::with_session_id_for_agent_thread(
            Config::with_defaults(memory_dir),
            thread.session_id.clone(),
            home::DEFAULT_AGENT_ID,
            Arc::clone(&root),
            thread.canonical_path,
        )
        .unwrap();
        let mut memory_only = types::message::Message::user("memory only follow up");
        memory_only.compressed_content = Some(marker);
        retry.record_items(vec![memory_only]).await;
        root.enqueue_message(
            &subagents::AgentPath::root(),
            subagents::MessageAgentV2Request {
                target: "worker".into(),
                message: "arrived after the partial delivery".into(),
            },
            true,
        )
        .unwrap();

        assert_eq!(
            drain_mailbox_at_safe_boundary(&mut retry)
                .await
                .unwrap()
                .delivered,
            1
        );
        assert_eq!(graph.pending_for(&thread.thread_id, 0).unwrap().len(), 1);
        assert_eq!(sessions.get_messages(&thread.session_id).unwrap().len(), 1);
        let history = retry.clone_history().await;
        assert_eq!(history.len(), 1);
        let provider = crate::prompt::messages::to_provider_messages("", &history);
        assert_eq!(
            serde_json::to_string(&provider)
                .unwrap()
                .matches("memory only follow up")
                .count(),
            1
        );
        assert_eq!(
            drain_mailbox_at_safe_boundary(&mut retry)
                .await
                .unwrap()
                .delivered,
            0
        );
        assert_eq!(graph.pending_for(&thread.thread_id, 0).unwrap().len(), 1);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mailbox_retry_with_both_sides_present_only_acks_once() {
        let temp = tempfile::tempdir().unwrap();
        let graph = subagents::AgentGraphStore::open(temp.path().join("agents-v2.db")).unwrap();
        let root = subagents::AgentControl::open(
            "root-v2".into(),
            graph.clone(),
            subagents::Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .unwrap();
        let reservation = root
            .reserve_spawn(&subagents::AgentPath::root(), "worker")
            .unwrap();
        let thread = reservation.thread().clone();
        reservation.commit().unwrap();
        root.enqueue_message(
            &subagents::AgentPath::root(),
            subagents::MessageAgentV2Request {
                target: "worker".into(),
                message: "already on both sides".into(),
            },
            true,
        )
        .unwrap();
        let memory_dir = temp.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions
            .create_session(&thread.session_id, "tauri", None, None, None)
            .unwrap();
        let mut retry = Session::with_session_id_for_agent_thread(
            Config::with_defaults(memory_dir),
            thread.session_id.clone(),
            home::DEFAULT_AGENT_ID,
            Arc::clone(&root),
            thread.canonical_path,
        )
        .unwrap();
        let cancel = retry.cancel_signal();
        retry.set_turn_input_after_memory_write_hook(Some(Arc::new(move || cancel.cancel())));

        assert!(drain_mailbox_at_safe_boundary(&mut retry).await.is_err());
        assert_eq!(sessions.get_messages(&thread.session_id).unwrap().len(), 1);
        assert_eq!(retry.clone_history().await.len(), 1);
        assert_eq!(graph.pending_for(&thread.thread_id, 0).unwrap().len(), 1);
        retry.set_turn_input_after_memory_write_hook(None);
        retry.cancel_signal().reset();

        assert_eq!(
            drain_mailbox_at_safe_boundary(&mut retry)
                .await
                .unwrap()
                .delivered,
            1
        );
        assert!(graph.pending_for(&thread.thread_id, 0).unwrap().is_empty());
        assert_eq!(sessions.get_messages(&thread.session_id).unwrap().len(), 1);
        let history = retry.clone_history().await;
        assert_eq!(history.len(), 1);
        assert_eq!(
            serde_json::to_string(&crate::prompt::messages::to_provider_messages("", &history))
                .unwrap()
                .matches("already on both sides")
                .count(),
            1
        );
        assert_eq!(
            drain_mailbox_at_safe_boundary(&mut retry)
                .await
                .unwrap()
                .delivered,
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn main_steer_retry_repairs_db_only_delivery_with_media() {
        let temp = tempfile::tempdir().unwrap();
        let graph = subagents::AgentGraphStore::open(temp.path().join("agents-v2.db")).unwrap();
        let root = subagents::AgentControl::open(
            "root-v2".into(),
            graph.clone(),
            subagents::Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .unwrap();
        root.persist_main_steer(
            &subagents::AgentPath::root(),
            encode_main_steer_input(&TurnInput::UserInput {
                content: "steer through failure".into(),
                image_data_urls: vec!["data:image/png;base64,c3RlZXI=".into()],
            })
            .unwrap(),
        )
        .unwrap();
        let memory_dir = temp.path().join("memory");
        let sessions =
            session::SessionStore::open_sessions_dir(&memory_dir.join("sessions")).unwrap();
        sessions
            .create_session("root-v2", "tauri", None, None, None)
            .unwrap();
        let mut retry = Session::with_session_id_for_agent_thread(
            Config::with_defaults(memory_dir),
            "root-v2".into(),
            home::DEFAULT_AGENT_ID,
            Arc::clone(&root),
            subagents::AgentPath::root(),
        )
        .unwrap();
        let cancel = retry.cancel_signal();
        retry.set_turn_input_after_db_write_hook(Some(Arc::new(move || {
            cancel.cancel();
            anyhow::bail!("steer failpoint after DB write")
        })));

        assert!(drain_mailbox_at_safe_boundary(&mut retry).await.is_err());
        assert_eq!(sessions.get_messages("root-v2").unwrap().len(), 1);
        assert!(retry.clone_history().await.is_empty());
        assert_eq!(graph.pending_for("root-v2", 0).unwrap().len(), 1);
        retry.set_turn_input_after_db_write_hook(None);
        retry.cancel_signal().reset();

        assert_eq!(
            drain_mailbox_at_safe_boundary(&mut retry)
                .await
                .unwrap()
                .delivered,
            1
        );
        assert!(graph.pending_for("root-v2", 0).unwrap().is_empty());
        let history = retry.clone_history().await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content_str(), "steer through failure");
        assert_eq!(history[0].media.len(), 1);
        assert_eq!(
            drain_mailbox_at_safe_boundary(&mut retry)
                .await
                .unwrap()
                .delivered,
            0
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn mailbox_waits_until_the_current_user_turn_has_an_assistant_boundary() {
        let temp = tempfile::tempdir().unwrap();
        let graph = subagents::AgentGraphStore::open(temp.path().join("agents-v2.db")).unwrap();
        let root = subagents::AgentControl::open(
            "root-v2".into(),
            graph.clone(),
            subagents::Limits {
                max_threads: 8,
                max_depth: 4,
                max_running: 2,
            },
        )
        .unwrap();
        root.persist_main_steer(
            &subagents::AgentPath::root(),
            encode_main_steer_input(&TurnInput::UserInput {
                content: "early steer".into(),
                image_data_urls: Vec::new(),
            })
            .unwrap(),
        )
        .unwrap();
        let mut session = Session::with_session_id_for_agent_thread(
            Config::with_defaults(temp.path().join("memory")),
            "root-v2".into(),
            home::DEFAULT_AGENT_ID,
            Arc::clone(&root),
            subagents::AgentPath::root(),
        )
        .unwrap();
        session
            .record_items(vec![types::message::Message::user("initial")])
            .await;

        let deferred = drain_mailbox_at_safe_boundary(&mut session).await.unwrap();
        assert_eq!(deferred.delivered, 0);
        assert!(deferred.deferred);
        assert_eq!(graph.pending_for("root-v2", 0).unwrap().len(), 1);

        session
            .record_items(vec![types::message::Message::assistant("first answer")])
            .await;
        let delivered = drain_mailbox_at_safe_boundary(&mut session).await.unwrap();
        assert_eq!(delivered.delivered, 1);
        assert_eq!(delivered.delivered_steer_ids.len(), 1);
        assert!(!delivered.deferred);
        assert!(graph.pending_for("root-v2", 0).unwrap().is_empty());
        assert!(crate::runtime::validate_message_order(
            &session.clone_history().await
        ));
    }
}
