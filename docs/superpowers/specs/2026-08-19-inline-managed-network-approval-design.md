# Inline Managed Network Approval Design

**日期：** 2026-08-19

**状态：** 设计完成，待分批实施

**范围：** 前台 `terminal` / `code_exec` 的 managed-network host 审批、session cache 与持久策略 amendment

## 1. 背景

Astro v2.28 已将前台进程工具接入 attempt-scoped managed HTTPS CONNECT proxy：

- 子进程只能连接当前 proxy 的精确 loopback port；
- allow/deny、本地地址防御和 DNS rebinding 在 proxy 边界执行；
- 拒绝会以 `NetworkPolicyDecisionPayload` 穿过 `SandboxErr::Denied`；
- `ToolOrchestrator` 不会把 network denial 错误升级为文件系统 full-access retry。

当前 allowlist miss 仍直接返回 403。下一阶段需把 proxy 已有的异步
`NetworkPolicyDecider` 接到 Astro 审批通道，并明确一次、会话与持久规则的作用域。

## 2. Codex 源码基线与关键纠正

本设计对照本地 Codex 源码 `/Users/iswm/CodeRope/codex/codex-rs`：

- `core/src/tools/network_approval.rs`
  - `NetworkApprovalService`、`HostApprovalKey`、`PendingHostApprovalKey`；
  - `NetworkApprovalMode::{Immediate, Deferred}`；
  - `PendingApprovalDecision::{AllowOnce, AllowForSession, Deny}`；
  - allowlist miss 通过 `handle_inline_policy_request` 异步等待审批。
- `core/src/network_policy_decision.rs`
  - `network_approval_context_from_payload`；
  - amendment host 必须与审批上下文匹配。
- `protocol/src/approvals.rs`
  - `NetworkApprovalProtocol`、`NetworkApprovalContext`；
  - `NetworkPolicyRuleAction`、`NetworkPolicyAmendment`。
- `core/src/session/mod.rs`
  - `Approved`、`ApprovedForSession` 与 `NetworkPolicyAmendment` 是不同审批结果；
  - 持久规则生效前会校验 normalized host 精确相等。
- `core/src/tools/orchestrator.rs`
  - `Immediate/Deferred` 表达进程审批归属何时收口，而不是“是否重跑命令”。

### 2.1 不重跑命令

Codex 的主路径不是“CONNECT 被拒绝 -> 子进程退出 -> 重跑整条命令”。
Proxy 在原始 socket 请求上等待审批；决议为 `Allow` 时，同一个 CONNECT 请求继续。

Astro 已有可 await 的 `NetworkPolicyDecider` trait，因此应直接对齐该语义。
不为 network approval 新增 command retry，避免重复文件写入、支付、发布等已发生副作用。

## 3. 设计目标

1. 只有 allowlist miss (`reason=not_allowed`) 可进入 host 审批。
2. 用户批准后放行原始 proxy request，不重跑 tool call。
3. 提供与 Codex 对齐的一次、会话和持久 host 规则。
4. 审批按 profile/environment + normalized host + protocol + port 精确归属。
5. 同一 call 的同 host 并发请求共用一个 pending approval，不产生重复弹窗。
6. 任何审批缺失、取消、超时或持久失败均 fail closed。
7. 保留 filesystem/network 正交性；network approval 不改变 sandbox mode 或 writable roots。

## 4. 非目标

本阶段不实现：

- plain HTTP forwarding、SOCKS5 或 MITM；当前只有 HTTPS CONNECT 真实执行路径。
- `terminal background=true` 的持久 proxy 租约和 `DeferredNetworkApproval`。
- Provider 控制面、进程内 `web_fetch` / `web_search` 或 MCP 常驻进程的网络改造。
- 任意 wildcard、CIDR、端口范围或 protocol 扩大审批。
- 将“完全访问”当作 headless 或使其绕过现有 permission profile。

## 5. 共享协议类型

`agent-types::network_policy` 增加 Codex 同名类型：

```rust
pub struct NetworkApprovalContext {
    pub host: String,
    pub protocol: NetworkApprovalProtocol,
}

pub enum NetworkPolicyRuleAction {
    Allow,
    Deny,
}

pub struct NetworkPolicyAmendment {
    pub host: String,
    pub action: NetworkPolicyRuleAction,
}
```

`NetworkApprovalContext` 只能从
`NetworkPolicyDecisionPayload::is_ask_from_decider()` 或代理内部的 allowlist miss 请求构造。
显式 deny、local/private defense 和 rebinding denial 不得伪造成可审批上下文。

Astro 当前没有 Codex 的统一 protocol `ReviewDecision`。本批不引入一个虚假的全局
`ReviewDecision` 仅为名字对齐；审批服务内部使用 Codex 同名
`PendingApprovalDecision::{AllowOnce, AllowForSession, Deny}`。等 Astro 统一命令、MCP、网络审批
wire contract 时，再整体迁入 `ReviewDecision`。

## 6. `NetworkApprovalService`

