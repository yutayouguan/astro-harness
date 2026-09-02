//! Shared pseudo-terminal sessions used by both the Desktop dock and Agent tools.
//!
//! `portable-pty` owns the platform-specific PTY/ConPTY implementation. This module only
//! provides Astro-specific scope isolation, bounded replay, lifecycle and multi-consumer cursors.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

use anyhow::{anyhow, Context};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;

const OUTPUT_CAPACITY_BYTES: usize = 1024 * 1024;
const MAX_READ_BYTES: usize = 64 * 1024;
pub const MAX_TERMINALS_PER_SCOPE: usize = 8;

#[derive(Debug, Clone, Copy)]
pub struct TerminalDimensions {
    pub cols: u16,
    pub rows: u16,
}

struct SessionRegistration {
    desktop_token: Option<String>,
    make_agent_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalSessionInfo {
    pub id: u64,
    pub scope: String,
    pub cwd: String,
    pub running: bool,
    pub exit_code: Option<u32>,
    pub base_cursor: u64,
    pub end_cursor: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TerminalReadResult {
    pub id: u64,
    pub data: Vec<u8>,
    pub next_cursor: u64,
    pub dropped: bool,
    pub running: bool,
    pub exit_code: Option<u32>,
}

#[derive(Default)]
struct OutputState {
    bytes: VecDeque<u8>,
    base_cursor: u64,
    running: bool,
    exit_code: Option<u32>,
    agent_cursor: u64,
    child_exited: bool,
    reader_closed: bool,
}

struct TerminalSession {
    id: u64,
    scope: PathBuf,
    cwd: PathBuf,
    sandbox_mode: types::SandboxMode,
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    output: Mutex<OutputState>,
    agent_io: tokio::sync::Mutex<()>,
    notify: Notify,
}

impl TerminalSession {
    fn info(&self) -> TerminalSessionInfo {
        let output = self.output.lock().expect("terminal output lock poisoned");
        TerminalSessionInfo {
            id: self.id,
            scope: self.scope.to_string_lossy().into_owned(),
            cwd: self.cwd.to_string_lossy().into_owned(),
            running: output.running,
            exit_code: output.exit_code,
            base_cursor: output.base_cursor,
            end_cursor: output.base_cursor + output.bytes.len() as u64,
        }
    }

    fn append(&self, bytes: &[u8]) {
        let mut output = self.output.lock().expect("terminal output lock poisoned");
        output.bytes.extend(bytes.iter().copied());
        while output.bytes.len() > OUTPUT_CAPACITY_BYTES {
            output.bytes.pop_front();
            output.base_cursor = output.base_cursor.saturating_add(1);
        }
        if output.agent_cursor < output.base_cursor {
            output.agent_cursor = output.base_cursor;
        }
        drop(output);
        self.notify.notify_waiters();
    }

    fn child_finished(&self, exit_code: Option<u32>) {
        let mut output = self.output.lock().expect("terminal output lock poisoned");
        output.child_exited = true;
        output.exit_code = exit_code;
        output.running = !output.reader_closed;
        drop(output);
        self.notify.notify_waiters();
    }

    fn reader_finished(&self) {
        let mut output = self.output.lock().expect("terminal output lock poisoned");
        output.reader_closed = true;
        output.running = !output.child_exited;
        drop(output);
        self.notify.notify_waiters();
    }

    fn read_locked(
        &self,
        output: &OutputState,
        cursor: u64,
        max_bytes: usize,
    ) -> TerminalReadResult {
        let end_cursor = output.base_cursor + output.bytes.len() as u64;
        let effective_cursor = cursor.max(output.base_cursor).min(end_cursor);
        let start = effective_cursor
            .saturating_sub(output.base_cursor)
            .min(output.bytes.len() as u64) as usize;
        let count = output.bytes.len().saturating_sub(start).min(max_bytes);
        let data = output
            .bytes
            .iter()
            .skip(start)
            .take(count)
            .copied()
            .collect();
        TerminalReadResult {
            id: self.id,
            data,
            next_cursor: effective_cursor + count as u64,
            dropped: cursor < output.base_cursor,
            running: output.running,
            exit_code: output.exit_code,
        }
    }
}

#[derive(Default)]
struct RegistryState {
    sessions: HashMap<u64, Arc<TerminalSession>>,
    agent_by_scope: HashMap<PathBuf, u64>,
    desktop_by_token: HashMap<(PathBuf, String), u64>,
}

pub struct TerminalSessionManager {
    next_id: AtomicU64,
    lifecycle: Mutex<()>,
    registry: RwLock<RegistryState>,
}

impl Default for TerminalSessionManager {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            lifecycle: Mutex::new(()),
            registry: RwLock::new(RegistryState::default()),
        }
    }
}

