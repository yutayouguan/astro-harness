# Inline Managed Network Approval Implementation Plan

> 本计划依赖已完成的 attempt-scoped managed CONNECT proxy。每个 Task 都必须先写失败测试，单独验证并单独提交。

**Goal:** 将 managed proxy allowlist miss 接入 Codex 对齐的内联 host 审批，在不重跑命令的前提下支持 allow-once、allow-for-session 和 persistent exact-host amendment。

**Architecture:** `SessionServices` 拥有 session-scoped `NetworkApprovalService`；`ToolOrchestrator` 在启动 attempt-scoped proxy 时传入 call attribution 和异步 `NetworkPolicyDecider`。Allowlist miss 在原始 CONNECT 上 park，审批后返回 `NetworkDecision::Allow`，不创建第二个 sandbox attempt。Session cache 以 profile/host/protocol/port 为 key；persistent amendment 只能向 active leaf custom profile 写入 normalized exact host。

**Reference:** `docs/superpowers/specs/2026-08-19-inline-managed-network-approval-design.md`

---

### Task 1: 增加 canonical network approval contracts

**Files:**
- Modify: `crates/agent-types/src/network_policy.rs`
- Modify: `crates/agent-types/src/lib.rs`

- [ ] **Step 1: 先写 serde/contract 失败测试**

  锁定 `NetworkApprovalContext`、`NetworkPolicyRuleAction`、`NetworkPolicyAmendment` 的
  snake_case wire format，以及 `NetworkApprovalProtocol::Https` 对 `https_connect` /
  `http-connect` 的兼容。

- [ ] **Step 2: 实现最小共享类型并 re-export**

  名称、字段和 serde 语义与 Codex `protocol/src/approvals.rs` 对齐。
  不在 `types` crate 中加入 persistence 或审批业务逻辑。

- [ ] **Step 3: 验证并提交**

```bash
cargo fmt --all
cargo test -p types network_policy
cargo clippy -p types --all-targets --no-deps -- -D warnings
git diff --check
git add -- crates/agent-types/src/network_policy.rs crates/agent-types/src/lib.rs
git commit -m "feat: add network approval contracts"
```

### Task 2: 为 attempt proxy 注入 decider 与 execution attribution

**Files:**
- Modify: `crates/agent-network-proxy/src/proxy.rs`
- Modify: `crates/agent-network-proxy/src/http_proxy.rs`
- Modify: `crates/agent-network-proxy/src/runtime.rs`
- Modify: `crates/agent-network-proxy/src/lib.rs`
- Modify: `crates/agent-network-proxy/tests/managed_attempt.rs`
- Modify: `crates/agent-network-proxy/tests/http_connect.rs`

- [ ] **Step 1: 先写原请求内联放行测试**

  启动 decider 初始 park 的 proxy，发送一个 CONNECT，确认在决议前 socket
  未返回 403；释放 `Allow` 后同一 socket 继续到 upstream。
  测试同时断言 request 中的 `environment_id` 和 `execution_id`。

- [ ] **Step 2: 增加启动参数**

  为 `StartedNetworkProxy` 增加一个显式的 decider/attribution 启动入口，
  保留现有 `start(state)` 作为无 decider 兼容入口。

- [ ] **Step 3: 在 `NetworkPolicyRequest` 写入 attribution**

  HTTP proxy 不从 command string 推断归属，而是复制启动时固定的
  environment/execution metadata。

- [ ] **Step 4: 回归 hard denial 不可覆盖**

  测试 explicit deny、local defense 和 rebinding 均不调用 decider，
  `Ask` / `Deny` 仍以 structured 403 返回。

- [ ] **Step 5: 验证并提交**

```bash
cargo fmt --all
cargo test -p network-proxy
cargo clippy -p network-proxy --all-targets --no-deps -- -D warnings
git diff --check
git add -- crates/agent-network-proxy
git commit -m "feat: attribute managed network policy requests"
```

### Task 3: 实现 `NetworkApprovalService` 和 session cache

**Files:**
- Create: `crates/agent-core/src/control/network_approval.rs`
- Modify: `crates/agent-core/src/control/mod.rs`
- Modify: `crates/agent-core/src/runtime/session_services.rs`
- Modify: `crates/agent-core/src/runtime/mod.rs`

