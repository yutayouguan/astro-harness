# Agent Harness 执行外壳详细设计

> 阶段：详细设计
>
> 状态：当前实现基线
>
> 更新：2026-08-30

## 1. 目标与定义

Agent Harness 是包裹在 LLM 外围的执行外壳。它把“模型产生下一个 action”转换为“可取消、可审批、可恢复、可审计的真实执行”。

```text
Agent = Model + Harness
Harness = Loop + Context + Tools + Safety + Recovery + Observability + Environment
```

本文仅描述当前实现；宏观边界见 [Agent Harness 总体架构](../../03-系统设计阶段/01-架构设计/11-Agent-Harness总体架构.md)。

## 2. 核心对象

| 对象 | 所有权 | 责任 |
| --- | --- | --- |
| `AstroThread` | Server/runner | 绑定 `SessionIo`、启动 submission actor、提交 `Op`、读取 `Event` |
| `Session` | Thread | 保存会话级状态、服务、MCP Hub、当前任务与事件分发器 |
| `SessionIo` | Thread | submission channel、event channel、status watch、termination signal |
| `SessionTask` | Session | 可取消的一类会话任务抽象 |
| `RegularTask` | active turn | 准备 prompt，执行常规 Reason/Act/Observe 循环 |
| `TurnContext` | task | turn identity、交互模式、输入准入与事件归属 |
| `StepContext` | sampling step | 本 step 的 provider settings generation、工具 schema、wire route、配置和执行快照 |
| `ToolRouter` | step | 将 wire name 解析为已注册 handler/MCP，保持 namespace 边界 |
| `RolloutRecorder` | Session runtime I/O | append-only 写入、flush、shutdown 和恢复事实源 |

`AgentLoop` 仅是 `Session` 的兼容别名。新设计和新代码应使用 `Session`，避免把“会话运行时”和“内层 sampling loop”混为同一概念。

## 3. 提交与单活跃任务

`AstroThread::spawn()` 将 runtime I/O 原子绑定到 `Session`，然后启动唯一 `submission_loop`。

```text
AstroThread::submit(Op)
  -> SessionIo.tx_sub
  -> submission_loop
  -> TurnInput: Session::submit_turn_input
  -> Interrupt: Session::abort_all_tasks
  -> control op: Session::dispatch_control_op
  -> Shutdown: abort -> release runtime -> flush rollout -> close stream
```

Turn 录取由 `admission_lock` 序列化，任务替换由 `task_admission` 序列化。`active_turn` 最多持有一个活跃任务。新输入是 start 还是 steer 由 `TurnInputMode` 和当前状态共同决定，不应在 UI 端自行猜测。

## 4. 常规 Turn 管线

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

内层循环不得绕开以下顺序：

1. assistant tool call 先记录；
2. 工具再执行；
3. tool result 带原 call id 记录；
4. 下一次 sampling 使用成对历史。

这个顺序同时是恢复、审计和 Provider 协议的不变量。

## 5. Prompt Scaffold 契约

Harness 负责构造 scaffold，但 scaffold 不等于 Harness。当前边界为：

```text
PromptContract {
  base_instructions,
  context: Vec<ResponseItem>,
  context_sections,
  usage,
}

ResponsesRequest {
  instructions: base_instructions,
  input: Vec<ResponseItem>,
  tools: Vec<ToolDefinition>,
  ...
}
```

历史 `CompletionRequest` 已删除。稳定基础指令、带角色动态上下文和原生工具
schema 是三个边界，不应重新拼成一段无类型字符串，也不应先降级为 `Message`。

## 6. Provider step

每次 Provider sampling 使用 `StepContext.provider_settings` 中冻结的 `ModelTarget`、`ProviderConfig` 与 generation。live model/reasoning/service-tier 更新只在下一次 Step 捕获时生效；当前 response 产生的工具调用继续使用同一份设置。Fallback 只在尚未对用户产生可见输出的安全边界切换目标，避免将两个 Provider 的半段回答拼在同一 turn 中。

Provider 返回的 text、reasoning、usage 和 tool deltas 被规范化为统一 `StreamChunk`，但必须在请求和历史层保留 Provider 原生语义，尤其是 Responses API 的 custom/tool_search call-output pair。

