# Agent 生命周期详细设计

> 版本：v3.3
> 日期：2026-09-04
> 状态：当前实现基线
> 适用范围：`agent-core`、`agent-protocol`、`agent-realtime`、`agent-rollout`、`agent-subagents`、`agent-hooks`、`agent-mcp`、`agent-server`、Desktop shell

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
| Task | 一次可取消工作 | regular / compact / review / user shell、cancel token、join handle |
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
- `ResolveElicitation`、`TurnSettings`、`ApproveGuardianDeniedAction`；
- `RunUserShellCommand`；
- `RealtimeConversationStart/Audio/Text/Speech/Close/ListVoices`；
- `RefreshMcpServers`、`ReloadUserConfig`；
- `Compact`、`Review`、`ThreadRollback`；
- `InterAgentCommunication`、`EmitExtension`、`Shutdown`。

`TurnInputRequest` 保存 `input: Vec<TurnInput>` 和只在输入被接受后应用的 `thread_settings`。`TurnInputMode` 支持 `StartOrSteer`、`StartIfIdle` 与带 expected turn id 的 `Steer`。reply 只确认 started/steered/not-submitted，不等待 turn 结束。

## 3. SessionTask

`SessionTask` 是可恢复任务抽象，当前有四种 `TaskKind`：

| Task | 行为 |
| --- | --- |
| `RegularTask` | 正常 Responses sampling 与工具循环 |
| `CompactTask` | 执行 compact、更新 canonical history、发送 compact 生命周期事件 |
| `ReviewTask` | 在隔离配置中运行只读代码审查，再回传结果并清理资源 |
| `UserShellTask` | 运行用户明确输入的 login-shell 命令，投影命令事件并参与取消收敛 |

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

Agent 请求只走 Responses API。Session、rollout 和 Desktop history RPC 的 canonical history 是 `ResponseItem`；UI 仅在渲染边界生成 `ConversationEntry`，非 Agent 兼容入口单独使用 `ChatCompletionMessage`。

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

## 10. 交互控制面

交互控制操作都通过 `AstroThread::submit(Op)` 进入单一 submission queue。gRPC/Tauri 层只做 DTO 转换和应答映射，不直接修改 `Session`、`TurnContext` 或 MCP 状态。

### 10.1 `request_user_input_async`

`request_user_input_async` 在当前 turn 继续运行时向用户发送一组结构化问题。

```text
tool call { questions: [{ title, options? }] }
  -> validate and render readable fallback text
  -> ItemStarted(AgentMessage { delivery: Async, questions })
  -> ItemCompleted(same stable item id)
  -> rollout before live projection
  -> { accepted: true }
  -> current turn continues
```

- `questions` 和每个 `title` 不能为空；`options` 缺失表示纯自由文本。
- Desktop 始终允许自由文本回答，回答作为普通 user input 进入 active turn。
- started/completed 共用稳定 item id，断线 Resume 按 id 去重。
- `send_user_message_async` 已移除，模型和调用方只能使用 `request_user_input_async`。

### 10.2 `ResolveElicitation`

```text
MCP server: elicitation/create
  -> McpElicitationBroker::request
  -> pending[(server_name, request_id)]
  -> desktop elicitation event
  -> ResolveElicitation { action, content, meta }
  -> broker.resolve
  -> original MCP request future resumes
```

- `accept`、`decline`、`cancel` 保持原语义；只有 `accept` 会向 MCP server 提交 content。
- pending key 是 `(server_name, request_id)`，一次 resolve 只移除对应项，不清空其他待处理请求。
- broker 最多保留 128 个 pending request，UI channel 使用非阻塞有界入队；满载、重复 id、channel 关闭均 fail closed。
- cleanup token 将超时/取消的旧 future 与同 key 的新请求区分，防止旧 cleanup 误删新 pending entry。

### 10.3 `TurnSettings`

`TurnSettingsUpdate` 可更新 `model`、`reasoning_effort`、`reasoning_summary` 和 `service_tier`。嵌套 `Option` 区分“不变”与“清空”。

1. submission loop 校验 `turn_id` 必须等于 active task 的 `TurnContext::sub_id()`；
2. `TurnContext` 克隆当前 provider settings，在副本上完成全部校验和更新；
3. 失败返回 `TargetUnavailable` 或 `Rejected`，原快照不变；
4. 成功时递增 generation 并原子替换快照，只供下一 sampling step 读取。

它不追溯修改已发出的 Provider 请求，也不更改 Thread 默认配置或其他 turn。

`reasoning_effort = persistent` 有额外前置条件：当前 backend 必须是 OpenAI，且 Session 已从
模型目录保存非空 `astro_persistent_instructions`。Server 在初始 Chat 配置与 active-turn 更新
两个入口都校验；Provider 最终把 wire effort 映射为 `disabled` 并消费内部指令键。其他
Provider 不做透传或降级。

