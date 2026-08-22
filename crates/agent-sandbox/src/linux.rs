//! Linux Bubblewrap 沙箱后端。
//!
//! 使用 `bwrap` 创建用户命名空间隔离的进程环境：只读根挂载 + 可写工作区 +
//! 元数据保护 + 可选网络命名空间隔离。

use std::path::Path;
use std::process::Command;

use super::SandboxPolicy;
use network_proxy::ManagedNetworkSandboxContext;
use types::SandboxMode;

const BWRAP_BINARY: &str = "bwrap";

const PROTECTED_METADATA_DIRS: &[&str] = &[".git", ".agents", ".astro", ".codex"];

/// Probe whether bubblewrap is available on the system.
pub fn probe_bwrap() -> Option<String> {
    Command::new("which")
        .arg(BWRAP_BINARY)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if path.is_empty() {
                None
            } else {
                Some(path)
            }
        })
}

/// Build a bwrap command wrapping the given program under the sandbox policy.
pub fn bwrap_command(policy: &SandboxPolicy, program: &str) -> Command {
    let bwrap_path = probe_bwrap().unwrap_or_else(|| BWRAP_BINARY.to_string());
    let mut cmd = Command::new(bwrap_path);

    // Global read-only root
    cmd.args(["--ro-bind", "/", "/"]);

    // Minimal device tree
    cmd.args(["--dev", "/dev"]);

    // Tmpfs for /tmp
    cmd.args(["--tmpfs", "/tmp"]);

    // Writable roots
    if policy.mode == SandboxMode::WorkspaceWrite {
        for root in &policy.writable_roots {
            let root_str = root.to_string_lossy();
            cmd.args(["--bind", &root_str, &root_str]);

            // Protect metadata directories by re-mounting read-only
            for protected in PROTECTED_METADATA_DIRS {
                let protected_path = root.join(protected);
                if protected_path.exists() {
                    let p = protected_path.to_string_lossy();
                    cmd.args(["--ro-bind", &p, &p]);
                }
            }
        }
    }

    // Process isolation
    cmd.args([
        "--unshare-user",
        "--unshare-pid",
        "--unshare-ipc",
        "--new-session",
        "--die-with-parent",
        "--cap-drop",
        "ALL",
    ]);

    // Network isolation
    if let Some(managed_network) = &policy.managed_network {
        if !managed_network.allow_local_binding {
            cmd.arg("--unshare-net");
        }
        // Inject proxy environment variables
        for port in &managed_network.loopback_ports {
            let proxy_url = format!("http://127.0.0.1:{port}");
            cmd.args(["--setenv", "HTTP_PROXY", &proxy_url]);
            cmd.args(["--setenv", "HTTPS_PROXY", &proxy_url]);
            cmd.args(["--setenv", "http_proxy", &proxy_url]);
            cmd.args(["--setenv", "https_proxy", &proxy_url]);
            cmd.args(["--setenv", "ALL_PROXY", &proxy_url]);
            cmd.args(["--setenv", "all_proxy", &proxy_url]);
            cmd.args(["--setenv", "ASTRO_NETWORK_PROXY_ACTIVE", "1"]);
        }
    } else if !policy.network_access {
        cmd.arg("--unshare-net");
    }

    // The actual command
    cmd.arg("--").arg(program);

    cmd
}

/// Build a tokio async command wrapping the given program under the sandbox policy.
pub fn bwrap_tokio_command(policy: &SandboxPolicy, program: &str) -> tokio::process::Command {
    let std_cmd = bwrap_command(policy, program);
    tokio::process::Command::from(std_cmd)
}

/// Check if a given exit code indicates a seccomp/signal-based denial.
pub fn is_seccomp_signal_exit(exit_code: i32) -> bool {
    // SIGSYS = 31 on Linux; exit code = 128 + signal number
    exit_code == 128 + 31
}

#[cfg(test)]
#[cfg(target_os = "linux")]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_policy(writable_roots: Vec<PathBuf>) -> SandboxPolicy {
        SandboxPolicy {
            mode: SandboxMode::WorkspaceWrite,
            writable_roots,
            network_access: false,
            managed_network: None,
        }
    }

    #[test]
    fn bwrap_command_includes_readonly_root_and_writable_bind() {
        let dir = tempfile::tempdir().unwrap();
        let policy = test_policy(vec![dir.path().to_path_buf()]);
        let cmd = bwrap_command(&policy, "echo");
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        assert!(args.windows(2).any(|w| w[0] == "--ro-bind" && w[1] == "/"));
        assert!(args.contains(&"--unshare-user".to_string()));
        assert!(args.contains(&"--unshare-pid".to_string()));
        assert!(args.contains(&"--die-with-parent".to_string()));
        assert!(args.contains(&"--unshare-net".to_string()));

        let root_str = dir.path().to_string_lossy().to_string();
        assert!(args
            .windows(3)
            .any(|w| w[0] == "--bind" && w[1] == root_str && w[2] == root_str));
    }

    #[test]
    fn bwrap_command_protects_existing_metadata_dirs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let policy = test_policy(vec![dir.path().to_path_buf()]);
        let cmd = bwrap_command(&policy, "echo");
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        let git_path = dir.path().join(".git").to_string_lossy().to_string();
        assert!(args
            .windows(3)
            .any(|w| w[0] == "--ro-bind" && w[1] == git_path));
    }

    #[test]
    fn bwrap_command_injects_proxy_env_with_managed_network() {
        let dir = tempfile::tempdir().unwrap();
        let mut policy = test_policy(vec![dir.path().to_path_buf()]);
        policy.managed_network = Some(ManagedNetworkSandboxContext {
            loopback_ports: vec![8080],
            allow_local_binding: false,
        });
        let cmd = bwrap_command(&policy, "echo");
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        assert!(args.windows(3).any(|w| w[0] == "--setenv"
            && w[1] == "HTTPS_PROXY"
            && w[2] == "http://127.0.0.1:8080"));
    }

    #[test]
    fn seccomp_signal_exit_code_detection() {
        assert!(is_seccomp_signal_exit(159)); // 128 + 31 (SIGSYS)
        assert!(!is_seccomp_signal_exit(1));
        assert!(!is_seccomp_signal_exit(0));
        assert!(!is_seccomp_signal_exit(127));
    }
}
