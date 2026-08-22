# sandbox

Agent 派生进程的 OS 级平台沙箱入口：基于三级权限模型生成平台原生沙箱策略，限制文件写入范围和网络访问，并提供 append-only 安全审计日志。

## 核心职责

- 封装 `SandboxPolicy`：三级权限模型（ReadOnly / WorkspaceWrite / DangerFullAccess）、可写根目录列表、网络访问策略
- 提供 `SandboxRunner`：探测平台后端可用性，包装 `tokio::process::Command` 和 `std::process::Command` 注入沙箱策略
- macOS Seatbelt SBPL 配置文件生成：参数化路径传递、regex 保护元数据（`.git`/`.agents`/`.astro`/`.codex`）
- Linux Bubblewrap 后端：用户/PID/IPC/网络命名空间隔离、只读根挂载、元数据只读保护
- Windows Job Object 后端（v1）：进程隔离、代理环境变量注入
- 沙箱拒绝检测：`is_likely_sandbox_denied()` 保守分类器，支持 Codex 重试升级决策
- 托管网络沙箱：`ManagedNetworkSandboxContext` 精确 loopback 端口放行与本地绑定策略
- 安全审计日志：append-only JSONL（`audit/sandbox.jsonl`），8MB 轮转归档，分页查询

## 模块结构

| 文件 | 职责 |
|------|------|
| `lib.rs` | 沙箱核心：`SandboxPolicy`、`SandboxRunner`（平台探测与命令包装）、macOS Seatbelt SBPL 生成、`is_likely_sandbox_denied()` 拒绝分类器 |
| `linux.rs` | Linux Bubblewrap 后端：`bwrap_command()` 挂载/命名空间组装、`probe_bwrap()` 探测、seccomp 信号检测 |
| `windows.rs` | Windows Job Object 后端（v1）：`windows_command()` 进程隔离、`WindowsAclSetup` 类型准备 |
| `audit.rs` | 安全审计层：append-only JSONL 写入、8MB 轮转归档、分页查询 |

## 核心类型与 API

- `SandboxPolicy` — 沙箱策略封装
  - `new(mode, workspace_root, extra_writable_roots, network_access)` — 构造策略，自动规范化与去重可写根
  - `unrestricted_file_system(execution_root, network_access)` — 全文件系统写入但保留网络策略独立性
  - `with_managed_network(context)` — 附加托管网络上下文（精确端口放行）
  - `profile_hash_material()` — 策略哈希材料（不含敏感路径），供审计使用
- `SandboxRunner` — 沙箱执行器
  - `probe()` — 探测平台后端可用性，返回 `SandboxHealth`
  - `tokio_command(policy, program)` — 异步沙箱化命令
  - `std_command(policy, program)` — 同步沙箱化命令
- `SandboxBackend` — 后端枚举：`MacosSeatbelt` / `LinuxBubblewrap` / `WindowsNative` / `Unrestricted`
- `SandboxHealthStatus` — 可用性：`Available` / `Unavailable`
- `ExecToolCallOutput` — 结构化进程输出：exit_code / stdout / stderr / aggregated_output
- `is_likely_sandbox_denied(mode, output)` — 拒绝分类器：仅沙箱激活 + 非零退出 + 已知拒绝信号时返回 true
- `SandboxError` / `SandboxErr` — 错误类型：`Denied` / `InvalidRoot` / `RootNotDirectory` / `BackendUnavailable`
- `SandboxAuditEvent` — JSONL 审计事件
- `SandboxAuditKind` — 审计类别：`Spawned` / `Denied` / `BackendUnavailable`
- `SandboxAuditMetadata` — 调用方上下文：audit_root / session_id / turn_id / tool_name / profile_id
  - `record(kind, policy, target, result, duration_ms)` — 记录审计事件
  - `record_prepare_error(policy, target, error, duration_ms)` — 记录准备阶段错误

## Crate 关系

| 方向 | crate | 说明 |
|------|-------|------|
| 依赖 | `types` | 共享 `SandboxMode` 枚举、`NetworkPolicyDecisionPayload` |
| 依赖 | `network-proxy` | 提供 `ManagedNetworkSandboxContext`（loopback 端口与本地绑定策略） |
| 被依赖 | `agent`（agent-core） | `exec::subagents` 构造 `SandboxPolicy` 用于子 Agent 进程隔离 |
| 被依赖 | `tools`（agent-tools） | Terminal 工具通过 `SandboxRunner` 包装命令执行 |

## 平台后端支持

| 平台 | 后端 | 状态 |
|------|------|------|
| macOS | Seatbelt（`sandbox-exec`） | 已实现，参数化路径 + regex 保护 |
| Linux | Bubblewrap（`bwrap`） | 已实现，命名空间隔离 + 只读挂载 |
| Windows | Job Object | v1 已实现（进程隔离），v2 计划 ACL + WFP |
| 其他 | Unrestricted | 不支持，fail-closed |

macOS Seatbelt SBPL 特性：
- `(deny default)` 默认拒绝 + `(allow file-read*)` 全局可读
- 参数化路径 `-DWRITABLE_ROOT_N=<path>` + `(subpath (param "WRITABLE_ROOT_N"))`
- Regex 元数据保护 `(deny file-write* (regex #"/.git(/|$)"))` 四目录
- 网络策略：端口级精确放行（managed）或 `(allow network*)`（unmanaged）

Linux Bubblewrap 特性：
- `--ro-bind / /` 全局只读 + `--bind` 可写工作区
- `--ro-bind .git .git` 元数据只读保护
- `--unshare-user/pid/ipc/net` 命名空间隔离
- `--setenv` 代理环境变量注入

## 关键不变量

1. **Fail closed**：受限模式下平台后端不可用时拒绝执行，不回退到无沙箱直跑
2. **元数据保护**：WorkspaceWrite 模式下 `.git`/`.agents`/`.astro`/`.codex` 目录始终只读
3. **DangerFullAccess 跳过沙箱**：该模式直接返回原生 Command，不经过平台沙箱
4. **拒绝分类保守**：exit code 2/126/127 排除在外（命令本身不存在或语法错误，非沙箱拒绝）
5. **托管网络精确放行**：`with_managed_network` 仅允许指定 loopback 端口，其余网络访问被拒绝
6. **审计日志不可变**：append-only JSONL，只追加不修改，8MB 自动轮转（最多 3 个归档）
7. **策略哈希稳定性**：`profile_hash_material` 不含文件路径，确保不同机器上相同策略产生相同哈希

## 测试

```bash
# 全部测试
cargo test -p sandbox

# macOS Seatbelt 实际隔离测试（需 macOS 环境）
cargo test -p sandbox seatbelt_enforces_workspace_write_boundary
cargo test -p sandbox seatbelt_keeps_workspace_metadata_read_only
cargo test -p sandbox -- --nocapture
```
