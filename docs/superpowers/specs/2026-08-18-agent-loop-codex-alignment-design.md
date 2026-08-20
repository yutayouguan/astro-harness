# Agent Loop Codex 架构对齐设计

**日期:** 2026-08-18

**状态:** 已实现

**范围:** Agent Core、rollout、app-server、gRPC/Tauri/exec 事件映射

**参考基线:** OpenAI Codex `codex-rs`，提交 `ede5247893a50297a47c9aa5038e6ab28312ff50`（2026-08-18）

## 1. 背景

Astro 原有 `MultiTurnStreamItem`、Core `EventBus` 和 `SessionEventHub` 三套事实路径，无法同时
保证统一生命周期、多订阅者、断线恢复和后台副作用。迁移完成后，运行时只保留以下事实链：

```text
AstroThread::submit(Op)
  → bounded(512)
  → Session::submission_loop
  → SessionTask::run_turn
  → EventMsg
  → rollout policy + append
  → Core event queue
  → one Server listener
  → ThreadHistoryBuilder
  → connection queues bounded(128)
  → Tauri / exec / compatibility Chat
```

`SessionEventHub`、Core `EventBus`、Core `MultiTurnStreamItem` 和 cursor replay 已从运行时删除。
兼容类型只能存在于协议/桌面映射边界，不能成为第二个 emitter、history 或恢复事实源。

## 2. 目标与非目标

### 2.1 目标

1. 每个 Thread 只有一个长期存活的 `Session` actor 和一条有序提交队列。
2. 前台、后台、工具、审批、MCP、Hook、Subagent 和 Compaction 使用同一 `EventMsg` 协议。
3. rollout 成为线程历史和恢复的权威事实源；SQLite 退化为查询投影。
4. Core 保持单一顺序事件流；Server 负责协议映射和多订阅者 fan-out。
5. 断线恢复采用“持久历史 + 活动 Turn 快照 + 实时流”，不承诺补发瞬时 delta。
6. 慢消费者只能影响自己的连接，不能阻塞 Session 或其他消费者。
7. 保留 Astro 的多 Provider、工具、记忆、Cron、工作流和桌面能力。

### 2.2 非目标

- 不重写 Provider、工具执行器或 HITL 的业务逻辑。
- 不要求 Astro 暴露 Codex 品牌命名；对外仍使用 Astro 协议名称。
- 不把每一个 token delta 持久化。
- 不使用 `event_id / stream_id / after_event_id` 实现瞬时事件的精确重放。
- 不在本阶段改变 Subagent 的权限继承或持久化模型。
- 不让 gRPC、Tauri 或 exec 直接读取 Core 内部队列。

## 3. 核心决策

| 项目 | 决策 |
|---|---|
| Thread 句柄 | 新增稳定的 `AstroThread`，内部持有 `Arc<Session>` 与 `SessionIo` |
| 提交队列 | `async_channel::bounded(512)`，满时对提交方施加背压 |
| Core 事件队列 | 单一 `async_channel::unbounded()`，保持 Session 内产生顺序 |
| 执行所有权 | 长期存活的 `Session::submission_loop` 分发 `Op` |
| 运行任务 | 复用现有 `SessionTask`、`ActiveTurn`、`TurnContext`、`StepContext` |
| 事件协议 | `Event { id, msg: EventMsg }` + `TurnItem` |
| 持久化 | JSONL rollout 为权威源；SQLite 为可重建投影 |
| 多订阅者 | Core 单 receiver，app-server 单 listener，多连接 fan-out |
| 连接队列 | 每连接有界队列 128；`try_send` 失败即断开慢连接 |
| 恢复 | `thread/resume` 返回持久历史与活动 Turn 快照，再继续实时流 |
| 空闲卸载 | 无订阅且非活动持续 30 分钟后，提交 `Shutdown` 并 flush rollout |
| 兼容 | Chat/Tauri 边界从 ThreadEvent 单向映射；不存在兼容事件源或 SessionEvents 订阅链 |

## 4. Core 运行架构

### 4.1 `AstroThread` 与 `SessionIo`

`AstroThread` 是 Server 持有并供 Tauri、exec 和 Chat 边界间接驱动的稳定线程句柄：

