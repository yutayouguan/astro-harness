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

/// 探测系统上是否可用 bubblewrap。
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

/// 在沙箱策略下构建包裹指定程序的 bwrap 命令。
pub fn bwrap_command(policy: &SandboxPolicy, program: &str) -> Command {
    let bwrap_path = probe_bwrap().unwrap_or_else(|| BWRAP_BINARY.to_string());
    let mut cmd = Command::new(bwrap_path);

    // 全局只读根目录
    cmd.args(["--ro-bind", "/", "/"]);

    // 最小设备树
    cmd.args(["--dev", "/dev"]);

    // /tmp 使用 tmpfs
    cmd.args(["--tmpfs", "/tmp"]);

    // 可写根目录
    if policy.mode == SandboxMode::WorkspaceWrite {
        for root in &policy.writable_roots {
            let root_str = root.to_string_lossy();
            cmd.args(["--bind", &root_str, &root_str]);

            // 将元数据目录重新挂载为只读以保护它们
            for protected in PROTECTED_METADATA_DIRS {
                let protected_path = root.join(protected);
                if protected_path.exists() {
                    let p = protected_path.to_string_lossy();
                    cmd.args(["--ro-bind", &p, &p]);
                }
            }
        }
    }

    // 进程隔离
    cmd.args([
        "--unshare-user",
        "--unshare-pid",
        "--unshare-ipc",
        "--new-session",
        "--die-with-parent",
        "--cap-drop",
        "ALL",
    ]);

    // 网络隔离
    if let Some(managed_network) = &policy.managed_network {
        if !managed_network.allow_local_binding {
            cmd.arg("--unshare-net");
        }
        // 注入代理环境变量
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

    // 实际执行的命令
    cmd.arg("--").arg(program);

    cmd
}

/// 在沙箱策略下构建包裹指定程序的 tokio 异步命令。
pub fn bwrap_tokio_command(policy: &SandboxPolicy, program: &str) -> tokio::process::Command {
    let std_cmd = bwrap_command(policy, program);
    tokio::process::Command::from(std_cmd)
}

/// 检查给定的退出码是否表示 seccomp/信号导致的拒绝。
pub fn is_seccomp_signal_exit(exit_code: i32) -> bool {
    // Linux 上 SIGSYS = 31；退出码 = 128 + 信号编号
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
