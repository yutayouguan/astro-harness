# Subagent（Codex V2 Agent Threads）系统设计

> **术语与 Harness 定位（2026-09-07）**：本文统一使用 **Subagent** 表示产品能力，使用 **Agent Thread** 表示其运行时抽象。Subagent 是 Harness 的 Orchestrator 子系统。当前唯一实现是 Codex V2 Agent Threads：`spawn_agent/list_agents/send_message/followup_task/wait_agent/interrupt_agent`，Graph/mailbox/status 位于 `{base}/data/subagents-v2.db`，真实对话位于 `{base}/data/state.db`。旧 `delegate_task/Supervisor/DelegateRunner` 仅作迁移背景。

> 文档状态：定稿 | 阶段：系统设计 | 对应需求：F-30 Subagent
> 架构版本：Codex V2 Agent Thread（替代原 Supervisor/DelegateRunner 设计）

---

## 一、系统边界

Subagent 是持久的父子 Agent Thread 协作机制，不是 Skill 的别名，也不是桌面端显式创建 git worktree 的多任务功能。它的责任边界是：

- 建立并持久化 Agent Tree、mailbox 与状态转移；
- 为每个子线程创建独立 Session，并按 `fork_turns` 复制父历史；
- 在父权限上限内解析子 Agent 的模型、推理强度、Skill 和 sandbox 配置；
- 通过六个模型工具和桌面控制面支持派生、通信、追问、等待、中断与关闭。

## 二、设计哲学

子 Agent 是一个独立的 **Agent Thread**，拥有自己的会话上下文、模型调用、工具循环和持久化状态。父 Agent 通过 `spawn_agent` 派生线程，通过 `send_message` / `followup_task` 追加交互，通过 `wait_agent` / `interrupt_agent` 管理生命周期。子 Agent 只把精简摘要回传给父 Agent，不把中间日志灌入父上下文，以此保持 Token 高效。

与 V1 设计（`DelegateRunner` / `Supervisor` / `spawn_depth`）的核心区别：

- **线程而非一次性委派**：子 Agent 是可追问、可恢复的持久线程，不是一次性同步调用
- **显式路径寻址**：每个子 Agent 有 `AgentPath`（如 `/root/research`），父 Agent 通过路径名定位子线程
- **不再有嵌套深度计数**：`AgentRegistry` 通过 `Limits.max_depth` 控制树深度，不使用 `AgentContext.depth` 递增模式
- **配置从 `.astro/agents` 加载**：自定义 Agent 类型定义在可信项目
  `.astro/agents/*.toml` 或全局 `~/.astro/agents/*.toml`；`.codex` 不生效

---

## 三、模型工具接口

V2 提供 **6 个模型工具**（LLM 可直接调用）和 **6 个桌面控制面命令**：

### 模型工具（crate: `agent-tools`）

| 工具名 | 用途 |
|--------|------|
| `spawn_agent` | 在当前路径下派生子 Agent Thread |
| `list_agents` | 列出活跃的子 Agent 树快照 |
| `send_message` | 向目标线程的邮箱投递消息 |
| `followup_task` | 向已完成的线程追加后续任务（触发新 turn） |
| `wait_agent` | 等待子 Agent 邮箱活动或完成 |
| `interrupt_agent` | 中断目标线程的当前执行 |

### 桌面控制面（Tauri commands）

| 命令 | 用途 |
|------|------|
| `list_subagent_threads` | 读取根 Session 对应的 Agent Tree 快照 |
| `read_subagent_thread` | 读取子 Agent 线程的完整 Session 时间线 |
| `send_subagent_message` | 以 follow-up 语义追加消息并触发或排队新 turn |
| `interrupt_subagent_thread` | 中断子 Agent 的当前 turn |
| `close_subagent_thread` | 关闭子 Agent 子树（级联终止） |
| `list_subagent_definitions` | 列出当前可用的自定义 Agent 定义 |

### spawn_agent 请求结构

```rust
pub struct SpawnAgentV2Request {
    pub task_name: String,              // 子任务名称（同时作为路径段）
    pub message: String,                // 初始任务描述
    pub agent_type: Option<String>,     // 可选：自定义 Agent 类型（加载 .astro/agents 定义）
    pub model: Option<String>,          // 可选：指定模型
    pub reasoning_effort: Option<String>, // 可选：推理强度
    pub fork_turns: Option<String>,     // 上下文快照策略：none | all | N
}
```

`fork_turns` 显式控制初始上下文快照，不再强制"完全隔离且不可派生后追问"。

---

## 四、AgentControl 根级控制器

```rust
pub struct AgentControl {
    root_thread_id: String,
    store: AgentGraphStore,          // SQLite 持久化（subagents-v2.db）
    registry: Arc<AgentRegistry>,    // RAII 内存注册表
    activity: Arc<ActivityBus>,      // 活动事件总线
    runtimes: Arc<RuntimeHandleRegistry>, // 运行时句柄（中断/终止回调）
    runtime_lifecycle: Arc<Mutex<RuntimeLifecycleState>>, // close/spawn 准入
    root_service_tier: Arc<Mutex<Option<String>>>,        // 根任务服务层级
    lifecycle_notify: Arc<Notify>,                        // 生命周期协调
}
```

