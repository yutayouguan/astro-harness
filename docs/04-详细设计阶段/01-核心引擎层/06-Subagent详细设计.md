# Subagent（Codex V2 Agent Threads）详细设计

> **Harness 当前基线（2026-09-07）**：本文是 Subagent 子系统的现行代码级契约。模型工具只有 `spawn_agent/list_agents/send_message/followup_task/wait_agent/interrupt_agent`；Graph/mailbox/status 使用 `{base}/data/subagents-v2.db`，真实 Session 时间线使用 `{base}/data/state.db`，不隐式创建 worktree。旧 Supervisor/DelegateRunner 内容仅是迁移背景，不构成兼容接口。

> 版本：v2.0 | 日期：2026-08-20 | 状态：已落地
> 对应需求：F-30 Subagent、F-04 Skills 系统、M-08 安全边界
> 架构版本：Codex V2 Agent Thread（替代原 Supervisor/DelegateRunner/spawn_depth 设计）
> 上游文档：[Subagent 系统设计](../../03-系统设计阶段/02-核心功能模块/05-Subagent系统设计.md)

---

## 0. Codex V2 Agent Thread 架构概述

### 0.1 核心模型

- 子 Agent 是独立的 **Agent Thread**，具有自己的会话上下文、模型调用、工具循环和持久化状态。
- 父 Agent 通过 6 个模型工具（`spawn_agent` / `list_agents` / `send_message` / `followup_task` / `wait_agent` / `interrupt_agent`）管理线程。
- 桌面控制面提供 `read_subagent_thread` / `close_subagent_thread` 额外操作。
- `fork_turns = none | all | N` 显式控制初始上下文快照，不再强制"完全隔离且不可派生后追问"。
- 默认共享当前项目工作区，不隐式创建 git worktree；并行写任务必须由父 Agent 规划文件所有权。

### 0.2 运行方式与权限解耦

| 维度 | 可选值 | 职责 |
| --- | --- | --- |
| Agent 线程 | primary / spawned | 上下文与生命周期隔离 |
| 运行适配器 | foreground / background | 是否输出 UI 流、时间线和交互事件 |
| 权限策略 | read-only / workspace-write / danger-full-access | 文件、终端和网络的可用边界 |
| 审批策略 | user / auto-review / never | 风险操作的决策方式 |

`danger-full-access` 不等于 background，background 也不等于无权限约束。子 Agent 继承父任务已解析的权限上限，自定义 Agent 只能收紧，不能扩权。

### 0.3 统一运行内核

foreground 与 background 必须共享同一个多轮 round engine：Provider fallback、context maintenance、hooks、tool execution、iteration budget、usage 和 cancellation 只实现一次。差异仅由适配器表达：

```text
AgentThread
  └─ UnifiedRoundEngine
       ├─ ForegroundAdapter  -> token/timeline/HITL UI
       └─ BackgroundAdapter  -> summary/status/non-interactive result
```

Cron 和 Subagent 使用 `BackgroundAdapter`；普通聊天使用 `ForegroundAdapter`。旧 `headless` 术语废弃，避免与 sandbox / approval 语义混淆。

当前实现以 `streaming::run_multi_turn_stream` 作为统一 round engine，
`exec::background` 仅消费事件并返回最终文本与 usage；它不再包含独立的 Provider、预算或工具执行循环。

### 0.4 已删除的 V1 概念

以下 V1 概念在 V2 中已不存在：

- `DelegateRunner` / `DelegateAsyncSpawner` / `AsyncDelegateRegistry`
- `Supervisor` / `ChildAgent` / `ChildState` / `SubAgentResult`
- `spawn_depth` / `AgentContext.depth` 递增
- `OrchestrationDb` / `TeamDefinition` / Coordinate / Route / Broadcast / Tasks 模式
- `delegate_task` / `delegate_task_batch` 工具
- `agent-orchestration` crate

---

## 1. 子 Agent 架构概述

