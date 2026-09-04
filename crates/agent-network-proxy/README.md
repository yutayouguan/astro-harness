# network-proxy

受管网络代理 -- 为 Agent 子进程提供基于策略的出站网络隔离与审批机制。

属于 [Astro Agent](../../README.md) workspace，详见根目录 `CLAUDE.md` 的 Crate Map。

## 核心职责

1. **HTTP CONNECT 代理** -- 在 `127.0.0.1` 本地回环地址启动 HTTP/1 CONNECT 隧道代理，拦截子进程的所有 HTTPS 出站连接，强制走策略评估链路。
2. **域名策略引擎** -- 基于 `NetworkPolicy`（allowlist/denylist glob 模式）对目标域名做 Allow/Deny/Ask 三态裁决；支持 `**. / *.` 通配与 IPv4/IPv6 归一化。
3. **DNS 重绑定防护** -- `connect_checked` 在 TCP 连接前校验 DNS 解析结果，拒绝公网域名解析到私有/回环 IP 的重绑定攻击。
4. **可插拔策略决策器** -- `NetworkPolicyDecider` trait 允许上层（如 HITL 审批 UI）异步介入未覆盖域名的放行/拦截决策。
5. **拦截记录收集** -- `NetworkProxyState` 维护最近 64 条被拦截请求（`BlockedRequest`），供 UI 展示网络审批卡片。
6. **Header requirement 保留** -- 从 active leaf profile 携带 host/method/path-prefix/header 规则，Debug 只显示 host 与 header 名。当前 CONNECT 隧道不能观察 TLS 内的 method/path，因此不会伪装成已执行注入；规则留给具备 HTTP 可见性的 transport。

## 模块结构

| 文件 | 职责 |
|---|---|
| `lib.rs` | Crate 入口，re-export 公开 API |
| `proxy.rs` | `NetworkProxy` / `NetworkProxyBuilder` / `NetworkProxyHandle` -- 代理的构建、启动、环境变量注入（`prepare`）、生命周期管理 |
| `runtime.rs` | `NetworkProxyState` -- 运行时状态核心：从 `NetworkPolicy` 构建 allow/deny GlobSet，执行域名策略评估，记录被拦截请求 |
| `policy.rs` | `Host` 归一化、`is_loopback_host` / `is_non_public_ip` 判定、域名 glob 编译（`compile_allowlist_globset` / `compile_denylist_globset`） |
| `network_policy.rs` | `NetworkPolicyDecider` trait 定义、`NetworkDecision` 枚举（Allow/Deny）、`NetworkPolicyRequest` 请求描述、`NetworkProtocol` 协议类型 |
| `connect_policy.rs` | `connect_checked` -- DNS 解析 + 私有 IP 重绑定校验 + 带超时的 TCP 连接 |
| `http_proxy.rs` | HTTP CONNECT 代理实现 -- accept 循环、并发限制（256）、请求头解析、策略拦截响应、双向流复制 |

## 核心类型与 API

### 结构体

- **`NetworkProxy`** -- 代理实例，持有监听地址与状态引用；通过 `builder()` 构建
- **`NetworkProxyBuilder`** -- Builder 模式：`state()` / `http_addr()` / `policy_decider()` / `build()`
- **`NetworkProxyHandle`** -- 运行句柄：`wait()` 阻塞等待、`shutdown()` 优雅关闭、Drop 时自动 abort
- **`StartedNetworkProxy`** -- 已启动的代理组合体，封装 proxy + handle
- **`PreparedManagedNetwork`** -- `prepare()` 返回值，含注入到子进程的环境变量（`HTTP_PROXY` / `HTTPS_PROXY` / `NO_PROXY` 等）
- **`ManagedNetworkSandboxContext`** -- 沙箱上下文：回环端口列表、是否允许本地绑定
- **`NetworkProxyState`** -- 运行时策略状态：GlobSet 匹配、域名阻断评估、被拦截请求队列
- **`BlockedRequest`** -- 被拦截请求记录：host/port/protocol/reason/decision/source
- **`NetworkPolicyRequest`** -- 策略评估请求描述
- **`NetworkHeaderInjection`** -- header 注入要求；值可序列化但在 Debug 输出中始终脱敏

### 枚举

- **`NetworkDecision`** -- `Allow` | `Deny { reason, source, decision }`
- **`NetworkProtocol`** -- `Http` / `HttpsConnect` / `Socks5Tcp` / `Socks5Udp`
- **`HostBlockDecision`** -- `Allowed` | `Blocked(HostBlockReason)`
- **`HostBlockReason`** -- `Denied` / `NotAllowed` / `NotAllowedLocal`

### Trait

- **`NetworkPolicyDecider`** -- `fn decide(&self, request) -> Future<Output = NetworkDecision>`；支持 `Arc<D>` 与闭包自动实现

### 关键函数

- `is_loopback_host(host: &Host) -> bool` -- 判断是否为回环地址
- `is_non_public_ip(ip: IpAddr) -> bool` -- 判断是否为非公网 IP（私有/回环/链路本地/多播等）
- `normalize_host(host: &str) -> String` -- 域名/IP 归一化（小写、去尾点、IPv6 去 scope）

## 与其他 crate 的关系

- **`types`**（`agent-types`）-- 依赖 `NetworkPolicy`、`NetworkAccess`、`NetworkPolicyDecision`、`NetworkDecisionSource` 等共享类型
- **`agent-core`**（`agent`）-- Agent 运行时在子进程执行前通过本 crate 创建受管代理，注入环境变量
- **`agent-sandbox`** -- 沙箱层使用 `ManagedNetworkSandboxContext` 配置子进程的网络隔离参数
- **`a2ui`** -- 网络审批 UI 卡片（`build_network_approval_surface`）使用 `BlockedRequest` 数据

## 测试运行命令

```bash
# 全部测试（单元 + 集成）
cargo test -p network-proxy

# 仅单元测试
cargo test -p network-proxy --lib

# 集成测试（tests/ 目录）
cargo test -p network-proxy --test http_connect
cargo test -p network-proxy --test managed_attempt
cargo test -p network-proxy --test network_decision
cargo test -p network-proxy --test network_policy
```
