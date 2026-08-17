//! Agent 派生进程的 OS 级沙箱入口。
//!
//! 受限模式必须由平台后端完整执行；后端不可用时 fail closed，禁止直跑。

use serde::Serialize;
use std::path::{Path, PathBuf};
use types::SandboxMode;

mod audit;

pub use audit::{
    append_sandbox_audit, clear_sandbox_audits, list_recent_sandbox_audits,
    list_sandbox_audits_before, sandbox_audit_archive_path, sandbox_audit_path,
    try_append_sandbox_audit, SandboxAuditEvent, SandboxAuditKind, SandboxAuditMetadata,
    MAX_SANDBOX_AUDIT_FILE_BYTES, SANDBOX_AUDIT_ARCHIVE_COUNT,
};

const MACOS_SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxPolicy {
    pub mode: SandboxMode,
    pub writable_roots: Vec<PathBuf>,
    pub network_access: bool,
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
            network_access: mode == SandboxMode::DangerFullAccess || network_access,
        })
    }

    pub fn profile_hash_material(&self) -> String {
        format!(
            "{:?}|{}|{}",
            self.mode,
            self.network_access,
            self.writable_roots
                .iter()
                .map(|path| path.to_string_lossy())
                .collect::<Vec<_>>()
                .join("|")
        )
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
pub enum SandboxError {
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

#[derive(Debug, Clone, Default)]
pub struct SandboxRunner;

impl SandboxRunner {
    pub fn probe(&self) -> SandboxHealth {
        #[cfg(target_os = "macos")]
        {
            let available = Path::new(MACOS_SANDBOX_EXEC).is_file();
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
                    format!("{MACOS_SANDBOX_EXEC} is missing")
                },
            }
        }
        #[cfg(target_os = "linux")]
        {
            SandboxHealth {
                backend: SandboxBackend::LinuxBubblewrap,
                status: SandboxHealthStatus::Unavailable,
                detail: "bubblewrap backend is not implemented yet".to_string(),
            }
        }
        #[cfg(target_os = "windows")]
        {
            SandboxHealth {
                backend: SandboxBackend::WindowsNative,
                status: SandboxHealthStatus::Unavailable,
                detail: "native Windows backend is not implemented yet".to_string(),
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
            let mut command = tokio::process::Command::new(MACOS_SANDBOX_EXEC);
            command.arg("-p").arg(macos_profile(policy)).arg(program);
            Ok(command)
        }
        #[cfg(not(target_os = "macos"))]
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
            let mut command = std::process::Command::new(MACOS_SANDBOX_EXEC);
            command.arg("-p").arg(macos_profile(policy)).arg(program);
            Ok(command)
        }
        #[cfg(not(target_os = "macos"))]
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

#[cfg(target_os = "macos")]
fn macos_profile(policy: &SandboxPolicy) -> String {
    let mut profile = String::from(
        "(version 1)\n(deny default)\n(allow file-read*)\n(allow process*)\n(allow sysctl-read)\n(allow mach-lookup)\n(allow ipc-posix-shm)\n(allow signal)\n",
    );
    if policy.mode == SandboxMode::WorkspaceWrite {
        for root in &policy.writable_roots {
            profile.push_str(&format!(
                "(allow file-write* (subpath \"{}\"))\n",
                seatbelt_escape(root)
            ));
            for protected in [".git", ".agents", ".codex"] {
                profile.push_str(&format!(
                    "(deny file-write* (subpath \"{}\"))\n",
                    seatbelt_escape(&root.join(protected))
                ));
            }
        }
    }
    if policy.network_access {
        profile.push_str("(allow network*)\n");
    }
    profile
}

#[cfg(target_os = "macos")]
fn seatbelt_escape(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_profile_protects_metadata_and_defaults_network_off() {
        let dir = tempfile::tempdir().unwrap();
        let policy =
            SandboxPolicy::new(SandboxMode::WorkspaceWrite, dir.path(), Vec::new(), false).unwrap();
        let profile = macos_profile(&policy);
        assert!(profile.contains("(deny default)"));
        assert!(profile.contains(".git"));
        assert!(!profile.contains("(allow network*)"));
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