### 1.1 设计动机

主 Agent 执行复杂任务时面临三重挑战：

1. **上下文窗口压力**：单次对话的 token 容量有限，大规模代码审查或多文件重构容易撞上 token 预算上限
2. **任务并行需求**：互不依赖的子任务（如同时搜索三个目录、并行审查多个文件）天然适合并发执行
3. **专项能力隔离**：安全审计、翻译、测试生成等子任务需要独立的系统提示和工具集，混合在一个上下文中会降低推理质量

V2 通过 **Agent Thread** 模式解决上述问题：父 Agent 将子任务派生为独立的 Agent Thread，每个线程拥有独立的 Session（干净的消息历史）、自己的模型调用循环和持久化状态。线程完成后返回精简摘要，父 Agent 还可通过邮箱追问和追加任务。

### 1.2 核心原则

| 原则 | 说明 |
|------|------|
| 线程级隔离 | 子 Agent Thread 拥有独立的 Session 上下文，通过 `fork_turns` 控制初始快照 |
| 权限不可升级 | 子 Agent 的权限上限 = 父 Agent 已解析权限上限，自定义 Agent 只能收紧 |
| Token 高效 | 子 Agent 仅返回精简摘要，不向父 Agent 回传完整对话历史 |
| 树深度有界 | `AgentRegistry.Limits.max_depth` 控制最大树深度，超限返回工具错误 |
| 可追问可恢复 | 线程可通过 `send_message` / `followup_task` 持续交互，不是一次性委派 |

### 1.3 V2 Agent Thread 树

```text
                    AgentThread (root)
                     path: /root
                         │
               ┌─────────┼─────────┐
               │         │         │
          spawn_agent spawn_agent spawn_agent
               │         │         │
               ▼         ▼         ▼
          Thread A    Thread B   Thread C
        /root/research /root/review /root/test
               │
               ▼
          Thread A1
       /root/research/deep
```

每个线程有唯一的 `AgentPath`（如 `/root/research`），父 Agent 通过路径名寻址子线程。`AgentGraphStore` 持久化完整线程树。

---

## 2. AgentControl 根级控制器

### 2.1 核心结构体

```rust
// crates/agent-subagents/src/control.rs

/// 根级共享控制器：聚合存储、注册表、活动总线和运行时句柄。
#[derive(Clone)]
pub struct AgentControl {
    root_thread_id: String,
    store: AgentGraphStore,              // SQLite 持久化
    registry: Arc<AgentRegistry>,        // RAII 内存注册表
    activity: Arc<ActivityBus>,          // 活动事件总线
    runtimes: Arc<RuntimeHandleRegistry>, // 运行时句柄注册
    runtime_lifecycle: Arc<Mutex<RuntimeLifecycleState>>, // close/spawn 准入状态
    root_service_tier: Arc<Mutex<Option<String>>>,        // 根服务层级
    lifecycle_notify: Arc<Notify>,                        // 准入协调通知
    #[cfg(test)]
    before_runtime_insert_hook: Arc<Mutex<Option<BeforeRuntimeInsertHook>>>,
}
```

`AgentControl::open()` 确保根线程存在，并从 `AgentGraphStore` 快照重建 `AgentRegistry`。如果快照中仍有非根 `PendingInit` 预留，它会拒绝打开，要求先走独占恢复，而不是静默清理。每个根 Agent（主对话）对应一个 `AgentControl` 实例。

### 2.2 AgentGraphStore 持久化

```rust
// crates/agent-subagents/src/store.rs

/// 子 Agent 图的 SQLite 存储句柄（WAL 模式）。
#[derive(Debug, Clone)]
pub struct AgentGraphStore {
    pool: SqlitePool,
    path: PathBuf,  // 默认 ~/.astro/data/subagents-v2.db
}
```

`AgentGraphStore` 管理：