impl TerminalSessionManager {
    pub fn ensure_shell(
        &self,
        scope: &Path,
        cwd: &Path,
        policy: &sandbox::SandboxPolicy,
        cols: u16,
        rows: u16,
    ) -> anyhow::Result<TerminalSessionInfo> {
        self.ensure_shell_with_options(scope, cwd, policy, cols, rows, false)
    }

    /// Opens the shared shell and optionally replaces it when the Desktop user changes the
    /// configured execution mode. Agent callers use [`Self::ensure_shell`] so they can never
    /// replace a more permissive user-owned session as a side effect of attaching.
    pub fn ensure_shell_with_options(
        &self,
        scope: &Path,
        cwd: &Path,
        policy: &sandbox::SandboxPolicy,
        cols: u16,
        rows: u16,
        replace_mode_mismatch: bool,
    ) -> anyhow::Result<TerminalSessionInfo> {
        // Process creation is synchronous and uncommon. Serializing it prevents two callers from
        // both observing an empty scope and leaking a second, unreachable shell.
        let _lifecycle = self
            .lifecycle
            .lock()
            .expect("terminal lifecycle lock poisoned");
        let scope = canonical_directory(scope, "terminal scope")?;
        let cwd = canonical_directory(cwd, "terminal cwd")?;
        if !cwd.starts_with(&scope) {
            anyhow::bail!("terminal cwd must stay inside its project scope");
        }

        // Drop the registry read guard before entering the branch. An `if let` scrutinee
        // temporary lives through the whole statement, so keeping the lookup inline would
        // self-deadlock below when a stale session has to be removed under the write lock.
        let existing_id = {
            self.registry
                .read()
                .expect("terminal registry lock poisoned")
                .agent_by_scope
                .get(&scope)
                .copied()
        };
        if let Some(existing_id) = existing_id {
            let existing = self.session(existing_id)?;
            let info = existing.info();
            if info.running {
                if !replace_mode_mismatch || existing.sandbox_mode == policy.mode {
                    ensure_mode_allows(policy.mode, existing.sandbox_mode)?;
                    return Ok(info);
                }
                existing
                    .killer
                    .lock()
                    .expect("terminal killer lock poisoned")
                    .kill()
                    .context("replace terminal after execution mode changed")?;
            }
            let mut registry = self
                .registry
                .write()
                .expect("terminal registry lock poisoned");
            remove_session_from_registry(&mut registry, existing_id);
        }

        let shell = default_shell();
        let shell_text = shell.to_string_lossy();
        let mut command = sandbox::SandboxRunner
            .std_command(policy, &shell_text)
            .context("prepare terminal sandbox")?;
        configure_interactive_shell(&mut command, &shell, policy.mode);
        command.current_dir(&cwd);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("ASTRO_TERMINAL", "1");
        self.spawn(
            scope,
            cwd,
            command,
            policy.mode,
            SessionRegistration {
                desktop_token: None,
                make_agent_default: true,
            },
            TerminalDimensions { cols, rows },
        )
    }

