//! Codex-compatible command hook configuration and execution.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::task::JoinSet;
use tracing::warn;

use crate::{HookEvent, HookPayload};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);
const SESSION_END_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_SESSION_END_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HooksFile {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub hooks: HashMap<String, Vec<MatcherGroup>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MatcherGroup {
    #[serde(default)]
    pub matcher: Option<String>,
    #[serde(default)]
    pub hooks: Vec<HookHandlerConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum HookHandlerConfig {
    #[serde(rename = "command")]
    Command {
        command: String,
        #[serde(default, rename = "commandWindows", alias = "command_windows")]
        command_windows: Option<String>,
        #[serde(default, rename = "timeout")]
        timeout_sec: Option<u64>,
        #[serde(default)]
        r#async: bool,
        #[serde(default, rename = "statusMessage")]
        status_message: Option<String>,
    },
    #[serde(rename = "prompt")]
    Prompt {},
    #[serde(rename = "agent")]
    Agent {},
    #[serde(rename = "mcp_tool")]
    McpTool {
        server: String,
        tool: String,
        #[serde(default)]
        input: serde_json::Map<String, Value>,
        #[serde(default, rename = "timeout")]
        timeout_sec: Option<u64>,
        #[serde(default, rename = "statusMessage")]
        status_message: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionVote {
    Allow,
    Deny,
}

#[derive(Debug, Clone, Default)]
pub struct CommandHookDecision {
    pub block_reason: Option<String>,
    pub permission: Option<PermissionVote>,
    pub updated_input: Option<Value>,
    pub additional_context: Option<String>,
    pub feedback: Option<String>,
    pub keep_going: Option<String>,
}

#[derive(Debug, Clone)]
struct ConfiguredCommand {
    matcher: Option<Regex>,
    command: String,
    timeout: Duration,
    asynchronous: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CommandHookRunner {
    handlers: HashMap<String, Vec<ConfiguredCommand>>,
}

impl CommandHookRunner {
    pub fn load(root: &Path) -> anyhow::Result<Self> {
        let path = root.join("hooks.json");
        if !path.is_file() {
            return Ok(Self::default());
        }
        let source = std::fs::read_to_string(&path)?;
        let file: HooksFile = serde_json::from_str(&source)?;
        Self::from_file(file, &path)
    }

    pub fn from_file(file: HooksFile, source: &Path) -> anyhow::Result<Self> {
        let mut handlers = HashMap::new();
        for (event, groups) in file.hooks {
            if !HookEvent::CODEX
                .iter()
                .any(|candidate| candidate.as_str() == event)
            {
                warn!(%event, source = %source.display(), "ignoring unsupported command hook event");
                continue;
            }
            for group in groups {
                let matcher = match compile_matcher(group.matcher.as_deref()) {
                    Ok(matcher) => matcher,
                    Err(error) => {
                        warn!(%event, %error, source = %source.display(), "invalid hook matcher group disabled");
                        continue;
                    }
                };
                for handler in group.hooks {
                    let HookHandlerConfig::Command {
                        command,
                        command_windows,
                        timeout_sec,
                        r#async,
                        status_message: _,
                    } = handler
                    else {
                        warn!(%event, source = %source.display(), "hook handler type is parsed but not executable");
                        continue;
                    };
                    #[cfg(windows)]
                    let command = command_windows.unwrap_or(command);
                    #[cfg(not(windows))]
                    let _ = command_windows;
                    let requested = timeout_sec.map(Duration::from_secs);
                    let timeout = if event == crate::SESSION_END {
                        requested
                            .unwrap_or(SESSION_END_TIMEOUT)
                            .min(MAX_SESSION_END_TIMEOUT)
                    } else {
                        requested.unwrap_or(DEFAULT_TIMEOUT)
                    };
                    handlers.entry(event.clone()).or_insert_with(Vec::new).push(
                        ConfiguredCommand {
                            matcher: matcher.clone(),
                            command,
                            timeout,
                            asynchronous: r#async && event != crate::SESSION_END,
                        },
                    );
                }
            }
        }
        Ok(Self { handlers })
    }

    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    pub fn handler_count(&self) -> usize {
        self.handlers.values().map(Vec::len).sum()
    }

    pub async fn run(&self, event: &str, payload: &HookPayload) -> Vec<CommandHookDecision> {
        let Some(handlers) = self.handlers.get(event) else {
            return Vec::new();
        };
        let matcher_values = matcher_values(event, payload);
        let input = match serde_json::to_vec(&payload.for_event(event)) {
            Ok(input) => input,
            Err(error) => {
                warn!(%event, %error, "failed to serialize command hook input");
                return Vec::new();
            }
        };
        let cwd = PathBuf::from(&payload.cwd);
        let environment = hook_environment(event, payload);
        let ignore_matcher = matches!(
            event,
            crate::USER_PROMPT_SUBMIT | crate::STOP | crate::INTERRUPT
        );
        let mut synchronous = JoinSet::new();
        for handler in handlers
            .iter()
            .filter(|handler| {
                ignore_matcher || matcher_matches(handler.matcher.as_ref(), &matcher_values)
            })
            .cloned()
        {
            let input = input.clone();
            let cwd = cwd.clone();
            let environment = environment.clone();
            if handler.asynchronous {
                std::thread::spawn(move || {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build();
                    let result = match runtime {
                        Ok(runtime) => {
                            runtime.block_on(run_command(&handler, &input, &cwd, &environment))
                        }
                        Err(error) => Err(error.into()),
                    };
                    if let Err(error) = result {
                        warn!(%error, "asynchronous command hook failed");
                    }
                });
            } else {
                synchronous
                    .spawn(async move { run_command(&handler, &input, &cwd, &environment).await });
            }
        }

        let mut decisions = Vec::new();
        while let Some(result) = synchronous.join_next().await {
            match result {
                Ok(Ok(output)) => decisions.push(parse_output(event, &output)),
                Ok(Err(error)) => warn!(%event, %error, "command hook failed open"),
                Err(error) => warn!(%event, %error, "command hook task failed open"),
            }
        }
        decisions
    }
}

fn compile_matcher(matcher: Option<&str>) -> anyhow::Result<Option<Regex>> {
    match matcher.map(str::trim) {
        None | Some("") | Some("*") => Ok(None),
        Some(matcher) => Ok(Some(Regex::new(matcher)?)),
    }
}

fn matcher_values(event: &str, payload: &HookPayload) -> Vec<String> {
    let value = match event {
        crate::SESSION_START => payload.source.clone(),
        crate::SESSION_END => payload.reason.clone(),
        crate::PRE_TOOL_USE | crate::PERMISSION_REQUEST | crate::POST_TOOL_USE => {
            payload.tool_name.clone()
        }
        crate::PRE_COMPACT | crate::POST_COMPACT => payload.trigger.clone(),
        crate::SUBAGENT_START | crate::SUBAGENT_STOP => payload.agent_type.clone(),
        _ => None,
    };
    let mut values = value.into_iter().collect::<Vec<_>>();
    if matches!(
        event,
        crate::PRE_TOOL_USE | crate::PERMISSION_REQUEST | crate::POST_TOOL_USE
    ) {
        if values.iter().any(|value| value == "terminal") {
            values.push("Bash".into());
        }
        if values.iter().any(|value| value == "apply_patch") {
            values.extend(["Edit".into(), "Write".into()]);
        }
    }
    values
}

fn matcher_matches(matcher: Option<&Regex>, values: &[String]) -> bool {
    matcher.is_none_or(|matcher| values.iter().any(|value| matcher.is_match(value)))
}

fn hook_environment(event: &str, payload: &HookPayload) -> Vec<(String, String)> {
    let mut environment = vec![
        ("ASTRO_HOOK_EVENT".into(), event.into()),
        ("ASTRO_HOOK_SESSION".into(), payload.session_id.clone()),
        ("ASTRO_HOOK_DETAIL".into(), payload.detail.clone()),
    ];
    if let Some(turn_id) = payload.turn_id.as_ref().filter(|value| !value.is_empty()) {
        environment.push(("ASTRO_HOOK_TURN".into(), turn_id.clone()));
    }
    if let Some(tool_name) = &payload.tool_name {
        environment.push(("ASTRO_HOOK_TOOL".into(), tool_name.clone()));
    }
    if let Some(message) = payload
        .prompt
        .as_ref()
        .or(payload.last_assistant_message.as_ref())
    {
        environment.push(("ASTRO_HOOK_MESSAGE".into(), message.clone()));
    }
    environment
}

#[derive(Debug)]
struct CommandOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

async fn run_command(
    handler: &ConfiguredCommand,
    input: &[u8],
    cwd: &Path,
    hook_environment: &[(String, String)],
) -> anyhow::Result<CommandOutput> {
    let mut command = Command::new(if cfg!(windows) { "cmd" } else { "sh" });
    if cfg!(windows) {
        command.args(["/C", &handler.command]);
    } else {
        command.args(["-c", &handler.command]);
    }
    command
        .env_clear()
        .envs(minimal_environment())
        .envs(hook_environment.iter().cloned())
        .current_dir(if cwd.is_dir() { cwd } else { Path::new(".") })
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input).await?;
        stdin.shutdown().await?;
    }
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let stdout_task = tokio::spawn(read_capped(stdout));
    let stderr_task = tokio::spawn(read_capped(stderr));
    let status = match tokio::time::timeout(handler.timeout, child.wait()).await {
        Ok(status) => status?,
        Err(_) => {
            let _ = child.kill().await;
            anyhow::bail!("timeout after {}s", handler.timeout.as_secs());
        }
    };
    let stdout = stdout_task.await??;
    let stderr = stderr_task.await??;
    Ok(CommandOutput {
        status,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

async fn read_capped(reader: impl AsyncRead + Unpin) -> anyhow::Result<Vec<u8>> {
    let mut output = Vec::new();
    reader
        .take((MAX_OUTPUT_BYTES + 1) as u64)
        .read_to_end(&mut output)
        .await?;
    anyhow::ensure!(
        output.len() <= MAX_OUTPUT_BYTES,
        "hook output exceeds 1 MiB"
    );
    Ok(output)
}

fn minimal_environment() -> impl Iterator<Item = (String, String)> {
    const ALLOWED: &[&str] = &["PATH", "HOME", "USER", "TMPDIR", "LANG", "LC_ALL", "SHELL"];
    ALLOWED
        .iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| ((*key).into(), value)))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireOutput {
    #[serde(default = "default_true", rename = "continue")]
    continue_processing: bool,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    decision: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    hook_specific_output: Option<HookSpecificOutput>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HookSpecificOutput {
    #[serde(default)]
    hook_event_name: Option<String>,
    #[serde(default)]
    additional_context: Option<String>,
    #[serde(default)]
    updated_input: Option<Value>,
    #[serde(default)]
    permission_decision: Option<String>,
    #[serde(default)]
    permission_decision_reason: Option<String>,
    #[serde(default)]
    decision: Option<PermissionDecision>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PermissionDecision {
    behavior: String,
    #[serde(default)]
    message: Option<String>,
}

const fn default_true() -> bool {
    true
}

fn parse_output(event: &str, output: &CommandOutput) -> CommandHookDecision {
    let stderr = output.stderr.trim();
    if output.status.code() == Some(2) {
        return CommandHookDecision {
            block_reason: Some(if stderr.is_empty() {
                "hook exited with status 2".into()
            } else {
                stderr.into()
            }),
            ..Default::default()
        };
    }
    if !output.status.success() {
        warn!(%event, status = %output.status, stderr, "command hook exited unsuccessfully");
        return CommandHookDecision::default();
    }
    let stdout = output.stdout.trim();
    if stdout.is_empty() {
        return CommandHookDecision::default();
    }
    let wire = match serde_json::from_str::<WireOutput>(stdout) {
        Ok(wire) => wire,
        Err(_)
            if matches!(
                event,
                crate::SESSION_START | crate::SUBAGENT_START | crate::USER_PROMPT_SUBMIT
            ) =>
        {
            return CommandHookDecision {
                additional_context: Some(stdout.into()),
                ..Default::default()
            };
        }
        Err(error) => {
            warn!(%event, %error, "invalid command hook JSON output; ignored");
            return CommandHookDecision::default();
        }
    };
    let specific = wire.hook_specific_output.unwrap_or_default();
    if specific
        .hook_event_name
        .as_deref()
        .is_some_and(|name| name != event)
    {
        warn!(%event, "command hook output names a different event; ignored");
        return CommandHookDecision::default();
    }
    let stop_reason = wire
        .stop_reason
        .filter(|reason| !reason.trim().is_empty())
        .unwrap_or_else(|| "hook requested stop".into());
    if !wire.continue_processing {
        return CommandHookDecision {
            block_reason: Some(stop_reason),
            ..Default::default()
        };
    }
    let mut decision = CommandHookDecision {
        additional_context: specific.additional_context,
        updated_input: specific.updated_input,
        ..Default::default()
    };
    match event {
        crate::PRE_TOOL_USE => match specific.permission_decision.as_deref() {
            Some("deny") => {
                decision.block_reason = specific.permission_decision_reason.or(wire.reason)
            }
            Some("allow") => {}
            _ if wire.decision.as_deref() == Some("block") => decision.block_reason = wire.reason,
            _ => {}
        },
        crate::PERMISSION_REQUEST => {
            if let Some(permission) = specific.decision {
                decision.permission = match permission.behavior.as_str() {
                    "allow" => Some(PermissionVote::Allow),
                    "deny" => Some(PermissionVote::Deny),
                    _ => None,
                };
                if decision.permission == Some(PermissionVote::Deny) {
                    decision.block_reason = permission.message;
                }
            }
        }
        crate::STOP | crate::SUBAGENT_STOP if wire.decision.as_deref() == Some("block") => {
            decision.keep_going = wire.reason;
        }
        crate::POST_TOOL_USE | crate::USER_PROMPT_SUBMIT
            if wire.decision.as_deref() == Some("block") =>
        {
            decision.block_reason = wire.reason;
        }
        _ => {}
    }
    decision
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(command: String, matcher: Option<&str>, asynchronous: bool) -> HooksFile {
        HooksFile {
            hooks: HashMap::from([(
                crate::PRE_TOOL_USE.into(),
                vec![MatcherGroup {
                    matcher: matcher.map(str::to_string),
                    hooks: vec![HookHandlerConfig::Command {
                        command,
                        command_windows: None,
                        timeout_sec: Some(2),
                        r#async: asynchronous,
                        status_message: None,
                    }],
                }],
            )]),
            ..Default::default()
        }
    }

    #[test]
    fn parses_codex_hooks_json_shape_and_rejects_bad_matcher() {
        let parsed: HooksFile = serde_json::from_str(
            r#"{"hooks":{"PreToolUse":[{"matcher":"^Bash$","hooks":[{"type":"command","command":"true","timeout":2}]}]}}"#,
        )
        .unwrap();
        assert!(CommandHookRunner::from_file(parsed, Path::new("hooks.json")).is_ok());
        let invalid = CommandHookRunner::from_file(
            file("true".into(), Some("["), false),
            Path::new("hooks.json"),
        )
        .unwrap();
        assert!(invalid.is_empty());
    }

    #[tokio::test]
    async fn command_receives_json_and_can_modify_matching_tool_input() {
        let command = r#"read payload; printf '%s' "$payload" | grep -q '"hook_event_name":"PreToolUse"' || exit 1; printf '%s' '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{"safe":true}}}'"#;
        let runner = CommandHookRunner::from_file(
            file(command.into(), Some("^Bash$"), false),
            Path::new("hooks.json"),
        )
        .unwrap();
        let decisions = runner
            .run(
                crate::PRE_TOOL_USE,
                &HookPayload {
                    cwd: std::env::current_dir()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    tool_name: Some("terminal".into()),
                    ..Default::default()
                },
            )
            .await;
        assert_eq!(decisions.len(), 1);
        assert_eq!(
            decisions[0].updated_input,
            Some(serde_json::json!({"safe": true}))
        );
    }

    #[tokio::test]
    async fn exit_two_blocks_with_stderr_reason() {
        let runner = CommandHookRunner::from_file(
            file("printf denied >&2; exit 2".into(), None, false),
            Path::new("hooks.json"),
        )
        .unwrap();
        let decisions = runner
            .run(
                crate::PRE_TOOL_USE,
                &HookPayload {
                    cwd: std::env::current_dir()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    tool_name: Some("terminal".into()),
                    ..Default::default()
                },
            )
            .await;
        assert_eq!(decisions[0].block_reason.as_deref(), Some("denied"));
    }
}