`SessionServices` 拥有一个 session-scoped `Arc<NetworkApprovalService>`，`AgentLoop`
通过 session services 访问它。服务负责审批状态，proxy 仍只负责强制执行。

```rust
struct HostApprovalKey {
    environment_id: String,
    host: String,
    protocol: &'static str,
    port: u16,
}

struct PendingHostApprovalKey {
    host: HostApprovalKey,
    turn_id: String,
    execution_id: Option<String>,
}

pub(crate) struct NetworkApprovalService {
    pending_host_approvals: Mutex<HashMap<PendingHostApprovalKey, Arc<PendingHostApproval>>>,
    session_policy_commit_lock: tokio::sync::Mutex<()>,
    session_approved_hosts: tokio::sync::Mutex<HashSet<HostApprovalKey>>,
    session_denied_hosts: tokio::sync::Mutex<HashSet<HostApprovalKey>>,
}
```

Astro 尚无多 environment 执行器，`environment_id` 首批使用已固定在
`StepContext` 中的 active permission profile id。这个字段保留 Codex 命名，后续接入
remote environment 时不需更改 key contract。

host 经 `normalize_host` 归一化，protocol 和 port 不可省略。
例如 `https://example.com:443` 的会话批准不能自动放行 HTTP:80 或 SOCKS5。

## 7. 代理与请求归属

Attempt-scoped listener 继续保留，但启动时附加：

- `Arc<dyn NetworkPolicyDecider>`；
- `environment_id`；
- `execution_id = tool_call_id`。

`http_proxy` 构造 `NetworkPolicyRequest` 时写入两个归属字段。虽然当前每个 proxy
只服务一个 attempt，仍保留显式 attribution，以便 audit、pending dedupe 与未来
session proxy 迁移。

`NetworkProxyState::evaluate_host_policy` 的顺序不改变：

1. explicit deny -> `Deny(BaselinePolicy)`；
2. local/private defense -> `Deny(BaselinePolicy)`；
3. allowlist hit -> `Allow`；
4. allowlist miss -> 调用 `NetworkPolicyDecider`；
5. DNS rebinding -> `Deny(ProxyState)`。

Decider 只能处理第 4 种情况。

## 8. 内联审批生命周期

```text
ToolOrchestrator::run
  -> capture profile/environment + turn + call attribution
  -> build NetworkApprovalSpec
  -> start attempt-scoped proxy with NetworkPolicyDecider
  -> spawn sandboxed child
  -> child sends CONNECT(host, port)
  -> NetworkProxyState detects allowlist miss
  -> NetworkApprovalService::handle_inline_policy_request
       -> validate attribution and active turn
       -> check session denied cache
       -> check session approved cache
       -> dedupe pending request by host + turn + execution
       -> apply approval policy/reviewer
       -> emit network-specific HITL or auto-review
       -> resolve AllowOnce / AllowForSession / Deny / amendment
  -> NetworkDecision::Allow
       -> the same CONNECT request dials upstream and continues
  -> or NetworkDecision::Deny
       -> proxy returns structured 403
       -> runtime converts blocked request to SandboxErr::Denied
       -> orchestrator finalizes without filesystem escalation
```

重要不变量：批准不创建第二个 `SandboxAttempt`，不重新运行 shell/code，
不退还 tool-round budget。

## 9. 审批结果与作用域

| UI 决议 | 内部决议 | 当前请求 | 后续请求 | 持久化 |
|---|---|---|---|---|
| 允许一次 | `AllowOnce` | 放行 | 再次审批 | 否 |
| 本会话允许 | `AllowForSession` | 放行 | 相同 key 自动放行 | 否 |
| 始终允许 | `NetworkPolicyAmendment(Allow)` | 规则生效后放行 | 新会话也放行 | 是 |
| 拒绝 | `Deny` | 403 | 不缓存普通拒绝 | 否 |
| 始终拒绝（后续 UI） | `NetworkPolicyAmendment(Deny)` | 403 | 新会话也拒绝 | 是 |

与 Codex 一致，普通的“拒绝”不默默写入 session deny cache；只有明确的持久
deny amendment 才改变后续策略。

`ApprovalsReviewer::AutoReview` 只能产生 `AllowOnce` 或 `Deny`。
Session 和 persistent 扩大必须由用户显式选择，不接受模型自动代替。

## 10. 持久 `NetworkPolicyAmendment`

Astro 暂无 Codex 独立 execpolicy network-rule store。首批的等价持久边界是当前
agent `config.yaml` 中选中的显式 custom permission profile：

```yaml
permissions:
  profiles:
    <environment_id>:
      network:
        domains:
          example.com: allow
```

持久约束：

1. amendment host 和 `NetworkApprovalContext.host` 经 `normalize_host` 后必须精确相等。
2. 只写 normalized exact host，不接受 wildcard、URL path、scheme 或嵌入端口。
3. 只能修改选中的显式 custom profile；内置 `:workspace` / `:read-only` /
   `:danger-full-access` 不可被就地改写。