    /// Creates or reattaches one Desktop-owned terminal tab. A stable client token makes this
    /// idempotent across React remounts and prevents StrictMode from leaking duplicate shells.
    pub fn open_desktop_shell(
        &self,
        scope: &Path,
        cwd: &Path,
        policy: &sandbox::SandboxPolicy,
        client_token: &str,
        make_agent_default: bool,
        dimensions: TerminalDimensions,
    ) -> anyhow::Result<TerminalSessionInfo> {
        let client_token = client_token.trim();
        if client_token.is_empty() || client_token.len() > 128 {
            anyhow::bail!("terminal client token must contain 1 to 128 characters");
        }
        if make_agent_default && policy.mode != types::SandboxMode::WorkspaceWrite {
            anyhow::bail!("the AI default terminal must use the project sandbox");
        }

        let _lifecycle = self
            .lifecycle
            .lock()
            .expect("terminal lifecycle lock poisoned");
        let scope = canonical_directory(scope, "terminal scope")?;
        let cwd = canonical_directory(cwd, "terminal cwd")?;
        if !cwd.starts_with(&scope) {
            anyhow::bail!("terminal cwd must stay inside its project scope");
        }
        let key = (scope.clone(), client_token.to_owned());

        // Do not keep the read guard alive across this branch: reattaching the AI-default tab
        // updates `agent_by_scope` and therefore needs the registry write lock.
        let existing_id = {
            self.registry
                .read()
                .expect("terminal registry lock poisoned")
                .desktop_by_token
                .get(&key)
                .copied()
        };
        if let Some(existing_id) = existing_id {
            let existing = self.session(existing_id)?;
            if existing.info().running {
                if existing.sandbox_mode != policy.mode {
                    anyhow::bail!("terminal tab execution mode changed; close it before reopening");
                }
                if make_agent_default {
                    self.registry
                        .write()
                        .expect("terminal registry lock poisoned")
                        .agent_by_scope
                        .insert(scope, existing_id);
                }
                return Ok(existing.info());
            }
            remove_session_from_registry(
                &mut self
                    .registry
                    .write()
                    .expect("terminal registry lock poisoned"),
                existing_id,
            );
        }

        // The Agent may have created its implicit terminal before the Desktop dock was opened.
        // Adopt that process into the stable UI tab instead of spawning a hidden duplicate.
        if make_agent_default {
            let existing_agent_id = self
                .registry
                .read()
                .expect("terminal registry lock poisoned")
                .agent_by_scope
                .get(&scope)
                .copied();
            if let Some(existing_id) = existing_agent_id {
                let existing = self.session(existing_id)?;
                if existing.info().running && existing.sandbox_mode == policy.mode {
                    let mut registry = self
                        .registry
                        .write()
                        .expect("terminal registry lock poisoned");
                    registry
                        .desktop_by_token
                        .retain(|_, session_id| *session_id != existing_id);
                    registry.desktop_by_token.insert(key, existing_id);
                    return Ok(existing.info());
                }
            }
        }

        let active_count = self
            .registry
            .read()
            .expect("terminal registry lock poisoned")
            .sessions
            .values()
            .filter(|session| session.scope == scope && session.info().running)
            .count();
        if active_count >= MAX_TERMINALS_PER_SCOPE {
            anyhow::bail!("terminal limit reached for this project ({MAX_TERMINALS_PER_SCOPE})");
        }

        let shell = default_shell();
        let shell_text = shell.to_string_lossy();
        let mut command = sandbox::SandboxRunner
            .std_command(policy, &shell_text)
            .context("prepare terminal sandbox")?;
        configure_interactive_shell(&mut command, &shell, policy.mode);
        command.current_dir(&cwd);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("ASTRO_TERMINAL", "1");
        self.spawn(
            scope,
            cwd,
            command,
            policy.mode,
            SessionRegistration {
                desktop_token: Some(client_token.to_owned()),
                make_agent_default,
            },
            dimensions,
        )
    }

    pub fn active_for_scope(&self, scope: &Path) -> Option<TerminalSessionInfo> {
        let scope = scope.canonicalize().ok()?;
        let registry = self.registry.read().ok()?;
        let id = registry.agent_by_scope.get(&scope)?;
        registry
            .sessions
            .get(id)
            .filter(|session| session.info().running)
            .map(|session| session.info())
    }

