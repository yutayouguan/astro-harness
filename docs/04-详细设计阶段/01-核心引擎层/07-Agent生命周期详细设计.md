# Agent 生命周期详细设计

> 版本：v3.0
> 日期：2026-09-01
> 状态：当前实现基线
> 适用范围：`agent-core`、`agent-protocol`、`agent-rollout`、`agent-subagents`、`agent-hooks`

## 1. 生命周期层级

Astro 将 Agent 运行分为六层：

```text
ThreadManager
  -> AstroThread
     -> Session
        -> SessionTask
           -> TurnContext
              -> StepContext
                 -> tool attempt
```

| 层级 | 生命周期 | 所有状态 |
| --- | --- | --- |
| Thread | 跨多个 turn | identity、submission queue、event receiver、rollout binding |
| Session | thread 驻留期 | services、配置、active task、history、event dispatch |
| Task | 一次可取消工作 | regular / compact / review、cancel token、join handle |
| Turn | 一条用户意图 | turn id、权限、交互模式、项目/父子上下文 |
| Step | 一次 model sampling | model target、工具/MCP 快照、prompt contract |
| Attempt | 一次工具执行 | approval、sandbox、managed network、hook 和结果 |

## 2. Thread 与提交队列

`AstroThread::spawn()` 将 `Session`、`SessionIo` 和 `RolloutRecorder` 绑定一次，并启动长期 `submission_loop`。外部状态变更必须作为 `Op` 顺序提交，不能绕过队列并发修改 Session。

```text
AstroThread::submit(Op)
  -> bounded submission channel (512)
  -> Session::submission_loop
  -> admission / active task / control handler
  -> EventMsg
```

主要 `Op`：

- `TurnInput`、`RecoverTurn`、`SuspendTurnAndShutdown`；
- `Interrupt`、`CleanBackgroundTerminals`；
- `ThreadSettings`、approval/user-input/permission/dynamic-tool response；
- `RefreshMcpServers`、`ReloadUserConfig`；
- `Compact`、`Review`、`ThreadRollback`；
- `InterAgentCommunication`、`EmitExtension`、`Shutdown`。

`TurnInputRequest` 保存 `input: Vec<TurnInput>` 和只在输入被接受后应用的 `thread_settings`。`TurnInputMode` 支持 `StartOrSteer`、`StartIfIdle` 与带 expected turn id 的 `Steer`。reply 只确认 started/steered/not-submitted，不等待 turn 结束。

## 3. SessionTask

`SessionTask` 是可恢复任务抽象，当前只有三种 `TaskKind`：

| Task | 行为 |
| --- | --- |
| `RegularTask` | 正常 Responses sampling 与工具循环 |
| `CompactTask` | 执行 compact、更新 canonical history、发送 compact 生命周期事件 |
| `ReviewTask` | 在隔离配置中运行只读代码审查，再回传结果并清理资源 |

`ActiveTurn` 最多持有一个 `RunningTask`。它保存 task、kind、`CancellationToken`、`TurnContext`、完成信号、主 handle 和 auxiliary handles。

启动新 task 的顺序：

1. 获取 task admission 锁；
2. 根据 mode 判断 start、steer 或拒绝；
3. 必要时取消并等待旧 task 收敛；
4. 创建 `TurnContext` 和 terminal ownership；
5. 注册 `RunningTask`；
6. 发送 `TurnStarted`；
7. 执行 task；
8. 恰好发送一个 `TurnComplete` 或 `TurnAborted` 并清理 registry。

错误事件是诊断，不替代 terminal event。

## 4. Regular turn

```text
prepare_turn
  -> commit user input
  -> reload tools/MCP
  -> build PromptContract
  -> build canonical Vec<ResponseItem>
  -> Responses streaming
  -> accumulate response items
  -> persist assistant call/output state
  -> execute tools
  -> next sampling step or final answer
```

每个 sampling step 重新创建 `StepContext`。它冻结本次可见工具、MCP、路由、权限和工作目录；热加载只影响下一 step。模型不能调用生成该 call 时不可见的工具。

Agent 请求只走 Responses API。Session 与 rollout 的 canonical history 是 `ResponseItem`；`Message` 仅是查询/UI/非 Agent 兼容投影。

## 5. Steer、Interrupt、Suspend 与 Recover

### 5.1 Steer

Steer 将新输入交给当前 regular turn，不创建新 `TurnContext`。带 `expected_turn_id` 的模式会拒绝投递到错误 turn，防止旧 UI 操作污染新任务。

### 5.2 Interrupt

Interrupt 先触发 typed `Interrupt` hook，再取消 active task 和其 auxiliary handles。task 有固定 abort 等待上限；超时后终止 handle。最终由 task owner 发送 `TurnAborted(Interrupted)`。

### 5.3 Suspend / Recover

`SuspendTurnAndShutdown` 用于把未完成 regular turn 移交给另一个 runtime：

- 非 regular task 返回 `UnsupportedTask`；
- 存在 live descendants 返回 `HasLiveDescendants`；
- 没有 active task 返回 `NotActive`；
- 成功时先 flush rollout，停止 task，但不发送 terminal turn event。