```rust
pub struct AstroThread {
    session: Arc<Session>,
    io: SessionIo,
}

pub struct SessionIo {
    tx_sub: async_channel::Sender<Submission>,
    rx_event: async_channel::Receiver<Event>,
    status_rx: watch::Receiver<AgentStatus>,
    termination: Shared<BoxFuture<'static, ()>>,
}
```

公开能力只包含：

- `submit(op) -> Result<submission_id, SubmitError>`；
- `next_event() -> Result<Event, RecvError>`；
- `status()` / `subscribe_status()`；
- `termination()`；
- `flush_rollout()`。

调用方不能绕过 `submit` 直接调用 `run_turn`，也不能直接持有事件 sender。

### 4.2 `Submission` 与 `Op`

```rust
pub struct Submission {
    pub id: String,
    pub op: Op,
}
```

`Op` 至少覆盖：

- reply-bearing `TurnInput { request, mode, reply }`、`Interrupt`、`Shutdown`；
- `ThreadSettings`、`Compact`、`ThreadRollback`；
- exec / patch 审批结果；
- 用户问题回答、权限结果、动态工具结果；
- MCP refresh / elicitation；
- Subagent / inter-agent communication；
- review、后台唤醒和其他 Session 控制操作。

提交队列容量固定为 512。`submit` 在队列满时等待容量，不丢弃 Op；队列关闭时返回明确错误。提交成功只表示 Session 已接收操作，不表示 Turn 已完成。

`TurnInput` 的 mode 对齐 Codex：

- `StartOrSteer`：优先把输入注入可 steering 的活动 Turn；没有活动 Turn 时才启动新 Turn；
- `StartIfIdle`：仅在线程空闲时启动，否则返回 `NotSubmitted`；
- `Steer { expected_turn_id }`：只允许注入指定活动 Turn，不匹配则返回 `NotSubmitted`。

reply 在 Session 完成“started / steered / not submitted”判定后返回，不等待模型采样或整个 Turn 完成。

### 4.3 `submission_loop`

Session 创建时启动唯一、长期存活的 `submission_loop`：

```text
while let Ok(submission) = rx_sub.recv().await:
    match submission.op:
        TurnInput       → start / steer / reject → reply
        Interrupt       → abort active task
        ApprovalResult  → 唤醒对应 call_id
        ThreadSettings  → 更新 Session 配置并发事件
        Compact         → spawn CompactTask
        Shutdown        → 停止接收、终止任务、flush、退出
```

所有控制操作与 Turn 输入共享同一顺序边界。审批响应、打断和配置更新不再通过旁路直接修改 Session。

### 4.4 Turn 与 Step

现有 `SessionTask`、`RunningTask`、`ActiveTurn`、`TurnContext`、`StepContext` 保留并成为正式执行模型：

- `TurnContext` 在一次 Turn 内固定模型、权限、工作区、追踪和预算配置；
- `StepContext` 描述一次采样或工具步骤实际使用的上下文；
- `RegularTask::run` 调用真正的 `run_turn` 循环；
- 普通 `TurnInput` 不隐式替换活动 Turn，而是依据 mode steering 或拒绝；
- 显式启动会替换 SessionTask 的控制操作，才以 `Replaced` 终止旧任务；
- Session task 生命周期层是唯一 Turn 终止事件生成边界：完成/失败由 `on_task_finished` 发出，显式 abort 由 `abort_turn_if_active` / `abort_all_tasks` 发出。

## 5. 统一事件协议

### 5.1 Envelope

```rust
pub struct Event {
    pub id: String,
    pub msg: EventMsg,
}
```

`id` 是事件所属的 submission / turn correlation id，不承担断线 replay cursor 职责。Thread 内事件由 Session 串行生成，顺序与 Core receiver 观察顺序一致。

### 5.2 生命周期事件

必须具备以下稳定事件：

- `TurnStarted`；
- `ItemStarted`；
- `ItemCompleted`；
- `Error`、`Warning`、`StreamError`；
- `TurnComplete`；
- `TurnAborted`；
- `ThreadSettingsApplied`、`ThreadRolledBack`；
- `TokenCount`、`ContextCompacted`；
- `ShutdownComplete`。

`Done` 与 `RunFinished` 不再作为独立终止协议。

实时控制与进度事件使用同一个 `EventMsg` 枚举，但按职责分组：