### 10.4 Guardian assessment / retry

Guardian 对高风险工具操作发送 `in_progress`、`approved`、`denied` 或 `aborted` assessment event。拒绝项以 assessment id 保留，并绑定 `SHA-256(tool_name + NUL + serialized_arguments)` 得到的 canonical action。

```text
denied assessment
  -> user approves assessment_id
  -> enqueue one AuthorizedRetry
  -> model/user retries an identical tool call
  -> consume the matching authorization once
  -> execute without repeating the same Guardian denial
```

- 授权不等于立即重放，必须再出现内容完全一致的工具调用才可消费。
- 每个授权只允许一次重试；不同 assessment 即使对应相同 action，也以 FIFO 顺序独立保留。
- pending denied 和 authorized retry 各限 128 项，超限时淘汰旧项，避免 Session 常驻状态无界增长。
- 当前后端、gRPC 和 Tauri command 已完整接线；桌面端仍需从 assessment 表面显式触发授权，不应将拒绝自动视为同意。

### 10.5 独立用户 Shell

`RunUserShellCommand` 只承载用户明确输入的命令，不注册为模型可见工具。运行时使用 `$SHELL -lc`；`$SHELL` 不是有效绝对文件时回退到 `/bin/sh`。cwd 依次取显式参数、turn project root、workspace dir。

- 有 active task 时，Shell 作为其 child task 运行，共享 turn id 并继承取消；无 active task 时创建独立 `UserShellTask` 和 turn。
- 进程明确标记 `origin=user` 与 `sandbox=disabled`。这是用户终端能力边界，不应被 Agent 工具路径复用。
- stdout/stderr 都以 delta 事件流式输出，每路最多捕获 1 MiB；超限后只发送一次截断标记，完成项携带 `stdout_truncated` / `stderr_truncated`。
- 取消时 kill 子进程、回收读取 task，并以 failed/cancelled 命令项收敛。

### 10.6 Realtime 会话

Realtime 是 Thread 所有的会话级连接，并非一个普通 sampling step。`agent-realtime` 独立拥有 transport negotiation、Provider wire decoding、typed event、history reducer 和 handoff wire；`agent-core` 只负责把 handoff 转换为普通 Agent turn，并将完整历史写入 rollout。Provider 原始 JSON 不跨越 crate 边界。

- `RealtimeConversationStart` 的 reply 在 transport-specific readiness 成功或失败后才完成：WebSocket 等待 session handshake，WebRTC 等待 call 创建与 SDP answer，ExistingCall 启动 sideband 生命周期。
- Realtime `ModelTarget` 和 API credential 仅属于该连接，不改写普通文本回合的 primary/fallback targets。
- `include_startup_context` 默认为 `true`：注入截断后的 system prompt，并取最近 32 条 user/assistant 文本项；显式关闭时不注入。
- transport 支持 `websocket`、`webrtc { sdp }` 和 `existing_call { call_id }`。WebRTC 使用 unified SDP 交换媒体并以 call id 建立 server sideband；ExistingCall 只附加 sideband，不发送 session update。
- V2/V3 是 Astro `RealtimeVersion` 的 wire 协议代号，不是模型版本或 OpenAI 产品代际。
  V2 映射 OpenAI/Azure GA `/v1/realtime` 事件族；V3 是仅用于 Codex `/live` 的显式
  frameless/live 协议。V3 sideband 支持有界指数退避重连；V2 断开即关闭，避免自动
  重放非幂等音频或 response request。
- `HandoffRequested` 可 start/steer 普通 Agent turn，并把 assistant delta 按 `thinking`、`commentary` 或 `bem_tags` 模式回传。BEM 识别 `[ANALYSIS]`、`[COMMENTARY]`、`[FINAL]` 及配置前缀，区分 commentary 和 speakable output；turn terminal 后 handoff 只完成一次。
- `RealtimeHistory` 只持久 session started、完整转录段、BEM item promotion 和 session closed outcome；原始 delta 与音频不落盘，并通过 `RolloutItem::RealtimeItem` 参与确定性恢复。同一 Thread 开启新 Realtime session 时，reducer 先封口上一 session 的完整 transcript 与 closed outcome，再追加新 session started，跨 session 顺序不丢失。
- Desktop WebRTC 路径使用 browser media/data channel 和 unified SDP；ExistingCall 只要求 call id，不创建重复的本地采集链。
- 连接和媒体通道必须有界；启动、传输或关闭失败通过 typed error/closed event 收敛，不留下伪活跃会话。

### 10.7 Extension reconcile

`ReconcileExtensions` 通过同一 Thread control plane 请求重新发现扩展，但不修改 active turn：

1. 发现并完整校验 next snapshot；
2. 与当前/baseline snapshot 比较稳定 fingerprint；
3. 返回 changed extension IDs 及 MCP/Skills/Hooks/toolsets refresh flags；
4. 有差异时写入 `pending_extension_snapshot`；
5. 下一 turn 首次请求扩展快照时一次性发布，当前 turn 的 `OnceLock` 保持不变。