| 数据 | 说明 |
|------|------|
| `AgentThreadV2` | 线程元数据（路径、状态、时间戳） |
| `StoredStatusEvent` | 持久化的状态事件序列（含序号） |
| `MailboxMessage` | 线程间邮箱消息 |
| `AgentRuntimeDescriptorV2` | 恢复持久线程所需的运行时描述符 |
| `ThreadReservation` | 运行器启动前的预留身份 |

### 2.3 AgentRegistry RAII 注册表

```rust
// crates/agent-subagents/src/registry.rs

/// Agent 线程树的资源配额。
pub struct Limits {
    pub max_threads: usize,    // 最大线程数
    pub max_depth: usize,      // 最大树深度
    pub max_running: usize,    // 最大并发执行数
}

/// 线程树内存注册表，管理路径→线程映射、预留槽位和执行许可。
pub struct AgentRegistry {
    root_thread_id: String,
    limits: Limits,
    state: Mutex<RegistryState>,
}
```

`AgentRegistry` 提供：

- `reserve_spawn()` — 在父路径下预留子线程派生槽位（返回 `SpawnReservation`）
- `acquire_execution()` — 获取执行许可（`ExecutionPermit`，RAII 持有）
- 路径→线程 ID 的双向查询
- 深度和并发限制的前置检查

### 2.4 RuntimeHandleRegistry

```rust
// crates/agent-subagents/src/control.rs

/// Agent 运行时句柄，提供中断和终止回调。
#[derive(Clone)]
pub struct AgentRuntimeHandle {
    pub interrupt: Arc<dyn Fn() + Send + Sync>,
    pub terminate: Arc<dyn Fn() + Send + Sync>,
}

/// 线程 ID 到运行时句柄的注册表。
#[derive(Default)]
pub struct RuntimeHandleRegistry {
    handles: Mutex<HashMap<String, AgentRuntimeHandle>>,
}
```

运行时启动后注册句柄，`interrupt_agent` 和 `close_subagent_thread` 通过句柄执行中断/终止。

---

## 3. 线程状态机

### 3.1 状态定义

```rust
pub enum AgentStatusV2 {
    PendingInit,                           // 预留完成，运行时尚未启动
    Running,                               // round_loop 执行中
    Interrupted,                           // 被 interrupt_agent 中断
    Completed { last_message: String },    // 正常完成
    Errored { message: String },           // 执行出错
    Shutdown,                              // 运行时已终止
}
```

### 3.2 状态转换

```text
PendingInit ──────→ Running
                      │
                      ├──→ Completed { last_message }
                      │      ↓ (followup_task)
                      │    Running  ← 可重新进入
                      │
                      ├──→ Interrupted
                      │      ↓ (followup_task)
                      │    Running  ← 可恢复
                      │
                      └──→ Errored { message }
                             │
                             ▼
                          Shutdown
```

### 3.3 RunnerEvent 驱动投影

```rust
pub enum RunnerEvent {
    TurnStarted { turn_id: String },
    TurnCompleted { turn_id: String, last_message: String },
    TurnInterrupted { turn_id: String, reason: String },
    TurnErrored { turn_id: String, message: String },
    RuntimeTerminated,
}
```

`RunnerEvent` 由子 Agent 运行时发出，驱动 `AgentStatusV2` 状态投影并唤醒 `wait_agent` 等待方。

---

## 4. 权限继承模型

### 4.1 核心原则：不可升级

子 Agent 的权限上限始终不超出父 Agent 已解析的权限。自定义 Agent 类型（`.astro/agents/*.toml`）可通过 `sandbox_mode` 等字段收紧权限，但不能扩权。

### 4.2 自定义 Agent 配置

配置仅从 `.astro/agents` 目录加载，`.codex/agents` 会被忽略：

```text
加载顺序（后加载覆盖前加载）：
1. 内置 Agent 定义
2. ~/.astro/agents/*.toml         （全局自定义）
3. <project>/.astro/agents/*.toml （可信项目自定义）
```