| 类别 | 代表事件 |
|---|---|
| 消息与推理 | `AgentMessageContentDelta`、`PlanDelta`、`ReasoningContentDelta` |
| 命令与补丁 | `ExecCommandBegin/OutputDelta/End`、`PatchApplyBegin/Updated/End` |
| 审批与人工输入 | `ExecApprovalRequest`、`ApplyPatchApprovalRequest`、`RequestPermissions`、`RequestUserInput` |
| MCP 与动态工具 | `McpToolCallBegin/End`、`DynamicToolCallRequest/Response`、`ElicitationRequest` |
| Hook | `HookStarted`、`HookCompleted` |
| Subagent | collab spawn / interaction / wait / close / resume begin/end、`SubAgentActivity` |
| 上下文与线程 | `ContextCompacted`、`TokenCount`、`ThreadSettingsApplied`、`ThreadRolledBack` |

这些事件由 Core 产生；app-server 只负责状态归并与外部协议映射，不能重新推断工具或 Turn 生命周期。

### 5.3 `TurnItem` 粒度

统一使用 Codex 形态的 `TurnItem` 表达可展示、可恢复的工作单元：

- `UserMessage`、`HookPrompt`、`AgentMessage`、`Plan`、`Reasoning`；
- `CommandExecution`、`DynamicToolCall`、`McpToolCall`；
- `CollabAgentToolCall`、`SubAgentActivity`；
- `WebSearch`、`ImageView`、`ImageGeneration`；
- `FileChange`、`ContextCompaction`；
- `EnteredReviewMode`、`ExitedReviewMode`；
- `Extension`。

Astro 特有的记忆 review、标题更新、媒体任务等优先通过 namespaced `Extension` item 表达，避免创建第二套顶层事件系统。

### 5.4 增量事件

token、reasoning、plan、exec stdout、patch 更新和图像生成进度仍使用细粒度 delta/begin/update 事件。它们用于实时体验，但通常不进入 rollout。最终状态由 `ItemCompleted` 或其他稳定完成事件收敛。

### 5.5 终止不变量

每个 `TurnStarted` 必须对应且只能对应一个终止事件：

| 结果 | 事件序列 |
|---|---|
| 成功 | `TurnComplete { error: None }` |
| 非中断错误 | `Error` → `TurnComplete { error: Some(error) }` |
| 用户打断 | `TurnAborted { reason: Interrupted }` |
| 被新 Turn 替换 | `TurnAborted { reason: Replaced }` |
| review 结束 | `TurnAborted { reason: ReviewEnded }` |
| 预算耗尽 | `TurnAborted { reason: BudgetLimited }` |

`Error` 不能代替终止事件。前台与后台路径都必须满足该不变量。

## 6. Rollout 与恢复事实源

### 6.1 文件与记录类型

每个 Thread 使用一条 append-only JSONL rollout，建议路径：

```text
~/.astro/sessions/rollouts/YYYY/MM/DD/rollout-{timestamp}-{thread_id}.jsonl
```

记录类型包括：

- `SessionMeta`；
- `ResponseItem`；
- `EventMsg`；
- `TurnContext` / `WorldState`；
- `Compacted`；
- inter-agent communication 与必要元数据。

### 6.2 写入顺序

所有 Core 事件统一经过：

```text
Session::send_event
  → rollout policy 判断是否持久化
  → persist_rollout_items(...).await
  → deliver_event_raw(event)
```

可持久事件在提交给 rollout writer 后才进入 Core 事件队列。rollout writer 保持调用顺序；`flush_rollout` 是关闭、卸载和显式同步时的持久化屏障。

若单次持久化失败：

- 记录错误，但仍投递实时事件，避免卡死 Agent；
- 失败事件不能被宣称为已持久化；
- `flush` 只保证已成功进入 writer 的记录落盘，不凭空恢复此前被拒绝的 append；
- 恢复结果以实际 rollout 内容为准。

这与 Codex 的“持久化失败不阻断实时交付”语义一致。

### 6.3 Persistence policy

新 Thread 默认使用 `Paginated` history mode；旧 Astro Thread 迁移期标记为 `Legacy`。