4. 继承 profile 写入选中 leaf profile，不篡改 parent。
5. 在 `session_policy_commit_lock` 内完成重读、验证、原子保存与 cache 更新。
6. 持久失败时不将该决议降级为 allow-once；当前请求保持拒绝并告警。

这是 Astro 现有存储架构与 Codex execpolicy 之间的有意适配；对外的
`NetworkPolicyAmendment` 和安全语义保持一致。

## 11. HITL 界面与 hooks

网络审批使用专用 A2UI surface，不复用“Retry outside sandbox”文案。界面展示：

- command/tool 摘要；
- `protocol://host:port`；
- 当前 profile/environment；
- “允许一次”、“本会话允许”、“始终允许”、“拒绝”。

响应 payload 使用明确 scope，不从按钮文案反向推断：

```json
{ "approved": true, "scope": "once" }
{ "approved": true, "scope": "session" }
{ "approved": true, "scope": "persistent_rule" }
{ "approved": false }
```

`permission_request` / `post_permission_response` hook 沿用当前总线，但 payload 增加
`network_approval_context` 和 `choice`，且不暴露完整环境变量或凭证。

## 12. 取消、超时与并发

- pending approval owner 被 drop 时必须将同一 generation 解析为 `Deny`，不删除后来的同 host 请求。
- tool 取消、命令超时或 proxy lease drop 必须取消对应 HITL wait，不留孤儿弹窗。
- 审批超时不得长于当前 tool attempt 剩余 deadline。
- 同一 `PendingHostApprovalKey` 的并发 waiter 共用决议；不同 execution 不共用 allow-once。
- session cache 查询和持久提交按固定锁顺序执行，避免 allow/deny 交叉覆盖。

## 13. 安全不变量

1. explicit deny 永远先于 session allow 和 decider。
2. local/private defense 与 DNS rebinding 不可通过 host approval 覆盖。
3. 批准 exact host 不放行其子域、父域、另一 port 或 protocol。
4. 审批只改变 proxy decision，不改变 filesystem policy、process sandbox 或 secret scrub。
5. 无 active turn、无 reviewer、`ApprovalPolicy::Never`、校验失败均 deny。
6. 最终到达 runtime 的 `Ask` 仍是 403/fail closed，从不被解释为默认批准。
7. 第二次审批不能与 filesystem escalation 合并成“完全访问”。

## 14. 审计与错误语义

`PermissionAuditEvent` 记录 network request 的非敏感字段：

- profile/environment id；
- normalized host、protocol、port；
- `allow_once | allow_for_session | persistent_allow | deny | timeout | cancelled`；
- reviewer 和 duration；
- 当前 request 是否由 session cache 命中。

Proxy 或 persistence 错误不冒充为用户拒绝。它们记录为系统错误，对当前 socket
仍 fail closed。不记录 URL path、query、header、request body 或 credential。

## 15. 测试策略

### 15.1 `agent-types` / `agent-network-proxy`

- canonical type serde 与 HTTPS aliases。
- 只有 allowlist miss 会调用 decider。
- decider 等待后返回 `Allow`，同一 CONNECT 得到 200，不出现第二次进程执行。
- explicit deny、local defense 与 rebinding 均不调用 decider。
- request 保留 environment/execution attribution。

### 15.2 `agent-core`

- allow once、session allow、persistent allow 和 deny 的状态转移。
- 同 key 并发请求只产生一个 HITL。
- 不同 call 的 allow-once 不串扰。
- `ApprovalPolicy::Never`、reviewer unavailable、timeout、cancel 全部 deny。
- AutoReview 不能生成 session/persistent grant。
- network denial 仍不进入 filesystem retry。

### 15.3 `agent-memory` / UI

- amendment host normalized match 成功，mismatch/wildcard 拒绝。
- 只写 leaf custom profile，保留 YAML 其他键，原子更新。
- persistence 失败不更新 session allow cache。
- 四个审批按钮生成正确 structured scope payload。
- HITL 取消后 surface 不再可恢复旧请求。

### 15.4 分层验证

```bash
cargo test -p types -p network-proxy -p memory -p agent -p tools -p sandbox
cargo check --workspace --all-targets
cargo clippy -p types -p network-proxy -p memory -p agent -p tools -p sandbox \
  --all-targets --no-deps -- -D warnings
cd apps/desktop && npx tsc --noEmit
cd apps/desktop && npm run build
cargo fmt --all -- --check
git diff --check
```

## 16. 与 Codex 的有意差异

1. Codex 当前使用 session-managed proxy 并通过 `for_execution` 分配执行归属；
   Astro 保留 attempt-scoped listener，但仍显式传递 environment/execution id。
2. Codex 的持久 amendment 写入 execpolicy；Astro 首批写入当前 custom permission profile 的
   exact domain rule。
3. Codex 有 `DeferredNetworkApproval` 支持持久 unified-exec process；Astro 的 managed
   background process 尚未开放，因此本批只实现 immediate foreground lifecycle。

这些差异都不改变对用户可见的 host 审批语义和 fail-closed 安全边界。