新 runtime 使用 `RecoverTurn { turn_id }` 继续已有 turn，不追加伪造的用户输入。

## 6. Compact 与历史控制

显式 `CompactTask` 与运行中压缩共享 canonical history 原则：

1. 发送 `PreCompact`；
2. 以 Responses-only 辅助模型生成摘要，失败时使用受控 fallback；
3. 保存 `RolloutItem::Compacted` 与 canonical replacement；
4. 更新 Session history；
5. 发送 `PostCompact` / `ContextCompacted`。

工具结果原文与 Provider 视图分离：`content` 保持原文，`compressed_content` 或 spill stub 只影响模型可见视图。压缩不得把原生 tool call/output 降为普通 `Message` 后再作为权威历史。

`ThreadRollback` 是累计、可回放的 durable 控制事件。恢复时按 rollout 顺序应用，SQLite 投影可重建；不能通过直接删除消息替代 rollback 语义。

## 7. ReviewTask

Review 使用隔离的任务上下文：固定 review system prompt、受限只读工具、独立 turn/event forwarding 和资源清理。它不能修改文件、创建提交、派生任务或继承普通 Agent 的任意工具暴露。

进入/退出 review mode 通过稳定 TurnItem 表达。review 结束或被替换时，临时目录、事件 tap、辅助 handle 与状态必须全部收敛。

## 8. Hook 生命周期

Hooks 已是当前生命周期的一部分，不是未来 Plugin phase：

- session：`SessionStart` / `SessionEnd`；
- input：`UserPromptSubmit`；
- tool：`PreToolUse` / `PermissionRequest` / `PostToolUse`；
- compact：`PreCompact` / `PostCompact`；
- terminal：`Stop` / `Interrupt`；
- subagent：`SubagentStart` / `SubagentStop`。

Core 使用 typed request/outcome；Command/MCP handler 使用事件专属 JSON schema。Async hook 由 session runtime 所有，shutdown 时取消并 drain。`SessionEnd` 只发送一次并强制同步。

当前 Plugin bus 是进程级；Command/MCP runtime 是 session-bound；Turn/Step 数据显式进入 request。系统不包含 executor-scoped plugin/request metadata。

## 9. Subagent 生命周期

`agent-subagents` 是唯一子 Agent 模型。每个子 Agent 是完整 thread，有独立 session、rollout、消息时间线和状态；父子共享 Agent Graph 控制面，但权限只可收窄。

模型可使用六个协作工具：`spawn_agent`、`list_agents`、`send_message`、`followup_task`、`wait_agent`、`interrupt_agent`。`send_message` 只入 mailbox；`followup_task` 在 idle 时触发新 turn。子 Agent 不隐式创建 git worktree。

## 10. 事件与持久化

`Session::send_event` 在 `event_dispatch` guard 内执行：

```text
normalize event identity
  -> apply rollout persistence policy
  -> append durable event
  -> deliver live event
```

`ResponseItem`、completed items、turn terminal、usage、thread settings 和 rollback 是可恢复事实。delta、approval prompt、Hook run、diagnostic error 是 transient。Server listener 将同一事件流投影给 gRPC/Tauri；恢复使用 rollout snapshot + live boundary。

SessionStore 是查询、FTS 和 UI read model，不是工具执行事实源。其消息可从 rollout 重建。

## 11. 核心不变量

1. 一个 Session 同时最多一个 active task。
2. 一个 turn 只创建一个 `TurnContext`；fallback 不改变它。
3. 每次 sampling 有独立 `StepContext` 和同源 `ToolRouter`。
4. assistant tool call 先持久化，工具执行后保存 matching output，之后才能继续 sampling。
5. Agent primary、fallback 与辅助任务都只使用 Responses-capable Provider。
6. 每个 `TurnStarted` 恰好对应一个 `TurnComplete` 或 `TurnAborted`，suspend handoff 除外。
7. task cancellation 必须传播到工具、hook、子进程和 auxiliary handles。
8. durable event 先落 rollout，再 live 投递。
9. 真实对话与 Agent Graph 状态分库存储，互不冒充事实源。

## 12. 验证

```bash
cargo test -p agent
cargo test -p agent-protocol
cargo test -p agent-rollout
cargo test -p subagents
cargo test -p hooks
```

重点覆盖：task replacement、steer turn identity、interrupt terminal、suspend/recover、compact replacement、rollback replay、review cleanup、tool call/output pairing、Hook shutdown 和 rollout-before-live ordering。

## 13. 相关设计

- [Responses 原生 Agent 运行时架构](../../03-系统设计阶段/01-架构设计/12-Responses原生Agent运行时架构.md)
- [Agent 事件与恢复详细设计](12-Agent事件与恢复详细设计.md)
- [Hooks 系统详细设计](08-Hooks系统详细设计.md)
- [Agent Harness 执行外壳详细设计](14-Agent-Harness执行外壳详细设计.md)
- [工具系统详细设计](../04-工具与扩展生态/02-工具系统详细设计.md)
