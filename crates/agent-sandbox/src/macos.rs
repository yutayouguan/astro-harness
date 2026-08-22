//! macOS Seatbelt 沙箱后端。
//!
//! 通过 `/usr/bin/sandbox-exec -p <SBPL>` 执行子进程，SBPL profile 动态生成：
//! - 参数化可写路径 `-DWRITABLE_ROOT_N=<path>`
//! - Regex 元数据保护（`.git`/`.agents`/`.astro`/`.codex`）
//! - 端口级网络精确放行（managed network）

use std::path::Path;

use super::SandboxPolicy;
use types::SandboxMode;

pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

pub const PROTECTED_METADATA_DIRS: &[&str] = &[".git", ".agents", ".astro", ".codex"];

/// Check if sandbox-exec is available.
pub fn probe() -> bool {
    Path::new(SANDBOX_EXEC).is_file()
}

/// Build a tokio async command wrapping the program under the Seatbelt sandbox.
pub fn seatbelt_tokio_command(policy: &SandboxPolicy, program: &str) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(SANDBOX_EXEC);
    command.arg("-p").arg(seatbelt_profile(policy));
    for (i, root) in policy.writable_roots.iter().enumerate() {
        command.arg(format!("-DWRITABLE_ROOT_{i}={}", root.display()));
    }
    command.arg(program);
    command
}

/// Build a std sync command wrapping the program under the Seatbelt sandbox.
pub fn seatbelt_std_command(policy: &SandboxPolicy, program: &str) -> std::process::Command {
    let mut command = std::process::Command::new(SANDBOX_EXEC);
    command.arg("-p").arg(seatbelt_profile(policy));
    for (i, root) in policy.writable_roots.iter().enumerate() {
        command.arg(format!("-DWRITABLE_ROOT_{i}={}", root.display()));
    }
    command.arg(program);
    command
}

/// Generate the SBPL profile string for the given sandbox policy.
pub fn seatbelt_profile(policy: &SandboxPolicy) -> String {
    let mut profile = String::from(concat!(
        "(version 1)\n",
        "(deny default)\n",
        "(allow file-read*)\n",
        "(allow process*)\n",
        "(allow sysctl-read)\n",
        "(allow mach-lookup)\n",
        "(allow ipc-posix-shm)\n",
        "(allow signal)\n",
    ));

    if policy.mode == SandboxMode::WorkspaceWrite {
        for (i, root) in policy.writable_roots.iter().enumerate() {
            profile.push_str(&format!(
                "(allow file-write* (subpath (param \"WRITABLE_ROOT_{i}\")))\n"
            ));
            let _ = root;
        }
        for dir in PROTECTED_METADATA_DIRS {
            profile.push_str(&format!("(deny file-write* (regex #\"/{dir}(/|$)\"))\n"));
        }
    }

    if let Some(managed_network) = &policy.managed_network {
        if managed_network.allow_local_binding {
            profile.push_str("; allow local binding and loopback traffic\n");
            profile.push_str("(allow network-bind (local ip \"*:*\"))\n");
            profile.push_str("(allow network-inbound (local ip \"localhost:*\"))\n");
            profile.push_str("(allow network-outbound (remote ip \"localhost:*\"))\n");
            if !managed_network.loopback_ports.is_empty() {
                profile.push_str(
                    "; allow DNS lookups while application traffic remains proxy-routed\n",
                );
                profile.push_str("(allow network-outbound (remote ip \"*:53\"))\n");
            }
        }
        for port in &managed_network.loopback_ports {
            profile.push_str(&format!(
                "(allow network-outbound (remote ip \"localhost:{port}\"))\n"
            ));
        }
    } else if policy.network_access {
        profile.push_str("(allow network*)\n");
    }

    profile
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_contains_deny_default_and_all_metadata_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let policy =
            SandboxPolicy::new(SandboxMode::WorkspaceWrite, dir.path(), Vec::new(), false).unwrap();
        let profile = seatbelt_profile(&policy);

        assert!(profile.contains("(deny default)"));
        for dir_name in PROTECTED_METADATA_DIRS {
            assert!(
                profile.contains(&format!("(deny file-write* (regex #\"/{dir_name}(/|$)\"))")),
                "missing protection for {dir_name}"
            );
        }
        assert!(profile.contains("WRITABLE_ROOT_0"));
        assert!(!profile.contains("(allow network*)"));
    }

    #[test]
    fn unmanaged_network_access_emits_allow_network() {
        let dir = tempfile::tempdir().unwrap();
        let policy = SandboxPolicy::new(SandboxMode::WorkspaceWrite, dir.path(), [], true).unwrap();
        let profile = seatbelt_profile(&policy);
        assert!(profile.contains("(allow network*)"));
    }

    #[test]
    fn managed_network_allows_only_exact_proxy_port() {
        let dir = tempfile::tempdir().unwrap();
        let policy = SandboxPolicy::new(SandboxMode::WorkspaceWrite, dir.path(), [], false)
            .unwrap()
            .with_managed_network(network_proxy::ManagedNetworkSandboxContext {
                loopback_ports: vec![43_117],
                allow_local_binding: false,
            });
        let profile = seatbelt_profile(&policy);

        assert!(profile.contains("(allow network-outbound (remote ip \"localhost:43117\"))"));
        assert!(!profile.contains("(allow network*)"));
        assert!(!profile.contains("localhost:*"));
        assert!(!profile.contains("network-bind"));
    }

    #[test]
    fn local_binding_adds_loopback_rules_and_dns() {
        let dir = tempfile::tempdir().unwrap();
        let policy = SandboxPolicy::new(SandboxMode::WorkspaceWrite, dir.path(), [], false)
            .unwrap()
            .with_managed_network(network_proxy::ManagedNetworkSandboxContext {
                loopback_ports: vec![43_117],
                allow_local_binding: true,
            });
        let profile = seatbelt_profile(&policy);

        assert!(profile.contains("(allow network-bind (local ip \"*:*\"))"));
        assert!(profile.contains("(allow network-inbound (local ip \"localhost:*\"))"));
        assert!(profile.contains("(allow network-outbound (remote ip \"localhost:*\"))"));
        assert!(profile.contains("(allow network-outbound (remote ip \"*:53\"))"));
        assert!(!profile.contains("(allow network*)"));
    }

    #[test]
    fn read_only_mode_has_no_writable_root_params() {
        let dir = tempfile::tempdir().unwrap();
        let policy =
            SandboxPolicy::new(SandboxMode::ReadOnly, dir.path(), Vec::new(), false).unwrap();
        let profile = seatbelt_profile(&policy);

        assert!(profile.contains("(deny default)"));
        assert!(!profile.contains("WRITABLE_ROOT"));
        assert!(!profile.contains("file-write*"));
    }
}