- [ ] **Step 1: 先写 service state-machine 失败测试**

  通过测试 reviewer 锁定：

  - allow-once 不进 session cache；
  - allow-for-session 只命中相同 profile/host/protocol/port；
  - deny cache 先于 allow cache；
  - 同 `PendingHostApprovalKey` 并发请求只触发一次 reviewer；
  - owner drop 将 waiter 全部 fail closed，且不删除新 generation。

- [ ] **Step 2: 实现 Codex 同名内部类型**

  实现 `HostApprovalKey`、`PendingHostApprovalKey`、`PendingHostApproval`、
  `PendingApprovalDecision` 和 `PendingHostApprovalOwner`。

- [ ] **Step 3: 实现 cache/commit 锁顺序**

  所有 allow/deny cache 修改均在 `session_policy_commit_lock` 下执行，
  不跨 await 持有 `std::sync::MutexGuard`。

- [ ] **Step 4: 将 service 绑定到 `SessionServices` lifecycle**

  不使用全局 singleton，不跨 session 复用 host cache；`AgentLoop` 只通过
  已有 `SessionServices` 边界访问该服务。

- [ ] **Step 5: 验证并提交**

```bash
cargo fmt --all
cargo test -p agent network_approval
cargo clippy -p agent --all-targets --no-deps -- -D warnings
git diff --check
git add -- crates/agent-core
git commit -m "feat: add session network approval service"
```

### Task 4: 接入 cancellation-aware network HITL

**Files:**
- Modify: `crates/agent-core/src/streaming/hitl_bridge.rs`
- Modify: `crates/agent-core/src/streaming/tools_exec.rs`
- Modify: `crates/agent-core/src/control/network_approval.rs`
- Modify: `crates/agent-a2ui/src/templates.rs` (or CodeGraph resolved template module)
- Modify: `apps/desktop/src/hooks/chat/useChatSession.ts`
- Modify: relevant A2UI tests

- [ ] **Step 1: 先写 scope payload 和 cancellation 失败测试**

  覆盖 once/session/persistent/deny 四种 payload，以及 tool cancel、command timeout、
  event publish failure 对 pending gate 的清理。

- [ ] **Step 2: 增加专用 network approval surface**

  显示 command 摘要、target 和 profile；使用结构化 `scope`，
  不复用 command allowlist 的 `always` bool 解析逻辑。

- [ ] **Step 3: 实现 cancellation-aware park**

  先 `begin_wait`、再 emit；event publish 失败立即 `abort_wait`。
  等待同时监听 attempt cancellation/deadline，任意一路结束都只清理自己的 interrupt。

- [ ] **Step 4: 将 approval policy/reviewer 映射为受限决议**

  User reviewer 可返回全部 scope；AutoReview 仅允许 allow-once/deny；
  `ApprovalPolicy::Never` 和 reviewer unavailable 直接 deny。

- [ ] **Step 5: 验证并提交**

```bash
cargo fmt --all
cargo test -p a2ui -p agent network_approval
cd apps/desktop && npx tsc --noEmit
cd apps/desktop && npm run build
cargo clippy -p a2ui -p agent --all-targets --no-deps -- -D warnings
git diff --check
git add -- crates/agent-a2ui crates/agent-core apps/desktop/src/hooks/chat/useChatSession.ts
git commit -m "feat: add managed network approval surface"
```

### Task 5: 把 decider 接入 `ToolOrchestrator`

**Files:**
- Modify: `crates/agent-core/src/streaming/tools_exec.rs`
- Modify: `crates/agent-core/src/runtime/tool_dispatch.rs` only if cancellation ownership requires
- Modify: focused orchestrator tests

- [ ] **Step 1: 先写端到端 orchestrator 失败测试**

  记录 tool handler 执行次数，触发一个 allowlist miss，批准后断言：

  - 原请求成功；
  - handler 只执行一次；
  - 未构造 escalated `SandboxAttempt`；
  - filesystem policy 和 network sandbox context 未改变。

