# Agent Harness 总体架构

> 阶段：系统设计
>
> 状态：当前实现基线
>
> 更新：2026-09-04
>
> 适用范围：Astro 中包裹模型、驱动多步任务并把意图落到真实环境的工程化运行系统

## 1. 定义

Astro 统一采用以下定义：

> **Agent = Model + Harness**

- **Model** 负责推理、决策、生成文本或工具调用意图。
- **Harness**（智能体运行框架/执行外壳）负责将意图落到真实环境，并管理整个过程。

`harness` 沿用 *test harness* 的含义：它把被驱动的组件固定在可控条件下运行，向外提供执行环境，向内提供边界、观测和恢复能力。

Agent Harness 不是一段 system prompt，也不是一个工具调用函数。它是从提交、推理、行动、观察到恢复的完整运行闭环。

## 2. 术语边界

| 术语 | 责任 | 在 Astro 中的对应 |
| --- | --- | --- |
| Model | 推理、生成、工具选择 | `agent-providers`、`ModelTarget`、Provider 适配器 |
| Scaffold | 基础指令、动态上下文、工具 schema、输出契约 | `PromptContract`、`ResponsesRequest.tools` |
| Harness | 运行循环、状态、工具、权限、恢复和观测 | `agent-core` 为中心的跨 crate 系统 |
| Framework | 构建 Agent 的 API/组件集 | Astro 整体可被视为框架，但不与 Harness 同义 |
| Orchestrator | 协调多 Agent/多任务 | `agent-subagents`、Agent Threads 控制面 |
| Gateway | 模型或工具流量的鉴权、路由和边界 | Provider dispatch、MCP Hub、managed network proxy |

Scaffold 是 Harness 在每个 sampling step 中交给 Model 的“可见工作面”；Orchestrator 和 Gateway 是 Harness 的专门子系统，而不是 Harness 的别名。

## 3. 职责范围

Astro Agent Harness 包含七个不可缺失的职责面：

1. **执行循环**：接收输入，调用模型，处理 tool call，回灌观察，继续 sampling，直到完成、中断或耗尽。
2. **上下文与记忆**：构造每次模型可见内容，管理长期记忆、FTS 召回、工具结果压缩和上下文预算。
3. **工具中介**：登记和暴露工具，保存原生协议语义，校验参数，执行并将结果恢复为模型历史。
4. **权限与安全**：实施交互模式、审批、沙箱、路径边界、网络策略、MCP 审批和 hook 拦截。
5. **错误与恢复**：处理超时、取消、Provider fallback、残缺工具调用、压缩失败和进程重启。
6. **可观测与审计**：持久化事件、token/成本、工具调用、审批决策、安全拒绝和子 Agent 活动。
7. **运行环境**：承载终端、浏览器、MCP、媒体 Provider、Cron 和工作流执行。

## 4. 系统边界

```text
Desktop / gRPC / Cron / Subagent runner
                  |
                  v
       agent-protocol::Op
                  |
                  v
        AstroThread + SessionIo
                  |
                  v
          submission_loop
                  |
          SessionTask / RegularTask
                  |
                  v
      prepare_turn + PromptContract
                  |
                  v
   native Vec<ResponseItem>
                  |
                  v
       Responses streaming request
                  |
      text/reasoning/tool deltas
                  |
                  v
  ToolAccumulator -> StepContext -> ToolRouter
                  |
  approval -> sandbox -> hooks -> handler/MCP
                  |
                  v
       observation / tool result
                  |
          next sampling step
                  |
                  v
 agent-protocol::EventMsg / TurnItem
                  |
        rollout append-only log
                  |
      live listener + DB projection
```

Harness 的上边界是 `Op`：外部提交用户输入、中断、设置更新或扩展事件。下边界是真实执行环境和 Provider。对外观察边界是 `EventMsg`/`TurnItem`。

## 5. Reason → Act → Observe 循环

### 5.1 Reason

`RegularTask` 在开始时调用 `Session::prepare_turn()`：

- 提交并持久化本轮用户输入；
- 热加载 tool gates 和 MCP；
- 构建 `PromptContract`；
- 捕获本次 step 的工具与配置快照；
- 向 Provider 发起流式请求。

