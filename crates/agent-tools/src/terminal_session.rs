//! Shared pseudo-terminal sessions used by both the Desktop dock and Agent tools.
//!
//! `portable-pty` owns the platform-specific PTY/ConPTY implementation. This module only
//! provides Astro-specific scope isolation, bounded replay, lifecycle and multi-consumer cursors.

use std::collections::{HashMap, VecDeque};
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
        let effective_cursor = cursor.max(output.base_cursor);
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
    active_by_scope: HashMap<PathBuf, u64>,
}

pub struct TerminalSessionManager {
    next_id: AtomicU64,
    registry: RwLock<RegistryState>,
}

impl Default for TerminalSessionManager {
    fn default() -> Self {
        Self {
            next_id: AtomicU64::new(1),
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
        let scope = canonical_directory(scope, "terminal scope")?;
        let cwd = canonical_directory(cwd, "terminal cwd")?;
        if !cwd.starts_with(&scope) {
            anyhow::bail!("terminal cwd must stay inside its project scope");
        }

        if let Some(existing_id) = self
            .registry
            .read()
            .expect("terminal registry lock poisoned")
            .active_by_scope
            .get(&scope)
            .copied()
        {
            let existing = self.session(existing_id)?;
            let info = existing.info();
            if info.running {
                ensure_mode_allows(policy.mode, existing.sandbox_mode)?;
                return Ok(info);
            }
            let mut registry = self
                .registry
                .write()
                .expect("terminal registry lock poisoned");
            registry.active_by_scope.remove(&scope);
            registry.sessions.remove(&existing_id);
        }

        let shell = default_shell();
        let shell_text = shell.to_string_lossy();
        let mut command = sandbox::SandboxRunner
            .std_command(policy, &shell_text)
            .context("prepare terminal sandbox")?;
        configure_interactive_shell(&mut command, &shell);
        command.current_dir(&cwd);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("ASTRO_TERMINAL", "1");
        self.spawn(scope, cwd, command, policy.mode, cols, rows)
    }

    pub fn active_for_scope(&self, scope: &Path) -> Option<TerminalSessionInfo> {
        let scope = scope.canonicalize().ok()?;
        let registry = self.registry.read().ok()?;
        let id = registry.active_by_scope.get(&scope)?;
        registry.sessions.get(id).map(|session| session.info())
    }

    pub fn spawn(
        &self,
        scope: PathBuf,
        cwd: PathBuf,
        command: Command,
        sandbox_mode: types::SandboxMode,
        cols: u16,
        rows: u16,
    ) -> anyhow::Result<TerminalSessionInfo> {
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: rows.clamp(2, 500),
                cols: cols.clamp(2, 500),
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

        {
            let mut registry = self
                .registry
                .write()
                .expect("terminal registry lock poisoned");
            registry.sessions.insert(id, Arc::clone(&session));
            registry.active_by_scope.insert(scope, id);
        }

        let reader_session = Arc::clone(&session);
        std::thread::Builder::new()
            .name(format!("astro-terminal-read-{id}"))
            .spawn(move || read_loop(reader_session, reader))
            .context("spawn terminal reader")?;
        let wait_session = Arc::clone(&session);
        std::thread::Builder::new()
            .name(format!("astro-terminal-wait-{id}"))
            .spawn(move || {
                let exit_code = child.wait().ok().map(|status| status.exit_code());
                wait_session.child_finished(exit_code);
            })
            .context("spawn terminal waiter")?;

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
            "active terminal sandbox ({session:?}) is broader than this Agent permission profile ({requested:?})"
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
fn configure_interactive_shell(command: &mut Command, _shell: &Path) {
    command.arg("-l");
}

#[cfg(windows)]
fn configure_interactive_shell(command: &mut Command, _shell: &Path) {
    command.args(["-NoLogo", "-NoExit"]);
}

#[cfg(not(any(unix, windows)))]
fn configure_interactive_shell(_command: &mut Command, _shell: &Path) {}

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
                80,
                24,
            )
            .unwrap();

        let result = manager.read(session.id, 0, 1024, 3_000).await.unwrap();
        assert_eq!(String::from_utf8_lossy(&result.data), "astro-pty");
        assert_eq!(result.next_cursor, 9);
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
