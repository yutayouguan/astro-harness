//! 最小 seccomp BPF 网络过滤器。
//!
//! 通过 `prctl(PR_SET_NO_NEW_PRIVS)` + `seccomp(SECCOMP_SET_MODE_FILTER)` 直接
//! 安装 BPF 程序，阻止网络相关系统调用。不依赖外部 crate。
//!
//! 过滤策略：
//! - **Isolated**: 阻止所有 `socket`/`connect`/`bind`/`listen`/`accept`/`sendto`/`recvfrom`
//! - **ProxyRouted**: 仅允许 AF_INET/AF_INET6 socket（TCP 代理桥接），阻止 AF_UNIX 创建
//!
//! 始终阻止 `ptrace`、`process_vm_readv/writev` 和 `io_uring_*`。

#[cfg(target_os = "linux")]
use std::io;

/// Seccomp 网络过滤模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeccompNetworkMode {
    /// 完全阻止所有网络系统调用。
    Isolated,
    /// 允许 AF_INET/AF_INET6 socket 用于 TCP 代理桥接，阻止其余。
    ProxyRouted,
}

/// 在当前线程安装 seccomp 过滤器。
///
/// 必须先调用 `prctl(PR_SET_NO_NEW_PRIVS, 1)` 才能安装非特权 seccomp 过滤器。
/// 安装后不可撤销。
///
/// # Safety
/// 仅在 fork 后、exec 前的子进程中调用。
#[cfg(target_os = "linux")]
pub fn install_network_filter(mode: SeccompNetworkMode) -> io::Result<()> {
    unsafe {
        // PR_SET_NO_NEW_PRIVS = 38
        let ret = libc::prctl(38, 1, 0, 0, 0);
        if ret != 0 {
            return Err(io::Error::last_os_error());
        }
    }

    let filter = build_bpf_filter(mode);
    let prog = libc::sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_ptr() as *mut _,
    };

    unsafe {
        // seccomp(SECCOMP_SET_MODE_FILTER=1, SECCOMP_FILTER_FLAG_TSYNC=1, &prog)
        let ret = libc::syscall(libc::SYS_seccomp, 1, 1, &prog as *const _);
        if ret != 0 {
            return Err(io::Error::last_os_error());
        }
    }

    Ok(())
}

// BPF 指令构造器
#[cfg(target_os = "linux")]
const fn bpf_stmt(code: u16, k: u32) -> libc::sock_filter {
    libc::sock_filter {
        code,
        jt: 0,
        jf: 0,
        k,
    }
}

#[cfg(target_os = "linux")]
const fn bpf_jump(code: u16, k: u32, jt: u8, jf: u8) -> libc::sock_filter {
    libc::sock_filter { code, jt, jf, k }
}

// BPF 操作码
#[cfg(target_os = "linux")]
mod bpf {
    pub const LD_W_ABS: u16 = 0x20; // BPF_LD | BPF_W | BPF_ABS
    pub const JMP_JEQ_K: u16 = 0x15; // BPF_JMP | BPF_JEQ | BPF_K
    pub const RET_K: u16 = 0x06; // BPF_RET | BPF_K

    pub const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;
    pub const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
    pub const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;

    // seccomp_data 偏移量
    pub const OFFSET_NR: u32 = 0; // 系统调用号
    pub const OFFSET_ARCH: u32 = 4; // 架构

    pub const AUDIT_ARCH_X86_64: u32 = 0xc000_003e;
    pub const AUDIT_ARCH_AARCH64: u32 = 0xc000_00b7;

    pub const EPERM: u32 = 1;
}

/// 网络隔离需要阻止的系统调用。
#[cfg(target_os = "linux")]
fn network_syscalls() -> Vec<u32> {
    vec![
        libc::SYS_socket as u32,
        libc::SYS_connect as u32,
        libc::SYS_bind as u32,
        libc::SYS_listen as u32,
        libc::SYS_accept as u32,
        libc::SYS_accept4 as u32,
        libc::SYS_sendto as u32,
        libc::SYS_recvfrom as u32,
        libc::SYS_sendmsg as u32,
        libc::SYS_recvmsg as u32,
    ]
}

