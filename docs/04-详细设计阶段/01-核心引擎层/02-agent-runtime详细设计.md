# agent-runtime 详细设计

> **当前基线（2026-09-07）**：旧独立 `agent-runtime` crate 已不存在，其功能已全部合并到 `agent-core`（package name: `agent`）的以下子模块：`runtime/`（Session 生命周期、AstroThread、submission_loop、TurnContext、StepContext、ToolRouter、预算）、`tasks/`（SessionTask、ActiveTurn、四种 TaskKind）、`streaming/`（多轮 streaming、Provider fallback、工具执行）、`exec/`（AgentControlDirectory、AgentRuntimeManager、subagents、cron、background）、`control/`（HITL、interrupt、smart approval、guardian）、`prompt/`（上下文组装）。入口为 `AstroThread -> submission_loop -> SessionTask::run -> streaming::multi_turn::run_turn`。以 [Agent Harness 执行外壳](14-Agent-Harness执行外壳详细设计.md) 为准。

> 本文档由原 02-agent-runtime详细设计 和 34-agent-runtime执行引擎 合并而成。

---

## 1. 当前架构概述

独立 `agent-runtime` crate 已合并到 `agent-core`。当前的执行编排层级：

| 层 | 位置 | 职责 |
| --- | --- | --- |
| 中央运行时 | `agent-core`（`crates/agent-core`） | Session 生命周期、submission_loop、Turn/Step 管线、工具路由、HITL、压缩、prompt |
| 多厂商 Provider | `agent-providers`（`crates/agent-providers`） | Responses/Chat/Anthropic/Gemini 协议、流式解析、fallback |
| gRPC 服务端 | `agent-server`（`crates/agent-server`） | Thread submit/resume/subscribe RPC、ThreadHistoryBuilder |
| 桌面集成 | `apps/desktop/src-tauri` | Tauri 2 shell、内嵌 backend、事件桥接 |

当前核心对象：

- **AstroThread** -- 绑定 Session/SessionIo/RolloutRecorder，启动 submission_loop，提交 Op，读取 Event
- **Session**（`AgentLoop` 别名）-- 会话级状态、服务、MCP Hub、当前任务与事件分发
- **SessionTask** -- 可取消的四种任务：RegularTask / CompactTask / ReviewTask / UserShellTask
- **TurnContext** -- 每轮上下文（turn id、交互模式、输入准入）
- **StepContext** -- 每次 sampling 的工具 schema、wire route、配置快照
- **ToolRouter** -- wire name 到 handler/MCP 路由

当前模块结构见 [01-agent-core详细设计.md Section 2](01-agent-core详细设计.md)。

---

## 2. 常规 Turn 管线

```text
RegularTask::run
  -> TurnStarted
  -> Session::prepare_turn(input)
       -> begin_user_turn
       -> persist user input
       -> reload tools and MCP
       -> context retrieval / memory
       -> PromptContract
       -> StepContext seed
  -> open input admission
  -> streaming::multi_turn::run_turn
       -> maintain context
       -> capture step context
       -> provider stream with fallback
       -> persist assistant tool calls
       -> execute tools
       -> persist observations
       -> repeat or finish
  -> AgentEnd hook
  -> TurnComplete / TurnAborted
```

内层循环不得绕开以下顺序：assistant tool call 先记录 -> 工具再执行 -> tool result 带原 call id 记录 -> 下一次 sampling 使用成对历史。

---

## 3. run_turn 内层循环

`streaming::multi_turn::run_turn` 是核心 turn 循环（位于 `crates/agent-core/src/streaming/multi_turn.rs`）。

### 3.1 RunTurnArgs

```rust
pub(crate) struct RunTurnArgs {
    pub session: Arc<Session>,
    pub turn_context: Arc<TurnContext>,
    pub targets: Vec<ModelTarget>,       // primary + fallback 链
    pub base_config: ProviderConfig,
    pub system_prompt: String,
    pub prompt: Option<TurnResult>,      // Continue / Steered / prebuilt prompt
    pub pause: Arc<PauseControl>,
    pub hitl_gate: Arc<HitlGate>,
    pub responses_override: Option<ResponsesOverride>,
    pub drain_mailbox: bool,             // 是否消费 subagent 邮箱
}
```