```toml
# .astro/agents/reviewer.toml
name = "reviewer"
description = "代码审查专家"
developer_instructions = "你是一个代码审查专家，专注于..."
model = "claude-sonnet-4"
sandbox_mode = "read-only"

[skills]
config = [
  { path = "security-review", enabled = true }
]
```

### 4.3 工具集控制

子 Agent 的工具集由运行时环境决定：

- 继承父 Agent 的核心工具集
- 自定义 Agent 可通过 `skills` 配置额外启用/禁用 Skill
- 6 个子 Agent 管理工具（`spawn_agent` 等）在子 Agent 内同样可用，允许子 Agent 再次派生（由 `AgentRegistry.Limits.max_depth` 控制）

---

## 5. 并发与资源控制

### 5.1 Limits 配额

```rust
pub struct Limits {
    pub max_threads: usize,    // 线程树最大总线程数
    pub max_depth: usize,      // 最大树深度（路径段数）
    pub max_running: usize,    // 最大并发执行线程数
}
```

### 5.2 SpawnReservation RAII 预留

派生前先通过 `AgentRegistry::reserve_spawn()` 预留槽位：

```rust
// crates/agent-subagents/src/registry.rs

/// 派生预留令牌，drop 时自动回滚未提交的预留。
pub struct SpawnReservation<'a> {
    registry: &'a AgentRegistry,
    path: AgentPath,
    thread_id: String,
    thread: AgentThreadV2,
    persisted_store: Option<&'a AgentGraphStore>,
    activity: Option<&'a ActivityBus>,
    active: bool,
}

impl SpawnReservation<'_> {
    pub async fn commit(self) -> anyhow::Result<()>;  // 确认派生（异步，含持久化校验）
    pub async fn abort(self) -> anyhow::Result<()>;   // 取消预留（异步，含持久化回滚）
    // Drop 自动 abort
}
```

### 5.3 ExecutionPermit

```rust
// crates/agent-subagents/src/registry.rs

/// 执行许可令牌，drop 时自动释放并发执行槽位。
#[derive(Debug)]
pub struct ExecutionPermit<'a> {
    registry: &'a AgentRegistry,
    thread_id: String,
    active: bool,
}
```

超出 `max_running` 时，新的执行请求会返回错误，不静默排队。

### 5.4 AgentSpawnReservation 层

`AgentControl` 在 `SpawnReservation` 外包装 `AgentSpawnReservation`（`control.rs`），增加生命周期租约管理：

```rust
// crates/agent-subagents/src/control.rs

/// 派生预留：在提交/中止/丢弃前对桌面递归关闭可见。
pub struct AgentSpawnReservation<'a> {
    inner: Option<SpawnReservation<'a>>,
    control: &'a AgentControl,
    lease_id: String,
}

impl AgentSpawnReservation<'_> {
    pub async fn commit(self) -> anyhow::Result<()>;  // 提交后释放租约
    pub async fn abort(self) -> anyhow::Result<()>;   // 取消预留后释放租约
    // Drop 自动回滚 inner 并释放租约
}
```

在提交/中止/丢弃前对桌面递归关闭（`CloseAdmissionGuard`）可见，防止在关闭操作进行中的路径前缀下派生新线程。`CloseAdmissionGuard`（`control.rs`）阻止新派生进入关闭中的路径前缀，并等待进行中的派生完成。

---

## 6. 线程间通信

### 6.1 通信模型

V2 使用 **邮箱 + 路径寻址** 通信模型：

```text
父 Agent ──spawn_agent──→ 子 Thread（PendingInit → Running）
    │
    ├──send_message──→ 子 Thread 邮箱（不触发新 turn，等待子 Agent 下次检查）
    │
    ├──followup_task──→ 子 Thread（触发新 turn，重新进入 Running）
    │
    ├──wait_agent──→ 等待邮箱活动 / 被转向 / 超时
    │
    └──interrupt_agent──→ 子 Thread（Running → Interrupted）
```