    fn spawn(
        &self,
        scope: PathBuf,
        cwd: PathBuf,
        command: Command,
        sandbox_mode: types::SandboxMode,
        registration: SessionRegistration,
        dimensions: TerminalDimensions,
    ) -> anyhow::Result<TerminalSessionInfo> {
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: dimensions.rows.clamp(2, 500),
                cols: dimensions.cols.clamp(2, 500),
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("open pseudo terminal")?;
        let reader = pair.master.try_clone_reader().context("clone PTY reader")?;
        let writer = pair.master.take_writer().context("take PTY writer")?;
        let mut child = pair
            .slave
            .spawn_command(command_builder_from_std(&command))
            .context("spawn terminal shell")?;
        let killer = child.clone_killer();
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let session = Arc::new(TerminalSession {
            id,
            scope: scope.clone(),
            cwd,
            sandbox_mode,
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            killer: Mutex::new(killer),
            output: Mutex::new(OutputState {
                running: true,
                ..OutputState::default()
            }),
            agent_io: tokio::sync::Mutex::new(()),
            notify: Notify::new(),
        });

        let reader_session = Arc::clone(&session);
        if let Err(error) = std::thread::Builder::new()
            .name(format!("astro-terminal-read-{id}"))
            .spawn(move || read_loop(reader_session, reader))
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error).context("spawn terminal reader");
        }
        let wait_session = Arc::clone(&session);
        if let Err(error) = std::thread::Builder::new()
            .name(format!("astro-terminal-wait-{id}"))
            .spawn(move || {
                let exit_code = child.wait().ok().map(|status| status.exit_code());
                wait_session.child_finished(exit_code);
            })
        {
            let _ = session
                .killer
                .lock()
                .expect("terminal killer lock poisoned")
                .kill();
            return Err(error).context("spawn terminal waiter");
        }

        {
            let mut registry = self
                .registry
                .write()
                .expect("terminal registry lock poisoned");
            registry.sessions.insert(id, Arc::clone(&session));
            if let Some(token) = registration.desktop_token {
                registry.desktop_by_token.insert((scope.clone(), token), id);
            }
            if registration.make_agent_default {
                registry.agent_by_scope.insert(scope, id);
            }
        }

        Ok(session.info())
    }

    pub fn info(&self, id: u64) -> anyhow::Result<TerminalSessionInfo> {
        Ok(self.session(id)?.info())
    }

    pub fn ensure_scope(&self, id: u64, scope: &Path) -> anyhow::Result<()> {
        let requested = canonical_directory(scope, "terminal scope")?;
        let session = self.session(id)?;
        if session.scope != requested {
            anyhow::bail!("terminal session {id} belongs to a different project scope");
        }
        Ok(())
    }

    pub fn ensure_access(
        &self,
        id: u64,
        scope: &Path,
        policy: &sandbox::SandboxPolicy,
    ) -> anyhow::Result<()> {
        self.ensure_scope(id, scope)?;
        let session = self.session(id)?;
        ensure_mode_allows(policy.mode, session.sandbox_mode)
    }

    pub fn write(&self, id: u64, data: &[u8]) -> anyhow::Result<()> {
        let session = self.session(id)?;
        if !session.info().running {
            anyhow::bail!("terminal session {id} has exited");
        }
        let mut writer = session
            .writer
            .lock()
            .expect("terminal writer lock poisoned");
        writer.write_all(data).context("write terminal input")?;
        writer.flush().context("flush terminal input")?;
        Ok(())
    }

    pub fn resize(&self, id: u64, cols: u16, rows: u16) -> anyhow::Result<()> {
        let session = self.session(id)?;
        let result = session
            .master
            .lock()
            .expect("terminal master lock poisoned")
            .resize(PtySize {
                rows: rows.clamp(2, 500),
                cols: cols.clamp(2, 500),
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("resize terminal");
        result
    }

    pub fn kill(&self, id: u64) -> anyhow::Result<()> {
        let session = self.session(id)?;
        if !session.info().running {
            return Ok(());
        }
        let result = session
            .killer
            .lock()
            .expect("terminal killer lock poisoned")
            .kill()
            .context("kill terminal");
        result
    }

    /// Closes a Desktop tab and removes its routing metadata. Other sessions in the same project
    /// remain untouched.
    pub fn close(&self, id: u64) -> anyhow::Result<()> {
        let _lifecycle = self
            .lifecycle
            .lock()
            .expect("terminal lifecycle lock poisoned");
        let session = self
            .registry
            .read()
            .expect("terminal registry lock poisoned")
            .sessions
            .get(&id)
            .cloned();
        let Some(session) = session else {
            return Ok(());
        };
        if session.info().running {
            session
                .killer
                .lock()
                .expect("terminal killer lock poisoned")
                .kill()
                .context("close terminal")?;
        }
        remove_session_from_registry(
            &mut self
                .registry
                .write()
                .expect("terminal registry lock poisoned"),
            id,
        );
        Ok(())
    }

    pub async fn read(
        &self,
        id: u64,
        cursor: u64,
        max_bytes: usize,
        wait_ms: u64,
    ) -> anyhow::Result<TerminalReadResult> {
        let session = self.session(id)?;
        read_session(&session, cursor, max_bytes, wait_ms).await
    }

    /// Serialize Agent-side interactions so concurrent tool calls cannot overwrite the
    /// independent output cursor or interleave command writes.
    pub async fn interact_for_agent(
        &self,
        id: u64,
        input: Option<&[u8]>,
        reset_cursor: bool,
        max_bytes: usize,
        wait_ms: u64,
    ) -> anyhow::Result<TerminalReadResult> {
        let session = self.session(id)?;
        let _guard = session.agent_io.lock().await;
        let cursor = {
            let mut output = session
                .output
                .lock()
                .expect("terminal output lock poisoned");
            if reset_cursor {
                output.agent_cursor = output.base_cursor + output.bytes.len() as u64;
            }
            output.agent_cursor
        };
        let wrote_input = input.is_some_and(|input| !input.is_empty());
        if let Some(input) = input.filter(|input| !input.is_empty()) {
            self.write(id, input)?;
        }
        // Interactive shells echo input immediately. Waiting for the requested yield interval
        // before taking a snapshot keeps that echo from prematurely ending the tool call before
        // the command has produced useful output. Pure polls still long-poll until output arrives.
        if wrote_input && wait_ms > 0 {
            tokio::time::sleep(Duration::from_millis(wait_ms.min(30_000))).await;
        }
        let result = read_session(
            &session,
            cursor,
            max_bytes,
            if wrote_input { 0 } else { wait_ms },
        )
        .await?;
        session
            .output
            .lock()
            .expect("terminal output lock poisoned")
            .agent_cursor = result.next_cursor;
        Ok(result)
    }

    fn session(&self, id: u64) -> anyhow::Result<Arc<TerminalSession>> {
        self.registry
            .read()
            .expect("terminal registry lock poisoned")
            .sessions
            .get(&id)
            .cloned()
            .ok_or_else(|| anyhow!("unknown terminal session {id}"))
    }
}

fn remove_session_from_registry(registry: &mut RegistryState, id: u64) {
    registry.sessions.remove(&id);
    registry
        .agent_by_scope
        .retain(|_, session_id| *session_id != id);
    registry
        .desktop_by_token
        .retain(|_, session_id| *session_id != id);
}

async fn read_session(
    session: &Arc<TerminalSession>,
    cursor: u64,
    max_bytes: usize,
    wait_ms: u64,
) -> anyhow::Result<TerminalReadResult> {
    let max_bytes = max_bytes.clamp(1, MAX_READ_BYTES);
    let deadline = tokio::time::Instant::now() + Duration::from_millis(wait_ms.min(30_000));
    loop {
        let notified = session.notify.notified();
        let snapshot = {
            let output = session
                .output
                .lock()
                .expect("terminal output lock poisoned");
            session.read_locked(&output, cursor, max_bytes)
        };
        if !snapshot.data.is_empty() || !snapshot.running || wait_ms == 0 {
            return Ok(snapshot);
        }
        if tokio::time::timeout_at(deadline, notified).await.is_err() {
            return Ok(snapshot);
        }
    }
}

fn read_loop(session: Arc<TerminalSession>, mut reader: Box<dyn Read + Send>) {
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => session.append(&buffer[..count]),
        }
    }
    session.reader_finished();
}