### 3.2 循环流程

```text
run_turn(args, cancellation_token) -> SessionTaskResult
  │
  ├── 创建 IterationBudget（默认 90 轮）
  │
  └── loop {
        ① 检查 cancel / pause
        ② 记录 pending input
        ③ 如果 drain_mailbox：drain_mailbox_at_safe_boundary
        ④ pre_llm_maintenance（压缩）
        ⑤ capture_step_context（冻结工具路由、history、prompt context）
        ⑥ build_prompt（从 PromptContract + StepContext 构建指令+输入+工具）
        ⑦ 检查 settings generation 更新
        ⑧ ProviderStreamer::stream_prompt（含 fallback 链）
        ⑨ 累积 text / reasoning / tool deltas
        ⑩ thinking-only 重试（最多 MAX_THINKING_ONLY_RETRIES = 1 次）
        ⑪ Stop hook 验证（最多 MAX_VERIFY_ATTEMPTS = 2 次）
        ⑫ TransformFinalLlmOutput hook
        ⑬ 执行工具（串行或并发）
        ⑭ 记录工具结果
        ⑮ post_tool_maintenance（压缩 + stop_after_tool_call 检查）
        ⑯ code_exec 轮次 refund budget
        ⑰ 无更多工具调用 → 退出；否则回到 ①
      }
  │
  └── budget 耗尽 → run_max_iterations_summary（空工具最终一轮摘要）
```

### 3.3 ProviderStreamer 与 Fallback

```rust
pub struct ProviderStreamer {
    targets: Vec<ModelTarget>,              // primary + fallback
    base_config: ProviderConfig,
    responses_override: Option<ResponsesOverride>,
}
```

`try_stream_responses_with_fallback` 按 targets 顺序尝试。仅在首个有意义 content 到达**之前**允许 failover。`is_failover_eligible` 对 429/401/403/5xx/timeout/connection/TLS/DNS 返回 true。

### 3.4 RunPhase 状态机

```rust
pub enum RunPhase {
    StreamingLlm,
    ExecutingTools,
    AwaitingHitl,
    Summarizing,
    Finished,
    Cancelled,
    Error,
}
```

`RunState` 在 `AwaitingHitl` 阶段携带 `RunRequirements`（`UserConfirmation` / `UserInput`），前端据此显示不同 UI。

---

## 4. 任务系统

### 4.1 SessionTask trait

```rust
pub(crate) trait SessionTask: Send + Sync + 'static {
    fn kind(&self) -> TaskKind;
    fn span_name(&self) -> &'static str;
    async fn run(self: Arc<Self>, session: Arc<Session>, ctx: Arc<TurnContext>,
                 input: Vec<TurnInput>, cancellation_token: CancellationToken)
        -> SessionTaskResult;
    async fn abort(&self, session: Arc<Session>, ctx: Arc<TurnContext>) { }
}
```

### 4.2 四种 TaskKind

| TaskKind | 行为 |
|---|---|
| `Regular` | 标准 Responses sampling 与工具循环。支持正常模式和 recovery 模式（不追加新 input）。 |
| `Compact` | 手动会话压缩。拒绝用户输入，生成 `generate_manual_summary()`，替换 history，发送 `ContextCompacted` 事件。 |
| `Review` | 隔离代码审查。创建 `isolated_review_session`（只读权限、网络禁用、工具白名单限制），在子 Session 中运行 `run_turn`，将结果回传父 Session。 |
| `UserShell` | 用户发起的 Shell 命令。检测 `$SHELL`（fallback `/bin/sh`），stdout/stderr 独立流式输出 `ExecCommandOutputDelta`，单流上限 `MAX_SHELL_CAPTURE_BYTES` = 1MB。 |

### 4.3 ActiveTurn

