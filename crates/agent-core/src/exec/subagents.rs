//! First-class subagent thread runner.

use std::sync::Arc;

use subagents::{
    AgentThreadCommand, AgentThreadControl, AgentThreadStatus, AgentThreadStore, LiveAgentThreads,
    SpawnAgentRequest,
};
use tokio::sync::mpsc;

use crate::runtime::{AgentConfig, AgentLoop, TurnResult};

pub async fn run_agent_thread(
    thread_id: String,
    request: SpawnAgentRequest,
    control: Arc<AgentThreadControl>,
    mut commands: mpsc::UnboundedReceiver<AgentThreadCommand>,
) -> anyhow::Result<()> {
    let store = AgentThreadStore::open_default()?;
    let mut agent = build_agent(&thread_id, &request)?;
    let targets = agent.chat_targets().to_vec();
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
            let result = run_turn(&mut agent, &targets, message, Arc::clone(&control)).await;
            if control.is_closed() {
                break;
            }
            if control.is_interrupted() {
                if request.interrupt_message {
                    let _ = agent.record_user_message(
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

fn build_agent(thread_id: &str, request: &SpawnAgentRequest) -> anyhow::Result<AgentLoop> {
    let memory_dir = home::default_memory_dir();
    let mut config = AgentConfig::with_defaults(memory_dir.clone());
    config.soul = format!(
        "{}\n\n## Subagent developer instructions\n{}",
        config.soul, request.developer_instructions
    );
    if let Some(effort) = request.model_reasoning_effort.as_deref() {
        config.additional_params = serde_json::json!({ "reasoning_effort": effort });
    }
    let mut agent = AgentLoop::with_session_id_for_agent(
        config,
        thread_id.to_string(),
        &request.parent_agent_id,
    )?;
    agent.set_project_root(request.project_root.clone());
    agent.set_permission_profile(sandbox_profile(request.sandbox_mode.as_deref()));
    if let Some(bus) = request.hook_bus.as_ref() {
        agent.set_hook_bus(Arc::clone(bus));
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
    agent.set_chat_targets(targets);
    Ok(agent)
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
    agent: &mut AgentLoop,
    targets: &[types::ChatTarget],
    message: String,
    control: Arc<AgentThreadControl>,
) -> anyhow::Result<String> {
    let turn = agent.run_turn(&message, "subagent-thread").await?;
    match turn {
        TurnResult::Finished(message) => Ok(message),
        TurnResult::Continue { system_prompt, .. } => {
            let (output, _) = crate::exec::headless::run_headless_multi_turn_controlled(
                agent,
                targets.to_vec(),
                system_prompt,
                Some(control),
            )
            .await?;
            Ok(output)
        }
        TurnResult::BudgetExhausted => anyhow::bail!("subagent turn budget exhausted"),
        TurnResult::MaxDepth => anyhow::bail!("subagent tool depth exhausted"),
        TurnResult::ToolCalls(_) | TurnResult::Interrupted => {
            anyhow::bail!("unsupported subagent turn result")
        }
    }
}