| 类型 | Paginated | Legacy |
|---|---:|---:|
| 可持久 `ResponseItem` | 是 | 是 |
| Session/Turn/WorldState/Compaction marker | 是 | 是 |
| `TurnStarted` / `TurnComplete` / `TurnAborted` | 是 | 是 |
| `ThreadSettingsApplied` / rollback / goal / token count | 是 | 是 |
| `ItemCompleted` | 是 | 仅无等价 raw item 的特殊项 |
| legacy message/tool completion events | 否 | 是 |
| `ItemStarted` / begin / delta / stdout / approval request | 否 | 否 |
| `Error` / `Warning` / `StreamError` | 否 | 否 |

具体枚举必须集中在单一 `rollout::policy` 模块，禁止 emitter 自行决定是否落盘。

### 6.4 SQLite 投影

`SessionStore` 继续提供消息查询、FTS、billing 和 UI 列表，但不再是运行时恢复的唯一事实源：

- 新运行数据先生成 `RolloutItem`；
- `rebuild_messages_from_rollout` 可在单事务内重建指定 session 的 SQLite messages；
- 投影失败不修改 rollout；
- SQLite 可从 rollout 重建；
- 迁移期允许兼容读旧消息，但禁止长期维持两套独立写入语义。

“assistant 含 tool_calls 的记录先于工具执行”的现有不变量同时由 SQLite 记录顺序和相应
`ResponseItem` rollout 顺序保证。

## 7. app-server、多订阅者与背压

### 7.1 单 listener

Core 不广播给多个消费者。app-server 为每个加载的 Thread 维护唯一 listener：

```text
AstroThread::next_event()
  → ThreadHistoryBuilder::track(event)
  → EventMsg → app-server typed notification 映射
  → 查询 subscribed_connection_ids
  → 向各连接有界队列 try_send
```

`ThreadHistoryBuilder` 在发送通知前更新活动 Turn 状态，保证 resume 快照与已观察事件一致。

### 7.2 订阅关系

`ThreadStateManager` 维护：

- `thread_id → ThreadState`；
- `thread_id → HashSet<ConnectionId>`；
- 当前活动 Turn 的 `ThreadHistoryBuilder`；
- listener command queue；
- 是否存在订阅者的 watch 状态。

`SubmitTurn`/`ResumeThread` 把当前连接订阅到目标 Thread；`UnsubscribeThread` 显式移除。一个连接
可订阅多个 Thread，一个 Thread 可被 Tauri、exec 和兼容 Chat 等多个连接同时订阅。

### 7.3 慢消费者

每个传输连接使用容量 128 的独立 outbound queue：

- fan-out 使用 `try_send`，禁止 await 某个消费者腾出空间；
- 队列满时将该连接标记为 `slow_consumer` 并断开；
- 其他连接继续接收同一 Event；
- Session、rollout writer 和 Thread listener 不等待慢连接；
- 客户端断开后通过 `thread/resume` 恢复稳定状态。

不得使用“覆盖最旧事件”或静默丢弃来隐藏 lag。

## 8. 断线恢复

### 8.1 Resume 模型

恢复使用 snapshot + live stream，而不是 transient event replay：

1. 定位或重新加载 Thread rollout；
2. 按 history mode 重建稳定 Turn/Item 历史；
3. 对已加载 Thread，从 listener 持有的 `ThreadHistoryBuilder` 取得活动 Turn 快照；
4. 合并持久历史和活动 Turn，生成 `ThreadResumeResponse`；
5. 把连接加入 Thread 订阅集合；
6. 返回 response，随后继续实时通知。

对于正在运行的 Thread，快照生成、连接加入和 resume response 必须通过 listener command 串行化。这样不会出现“快照已生成、订阅尚未生效”的事件空窗。

对于冷恢复，先创建 Session 和 listener，再返回由 rollout 重建的快照；冷恢复时不存在旧进程中的活动 Turn。

### 8.2 恢复边界

- 持久化的 Turn、Item、ResponseItem 和配置可恢复；
- 活动 Turn 使用内存 snapshot 表达当前状态；
- token delta、reasoning delta、stdout 等瞬时事件不补发；
- 未完成 Item 最终通过新的 live event 或终止状态收敛；
- 活动审批和 server request 重新发送给新连接；
- `event_id / stream_id / after_event_id` cursor replay 已删除；恢复不接受瞬时事件游标；
- `include_turns` 控制 Resume 是否返回 completed turns；当前协议不提供 event/history replay
  cursor。