```rust
pub(crate) struct ActiveTurn {
    task: Option<RunningTask>,
}

pub(crate) struct RunningTask {
    kind: TaskKind,
    task: Arc<dyn AnySessionTask>,
    cancellation_token: CancellationToken,
    turn_context: Arc<TurnContext>,
    completion: CancellationToken,      // 生命周期信号
    handle: JoinHandle<()>,
    auxiliary_handles: Vec<JoinHandle<()>>,
}
```

`Session` 最多持有一个 `ActiveTurn`。`spawn_task` 先 abort 旧任务再安装新任务。abort 超时：`TASK_ABORT_TIMEOUT` = 5s（生产）/ 50ms（测试）。

---

## 5. 工具执行

### 5.1 串行执行

`execute_tools_serial`（位于 `streaming/tools_exec.rs`）逐个工具执行，每个工具经过授权链：

1. **Workflow/Browser/MCP 预检** -- 特殊工具类型的快速路径
2. **只读变异预检** -- 判断是否为纯读操作
3. **危险命令分类** -- Guardian 评估（`GuardianVerdict`：`ApproveOnce` / `Deny` / `Indeterminate`）
4. **沙箱策略** -- `PermissionProfile`（read-only / workspace-write / danger-full-access）
5. **受管网络代理** -- `ManagedNetworkProxy` per-attempt 租约
6. **工具调用** -- `handle_tool_invocation_with_once_grants`
7. **沙箱拒绝重试** -- 自动升级权限重试
8. **HITL park** -- confirm/clarify 结果的 HITL 阻塞

### 5.2 并发执行

`execute_tools_concurrent` 通过 `JoinSet::spawn_blocking` 并发执行所有工具调用，使用 `ToolCallRuntime`，结果按原始顺序收集。

### 5.3 权限审批路由

```rust
pub(crate) enum ApprovalRoute {
    Deny,           // 硬拒绝
    Allowlist,      // 命令白名单自动通过
    TypeAllowlist,  // 类型白名单自动通过
    Off,            // 审批关闭
    Smart,          // SmartApproval 自动评估
    Manual,         // 用户手动确认
}
```

---

## 6. 控制层

### 6.1 HitlGate（HITL 闸门）

Hermes 风格阻塞闸门，位于 `control/hitl.rs`：

```rust
pub struct HitlGate {
    // 单会话闸门，per-interrupt oneshot
}

pub struct HitlRequest {
    pub tool_call_id: String,
    pub reason: String,          // "confirmation" / "tool_call" / "input_required"
    pub message: String,
    pub operations: Value,       // A2UI 操作列表
    pub response_schema: Value,  // JSON Schema 校验
    pub timeout: Duration,       // 默认 HITL_DEFAULT_TIMEOUT_SECS = 600
}
```

关键方法：
- `request(req) -> (Interrupt, HitlResolution)` -- park 直到 resume/cancel/timeout
- `resolve(items: &[ResumeItem])` -- 带 schema 校验的批量 resolve
- `cancel_all()` -- 取消所有 pending interrupt

### 6.2 Interrupt 状态机

AG-UI 风格 interrupt 挂起，位于 `control/interrupt.rs`：

```rust
pub struct Interrupt {
    pub id: String,
    pub reason: String,              // "tool_call" / "input_required" / "confirmation"
    pub message: String,
    pub tool_call_id: String,
    pub response_schema_json: String,
    pub expires_at: String,
}

pub struct ResumeItem {
    pub interrupt_id: String,
    pub status: String,              // "resolved" / "cancelled"
    pub payload_json: String,
}
```

`InterruptPending::apply_resume` 要求覆盖所有 open interrupt；校验 payload 符合 reason 语义（`confirmation` 需 `approved: bool`，`input_required` 需非空 `value`）。

### 6.3 SmartApproval

位于 `control/smart_approval.rs`。使用辅模型在 8 秒超时内自动评估工具调用风险：

```rust
pub struct SmartApprovalContext {
    pub recent_turns: Vec<TurnSummary>,      // 近期对话摘要
    pub current_task_description: String,
    pub tool_call_chain: String,             // 工具调用链
}

pub enum GuardianDecision {
    ApproveOnce,     // 自动通过
    Deny,            // 拒绝
    Indeterminate,   // 不确定，fallback 到用户确认
}
```