fn command_builder_from_std(command: &Command) -> CommandBuilder {
    let mut builder = CommandBuilder::new(command.get_program());
    builder.args(command.get_args());
    if let Some(cwd) = command.get_current_dir() {
        builder.cwd(cwd);
    }
    for (key, value) in command.get_envs() {
        if let Some(value) = value {
            builder.env(key, value);
        } else {
            builder.env_remove(key);
        }
    }
    builder
}

fn canonical_directory(path: &Path, label: &str) -> anyhow::Result<PathBuf> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("resolve {label}: {}", path.display()))?;
    if !canonical.is_dir() {
        anyhow::bail!("{label} is not a directory: {}", canonical.display());
    }
    Ok(canonical)
}

fn ensure_mode_allows(
    requested: types::SandboxMode,
    session: types::SandboxMode,
) -> anyhow::Result<()> {
    let rank = |mode| match mode {
        types::SandboxMode::ReadOnly => 0,
        types::SandboxMode::WorkspaceWrite => 1,
        types::SandboxMode::DangerFullAccess => 2,
    };
    if rank(session) > rank(requested) {
        anyhow::bail!(
            "active system terminal is broader than this Agent permission profile ({requested:?}); switch the terminal to Project sandbox or use the danger-full-access profile"
        );
    }
    Ok(())
}

#[cfg(unix)]
fn default_shell() -> PathBuf {
    std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_file())
        .unwrap_or_else(|| PathBuf::from("/bin/sh"))
}

#[cfg(windows)]
fn default_shell() -> PathBuf {
    PathBuf::from("powershell.exe")
}

#[cfg(not(any(unix, windows)))]
fn default_shell() -> PathBuf {
    PathBuf::from("sh")
}

#[cfg(unix)]
fn configure_interactive_shell(
    command: &mut Command,
    shell: &Path,
    sandbox_mode: types::SandboxMode,
) {
    if sandbox_mode == types::SandboxMode::DangerFullAccess {
        command.arg("-l");
        return;
    }

    // Restricted terminals can read the user's shell configuration but cannot safely execute it:
    // Oh My Zsh, Powerlevel10k and history plugins write to HOME and the system temp directory
    // during startup. Use the shell's no-rc mode and a small explicit prompt instead of granting
    // those directories to the sandbox.
    let shell_name = shell
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    match shell_name {
        "zsh" => {
            command.arg("-f");
            command.env("PROMPT", "%F{cyan}AI%f %F{blue}%~%f %# ");
            command.env("RPROMPT", "");
        }
        "bash" => {
            command.args(["--noprofile", "--norc"]);
            command.env("PS1", "\\[\\e[36m\\]AI\\[\\e[0m\\] \\w \\$ ");
        }
        "fish" => {
            command.arg("--no-config");
        }
        _ => {}
    }
    command.env("ASTRO_TERMINAL_PROFILE", "isolated");
    command.env("HISTFILE", "/dev/null");
    command.env("ZDOTDIR", "/dev/null");
    command.env("BASH_ENV", "/dev/null");
    command.env("ENV", "/dev/null");
    if let Some(path) = isolated_shell_path() {
        command.env("PATH", path);
    }
}