`PromptContract` 区分稳定基础指令和带角色的动态上下文。原生工具 schema 独立位于 `ResponsesRequest.tools`，不伪装成 system prompt 文本。

### 5.2 Act

Provider 返回原生 tool call 后，Harness 必须依次完成：

```text
分片累积 -> canonical name -> StepContext 可见性校验
-> JSON/Freeform 参数校验 -> 交互模式检查
-> 审批/HITL -> sandbox/network policy -> PreToolUse
-> handler/MCP -> TransformToolResult -> PostToolUse
```

`StepContext` 是安全不变量：只有生成该调用时已暴露的工具才能被模型直调；`tool_search` 激活的 Deferred 工具从下一次 sampling 开始进入新的快照。所有调用都经过 Rust 宿主的审批、沙箱、hook 和计数链。

### 5.3 Observe

工具结果先记录为可恢复的结构化项，再回灌 Provider history。大结果可 spill 到磁盘；Provider 视图可使用 `compressed_content`，而原始 `content` 保留。Responses API 中 Function、custom 和 tool_search 必须保持 call/output 类型成对。

### 5.4 Continue or stop

循环在以下情况停止：

- 模型返回最终文本且无工具调用；
- 用户取消或新提交覆盖活跃任务；
- tool-round、token 或费用预算耗尽；
- 需要人类决策而进入等待状态；
- Provider 和 fallback 链无法继续；
- 发生不可恢复的协议、安全或持久化错误。

## 6. 运行层级

| 层级 | 对象 | 生命周期 | 关键不变量 |
| --- | --- | --- | --- |
| Thread | `AstroThread` | 一个会话的长生命周期 | 单一 submission loop，I/O 只绑定一次 |
| Session | `Session` | 会话共享状态和服务 | 单活跃 turn，序列化录取和事件分发 |
| Task | `SessionTask` / `RegularTask` | 一次可取消工作 | 安装新任务前中止旧任务 |
| Turn | `TurnContext` | 一条用户意图的完整执行 | tool-round 归零，输入准入可开关 |
| Step | `StepContext` | 一次模型 sampling 及其工具执行 | 工具、路由、配置快照不被热加载突变 |
| Attempt | sandbox/tool execution attempt | 一次真实执行尝试 | 审批和 managed-network lease 只归属本次尝试 |

## 7. 上下文与记忆

Harness 对“模型看到什么”负最终责任：

- 稳定基础指令：人格、固定政策和安全边界；
- Developer 上下文：AGENTS/TOOLS、记忆、每日上下文、召回内容、Skill 和交互模式；
- 用户输入：原始用户消息和多模态附件；
- 原生工具：作为 Provider 请求的独立 schema 输入；
- 历史观察：文本、reasoning、tool call/result 和压缩视图。

`maintain_tool_context()` 按 prune → 辅助模型摘要 → head/tail fallback 处理超大工具结果，并使用同轮防抖避免压缩抖动。

## 8. 工具和运行环境

Harness 通过 `ToolRegistry` 区分工具是否可执行、是否直接对模型可见，以及是否可由 `tool_search` 延迟发现。Provider 协议保留 Function、Freeform、Namespace、ToolSearch 和 WebSearch 的原生差异。

具体执行环境包括：

- `terminal` / `exec_command`：受 OS sandbox、审批和可选 managed proxy 管理；
- `tool_search`：以原生 wire type 搜索并激活 Deferred 内置工具和 MCP 工具；
- Namespace：定义、call、StepContext 路由和 output 都保留分离的 `(namespace, name)`，不接受展平名伪装模型直调；
- MCP：每 Agent 连接池、延迟工具发现和调用时审批；模型侧使用 `namespace=mcp__{server}` + 原生子工具名，内部 qualified key 只在 `McpHub` 分发边界使用；
- Workflow：每个 sampling Step 从 WorkflowStore 重建 `workflow` namespace，并在 `ToolRouter` 中冻结工作流定义与执行器；Deferred 工作流只能由可信 `tool_search_output` 激活；
- Browser：任务绑定的隔离浏览器会话；
- 媒体/工作流/Cron：作为扩展执行面，复用 Harness 的 Provider、持久化和观测能力。

## 9. 安全模型

安全不是工具 handler 末端的单次 `if`，而是沿整条执行链的多层决策：

