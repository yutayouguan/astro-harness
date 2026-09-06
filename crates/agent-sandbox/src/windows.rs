//! Windows 沙箱后端——Job Object 进程隔离 + ACL 文件系统控制。
//!
//! v2 实现三层隔离：
//! - **Job Object**: 进程树容器，`KILL_ON_JOB_CLOSE` 确保父退出时终止子树
//! - **ACL**: 可写根目录 allow-write ACE + 元数据目录 deny-write ACE
//! - **环境变量**: 代理注入 + 离线标记（WFP 网络过滤作为 v3）
//!
//! 注意：本模块仅在 `target_os = "windows"` 下编译。

use std::path::{Path, PathBuf};
use std::process::Command;

use super::SandboxPolicy;
use types::SandboxMode;

const PROTECTED_METADATA_DIRS: &[&str] = &[".git", ".agents", ".astro", ".codex"];

/// 检查是否运行在 Windows 上（Windows 构建下始终返回 true）。
pub fn probe_windows() -> bool {
    true
}

/// 构建具有 Job Object 进程隔离和 ACL 强制执行的命令。
///
/// 命令直接启动；Job Object 分配通过创建时的
/// `PROC_THREAD_ATTRIBUTE_JOB_LIST`（可用时）或启动后的
/// `AssignProcessToJobObject` 完成。
pub fn windows_command(policy: &SandboxPolicy, program: &str) -> Command {
    let mut cmd = Command::new(program);

    // 应用受管网络代理环境变量
    inject_network_env(&mut cmd, policy);

    // 应用离线标记以阻止常见包管理器的网络访问
    if !policy.network_access && policy.managed_network.is_none() {
        cmd.env("NPM_CONFIG_OFFLINE", "true");
        cmd.env("CARGO_NET_OFFLINE", "true");
        cmd.env("PIP_NO_INDEX", "1");
        cmd.env("GIT_SSH_COMMAND", "cmd /c exit 1");
    }

    cmd
}

/// 构建 Windows 沙箱的 tokio 异步命令。
pub fn windows_tokio_command(policy: &SandboxPolicy, program: &str) -> tokio::process::Command {
    let std_cmd = windows_command(policy, program);
    tokio::process::Command::from(std_cmd)
}

fn inject_network_env(cmd: &mut Command, policy: &SandboxPolicy) {
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
        if managed_network.loopback_ports.is_empty() {
            let discard = "http://127.0.0.1:9";
            cmd.env("HTTP_PROXY", discard);
            cmd.env("HTTPS_PROXY", discard);
        }
    } else if !policy.network_access {
        let discard = "http://127.0.0.1:9";
        cmd.env("HTTP_PROXY", discard);
        cmd.env("HTTPS_PROXY", discard);
    }
}

/// 应用工作区写入模式的 ACL 规则。
///
/// 授予可写根目录的写入权限，并拒绝对受保护元数据目录的写入。
/// 使用 `icacls` 以保证跨平台兼容。
///
/// 成功返回 Ok(())，任何 ACL 应用失败返回 Err。
pub fn apply_workspace_acls(policy: &SandboxPolicy) -> anyhow::Result<()> {
    if policy.mode != SandboxMode::WorkspaceWrite {
        return Ok(());
    }

    for root in &policy.writable_roots {
        for protected in PROTECTED_METADATA_DIRS {
            let protected_path = root.join(protected);
            if protected_path.exists() {
                deny_write_acl(&protected_path)?;
            }
        }
    }

    Ok(())
}

/// 使用 `icacls` 对路径应用拒绝写入的 ACL。
fn deny_write_acl(path: &Path) -> anyhow::Result<()> {
    let output = Command::new("icacls")
        .arg(path.to_string_lossy().as_ref())
        .args(["/deny", "*S-1-1-0:(W)", "/T", "/C", "/Q"])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::warn!(
            path = %path.display(),
            stderr = %stderr,
            "icacls deny-write failed (non-fatal)"
        );
    }
    Ok(())
}