#[cfg(unix)]
fn isolated_shell_path() -> Option<std::ffi::OsString> {
    let mut paths = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        paths.push(home.join(".cargo/bin"));
        paths.push(home.join(".local/bin"));
    }
    paths.extend([
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/opt/homebrew/sbin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/local/sbin"),
    ]);
    if let Some(current) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&current));
    }
    let mut seen = HashSet::new();
    paths.retain(|path| path.is_dir() && seen.insert(path.clone()));
    std::env::join_paths(paths).ok()
}

#[cfg(windows)]
fn configure_interactive_shell(
    command: &mut Command,
    _shell: &Path,
    _sandbox_mode: types::SandboxMode,
) {
    command.args(["-NoLogo", "-NoExit"]);
}

#[cfg(not(any(unix, windows)))]
fn configure_interactive_shell(
    _command: &mut Command,
    _shell: &Path,
    _sandbox_mode: types::SandboxMode,
) {
}

static SHARED_TERMINAL_SESSIONS: OnceLock<TerminalSessionManager> = OnceLock::new();

pub fn shared_terminal_sessions() -> &'static TerminalSessionManager {
    SHARED_TERMINAL_SESSIONS.get_or_init(TerminalSessionManager::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_output_reports_dropped_cursor() {
        let output = OutputState {
            bytes: VecDeque::from(b"world".to_vec()),
            base_cursor: 5,
            running: true,
            exit_code: None,
            agent_cursor: 5,
            child_exited: false,
            reader_closed: false,
        };
        let session = TerminalSession {
            id: 7,
            scope: PathBuf::from("/tmp"),
            cwd: PathBuf::from("/tmp"),
            sandbox_mode: types::SandboxMode::WorkspaceWrite,
            master: Mutex::new(
                native_pty_system()
                    .openpty(PtySize::default())
                    .unwrap()
                    .master,
            ),
            writer: Mutex::new(Box::new(Vec::<u8>::new())),
            killer: Mutex::new(Box::new(TestKiller)),
            output: Mutex::new(OutputState::default()),
            agent_io: tokio::sync::Mutex::new(()),
            notify: Notify::new(),
        };
        let result = session.read_locked(&output, 0, 64);
        assert_eq!(result.data, b"world");
        assert_eq!(result.next_cursor, 10);
        assert!(result.dropped);

        let ahead = session.read_locked(&output, 999, 64);
        assert!(ahead.data.is_empty());
        assert_eq!(ahead.next_cursor, 10);
    }

    #[derive(Debug)]
    struct TestKiller;

    impl ChildKiller for TestKiller {
        fn kill(&mut self) -> std::io::Result<()> {
            Ok(())
        }

        fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
            Box::new(Self)
        }
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn pty_output_can_be_replayed_from_an_independent_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let scope = dir.path().canonicalize().unwrap();
        let manager = TerminalSessionManager::default();
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "printf astro-pty"]);
        command.current_dir(&scope);
        let session = manager
            .spawn(
                scope.clone(),
                scope,
                command,
                types::SandboxMode::WorkspaceWrite,
                SessionRegistration {
                    desktop_token: None,
                    make_agent_default: true,
                },
                TerminalDimensions { cols: 80, rows: 24 },
            )
            .unwrap();

        let result = manager.read(session.id, 0, 1024, 3_000).await.unwrap();
        assert_eq!(String::from_utf8_lossy(&result.data), "astro-pty");
        assert_eq!(result.next_cursor, 9);
    }

    #[cfg(target_os = "macos")]
    #[tokio::test(flavor = "current_thread")]
    async fn sandboxed_zsh_can_initialize_pty_job_control() {
        let dir = tempfile::tempdir().unwrap();
        let scope = dir.path().canonicalize().unwrap();
        let policy = sandbox::SandboxPolicy::new(
            types::SandboxMode::WorkspaceWrite,
            &scope,
            Vec::new(),
            true,
        )
        .unwrap();
        let mut command = sandbox::SandboxRunner
            .std_command(&policy, "/bin/zsh")
            .unwrap();
        configure_interactive_shell(
            &mut command,
            Path::new("/bin/zsh"),
            types::SandboxMode::WorkspaceWrite,
        );
        command.current_dir(&scope);
        command.env("TERM", "xterm-256color");

        let manager = TerminalSessionManager::default();
        let session = manager
            .spawn(
                scope.clone(),
                scope,
                command,
                types::SandboxMode::WorkspaceWrite,
                SessionRegistration {
                    desktop_token: Some("sandboxed-zsh".into()),
                    make_agent_default: true,
                },
                TerminalDimensions { cols: 80, rows: 24 },
            )
            .unwrap();
        let mut cursor = 0;
        let mut bytes = Vec::new();
        for _ in 0..4 {
            let output = manager.read(session.id, cursor, 4096, 1_000).await.unwrap();
            cursor = output.next_cursor;
            bytes.extend_from_slice(&output.data);
            if bytes.windows(2).any(|window| window == b"AI") {
                break;
            }
        }
        let text = String::from_utf8_lossy(&bytes);

        assert!(text.contains("AI"), "terminal output: {text:?}");
        assert!(
            !text.contains("can't set tty pgrp"),
            "terminal output: {text:?}"
        );
        assert!(!text.contains("&#x20;"), "terminal output: {text:?}");
        manager.close(session.id).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_open_reuses_one_interactive_shell() {
        let dir = tempfile::tempdir().unwrap();
        let scope = dir.path().canonicalize().unwrap();
        let manager = Arc::new(TerminalSessionManager::default());
        let policy = Arc::new(sandbox::SandboxPolicy {
            mode: types::SandboxMode::DangerFullAccess,
            writable_roots: vec![scope.clone()],
            readable_roots: Vec::new(),
            network_access: true,
            managed_network: None,
        });
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let handles = (0..2)
            .map(|_| {
                let manager = Arc::clone(&manager);
                let policy = Arc::clone(&policy);
                let barrier = Arc::clone(&barrier);
                let scope = scope.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    manager
                        .ensure_shell(&scope, &scope, &policy, 80, 24)
                        .unwrap()
                })
            })
            .collect::<Vec<_>>();
        let sessions = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(sessions[0].id, sessions[1].id);

        let output = manager
            .interact_for_agent(
                sessions[0].id,
                Some(b"printf '__astro_terminal_ready__\\n'\n"),
                true,
                64 * 1024,
                750,
            )
            .await
            .unwrap();
        assert!(
            String::from_utf8_lossy(&output.data).contains("__astro_terminal_ready__"),
            "terminal output: {}",
            String::from_utf8_lossy(&output.data),
        );
        manager.kill(sessions[0].id).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn desktop_client_token_reopens_the_same_shell() {
        let dir = tempfile::tempdir().unwrap();
        let scope = dir.path().canonicalize().unwrap();
        let manager = TerminalSessionManager::default();
        let policy = sandbox::SandboxPolicy::new(
            types::SandboxMode::DangerFullAccess,
            &scope,
            Vec::new(),
            true,
        )
        .unwrap();

        let first = manager
            .open_desktop_shell(
                &scope,
                &scope,
                &policy,
                "stable-tab",
                false,
                TerminalDimensions { cols: 80, rows: 24 },
            )
            .unwrap();
        let reopened = manager
            .open_desktop_shell(
                &scope,
                &scope,
                &policy,
                "stable-tab",
                false,
                TerminalDimensions {
                    cols: 120,
                    rows: 32,
                },
            )
            .unwrap();
        assert_eq!(first.id, reopened.id);
        manager.close(first.id).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn desktop_tabs_are_independent_and_only_one_is_the_agent_default() {
        let dir = tempfile::tempdir().unwrap();
        let scope = dir.path().canonicalize().unwrap();
        let manager = TerminalSessionManager::default();

        let spawn_tab = |token: &str, agent_default: bool| {
            let mut command = Command::new("/bin/sh");
            command.arg("-c").arg("cat").current_dir(&scope);
            manager
                .spawn(
                    scope.clone(),
                    scope.clone(),
                    command,
                    types::SandboxMode::WorkspaceWrite,
                    SessionRegistration {
                        desktop_token: Some(token.to_owned()),
                        make_agent_default: agent_default,
                    },
                    TerminalDimensions { cols: 80, rows: 24 },
                )
                .unwrap()
        };

        let user = spawn_tab("user-tab", false);
        let agent = spawn_tab("agent-tab", true);
        assert_ne!(user.id, agent.id);
        assert_eq!(manager.active_for_scope(&scope).unwrap().id, agent.id);

        manager.close(user.id).unwrap();
        manager.close(user.id).unwrap();
        assert!(manager.info(user.id).is_err());
        assert!(manager.info(agent.id).is_ok());
        manager.close(agent.id).unwrap();
        assert!(manager.active_for_scope(&scope).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn desktop_ai_tab_adopts_an_existing_agent_shell() {
        let dir = tempfile::tempdir().unwrap();
        let scope = dir.path().canonicalize().unwrap();
        let manager = TerminalSessionManager::default();
        let policy = sandbox::SandboxPolicy::new(
            types::SandboxMode::WorkspaceWrite,
            &scope,
            Vec::new(),
            true,
        )
        .unwrap();

        let agent = manager
            .ensure_shell(&scope, &scope, &policy, 80, 24)
            .unwrap();
        let desktop = manager
            .open_desktop_shell(
                &scope,
                &scope,
                &policy,
                "ai-tab",
                true,
                TerminalDimensions {
                    cols: 120,
                    rows: 32,
                },
            )
            .unwrap();
        let reattached = manager
            .open_desktop_shell(
                &scope,
                &scope,
                &policy,
                "ai-tab",
                true,
                TerminalDimensions {
                    cols: 120,
                    rows: 32,
                },
            )
            .unwrap();

        assert_eq!(desktop.id, agent.id);
        assert_eq!(reattached.id, agent.id);
        manager.close(agent.id).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn desktop_can_open_system_then_project_terminal_and_restart_project_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let scope = dir.path().canonicalize().unwrap();
        let manager = TerminalSessionManager::default();
        let system_policy = sandbox::SandboxPolicy::new(
            types::SandboxMode::DangerFullAccess,
            &scope,
            Vec::new(),
            true,
        )
        .unwrap();
        let project_policy = crate::context::build_command_sandbox_policy_with_roots(
            dir.path(),
            &scope,
            std::slice::from_ref(&scope),
            None,
            false,
            None,
        )
        .unwrap();

        let system = manager
            .open_desktop_shell(
                &scope,
                &scope,
                &system_policy,
                "user-tab",
                false,
                TerminalDimensions {
                    cols: 120,
                    rows: 32,
                },
            )
            .unwrap();
        let project = manager
            .open_desktop_shell(
                &scope,
                &scope,
                &project_policy,
                "ai-tab",
                true,
                TerminalDimensions {
                    cols: 120,
                    rows: 32,
                },
            )
            .unwrap();
        assert_ne!(system.id, project.id);

        manager.close(project.id).unwrap();
        let restarted = manager
            .open_desktop_shell(
                &scope,
                &scope,
                &project_policy,
                "ai-tab",
                true,
                TerminalDimensions {
                    cols: 120,
                    rows: 32,
                },
            )
            .unwrap();
        assert_ne!(project.id, restarted.id);

        manager.close(system.id).unwrap();
        manager.close(restarted.id).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn restricted_zsh_uses_an_isolated_startup_profile() {
        let mut command = Command::new("/bin/zsh");
        configure_interactive_shell(
            &mut command,
            Path::new("/bin/zsh"),
            types::SandboxMode::WorkspaceWrite,
        );

        assert_eq!(command.get_args().collect::<Vec<_>>(), ["-f"]);
        let environment = command
            .get_envs()
            .map(|(key, value)| (key.to_string_lossy().into_owned(), value.map(Into::into)))
            .collect::<std::collections::HashMap<String, Option<std::ffi::OsString>>>();
        assert_eq!(
            environment.get("ASTRO_TERMINAL_PROFILE"),
            Some(&Some("isolated".into()))
        );
        assert_eq!(environment.get("HISTFILE"), Some(&Some("/dev/null".into())));
    }

    #[cfg(unix)]
    #[test]
    fn unrestricted_zsh_keeps_the_user_login_profile() {
        let mut command = Command::new("/bin/zsh");
        configure_interactive_shell(
            &mut command,
            Path::new("/bin/zsh"),
            types::SandboxMode::DangerFullAccess,
        );

        assert_eq!(command.get_args().collect::<Vec<_>>(), ["-l"]);
        assert!(command.get_envs().all(|(key, _)| key != "ZDOTDIR"));
    }

    #[test]
    fn agent_cannot_attach_to_a_more_permissive_terminal() {
        assert!(ensure_mode_allows(
            types::SandboxMode::ReadOnly,
            types::SandboxMode::WorkspaceWrite,
        )
        .is_err());
        assert!(ensure_mode_allows(
            types::SandboxMode::DangerFullAccess,
            types::SandboxMode::WorkspaceWrite,
        )
        .is_ok());
    }
}