### 6.2 邮箱系统

```rust
// crates/agent-subagents/src/mailbox.rs

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MailboxMessage {
    pub sequence: i64,
    pub message_id: String,
    pub sender_thread_id: String,
    pub recipient_thread_id: String,
    pub kind: MailboxKind,
    pub payload: String,
    pub trigger_turn: bool,
}

/// 投递时使用的新消息结构（含幂等键）。
pub struct NewMailboxMessage {
    pub message_id: String,
    pub idempotency_key: String,
    pub sender_thread_id: String,
    pub recipient_thread_id: String,
    pub kind: MailboxKind,
    pub payload: String,
    pub trigger_turn: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MailboxKind {
    Message,    // 普通消息
    Followup,   // 追加任务
    Steer,      // 主任务转向
    Result,     // 结果回传
    Status,     // 状态通知
}
```

邮箱消息持久化在 `AgentGraphStore` 中。`send_message` 投递 `MailboxKind::Message` 到目标线程邮箱；`followup_task` 投递 `MailboxKind::Followup` 并设 `trigger_turn = true`；`wait_agent` 通过 `ActivityBus` 监听邮箱活动。

### 6.3 ActivityBus

```rust
// crates/agent-subagents/src/activity.rs

/// 有界环形缓冲活动总线，支持 watch 通知的异步等待。
pub struct ActivityBus {
    sequence: AtomicU64,
    tx: watch::Sender<ActivityCursor>,
    events: Mutex<VecDeque<AgentActivity>>,
}

/// 活动事件类型：派生、邮箱、状态变更、边关闭、主任务转向。
pub enum AgentActivityKind {
    Spawned { thread_id: String },
    Mailbox { thread_id: String },
    StatusChanged { thread_id: String },
    EdgeClosed { thread_id: String },
    MainSteer,
}

/// 带序号的活动事件，可选附带线程快照。
pub struct AgentActivity {
    pub sequence: u64,
    pub kind: AgentActivityKind,
    pub thread: Option<AgentThreadV2>,
}

/// 单调递增的活动序号游标，用于增量拉取事件。
pub struct ActivityCursor(pub u64);
```

`ActivityBus` 提供带 `ActivityCursor` 的增量活动观察机制，内部使用 `watch::channel` 驱动异步等待。`wait_agent` 在此等待直到有邮箱活动（`Mailbox`）、被转向（`MainSteer`）或超时。

---

## 7. 6 个模型工具详细设计

### 7.1 工具注册

```rust
// crates/agent-tools/src/builtin/agents/subagent.rs

pub const V2_AGENT_TOOL_NAMES: [&str; 6] = [
    "spawn_agent",
    "list_agents",
    "send_message",
    "followup_task",
    "wait_agent",
    "interrupt_agent",
];
```

6 个工具批量注册到 `ToolRegistry`，统一使用 `lifecycle_defaults` 风险等级配置。

### 7.2 spawn_agent

| 字段 | 类型 | 说明 |
|------|------|------|
| `task_name` | String（必填） | 子任务名称，同时用于生成 AgentPath 路径段 |
| `message` | String（必填） | 初始任务描述 |
| `agent_type` | Option\<String\> | 自定义 Agent 类型（加载 `.astro/agents` 定义） |
| `model` | Option\<String\> | 指定模型 |
| `reasoning_effort` | Option\<String\> | 推理强度 |
| `fork_turns` | Option\<String\> | 上下文快照策略：`none`（默认）/ `all` / 数字 N |

执行流程：

1. 解析 `agent_type`，从 `AgentCatalog` 加载自定义定义
2. `AgentControl` 检查 `CloseAdmissionGuard`（是否有正在关闭的前缀）
3. `AgentControl::reserve_spawn_typed()` 预留槽位并持久化 `ThreadReservation`
4. 构建 `SpawnRuntimeV2Request`，持久化非机密 `AgentRuntimeDescriptorV2`
5. 按 `fork_turns` 创建子 Session，校验运行时材料，登记可恢复请求
6. `AgentSpawnReservation::commit()` 提交图身份
7. `AgentRuntimeManager::start_turn()` 异步启动，等待 `TurnStarted` 和运行时句柄完成准入确认
8. 返回 `SpawnAgentV2Result { thread }`