## 9. 前台、后台与客户端映射

### 9.1 前后台统一

前台 Chat、Cron/headless 和 Subagent 都由同一 `SessionTask` 生命周期产生 `EventMsg`。终态后的
memory review、title 和 global pending 则提交 `Op::EmitExtension`，继续进入相同的
rollout-before-live 事实链，而不是创建后台广播总线。命名空间契约为：

- `astro.memory`: `{ source, target, summary, live_written }`；
- `astro.pending`: `{ pending_count, reason }`，归属固定 workspace thread
  `astro-workspace-events`；
- `astro.session_metadata`: `{ title }`；
- `astro.background_complete`: 正常 payload 为 `{}`，turn id 由事件 envelope 承载；sink 超时时为
  `{ turn_id, expired: true }`。

Server 在成功终态保留按 logical connection id 归属的 background extension sink；同 id 新代连接
可接收迟到 extension，不同连接隔离。side-effect supervisor 有总超时，成功、错误、超时和取消
最终都会完成 marker 或直接 expire 原 sink，且不会通过 `get_or_create` 复活已 release 的线程。
后台收集器不得忽略：

- `ItemStarted` / `ItemCompleted`；
- 工具和 MCP 生命周期；
- activity / context usage；
- `TurnComplete` / `TurnAborted`；
- Extension item。

没有订阅者时，Server listener 仍持续消费 Core 事件并更新状态。Thread 只有在“无订阅者、无
background sink 且非活动”持续 30 分钟后才能卸载。

### 9.2 Server / Tauri / exec

- app-server 是 Core EventMsg 到外部 typed notifications 的唯一映射层；
- Tauri、exec 和兼容 Chat 消费同一协议，不各自解释 Core 私有枚举；
- 同一个 EventMsg 只映射一次，再 fan-out 到连接；
- UI 的活动卡、正文 delta、审批表面和工具结果都以 `turn_id + item_id` 关联。

### 9.3 兼容 Chat RPC

`Chat` RPC 保留为边界适配器：

1. 先确保当前连接已订阅 Thread；
2. 提交 `Op::TurnInput`；
3. 过滤对应 `turn_id` 的共享事件；
4. 映射成旧 Chat stream item；
5. 收到唯一终止事件后结束兼容流。

兼容适配器不能创建独立 Agent Loop、独立事件 hub 或第二份生命周期状态。`Done` 只允许存在于
Chat/Tauri compatibility adapter；Core 终态只有 `TurnComplete` 或 `TurnAborted`。

## 10. Shutdown 与故障语义

### 10.1 正常关闭

`Op::Shutdown` 执行：

1. 停止接收新的非关闭提交；
2. 取消活动 SessionTask 和待审批等待；
3. 生成必要的 `TurnAborted`；
4. flush rollout；
5. 发出 `ShutdownComplete`；
6. 关闭事件 sender；
7. 完成 termination future。

### 10.2 异常退出

进程崩溃后只恢复 rollout 中已存在的稳定记录。重建器必须：

- 将有 `TurnStarted` 但没有终止事件的历史 Turn 标记为 interrupted/stale；
- 不伪造瞬时 delta；
- 不把 SQLite 投影中较新的孤立数据覆盖回 rollout；
- 允许用户提交新 Turn 继续线程。

## 11. 迁移策略

### 阶段 A：协议与 rollout 基础（已完成）

- 建立 `Op`、`Submission`、`Event`、`EventMsg`、`TurnItem`；
- 实现 `rollout::policy`、writer、flush 与 reconstruction；
- 新 Thread 默认 `Paginated`，旧 Thread 标记 `Legacy`。

### 阶段 B：Session actor（已完成）

- 引入 `AstroThread` / `SessionIo`；
- 建立 512 submission queue 和长期 `submission_loop`；
- 把 Turn、审批、打断、配置和 Shutdown 迁入 `Op`；
- 收敛终止事件到 `on_task_finished`。

### 阶段 C：Server listener（已完成）

- 建立 ThreadStateManager、ThreadHistoryBuilder、单 listener；
- 建立多连接订阅、128 outbound queue、慢连接断开；
- 实现 thread start/resume/unsubscribe 和 30 分钟空闲卸载。

