use agent::exec::dispatch::{
    CloseSubtreeError, DefaultDesktopAgentThreadControl, DesktopAgentThreadControl,
    DesktopFollowupContextUnavailable,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RootSessionArgs {
    pub root_session_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TargetArgs {
    pub root_session_id: String,
    pub target: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FollowupArgs {
    pub root_session_id: String,
    pub target: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDefinitionDto {
    pub name: String,
    pub description: String,
    pub model: Option<String>,
    pub model_reasoning_effort: Option<String>,
}

fn desktop_control() -> DefaultDesktopAgentThreadControl {
    DefaultDesktopAgentThreadControl::new(home::default_memory_dir())
}

fn command_error(operation: &str, error: anyhow::Error) -> String {
    if let Some(partial) = error.downcast_ref::<CloseSubtreeError>() {
        tracing::warn!(operation, "desktop Agent Thread command failed");
        return format!("agent subtree close stopped at {}", partial.failed_path());
    }
    if error
        .downcast_ref::<DesktopFollowupContextUnavailable>()
        .is_some()
    {
        tracing::warn!(
            operation,
            "desktop Agent Thread root context is unavailable"
        );
        return "Open or resume the root task before following up this Agent Thread.".into();
    }
    if error
        .downcast_ref::<subagents::LegacyRuntimeDescriptorUnavailable>()
        .is_some()
    {
        tracing::warn!(operation, "legacy Agent Thread cannot be recovered safely");
        return "This Agent Thread predates safe recovery. Create a new Agent Thread to continue."
            .into();
    }
    tracing::warn!(operation, "desktop Agent Thread command failed");
    format!("{operation} failed")
}

#[tauri::command]
pub async fn list_subagent_threads(
    args: RootSessionArgs,
) -> Result<subagents::AgentTreeSnapshotV2, String> {
    desktop_control()
        .snapshot(&args.root_session_id)
        .await
        .map_err(|error| command_error("list agent threads", error))
}

#[tauri::command]
pub async fn read_subagent_thread(
    args: TargetArgs,
) -> Result<subagents::AgentThreadDetailV2, String> {
    desktop_control()
        .read_thread(&args.root_session_id, &args.target)
        .await
        .map_err(|error| command_error("read agent thread", error))
}

/// The existing shell command name is retained, but V2 semantics are a
/// turn-triggering follow-up rather than the removed legacy resident channel.
#[tauri::command]
pub async fn send_subagent_message(args: FollowupArgs) -> Result<subagents::AgentThreadV2, String> {
    desktop_control()
        .followup(&args.root_session_id, &args.target, args.message)
        .await
        .map_err(|error| command_error("follow up agent thread", error))
}

#[tauri::command]
pub async fn interrupt_subagent_thread(
    args: TargetArgs,
) -> Result<subagents::InterruptAgentV2Result, String> {
    desktop_control()
        .interrupt(&args.root_session_id, &args.target)
        .await
        .map_err(|error| command_error("interrupt agent thread", error))
}

#[tauri::command]
pub async fn close_subagent_thread(
    args: TargetArgs,
) -> Result<subagents::AgentTreeSnapshotV2, String> {
    desktop_control()
        .close_subtree(&args.root_session_id, &args.target)
        .await
        .map_err(|error| command_error("close agent subtree", error))
}

#[tauri::command]
pub async fn list_subagent_definitions() -> Result<Vec<AgentDefinitionDto>, String> {
    let memory_dir = home::default_memory_dir();
    let project_root = worktree::resolve_project_root(None);
    let configuration = subagents::load_agent_configuration(&memory_dir, project_root.as_deref())
        .map_err(|error| command_error("load agent configuration", error))?;
    Ok(configuration
        .catalog
        .agents
        .into_values()
        .map(|definition| AgentDefinitionDto {
            name: definition.name,
            description: definition.description,
            model: definition.model,
            model_reasoning_effort: definition.model_reasoning_effort,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_desktop_arguments_reject_legacy_aliases() {
        assert!(serde_json::from_value::<TargetArgs>(serde_json::json!({
            "rootSessionId": "root",
            "target": "/root/worker"
        }))
        .is_ok());
        assert!(serde_json::from_value::<FollowupArgs>(serde_json::json!({
            "rootSessionId": "root",
            "target": "/root/worker",
            "message": "continue"
        }))
        .is_ok());

        for legacy in [
            serde_json::json!({"parentSessionId":"root","threadId":"child"}),
            serde_json::json!({"rootSessionId":"root","threadId":"child"}),
            serde_json::json!({"rootSessionId":"root","target":"/root/worker","includeClosed":true}),
        ] {
            assert!(serde_json::from_value::<TargetArgs>(legacy).is_err());
        }
        assert!(
            serde_json::from_value::<RootSessionArgs>(serde_json::json!({
                "rootSessionId": "root",
                "includeClosed": true
            }))
            .is_err()
        );
    }

    #[test]
    fn command_errors_do_not_reflect_sensitive_payloads() {
        let error = command_error(
            "read agent thread",
            anyhow::anyhow!("api_key=secret payload=user-private base_url=https://private"),
        );
        assert_eq!(error, "read agent thread failed");
        assert!(!error.contains("secret"));
        assert!(!error.contains("user-private"));
    }

    #[test]
    fn missing_root_context_is_actionable_without_reflecting_payloads() {
        let error = command_error(
            "follow up agent thread",
            anyhow::Error::new(DesktopFollowupContextUnavailable)
                .context("api_key=secret payload=user-private"),
        );
        assert_eq!(
            error,
            "Open or resume the root task before following up this Agent Thread."
        );
        assert!(!error.contains("secret"));
        assert!(!error.contains("user-private"));
    }

    #[test]
    fn legacy_descriptor_error_is_actionable_without_runtime_details() {
        let error = command_error(
            "follow up agent thread",
            anyhow::Error::new(subagents::LegacyRuntimeDescriptorUnavailable),
        );
        assert_eq!(
            error,
            "This Agent Thread predates safe recovery. Create a new Agent Thread to continue."
        );
    }
}
