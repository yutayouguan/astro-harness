//! Command hook configuration and execution.

use std::collections::HashMap;
#[cfg(not(windows))]
use std::ffi::OsStr;
use std::ffi::OsString;
use std::future::Future;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;
use std::time::Instant;

use agent_config::loader::{load_local_config, LocalConfigOptions, ProjectTrust};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::warn;

#[cfg(windows)]
use crate::windows_job::WindowsJobObject;

use crate::mcp::{expand_argument_template, unavailable_executor, HookMcpCall, HookMcpExecutor};
use crate::run::{
    unix_timestamp, HookExecutionMode, HookHandlerType, HookOutputEntry, HookOutputEntryKind,
    HookRunRecord, HookRunStatus, HookRunStore, HookScope, HookSource, HookTrustStatus,
};
use crate::{HookEvent, HookPayload};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);
const SESSION_END_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_SESSION_END_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_ENV_VALUE_BYTES: usize = 8 * 1024;
const DEFAULT_ADDITIONAL_CONTEXT_TOKEN_LIMIT: usize = 2_500;
const MAX_CONCURRENT_ASYNC_HOOKS: usize = 8;
const NON_INHERITABLE_ENV_VARS: &[&str] = &[
    "CODEX_EXEC_SERVER_NOISE_AUTH_TOKEN",
    "NODE_REPL_AUTH_TOKEN",
    "OPENAI_FEDERATION_RULE_ID",
    "OPENAI_IDENTITY_TOKEN_FILE",
    "OPENAI_WORKLOAD_IDENTITY_CONTEXT",
];
static NEXT_RUN_ID: AtomicU64 = AtomicU64::new(1);

struct AsyncHookRuntimeState {
    concurrency_limit: Arc<Semaphore>,
    tasks: JoinSet<()>,
}

impl Default for AsyncHookRuntimeState {
    fn default() -> Self {
        Self {
            concurrency_limit: Arc::new(Semaphore::new(MAX_CONCURRENT_ASYNC_HOOKS)),
            tasks: JoinSet::new(),
        }
    }
}

#[derive(Default)]
struct AsyncHookRuntime {
    state: Mutex<AsyncHookRuntimeState>,
}

struct AsyncHookCancellation<F: FnOnce()> {
    callback: Option<F>,
}

impl<F: FnOnce()> AsyncHookCancellation<F> {
    fn disarm(&mut self) {
        self.callback = None;
    }
}

impl<F: FnOnce()> Drop for AsyncHookCancellation<F> {
    fn drop(&mut self) {
        if let Some(callback) = self.callback.take() {
            callback();
        }
    }
}

impl AsyncHookRuntime {
    fn lock_state(&self) -> MutexGuard<'_, AsyncHookRuntimeState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn schedule(
        &self,
        task: impl Future<Output = ()> + Send + 'static,
        on_cancel: impl FnOnce() + Send + 'static,
    ) -> anyhow::Result<()> {
        let runtime = match async_hook_runtime() {
            Ok(runtime) => runtime,
            Err(error) => {
                on_cancel();
                return Err(error);
            }
        };
        let mut state = self.lock_state();
        if state.concurrency_limit.is_closed() {
            on_cancel();
            anyhow::bail!("asynchronous hook runtime is shut down");
        }

        while state.tasks.try_join_next().is_some() {}
        let concurrency_limit = Arc::clone(&state.concurrency_limit);
        let mut cancellation = AsyncHookCancellation {
            callback: Some(on_cancel),
        };
        state.tasks.spawn_on(
            async move {
                let Ok(_permit) = concurrency_limit.acquire_owned().await else {
                    return;
                };
                task.await;
                cancellation.disarm();
            },
            runtime.handle(),
        );
        Ok(())
    }

    async fn shutdown(&self) {
        let mut tasks = {
            let mut state = self.lock_state();
            state.concurrency_limit.close();
            std::mem::take(&mut state.tasks)
        };
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    }
}

fn async_hook_runtime() -> anyhow::Result<&'static tokio::runtime::Runtime> {
    static RUNTIME: OnceLock<Result<tokio::runtime::Runtime, String>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_name("astro-async-hook")
                .enable_all()
                .build()
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| anyhow::anyhow!(error.clone()))
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HooksFile {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub hooks: HashMap<String, Vec<MatcherGroup>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct HooksToml {
    #[serde(default)]
    state: HashMap<String, HookStateToml>,
    #[serde(default, flatten)]
    events: HashMap<String, Vec<MatcherGroup>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct HookStateToml {
    #[serde(default, rename = "enabled")]
    enabled: Option<bool>,
    #[serde(default, rename = "trusted_hash")]
    trusted_hash: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ConfigToml {
    #[serde(default)]
    hooks: Option<HooksToml>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MatcherGroup {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
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
        #[serde(default, rename = "additionalContextLimit")]
        additional_context_limit: Option<usize>,
    },
    #[serde(rename = "prompt")]
    Prompt {},
    #[serde(rename = "agent")]
    Agent {},
    #[serde(rename = "mcp_tool")]
    McpTool {
        server: String,
        tool: String,
        #[serde(default, deserialize_with = "deserialize_mcp_tool_input")]
        input: serde_json::Map<String, Value>,
        #[serde(default, rename = "timeout")]
        timeout_sec: Option<u64>,
        #[serde(default, rename = "statusMessage")]
        status_message: Option<String>,
    },
}

fn deserialize_mcp_tool_input<'de, D>(
    deserializer: D,
) -> Result<serde_json::Map<String, Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let input = serde_json::Map::deserialize(deserializer)?;
    toml::Value::try_from(&input).map_err(|error| {
        serde::de::Error::custom(format!(
            "MCP hook input must be representable as TOML: {error}"
        ))
    })?;
    Ok(input)
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
    pub stop_reason: Option<String>,
    pub warnings: Vec<String>,
    pub error: Option<String>,
    pub(crate) hook_run_id: Option<String>,
    pub(crate) completion_order: Option<usize>,
}

#[derive(Debug, Clone)]
struct ConfiguredHandler {
    rule_id: Option<String>,
    id: String,
    key: String,
    enabled: bool,
    current_hash: String,
    trust_status: HookTrustStatus,
    source: String,
    source_kind: HookSource,
    matcher: Option<Regex>,
    timeout: Duration,
    asynchronous: bool,
    status_message: Option<String>,
    additional_context_limit: Option<usize>,
    kind: ConfiguredHandlerKind,
}

#[derive(Debug, Clone)]
enum ConfiguredHandlerKind {
    Command {
        command: String,
    },
    McpTool {
        server: String,
        tool: String,
        input: serde_json::Map<String, Value>,
    },
}

impl ConfiguredHandlerKind {
    fn identity(&self) -> String {
        match self {
            Self::Command { command } => command.clone(),
            Self::McpTool {
                server,
                tool,
                input,
            } => {
                format!("{server}:{tool}:{}", Value::Object(input.clone()))
            }
        }
    }

    const fn handler_type(&self) -> HookHandlerType {
        match self {
            Self::Command { .. } => HookHandlerType::Command,
            Self::McpTool { .. } => HookHandlerType::McpTool,
        }
    }

    fn command(&self) -> Option<&str> {
        match self {
            Self::Command { command } => Some(command),
            Self::McpTool { .. } => None,
        }
    }

    fn server(&self) -> Option<&str> {
        match self {
            Self::McpTool { server, .. } => Some(server),
            Self::Command { .. } => None,
        }
    }

    fn tool(&self) -> Option<&str> {
        match self {
            Self::McpTool { tool, .. } => Some(tool),
            Self::Command { .. } => None,
        }
    }
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

#[derive(Clone)]
pub struct CommandHookRunner {
    configuration_error: Option<String>,
    trust_hashes: HashMap<String, String>,
    authority_root: Option<PathBuf>,
    handlers: HashMap<HookEvent, Vec<ConfiguredHandler>>,
    runs: HookRunStore,
    sources: Vec<CommandHookSourceSummary>,
    mcp_executor: Arc<dyn HookMcpExecutor>,
    async_runtime: Arc<AsyncHookRuntime>,
    environment: Arc<Vec<(OsString, OsString)>>,
}

impl std::fmt::Debug for CommandHookRunner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommandHookRunner")
            .field("handlers", &self.handlers)
            .field("runs", &self.runs)
            .field("sources", &self.sources)
            .finish_non_exhaustive()
    }
}