## 7. Tool step

### 7.1 可见性和可调用性

`ToolExposure`（定义在 `agent-types`）将两个问题分开：

- schema 是否直接发给模型；
- handler 是否允许在受信任的嵌套路径执行。

五个变体——Direct、DirectModelOnly、Deferred、DeferredModelOnly、Hidden——不是权限等级。Deferred 工具被 `tool_search` 激活后仍需经过授权和沙箱。注意 `CodeModeOnly` 不属于 `ToolExposure`，而是 `ToolMode`（`agent-types`）的独立维度，控制当前采样步骤的工具投影方式。

### 7.2 执行快照

`StepContext` 同时持有 provider settings snapshot 与 `ToolRouter`，后者保存 model-visible tools（`model_routes`）和全量 routable tools（`routes`）。

- 模型发起的工具调用必须通过 `ToolRouter::model_can_call()`；
- 热重载工具和 MCP 不能改变已生成调用的执行边界。

### 7.3 结果生命周期

工具结果通过统一 `ToolOutput` 进入：

```text
raw output
  -> hook transforms
  -> spill decision
  -> provider view compression
  -> rollout/session record
  -> next-step history
```

`content` 保留原文；`compressed_content` 只是 Provider 视图。
工具可附带最大 16 KiB 的 host-only result metadata；超限时只保留
`omitted_due_to_size_limit` 标记。metadata 随 matching output item 持久化，但不会进入模型请求。

## 8. Code Mode runtime 与模型投影

Direct 路径是当前 Provider 默认；QuickJS runtime 已实现，但 `CodeModeOnly` 的模型目录选择和 Provider 工具投影仍待接线。

Code Mode 原先将多步确定性编排放入一个 JavaScript cell；保留的兼容运行时现已改为进程内 QuickJS：

- `exec` 是 grammar-constrained Freeform tool；
- `wait` 是 Function tool；
- 每 cell 使用专用线程和新的 QuickJS runtime/context；
- 共享 `store/load` 位于 Session 服务而不是 QuickJS context；
- `yield_control()` 使 cell 保持存活，`wait` 负责 resume/terminate；
- 嵌套工具通过 Rust 异步通道回到 Host。
- `ALL_TOOLS` 只投影名称和描述；description 自带精简 TypeScript 调用声明，不另设 `getToolSchema()`。

Direct 模式在 Provider `tools` 中传递每个工具的完整 Schema；CodeModeOnly 顶层只传递 `exec` / `wait`，常用工具声明预置在 `exec` 描述中，延迟工具可从 `ALL_TOOLS` 检索，业务调用只能命中 cell 的冻结快照。CodeModeOnly 宿主不可用时必须 fail closed，不能自动扩大为 Direct 工具集。

Code Mode 不是绕过 Harness 的后门；它是 Harness 内的另一种工具调度器。

## 9. 审批、沙箱和 Hooks

工具执行尝试必须将以下决策保持在同一 attempt 中：

- 交互模式和用户授权；
- `SessionApprovalCache` 的当会话决定；
- 工作区写入 grant；
- `SandboxPolicy` 和可选 managed-network lease；
- PreToolUse/PostToolUse/TransformToolResult hook 结果；
- 取消信号、tool-round 和 usage attribution。

任何子进程重试都是新 attempt，不得默认继承上一次的临时网络 lease 或提权状态。

## 10. 事件原子性

`Session::send_event*` 在 `event_dispatch` 下序列化，以保持：

1. 运行状态归约；
2. rollout 持久化；
3. live event 交付。

在需要持久化的事件上，不得先推送 live 再尝试写入 rollout，否则 UI 已观察到的状态可能在重启后消失。

### 10.1 Token usage 双口径

Provider 的单次 usage 先归一化为：

```text
input_tokens              = 未缓存输入
cache_read_tokens         = 缓存命中输入
cache_write_tokens        = 缓存写入输入
output_tokens             = 总输出（包含 reasoning）
reasoning_tokens          = output_tokens 子集
reported_total_tokens     = Provider wire total（可缺失）
calculated_total_tokens   = input + cache_read + cache_write + output
```