- [ ] **Step 2: 构造 `NetworkApprovalSpec` 和 per-attempt decider**

  Spec 使用 `StepContext` 中已冻结的 profile，turn id、call id、tool name 和
  command preview，不在 proxy callback 中重读可热更新的选择。

- [ ] **Step 3: 将 decider/attribution 传入 `StartedNetworkProxy`**

  子进程启动前必须完成 proxy bind 和 decider 注入；任一步失败都不 spawn child。

- [ ] **Step 4: 保留 structured denial 分路**

  deny/timeout/cancel 仍由 runtime 转成 `SandboxErr::Denied` 且
  `network_policy_decision.is_some()`，orchestrator 不进入 filesystem review/retry。

- [ ] **Step 5: 验证并提交**

```bash
cargo fmt --all
cargo test -p agent tool_orchestrator
cargo test -p tools terminal
cargo test -p tools code_exec
cargo clippy -p agent -p tools --all-targets --no-deps -- -D warnings
git diff --check
git add -- crates/agent-core crates/agent-tools
git commit -m "feat: approve managed network requests inline"
```

### Task 6: 实现 persistent exact-host amendment

**Files:**
- Modify: `crates/agent-memory/src/config.rs`
- Modify: `crates/agent-memory/tests/*` or inline config tests
- Modify: `crates/agent-core/src/control/network_approval.rs`
- Modify: relevant permission audit tests

- [ ] **Step 1: 先写持久校验失败测试**

  覆盖 normalized exact match、mismatch、wildcard、built-in profile、inherited leaf profile、
  YAML 其他键保留和保存失败。

- [ ] **Step 2: 增加 atomic config helper**

  在选中 custom leaf profile 的 `network.domains` 中写入 exact host action，
  使用现有 YAML 保留式读改写通道，不重建整份 config。

- [ ] **Step 3: 实现 `persist_network_policy_amendment`**

  严格比较 amendment/context normalized host。只有持久成功后才更新
  `session_approved_hosts` / `session_denied_hosts` 并放行当前请求。

- [ ] **Step 4: 记录 audit 与非敏感结果**

  区分 `persistent_allow`、`persistent_deny`、`persistence_failed`，
  不记录 URL path/query/header/body。

- [ ] **Step 5: 验证并提交**

```bash
cargo fmt --all
cargo test -p memory permission
cargo test -p agent network_approval
cargo clippy -p memory -p agent --all-targets --no-deps -- -D warnings
git diff --check
git add -- crates/agent-memory crates/agent-core
git commit -m "feat: persist exact host network amendments"
```

### Task 7: 架构文档、全量验证与独立复审

**Files:**
- Modify: `docs/04-详细设计阶段/01-核心引擎层/07-Agent生命周期详细设计.md`
- Modify: this plan checklist
- Modify: design status only if implementation reveals an approved deviation

- [ ] **Step 1: 把已落地生命周期写入 Agent 架构设计**

  记录 inline request continuation、service/cache ownership、persistent amendment 边界、
  cancellation 和实际测试证据；不把未实现的 Deferred/background 标为完成。

- [ ] **Step 2: 运行分层全量验证**

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

- [ ] **Step 3: 安全复审**

  重点检查：命令是否可能重跑、hard deny 是否可被覆盖、cache key 是否缺 protocol/port、
  cancellation 是否留下孤儿 HITL、persistent amendment 是否能写 wildcard/错 profile，
  以及审计是否泄露 credential。

- [ ] **Step 4: 只提交本计划的 tracked files**

```bash
git status --short
git diff --check
git add -- docs/04-详细设计阶段/01-核心引擎层/07-Agent生命周期详细设计.md \
  docs/superpowers/plans/2026-08-19-inline-managed-network-approval.md \
  docs/superpowers/specs/2026-08-19-inline-managed-network-approval-design.md
git commit -m "docs: record inline network approval lifecycle"
```

## 实施停止条件

任一 Task 出现以下情况时，不带着未证实假设继续下一 Task：

- 必须重跑整条命令才能放行网络；
- macOS sandbox 需要放开任意 outbound 而非精确 proxy port；
- approval callback 无法绑定 turn/call；
- persistent rule 无法在保留其他 config 的前提下原子更新；
- 完成必须覆盖工作区内与本任务无关的已跟踪修改。