impl Default for CommandHookRunner {
    fn default() -> Self {
        Self {
            handlers: HashMap::new(),
            configuration_error: None,
            trust_hashes: HashMap::new(),
            authority_root: None,
            runs: HookRunStore::default(),
            sources: Vec::new(),
            mcp_executor: unavailable_executor(),
            async_runtime: Arc::new(AsyncHookRuntime::default()),
            environment: Arc::new(std::env::vars_os().collect()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommandHookSummary {
    pub id: String,
    pub key: String,
    pub enabled: bool,
    pub current_hash: String,
    pub trust_status: HookTrustStatus,
    pub event_name: String,
    pub matcher: Option<String>,
    pub handler_type: HookHandlerType,
    pub command: Option<String>,
    pub server: Option<String>,
    pub tool: Option<String>,
    pub source: String,
    pub asynchronous: bool,
    pub additional_context_limit: Option<usize>,
}

impl CommandHookRunner {
    pub fn blocked(error: impl Into<String>) -> Self {
        Self {
            configuration_error: Some(error.into()),
            ..Default::default()
        }
    }
    pub fn load(root: &Path) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !crate::migration::marker(root).exists(),
            "extension migration is incomplete"
        );
        Self::validate_migrated_home(root)
    }

    pub(crate) fn validate_migrated_home(root: &Path) -> anyhow::Result<Self> {
        let mut runner = Self {
            authority_root: Some(root.to_path_buf()),
            trust_hashes: crate::trust::load(root)?,
            runs: HookRunStore::with_audit_root(root),
            ..Default::default()
        };
        runner.load_scope(root, CommandHookScope::User, CommandHookTrust::Trusted)?;
        Ok(runner)
    }

    pub fn load_for_project(astro_home: &Path, cwd: &Path) -> anyhow::Result<Self> {
        let mut runner = Self::load(astro_home)?;
        let cwd = cwd.canonicalize()?;
        let loaded = load_local_config(&LocalConfigOptions::new(astro_home, &cwd))?;
        runner.authority_root = Some(loaded.project_root.clone());
        let trust = CommandHookTrust::from(loaded.project_trust);
        for directory in project_hook_dirs(&loaded.project_root, &cwd) {
            let source_paths = [directory.join("hooks.json"), directory.join("config.toml")];
            if !source_paths.iter().any(|path| path.is_file()) {
                continue;
            }
            if trust != CommandHookTrust::Trusted {
                for path in source_paths.into_iter().filter(|path| path.is_file()) {
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
                }
                continue;
            }
            runner.load_scope(&directory, CommandHookScope::Project, trust)?;
        }
        Ok(runner)
    }

    fn load_scope(
        &mut self,
        directory: &Path,
        scope: CommandHookScope,
        trust: CommandHookTrust,
    ) -> anyhow::Result<()> {
        let path = directory.join("config.toml");
        let value = agent_config::sources::read_root(&path)?;
        let boundary = self.authority_root.as_deref().unwrap_or(directory);
        let documents = agent_config::sources::documents(
            &path,
            &value,
            agent_config::sources::Domain::Hooks,
            boundary,
        )?;
        let legacy = directory.join("hooks.json");
        if legacy.exists() {
            let legacy = legacy.canonicalize()?;
            anyhow::ensure!(documents.iter().any(|doc| doc.path == legacy && !doc.root), "hooks.json must be explicitly referenced by config_sources.hooks; run the extension config migration");
        }
        let mut controls = HashMap::new();
        let mut definitions = Vec::new();
        for doc in documents {
            let config: ConfigToml = doc.value.try_into().map_err(|_| {
                anyhow::anyhow!("invalid Hook configuration in {}", doc.path.display())
            })?;
            let Some(hooks) = config.hooks else {
                continue;
            };
            anyhow::ensure!(
                hooks
                    .state
                    .values()
                    .all(|state| state.trusted_hash.is_none()),
                "trusted_hash must be migrated to security/hooks/trust.json"
            );
            controls.extend(
                hooks
                    .state
                    .into_iter()
                    .map(|(key, state)| (crate::trust::canonical_key(&key), state)),
            );
            if !hooks.events.is_empty() {
                definitions.push((
                    doc.path,
                    HooksFile {
                        description: None,
                        hooks: hooks.events,
                    },
                ));
            }
        }
        for (source, file) in definitions {
            self.sources.push(CommandHookSourceSummary {
                path: source.to_string_lossy().into_owned(),
                scope,
                trust,
                enabled: true,
                reason: None,
            });
            self.extend_from_file(file, &source, &controls, scope, trust);
        }
        Ok(())
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
        runner.extend_from_file(
            file,
            source,
            &HashMap::new(),
            CommandHookScope::User,
            CommandHookTrust::Trusted,
        );
        Ok(runner)
    }

    fn extend_from_file(
        &mut self,
        file: HooksFile,
        source: &Path,
        states: &HashMap<String, HookStateToml>,
        source_scope: CommandHookScope,
        source_trust: CommandHookTrust,
    ) {
        for (event, groups) in file.hooks {
            let Some(event_name) = HookEvent::from_command_name(&event) else {
                warn!(%event, source = %source.display(), "ignoring unsupported command hook event");
                continue;
            };
            for (group_index, group) in groups.into_iter().enumerate() {
                if let Some(rule_id) = group.id.as_deref() {
                    // Higher project layers replace one logical rule, not an event's entire array.
                    for handlers in self.handlers.values_mut() {
                        handlers.retain(|handler| handler.rule_id.as_deref() != Some(rule_id));
                    }
                }
                let group_for_hash = group.clone();
                let matcher_source = group.matcher.clone();
                let matcher = match compile_matcher(group.matcher.as_deref()) {
                    Ok(matcher) => matcher,
                    Err(error) => {
                        warn!(%event, %error, source = %source.display(), "invalid hook matcher group disabled");
                        continue;
                    }
                };
                for (handler_index, handler) in group.hooks.into_iter().enumerate() {
                    let key = match group.id.as_deref() {
                        Some(id) => format!("{}:rule:{id}:{handler_index}", source.display()),
                        None => hook_key(source, event_name, group_index, handler_index),
                    };
                    let state = states
                        .get(&key)
                        .or_else(|| group.id.as_ref().and_then(|id| states.get(id)));
                    let (kind, requested, asynchronous, status_message, additional_context_limit) =
                        match handler {
                            HookHandlerConfig::Command {
                                command,
                                command_windows,
                                timeout_sec,
                                r#async,
                                status_message,
                                additional_context_limit,
                            } => {
                                #[cfg(windows)]
                                let command = command_windows.unwrap_or(command);
                                #[cfg(not(windows))]
                                let _ = command_windows;
                                if command.trim().is_empty() {
                                    warn!(%event, source = %source.display(), "empty hook command disabled");
                                    continue;
                                }
                                (
                                    ConfiguredHandlerKind::Command { command },
                                    timeout_sec.map(Duration::from_secs),
                                    r#async,
                                    status_message,
                                    additional_context_limit,
                                )
                            }
                            HookHandlerConfig::McpTool {
                                server,
                                tool,
                                input,
                                timeout_sec,
                                status_message,
                            } => {
                                if server.trim().is_empty() || tool.trim().is_empty() {
                                    warn!(%event, source = %source.display(), "MCP hook server and tool must not be empty");
                                    continue;
                                }
                                (
                                    ConfiguredHandlerKind::McpTool {
                                        server,
                                        tool,
                                        input,
                                    },
                                    timeout_sec.map(Duration::from_secs),
                                    false,
                                    status_message,
                                    None,
                                )
                            }
                            HookHandlerConfig::Prompt {} | HookHandlerConfig::Agent {} => {
                                warn!(%event, source = %source.display(), "hook handler type is not supported yet");
                                continue;
                            }
                        };
                    let additional_context_limit = if matches!(
                        event_name,
                        HookEvent::PreToolUse
                            | HookEvent::PostToolUse
                            | HookEvent::SessionStart
                            | HookEvent::UserPromptSubmit
                            | HookEvent::SubagentStart
                    ) {
                        additional_context_limit
                    } else {
                        if additional_context_limit.is_some() {
                            warn!(%event, source = %source.display(), "additionalContextLimit ignored for event without additional context");
                        }
                        None
                    };
                    let timeout = normalize_timeout(event_name, requested);
                    let normalized_handler = normalized_handler_config(
                        &kind,
                        timeout,
                        asynchronous,
                        status_message.clone(),
                        additional_context_limit,
                    );
                    let current_hash = hook_hash(
                        event_name,
                        matcher_source.as_deref(),
                        &group_for_hash,
                        normalized_handler,
                    );
                    let trust_status = match self.trust_hashes.get(&key).map(String::as_str) {
                        Some(trusted_hash) if trusted_hash == current_hash => {
                            HookTrustStatus::Trusted
                        }
                        Some(_) => HookTrustStatus::Modified,
                        None if source_trust == CommandHookTrust::Trusted => {
                            HookTrustStatus::Trusted
                        }
                        None => HookTrustStatus::Untrusted,
                    };
                    let enabled = state.and_then(|state| state.enabled) != Some(false)
                        && trust_status != HookTrustStatus::Modified;
                    let id = if group.id.is_some() {
                        key.clone()
                    } else {
                        handler_id(
                            source,
                            &event,
                            group_index,
                            handler_index,
                            matcher_source.as_deref(),
                            &kind.identity(),
                        )
                    };
                    self.handlers
                        .entry(event_name)
                        .or_default()
                        .push(ConfiguredHandler {
                            rule_id: group.id.clone(),
                            id,
                            key,
                            enabled,
                            current_hash,
                            trust_status,
                            source: source.to_string_lossy().into_owned(),
                            source_kind: match source_scope {
                                CommandHookScope::User => HookSource::User,
                                CommandHookScope::Project => HookSource::Project,
                            },
                            matcher: matcher.clone(),
                            timeout,
                            asynchronous: asynchronous && event_name != HookEvent::SessionEnd,
                            status_message,
                            additional_context_limit,
                            kind,
                        });
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        if self.configuration_error.is_some() {
            return false;
        }
        self.handlers
            .values()
            .all(|handlers| handlers.iter().all(|handler| !handler.enabled))
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
                    key: handler.key.clone(),
                    enabled: handler.enabled,
                    current_hash: handler.current_hash.clone(),
                    trust_status: handler.trust_status,
                    event_name: event_name.as_str().to_string(),
                    matcher: handler
                        .matcher
                        .as_ref()
                        .map(|matcher| matcher.as_str().to_string()),
                    handler_type: handler.kind.handler_type(),
                    command: handler.kind.command().map(ToOwned::to_owned),
                    server: handler.kind.server().map(ToOwned::to_owned),
                    tool: handler.kind.tool().map(ToOwned::to_owned),
                    source: handler.source.clone(),
                    asynchronous: handler.asynchronous,
                    additional_context_limit: handler.additional_context_limit,
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

    pub(crate) fn share_run_store(mut self, mut runs: HookRunStore) -> Self {
        if runs.audit_file.is_none() {
            runs.audit_file = self.runs.audit_file.clone();
        }
        self.runs = runs;
        self
    }

    pub(crate) fn run_store(&self) -> HookRunStore {
        self.runs.clone()
    }

    pub fn with_mcp_executor(mut self, executor: Arc<dyn HookMcpExecutor>) -> Self {
        self.mcp_executor = executor;
        // `with_mcp_executor` is the session-binding boundary. Configured hooks and their
        // run store may be shared, but asynchronous task ownership must remain per session.
        self.async_runtime = Arc::new(AsyncHookRuntime::default());
        self.environment = Arc::new(std::env::vars_os().collect());
        self
    }

    pub async fn shutdown(&self) {
        self.async_runtime.shutdown().await;
    }

    pub async fn run(&self, event: &str, payload: &HookPayload) -> Vec<CommandHookDecision> {
        self.run_selected(event, payload, false).await
    }

    pub(crate) async fn run_memory_consolidation(
        &self,
        event: &str,
        payload: &HookPayload,
    ) -> Vec<CommandHookDecision> {
        self.run_selected(event, payload, true).await
    }

    async fn run_selected(
        &self,
        event: &str,
        payload: &HookPayload,
        memory_consolidation: bool,
    ) -> Vec<CommandHookDecision> {
        if let Some(error) = &self.configuration_error {
            return vec![CommandHookDecision {
                block_reason: Some(format!("Hook configuration unavailable: {error}")),
                error: Some(error.clone()),
                ..Default::default()
            }];
        }
        let Some(event_name) = HookEvent::from_command_name(event) else {
            return Vec::new();
        };
        let Some(handlers) = self.handlers.get(&event_name) else {
            return Vec::new();
        };
        let matcher_values = matcher_values(event_name, payload);
        let input = match serde_json::to_vec(&payload.command_input_for_event(event_name)) {
            Ok(input) => input,
            Err(error) => {
                warn!(%event, %error, "failed to serialize command hook input");
                return Vec::new();
            }
        };
        let cwd = PathBuf::from(&payload.cwd);
        let hook_environment = hook_environment(event, payload);
        let ignore_matcher = matches!(
            event_name,
            HookEvent::UserPromptSubmit | HookEvent::Stop | HookEvent::Interrupt
        );
        let mut synchronous = JoinSet::new();
        for (configured_order, handler) in handlers
            .iter()
            .filter(|handler| {
                handler.enabled
                    && (!memory_consolidation
                        || memory_consolidation_source_allowed(handler.source_kind))
                    && (ignore_matcher
                        || matcher_matches(handler.matcher.as_ref(), &matcher_values))
            })
            .cloned()
            .enumerate()
        {
            let run_id = format!("hook-run-{}", NEXT_RUN_ID.fetch_add(1, Ordering::Relaxed));
            self.runs.start(
                payload.session_id.clone(),
                payload.turn_id.clone(),
                HookRunRecord {
                    id: run_id.clone(),
                    event_name: event.to_string(),
                    handler_id: handler.id.clone(),
                    handler_type: handler.kind.handler_type(),
                    execution_mode: if handler.asynchronous {
                        HookExecutionMode::Async
                    } else {
                        HookExecutionMode::Sync
                    },
                    scope: hook_scope(event),
                    source_path: handler.source.clone(),
                    source: handler.source_kind,
                    display_order: configured_order,
                    status: HookRunStatus::Running,
                    status_message: handler.status_message.clone(),
                    summary: handler.status_message.clone().unwrap_or_else(|| {
                        format!("running {} hook", handler.kind.handler_type().label())
                    }),
                    started_at: unix_timestamp(),
                    completed_at: None,
                    duration_ms: None,
                    entries: Vec::new(),
                },
            );
            let input = input.clone();
            let cwd = cwd.clone();
            let hook_environment = hook_environment.clone();
            let session_environment = Arc::clone(&self.environment);
            let mcp_executor = Arc::clone(&self.mcp_executor);
            if handler.asynchronous {
                let additional_context_limit = handler.additional_context_limit;
                let session_id = payload.session_id.clone();
                let runs = self.runs.clone();
                let run_event = event.to_string();
                let started = Instant::now();
                let task_run_id = run_id.clone();
                let cancelled_runs = self.runs.clone();
                let cancelled_run_id = run_id.clone();
                let cancelled_event = event.to_string();
                if let Err(error) = self.async_runtime.schedule(
                    async move {
                        let result = run_handler(
                            &handler,
                            &input,
                            &cwd,
                            &session_environment,
                            &hook_environment,
                            mcp_executor.as_ref(),
                        )
                        .await;
                        if let Err(error) = &result {
                            warn!(%error, "asynchronous command hook failed");
                        }
                        let mut decision = result
                            .as_ref()
                            .ok()
                            .map(|output| parse_output(&run_event, output, false));
                        if let Some(decision) = decision.as_mut() {
                            if let Some(context) = decision.additional_context.take() {
                                decision.additional_context = Some(
                                    maybe_spill_additional_context(
                                        &session_id,
                                        &task_run_id,
                                        context,
                                        additional_context_limit,
                                    )
                                    .await,
                                );
                            }
                        }
                        finish_run(
                            &runs,
                            &task_run_id,
                            &run_event,
                            started,
                            result.as_ref(),
                            decision.as_ref(),
                        );
                    },
                    move || {
                        let summary = format!("{cancelled_event} hook cancelled");
                        cancelled_runs.finish_silently(
                            &cancelled_run_id,
                            HookRunStatus::Failed,
                            summary.clone(),
                            started.elapsed().as_millis().min(u64::MAX as u128) as u64,
                            vec![HookOutputEntry {
                                kind: HookOutputEntryKind::Error,
                                text: summary,
                            }],
                        );
                    },
                ) {
                    warn!(%event, %error, "failed to schedule asynchronous command hook");
                }
            } else {
                let run_event = event.to_string();
                synchronous.spawn(async move {
                    let started = Instant::now();
                    let additional_context_limit = handler.additional_context_limit;
                    let result = run_handler(
                        &handler,
                        &input,
                        &cwd,
                        &session_environment,
                        &hook_environment,
                        mcp_executor.as_ref(),
                    )
                    .await;
                    (
                        configured_order,
                        run_id,
                        run_event,
                        started,
                        additional_context_limit,
                        result,
                    )
                });
            }
        }

        let mut completed = Vec::new();
        while let Some(result) = synchronous.join_next().await {
            match result {
                Ok((configured_order, run_id, run_event, started, limit, Ok(output))) => {
                    let mut decision = parse_output(event, &output, true);
                    if let Some(context) = decision.additional_context.take() {
                        decision.additional_context = Some(
                            maybe_spill_additional_context(
                                &payload.session_id,
                                &run_id,
                                context,
                                limit,
                            )
                            .await,
                        );
                    }
                    decision.hook_run_id = Some(run_id.clone());
                    decision.completion_order = Some(completed.len());
                    finish_run(
                        &self.runs,
                        &run_id,
                        &run_event,
                        started,
                        Ok(&output),
                        Some(&decision),
                    );
                    completed.push((configured_order, decision));
                }
                Ok((_, run_id, run_event, started, _, Err(error))) => {
                    finish_run(&self.runs, &run_id, &run_event, started, Err(&error), None);
                    warn!(%event, %error, "command hook failed open");
                }
                Err(error) => warn!(%event, %error, "command hook task failed open"),
            }
        }
        completed.sort_by_key(|(configured_order, _)| *configured_order);
        completed
            .into_iter()
            .map(|(_, decision)| decision)
            .collect()
    }
}

fn memory_consolidation_source_allowed(source: HookSource) -> bool {
    !matches!(
        source,
        HookSource::User | HookSource::Project | HookSource::SessionFlags | HookSource::Plugin
    )
}

fn handler_id(
    source: &Path,
    event: &str,
    group_index: usize,
    handler_index: usize,
    matcher: Option<&str>,
    identity: &str,
) -> String {
    let mut digest = Sha256::new();
    digest.update(source.to_string_lossy().as_bytes());
    digest.update([0]);
    for part in [
        event,
        &group_index.to_string(),
        &handler_index.to_string(),
        matcher.unwrap_or(""),
        identity,
    ] {
        digest.update(part.as_bytes());
        digest.update([0]);
    }
    format!("hook-{:x}", digest.finalize())
}

fn normalize_timeout(event: HookEvent, requested: Option<Duration>) -> Duration {
    if matches!(event, HookEvent::SessionEnd | HookEvent::Interrupt) {
        let seconds = requested
            .unwrap_or(SESSION_END_TIMEOUT)
            .as_secs()
            .clamp(1, MAX_SESSION_END_TIMEOUT.as_secs());
        Duration::from_secs(seconds)
    } else {
        Duration::from_secs(requested.unwrap_or(DEFAULT_TIMEOUT).as_secs().max(1))
    }
}

fn normalized_handler_config(
    kind: &ConfiguredHandlerKind,
    timeout: Duration,
    asynchronous: bool,
    status_message: Option<String>,
    additional_context_limit: Option<usize>,
) -> HookHandlerConfig {
    match kind {
        ConfiguredHandlerKind::Command { command } => HookHandlerConfig::Command {
            command: command.clone(),
            command_windows: None,
            timeout_sec: Some(timeout.as_secs()),
            r#async: asynchronous,
            status_message,
            additional_context_limit: additional_context_limit
                .filter(|limit| *limit != DEFAULT_ADDITIONAL_CONTEXT_TOKEN_LIMIT),
        },
        ConfiguredHandlerKind::McpTool {
            server,
            tool,
            input,
        } => HookHandlerConfig::McpTool {
            server: server.clone(),
            tool: tool.clone(),
            input: input.clone(),
            timeout_sec: Some(timeout.as_secs()),
            status_message,
        },
    }
}

#[derive(Serialize)]
struct NormalizedHookIdentity {
    event_name: &'static str,
    #[serde(flatten)]
    group: MatcherGroup,
}

fn hook_hash(
    event: HookEvent,
    matcher: Option<&str>,
    group: &MatcherGroup,
    handler: HookHandlerConfig,
) -> String {
    let mut group = group.clone();
    group.matcher = matcher.map(ToOwned::to_owned);
    group.hooks = vec![handler];
    let identity = NormalizedHookIdentity {
        event_name: event.key_label(),
        group,
    };
    let value = toml::Value::try_from(identity).expect("normalized hook identity serializes");
    let json = serde_json::to_value(value).unwrap_or(Value::Null);
    let serialized = serde_json::to_vec(&canonical_json(&json)).unwrap_or_default();
    let mut digest = Sha256::new();
    digest.update(serialized);
    format!("sha256:{:x}", digest.finalize())
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            Value::Object(
                keys.into_iter()
                    .filter_map(|key| {
                        map.get(key)
                            .map(|value| (key.clone(), canonical_json(value)))
                    })
                    .collect(),
            )
        }
        Value::Array(values) => Value::Array(values.iter().map(canonical_json).collect()),
        _ => value.clone(),
    }
}

fn hook_key(source: &Path, event: HookEvent, group_index: usize, handler_index: usize) -> String {
    crate::trust::canonical_key(&format!(
        "{}:{}:{group_index}:{handler_index}",
        source.display(),
        event.key_label()
    ))
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
    result: Result<&HandlerOutput, &anyhow::Error>,
    decision: Option<&CommandHookDecision>,
) {
    let duration_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
    let (status, summary) = match result {
        Ok(_output) if decision.is_some_and(|decision| decision.error.is_some()) => (
            HookRunStatus::Failed,
            decision
                .and_then(|decision| decision.error.clone())
                .unwrap_or_else(|| format!("{event} hook returned invalid output")),
        ),
        Ok(_output) if decision.is_some_and(|decision| decision.keep_going.is_some()) => (
            HookRunStatus::Blocked,
            decision
                .and_then(|decision| decision.keep_going.clone())
                .unwrap_or_else(|| format!("{event} hook requested continuation")),
        ),
        Ok(_output) if decision.is_some_and(|decision| decision.stop_reason.is_some()) => (
            HookRunStatus::Stopped,
            decision
                .and_then(|decision| decision.stop_reason.clone())
                .unwrap_or_else(|| format!("{event} hook stopped processing")),
        ),
        Ok(output)
            if output.exit_code == Some(2)
                || decision.is_some_and(|decision| decision.block_reason.is_some()) =>
        {
            (
                HookRunStatus::Blocked,
                decision
                    .and_then(|decision| decision.block_reason.clone())
                    .or_else(|| stderr_reason(output))
                    .unwrap_or_else(|| format!("{event} hook blocked")),
            )
        }
        Ok(output) if output.exit_code == Some(0) => {
            (HookRunStatus::Completed, format!("{event} hook completed"))
        }
        Ok(output) => (
            HookRunStatus::Failed,
            stderr_reason(output).unwrap_or_else(|| {
                output.exit_code.map_or_else(
                    || format!("{event} hook exited without a status code"),
                    |code| format!("{event} hook exited with status {code}"),
                )
            }),
        ),
        Err(error) => (HookRunStatus::Failed, error.to_string()),
    };
    let entries = decision.map_or_else(
        || {
            (status == HookRunStatus::Failed)
                .then(|| HookOutputEntry {
                    kind: HookOutputEntryKind::Error,
                    text: summary.clone(),
                })
                .into_iter()
                .collect()
        },
        decision_entries,
    );
    runs.finish(run_id, status, summary, duration_ms, entries);
}

fn decision_entries(decision: &CommandHookDecision) -> Vec<HookOutputEntry> {
    let mut entries = Vec::new();
    entries.extend(
        decision
            .warnings
            .iter()
            .cloned()
            .map(|text| HookOutputEntry {
                kind: HookOutputEntryKind::Warning,
                text,
            }),
    );
    if let Some(text) = &decision.stop_reason {
        entries.push(HookOutputEntry {
            kind: HookOutputEntryKind::Stop,
            text: text.clone(),
        });
    }
    if let Some(text) = &decision.block_reason {
        entries.push(HookOutputEntry {
            kind: HookOutputEntryKind::Feedback,
            text: text.clone(),
        });
    }
    if let Some(text) = &decision.additional_context {
        entries.push(HookOutputEntry {
            kind: HookOutputEntryKind::Context,
            text: text.clone(),
        });
    }
    if let Some(text) = &decision.feedback {
        entries.push(HookOutputEntry {
            kind: HookOutputEntryKind::Feedback,
            text: text.clone(),
        });
    }
    if let Some(text) = &decision.error {
        entries.push(HookOutputEntry {
            kind: HookOutputEntryKind::Error,
            text: text.clone(),
        });
    }
    entries
}

fn hook_scope(event: &str) -> HookScope {
    if matches!(
        event,
        crate::SESSION_START | crate::SESSION_END | crate::SUBAGENT_START
    ) {
        HookScope::Thread
    } else {
        HookScope::Turn
    }
}

fn compile_matcher(matcher: Option<&str>) -> anyhow::Result<Option<Regex>> {
    match matcher.map(str::trim) {
        None | Some("") | Some("*") => Ok(None),
        Some(matcher) => Ok(Some(Regex::new(matcher)?)),
    }
}

fn matcher_values(event: HookEvent, payload: &HookPayload) -> Vec<String> {
    let value = match event {
        HookEvent::SessionStart => payload.source.clone(),
        HookEvent::SessionEnd => payload.reason.clone(),
        HookEvent::PreToolUse | HookEvent::PermissionRequest | HookEvent::PostToolUse => {
            payload.tool_name.clone()
        }
        HookEvent::PreCompact | HookEvent::PostCompact => payload.trigger.clone(),
        HookEvent::SubagentStart | HookEvent::SubagentStop => payload.agent_type.clone(),
        _ => None,
    };
    let mut values = value.into_iter().collect::<Vec<_>>();
    if matches!(
        event,
        HookEvent::PreToolUse | HookEvent::PermissionRequest | HookEvent::PostToolUse
    ) {
        values.extend(payload.matcher_aliases.iter().cloned());
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
    crate::shell::env_from_payload(event, payload)
        .into_iter()
        .map(|(key, value)| (key, truncate_env_value(&value)))
        .collect()
}

fn truncate_env_value(value: &str) -> String {
    if value.len() <= MAX_ENV_VALUE_BYTES {
        return value.to_string();
    }
    let boundary = value
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= MAX_ENV_VALUE_BYTES)
        .last()
        .unwrap_or(0);
    value[..boundary].to_string()
}

async fn maybe_spill_additional_context(
    session_id: &str,
    run_id: &str,
    text: String,
    configured_limit: Option<usize>,
) -> String {
    let token_limit = configured_limit.unwrap_or(DEFAULT_ADDITIONAL_CONTEXT_TOKEN_LIMIT);
    if token_limit == 0 || approximate_tokens(&text) <= token_limit {
        return text;
    }

    let safe_session = sanitize_path_component(session_id, "unknown-session");
    let safe_run = sanitize_path_component(run_id, "hook-run");
    let output_dir = std::env::temp_dir().join("hook_outputs").join(safe_session);
    let path = output_dir.join(format!("{safe_run}.txt"));
    let footer = format!("\n\nFull hook output saved to: {}", path.display());
    let preview_limit = token_limit.saturating_sub(approximate_tokens(&footer));
    let preview = truncate_to_token_budget(&text, preview_limit);
    if tokio::fs::create_dir_all(&output_dir).await.is_ok()
        && tokio::fs::write(&path, text.as_bytes()).await.is_ok()
    {
        format!("{preview}{footer}")
    } else {
        warn!(path = %path.display(), "failed to spill oversized hook additional context");
        preview
    }
}

fn approximate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

fn truncate_to_token_budget(text: &str, token_limit: usize) -> String {
    let char_limit = token_limit.saturating_mul(4);
    let chars = text.chars().collect::<Vec<_>>();
    if chars.len() <= char_limit {
        return text.to_string();
    }
    let marker = "\n... hook output truncated ...\n";
    let content_limit = char_limit.saturating_sub(marker.chars().count());
    let head = content_limit / 2;
    let tail = content_limit.saturating_sub(head);
    format!(
        "{}{}{}",
        chars[..head].iter().collect::<String>(),
        marker,
        chars[chars.len().saturating_sub(tail)..]
            .iter()
            .collect::<String>()
    )
}

fn sanitize_path_component(value: &str, fallback: &str) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        fallback.to_string()
    } else {
        sanitized
    }
}

#[derive(Debug)]
struct HandlerOutput {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

struct ProcessTreeGuard {
    process_id: Option<u32>,
    #[cfg(windows)]
    job: Option<WindowsJobObject>,
}

impl ProcessTreeGuard {
    fn disarm(&mut self) {
        self.process_id = None;
    }
}

impl Drop for ProcessTreeGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(process_id) = self.process_id.and_then(|id| i32::try_from(id).ok()) {
            // Command hooks run as process-group leaders. Cancellation must also terminate
            // descendants that inherited stdout/stderr, otherwise their readers can leak.
            unsafe {
                libc::kill(-process_id, libc::SIGKILL);
            }
        }
        #[cfg(windows)]
        if let Some(process_id) = self.process_id {
            if let Some(job) = self.job.as_ref() {
                let _ = job.terminate();
            } else {
                let _ = std::process::Command::new("taskkill")
                    .args(["/PID", &process_id.to_string(), "/T", "/F"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
            }
        }
    }
}

async fn run_handler(
    handler: &ConfiguredHandler,
    hook_input: &[u8],
    cwd: &Path,
    session_environment: &[(OsString, OsString)],
    hook_environment: &[(String, String)],
    mcp_executor: &dyn HookMcpExecutor,
) -> anyhow::Result<HandlerOutput> {
    match &handler.kind {
        ConfiguredHandlerKind::Command { command } => {
            run_command(
                handler,
                command,
                hook_input,
                cwd,
                session_environment,
                hook_environment,
            )
            .await
        }
        ConfiguredHandlerKind::McpTool {
            server,
            tool,
            input,
        } => {
            let hook_input: Value = serde_json::from_slice(hook_input)?;
            let input = expand_argument_template(input, &hook_input)?;
            let call = HookMcpCall {
                server: server.clone(),
                tool: tool.clone(),
                input,
                timeout: handler.timeout,
            };
            let stdout = tokio::time::timeout(handler.timeout, mcp_executor.execute(call))
                .await
                .map_err(|_| {
                    anyhow::anyhow!("MCP hook timed out after {}s", handler.timeout.as_secs())
                })??;
            anyhow::ensure!(
                stdout.len() <= MAX_OUTPUT_BYTES,
                "hook output exceeds 1 MiB"
            );
            Ok(HandlerOutput {
                exit_code: Some(0),
                stdout,
                stderr: String::new(),
            })
        }
    }
}

async fn run_command(
    handler: &ConfiguredHandler,
    command_text: &str,
    input: &[u8],
    cwd: &Path,
    session_environment: &[(OsString, OsString)],
    hook_environment: &[(String, String)],
) -> anyhow::Result<HandlerOutput> {
    let mut command = default_shell_command(session_environment);
    #[cfg(windows)]
    command.raw_arg(format!(r#""{command_text}""#));
    #[cfg(not(windows))]
    command.arg(command_text);
    command
        .env_clear()
        .envs(session_environment.iter().cloned())
        .envs(hook_environment.iter().cloned())
        .current_dir(if cwd.is_dir() { cwd } else { Path::new(".") })
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    scrub_non_inheritable_env_vars(command.as_std_mut());
    #[cfg(unix)]
    command.process_group(0);

    #[cfg(windows)]
    let mut process_tree_job = WindowsJobObject::create().ok();
    #[cfg(windows)]
    let child = match process_tree_job.as_ref() {
        Some(job) => match job.spawn_contained(&mut command) {
            Ok(child) => Ok(child),
            Err(_) => {
                process_tree_job = None;
                command.creation_flags(0);
                command.spawn()
            }
        },
        None => command.spawn(),
    };
    #[cfg(not(windows))]
    let child = command.spawn();

    let mut child = child?;
    let mut process_tree_guard = ProcessTreeGuard {
        process_id: child.id(),
        #[cfg(windows)]
        job: process_tree_job,
    };
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let mut stdout_task = tokio::spawn(read_capped(stdout));
    let mut stderr_task = tokio::spawn(read_capped(stderr));
    let mut stdin = child.stdin.take();
    let execution = async {
        if let Some(mut stdin) = stdin.take() {
            match stdin.write_all(input).await {
                Ok(()) => {
                    if let Err(error) = stdin.shutdown().await {
                        if error.kind() != ErrorKind::BrokenPipe {
                            return Err(error.into());
                        }
                    }
                }
                Err(error) if error.kind() == ErrorKind::BrokenPipe => {}
                Err(error) => {
                    return Err(error.into());
                }
            }
        }
        // Wait for the process and both pipe readers together. A reader can fail early when
        // the output limit is exceeded; observing that failure immediately prevents the child
        // from blocking forever on a full pipe until the outer timeout expires.
        let (status, stdout, stderr) = tokio::try_join!(
            async { child.wait().await.map_err(anyhow::Error::from) },
            async { (&mut stdout_task).await.map_err(anyhow::Error::from)? },
            async { (&mut stderr_task).await.map_err(anyhow::Error::from)? },
        )?;
        Ok::<_, anyhow::Error>(HandlerOutput {
            exit_code: status.code(),
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        })
    };
    match tokio::time::timeout(handler.timeout, execution).await {
        Ok(Ok(output)) => {
            #[cfg(windows)]
            if let Some(job) = process_tree_guard.job.as_ref() {
                let _ = job.preserve_descendants();
            }
            process_tree_guard.disarm();
            Ok(output)
        }
        Ok(Err(error)) => {
            terminate_child_tree(&mut child).await;
            process_tree_guard.disarm();
            stdout_task.abort();
            stderr_task.abort();
            Err(error)
        }
        Err(_) => {
            terminate_child_tree(&mut child).await;
            process_tree_guard.disarm();
            stdout_task.abort();
            stderr_task.abort();
            anyhow::bail!("timeout after {}s", handler.timeout.as_secs());
        }
    }
}

async fn terminate_child_tree(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(process_id) = child.id().and_then(|id| i32::try_from(id).ok()) {
        // The child is its own process-group leader, so a negative pid targets descendants too.
        unsafe {
            libc::kill(-process_id, libc::SIGKILL);
        }
    }
    let _ = child.kill().await;
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

fn default_shell_command(environment: &[(OsString, OsString)]) -> Command {
    #[cfg(windows)]
    let (environment_variable, fallback_program, argument) = ("COMSPEC", "cmd.exe", "/C");

    #[cfg(not(windows))]
    let (environment_variable, fallback_program, argument) = ("SHELL", "/bin/sh", "-lc");

    let program = environment
        .iter()
        .find(|(key, _)| {
            #[cfg(windows)]
            {
                key.to_str()
                    .is_some_and(|key| key.eq_ignore_ascii_case(environment_variable))
            }

            #[cfg(not(windows))]
            {
                key == OsStr::new(environment_variable)
            }
        })
        .map(|(_, value)| value.clone())
        .unwrap_or_else(|| OsString::from(fallback_program));

    let mut command = Command::new(program);
    command.arg(argument);
    command
}

fn scrub_non_inheritable_env_vars(command: &mut std::process::Command) {
    let configured_names = command
        .get_envs()
        .map(|(name, _)| name.to_os_string())
        .collect::<Vec<_>>();

    for name in NON_INHERITABLE_ENV_VARS {
        command.env_remove(name);
    }
    for name in std::env::vars_os()
        .map(|(name, _)| name)
        .chain(configured_names)
    {
        if name.to_str().is_some_and(|name| {
            NON_INHERITABLE_ENV_VARS
                .iter()
                .any(|restricted| restricted.eq_ignore_ascii_case(name))
        }) {
            command.env_remove(name);
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct WireOutput {
    #[serde(default = "default_true", rename = "continue")]
    continue_processing: bool,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    system_message: Option<String>,
    #[serde(default)]
    suppress_output: bool,
    #[serde(default)]
    decision: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    hook_specific_output: Option<HookSpecificOutput>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
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
    #[serde(default, rename = "updatedMCPToolOutput")]
    updated_mcp_tool_output: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
struct PermissionDecision {
    behavior: String,
    #[serde(default)]
    updated_input: Option<Value>,
    #[serde(default)]
    updated_permissions: Option<Value>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    interrupt: bool,
}

const fn default_true() -> bool {
    true
}

fn stderr_reason(output: &HandlerOutput) -> Option<String> {
    let reason = output.stderr.trim();
    (!reason.is_empty()).then(|| reason.to_string())
}

fn parse_output(
    event: &str,
    output: &HandlerOutput,
    can_apply_control_effects: bool,
) -> CommandHookDecision {
    let stderr = output.stderr.trim();
    if output.exit_code == Some(2) {
        if !can_apply_control_effects || !supports_exit_two_block(event) {
            return invalid_output("hook exited with code 2");
        }
        if stderr.is_empty() {
            return invalid_output(format!(
                "{event} hook exited with code 2 but did not provide a reason"
            ));
        }
        return CommandHookDecision {
            block_reason: Some(stderr.into()),
            ..Default::default()
        };
    }
    if output.exit_code != Some(0) {
        warn!(%event, exit_code = ?output.exit_code, stderr, "hook exited unsuccessfully");
        return invalid_output(output.exit_code.map_or_else(
            || "hook exited without a status code".to_string(),
            |code| format!("hook exited with code {code}"),
        ));
    }
    let stdout = output.stdout.trim();
    if stdout.is_empty() {
        return CommandHookDecision::default();
    }
    if event == crate::SESSION_END {
        return CommandHookDecision::default();
    }
    let wire = match serde_json::from_str::<Value>(stdout).and_then(|value| {
        if !event_output_has_codex_shape(event, &value) {
            return Err(serde_json::Error::io(std::io::Error::new(
                ErrorKind::InvalidData,
                format!("fields do not match the {event} output schema"),
            )));
        }
        serde_json::from_value::<WireOutput>(value)
    }) {
        Ok(wire) => wire,
        Err(_)
            if matches!(
                event,
                crate::SESSION_START | crate::SUBAGENT_START | crate::USER_PROMPT_SUBMIT
            ) && !looks_like_json(stdout) =>
        {
            return CommandHookDecision {
                additional_context: Some(stdout.into()),
                ..Default::default()
            };
        }
        Err(error) => {
            warn!(%event, %error, "invalid hook JSON output; ignored");
            return invalid_output(format!("hook returned invalid {event} JSON output"));
        }
    };
    let specific_present = wire.hook_specific_output.is_some();
    let specific = wire.hook_specific_output.unwrap_or_default();
    if specific_present && specific.hook_event_name.as_deref() != Some(event) {
        warn!(%event, "hook output has a missing or mismatched hookEventName; ignored");
        return invalid_output(format!(
            "{event} hook returned hookSpecificOutput without the matching hookEventName"
        ));
    }
    let warnings = wire.system_message.into_iter().collect::<Vec<_>>();
    let stop_reason = wire
        .stop_reason
        .as_deref()
        .filter(|reason| !reason.trim().is_empty())
        .map_or_else(|| "hook requested stop".into(), ToOwned::to_owned);
    if can_apply_control_effects && !wire.continue_processing && supports_continue_false(event) {
        let feedback = (event == crate::POST_TOOL_USE).then(|| {
            wire.reason
                .as_deref()
                .and_then(trimmed_reason)
                .unwrap_or_else(|| stop_reason.clone())
        });
        return CommandHookDecision {
            stop_reason: Some(stop_reason),
            feedback,
            warnings,
            ..Default::default()
        };
    }
    let mut decision = CommandHookDecision {
        additional_context: specific.additional_context,
        warnings,
        ..Default::default()
    };
    if !can_apply_control_effects {
        return decision;
    }
    match event {
        crate::PRE_TOOL_USE => {
            if !wire.continue_processing {
                return invalid_output_with_warnings(
                    "PreToolUse hook returned unsupported continue:false",
                    decision.warnings,
                );
            }
            if wire.stop_reason.is_some() {
                return invalid_output_with_warnings(
                    "PreToolUse hook returned unsupported stopReason",
                    decision.warnings,
                );
            }
            if wire.suppress_output {
                return invalid_output_with_warnings(
                    "PreToolUse hook returned unsupported suppressOutput",
                    decision.warnings,
                );
            }
            match specific.permission_decision.as_deref() {
                Some("allow") if specific.updated_input.is_none() => {
                    decision.error = Some(
                        "PreToolUse hook returned permissionDecision:allow without updatedInput"
                            .into(),
                    )
                }
                Some("allow") => decision.updated_input = specific.updated_input,
                Some("deny") => {
                    decision.block_reason = specific
                        .permission_decision_reason
                        .as_deref()
                        .and_then(trimmed_reason);
                    if decision.block_reason.is_none() {
                        decision.error = Some("PreToolUse hook returned permissionDecision:deny without a non-empty permissionDecisionReason".into());
                    }
                }
                Some("ask") => {
                    decision.error =
                        Some("PreToolUse hook returned unsupported permissionDecision:ask".into())
                }
                Some(other) => {
                    decision.error = Some(format!(
                        "PreToolUse hook returned unknown permissionDecision:{other}"
                    ))
                }
                None if specific.updated_input.is_some() => {
                    decision.error = Some(
                        "PreToolUse hook returned updatedInput without permissionDecision:allow"
                            .into(),
                    )
                }
                None if specific.permission_decision_reason.is_some() => decision.error = Some(
                    "PreToolUse hook returned permissionDecisionReason without permissionDecision"
                        .into(),
                ),
                None if wire.decision.as_deref() == Some("approve") => {
                    decision.error =
                        Some("PreToolUse hook returned unsupported decision:approve".into())
                }
                None if wire.decision.as_deref() == Some("block") => {
                    decision.block_reason = wire.reason.as_deref().and_then(trimmed_reason);
                    if decision.block_reason.is_none() {
                        decision.error = Some(
                            "PreToolUse hook returned decision:block without a non-empty reason"
                                .into(),
                        );
                    }
                }
                None if wire.reason.is_some() => {
                    decision.error = Some("PreToolUse hook returned reason without decision".into())
                }
                None if wire.decision.is_some() => {
                    decision.error = Some("PreToolUse hook returned an unknown decision".into())
                }
                None => {}
            }
        }
        crate::PERMISSION_REQUEST => {
            if !wire.continue_processing {
                return invalid_output_with_warnings(
                    "PermissionRequest hook returned unsupported continue:false",
                    decision.warnings,
                );
            }
            if wire.stop_reason.is_some() {
                return invalid_output_with_warnings(
                    "PermissionRequest hook returned unsupported stopReason",
                    decision.warnings,
                );
            }
            if wire.suppress_output {
                return invalid_output_with_warnings(
                    "PermissionRequest hook returned unsupported suppressOutput",
                    decision.warnings,
                );
            }
            if wire.decision.is_some() || wire.reason.is_some() {
                return invalid_output_with_warnings(
                    "PermissionRequest hook returned unsupported top-level decision fields",
                    decision.warnings,
                );
            }
            if let Some(permission) = specific.decision {
                if permission.updated_input.is_some() {
                    decision.error =
                        Some("PermissionRequest hook returned unsupported updatedInput".into());
                    return decision;
                }
                if permission.updated_permissions.is_some() {
                    decision.error = Some(
                        "PermissionRequest hook returned unsupported updatedPermissions".into(),
                    );
                    return decision;
                }
                if permission.interrupt {
                    decision.error =
                        Some("PermissionRequest hook returned unsupported interrupt:true".into());
                    return decision;
                }
                decision.permission = match permission.behavior.as_str() {
                    "allow" => Some(PermissionVote::Allow),
                    "deny" => Some(PermissionVote::Deny),
                    other => {
                        decision.error = Some(format!(
                            "PermissionRequest hook returned unknown behavior:{other}"
                        ));
                        None
                    }
                };
                if decision.permission == Some(PermissionVote::Deny) {
                    decision.block_reason = permission
                        .message
                        .as_deref()
                        .and_then(trimmed_reason)
                        .or_else(|| Some("PermissionRequest hook denied approval".into()));
                }
            }
        }
        crate::STOP | crate::SUBAGENT_STOP if wire.decision.as_deref() == Some("block") => {
            decision.keep_going = wire.reason.as_deref().and_then(trimmed_reason);
            if decision.keep_going.is_none() {
                decision.error = Some(format!(
                    "{event} hook returned decision:block without a non-empty reason"
                ));
            }
        }
        crate::POST_TOOL_USE | crate::USER_PROMPT_SUBMIT
            if wire.decision.as_deref() == Some("block") =>
        {
            decision.block_reason = wire.reason.as_deref().and_then(trimmed_reason);
            if decision.block_reason.is_none() {
                decision.error = Some(format!(
                    "{event} hook returned decision:block without a non-empty reason"
                ));
            }
        }
        _ => {}
    }
    if event == crate::POST_TOOL_USE {
        if wire.suppress_output {
            decision.error = Some("PostToolUse hook returned unsupported suppressOutput".into());
        } else if specific.updated_mcp_tool_output.is_some() {
            decision.error =
                Some("PostToolUse hook returned unsupported updatedMCPToolOutput".into());
        } else if wire.decision.is_none() && wire.reason.is_some() {
            decision.error = Some("PostToolUse hook returned reason without decision".into());
        }
    }
    if decision.error.is_some() {
        decision.block_reason = None;
        decision.permission = None;
        decision.updated_input = None;
        decision.additional_context = None;
        decision.feedback = None;
        decision.keep_going = None;
        decision.stop_reason = None;
    }
    decision
}

fn event_output_has_codex_shape(event: &str, value: &Value) -> bool {
    const UNIVERSAL: &[&str] = &["continue", "stopReason", "suppressOutput", "systemMessage"];
    const DECISION: &[&str] = &["decision", "reason"];
    const PRE_TOOL: &[&str] = &[
        "hookEventName",
        "additionalContext",
        "updatedInput",
        "permissionDecision",
        "permissionDecisionReason",
    ];
    const POST_TOOL: &[&str] = &["hookEventName", "additionalContext", "updatedMCPToolOutput"];
    const PERMISSION: &[&str] = &["hookEventName", "decision"];
    const CONTEXT: &[&str] = &["hookEventName", "additionalContext"];

    let Some(object) = value.as_object() else {
        return false;
    };
    let (top_level, specific) = match event {
        crate::PRE_TOOL_USE => (
            merge_fields(UNIVERSAL, &["decision", "reason", "hookSpecificOutput"]),
            Some(PRE_TOOL),
        ),
        crate::POST_TOOL_USE | crate::USER_PROMPT_SUBMIT => (
            merge_fields(UNIVERSAL, &["decision", "reason", "hookSpecificOutput"]),
            Some(if event == crate::POST_TOOL_USE {
                POST_TOOL
            } else {
                CONTEXT
            }),
        ),
        crate::PERMISSION_REQUEST => (
            merge_fields(UNIVERSAL, &["hookSpecificOutput"]),
            Some(PERMISSION),
        ),
        crate::SESSION_START | crate::SUBAGENT_START => (
            merge_fields(UNIVERSAL, &["hookSpecificOutput"]),
            Some(CONTEXT),
        ),
        crate::PRE_COMPACT | crate::POST_COMPACT => (UNIVERSAL.to_vec(), None),
        crate::STOP | crate::SUBAGENT_STOP => (merge_fields(UNIVERSAL, DECISION), None),
        crate::INTERRUPT => (vec!["systemMessage"], None),
        _ => return false,
    };
    if object.keys().any(|key| !top_level.contains(&key.as_str())) {
        return false;
    }

    match (object.get("hookSpecificOutput"), specific) {
        (None | Some(Value::Null), _) => true,
        (Some(Value::Object(output)), Some(allowed)) => {
            output.keys().all(|key| allowed.contains(&key.as_str()))
        }
        (Some(_), Some(_)) => true,
        (Some(_), None) => false,
    }
}

fn merge_fields<'a>(left: &'a [&'a str], right: &'a [&'a str]) -> Vec<&'a str> {
    left.iter().chain(right).copied().collect()
}

fn trimmed_reason(reason: &str) -> Option<String> {
    let reason = reason.trim();
    (!reason.is_empty()).then(|| reason.to_string())
}

fn looks_like_json(output: &str) -> bool {
    let output = output.trim_start();
    output.starts_with('{') || output.starts_with('[')
}

fn supports_continue_false(event: &str) -> bool {
    matches!(
        event,
        crate::SESSION_START
            | crate::PRE_COMPACT
            | crate::POST_COMPACT
            | crate::POST_TOOL_USE
            | crate::USER_PROMPT_SUBMIT
            | crate::STOP
            | crate::SUBAGENT_STOP
    )
}

fn supports_exit_two_block(event: &str) -> bool {
    matches!(
        event,
        crate::PRE_TOOL_USE
            | crate::PERMISSION_REQUEST
            | crate::POST_TOOL_USE
            | crate::USER_PROMPT_SUBMIT
            | crate::STOP
            | crate::SUBAGENT_STOP
    )
}

fn invalid_output(reason: impl Into<String>) -> CommandHookDecision {
    CommandHookDecision {
        error: Some(reason.into()),
        ..Default::default()
    }
}

fn invalid_output_with_warnings(
    reason: impl Into<String>,
    warnings: Vec<String>,
) -> CommandHookDecision {
    CommandHookDecision {
        error: Some(reason.into()),
        warnings,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingMcpExecutor {
        calls: Mutex<Vec<HookMcpCall>>,
        output: String,
    }

    impl HookMcpExecutor for RecordingMcpExecutor {
        fn execute(
            &self,
            call: HookMcpCall,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send + '_>>
        {
            self.calls.lock().unwrap().push(call);
            let output = self.output.clone();
            Box::pin(async move { Ok(output) })
        }
    }

    async fn wait_for_count(counter: &AtomicUsize, expected: usize) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while counter.load(AtomicOrdering::SeqCst) < expected {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }

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
        std::fs::write(
            project.join(".astro/config.toml"),
            "[config_sources]\nhooks=['hooks.json']\n",
        )
        .unwrap();
        (root, astro_home, cwd)
    }

    fn file(command: String, matcher: Option<&str>, asynchronous: bool) -> HooksFile {
        HooksFile {
            hooks: HashMap::from([(
                crate::PRE_TOOL_USE.into(),
                vec![MatcherGroup {
                    id: None,
                    matcher: matcher.map(str::to_string),
                    hooks: vec![HookHandlerConfig::Command {
                        command,
                        command_windows: None,
                        timeout_sec: Some(2),
                        r#async: asynchronous,
                        status_message: None,
                        additional_context_limit: None,
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
    fn mcp_hook_input_must_have_a_stable_toml_representation() {
        let error = serde_json::from_str::<HooksFile>(
            r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"mcp_tool","server":"policy","tool":"authorize","input":{"value":null}}]}]}}"#,
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("MCP hook input must be representable as TOML"));
    }

    #[tokio::test]
    async fn mcp_hook_expands_input_and_uses_normal_output_semantics() {
        let executor = Arc::new(RecordingMcpExecutor {
            output: r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{"safe":true}}}"#.into(),
            ..Default::default()
        });
        let runner = CommandHookRunner::from_file(
            HooksFile {
                hooks: HashMap::from([(
                    crate::PRE_TOOL_USE.into(),
                    vec![MatcherGroup {
                        id: None,
                        matcher: Some("terminal".into()),
                        hooks: vec![HookHandlerConfig::McpTool {
                            server: "policy".into(),
                            tool: "authorize".into(),
                            input: serde_json::from_value(serde_json::json!({
                                "tool": "${tool_name}",
                                "arguments": "${tool_input}"
                            }))
                            .unwrap(),
                            timeout_sec: Some(2),
                            status_message: None,
                        }],
                    }],
                )]),
                ..Default::default()
            },
            Path::new("hooks.json"),
        )
        .unwrap()
        .with_mcp_executor(executor.clone());

        let decisions = runner
            .run(
                crate::PRE_TOOL_USE,
                &HookPayload {
                    session_id: "session-1".into(),
                    cwd: std::env::current_dir()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    tool_name: Some("terminal".into()),
                    tool_input: Some(serde_json::json!({"command":"pwd"})),
                    ..Default::default()
                },
            )
            .await;

        assert_eq!(decisions.len(), 1);
        assert_eq!(
            decisions[0].updated_input,
            Some(serde_json::json!({"safe":true}))
        );
        let calls = executor.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].server, "policy");
        assert_eq!(calls[0].tool, "authorize");
        assert_eq!(calls[0].input["tool"], "terminal");
        assert_eq!(
            calls[0].input["arguments"],
            serde_json::json!({"command":"pwd"})
        );
        assert_eq!(runner.list()[0].handler_type, HookHandlerType::McpTool);
    }

    #[tokio::test]
    async fn session_end_mcp_hook_uses_the_codex_reason_payload() {
        let executor = Arc::new(RecordingMcpExecutor::default());
        let runner = CommandHookRunner::from_file(
            HooksFile {
                hooks: HashMap::from([(
                    crate::SESSION_END.into(),
                    vec![MatcherGroup {
                        id: None,
                        matcher: Some("other".into()),
                        hooks: vec![HookHandlerConfig::McpTool {
                            server: "audit".into(),
                            tool: "close".into(),
                            input: serde_json::from_value(serde_json::json!({
                                "reason": "${reason}"
                            }))
                            .unwrap(),
                            timeout_sec: Some(2),
                            status_message: None,
                        }],
                    }],
                )]),
                ..Default::default()
            },
            Path::new("hooks.json"),
        )
        .unwrap()
        .with_mcp_executor(executor.clone());

        let decisions = runner
            .run(
                crate::SESSION_END,
                &HookPayload {
                    session_id: "session-1".into(),
                    cwd: std::env::current_dir()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    reason: Some("other".into()),
                    ..Default::default()
                },
            )
            .await;

        assert_eq!(decisions.len(), 1);
        let calls = executor.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].server, "audit");
        assert_eq!(calls[0].tool, "close");
        assert_eq!(calls[0].input["reason"], "other");
    }

    #[test]
    fn pre_tool_use_rejects_updated_input_without_allow() {
        let decision = parse_output(
            crate::PRE_TOOL_USE,
            &HandlerOutput {
                exit_code: Some(0),
                stdout: r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","updatedInput":{"safe":true}}}"#.into(),
                stderr: String::new(),
            },
            true,
        );

        assert_eq!(decision.updated_input, None);
        assert_eq!(
            decision.error.as_deref(),
            Some("PreToolUse hook returned updatedInput without permissionDecision:allow")
        );
    }

    #[test]
    fn permission_request_rejects_universal_stop_fields() {
        let decision = parse_output(
            crate::PERMISSION_REQUEST,
            &HandlerOutput {
                exit_code: Some(0),
                stdout: r#"{"continue":false,"stopReason":"stop"}"#.into(),
                stderr: String::new(),
            },
            true,
        );

        assert_eq!(decision.permission, None);
        assert_eq!(
            decision.error.as_deref(),
            Some("PermissionRequest hook returned unsupported continue:false")
        );
    }

    #[test]
    fn json_like_invalid_user_prompt_output_fails_open() {
        let decision = parse_output(
            crate::USER_PROMPT_SUBMIT,
            &HandlerOutput {
                exit_code: Some(0),
                stdout: r#"{"unexpected":true}"#.into(),
                stderr: String::new(),
            },
            true,
        );

        assert!(decision.additional_context.is_none());
        assert!(decision.error.is_some());
    }

    #[test]
    fn output_fields_are_validated_against_each_codex_event_schema() {
        for (event, stdout) in [
            (crate::INTERRUPT, r#"{"continue":false}"#),
            (
                crate::PRE_COMPACT,
                r#"{"hookSpecificOutput":{"hookEventName":"PreCompact"}}"#,
            ),
            (crate::SESSION_START, r#"{"decision":"block"}"#),
            (
                crate::STOP,
                r#"{"hookSpecificOutput":{"hookEventName":"Stop"}}"#,
            ),
            (crate::PERMISSION_REQUEST, r#"{"reason":"denied"}"#),
        ] {
            let decision = parse_output(
                event,
                &HandlerOutput {
                    exit_code: Some(0),
                    stdout: stdout.into(),
                    stderr: String::new(),
                },
                true,
            );
            assert_eq!(
                decision.error,
                Some(format!("hook returned invalid {event} JSON output"))
            );
        }

        let session_end = parse_output(
            crate::SESSION_END,
            &HandlerOutput {
                exit_code: Some(0),
                stdout: r#"{"ignored":true}"#.into(),
                stderr: String::new(),
            },
            true,
        );
        assert!(session_end.error.is_none());
        assert!(session_end.block_reason.is_none());
        assert!(session_end.stop_reason.is_none());
    }

    #[tokio::test]
    async fn oversized_additional_context_is_spilled_with_recovery_path() {
        let context = "x".repeat(512);
        let command = format!(
            "printf '%s' '{}'",
            serde_json::json!({"hookSpecificOutput": {
                "hookEventName": "UserPromptSubmit",
                "additionalContext": context
            }})
        );
        let mut file = file(command, None, false);
        let HookHandlerConfig::Command {
            additional_context_limit,
            ..
        } = &mut file.hooks.get_mut(crate::PRE_TOOL_USE).unwrap()[0].hooks[0]
        else {
            unreachable!()
        };
        *additional_context_limit = Some(8);
        let groups = file.hooks.remove(crate::PRE_TOOL_USE).unwrap();
        file.hooks.insert(crate::USER_PROMPT_SUBMIT.into(), groups);
        let runner = CommandHookRunner::from_file(file, Path::new("hooks.json")).unwrap();

        let decisions = runner
            .run(
                crate::USER_PROMPT_SUBMIT,
                &HookPayload {
                    session_id: "spill-test".into(),
                    cwd: std::env::current_dir()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    prompt: Some("hello".into()),
                    ..Default::default()
                },
            )
            .await;

        let preview = decisions[0].additional_context.as_deref().unwrap();
        assert!(preview.contains("Full hook output saved to:"));
        let path = preview.rsplit_once(": ").unwrap().1;
        assert_eq!(std::fs::read_to_string(path).unwrap(), context);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn asynchronous_additional_context_is_spilled_before_completion() {
        let context = "y".repeat(512);
        let command = format!(
            "printf '%s' '{}'",
            serde_json::json!({"hookSpecificOutput": {
                "hookEventName": "UserPromptSubmit",
                "additionalContext": context
            }})
        );
        let mut file = file(command, None, true);
        let HookHandlerConfig::Command {
            additional_context_limit,
            ..
        } = &mut file.hooks.get_mut(crate::PRE_TOOL_USE).unwrap()[0].hooks[0]
        else {
            unreachable!()
        };
        *additional_context_limit = Some(8);
        let groups = file.hooks.remove(crate::PRE_TOOL_USE).unwrap();
        file.hooks.insert(crate::USER_PROMPT_SUBMIT.into(), groups);
        let runner = CommandHookRunner::from_file(file, Path::new("hooks.json")).unwrap();

        assert!(runner
            .run(
                crate::USER_PROMPT_SUBMIT,
                &HookPayload {
                    session_id: "async-spill-test".into(),
                    cwd: std::env::current_dir()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    prompt: Some("hello".into()),
                    ..Default::default()
                },
            )
            .await
            .is_empty());

        let preview = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let runs = runner.recent_runs();
                if let Some(entry) = runs
                    .first()
                    .filter(|run| run.status == HookRunStatus::Completed)
                    .and_then(|run| run.entries.first())
                {
                    break entry.text.clone();
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();

        assert!(preview.contains("Full hook output saved to:"));
        let path = preview.rsplit_once(": ").unwrap().1;
        assert_eq!(std::fs::read_to_string(path).unwrap(), context);
        let _ = std::fs::remove_file(path);
        runner.shutdown().await;
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
    fn hooks_are_discovered_from_config_toml() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join("config.toml"),
            r#"
                [hooks]
                [hooks.state."legacy-hook-key"]
                enabled = false

                [[hooks.PreToolUse]]
                matcher = "terminal"

                [[hooks.PreToolUse.hooks]]
                type = "command"
                command = "true"
                additionalContextLimit = 4096
            "#,
        )
        .unwrap();

        let runner = CommandHookRunner::load(root.path()).unwrap();

        assert_eq!(runner.handler_count(), 1);
        let hooks = runner.list();
        assert_eq!(hooks[0].handler_type, HookHandlerType::Command);
        assert_eq!(hooks[0].additional_context_limit, Some(4096));
        assert!(hooks[0].source.ends_with("config.toml"));
    }

    #[test]
    fn explicit_rule_identity_survives_reordering_and_project_override() {
        let (_root, astro_home, cwd) = project_fixture(
            "trusted",
            r#"{"hooks":{"PreToolUse":[{"id":"shared","hooks":[{"type":"command","command":"project"}]}]}}"#,
        );
        let global = astro_home.join("config.toml");
        let trust = std::fs::read_to_string(&global).unwrap();
        let rule = |id: &str| {
            format!("\n[[hooks.PreToolUse]]\nid={id:?}\n[[hooks.PreToolUse.hooks]]\ntype='command'\ncommand='global'\n")
        };
        std::fs::write(
            &global,
            format!("{trust}{}{}", rule("shared"), rule("keep")),
        )
        .unwrap();
        let first = CommandHookRunner::load(&astro_home).unwrap().list();
        std::fs::write(
            &global,
            format!("{trust}{}{}", rule("keep"), rule("shared")),
        )
        .unwrap();
        let second = CommandHookRunner::load(&astro_home).unwrap().list();
        for entry in &first {
            assert!(second
                .iter()
                .any(|other| other.id == entry.id && other.current_hash == entry.current_hash));
        }
        let effective = CommandHookRunner::load_for_project(&astro_home, &cwd)
            .unwrap()
            .list();
        assert_eq!(effective.len(), 2);
        assert_eq!(
            effective
                .iter()
                .filter(|h| h.command.as_deref() == Some("project"))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn invalid_configuration_runner_blocks_instead_of_disabling_hooks() {
        let runner = CommandHookRunner::blocked("fixture error");
        assert!(!runner.is_empty());
        let decisions = runner
            .run(crate::PRE_TOOL_USE, &HookPayload::default())
            .await;
        assert!(decisions[0]
            .block_reason
            .as_ref()
            .unwrap()
            .contains("fixture error"));
    }

    #[tokio::test]
    async fn config_toml_state_can_disable_one_handler_by_stable_key() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("config.toml");
        let key = hook_key(&source, HookEvent::PreToolUse, 0, 0);
        let quoted_key = toml::Value::String(key.clone()).to_string();
        std::fs::write(
            &source,
            format!(
                r#"
                    [hooks.state.{quoted_key}]
                    enabled = false

                    [[hooks.PreToolUse]]
                    matcher = "terminal"

                    [[hooks.PreToolUse.hooks]]
                    type = "command"
                    command = "true"
                "#
            ),
        )
        .unwrap();

        let runner = CommandHookRunner::load(root.path()).unwrap();

        assert_eq!(runner.handler_count(), 1);
        assert_eq!(runner.list()[0].key, key);
        assert!(!runner.list()[0].enabled);
        let decisions = runner
            .run(
                crate::PRE_TOOL_USE,
                &HookPayload {
                    session_id: "disabled-hook".into(),
                    cwd: root.path().to_string_lossy().into_owned(),
                    tool_name: Some("terminal".into()),
                    ..Default::default()
                },
            )
            .await;
        assert!(decisions.is_empty());
    }

    #[test]
    fn trusted_hash_detects_modified_hook_content() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("config.toml");
        let hook_body = |command: &str, state: &str| {
            format!(
                r#"
                    [hooks]
                    {state}
                    [[hooks.PreToolUse]]
                    matcher = "terminal"

                    [[hooks.PreToolUse.hooks]]
                    type = "command"
                    command = {command:?}
                "#
            )
        };
        std::fs::write(&source, hook_body("true", "")).unwrap();
        let initial = CommandHookRunner::load(root.path())
            .unwrap()
            .list()
            .remove(0);
        assert!(initial.current_hash.starts_with("sha256:"));

        crate::trust::record(root.path(), &initial.key, &initial.current_hash).unwrap();
        let state = String::new();
        std::fs::write(&source, hook_body("true", &state)).unwrap();
        let trusted = CommandHookRunner::load(root.path())
            .unwrap()
            .list()
            .remove(0);
        assert_eq!(trusted.trust_status, HookTrustStatus::Trusted);
        assert!(trusted.enabled);

        std::fs::write(&source, hook_body("false", &state)).unwrap();
        let modified = CommandHookRunner::load(root.path())
            .unwrap()
            .list()
            .remove(0);
        assert_ne!(modified.current_hash, initial.current_hash);
        assert_eq!(modified.trust_status, HookTrustStatus::Modified);
        assert!(!modified.enabled);
    }

    #[test]
    fn untrusted_project_hook_content_is_not_parsed() {
        let (_root, astro_home, cwd) =
            project_fixture("untrusted", "this is intentionally invalid JSON");

        let runner = CommandHookRunner::load_for_project(&astro_home, &cwd).unwrap();

        assert_eq!(runner.handler_count(), 0);
        assert_eq!(runner.sources().len(), 2);
        assert_eq!(runner.sources()[0].trust, CommandHookTrust::Untrusted);
        assert!(!runner.sources()[0].enabled);
        assert_eq!(
            runner.sources()[0].reason.as_deref(),
            Some("project is untrusted")
        );
    }

    #[test]
    fn project_trust_is_re_evaluated_on_each_load() {
        let (_root, astro_home, cwd) = project_fixture(
            "trusted",
            r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"true"}]}]}}"#,
        );
        assert_eq!(
            CommandHookRunner::load_for_project(&astro_home, &cwd)
                .unwrap()
                .handler_count(),
            1
        );
        let project = cwd.parent().unwrap();
        std::fs::write(
            astro_home.join("config.toml"),
            format!(
                "[projects.{:?}]\ntrust_level = 'untrusted'\n",
                project.to_string_lossy()
            ),
        )
        .unwrap();

        let reloaded = CommandHookRunner::load_for_project(&astro_home, &cwd).unwrap();

        assert_eq!(reloaded.handler_count(), 0);
        assert_eq!(reloaded.sources()[0].trust, CommandHookTrust::Untrusted);
        assert!(!reloaded.sources()[0].enabled);
    }

    #[test]
    fn command_environment_includes_llm_telemetry_fields() {
        let environment = hook_environment(
            crate::POST_LLM_CALL,
            &HookPayload {
                provider: Some("provider-a".into()),
                model: "model-a".into(),
                attempt: Some(3),
                duration_ms: Some(42),
                status: Some("succeeded".into()),
                ..Default::default()
            },
        );
        let value = |key: &str| {
            environment
                .iter()
                .find(|(candidate, _)| candidate == key)
                .map(|(_, value)| value.as_str())
        };

        assert_eq!(value("ASTRO_HOOK_PROVIDER"), Some("provider-a"));
        assert_eq!(value("ASTRO_HOOK_MODEL"), Some("model-a"));
        assert_eq!(value("ASTRO_HOOK_ATTEMPT"), Some("3"));
        assert_eq!(value("ASTRO_HOOK_DURATION_MS"), Some("42"));
        assert_eq!(value("ASTRO_HOOK_STATUS"), Some("succeeded"));
    }

    #[test]
    #[cfg(unix)]
    fn default_shell_uses_the_session_snapshot_and_login_mode() {
        let environment = vec![(OsString::from("SHELL"), OsString::from("/bin/zsh"))];
        let command = default_shell_command(&environment);

        assert_eq!(command.as_std().get_program(), OsStr::new("/bin/zsh"));
        assert_eq!(
            command.as_std().get_args().collect::<Vec<_>>(),
            vec![OsStr::new("-lc")]
        );
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn command_replays_session_environment_and_scrubs_internal_tokens() {
        let command =
            r#"test "$ASTRO_HOOK_SNAPSHOT_TEST" = "captured" && test -z "$NODE_REPL_AUTH_TOKEN""#;
        let mut runner = CommandHookRunner::from_file(
            file(command.into(), Some("^Bash$"), false),
            Path::new("hooks.json"),
        )
        .unwrap();
        runner.environment = Arc::new(vec![
            (OsString::from("SHELL"), OsString::from("/bin/sh")),
            (
                OsString::from("ASTRO_HOOK_SNAPSHOT_TEST"),
                OsString::from("captured"),
            ),
            (
                OsString::from("NODE_REPL_AUTH_TOKEN"),
                OsString::from("secret"),
            ),
        ]);

        let decisions = runner
            .run(
                crate::PRE_TOOL_USE,
                &HookPayload {
                    session_id: "session-1".into(),
                    turn_id: Some("turn-1".into()),
                    cwd: std::env::current_dir()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    tool_name: Some("Bash".into()),
                    ..Default::default()
                },
            )
            .await;

        assert_eq!(decisions.len(), 1);
        assert!(decisions[0].error.is_none());
        assert_eq!(runner.recent_runs()[0].status, HookRunStatus::Completed);
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
        assert_eq!(runs[0].handler_type, HookHandlerType::Command);
        assert_eq!(runs[0].execution_mode, HookExecutionMode::Sync);
        assert_eq!(runs[0].scope, HookScope::Turn);
        assert_eq!(runs[0].status, HookRunStatus::Completed);
        assert!(runs[0].completed_at.is_some());
        assert!(runs[0].duration_ms.is_some());
    }

    #[tokio::test]
    async fn fast_exiting_hook_keeps_stdout_when_it_does_not_read_large_stdin() {
        let history_owner = CommandHookRunner::default();
        let runner = CommandHookRunner::from_file(
            file(
                r#"printf '%s' '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{"safe":true}}}'"#.into(),
                None,
                false,
            ),
            Path::new("hooks.json"),
        )
        .unwrap()
        .share_run_store(history_owner.run_store());

        let decisions = runner
            .run(
                crate::PRE_TOOL_USE,
                &HookPayload {
                    cwd: std::env::current_dir()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    tool_name: Some("terminal".into()),
                    detail: "x".repeat(1024 * 1024),
                    ..Default::default()
                },
            )
            .await;

        assert_eq!(decisions.len(), 1, "runs: {:?}", runner.recent_runs());
        assert_eq!(
            decisions[0].updated_input,
            Some(serde_json::json!({"safe": true}))
        );
        assert_eq!(history_owner.recent_runs().len(), 1);
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn timeout_covers_stdin_and_descendant_output_pipes() {
        let runner = CommandHookRunner::from_file(
            HooksFile {
                hooks: HashMap::from([(
                    crate::PRE_TOOL_USE.into(),
                    vec![MatcherGroup {
                        id: None,
                        matcher: None,
                        hooks: vec![HookHandlerConfig::Command {
                            command: "sleep 10 & wait".into(),
                            command_windows: None,
                            timeout_sec: Some(1),
                            r#async: false,
                            status_message: None,
                            additional_context_limit: None,
                        }],
                    }],
                )]),
                ..Default::default()
            },
            Path::new("hooks.json"),
        )
        .unwrap();

        let decisions = tokio::time::timeout(
            Duration::from_secs(3),
            runner.run(
                crate::PRE_TOOL_USE,
                &HookPayload {
                    cwd: std::env::current_dir()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    tool_name: Some("terminal".into()),
                    detail: "x".repeat(1024 * 1024),
                    ..Default::default()
                },
            ),
        )
        .await
        .expect("hook timeout must include stdin and output pipe handling");

        assert!(decisions.is_empty());
        let runs = runner.recent_runs();
        assert_eq!(runs[0].status, HookRunStatus::Failed);
        assert!(runs[0].summary.contains("timeout after 1s"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn oversized_output_fails_without_waiting_for_command_timeout() {
        let runner = CommandHookRunner::from_file(
            file("yes x".into(), None, false),
            Path::new("hooks.json"),
        )
        .unwrap();

        let decisions = tokio::time::timeout(
            Duration::from_secs(3),
            runner.run(
                crate::PRE_TOOL_USE,
                &HookPayload {
                    cwd: std::env::current_dir()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    tool_name: Some("terminal".into()),
                    ..Default::default()
                },
            ),
        )
        .await
        .expect("output limit failure must interrupt the command immediately");

        assert!(decisions.is_empty());
        let runs = runner.recent_runs();
        assert_eq!(runs[0].status, HookRunStatus::Failed);
        assert!(runs[0].summary.contains("hook output exceeds 1 MiB"));
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

    #[tokio::test]
    async fn successful_process_with_deny_decision_is_recorded_as_blocked() {
        let runner = CommandHookRunner::from_file(
            file(
                r#"printf '%s' '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"unsafe"}}'"#.into(),
                None,
                false,
            ),
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

        assert_eq!(decisions[0].block_reason.as_deref(), Some("unsafe"));
        let runs = runner.recent_runs();
        assert_eq!(runs[0].status, HookRunStatus::Blocked);
        assert_eq!(runs[0].summary, "unsafe");
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn concurrent_hook_results_keep_configuration_order() {
        let output =
            |reason: &str| format!(r#"printf '%s' '{{"continue":false,"stopReason":"{reason}"}}'"#);
        let runner = CommandHookRunner::from_file(
            HooksFile {
                hooks: HashMap::from([(
                    crate::USER_PROMPT_SUBMIT.into(),
                    vec![MatcherGroup {
                        id: None,
                        matcher: None,
                        hooks: vec![
                            HookHandlerConfig::Command {
                                command: format!("sleep 1; {}", output("first")),
                                command_windows: None,
                                timeout_sec: Some(2),
                                r#async: false,
                                status_message: None,
                                additional_context_limit: None,
                            },
                            HookHandlerConfig::Command {
                                command: output("second"),
                                command_windows: None,
                                timeout_sec: Some(2),
                                r#async: false,
                                status_message: None,
                                additional_context_limit: None,
                            },
                        ],
                    }],
                )]),
                ..Default::default()
            },
            Path::new("hooks.json"),
        )
        .unwrap();

        let decisions = runner
            .run(
                crate::USER_PROMPT_SUBMIT,
                &HookPayload {
                    cwd: std::env::current_dir()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    prompt: Some("hello".into()),
                    ..Default::default()
                },
            )
            .await;

        assert_eq!(decisions.len(), 2);
        assert_eq!(decisions[0].stop_reason.as_deref(), Some("first"));
        assert_eq!(decisions[1].stop_reason.as_deref(), Some("second"));
        assert!(runner
            .recent_runs()
            .iter()
            .all(|run| run.status == HookRunStatus::Stopped));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn memory_consolidation_runs_only_policy_sources() {
        let mut runner = CommandHookRunner::from_file(
            HooksFile {
                hooks: HashMap::from([(
                    crate::STOP.into(),
                    vec![MatcherGroup {
                        id: None,
                        matcher: None,
                        hooks: vec![HookHandlerConfig::Command {
                            command: r#"printf '%s' '{"decision":"block","reason":"policy"}'"#
                                .into(),
                            command_windows: None,
                            timeout_sec: Some(2),
                            r#async: false,
                            status_message: None,
                            additional_context_limit: None,
                        }],
                    }],
                )]),
                ..Default::default()
            },
            Path::new("hooks.json"),
        )
        .unwrap();
        runner.handlers.get_mut(&HookEvent::Stop).unwrap()[0].source_kind = HookSource::System;

        let decisions = runner
            .run_memory_consolidation(
                crate::STOP,
                &HookPayload {
                    session_id: "session-1".into(),
                    turn_id: Some("turn-1".into()),
                    cwd: std::env::current_dir()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    ..Default::default()
                },
            )
            .await;

        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].keep_going.as_deref(), Some("policy"));
        assert_eq!(runner.recent_runs().len(), 1);
    }

    #[tokio::test]
    async fn async_hook_runtime_bounds_concurrency_and_closes_on_shutdown() {
        let runtime = AsyncHookRuntime::default();
        let started = Arc::new(AtomicUsize::new(0));
        let cancelled = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(Semaphore::new(0));

        for _ in 0..=MAX_CONCURRENT_ASYNC_HOOKS {
            let started = Arc::clone(&started);
            let cancelled = Arc::clone(&cancelled);
            let gate = Arc::clone(&gate);
            runtime
                .schedule(
                    async move {
                        started.fetch_add(1, AtomicOrdering::SeqCst);
                        gate.acquire_owned().await.unwrap().forget();
                    },
                    move || {
                        cancelled.fetch_add(1, AtomicOrdering::SeqCst);
                    },
                )
                .unwrap();
        }

        wait_for_count(&started, MAX_CONCURRENT_ASYNC_HOOKS).await;
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(
            started.load(AtomicOrdering::SeqCst),
            MAX_CONCURRENT_ASYNC_HOOKS
        );

        gate.add_permits(1);
        wait_for_count(&started, MAX_CONCURRENT_ASYNC_HOOKS + 1).await;
        runtime.shutdown().await;
        wait_for_count(&cancelled, MAX_CONCURRENT_ASYNC_HOOKS).await;
        assert_eq!(
            cancelled.load(AtomicOrdering::SeqCst),
            MAX_CONCURRENT_ASYNC_HOOKS
        );

        let cancelled_after_shutdown = Arc::new(AtomicUsize::new(0));
        let cancellation = Arc::clone(&cancelled_after_shutdown);
        let scheduled_after_shutdown = runtime.schedule(async {}, move || {
            cancellation.fetch_add(1, AtomicOrdering::SeqCst);
        });
        assert!(scheduled_after_shutdown.is_err());
        assert_eq!(cancelled_after_shutdown.load(AtomicOrdering::SeqCst), 1);
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn shutdown_marks_an_outstanding_async_command_hook_failed() {
        let runner = CommandHookRunner::from_file(
            file("sleep 30".into(), None, true),
            Path::new("hooks.json"),
        )
        .unwrap();

        let decisions = runner
            .run(
                crate::PRE_TOOL_USE,
                &HookPayload {
                    session_id: "session-1".into(),
                    turn_id: Some("turn-1".into()),
                    cwd: std::env::current_dir()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    tool_name: Some("terminal".into()),
                    ..Default::default()
                },
            )
            .await;

        assert!(decisions.is_empty());
        assert_eq!(runner.recent_runs()[0].status, HookRunStatus::Running);
        runner.shutdown().await;

        let runs = runner.recent_runs();
        assert_eq!(runs[0].status, HookRunStatus::Failed);
        assert_eq!(runs[0].summary, "PreToolUse hook cancelled");
    }
}
