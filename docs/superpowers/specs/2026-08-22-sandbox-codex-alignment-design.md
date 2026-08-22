# Sandbox Codex Alignment Design

> 三平台沙箱架构与 Codex 对齐设计。

## 三平台对比

| 维度 | macOS Seatbelt | Linux Bubblewrap | Windows (v1) |
|------|---------------|-----------------|--------------|
| 隔离机制 | SBPL profile + sandbox-exec | 用户/PID/IPC/网络命名空间 | Job Object 进程限制 |
| 文件系统 | `(deny default)` + 参数化可写路径 | `--ro-bind / /` + `--bind` 可写 | v2: ACL deny/allow ACEs |
| 元数据保护 | regex `/.git(/\|$)` 等四目录 | `--ro-bind` 元数据目录 | v2: deny-write ACEs |
| 网络隔离 | 端口级 `(allow network-outbound (remote ip "localhost:PORT"))` | `--unshare-net` 命名空间隔离 | v2: WFP 过滤 |
| 代理集成 | Seatbelt 端口规则 | `--setenv` 代理环境变量 | `cmd.env()` 代理环境变量 |
| 探测 | `/usr/bin/sandbox-exec` 存在 | `which bwrap` | 始终可用（Win 10+） |

## macOS Seatbelt 规则

### 参数化路径

Codex 对齐改动：writable root 通过 `-DWRITABLE_ROOT_N=<path>` 参数传入 sandbox-exec，SBPL 中使用 `(subpath (param "WRITABLE_ROOT_N"))` 引用。这避免了：
- Shell 转义问题（路径含空格/特殊字符）
- 路径注入风险

### Regex 元数据保护

保护四个目录（`.git`、`.agents`、`.astro`、`.codex`），使用 regex 规则：

```sbpl
(deny file-write* (regex #"/.git(/|$)"))
(deny file-write* (regex #"/.agents(/|$)"))
(deny file-write* (regex #"/.astro(/|$)"))
(deny file-write* (regex #"/.codex(/|$)"))
```

regex 匹配任意嵌套深度，优于按 writable root 逐个生成 subpath deny。

### 网络规则（三分支）

1. **Managed network + allow_local_binding**: bind/inbound/outbound localhost + DNS port 53
2. **Managed network 无 local binding**: 仅精确代理端口
3. **Unmanaged + network_access**: `(allow network*)`
4. **无网络**: `(deny default)` 覆盖

## Linux Bubblewrap

### 挂载策略

```
--ro-bind / /          # 全局只读
--dev /dev             # 最小设备树
--tmpfs /tmp           # 临时文件
--bind <root> <root>   # 可写工作区（per writable root）
--ro-bind <root>/.git <root>/.git  # 元数据只读保护
```

### 命名空间隔离

```
--unshare-user         # 用户命名空间
--unshare-pid          # PID 命名空间
--unshare-ipc          # IPC 命名空间
--unshare-net          # 网络命名空间（无 allow_local_binding 时）
--new-session          # 新会话
--die-with-parent      # 父进程退出时终止
--cap-drop ALL         # 放弃所有 capability
```

### 代理集成

通过 `--setenv` 注入 `HTTP_PROXY`/`HTTPS_PROXY` 等环境变量，指向 `http://127.0.0.1:<port>`。

**v2 增强方向：** TCP↔UDS 双向桥接实现完整的网络命名空间代理路由（对齐 Codex 的 proxy_routing.rs）。

## Windows Job Object (v1)

### v2 已实现

- **Job Object**: `create_job_object()` + `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`，进程树容器
- **ACL**: `apply_workspace_acls()` 通过 `icacls /deny *S-1-1-0:(W)` 保护元数据目录
- **网络隔离**: 代理环境变量注入 + 离线标记（NPM/Cargo/Pip/Git SSH）
- `assign_process_to_job()` 将子进程绑定到 Job Object

### v3 已实现

- **Restricted Token**: `CreateRestrictedToken` + `WRITE_RESTRICTED | DISABLE_MAX_PRIVILEGE | LUA_TOKEN`
- **Capability SID**: `generate_capability_sid()` 生成 `S-1-5-21-{random}` 合成标识符

### v4 计划

- **WFP 网络过滤**: Windows Filtering Platform 端口级阻塞（ICMP/DNS/SMB）
- **私有桌面隔离**: `CreateDesktopW` 防窗口消息攻击

## 否认检测

| 平台 | 信号 |
|------|------|
| 全平台 | "operation not permitted"、"permission denied"、"read-only file system"、"sandbox"、"failed to write file" |
| Linux | "seccomp"、"landlock"、exit code 159 (128 + SIGSYS) |
| macOS | "sandbox" |
| Windows | "access is denied"（v2） |

## Managed Network Proxy 集成

```
ToolOrchestrator::run
  → managed_network_policy_for_call (gate)
  → StartedNetworkProxy::start (listener)
  → SandboxPolicy.with_managed_network (port → SBPL/bwrap/env)
  → SandboxRunner.tokio_command (platform dispatch)
  → 子进程仅可连接代理端口
  → BlockedRequest → SandboxErr::Denied.network_policy_decision
```

## 状态

| 批次 | 内容 | 状态 |
|------|------|------|
| A | macOS 参数化路径 + regex 保护 + .codex | ✓ 已实现 |
| B | Linux Bubblewrap 后端 | ✓ 已实现 |
| C | Windows Job Object + ACL v2 | ✓ 已实现（Job Object + ACL + 离线标记） |
| D | 设计文档 | ✓ 本文档 |