### 7.3 list_agents

| 字段 | 类型 | 说明 |
|------|------|------|
| `path_prefix` | Option\<String\> | 可选路径前缀过滤 |

返回 `AgentTreeSnapshotV2`：包含根线程 ID、所有线程列表、活动序列号和 nullable
`root_service_tier`。持久层只负责图与状态，`AgentControl` 在快照返回前注入当前
根运行时的 service tier；旧后端缺少该字段时 Desktop 兼容为 `null`。

### 7.4 send_message

| 字段 | 类型 | 说明 |
|------|------|------|
| `target` | String（必填） | 目标线程的 AgentPath（如 `/root/research`） |
| `message` | String（必填） | 消息内容 |

投递消息到目标线程邮箱，返回 `MessageAgentV2Result { message_id, queued, turn_triggered }`。

### 7.5 followup_task

与 `send_message` 类似，但语义不同：`followup_task` 持久化一条会触发 turn 的邮箱消息。目标无活跃 turn 时立即启动；目标正在运行时原子排队，由当前 turn 交接给下一 turn；冷恢复时先从 runtime descriptor 和父运行时材料重建请求。

### 7.6 wait_agent

| 字段 | 类型 | 说明 |
|------|------|------|
| `timeout_ms` | Option\<i64\> | 超时时间（毫秒），默认 30000，范围 10000-3600000 |

等待子 Agent 邮箱活动或被转向，返回 `WaitAgentV2Result { message, timed_out }`。

`WaitOutcome` 三种结果：`MailboxActivity`、`Steered`、`TimedOut`。

### 7.7 interrupt_agent

| 字段 | 类型 | 说明 |
|------|------|------|
| `target` | String（必填） | 目标线程的 AgentPath |

通过 `RuntimeHandleRegistry` 查找目标线程的句柄，调用 `interrupt` 回调。返回 `InterruptAgentV2Result { thread, previous_status }`。

---

## 8. 桌面控制面

### 8.1 Tauri Commands

```rust
// apps/desktop/src-tauri/src/commands/subagents.rs

#[tauri::command]
pub async fn list_subagent_threads(args: RootSessionArgs) -> Result<AgentTreeSnapshotV2, String>;

#[tauri::command]
pub async fn read_subagent_thread(args: TargetArgs) -> Result<AgentThreadDetailV2, String>;

#[tauri::command]
pub async fn send_subagent_message(args: FollowupArgs) -> Result<AgentThreadV2, String>;

#[tauri::command]
pub async fn interrupt_subagent_thread(args: TargetArgs) -> Result<InterruptAgentV2Result, String>;

#[tauri::command]
pub async fn close_subagent_thread(args: TargetArgs) -> Result<AgentTreeSnapshotV2, String>;

#[tauri::command]
pub async fn list_subagent_definitions() -> Result<Vec<AgentDefinitionDto>, String>;
```

### 8.2 read_subagent_thread

返回 `AgentThreadDetailV2`，包含线程元数据（`AgentThreadV2`）和完整的 Session 时间线（`Vec<AgentThreadMessageV2>`）。每条 `AgentThreadMessageV2` 携带 `item: agent_protocol::ResponseItem`（原生 Responses 协议项）、`timestamp`、`token_count` 和 `finish_reason`。

### 8.3 close_subagent_thread

级联关闭指定路径下的整个子树：

1. `CloseAdmissionGuard` 阻止新派生进入该前缀
2. 等待所有进行中的派生完成
3. 依次终止活跃线程（通过 `RuntimeHandleRegistry`）
4. 更新 `AgentGraphStore` 中的状态
5. 返回更新后的树快照