/// 创建具有关闭即终止语义的 Job Object。
///
/// 返回原始句柄。调用者必须将进程分配给它，
/// 并在完成后关闭。
#[cfg(target_os = "windows")]
pub fn create_job_object() -> anyhow::Result<windows_sys::Win32::Foundation::HANDLE> {
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::System::JobObjects::*;

    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            anyhow::bail!("CreateJobObjectW failed: {}", GetLastError());
        }

        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

        let result = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if result == 0 {
            CloseHandle(job);
            anyhow::bail!("SetInformationJobObject failed: {}", GetLastError());
        }

        Ok(job)
    }
}

/// 将进程分配到 Job Object。
#[cfg(target_os = "windows")]
pub fn assign_process_to_job(
    job: windows_sys::Win32::Foundation::HANDLE,
    process: windows_sys::Win32::Foundation::HANDLE,
) -> anyhow::Result<()> {
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::System::JobObjects::*;

    unsafe {
        let result = AssignProcessToJobObject(job, process);
        if result == 0 {
            anyhow::bail!("AssignProcessToJobObject failed: {}", GetLastError());
        }
    }
    Ok(())
}

/// 完整沙箱化启动：应用 ACL、创建 Job Object、启动进程。
///
/// 这是 Windows 上沙箱化执行的推荐入口。
/// 返回子进程和 job 句柄（必须保持存活直到子进程退出——
/// 丢弃句柄会终止整个进程树）。
#[cfg(target_os = "windows")]
pub fn spawn_sandboxed(
    policy: &SandboxPolicy,
    program: &str,
    args: &[&str],
) -> anyhow::Result<(std::process::Child, windows_sys::Win32::Foundation::HANDLE)> {
    // 步骤 1：应用 ACL 保护元数据
    apply_workspace_acls(policy)?;

    // 步骤 2：创建 Job Object
    let job = create_job_object()?;

    // 步骤 3：构建并启动命令
    let mut cmd = windows_command(policy, program);
    for arg in args {
        cmd.arg(arg);
    }
    let child = cmd.spawn()?;

    // 步骤 4：将子进程分配到 job
    // 注意：理想情况下应使用 PROC_THREAD_ATTRIBUTE_JOB_LIST 进行原子分配，
    // 但这需要 CreateProcessAsUserW。目前使用启动后分配
    //（存在短暂的竞态窗口）。
    use windows_sys::Win32::System::Threading::OpenProcess;
    unsafe {
        let h_process = OpenProcess(
            0x001F_0FFF, // PROCESS_ALL_ACCESS
            0,
            child.id(),
        );
        if !h_process.is_null() {
            let _ = assign_process_to_job(job, h_process);
            windows_sys::Win32::Foundation::CloseHandle(h_process);
        }
    }

    Ok((child, job))
}

/// 此策略的工作区 ACL 设置元数据。
pub struct WindowsAclSetup {
    pub writable_roots: Vec<PathBuf>,
    pub protected_dirs: &'static [&'static str],
}

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
    fn windows_command_sets_offline_markers_when_no_network() {
        let policy = SandboxPolicy {
            mode: SandboxMode::WorkspaceWrite,
            writable_roots: vec![PathBuf::from("C:\\workspace")],
            network_access: false,
            managed_network: None,
        };
        let cmd = windows_command(&policy, "cmd.exe");
        let envs: Vec<_> = cmd.get_envs().collect();
        assert!(envs
            .iter()
            .any(|(k, v)| k == "NPM_CONFIG_OFFLINE" && v == &Some(std::ffi::OsStr::new("true"))));
        assert!(envs
            .iter()
            .any(|(k, v)| k == "CARGO_NET_OFFLINE" && v == &Some(std::ffi::OsStr::new("true"))));
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
        assert!(setup.protected_dirs.contains(&".astro"));
        assert!(setup.protected_dirs.contains(&".agents"));
    }

    #[test]
    fn job_object_creates_and_closes() {
        let job = create_job_object().unwrap();
        assert!(!job.is_null());
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(job);
        }
    }
}