`redact_sensitive(text)` 对敏感信息（API key、token 等）进行正则脱敏。

### 6.4 SessionApprovalCache

位于 `control/approval_cache.rs`。会话级审批缓存，同命令模式不重复弹窗：

```rust
pub struct ApprovalCacheKey {
    pub tool_name: String,
    pub command_prefix: String,    // 命令前 2 个 token
}
```

支持父子继承：`derive_child_cache` 创建子缓存，子读取父链但写入不向上传播。

### 6.5 渐进信任模型

位于 `control/trust_model.rs`：

| 连续审批次数 | 信任等级 |
|---|---|
| < 5 | `AlwaysAsk`（默认） |
| >= 5 | `SmartReview`（自动评估） |
| >= 10 | `AutoApprove`（自动通过） |

任意拒绝重置为 `AlwaysAsk`。不持久化，会话结束即丢弃。

### 6.6 网络审批

位于 `control/network_approval.rs`：

```rust
pub enum ApprovalScope {
    Once,        // 单次允许
    Session,     // 会话期间允许
    Persistent,  // 持久化到磁盘
}
```

`NetworkApprovalService` 使用 first-owner 模式：首个请求者成为 `PendingHostApprovalOwner`（RAII handle），并发请求同主机的调用方 `Join` 等待。Owner drop 时未 resolve 则自动 Deny。

---

## 7. 子 Agent 系统

### 7.1 AgentControlDirectory

进程级单例（`OnceLock`），位于 `exec/agent_control_directory.rs`：

```rust
const DEFAULT_LIMITS: Limits = Limits {
    max_threads: 32,    // 单根会话可派生的线程总数
    max_depth: 8,       // Agent 树最大嵌套深度
    max_running: 8,     // 同时活跃执行的子 Agent 并发数
};
```

`open_root(root_session_id)` 打开或复用 `AgentControl`（weak-ref 缓存），并执行崩溃恢复（清理 pending 预留、将 Running 恢复为 Interrupted）。

### 7.2 AgentRuntimeManager

进程级单例（`Arc`），位于 `exec/agent_runtime.rs`。管理活跃 Agent turn 的完整生命周期：

- `start_turn(request)` -- 编排 turn 生命周期（Session 创建、配置、background 执行）
- `interrupt(thread_id)` -- 中断活跃 turn
- `terminate(thread_id)` -- 终止 turn
- `active_count()` -- 当前活跃 turn 数

`StartTurnOwnerGuard`（RAII）确保 Drop 时持久化 interrupted/shutdown 终态、清理 active slots、通过 watch channel 发布终止信号。

### 7.3 DefaultAgentThreadDispatch

实现 `AgentThreadDispatch` trait（6 个模型可见工具 + 桌面控制面），位于 `exec/dispatch.rs`：

**模型工具**（6 个）：`spawn_agent`、`list_agents`、`send_message`、`followup_task`、`wait_agent`、`interrupt_agent`

**桌面控制面**（`DesktopAgentThreadControl` trait）：`snapshot`、`read_thread`、`followup`、`interrupt`、`close_subtree`

---

## 8. 后台执行

### 8.1 Background Turn

`exec/background.rs` 提供后台 turn 执行：

- `run_background_multi_turn(session, targets, input)` -- 基础后台 turn
- `run_background_multi_turn_controlled(session, targets, input, control)` -- 带 AgentThreadControl（interrupt/close 桥接）

桥接 `AgentThreadControl.cancelled()` 到 `PauseControl.cancel()`。使用 `install_multi_turn_task` 统一引擎，丢弃 UI 事件，收集 assistant text + usage。

### 8.2 Cron 执行

`exec/cron.rs`：`execute_job(job, credentials)` 运行定时任务，含孤儿恢复（`reconcile_orphaned_runs`）。

### 8.3 Memory Review

`exec/memory_review.rs`：`spawn_background_review_after_turn` 在 turn 结束后异步提炼记忆（preferred + fallback 链）。

### 8.4 Title Generation

