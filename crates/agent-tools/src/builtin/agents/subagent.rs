//! Strict Codex V2 Agent Thread model tools.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::engine::execution::SpawnAgentDispatchRequest;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

pub const CODEX_V2_AGENT_TOOL_NAMES: [&str; 6] = [
    "spawn_agent",
    "list_agents",
    "send_message",
    "followup_task",
    "wait_agent",
    "interrupt_agent",
];

const WAIT_DEFAULT_MS: i64 = 30_000;
const WAIT_MIN_MS: i64 = 10_000;
const WAIT_MAX_MS: i64 = 3_600_000;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SpawnAgentArgs {
    task_name: String,
    message: String,
    #[serde(default)]
    agent_type: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    fork_turns: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListAgentsArgs {
    #[serde(default)]
    path_prefix: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MessageArgs {
    target: String,
    message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WaitAgentArgs {
    #[serde(default)]
    timeout_ms: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct InterruptAgentArgs {
    target: String,
}

pub fn register(registry: &mut ToolRegistry) {
    let lifecycle = ToolEntry::lifecycle_defaults;
    let entries = [
        (
            "spawn_agent",
            "Spawn an independent Codex V2 agent task under the current agent path.",
            schema_for_args::<SpawnAgentArgs>(),
            "bot",
        ),
        (
            "list_agents",
            "List the root Agent Thread tree, optionally below a canonical or relative path prefix.",
            schema_for_args::<ListAgentsArgs>(),
            "list-tree",
        ),
        (
            "send_message",
            "Queue a durable message for an existing agent without starting a new turn.",
            schema_for_args::<MessageArgs>(),
            "send",
        ),
        (
            "followup_task",
            "Queue a durable follow-up and start or steer the target agent turn.",
            schema_for_args::<MessageArgs>(),
            "send",
        ),
        (
            "wait_agent",
            "Wait for Agent Thread activity or a main-session steer.",
            schema_for_args::<WaitAgentArgs>(),
            "clock",
        ),
        (
            "interrupt_agent",
            "Interrupt a child agent's active turn and wait for runner acknowledgement.",
            schema_for_args::<InterruptAgentArgs>(),
            "circle-stop",
        ),
    ];
    for (name, description, schema, icon) in entries {
        registry.register(ToolEntry {
            name: name.into(),
            toolset: "subagents".into(),
            description: description.into(),
            schema,
            check_fn: None,
            icon,
            ..lifecycle()
        });
    }
}

crate::submit_builtin_tool! {
    register: register,
    names: [
        "spawn_agent",
        "list_agents",
        "send_message",
        "followup_task",
        "wait_agent",
        "interrupt_agent"
    ],
    async_named: handle,
}

async fn handle(
    ctx: &mut ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    let dispatch = ctx
        .execution
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no agent thread dispatch configured"))?;
    match name {
        "spawn_agent" => {
            let parsed: SpawnAgentArgs = parse(name, args)?;
            require_non_empty("task_name", &parsed.task_name)?;
            require_non_empty("message", &parsed.message)?;
            validate_fork_turns(parsed.fork_turns.as_deref())?;
            let parent_model = if ctx.credentials.provider.trim().is_empty()
                && ctx.credentials.model.trim().is_empty()
            {
                None
            } else {
                Some(format!(
                    "{}:{}",
                    ctx.credentials.provider, ctx.credentials.model
                ))
            };
            let request = SpawnAgentDispatchRequest {
                request: subagents::SpawnAgentV2Request {
                    task_name: parsed.task_name.trim().to_string(),
                    message: parsed.message.trim().to_string(),
                    agent_type: clean_optional(parsed.agent_type),
                    model: clean_optional(parsed.model),
                    reasoning_effort: clean_optional(parsed.reasoning_effort),
                    fork_turns: clean_optional(parsed.fork_turns),
                },
                memory_dir: ctx.memory_dir.clone(),
                parent_agent_id: ctx.memory.agent_id.clone(),
                parent_model,
                parent_sandbox_mode: current_sandbox_mode(ctx),
                inherited_skill_config: ctx.skill_config_overrides.to_vec(),
                chat_targets: ctx.chat_targets.to_vec(),
                project_root: ctx.project_root.clone(),
                hook_bus: ctx.hook_bus.clone(),
            };
            Ok(serde_json::to_string(
                &dispatch.spawn_agent(request).await?,
            )?)
        }
        "list_agents" => {
            let parsed: ListAgentsArgs = parse(name, args)?;
            Ok(serde_json::to_string(
                &dispatch
                    .list_agents(subagents::ListAgentsV2Request {
                        path_prefix: clean_optional(parsed.path_prefix),
                    })
                    .await?,
            )?)
        }
        "send_message" | "followup_task" => {
            let parsed: MessageArgs = parse(name, args)?;
            require_non_empty("target", &parsed.target)?;
            require_non_empty("message", &parsed.message)?;
            let request = subagents::MessageAgentV2Request {
                target: parsed.target.trim().to_string(),
                message: parsed.message.trim().to_string(),
            };
            let result = if name == "send_message" {
                dispatch.send_message(request).await?
            } else {
                dispatch.followup_task(request).await?
            };
            Ok(serde_json::to_string(&result)?)
        }
        "wait_agent" => {
            let parsed: WaitAgentArgs = parse(name, args)?;
            let timeout_ms = normalize_wait_timeout(parsed.timeout_ms)?;
            Ok(serde_json::to_string(
                &dispatch
                    .wait_agent(subagents::WaitAgentV2Request {
                        timeout_ms: Some(timeout_ms),
                    })
                    .await?,
            )?)
        }
        "interrupt_agent" => {
            let parsed: InterruptAgentArgs = parse(name, args)?;
            require_non_empty("target", &parsed.target)?;
            Ok(serde_json::to_string(
                &dispatch
                    .interrupt_agent(subagents::InterruptAgentV2Request {
                        target: parsed.target.trim().to_string(),
                    })
                    .await?,
            )?)
        }
        _ => anyhow::bail!("unknown agent thread tool: {name}"),
    }
}

fn parse<T: for<'de> Deserialize<'de>>(name: &str, args: &serde_json::Value) -> anyhow::Result<T> {
    serde_json::from_value(args.clone())
        .map_err(|error| anyhow::anyhow!("{name} arguments: {error}"))
}

fn require_non_empty(field: &str, value: &str) -> anyhow::Result<()> {
    if value.trim().is_empty() {
        anyhow::bail!("{field} cannot be empty");
    }
    Ok(())
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn validate_fork_turns(value: Option<&str>) -> anyhow::Result<()> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    if matches!(value, "all" | "none") {
        return Ok(());
    }
    let turns = value
        .parse::<usize>()
        .map_err(|_| anyhow::anyhow!("fork_turns must be all, none, or a positive integer"))?;
    if turns == 0 {
        anyhow::bail!("fork_turns must be all, none, or a positive integer");
    }
    Ok(())
}

fn normalize_wait_timeout(value: Option<i64>) -> anyhow::Result<i64> {
    let value = value.unwrap_or(WAIT_DEFAULT_MS);
    if value > WAIT_MAX_MS {
        anyhow::bail!("timeout_ms must not exceed {WAIT_MAX_MS}");
    }
    Ok(value.max(WAIT_MIN_MS))
}

fn current_sandbox_mode(ctx: &ToolContext<'_>) -> String {
    let profile = ctx.active_permission_profile_id();
    match profile.as_str() {
        types::READ_ONLY_PROFILE => "read-only".into(),
        types::WORKSPACE_PROFILE => "workspace-write".into(),
        types::DANGER_FULL_ACCESS_PROFILE => "danger-full-access".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn v2_tool_arguments_reject_legacy_aliases() {
        assert!(parse::<SpawnAgentArgs>("spawn_agent", &json!({ "task": "x" })).is_err());
        assert!(parse::<MessageArgs>(
            "send_message",
            &json!({
                "thread_id": "x", "message": "m"
            })
        )
        .is_err());
        assert!(parse::<WaitAgentArgs>("wait_agent", &json!({ "thread_ids": ["x"] })).is_err());
        assert!(parse::<InterruptAgentArgs>(
            "interrupt_agent",
            &json!({ "target": "x", "extra": true })
        )
        .is_err());
    }

    #[test]
    fn v2_tool_arguments_accept_only_canonical_shapes() {
        let spawn: SpawnAgentArgs = parse(
            "spawn_agent",
            &json!({
                "task_name": "review",
                "message": "review the patch",
                "agent_type": "reviewer",
                "model": "openai:gpt-5.6",
                "reasoning_effort": "high",
                "fork_turns": "2"
            }),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(spawn).unwrap(),
            json!({
                "task_name": "review",
                "message": "review the patch",
                "agent_type": "reviewer",
                "model": "openai:gpt-5.6",
                "reasoning_effort": "high",
                "fork_turns": "2"
            })
        );
        assert!(parse::<ListAgentsArgs>("list_agents", &json!({"path_prefix":"/root"})).is_ok());
        assert!(
            parse::<MessageArgs>("send_message", &json!({"target":"worker","message":"note"}))
                .is_ok()
        );
        assert!(parse::<WaitAgentArgs>("wait_agent", &json!({"timeout_ms":30000})).is_ok());
        assert!(
            parse::<InterruptAgentArgs>("interrupt_agent", &json!({"target":"worker"})).is_ok()
        );
    }

    #[test]
    fn wait_timeout_contract_is_bounded() {
        assert_eq!(normalize_wait_timeout(None).unwrap(), 30_000);
        assert_eq!(normalize_wait_timeout(Some(-1)).unwrap(), 10_000);
        assert_eq!(normalize_wait_timeout(Some(1)).unwrap(), 10_000);
        assert_eq!(normalize_wait_timeout(Some(3_600_000)).unwrap(), 3_600_000);
        assert!(normalize_wait_timeout(Some(3_600_001)).is_err());
        assert!(parse::<WaitAgentArgs>("wait_agent", &json!({"timeout_ms":"10"})).is_err());
    }

    #[test]
    fn fork_turns_rejects_zero_and_malformed_values() {
        for valid in [None, Some("all"), Some("none"), Some("1"), Some("25")] {
            validate_fork_turns(valid).unwrap();
        }
        for invalid in [Some("0"), Some("-1"), Some("1.5"), Some("recent")] {
            assert!(
                validate_fork_turns(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
    }

    #[test]
    fn schemas_have_exact_required_and_optional_fields() {
        fn fields(schema: &serde_json::Value) -> (Vec<String>, Vec<String>) {
            let mut properties = schema["properties"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>();
            let mut required = schema["required"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|value| value.as_str().unwrap().to_string())
                .collect::<Vec<_>>();
            properties.sort();
            required.sort();
            (properties, required)
        }

        let strings = |values: &[&str]| values.iter().map(|value| (*value).to_string()).collect();
        assert_eq!(
            fields(&schema_for_args::<SpawnAgentArgs>()),
            (
                strings(&[
                    "agent_type",
                    "fork_turns",
                    "message",
                    "model",
                    "reasoning_effort",
                    "task_name"
                ]),
                strings(&["message", "task_name"]),
            )
        );
        assert_eq!(
            fields(&schema_for_args::<ListAgentsArgs>()),
            (strings(&["path_prefix"]), vec![])
        );
        assert_eq!(
            fields(&schema_for_args::<MessageArgs>()),
            (
                strings(&["message", "target"]),
                strings(&["message", "target"]),
            )
        );
        assert_eq!(
            fields(&schema_for_args::<WaitAgentArgs>()),
            (strings(&["timeout_ms"]), vec![])
        );
        assert_eq!(
            fields(&schema_for_args::<InterruptAgentArgs>()),
            (strings(&["target"]), strings(&["target"]),)
        );
    }
}
