//! First-class subagent thread runner.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use subagents::{
    AgentThreadCommand, AgentThreadControl, AgentThreadStatus, AgentThreadStore, LiveAgentThreads,
    SpawnAgentRequest,
};
use tokio::sync::{mpsc, Mutex};

use crate::runtime::{Config, Session, TurnResult};
use crate::streaming::ChatOverride;

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
                    );
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
    let turn = {
        let mut sess = session.lock().await;
        sess.start_or_steer_turn(&message, "subagent-thread")
            .await?
    };
    match turn {
        TurnResult::Finished(message) => Ok(message),
        TurnResult::Continue { system_prompt, .. } => {
            let (output, _) =
                crate::exec::background::run_background_multi_turn_controlled_with_chat(
                    Arc::clone(session),
                    targets.to_vec(),
                    system_prompt,
                    Some(control),
                    chat_override,
                )
                .await?;
            Ok(output)
        }
        TurnResult::BudgetExhausted => anyhow::bail!("subagent turn budget exhausted"),
        TurnResult::MaxDepth => anyhow::bail!("subagent tool depth exhausted"),
        TurnResult::Steered { .. } | TurnResult::ToolCalls(_) | TurnResult::Interrupted => {
            anyhow::bail!("unsupported subagent turn result")
        }
    }
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
}