### 阶段 D：客户端适配（已完成）

- 迁移 gRPC app-server；
- 迁移 Tauri 和前端活动时间线；
- 迁移 exec/headless；
- 旧 Chat RPC 变为兼容适配器。

### 阶段 E：删除旧路径（已完成）

- 删除 `MultiTurnStreamItem` 作为内部事实源；
- 删除未接入的 `EventBus`；
- 删除独立 `SessionEventHub`；
- 删除后台事件丢弃逻辑；
- 删除 `after_event_id` 恢复实现和重复状态机。

迁移后的兼容层只允许做单向协议投影，最终只保留一条事件事实链。

## 12. 验证方案

### 12.1 Core 单元测试

1. submission queue 容量为 512，满时背压且不乱序。
2. `submission_loop` 按提交顺序分发 Op。
3. `StartOrSteer`、`StartIfIdle`、`Steer` 分别返回正确的 started / steered / not-submitted 结果。
4. 显式替换 SessionTask 时，旧 Turn 正确以 `Replaced` 终止。
5. 成功、错误、中断、预算耗尽都只产生一个终止事件。
6. 错误路径产生 `Error + TurnComplete(error)`，而不是 `Error + Done`。
7. 可持久事件先调用 rollout append，再进入 Core queue。
8. rollout append 失败时仍投递事件，并记录持久化失败。
9. `flush` 只确认已接受记录，不宣称恢复失败 append。

### 12.2 Rollout 重建测试

1. Paginated `ItemCompleted` 可重建 TurnItem。
2. delta、stdout、approval request 不写入 rollout。
3. Legacy 与 Paginated 的 persistence policy 符合表格。
4. completed、errored、aborted、stale Turn 状态重建正确。
5. Compaction、MCP、Hook、Subagent 和 Extension item 可重建。
6. SQLite 投影删除后可从 rollout 重新生成。

### 12.3 Server 集成测试

1. 两个订阅者收到相同顺序的 typed notifications。
2. 慢连接队列满后被断开，快连接继续接收。
3. 无订阅者的活动 Thread 仍被 listener drain。
4. resume 返回持久历史与活动 Turn snapshot。
5. resume 命令与 listener 串行化，不存在 snapshot/live gap。
6. 恢复后不重复稳定 Item，不补发瞬时 delta。
7. 活动审批可重新发送给恢复连接。
8. 无订阅且空闲 30 分钟后才执行 Shutdown。

### 12.4 兼容与端到端测试

1. 旧 Chat 适配器与新 Thread 事件结果一致。
2. 前台与后台同类工具产生相同 Item 生命周期。
3. Tauri、exec 和第二客户端可同时观察同一 Thread。
4. 工具 stdout、patch、approval、MCP、Hook、Subagent、Compaction 粒度符合协议。
5. `cargo test --workspace --all-targets` 通过。
6. `cargo clippy --workspace --all-targets` 无新增告警。
7. `apps/desktop` TypeScript 检查与生产构建通过。

### 12.5 2026-08-20 实现验证记录

Focused 门全部通过：

- `cargo fmt --all -- --check`；
- `cargo test -p agent-protocol`：3 passed；
- `cargo test -p agent-rollout`：16 passed；
- `cargo test -p agent --test thread_event_lifecycle_test`：8 passed；
- `cargo test -p agent --test streaming_test`：28 passed；
- `cargo test -p server --test thread_events_test`：15 passed；
- `cargo test -p session --test rollout_projection_test`：6 passed。

完整回归结果：

- `cargo test --workspace --all-targets`：51 suites，1478 passed，2 ignored；
- `cd apps/desktop && npx tsc --noEmit`：通过；
- `cd apps/desktop && npm run build`：通过；Vite 仍报告既有 dynamic/static import 和
  大于 500 kB chunk 的非阻断 warning；
- legacy deletion `rg`：0 个 runtime 命中；
- EventMsg coverage `rg`：217 个 emitter、mapper 或 test 命中；
- `git diff --check`：通过。

`cargo clippy --workspace --all-targets -- -D warnings` 未通过既有 workspace baseline。当前改动
只有文档，报告的 Rust 告警来自未修改文件：

