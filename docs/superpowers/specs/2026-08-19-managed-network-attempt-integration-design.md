# Attempt-scoped Managed Network Integration Design

**日期：** 2026-08-19

**状态：** 已批准方案 A，待实施计划

**范围：** 前台 `terminal` / `code_exec` 的受管 HTTPS CONNECT 网络、Seatbelt 收口与 typed denial

## 1. 背景与问题

Astro 已经完成两层基础：

1. `agent-network-proxy` 可以依 `NetworkProxyState` 执行 host allow/deny、本地地址防御、
   DNS rebinding 二次检查，并返回带 decision/source 的 CONNECT 403。
2. `SandboxErr::Denied` 可以携带 `NetworkPolicyDecisionPayload`，`ToolOrchestrator`
   也已将网络拒绝与文件系统拒绝分流。

但真实子进程还没有使用该代理：`SandboxRunner` 没有只放行代理 loopback port，
`terminal` / `code_exec` 没有注入受管 proxy env，代理拒绝也没有还原为 typed
sandbox error。因此当前 permission profile 中的 domain 规则仍未端到端生效。

## 2. 官方 Codex 对齐基线

官方 Codex 的相关边界为：

- `StartedNetworkProxy` 同时保有 `NetworkProxy` 与 `NetworkProxyHandle`，防止 listener
  脱离所有权。
- `PreparedManagedNetwork` 包含已重写的命令环境和 `ManagedNetworkSandboxContext`。
- `ManagedNetworkSandboxContext.loopback_ports` 是平台沙箱唯一可放行的代理端口集。
- `BlockedRequest` 由代理记录，不从 `curl` / `wget` 命令文本或 stderr 猜测 host。
- 文件系统 denial 与 managed-network denial 在 runtime 边界统一转成
  `SandboxErr::Denied`，再交给 orchestrator。

Astro 沿用这些名称与职责，但首批选择 attempt-scoped listener，不立即引入
Codex 完整的 session proxy、environment proxy 和并发 attribution controller。这使单次前台
tool call 的 blocked request 天然隔离，不会与其他并发命令串扰。

## 3. 目标

1. 仅当全局 `network_proxy.enabled=true` 且当前 permission profile 的
   `network.enabled=true` 时，为该 foreground attempt 启动代理。
2. 将受管 proxy env 注入 `terminal` 和经环境清洗后的 `code_exec`。
3. 平台沙箱只允许子进程连接当前代理的 loopback port，禁止忽略 env 直连。
4. 代理中的拒绝事件以 `BlockedRequest` 保留 host/port/protocol/reason/decision/source，
   工具 runtime 将其转为 `SandboxErr::Denied.network_policy_decision`。
5. 不改变未启用 network proxy 的现有工具行为，不将一次网络权限传播到后续调用。

代理只为 `terminal action=run` 和 `code_exec` 准备，其他 tool call 不会因当前
profile 开启了 network 而启动 listener。当前 profile schema 无法区分“未填 bool”与
“显式 false”，因此本批不猜测 network 深度继承：仅当选中 custom profile 自身的
`network.enabled=true` 时启用代理，未显式开启时 fail closed 为无命令网络。

## 4. 非目标

本批不实现：

- `terminal background=true` 的跨 runtime 代理租约；在受管网络开启时明确 fail closed。
- plain HTTP absolute-form forwarding、SOCKS5、SSH ProxyCommand 或 MITM。
- network approval HITL、session host cache、临时 allow 规则和自动重试。
- session-scoped proxy 热更新、多执行并发 attribution 或 blocked-request 持久化。
- permission profile 的 network 字段深度继承与 schema presence 迁移。
- Provider 控制面、进程内 `web_fetch` / `web_search` 或 MCP 常驻进程的网络改造。

## 5. 核心类型与所有权

### 5.1 `StartedNetworkProxy`

`agent-network-proxy` 新增 `StartedNetworkProxy`：

```rust
pub struct StartedNetworkProxy {
    proxy: NetworkProxy,
    _handle: NetworkProxyHandle,
}
```

`StartedNetworkProxy::start(...)` 完成 build + run；最后一个所有者被丢弃时 handle
abort listener 与活跃 tunnel。该对象以 `Arc` 在 orchestrator attempt 和 `ToolContext`
间传递，不持久化，不传给下一次 tool call。

### 5.2 `ManagedNetworkSandboxContext`

```rust
pub struct ManagedNetworkSandboxContext {
    pub loopback_ports: Vec<u16>,
    pub allow_local_binding: bool,
}
```

`loopback_ports` 仅包含已绑定的 HTTP CONNECT listener port。它是平台沙箱的事实输入，
不从环境变量字符串反向解析。

### 5.3 `PreparedManagedNetwork`

```rust
pub struct PreparedManagedNetwork {
    pub env: HashMap<String, String>,
    pub sandbox_context: ManagedNetworkSandboxContext,
}
```

`NetworkProxy::prepare(env)` 覆盖受管 proxy keys，但不更改其他命令环境。环境至少包括：

- `HTTP_PROXY` / `HTTPS_PROXY` / `http_proxy` / `https_proxy`
- `ALL_PROXY` / `all_proxy`
- `ASTRO_NETWORK_PROXY_ACTIVE=1`
- `NO_PROXY` / `no_proxy`

`NO_PROXY` 在 `allow_local_binding=false` 时为空，防止 localhost/private target 绕过代理；
只有 `allow_local_binding=true` 时才恢复明确的 loopback/private bypass 值。

### 5.4 `BlockedRequest`