Responses API 官方 response usage 同时定义 `input_tokens`、`input_tokens_details.cached_tokens/cache_write_tokens`、`output_tokens`、`output_tokens_details.reasoning_tokens` 和 `total_tokens`。Astro 保留 wire total 用于审计，而不只保留本地重算值。官方字段见 [Responses API create response](https://developers.openai.com/api/reference/resources/responses/methods/create)。

`Usage::add_assign` 只用于 turn aggregate。上下文占用使用当前 sampling 的 `FinalUsage`，在同一分层基线上发出第二个 `ContextUsage`：

1. sampling 前：`local_estimate`，包含当前 `StepContext` 已暴露的工具；
2. usage 到达：`provider_reported` 或 `provider_recomputed`，top-line 替换为实际计数；
3. 下一 step：重新 capture，因此 `tool_search`/MCP 新激活 schema 才从此时进入分层估算。

`TokenCount` 与 `ContextUsage` 都是 durable rollout 事件。前者保存 turn aggregate，后者保存最近快照、数据来源和本地分层，使恢复后不需要猜测。

## 11. 中断、错误和降级

| 场景 | Harness 行为 |
| --- | --- |
| 用户 Interrupt | 取消 active task，终止活跃工具/子进程，发出 `TurnAborted` |
| Provider 首个可见 chunk 前失败 | 可切换 fallback target |
| Provider 已产生可见输出后失败 | 不拼接另一 Provider，以 stream error 收束 |
| 工具参数错误 | 返回结构化失败观察，允许模型重新规划 |
| 结构化 policy denial | 不误归类为文件系统 escalation |
| 压缩辅助模型失败 | head/tail fallback，原始内容不丢失 |

## 12. 持久化和恢复

Harness 使用两类存储：

- **事件事实源**：`agent-rollout`，用于恢复执行时间线和稳定 item identity；
- **查询投影**：`agent-session`，用于会话列表、消息查询、FTS、billing 和 UI。

恢复过程先从 rollout 重建 snapshot，然后建立 live boundary。不再以 Core EventBus 或单独 SQLite message 表作为权威恢复源。

## 13. 子 Agent

子 Agent 是 Harness 上的多任务编排，而不是内层 loop 的特殊递归分支。当前 V2 契约为：

- 唯一模型工具面是 spawn/list/send/followup/wait/interrupt；
- 每个子 Agent 有真实 Session 时间线；
- Agent Graph/mailbox/status 持久化与 Session 消息分离；
- 凭证只在内存传递；
- 权限继承只能收窄；
- worktree 是显式桌面多任务能力，不是 subagent 隐式副作用。

## 14. 测试契约

最小 Harness 回归矩阵应覆盖：

| 契约 | 主要测试区域 |
| --- | --- |
| submission 排序、start/steer/interrupt | `agent-core` runtime tests、`agent-server` thread tests |
| PromptContract 角色与用量 | `agent-core` prompt/runtime tests |
| provider stream/fallback | `agent-providers`、`agent-core` streaming tests |
| native tools/history | `agent-providers` Responses tests、`agent-tools` alignment tests |
| approval/sandbox/network | `agent-core`、`agent-tools`、`agent-sandbox`、`agent-network-proxy` |
| rollout/reconstruction/projection | `agent-rollout`、`agent-session`、`agent-server` |
| V2 Agent Threads | `agent-subagents`、`agent-core` subagent tests |
| gRPC/Tauri projection | `agent-server`、`apps/desktop/src-tauri` |

每次 Harness 变更至少必须验证直接影响的 crate；若全工作区回归被无关基线错误阻断，必须分开报告“目标检查”与“现有基线失败”。

## 15. 当前边界

1. `agent-protocol::Op` 中部分控制操作尚是预留分支，`submission_loop` 会返回 unsupported；协议存在不等于 runtime 已实现。
2. Provider-hosted `WebSearch` 已有协议表示，但当前 Registry 实际暴露的 `web_search` 是客户端 Deferred Function。
3. QuickJS Code Mode runtime 已实现，但模型目录选择与 Provider 端 `CodeModeOnly` 投影仍待接入；当前生产采样仍以 Direct 为准。
4. 工作流、Cron 和多模型对比使用 Harness 能力，但各自还有独立调度契约，不应混入单个 regular turn 状态机。