`exec/title_generation.rs`：`spawn_title_generation_after_turn` 异步生成会话标题（`TITLE_MAX_CHARS` = 40）。

### 8.5 Mid-run Summary

`exec/mid_run_summary.rs`：`MID_RUN_SUMMARY_RATIO` = 0.80 占用时自动生成 handoff 摘要，`collapse_history_with_handoff` 保留 head + summary + tail 结构。

---

## 9. 网络容错

### 9.1 两层重试

网络恢复对齐 Codex，分为互不混用的两层：

1. `agent-providers` request 层：默认在首次请求后最多重试 4 次，200ms 指数退避并带 `0.9..1.1` jitter；只处理 connection/timeout/network/5xx。
2. `agent-core::streaming` sampling 层：request retry 耗尽后，前台交互式 Turn 对明确的 `ConnectionFailed` 按 5/10/20/40/60 秒持续等待，60 秒封顶，直到恢复或用户中断。

### 9.2 错误分类策略

| 错误类型 | 处理策略 |
| --- | --- |
| DNS/TCP/TLS/CONNECT | 前台持续重连；后台有界重试；不切换 Provider |
| 请求超时 | request 层有限重试，随后进入有界 stream retry |
| 500 / 503 等 5xx | request 层有限重试；耗尽后可按显式 fallback 链切换 |
| 429 | 不进入通用 request retry；按服务端提示和显式 fallback 产品策略处理 |
| 400 / 上下文 / 内容拒绝 | 不重试、不 fallback，立即失败 |
| 401 / 403 | 不重试；保留显式 fallback 产品策略 |
| 用户取消 | 立即 `TurnAborted` |

### 9.3 断网事件

前台断网时发送专用、非终态 `StreamError`（不写入 assistant 内容、不结束 Turn、不切换模型）。对外通知使用 `ErrorNotification.will_retry=true`。网络恢复后自动继续同一 `turn_id`。等待实现必须同时监听 Turn cancellation 和 pause/interrupt，不能使用不可取消的裸 `sleep`。

### 9.4 重试参数

- request retry：默认 4 次，base 200ms、factor 2、jitter `0.9..1.1`
- stream retry：默认 5 次
- foreground connection retry：5 秒起步、倍增至 60 秒后保持，无次数上限
- background connection retry：必须有界
- 所有等待均可取消
- 全局 Token 速率窗口仍按 Provider 分别限速

---

## 10. 集成关系

```text
AppState（Tauri）
    ├── AstroThread
    │       ├── Session（agent-core）
    │       │       ├── SessionServices
    │       │       │       ├── SharedConversationStore（agent-session）
    │       │       │       ├── MemoryManager（agent-memory）
    │       │       │       ├── ToolRegistry（agent-tools）
    │       │       │       ├── CodeModeService（QuickJS）
    │       │       │       ├── AgentControl（agent-subagents）
    │       │       │       └── NetworkApprovalService
    │       │       ├── ThreadControls
    │       │       │       ├── PauseControl（agent-providers）
    │       │       │       ├── HitlGate（control/hitl）
    │       │       │       └── SessionApprovalCache（control/approval_cache）
    │       │       ├── McpHub（agent-mcp）
    │       │       ├── HookRuntime（agent-hooks）
    │       │       ├── AgentThreadDispatch（exec/dispatch）
    │       │       └── ActiveTurn（tasks/）
    │       └── SessionIo（bounded(512) submission + event channel）
    ├── AgentRuntimeManager（exec/agent_runtime — 进程单例）
    ├── AgentControlDirectory（exec/agent_control_directory — 进程单例）
    ├── ProviderRegistry（agent-providers）
    └── HitlRegistry（control/hitl — 进程级 gate 注册表）
```

---

## 11. 相关文档

- [01-agent-core详细设计.md](01-agent-core详细设计.md) -- Crate 职责、模块组织、核心对象、双重预算
- [03-交互执行模式设计.md](03-交互执行模式设计.md) -- 交互模式、输入准入、审批流程
- [14-Agent-Harness执行外壳详细设计.md](14-Agent-Harness执行外壳详细设计.md) -- Harness 当前实现基线