```rust
pub struct BlockedRequest {
    pub host: String,
    pub port: u16,
    pub protocol: NetworkProtocol,
    pub reason: String,
    pub decision: NetworkPolicyDecision,
    pub source: NetworkDecisionSource,
    pub client_addr: Option<String>,
    pub method: Option<String>,
}
```

`NetworkProxyState` 保留有界 FIFO；每个 attempt 拥有独立 state，因此 foreground runtime
可在子进程结束后消费最新拒绝，无需从 stderr 推断请求目标。

## 6. 执行流程

```text
ToolOrchestrator::run
  -> resolve selected permission profile snapshot
  -> if network_proxy.enabled && profile.network.enabled:
       StartedNetworkProxy::start(NetworkProxyState(profile.network))
  -> SandboxAttempt { managed_network: Some(Arc<StartedNetworkProxy>) }
  -> sandbox_policy_for_attempt()
       attach ManagedNetworkSandboxContext
  -> ToolExecutionGrants -> ToolContext
  -> terminal/code_exec
       build base env
       NetworkProxy::prepare(base env)
       spawn through SandboxRunner
       wait for foreground process
       consume BlockedRequest
       if blocked: SandboxErr::Denied { network_policy_decision: Some(...) }
  -> ToolOrchestrator sees structured network denial
       finalize output without filesystem escalation
```

`StartedNetworkProxy` 必须在 sandbox policy 应用前完成 bind，因为 Seatbelt 只能放行已知的
精确 port。启动失败是执行准备错误，子进程不能在没有代理的情况下继续。

## 7. Sandbox 约束

`SandboxPolicy` 携带 attempt-scoped `ManagedNetworkSandboxContext`。macOS Seatbelt profile：

- 总是只允许 `(allow network-outbound (remote ip "localhost:<proxy-port>"))`。
- `allow_local_binding=false` 时不增加其他 network 规则。
- `allow_local_binding=true` 时增加本地 bind 和 loopback 通信所需的最小规则，仍不放开任意外网直连。
- 禁止用 `(allow network*)` 代替精确代理端口。

`DangerFullAccess` 不启动 managed proxy；它保持全访问语义。只有可在平台沙箱内执行的
custom profile 可开启受管 domain policy。

## 8. 工具边界

### 8.1 `terminal`

- foreground `action=run` 使用 `PreparedManagedNetwork.env`。
- 进程完成后先检查 blocked request，再执行普通 filesystem denial classifier。
- managed network 开启时，`background=true` 在 spawn 前返回明确错误；禁止不注入代理地退化执行。
- `list/status/wait/kill` 不启动新代理。

### 8.2 `code_exec`

`code_exec` 继续 `env_clear()` 和 secret scrub。正确顺序是：

1. 从父进程生成 scrubbed base env。
2. 用 `NetworkProxy::prepare` 添加受管 proxy keys。
3. 把结果注入子进程。

不得为了代理恢复任何 API key、token 或其他父进程敏感变量。

## 9. 错误语义

- proxy build/run/prepare 失败：普通 execution setup error，不 spawn child。
- host policy denial：`SandboxErr::Denied` + `NetworkPolicyDecisionPayload`。
- DNS 或 upstream dial 失败：仍是 502/普通网络错误，不伪造 policy denial。
- network denial 不进入 filesystem escalation，也不扩大为 unrestricted network retry。
- 本批未实现 approval flow；`Ask` / `Deny` payload 均 fail closed 并保留完整归因，
  下一批再将 `Ask` 转换为 HITL/auto-review 与仅当次 host grant。

## 10. 测试策略

### 10.1 `agent-network-proxy`

- `prepare` 覆盖父进程的旧 proxy 变量，且不改变无关 env。
- `allow_local_binding` 切换 `NO_PROXY` 语义。
- host policy 拒绝和 rebinding 拒绝都记录可转换的 `BlockedRequest`。
- `StartedNetworkProxy` drop 后 listener 关闭。

### 10.2 `agent-sandbox`

- profile 只包含指定 loopback proxy port，不包含任意 outbound allow。
- 端口变化会改变 policy hash，审计可区分实际 attempt 边界。
- macOS 集成测试验证直连公网/非代理 loopback port 不被放行。

### 10.3 `agent-tools` / `agent-core`

- proxy 未启用时，`ToolExecutionGrants` / `ToolContext` 行为不变。
- foreground terminal 能观察到覆盖后的 `HTTPS_PROXY`。
- `code_exec` 保留安全 env 并只新增受管 proxy keys。
- blocked CONNECT 返回 typed `SandboxErr::Denied` 及正确 host/port/decision/source。
- managed network + `background=true` 明确拒绝，且没有子进程被启动。
- orchestrator 不会对 network denial 发起 filesystem full-access retry。

### 10.4 分层验证

```bash
cargo test -p network-proxy -p sandbox -p tools -p agent -p types -p memory
cargo check --workspace --all-targets
cargo clippy -p network-proxy -p sandbox -p tools -p agent -p types -p memory \
  --all-targets --no-deps -- -D warnings
cargo fmt --all -- --check
git diff --check
```

## 11. 分批实施边界

1. 先扩展 proxy 的 prepare/blocked-request/started ownership 契约。
2. 再扩展 sandbox exact-port policy。
3. 通过 `SandboxAttempt -> ToolExecutionGrants -> ToolContext` 传递一次性 proxy lease。
4. 最后接入 terminal/code_exec typed denial，并完成独立复审。

每个子步都先写行为失败测试，只修正与当前子步相关的实现。实施提交仍保持单一、
可独立验证，不夹带工作区内其他未跟踪设计文档。