解析、安装或升级失败不替换已激活 snapshot。event-stream manager 已具备 Server 删除与权限
generation 取消边界；普通 turn task 结束不等价于取消已托管订阅。具体 Server opener 与
Desktop 订阅入口仍待接线。

## 11. 事件与持久化

`Session::send_event` 在 `event_dispatch` guard 内执行：

```text
normalize event identity
  -> apply rollout persistence policy
  -> append durable event
  -> deliver live event
```

`ResponseItem`、completed items、turn terminal、usage、thread settings 和 rollback 是可恢复事实。delta、approval prompt、Hook run、diagnostic error 是 transient。Server listener 将同一事件流投影给 gRPC/Tauri；恢复使用 rollout snapshot + live boundary。

SessionStore 是查询、FTS 和 UI read model，不是工具执行事实源。其消息可从 rollout 重建。

Realtime 使用独立 `RealtimeItem` 持久化语义：会话启动、完整 transcript、BEM promotion 与关闭 outcome 是 durable；音频和逐字 delta 仍是 transient。

模型用量使用独立 `RolloutItem::TokenUsage`，而不是从 `TokenCount` 反推。每条
`TokenUsageRecord` 同时保存 latest 与 cumulative，compaction 后记录 checkpoint response id；
resume 恢复最后一条记录，fork 不复制父 Thread 的累计值。

## 12. 核心不变量

1. 一个 Session 同时最多一个 active task。
2. 一个 turn 只创建一个 `TurnContext`；fallback 不改变它。
3. 每次 sampling 有独立 `StepContext` 和同源 `ToolRouter`。
4. assistant tool call 先持久化，工具执行后保存 matching output，之后才能继续 sampling。
5. Agent primary、fallback 与辅助任务都只使用 Responses-capable Provider。
6. 每个 `TurnStarted` 恰好对应一个 `TurnComplete` 或 `TurnAborted`，suspend handoff 除外。
7. task cancellation 必须传播到工具、hook、子进程和 auxiliary handles。
8. durable event 先落 rollout，再 live 投递。
9. 真实对话与 Agent Graph 状态分库存储，互不冒充事实源。
10. 控制操作必须经 Thread submission queue 串行化，不绕过 Session 直接改写状态。
11. `TurnSettings` 只修改命中 turn 的下一 sampling step，Realtime target 只修改当前连接。
12. Elicitation 和 Guardian 授权都是可定位、有界的人在回路状态；Elicitation 可取消，解决和重试授权都只能消费一次。
13. 独立用户 Shell 不是模型工具；虽不进入 Agent sandbox，仍必须有任务归属、取消与输出上限。
14. Realtime Provider wire JSON 不跨越 `agent-realtime` 边界，持久化只记录经 reducer 归并的 `RealtimeItem`。
15. ExistingCall 不改写已建立会话的 session 配置；V3 可恢复 sideband，V2 不自动重放非幂等输入。
16. Extension snapshot 在 turn 内不可替换；reconcile 结果只允许从下一 turn 生效。
17. `persistent` reasoning 必须同时通过模型目录、Server 与 OpenAI adapter 三层门禁。
18. usage cumulative 从 durable checkpoint 恢复，fork 不继承父 Thread 累计值。

## 13. 验证

```bash
cargo test -p agent
cargo test -p agent-protocol
cargo test -p agent-rollout
cargo test -p subagents
cargo test -p hooks
cargo test -p agent-extensions
cargo test -p mcp
cargo test -p providers persistent_reasoning
```

重点覆盖：task replacement、steer turn identity、interrupt terminal、suspend/recover、compact replacement、rollback replay、review cleanup、tool call/output pairing、Hook shutdown、rollout-before-live ordering、Realtime transport/version/parser/history/handoff/reconnect、Elicitation 重复/取消、TurnSettings 原子替换、persistent 门禁、Extension next-turn 激活、MCP event stream 取消边界、usage checkpoint、Guardian 重复操作 FIFO 和 UserShell 输出截断。

## 14. 相关设计

- [Responses 原生 Agent 运行时架构](../../03-系统设计阶段/01-架构设计/12-Responses原生Agent运行时架构.md)
- [Agent 事件与恢复详细设计](12-Agent事件与恢复详细设计.md)
- [Hooks 系统详细设计](08-Hooks系统详细设计.md)
- [Agent Harness 执行外壳详细设计](14-Agent-Harness执行外壳详细设计.md)
- [工具系统详细设计](../04-工具与扩展生态/02-工具系统详细设计.md)
- [Realtime 子系统](../../realtime-subsystem.md)
- [2026-09-01 Codex 生命周期对齐更新说明](../../更新说明/2026-09-01-Codex生命周期对齐.md)
