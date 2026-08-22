//! Agent 派生进程的 OS 级沙箱入口。
//!
//! 受限模式必须由平台后端完整执行；后端不可用时 fail closed，禁止直跑。

use serde::Serialize;
use std::path::{Path, PathBuf};
use types::SandboxMode;

use network_proxy::ManagedNetworkSandboxContext;

mod audit;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod restricted_token;
#[cfg(target_os = "linux")]
pub mod seccomp;
#[cfg(target_os = "windows")]
pub mod windows;

pub use audit::{
    append_sandbox_audit, clear_sandbox_audits, list_recent_sandbox_audits,
    list_sandbox_audits_before, sandbox_audit_archive_path, sandbox_audit_path,
    try_append_sandbox_audit, SandboxAuditEvent, SandboxAuditKind, SandboxAuditMetadata,
    MAX_SANDBOX_AUDIT_FILE_BYTES, SANDBOX_AUDIT_ARCHIVE_COUNT,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxBackend {
    MacosSeatbelt,
    LinuxBubblewrap,
    WindowsNative,
    Unrestricted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxHealthStatus {
    Available,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SandboxHealth {
    pub backend: SandboxBackend,
    pub status: SandboxHealthStatus,
    pub detail: String,
}

/// Structured process result retained for sandbox-denial analysis and retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecToolCallOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub aggregated_output: String,
}

impl ExecToolCallOutput {
    pub fn new(exit_code: i32, stdout: impl Into<String>, stderr: impl Into<String>) -> Self {
        let mut output = Self {
            exit_code,
            stdout: stdout.into(),
            stderr: stderr.into(),
            aggregated_output: String::new(),
        };
        output.aggregated_output = output.render_text();
        output
    }

    pub fn render_text(&self) -> String {
        format!(
            "exit={}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.exit_code, self.stdout, self.stderr
        )
    }

    pub fn with_aggregated_output(mut self, output: impl Into<String>) -> Self {
        self.aggregated_output = output.into();
        self
    }
}

/// Conservative classifier matching Codex's retry gate: only a failed,
/// sandboxed attempt with a known denial signal is eligible for escalation.
pub fn is_likely_sandbox_denied(sandbox_mode: SandboxMode, output: &ExecToolCallOutput) -> bool {
    if sandbox_mode == SandboxMode::DangerFullAccess || output.exit_code == 0 {
        return false;
    }
    if [2, 126, 127].contains(&output.exit_code) {
        return false;
    }
    #[cfg(target_os = "linux")]
    if linux::is_seccomp_signal_exit(output.exit_code) {
        return true;
    }
    const DENIAL_SIGNALS: [&str; 7] = [
        "operation not permitted",
        "permission denied",
        "read-only file system",
        "seccomp",
        "sandbox",
        "landlock",
        "failed to write file",
    ];
    [&output.stderr, &output.stdout, &output.aggregated_output]
        .into_iter()
        .any(|section| {
            let lower = section.to_ascii_lowercase();
            DENIAL_SIGNALS.iter().any(|needle| lower.contains(needle))
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxPolicy {
    pub mode: SandboxMode,
    pub writable_roots: Vec<PathBuf>,
    /// Restricted read roots. When non-empty, only these paths are readable;
    /// when empty, the default `(allow file-read*)` grants full read access.
    pub readable_roots: Vec<PathBuf>,
    pub network_access: bool,
    pub managed_network: Option<ManagedNetworkSandboxContext>,
}

impl SandboxPolicy {
    pub fn new(
        mode: SandboxMode,
        workspace_root: impl AsRef<Path>,
        extra_writable_roots: impl IntoIterator<Item = PathBuf>,
        network_access: bool,
    ) -> Result<Self, SandboxError> {
        let mut writable_roots = Vec::new();
        if mode == SandboxMode::WorkspaceWrite {
            writable_roots.push(canonical_directory(workspace_root.as_ref())?);
            for root in extra_writable_roots {
                let root = canonical_directory(&root)?;
                if !writable_roots.contains(&root) {
                    writable_roots.push(root);
                }
            }
        }
        Ok(Self {
            mode,
            writable_roots,
            readable_roots: Vec::new(),
            network_access: mode == SandboxMode::DangerFullAccess || network_access,
            managed_network: None,
        })
    }

    /// Allow writes across the filesystem while keeping network policy independent.
    ///
    /// Unlike [`SandboxMode::DangerFullAccess`], this policy still runs through the
    /// platform sandbox, so `network_access = false` and protected workspace metadata
    /// remain enforceable.
    pub fn unrestricted_file_system(
        execution_root: impl AsRef<Path>,
        network_access: bool,
    ) -> Result<Self, SandboxError> {
        let execution_root = canonical_directory(execution_root.as_ref())?;
        let filesystem_root = execution_root
            .ancestors()
            .last()
            .expect("a canonical path always has an ancestor");
        Self::new(
            SandboxMode::WorkspaceWrite,
            &execution_root,
            [filesystem_root.to_path_buf()],
            network_access,
        )
    }

    /// Restrict file reads to only the specified roots.
    ///
    /// Platform-specific defaults (like `/usr`, `/bin`, `/dev`) are always readable.
    /// When `roots` is empty, this is a no-op (full read remains).
    pub fn with_restricted_read(mut self, roots: Vec<PathBuf>) -> Self {
        self.readable_roots = roots;
        self
    }

    pub fn with_managed_network(mut self, context: ManagedNetworkSandboxContext) -> Self {
        let mut loopback_ports = context
            .loopback_ports
            .into_iter()
            .filter(|port| *port != 0)
            .collect::<Vec<_>>();
        loopback_ports.sort_unstable();
        loopback_ports.dedup();
        self.network_access = false;
        self.managed_network = Some(ManagedNetworkSandboxContext {
            loopback_ports,
            allow_local_binding: context.allow_local_binding,
        });
        self
    }

    pub fn profile_hash_material(&self) -> String {
        let mut material = format!(
            "{:?}|{}|{}",
            self.mode,
            self.network_access,
            self.writable_roots
                .iter()
                .map(|path| path.to_string_lossy())
                .collect::<Vec<_>>()
                .join("|")
        );
        if let Some(managed_network) = &self.managed_network {
            material.push_str("|managed_network=");
            material.push_str(if managed_network.allow_local_binding {
                "local_binding"
            } else {
                "proxy_only"
            });
            material.push('|');
            material.push_str(
                &managed_network
                    .loopback_ports
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
        if !self.readable_roots.is_empty() {
            material.push_str("|readable=");
            material.push_str(
                &self
                    .readable_roots
                    .iter()
                    .map(|path| path.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("|"),
            );
        }
        material
    }
}

fn canonical_directory(path: &Path) -> Result<PathBuf, SandboxError> {
    let canonical = path
        .canonicalize()
        .map_err(|source| SandboxError::InvalidRoot {
            path: path.to_path_buf(),
            source,
        })?;
    if !canonical.is_dir() {
        return Err(SandboxError::RootNotDirectory(canonical));
    }
    Ok(canonical)
}

#[derive(Debug, thiserror::Error)]
pub enum SandboxErr {
    #[error(
        "sandbox denied exec error, exit code: {}, stdout: {}, stderr: {}",
        .output.exit_code,
        .output.stdout,
        .output.stderr
    )]
    Denied {
        output: Box<ExecToolCallOutput>,
        network_policy_decision: Option<types::NetworkPolicyDecisionPayload>,
    },
    #[error("sandbox root cannot be resolved: {path}: {source}")]
    InvalidRoot {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("sandbox root is not a directory: {0}")]
    RootNotDirectory(PathBuf),
    #[error("sandbox backend unavailable: {0}")]
    BackendUnavailable(String),
}

/// Compatibility alias for the pre-Codex-alignment public name.
pub type SandboxError = SandboxErr;

#[derive(Debug, Clone, Default)]
pub struct SandboxRunner;

impl SandboxRunner {
    pub fn probe(&self) -> SandboxHealth {
        #[cfg(target_os = "macos")]
        {
            let available = macos::probe();
            SandboxHealth {
                backend: SandboxBackend::MacosSeatbelt,
                status: if available {
                    SandboxHealthStatus::Available
                } else {
                    SandboxHealthStatus::Unavailable
                },
                detail: if available {
                    "macOS Seatbelt sandbox-exec is available".to_string()
                } else {
                    format!("{} is missing", macos::SANDBOX_EXEC)
                },
            }
        }
        #[cfg(target_os = "linux")]
        {
            let available = linux::probe_bwrap().is_some();
            SandboxHealth {
                backend: SandboxBackend::LinuxBubblewrap,
                status: if available {
                    SandboxHealthStatus::Available
                } else {
                    SandboxHealthStatus::Unavailable
                },
                detail: if available {
                    "bubblewrap (bwrap) is available".to_string()
                } else {
                    "bubblewrap (bwrap) not found in PATH".to_string()
                },
            }
        }
        #[cfg(target_os = "windows")]
        {
            SandboxHealth {
                backend: SandboxBackend::WindowsNative,
                status: if windows::probe_windows() {
                    SandboxHealthStatus::Available
                } else {
                    SandboxHealthStatus::Unavailable
                },
                detail: "Windows Job Object sandbox (v1: process isolation only)".to_string(),
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            SandboxHealth {
                backend: SandboxBackend::Unrestricted,
                status: SandboxHealthStatus::Unavailable,
                detail: "this operating system has no sandbox backend".to_string(),
            }
        }
    }

    pub fn tokio_command(
        &self,
        policy: &SandboxPolicy,
        program: &str,
    ) -> Result<tokio::process::Command, SandboxError> {
        if policy.mode == SandboxMode::DangerFullAccess {
            return Ok(tokio::process::Command::new(program));
        }
        self.ensure_available()?;
        #[cfg(target_os = "macos")]
        {
            Ok(macos::seatbelt_tokio_command(policy, program))
        }
        #[cfg(target_os = "linux")]
        {
            Ok(linux::bwrap_tokio_command(policy, program))
        }
        #[cfg(target_os = "windows")]
        {
            Ok(windows::windows_tokio_command(policy, program))
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            let _ = program;
            Err(SandboxError::BackendUnavailable(self.probe().detail))
        }
    }

    pub fn std_command(
        &self,
        policy: &SandboxPolicy,
        program: &str,
    ) -> Result<std::process::Command, SandboxError> {
        if policy.mode == SandboxMode::DangerFullAccess {
            return Ok(std::process::Command::new(program));
        }
        self.ensure_available()?;
        #[cfg(target_os = "macos")]
        {
            Ok(macos::seatbelt_std_command(policy, program))
        }
        #[cfg(target_os = "linux")]
        {
            Ok(linux::bwrap_command(policy, program))
        }
        #[cfg(target_os = "windows")]
        {
            Ok(windows::windows_command(policy, program))
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            let _ = program;
            Err(SandboxError::BackendUnavailable(self.probe().detail))
        }
    }

    fn ensure_available(&self) -> Result<(), SandboxError> {
        let health = self.probe();
        if health.status == SandboxHealthStatus::Available {
            Ok(())
        } else {
            Err(SandboxError::BackendUnavailable(health.detail))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn managed_network_context(
        ports: impl IntoIterator<Item = u16>,
        allow_local_binding: bool,
    ) -> network_proxy::ManagedNetworkSandboxContext {
        network_proxy::ManagedNetworkSandboxContext {
            loopback_ports: ports.into_iter().collect(),
            allow_local_binding,
        }
    }

    #[test]
    fn workspace_policy_canonicalizes_and_deduplicates_roots() {
        let dir = tempfile::tempdir().unwrap();
        let policy = SandboxPolicy::new(
            SandboxMode::WorkspaceWrite,
            dir.path(),
            [dir.path().to_path_buf()],
            false,
        )
        .unwrap();
        assert_eq!(policy.writable_roots.len(), 1);
        assert!(!policy.network_access);
    }

    #[test]
    fn full_access_does_not_require_platform_backend() {
        let dir = tempfile::tempdir().unwrap();
        let policy =
            SandboxPolicy::new(SandboxMode::DangerFullAccess, dir.path(), Vec::new(), false)
                .unwrap();
        assert!(SandboxRunner.tokio_command(&policy, "sh").is_ok());
    }

    #[test]
    fn unrestricted_filesystem_keeps_network_policy_independent() {
        let dir = tempfile::tempdir().unwrap();
        let policy = SandboxPolicy::unrestricted_file_system(dir.path(), false).unwrap();

        assert_eq!(policy.mode, SandboxMode::WorkspaceWrite);
        assert_eq!(policy.writable_roots[0], dir.path().canonicalize().unwrap());
        assert_eq!(
            policy.writable_roots.last().unwrap(),
            dir.path()
                .canonicalize()
                .unwrap()
                .ancestors()
                .last()
                .unwrap()
        );
        assert!(!policy.network_access);

        #[cfg(target_os = "macos")]
        {
            let profile = macos::seatbelt_profile(&policy);
            assert!(!profile.contains("(allow network*)"));
            assert!(profile.contains("(deny file-write* (regex #\"/.git(/|$)\"))"));
        }
    }

    #[test]
    fn managed_proxy_port_changes_policy_hash() {
        let root = tempfile::tempdir().unwrap();
        let first = SandboxPolicy::new(SandboxMode::ReadOnly, root.path(), [], false)
            .unwrap()
            .with_managed_network(managed_network_context([41_001], false));
        let second = SandboxPolicy::new(SandboxMode::ReadOnly, root.path(), [], false)
            .unwrap()
            .with_managed_network(managed_network_context([41_002], false));

        assert_ne!(
            first.profile_hash_material(),
            second.profile_hash_material()
        );
    }

    #[test]
    fn managed_network_context_normalizes_proxy_ports() {
        let root = tempfile::tempdir().unwrap();
        let policy = SandboxPolicy::new(SandboxMode::ReadOnly, root.path(), [], true)
            .unwrap()
            .with_managed_network(managed_network_context([43_117, 0, 43_116, 43_117], false));

        assert!(!policy.network_access);
        assert_eq!(
            policy.managed_network,
            Some(managed_network_context([43_116, 43_117], false))
        );
    }

    #[test]
    fn unmanaged_policy_keeps_legacy_hash_material() {
        let root = tempfile::tempdir().unwrap();
        let policy = SandboxPolicy::new(SandboxMode::ReadOnly, root.path(), [], false).unwrap();

        assert_eq!(policy.profile_hash_material(), "ReadOnly|false|");
    }

    #[test]
    fn denial_classifier_requires_active_sandbox_and_known_signal() {
        let denied = ExecToolCallOutput::new(1, "", "touch: Operation not permitted");
        assert!(is_likely_sandbox_denied(
            SandboxMode::WorkspaceWrite,
            &denied
        ));
        assert!(!is_likely_sandbox_denied(
            SandboxMode::DangerFullAccess,
            &denied
        ));

        let ordinary_failure = ExecToolCallOutput::new(1, "", "cargo test failed");
        assert!(!is_likely_sandbox_denied(
            SandboxMode::WorkspaceWrite,
            &ordinary_failure
        ));
    }

    #[test]
    fn denial_classifier_ignores_quick_command_rejections() {
        for exit_code in [2, 126, 127] {
            let output = ExecToolCallOutput::new(exit_code, "", "sandbox: permission denied");
            assert!(!is_likely_sandbox_denied(SandboxMode::ReadOnly, &output));
        }
    }

    #[test]
    fn sandbox_denied_preserves_structured_process_output() {
        let error = SandboxErr::Denied {
            output: Box::new(ExecToolCallOutput::new(
                1,
                "partial stdout",
                "Operation not permitted",
            )),
            network_policy_decision: None,
        };
        let SandboxErr::Denied {
            output,
            network_policy_decision,
        } = error
        else {
            panic!("expected denied error");
        };
        assert!(network_policy_decision.is_none());
        assert_eq!(output.exit_code, 1);
        assert_eq!(output.stdout, "partial stdout");
        assert_eq!(output.stderr, "Operation not permitted");
        assert_eq!(output.aggregated_output, output.render_text());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_profile_protects_metadata_and_defaults_network_off() {
        let dir = tempfile::tempdir().unwrap();
        let policy =
            SandboxPolicy::new(SandboxMode::WorkspaceWrite, dir.path(), Vec::new(), false).unwrap();
        let profile = macos::seatbelt_profile(&policy);
        assert!(profile.contains("(deny default)"));
        for dir_name in macos::PROTECTED_METADATA_DIRS {
            assert!(
                profile.contains(&format!("(deny file-write* (regex #\"/{dir_name}(/|$)\"))")),
                "missing protection for {dir_name}"
            );
        }
        assert!(profile.contains("WRITABLE_ROOT_0"));
        assert!(!profile.contains("(allow network*)"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn unmanaged_network_access_keeps_legacy_allow_rule() {
        let dir = tempfile::tempdir().unwrap();
        let policy = SandboxPolicy::new(SandboxMode::WorkspaceWrite, dir.path(), [], true).unwrap();
        let profile = macos::seatbelt_profile(&policy);

        assert!(policy.managed_network.is_none());
        assert!(profile.contains("(allow network*)"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn managed_network_profile_allows_only_exact_proxy_port() {
        let root = tempfile::tempdir().unwrap();
        let policy = SandboxPolicy::new(SandboxMode::WorkspaceWrite, root.path(), [], false)
            .unwrap()
            .with_managed_network(managed_network_context([43_117], false));
        let profile = macos::seatbelt_profile(&policy);

        assert!(profile.contains("(allow network-outbound (remote ip \"localhost:43117\"))"));
        assert!(!profile.contains("(allow network*)"));
        assert!(!profile.contains("localhost:*"));
        assert!(!profile.contains("network-bind"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn local_binding_adds_only_minimal_loopback_rules() {
        let root = tempfile::tempdir().unwrap();
        let policy = SandboxPolicy::new(SandboxMode::WorkspaceWrite, root.path(), [], false)
            .unwrap()
            .with_managed_network(managed_network_context([43_117], true));
        let profile = macos::seatbelt_profile(&policy);

        assert!(profile.contains("(allow network-bind (local ip \"*:*\"))"));
        assert!(profile.contains("(allow network-inbound (local ip \"localhost:*\"))"));
        assert!(profile.contains("(allow network-outbound (remote ip \"localhost:*\"))"));
        assert!(profile.contains("(allow network-outbound (remote ip \"*:53\"))"));
        assert!(!profile.contains("(allow network*)"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_rejects_non_proxy_loopback_port() {
        let allowed = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let denied = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let root = tempfile::tempdir().unwrap();
        let policy = SandboxPolicy::new(SandboxMode::ReadOnly, root.path(), [], false)
            .unwrap()
            .with_managed_network(managed_network_context(
                [allowed.local_addr().unwrap().port()],
                false,
            ));

        let mut allowed_command = SandboxRunner.std_command(&policy, "/usr/bin/nc").unwrap();
        allowed_command.args([
            "-z",
            "-w",
            "1",
            "127.0.0.1",
            &allowed.local_addr().unwrap().port().to_string(),
        ]);
        allowed_command
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let allowed_status = allowed_command.status().unwrap();
        assert!(allowed_status.success());

        let mut denied_command = SandboxRunner.std_command(&policy, "/usr/bin/nc").unwrap();
        denied_command.args([
            "-z",
            "-w",
            "1",
            "127.0.0.1",
            &denied.local_addr().unwrap().port().to_string(),
        ]);
        denied_command
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let denied_status = denied_command.status().unwrap();
        assert!(!denied_status.success());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_enforces_workspace_write_boundary() {
        let workspace = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let policy = SandboxPolicy::new(
            SandboxMode::WorkspaceWrite,
            workspace.path(),
            Vec::new(),
            false,
        )
        .unwrap();

        let allowed = workspace.path().join("allowed.txt");
        let status = SandboxRunner
            .std_command(&policy, "/usr/bin/touch")
            .unwrap()
            .arg(&allowed)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(allowed.exists());

        let denied = outside.path().join("denied.txt");
        let status = SandboxRunner
            .std_command(&policy, "/usr/bin/touch")
            .unwrap()
            .arg(&denied)
            .status()
            .unwrap();
        assert!(!status.success());
        assert!(!denied.exists());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_keeps_workspace_metadata_read_only() {
        let workspace = tempfile::tempdir().unwrap();
        let git = workspace.path().join(".git");
        std::fs::create_dir(&git).unwrap();
        let policy = SandboxPolicy::new(
            SandboxMode::WorkspaceWrite,
            workspace.path(),
            Vec::new(),
            false,
        )
        .unwrap();
        let target = git.join("config");
        let status = SandboxRunner
            .std_command(&policy, "/usr/bin/touch")
            .unwrap()
            .arg(&target)
            .status()
            .unwrap();
        assert!(!status.success());
        assert!(!target.exists());
    }
}
