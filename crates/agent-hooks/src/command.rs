//! Command hook configuration and execution.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use std::time::Instant;

use agent_config::loader::{load_local_config, LocalConfigOptions, ProjectTrust};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::task::JoinSet;
use tracing::warn;

use crate::run::{HookRunRecord, HookRunStatus, HookRunStore};
use crate::{HookEvent, HookPayload};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);
const SESSION_END_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_SESSION_END_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
static NEXT_RUN_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HooksFile {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub hooks: HashMap<String, Vec<MatcherGroup>>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MatcherGroup {
    #[serde(default)]
    pub matcher: Option<String>,
    #[serde(default)]
    pub hooks: Vec<HookHandlerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    id: String,
    source: String,
    matcher: Option<Regex>,
    command: String,
    timeout: Duration,
    asynchronous: bool,
    status_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandHookScope {
    User,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandHookTrust {
    Trusted,
    Untrusted,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandHookSourceSummary {
    pub path: String,
    pub scope: CommandHookScope,
    pub trust: CommandHookTrust,
    pub enabled: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CommandHookRunner {
    handlers: HashMap<String, Vec<ConfiguredCommand>>,
    runs: HookRunStore,
    sources: Vec<CommandHookSourceSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandHookSummary {
    pub id: String,
    pub event_name: String,
    pub matcher: Option<String>,
    pub command: String,
    pub source: String,
    pub asynchronous: bool,
}

impl CommandHookRunner {
    pub fn load(root: &Path) -> anyhow::Result<Self> {
        let path = root.join("hooks.json");
        let mut runner = Self::default();
        if path.is_file() {
            runner.load_file(&path, CommandHookScope::User, CommandHookTrust::Trusted)?;
        }
        Ok(runner)
    }

    pub fn load_for_project(astro_home: &Path, cwd: &Path) -> anyhow::Result<Self> {
        let mut runner = Self::load(astro_home)?;
        let cwd = cwd.canonicalize()?;
        let loaded = load_local_config(&LocalConfigOptions::new(astro_home, &cwd))?;
        let trust = CommandHookTrust::from(loaded.project_trust);
        for directory in project_hook_dirs(&loaded.project_root, &cwd) {
            let path = directory.join("hooks.json");
            if !path.is_file() {
                continue;
            }
            if trust != CommandHookTrust::Trusted {
                runner.sources.push(CommandHookSourceSummary {
                    path: path.to_string_lossy().into_owned(),
                    scope: CommandHookScope::Project,
                    trust,
                    enabled: false,
                    reason: Some(
                        match trust {
                            CommandHookTrust::Untrusted => "project is untrusted",
                            CommandHookTrust::Unknown => "project trust has not been granted",
                            CommandHookTrust::Trusted => unreachable!(),
                        }
                        .into(),
                    ),
                });
                continue;
            }
            runner.load_file(&path, CommandHookScope::Project, trust)?;
        }
        Ok(runner)
    }

    pub fn from_file(file: HooksFile, source: &Path) -> anyhow::Result<Self> {
        let mut runner = Self::default();
        runner.sources.push(CommandHookSourceSummary {
            path: source.to_string_lossy().into_owned(),
            scope: CommandHookScope::User,
            trust: CommandHookTrust::Trusted,
            enabled: true,
            reason: None,
        });
        runner.extend_from_file(file, source);
        Ok(runner)
    }

    fn load_file(
        &mut self,
        source: &Path,
        scope: CommandHookScope,
        trust: CommandHookTrust,
    ) -> anyhow::Result<()> {
        let raw = std::fs::read_to_string(source)?;
        let file: HooksFile = serde_json::from_str(&raw)?;
        self.sources.push(CommandHookSourceSummary {
            path: source.to_string_lossy().into_owned(),
            scope,
            trust,
            enabled: true,
            reason: None,
        });
        self.extend_from_file(file, source);
        Ok(())
    }

    fn extend_from_file(&mut self, file: HooksFile, source: &Path) {
        for (event, groups) in file.hooks {
            if !HookEvent::COMMAND_HOOK_EVENTS
                .iter()
                .any(|candidate| candidate.as_str() == event)
            {
                warn!(%event, source = %source.display(), "ignoring unsupported command hook event");
                continue;
            }
            for (group_index, group) in groups.into_iter().enumerate() {
                let matcher_source = group.matcher.clone();
                let matcher = match compile_matcher(group.matcher.as_deref()) {
                    Ok(matcher) => matcher,
                    Err(error) => {
                        warn!(%event, %error, source = %source.display(), "invalid hook matcher group disabled");
                        continue;
                    }
                };
                for (handler_index, handler) in group.hooks.into_iter().enumerate() {
                    let HookHandlerConfig::Command {
                        command,
                        command_windows,
                        timeout_sec,
                        r#async,
                        status_message,
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
                    let id = handler_id(
                        source,
                        &event,
                        group_index,
                        handler_index,
                        matcher_source.as_deref(),
                        &command,
                    );
                    self.handlers
                        .entry(event.clone())
                        .or_insert_with(Vec::new)
                        .push(ConfiguredCommand {
                            id,
                            source: source.to_string_lossy().into_owned(),
                            matcher: matcher.clone(),
                            command,
                            timeout,
                            asynchronous: r#async && event != crate::SESSION_END,
                            status_message,
                        });
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }

    pub fn handler_count(&self) -> usize {
        self.handlers.values().map(Vec::len).sum()
    }

    pub fn list(&self) -> Vec<CommandHookSummary> {
        let mut hooks = self
            .handlers
            .iter()
            .flat_map(|(event_name, handlers)| {
                handlers.iter().map(|handler| CommandHookSummary {
                    id: handler.id.clone(),
                    event_name: event_name.clone(),
                    matcher: handler
                        .matcher
                        .as_ref()
                        .map(|matcher| matcher.as_str().to_string()),
                    command: handler.command.clone(),
                    source: handler.source.clone(),
                    asynchronous: handler.asynchronous,
                })
            })
            .collect::<Vec<_>>();
        hooks.sort_by(|left, right| left.id.cmp(&right.id));
        hooks
    }

    pub fn recent_runs(&self) -> Vec<HookRunRecord> {
        self.runs.recent()
    }

    pub fn sources(&self) -> Vec<CommandHookSourceSummary> {
        self.sources.clone()
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
            let run_id = format!("hook-run-{}", NEXT_RUN_ID.fetch_add(1, Ordering::Relaxed));
            self.runs.start(HookRunRecord {
                id: run_id.clone(),
                event_name: event.to_string(),
                handler_id: handler.id.clone(),
                source: handler.source.clone(),
                status: HookRunStatus::Running,
                summary: handler
                    .status_message
                    .clone()
                    .unwrap_or_else(|| "running command hook".into()),
                duration_ms: None,
            });
            let input = input.clone();
            let cwd = cwd.clone();
            let environment = environment.clone();
            if handler.asynchronous {
                let runs = self.runs.clone();
                let run_event = event.to_string();
                std::thread::spawn(move || {
                    let started = Instant::now();
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build();
                    let result = match runtime {
                        Ok(runtime) => {
                            runtime.block_on(run_command(&handler, &input, &cwd, &environment))
                        }
                        Err(error) => Err(error.into()),
                    };
                    if let Err(error) = &result {
                        warn!(%error, "asynchronous command hook failed");
                    }
                    finish_run(&runs, &run_id, &run_event, started, result.as_ref());
                });
            } else {
                let run_event = event.to_string();
                synchronous.spawn(async move {
                    let started = Instant::now();
                    let result = run_command(&handler, &input, &cwd, &environment).await;
                    (run_id, run_event, started, result)
                });
            }
        }

        let mut decisions = Vec::new();
        while let Some(result) = synchronous.join_next().await {
            match result {
                Ok((run_id, run_event, started, Ok(output))) => {
                    let decision = parse_output(event, &output);
                    finish_run(&self.runs, &run_id, &run_event, started, Ok(&output));
                    decisions.push(decision);
                }
                Ok((run_id, run_event, started, Err(error))) => {
                    finish_run(&self.runs, &run_id, &run_event, started, Err(&error));
                    warn!(%event, %error, "command hook failed open");
                }
                Err(error) => warn!(%event, %error, "command hook task failed open"),
            }
        }
        decisions
    }
}

fn handler_id(
    source: &Path,
    event: &str,
    group_index: usize,
    handler_index: usize,
    matcher: Option<&str>,
    command: &str,
) -> String {
    let mut digest = Sha256::new();
    digest.update(source.to_string_lossy().as_bytes());
    digest.update([0]);
    for part in [
        event,
        &group_index.to_string(),
        &handler_index.to_string(),
        matcher.unwrap_or(""),
        command,
    ] {
        digest.update(part.as_bytes());
        digest.update([0]);
    }
    format!("command-{:x}", digest.finalize())
}

fn project_hook_dirs(project_root: &Path, cwd: &Path) -> Vec<PathBuf> {
    let mut directories = cwd
        .ancestors()
        .take_while(|directory| directory.starts_with(project_root))
        .map(|directory| directory.join(".astro"))
        .collect::<Vec<_>>();
    directories.reverse();
    directories
}

impl From<ProjectTrust> for CommandHookTrust {
    fn from(value: ProjectTrust) -> Self {
        match value {
            ProjectTrust::Trusted => Self::Trusted,
            ProjectTrust::Untrusted => Self::Untrusted,
            ProjectTrust::Unknown => Self::Unknown,
        }
    }
}

fn finish_run(
    runs: &HookRunStore,
    run_id: &str,
    event: &str,
    started: Instant,
    result: Result<&CommandOutput, &anyhow::Error>,
) {
    let duration_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    let (status, summary) = match result {
        Ok(output) if output.status.code() == Some(2) => (
            HookRunStatus::Blocked,
            stderr_reason(output).unwrap_or_else(|| format!("{event} command blocked")),
        ),
        Ok(output) if output.status.success() => (
            HookRunStatus::Completed,
            format!("{event} command completed"),
        ),
        Ok(output) => (
            HookRunStatus::Failed,
            stderr_reason(output)
                .unwrap_or_else(|| format!("{event} command exited with status {}", output.status)),
        ),
        Err(error) => (HookRunStatus::Failed, error.to_string()),
    };
    runs.finish(run_id, status, summary, duration_ms);
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

fn stderr_reason(output: &CommandOutput) -> Option<String> {
    let reason = output.stderr.trim();
    (!reason.is_empty()).then(|| reason.to_string())
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

    fn project_fixture(trust: &str, hooks: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let astro_home = root.path().join("home/.astro");
        let project = root.path().join("repo");
        let cwd = project.join("nested");
        std::fs::create_dir_all(&astro_home).unwrap();
        std::fs::create_dir_all(project.join(".git")).unwrap();
        std::fs::create_dir_all(project.join(".astro")).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(
            astro_home.join("config.toml"),
            format!(
                "[projects.{:?}]\ntrust_level = {:?}\n",
                project.to_string_lossy(),
                trust
            ),
        )
        .unwrap();
        std::fs::write(project.join(".astro/hooks.json"), hooks).unwrap();
        (root, astro_home, cwd)
    }

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
    fn parses_command_hooks_json_shape_and_rejects_bad_matcher() {
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

    #[test]
    fn trusted_project_hooks_are_discovered_and_enabled() {
        let (_root, astro_home, cwd) = project_fixture(
            "trusted",
            r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"true"}]}]}}"#,
        );

        let runner = CommandHookRunner::load_for_project(&astro_home, &cwd).unwrap();

        assert_eq!(runner.handler_count(), 1);
        assert_eq!(runner.sources().len(), 1);
        assert_eq!(runner.sources()[0].scope, CommandHookScope::Project);
        assert_eq!(runner.sources()[0].trust, CommandHookTrust::Trusted);
        assert!(runner.sources()[0].enabled);
    }

    #[test]
    fn untrusted_project_hook_content_is_not_parsed() {
        let (_root, astro_home, cwd) =
            project_fixture("untrusted", "this is intentionally invalid JSON");

        let runner = CommandHookRunner::load_for_project(&astro_home, &cwd).unwrap();

        assert_eq!(runner.handler_count(), 0);
        assert_eq!(runner.sources().len(), 1);
        assert_eq!(runner.sources()[0].trust, CommandHookTrust::Untrusted);
        assert!(!runner.sources()[0].enabled);
        assert_eq!(
            runner.sources()[0].reason.as_deref(),
            Some("project is untrusted")
        );
    }

    #[tokio::test]
    async fn command_receives_json_and_can_modify_matching_tool_input() {
        let command = r#"read payload; printf '%s' "$payload" | grep -q '"hook_event_name":"PreToolUse"' || exit 1; printf '%s' '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{"safe":true}}}'"#;
        let runner = CommandHookRunner::from_file(
            file(command.into(), Some("^Bash$"), false),
            Path::new("hooks.json"),
        )
        .unwrap();
        let listed = runner.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].event_name, crate::PRE_TOOL_USE);
        assert_eq!(listed[0].matcher.as_deref(), Some("^Bash$"));
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
        let runs = runner.recent_runs();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].handler_id, listed[0].id);
        assert_eq!(runs[0].status, HookRunStatus::Completed);
        assert!(runs[0].duration_ms.is_some());
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
        let runs = runner.recent_runs();
        assert_eq!(runs[0].status, HookRunStatus::Blocked);
        assert_eq!(runs[0].summary, "denied");
    }
}