### 8.4 前端集成

桌面前端通过上述 Tauri Commands 维护子 Agent 面板，展示：

- 线程树结构和状态
- 根任务实际生效的 service tier（存在时）
- 每个线程的消息时间线
- 追问和中断操作
- 自定义 Agent 定义列表

---

## 9. 子 Agent 执行流程

### 9.1 完整序列

```text
父 AgentLoop
  │
  ├─[1] spawn_agent
  ▼
AgentControl / AgentRegistry
  ├─[2] close admission + quota check
  ├─[3] persist ThreadReservation + runtime descriptor
  ├─[4] fork child Session + validate runtime material
  ├─[5] commit Agent Graph identity
  ▼
AgentRuntimeManager
  ├─[6] start child tokio task
  ├─[7] persist TurnStarted + register runtime handle
  └─[8] acknowledge startup
  ▼
父 AgentLoop receives SpawnAgentV2Result
  │
  ├─[9] wait_agent
  ▼
ActivityBus / AgentGraphStore
  └─[10] mailbox activity / terminal status / steer / timeout
```

### 9.2 关键步骤说明

1. **[1-3] 预检与预留**：`AgentControl` 检查 close 准入屏障和注册表限制，持久化线程预留和非机密 runtime descriptor。
2. **[4] 运行时物料与 Session**：按 `fork_turns` 创建子 Session，并校验 Provider fallback、MCP、Skills、sandbox 等物料。
3. **[5] 提交身份**：启动前提交 `AgentSpawnReservation`；启动失败时 cleanup guard 同时回滚 Agent Graph 身份和子 Session。
4. **[6-8] 启动准入**：`AgentRuntimeManager` 启动独立 tokio task，只有 `TurnStarted` 已持久化且运行时句柄已注册后，才向父 Agent 返回成功。
5. **[9-10] 等待与事件投影**：后续 `RunnerEvent` 持久化到 `AgentGraphStore`；父 Agent 通过 `wait_agent` 等待对其可见的 mailbox、子线程终态、steer 或超时。

---

## 10. 错误处理

### 10.1 失败模式与处理策略

| 失败模式 | 触发条件 | 处理策略 | 父 Agent 感知 |
|---------|---------|---------|-------------|
| 线程数超限 | `AgentRegistry.max_threads` 已满 | 立即拒绝 | 工具错误 |
| 深度超限 | 路径深度 > `Limits.max_depth` | 立即拒绝 | 工具错误 |
| 并发超限 | 活跃执行数 > `Limits.max_running` | 立即拒绝 | 工具错误 |
| 路径冲突 | 目标路径已被占用或预留 | 立即拒绝 | 工具错误 |
| 关闭屏障 | 目标前缀正在被 `close_subagent_thread` 关闭 | 立即拒绝 | 工具错误 |
| 执行错误 | 子 Agent round_loop 失败 | `TurnErrored` → `Errored` 状态 | wait_agent 返回错误信息 |
| 中断 | `interrupt_agent` 调用 | `TurnInterrupted` → `Interrupted` 状态 | wait_agent 感知 |
| 运行时终止 | 进程退出或 panic | `RuntimeTerminated` → `Shutdown` | wait_agent 感知 |
| 目标未找到 | 路径不存在于注册表 | 工具错误 | 工具错误 |
| 遗留描述符 | 线程在引入运行时描述符前创建 | `LegacyRuntimeDescriptorUnavailable` | 桌面提示重建 |

### 10.2 错误传播规则

子 Agent 的错误不会导致父 Agent 崩溃。所有子 Agent 错误都封装为工具结果，由父 Agent 的 LLM 决定如何处理。桌面控制面错误经过 `command_error()` 过滤，不反射敏感信息（API 密钥、用户数据等）。

---

## 11. 与 Skill 编排的关系

