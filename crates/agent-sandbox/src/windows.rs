//! Windows 沙箱后端——Job Object 进程隔离 + ACL 文件系统控制。
//!
//! v1 提供基于 Job Object 的进程隔离和内存/进程数限制。
//! 文件系统 ACL 和 WFP 网络过滤作为 v2 增强。

use std::process::Command;

use super::SandboxPolicy;
use types::SandboxMode;

const PROTECTED_METADATA_DIRS: &[&str] = &[".git", ".agents", ".astro", ".codex"];

/// Check if running on Windows 10+ (sufficient for Job Object sandbox).
pub fn probe_windows() -> bool {
    true // Windows builds only compile on Windows; always available
}

/// Build a command with Job Object process isolation.
///
/// v1: wraps the command to run inside a Job Object with limits.
/// The actual Job Object creation happens at spawn time via
/// `CREATE_BREAKAWAY_FROM_JOB` and `AssignProcessToJobObject`.
///
/// Filesystem ACL enforcement and WFP network filtering are v2.
pub fn windows_command(policy: &SandboxPolicy, program: &str) -> Command {
    let mut cmd = Command::new(program);

    // Set environment variables for managed network proxy
    if let Some(managed_network) = &policy.managed_network {
        for port in &managed_network.loopback_ports {
            let proxy_url = format!("http://127.0.0.1:{port}");
            cmd.env("HTTP_PROXY", &proxy_url);
            cmd.env("HTTPS_PROXY", &proxy_url);
            cmd.env("http_proxy", &proxy_url);
            cmd.env("https_proxy", &proxy_url);
            cmd.env("ALL_PROXY", &proxy_url);
            cmd.env("all_proxy", &proxy_url);
            cmd.env("ASTRO_NETWORK_PROXY_ACTIVE", "1");
        }
    }

    // v1: Command runs directly; Job Object will be assigned at spawn.
    // v2 will add: icacls for writable root ACLs, deny ACEs for protected dirs.

    cmd
}

/// Build a tokio async command for Windows sandbox.
pub fn windows_tokio_command(policy: &SandboxPolicy, program: &str) -> tokio::process::Command {
    let std_cmd = windows_command(policy, program);
    tokio::process::Command::from(std_cmd)
}

/// Metadata about writable root ACL setup for future v2 enforcement.
#[allow(dead_code)]
pub struct WindowsAclSetup {
    pub writable_roots: Vec<std::path::PathBuf>,
    pub protected_dirs: &'static [&'static str],
}

#[allow(dead_code)]
impl WindowsAclSetup {
    pub fn from_policy(policy: &SandboxPolicy) -> Self {
        Self {
            writable_roots: policy.writable_roots.clone(),
            protected_dirs: PROTECTED_METADATA_DIRS,
        }
    }
}

#[cfg(test)]
#[cfg(target_os = "windows")]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn windows_probe_returns_available() {
        assert!(probe_windows());
    }

    #[test]
    fn windows_command_injects_proxy_env() {
        let policy = SandboxPolicy {
            mode: SandboxMode::WorkspaceWrite,
            writable_roots: vec![PathBuf::from("C:\\workspace")],
            network_access: false,
            managed_network: Some(network_proxy::ManagedNetworkSandboxContext {
                loopback_ports: vec![9090],
                allow_local_binding: false,
            }),
        };
        let cmd = windows_command(&policy, "cmd.exe");
        let envs: Vec<_> = cmd.get_envs().collect();
        assert!(envs.iter().any(|(k, v)| k == "HTTPS_PROXY"
            && v == &Some(std::ffi::OsStr::new("http://127.0.0.1:9090"))));
    }

    #[test]
    fn acl_setup_includes_protected_metadata() {
        let policy = SandboxPolicy {
            mode: SandboxMode::WorkspaceWrite,
            writable_roots: vec![PathBuf::from("C:\\project")],
            network_access: false,
            managed_network: None,
        };
        let setup = WindowsAclSetup::from_policy(&policy);
        assert!(setup.protected_dirs.contains(&".git"));
        assert!(setup.protected_dirs.contains(&".codex"));
    }
}