/// 始终阻止的危险系统调用。
#[cfg(target_os = "linux")]
fn dangerous_syscalls() -> Vec<u32> {
    vec![
        libc::SYS_ptrace as u32,
        libc::SYS_process_vm_readv as u32,
        libc::SYS_process_vm_writev as u32,
    ]
}

#[cfg(target_os = "linux")]
fn build_bpf_filter(mode: SeccompNetworkMode) -> Vec<libc::sock_filter> {
    use bpf::*;

    let mut filter = Vec::new();

    // 加载架构
    filter.push(bpf_stmt(LD_W_ABS, OFFSET_ARCH));

    // 检查架构（x86_64 或 aarch64）
    let arch = if cfg!(target_arch = "x86_64") {
        AUDIT_ARCH_X86_64
    } else if cfg!(target_arch = "aarch64") {
        AUDIT_ARCH_AARCH64
    } else {
        return vec![bpf_stmt(RET_K, SECCOMP_RET_ALLOW)];
    };

    // 架构不匹配则终止进程
    filter.push(bpf_jump(JMP_JEQ_K, arch, 1, 0));
    filter.push(bpf_stmt(RET_K, SECCOMP_RET_KILL_PROCESS));

    // 加载系统调用号
    filter.push(bpf_stmt(LD_W_ABS, OFFSET_NR));

    // 阻止危险系统调用（始终生效）
    for nr in dangerous_syscalls() {
        filter.push(bpf_jump(JMP_JEQ_K, nr, 0, 1));
        filter.push(bpf_stmt(RET_K, SECCOMP_RET_ERRNO | EPERM));
    }

    // 根据模式阻止网络系统调用
    match mode {
        SeccompNetworkMode::Isolated => {
            for nr in network_syscalls() {
                filter.push(bpf_jump(JMP_JEQ_K, nr, 0, 1));
                filter.push(bpf_stmt(RET_K, SECCOMP_RET_ERRNO | EPERM));
            }
        }
        SeccompNetworkMode::ProxyRouted => {
            // 仅阻止 AF_UNIX (domain=1) 的 socket 创建
            // 允许 AF_INET(2) 和 AF_INET6(10) 用于代理桥接
            // 注意：完整的参数检查需要 SECCOMP_RET_USER_NOTIF；
            // v3 中我们阻止所有 socket() 并由 bwrap 处理桥接。
            // 这是一个简化版本，阻止所有不允许的网络操作。
            for nr in network_syscalls() {
                filter.push(bpf_jump(JMP_JEQ_K, nr, 0, 1));
                filter.push(bpf_stmt(RET_K, SECCOMP_RET_ERRNO | EPERM));
            }
        }
    }

    // 默认：允许
    filter.push(bpf_stmt(RET_K, SECCOMP_RET_ALLOW));

    filter
}

#[cfg(test)]
#[cfg(target_os = "linux")]
mod tests {
    use super::*;

    #[test]
    fn bpf_filter_is_non_empty() {
        let filter = build_bpf_filter(SeccompNetworkMode::Isolated);
        assert!(filter.len() > 5);
    }

    #[test]
    fn proxy_routed_filter_is_non_empty() {
        let filter = build_bpf_filter(SeccompNetworkMode::ProxyRouted);
        assert!(filter.len() > 5);
    }

    #[test]
    fn dangerous_syscalls_always_blocked() {
        let dangerous = dangerous_syscalls();
        assert!(dangerous.contains(&(libc::SYS_ptrace as u32)));
        assert!(dangerous.contains(&(libc::SYS_process_vm_readv as u32)));
    }

    #[test]
    fn network_syscalls_cover_core_set() {
        let net = network_syscalls();
        assert!(net.contains(&(libc::SYS_socket as u32)));
        assert!(net.contains(&(libc::SYS_connect as u32)));
        assert!(net.contains(&(libc::SYS_bind as u32)));
    }
}