| 层 | 决策 |
| --- | --- |
| 暴露 | tool gate、Skill additive override、`ToolExposure` 五级（Direct/DirectModelOnly/Deferred/DeferredModelOnly/Hidden）+ `ToolMode`（Direct/CodeMode/CodeModeOnly） |
| 快照 | `StepContext` 拒绝本 step 未暴露的模型调用 |
| 意图 | interaction mode 和参数 schema 校验 |
| 授权 | smart approval、MCP approval、HITL、session approval cache |
| 隔离 | `SandboxPolicy`、workspace roots、网络策略 |
| 扩展 | PreToolUse 可 block/modify，结果可 transform |
| 审计 | tool call、approval、sandbox denial、usage 和 rollout |

网络默认放开不等于无安全边界：进程内 HTTP 仍执行 SSRF 检查；启用 managed proxy 时，lease 只属于当前 tool attempt，不扩散到其他工具或 Provider。

## 10. 状态、事件和恢复

`agent-rollout` 的 append-only 记录是稳定事件恢复的事实源。Core 生成 `EventMsg`，Session 序列化完成：

```text
reduce state -> persist rollout -> deliver live event
```

Server 端每 Thread listener 向 gRPC/Tauri 投影 live stream；重启或订阅恢复时使用 rollout snapshot + live boundary，SessionStore 保存用于查询、FTS 和 UI 的投影。
通用 assistant/tool/hook `ResponseItem` 写入先追加 rollout，再更新
SessionStore 投影；mailbox/steer 的 user input 仍使用独立的两阶段准入协议，
以便在 ack 之前持久化去重 marker。

因此：

- Core `EventBus` 不再是权威事件通道；
- SQLite message row 不是完整的执行事件源；
- 恢复不应通过猜测最后一条消息来重执行副作用；
- 历史和 live 事件需通过稳定 item/turn identity 去重。

### 10.1 Usage 与上下文占用

Harness 同时维护两种不可混用的口径：

- **Turn aggregate usage**：一条用户输入引发的所有 sampling 请求之和，用于计费、单轮统计和 `TokenCount` 事件。
- **Latest sampling usage**：最近一次 Provider 请求的用量，用于校准当前上下文环，不能用整个 turn 的累计值替代。

`ContextUsage` 采用混合快照：顶层 `total_tokens` 优先使用 Provider 原始 total；Provider 未上报 total 时用其分项重算；整个 usage 缺失时才回退到本地字符估算。本地 `segments` 始终保留，用于解释 system、tools、MCP、memory 和 conversation 的组成。

```text
provider_reported  >  provider_recomputed  >  local_estimate
       top-line                top-line             top-line
                 + local estimated segments for explanation
```

`reasoning_tokens` 是 `output_tokens` 的子集，不再加到 total；`cached_input_tokens` 是 input 的子集，用于缓存命中率和差异化计价，不是额外上下文 segment。

每次完成模型响应后，Session 追加 `RolloutItem::TokenUsage(TokenUsageRecord)`：`latest`
保存最后一次 sampling，`cumulative` 保存 Thread 内累计，`compaction_response_id` 标记压缩
checkpoint。resume 读取最后一条 record；fork 不复制父 Thread 的累计值，避免跨分支重复计费。

## 11. 多 Agent 编排

`agent-subagents` 是当前唯一的子 Agent 模型。父 Agent 通过六个模型工具与 Agent Graph 交互：

- `spawn_agent`
- `list_agents`
- `send_message`
- `followup_task`
- `wait_agent`
- `interrupt_agent`

Graph、mailbox和状态投影持久化到 `{base}/subagents.db`，真实对话投影写入 `{base}/sessions/state.db`，rollout 事件仍位于 `{base}/sessions/rollouts/`。子 Agent 继承父任务权限且只可收窄；不隐式创建 git worktree。

`delegate_task`、`Supervisor`、旧 V1 子任务表和“子 Agent 必然拥有独立 worktree”均不得再被描述为当前运行方案。

## 12. Codex 对齐与 Astro 差异

### 12.1 已对齐的运行语义