- `crates/agent-types/src/message.rs:392`：`clippy::items_after_test_module`；
- `crates/agent-skills/src/installed.rs:461`：`clippy::unnecessary_sort_by`；
- `crates/agent-providers` lib tests：81 个既有 `clippy::unwrap_used`，首个为
  `src/compat/messages.rs:148`。
- `crates/agent-workflow` tests：36 个既有 `clippy::unwrap_used`，首个为
  `src/engine/dag.rs:265`。

该 baseline 不改变 focused 门与运行时验收结论；
`cargo clippy --workspace --all-targets -- -D warnings` 仍失败，workspace clippy 不能记为全绿。

## 13. 验收标准

- [x] 运行事件：所有 Turn/Item 生命周期来自统一 `EventMsg`。
- [x] 后台事件：后台路径不再丢弃工具、活动、上下文和终止事件。
- [x] 多订阅者：多个客户端可同时订阅，顺序一致。
- [x] 断线恢复：rollout + active snapshot 可恢复稳定线程状态。
- [x] 慢消费者：慢连接断开，不阻塞 Session 和其他连接。
- [x] 事件粒度：消息、reasoning、plan、exec、patch、approval、MCP、Hook、Subagent、Compaction 均有明确 Item/Delta 映射。
- [x] 终止一致性：每个 Turn 恰好一个 `TurnComplete` 或 `TurnAborted`。
- [x] 单一事实源：不存在第二套运行时事件 hub 或独立后台状态机。

## 14. 风险与约束

| 风险 | 缓解 |
|---|---|
| 迁移期双事件导致重复 UI | 所有兼容事件只能从新 EventMsg 派生，禁止双 emit |
| Core unbounded event queue 在无人消费时增长 | Thread 创建即启动 Server listener；活动 Thread 始终 drain |
| rollout 与 SQLite 暂时不一致 | rollout 权威，SQLite 仅投影并支持重建 |
| resume 快照与 live 流之间丢事件 | 对运行 Thread 通过 listener command 串行化 snapshot + subscribe |
| delta 不持久导致恢复后 UI 不完整 | ItemCompleted 与 active snapshot 收敛最终状态 |
| 慢客户端频繁断开 | 客户端 resume；监控 slow-consumer 计数和连接队列水位 |
| 大范围一次性重构难以验证 | 按 A-E 小批次执行，每批先测试、后实现、独立提交 |

## 15. 官方参考

- [`core/src/codex_thread.rs`](https://github.com/openai/codex/blob/ede5247893a50297a47c9aa5038e6ab28312ff50/codex-rs/core/src/codex_thread.rs)
- [`core/src/session/mod.rs`](https://github.com/openai/codex/blob/ede5247893a50297a47c9aa5038e6ab28312ff50/codex-rs/core/src/session/mod.rs)
- [`core/src/session/handlers.rs`](https://github.com/openai/codex/blob/ede5247893a50297a47c9aa5038e6ab28312ff50/codex-rs/core/src/session/handlers.rs)
- [`core/src/session/turn.rs`](https://github.com/openai/codex/blob/ede5247893a50297a47c9aa5038e6ab28312ff50/codex-rs/core/src/session/turn.rs)
- [`protocol/src/protocol.rs`](https://github.com/openai/codex/blob/ede5247893a50297a47c9aa5038e6ab28312ff50/codex-rs/protocol/src/protocol.rs)
- [`protocol/src/items.rs`](https://github.com/openai/codex/blob/ede5247893a50297a47c9aa5038e6ab28312ff50/codex-rs/protocol/src/items.rs)
- [`rollout/src/policy.rs`](https://github.com/openai/codex/blob/ede5247893a50297a47c9aa5038e6ab28312ff50/codex-rs/rollout/src/policy.rs)
- [`app-server/src/request_processors/thread_lifecycle.rs`](https://github.com/openai/codex/blob/ede5247893a50297a47c9aa5038e6ab28312ff50/codex-rs/app-server/src/request_processors/thread_lifecycle.rs)
- [`app-server/src/thread_state.rs`](https://github.com/openai/codex/blob/ede5247893a50297a47c9aa5038e6ab28312ff50/codex-rs/app-server/src/thread_state.rs)
- [`app-server/src/transport.rs`](https://github.com/openai/codex/blob/ede5247893a50297a47c9aa5038e6ab28312ff50/codex-rs/app-server/src/transport.rs)