自定义 Agent 定义（`.astro/agents/*.toml`）可在 `skills` 层声明启用的 Skill：

```toml
# .astro/agents/researcher.toml
name = "researcher"
description = "深度研究 Agent"
developer_instructions = "..."

[skills]
config = [
  { path = "deep-research", enabled = true },
  { path = "browser-fetch", enabled = true }
]
```

`SpawnRuntimeV2Request` 将 Skill 配置传递给子 Agent 运行时，子 Agent 在启动时加载指定的 Skill。

---

## 12. Tauri 集成

### 12.1 前端事件

子 Agent 系统通过 `ActivityBus` 和 `RunnerEvent` 驱动前端状态更新。桌面通过以下 Tauri Commands 查询状态：

| 命令 | 用途 | 返回类型 |
|------|------|---------|
| `list_subagent_threads` | 查询线程树快照 | `AgentTreeSnapshotV2` |
| `read_subagent_thread` | 读取线程详情和消息时间线 | `AgentThreadDetailV2` |
| `send_subagent_message` | 追问子线程 | `AgentThreadV2` |
| `interrupt_subagent_thread` | 中断子线程 | `InterruptAgentV2Result` |
| `close_subagent_thread` | 关闭子树 | `AgentTreeSnapshotV2` |
| `list_subagent_definitions` | 列出可用 Agent 定义 | `Vec<AgentDefinitionDto>` |

### 12.2 参数格式

```typescript
// Rust 侧使用 #[serde(rename_all = "camelCase", deny_unknown_fields)]

interface RootSessionArgs {
  rootSessionId: string;
}

interface TargetArgs {
  rootSessionId: string;
  target: string;  // AgentPath, e.g. "/root/research"
}

interface FollowupArgs {
  rootSessionId: string;
  target: string;
  message: string;
}
```

所有参数结构使用 `deny_unknown_fields` 和 `rename_all = "camelCase"`，旧格式（`parentSessionId`、`threadId`、`includeClosed`）被拒绝。

---

## 13. 设计约束

### 13.1 资源限制

| 约束 | 控制方 | 说明 |
|------|--------|------|
| 最大线程数 | `Limits.max_threads` | 整棵树的线程总数上限 |
| 最大树深度 | `Limits.max_depth` | AgentPath 路径段数上限 |
| 最大并发执行 | `Limits.max_running` | 同时处于 Running 状态的线程数 |
| 关闭屏障 | `CloseAdmissionGuard` | 关闭操作期间阻止新派生进入该前缀 |
| RAII 预留 | `SpawnReservation` | 异常时自动释放槽位 |

### 13.2 安全不变量

```text
INVARIANT 1: child.permissions ⊆ parent.permissions
             子 Agent 权限永远不超出父 Agent

INVARIANT 2: thread.canonical_path 在 AgentRegistry 中唯一
             不允许路径冲突

INVARIANT 3: thread.depth <= Limits.max_depth
             树深度有界

INVARIANT 4: running_count <= Limits.max_running
             并发执行有界

INVARIANT 5: SpawnReservation 的 commit/abort/drop 保证槽位释放
             不泄漏注册表资源
```

### 13.3 配置约定

- Agent 类型定义仅从 `.astro/agents` 加载，`.codex/agents` 不生效
- 全局配置：`~/.astro/agents/*.toml`
- 项目配置：`<project>/.astro/agents/*.toml`（可信后加载覆盖）
- 启用/禁用开关：`.astro/config.toml` 中的 `[agents]` 段

---

## 14. 相关文档

- [Subagent 系统设计](../../03-系统设计阶段/02-核心功能模块/05-Subagent系统设计.md) -- 系统边界、控制面与生命周期概览
- [01-agent-core详细设计.md](01-agent-core详细设计.md) -- AgentLoop 运行时、工具分发
- [12-Agent事件与恢复详细设计.md](12-Agent事件与恢复详细设计.md) -- EventMsg 单一事实链、恢复协议