- Thread 长生命周期 actor + submission queue；
- 单活跃 turn、中断和 steer 准入；
- `Op` / `EventMsg` / `TurnItem` 统一协议；
- append-only rollout 事件源和 live boundary；
- 每 step 不可变工具快照与 canonical 路由；
- Function/Freeform/Namespace/ToolSearch/WebSearch 原生工具定义；
- 内置工具直接调用、`tool_search`、Deferred tools 和 MCP 激活；
- hooks、approval、sandbox、tool-round 和 cancellation 传播。
- `request_user_input_async` 结构化异步问题与 durable questions；旧名不再注册；
- durable `ThreadSettingsApplied`、冷/热 Thread 元数据恢复与 active-turn 原子设置；
- turn-frozen `ExtensionSnapshot`、next-turn reconcile 与 process-owned MCP event-stream manager 基础；
- durable usage checkpoint、Realtime 多 session 边界与 fork 清零累计值；
- gated OpenAI `persistent` reasoning：目录指令存在才暴露，wire 映射为 `disabled`。

### 12.2 Astro 的扩展和差异

- Astro 是 Tauri + gRPC 的本地桌面工作站，同时提供内嵌 Server 投影。
- Astro 支持多 Provider 及媒体 Provider；Agent target 只接受 Responses，其他协议仅保留给工具、媒体和非 Agent 调用。
- Astro 增加了 MemoryManager、Knowledge DB、Workflow、Cron、A2UI 和用量成本子系统。
- Astro 直接向模型暴露内置工具和原生 `tool_search`，不要求用户安装 Node.js 作为工具编排宿主。
- Remote Extension Marketplace 仍等待 Astro 自有服务/认证/bundle 信任根；native voice helper
  仍等待签名的三平台 runtime。两者已完成设计审查，但不作为当前可用能力展示。
- `Op` 中已声明但 `submission_loop` 尚未实现的控制分支，必须标记为协议预留，不得写成已落地功能。

## 13. 实现映射

| Harness 能力 | 主要事实源 |
| --- | --- |
| Thread / submission | `agent-core/src/runtime/astro_thread.rs`、`session_io.rs`、`submission_loop.rs` |
| Session / turn / step | `runtime/session.rs`、`turn_context.rs`、`step_context.rs`、`turn_lifecycle.rs` |
| 模型工具循环 | `streaming/multi_turn.rs`、`tools_exec.rs`、`fallback.rs` |
| Prompt / context | `prompt/contract.rs`、`context_source.rs`、`runtime/system_prompt.rs` |
| Provider | `agent-providers/src/dispatch.rs`、`types/request.rs`、各 Provider adapter |
| 原生模型历史 | `agent-protocol/src/response_item.rs`、`agent-rollout/src/reconstruction.rs` |
| 工具 | `agent-tools/src/engine/registry.rs`、`dispatch.rs`、`execution.rs` |
| 协议 | `agent-protocol/src/submission.rs`、`event.rs`、`items.rs` |
| 事件源 | `agent-rollout/src/recorder.rs`、`reconstruction.rs`、`policy.rs` |
| Session 投影 | `agent-session/src/store/rollout_projection.rs` |
| 子 Agent | `agent-subagents/src/` |
| 沙箱 / 网络 | `agent-sandbox/src/`、`agent-network-proxy/src/` |
| Extension / MCP 生命周期 | `agent-extensions/src/lib.rs`、`agent-mcp/src/event_stream.rs`、`agent-core/src/runtime/mod.rs` |
| Realtime / usage | `agent-realtime/src/`、`agent-protocol/src/event.rs`、`agent-rollout/src/reconstruction.rs` |
| Server 投影 | `agent-server/src/thread_listener.rs`、`thread_manager.rs`、`grpc/thread_service.rs` |

## 14. 文档状态规则

所有与 Harness 相关的设计文档必须区分：

- **已实现**：有当前源码与测试证据；
- **部分实现**：主链存在，但某些协议分支或 UI 投影未完成；
- **目标设计**：尚无可执行路径，不得使用现在时宣称已落地；
- **历史参考**：仅用于解释决策演进，不能覆盖当前契约。

`docs/superpowers/plans/`、`docs/superpowers/specs/` 和 `_v0.3规划/` 保留作为时点记录，不随当前 Harness 重写。当其与本文或当前源码冲突时，以当前源码、本文和对应详细设计为准。

Responses-only 路由、原生历史和 Hook 交叉点的专项基线见 [Responses 原生 Agent 运行时架构](12-Responses原生Agent运行时架构.md)。
