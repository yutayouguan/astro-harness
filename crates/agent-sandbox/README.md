# agent-sandbox (package: `sandbox`)

Agent 派生进程的 OS 级平台沙箱入口：基于三级权限模型（ReadOnly / WorkspaceWrite / DangerFullAccess）生成平台原生沙箱策略，限制文件写入范围和网络访问，并提供 append-only 安全审计日志。

## 核心职责

1. **平台沙箱策略生成** — `SandboxPolicy` 封装三级权限模型、可写根目录列表和网络访问策略；macOS 上生成 Seatbelt SBPL 配置文件，保护工作区元数据（`.git`/`.agents`/`.astro`）为只读。
2. **沙箱命令执行** — `SandboxRunner` 探测平台后端可用性（macOS Seatbelt / Linux Bubblewrap / Windows Native），并包装 `tokio::process::Command` 和 `std::process::Command` 注入沙箱策略。受限模式后端不可用时 fail closed，禁止直跑。
3. **沙箱拒绝检测** — `is_likely_sandbox_denied()` 保守分类器：仅在沙箱激活、非零退出码（排除 2/126/127）且输出包含已知拒绝信号（`operation not permitted` / `permission denied` / `read-only file system` 等）时判定为沙箱拒绝，支持 Codex 重试升级决策。
4. **托管网络沙箱** — `ManagedNetworkSandboxContext` 支持精确的 loopback 端口放行和本地绑定策略，与 `network-proxy` crate 配合实现 attempt 级网络隔离。
5. **安全审计日志** — append-only JSONL 审计日志（`audit/sandbox.jsonl`），支持 8MB 轮转归档（最多 3 个归档文件）、策略哈希（不含敏感路径）、分页查询和批量清理。

## 模块结构

| 文件 | 职责 |
|---|---|
| `lib.rs` | 沙箱核心：`SandboxPolicy`（策略封装）、`SandboxRunner`（平台探测与命令包装）、`SandboxBackend`/`SandboxHealth`/`SandboxHealthStatus`（后端状态）、`ExecToolCallOutput`（结构化进程输出）、`is_likely_sandbox_denied()`（拒绝分类器）、`SandboxError`（错误类型）、macOS Seatbelt SBPL 配置文件生成 |
| `audit.rs` | 安全审计层：`SandboxAuditEvent`（审计事件结构体）、`SandboxAuditKind`（Spawned/Denied/BackendUnavailable）、`SandboxAuditMetadata`（调用方上下文）、append-only JSONL 写入（全局写锁）、8MB 大小轮转归档、`list_recent_sandbox_audits()` / `list_sandbox_audits_before()` 分页查询、`clear_sandbox_audits()` 批量清理 |

## 核心类型与 API

```rust
// 沙箱策略
pub struct SandboxPolicy {
    pub mode: SandboxMode,                // ReadOnly / WorkspaceWrite / DangerFullAccess
    pub writable_roots: Vec<PathBuf>,     // WorkspaceWrite 模式下的可写根目录
    pub network_access: bool,             // 是否允许网络访问
    pub managed_network: Option<ManagedNetworkSandboxContext>,
}
impl SandboxPolicy {
    pub fn new(mode, workspace_root, extra_writable_roots, network_access) -> Result<Self, SandboxError>;
    pub fn unrestricted_file_system(execution_root, network_access) -> Result<Self, SandboxError>;
    pub fn with_managed_network(self, context: ManagedNetworkSandboxContext) -> Self;
    pub fn profile_hash_material(&self) -> String;
}

// 沙箱执行器
pub struct SandboxRunner;
impl SandboxRunner {
    pub fn probe(&self) -> SandboxHealth;
    pub fn tokio_command(&self, policy: &SandboxPolicy, program: &str) -> Result<Command, SandboxError>;
    pub fn std_command(&self, policy: &SandboxPolicy, program: &str) -> Result<Command, SandboxError>;
}

// 后端与健康
pub enum SandboxBackend { MacosSeatbelt, LinuxBubblewrap, WindowsNative, Unrestricted }
pub enum SandboxHealthStatus { Available, Unavailable }
pub struct SandboxHealth { pub backend, pub status, pub detail }

// 进程输出与拒绝检测
pub struct ExecToolCallOutput { pub exit_code, pub stdout, pub stderr, pub aggregated_output }
pub fn is_likely_sandbox_denied(sandbox_mode: SandboxMode, output: &ExecToolCallOutput) -> bool;

// 错误
pub enum SandboxErr {
    Denied { output, network_policy_decision },
    InvalidRoot { path, source },
    RootNotDirectory(PathBuf),
    BackendUnavailable(String),
}
pub type SandboxError = SandboxErr;

// 审计
pub struct SandboxAuditEvent;            // JSONL 审计事件（含 id/event/policy_hash/backend/target/result）
pub enum SandboxAuditKind { Spawned, Denied, BackendUnavailable }
pub struct SandboxAuditMetadata;         // 调用方上下文（audit_root/session_id/turn_id/tool_name/profile_id）
impl SandboxAuditMetadata {
    pub fn new(audit_root, session_id, turn_id, tool_name, profile_id) -> Self;
    pub fn record(&self, kind, policy, target, result, duration_ms);
    pub fn record_prepare_error(&self, policy, target, error, duration_ms);
}
pub fn append_sandbox_audit(base: &Path, event: &SandboxAuditEvent) -> Result<()>;
pub fn try_append_sandbox_audit(base: &Path, event: SandboxAuditEvent);
pub fn list_recent_sandbox_audits(base: &Path, limit: usize) -> Result<Vec<SandboxAuditEvent>>;
pub fn list_sandbox_audits_before(base, before, limit) -> Result<Vec<SandboxAuditEvent>>;
pub fn clear_sandbox_audits(base: &Path) -> Result<(usize, u64)>;
pub fn sandbox_audit_path(base: &Path) -> PathBuf;
pub fn sandbox_audit_archive_path(base: &Path, index: usize) -> PathBuf;
pub const MAX_SANDBOX_AUDIT_FILE_BYTES: u64;   // 8MB
pub const SANDBOX_AUDIT_ARCHIVE_COUNT: usize;  // 3
```

## 与其他 crate 的关系

- **`types`（agent-types）** — 共享 `SandboxMode` 枚举和 `NetworkPolicyDecisionPayload`
- **`network-proxy`（agent-network-proxy）** — 提供 `ManagedNetworkSandboxContext`（loopback 端口和本地绑定策略）
- **被 `agent`（agent-core）** — `exec::subagents` 中构造 `SandboxPolicy` 用于子 Agent 进程隔离
- **被 `tools`（agent-tools）** — Terminal 工具执行时通过 `SandboxRunner` 包装命令

## 测试运行命令

```bash
# 运行 agent-sandbox 全部测试
cargo test -p sandbox

# 运行单个测试（macOS Seatbelt 实际隔离测试需 macOS 环境）
cargo test -p sandbox seatbelt_enforces_workspace_write_boundary
cargo test -p sandbox -- --nocapture
```