`AgentControl` 是根级共享控制器，聚合存储、注册表、活动总线、运行时句柄、close/spawn 准入与根服务层级。每个根 Agent 对应一个 `AgentControl` 实例。

---

## 五、线程状态机

```text
PendingInit → Running → Completed
                ├────→ Interrupted
                └────→ Errored

Completed / Interrupted / Errored ──followup_task──→ Running
PendingInit / Running / 终态 ──RuntimeTerminated──→ Shutdown
```

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

---

## 六、派生规则

- 子 Agent 默认共享当前项目工作区，不隐式创建 git worktree
- 并行写任务必须由父 Agent 规划文件所有权
- `AgentRegistry` 通过 `Limits` 控制：`max_threads`（最大线程数）、`max_depth`（最大树深度）、`max_running`（最大并发执行数）
- 超出限制时返回工具错误，不静默截断
- 子 Agent 继承父任务已解析的权限上限，自定义 Agent 只能收紧不能扩权
- `AgentSpawnReservation` 提供 RAII 预留：在 commit/abort/drop 前对桌面递归关闭可见

---

## 七、运行方式与权限解耦

| 维度 | 可选值 | 职责 |
| --- | --- | --- |
| Agent 线程 | primary / spawned | 上下文与生命周期隔离 |
| 运行适配器 | foreground / background | 是否输出 UI 流、时间线和交互事件 |
| 权限策略 | read-only / workspace-write / danger-full-access | 文件、终端和网络的可用边界 |
| 审批策略 | user / auto-review / never | 风险操作的决策方式 |

`danger-full-access` 不等于 background，background 也不等于无权限约束。

---

## 八、Subagent 生命周期

```text
父 Agent 调用 spawn_agent
        │
        ▼
AgentControl::reserve_spawn()
  ├─ AgentRegistry 检查 Limits（线程数、深度、并发）
  ├─ AgentGraphStore 持久化 ThreadReservation
  └─ 返回 AgentSpawnReservation（RAII）
        │
        ▼
子 Agent 运行时启动
  ├─ 注册 AgentRuntimeHandle（中断/终止回调）
  ├─ 状态 PendingInit → Running
  └─ 执行 round_loop（共享统一 round engine）
        │
        ▼
RunnerEvent 驱动状态投影
  ├─ TurnStarted → 保持 Running
  ├─ TurnCompleted → Completed { last_message }
  ├─ TurnInterrupted → Interrupted
  ├─ TurnErrored → Errored { message }
  └─ RuntimeTerminated → Shutdown
        │
        ▼
父 Agent 通过 wait_agent 感知完成
  └─ 可通过 send_message / followup_task 追加交互
```

---

## 九、统一运行内核

foreground 与 background 共享同一个多轮 round engine：Provider fallback、context maintenance、hooks、tool execution、iteration budget、usage 和 cancellation 只实现一次。差异仅由适配器表达：

```text
AgentThread
  └─ UnifiedRoundEngine
       ├─ ForegroundAdapter  -> token/timeline/HITL UI
       └─ BackgroundAdapter  -> summary/status/non-interactive result
```

Cron 和 Subagent 使用 `BackgroundAdapter`；普通聊天使用 `ForegroundAdapter`。

---

## 十、持久化与恢复

`AgentGraphStore`（`subagents-v2.db`，WAL 模式 SQLite）管理：

- 线程元数据（`AgentThreadV2`）
- 状态事件序列（`StoredStatusEvent`）
- 邮箱消息（`MailboxMessage`）
- 运行时描述符（`AgentRuntimeDescriptorV2`）

连接池统一由 `agent-db::AstroDb::open_pool_at_path` 创建，保留调用者指定的
数据库路径和最多 4 条连接。WAL 与空库的 incremental auto-vacuum 由共享层按
规范路径在进程内成功初始化一次；不得在每条连接的选项中重复设置文件级 PRAGMA，
也不得靠预建连接或提高 slow-acquire 告警阈值规避写锁竞争。连接扩容、重建及
同文件的新池应能在已有写事务期间读取 WAL 快照；实际写事务仍遵守 SQLite 单写者约束。

每个子 Agent 的 Session 消息存储在独立的 `SessionStore`（`state.db`）中，通过 `session_id` 关联。

---

## 十一、`spawn_agent` vs `execute_code` 决策原则

| 场景 | 选择 | 原因 |
| ---- | ---- | ---- |
| 需要推理、多步规划、调用 LLM | `spawn_agent` | 灵活但 Token 较高 |
| 机械性数据处理、脚本执行 | `execute_code` | Token 极低，速度快 |

---

## 相关文档

- [03-MCP集成.md](03-MCP集成.md) — MCP 集成设计
- [04-Skills系统.md](04-Skills系统.md) — Skills 系统设计
- [Subagent 详细设计](../../04-详细设计阶段/01-核心引擎层/06-Subagent详细设计.md) — V2 Agent Thread 代码级契约
