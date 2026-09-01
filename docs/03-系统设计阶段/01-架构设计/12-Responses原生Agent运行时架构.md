# Responses 原生 Agent 运行时架构

> 阶段：系统设计
>
> 状态：当前实现基线
>
> 更新：2026-09-01
>
> 适用范围：`agent-core`、`agent-protocol`、`agent-providers`、`agent-rollout`、`agent-session`、`agent-hooks`

## 1. 架构决策

Astro 的 Agent 对话链路只接受 Responses API。`agent_protocol::ResponseItem` 是模型历史、工具调用配对和 rollout 回放的 canonical 类型，不再先压平成 `Message` 再还原。

这项约束只限定 Agent 对话运行时。`agent-providers` 仍可为媒体工具、独立工具和其他非 Agent 调用保留 Chat Completions、Anthropic Messages、Gemini Native 或 Interactions 适配；这些协议不能进入 Agent 的 primary/fallback target chain。

核心不变量如下：

1. Agent target 必须通过 `supports_agent_responses()`，否则在发请求前拒绝。
2. Agent 请求必须使用 `ResponsesRequest.input: Vec<ResponseItem>`；`ChatCompletionRequest.input: Vec<Message>` 仅属于非 Agent 兼容入口。
3. Session 与 rollout 保存原生 `ResponseItem`；UI、搜索或旧接口所需的 `Message` 只能是派生投影，不能反向成为模型历史事实源。
4. Function、custom、tool search 等 call/output 项必须保持原始类型、标识和顺序；output 紧邻对应 call，不能跨轮重排。
5. 标题、压缩、记忆回顾、智能审批等 Agent 自有辅助任务同样走 Responses-only 入口。
6. Provider 的显式 `api_mode` 不能绕过 Agent capability gate。

## 2. 端到端数据流

```text
TurnInput
  -> AstroThread::submit(Op)
  -> submission_loop
  -> SessionTask (Regular / Compact / Review)
  -> TurnContext + PromptContract
  -> Vec<ResponseItem> canonical history
  -> ProviderStreamer::stream_response
  -> providers::agent_responses_stream
  -> POST /responses
  -> text/reasoning/tool-call deltas
  -> canonical ResponseItem completion
  -> rollout append
  -> tool execution / next sampling step
  -> EventMsg + TurnItem live projection
```

`PromptContract.base_instructions` 对应 Responses 的 `instructions`；用户输入、上下文消息、reasoning、assistant output、tool call 和 tool output 位于 `ResponsesRequest.input`；工具 schema 位于独立的 `tools` 字段。三者不能通过拼接 system 文本互相替代。

## 3. Canonical `ResponseItem`

`ResponseItem` 覆盖模型消息、reasoning、function/custom/tool-search 调用及其输出。它承担三项职责：

- **Provider 回放**：下一次 sampling 直接序列化为 Responses `input`。
- **执行关联**：使用 call id 将工具输出与模型发出的调用精确配对。
- **持久恢复**：`RolloutItem::ResponseItem` 直接写入 append-only rollout，重启后按原类型重建。

`Message` 仍可存在于数据库查询、UI 兼容和非 Agent provider API，但属于 read model 或兼容 DTO。任何投影都不得丢失 canonical history，也不得用于重新构造工具链的权威输入。

### 3.1 工具结果配对

工具执行遵循以下顺序：

```text
assistant call item 持久化
  -> 校验 call id / tool schema / StepContext
  -> approval + sandbox + hooks
  -> handler 或 MCP 执行
  -> matching output item 持久化
  -> call/output 成对进入下一次 Responses input
```

这避免了 `No tool output found for tool call ...`：输出不是按文本或工具名猜测，而是按原生 call id 绑定；历史归一化只修复边界，不得把 output 移到不相关的 call 后。

## 4. Provider 路由边界

Agent 调用入口是 `agent_responses_stream()` 和 `agent_responses_prompt()`。它们会：

- 规范化 provider id；
- 检查 `ProviderProfile.supports_responses` 或 custom provider 配置；
- 强制本次配置使用 `responses`；
- 将 `Vec<ResponseItem>` 放入 `ResponsesRequest.input`；
- 通过 Responses adapter 发起流式请求。

`chat_stream()`、`ChatCompletionRequest` 以及其他协议 adapter 是 provider/tool 层兼容能力，不是 Agent fallback。Agent 只读取 registry 的 `responses_model`，测试注入也直接接收 `ResponsesOverrideInput`；新增 Provider 若要成为 Agent 模型，必须先实现并验证 Responses 的文本、reasoning、工具调用、工具输出与 usage 契约，再标记 `supports_responses = true`。

当前内置 Agent-capable provider 由 `profile.rs` 的 capability 标志决定；文档不复制一份独立白名单，避免配置与实现漂移。

## 5. 生命周期与控制面

每个 `AstroThread` 拥有一个长生命周期 submission loop。`Session` 只允许一个活跃 `SessionTask`：

| Task | 用途 | 关键行为 |
| --- | --- | --- |
| `RegularTask` | 正常用户 turn | 多轮 sampling、工具执行、steer、中断 |
| `CompactTask` | 显式压缩 | `PreCompact`/压缩/`PostCompact`，用 canonical replacement 更新历史 |
| `ReviewTask` | 隔离代码审查 | 独立受限上下文和工具集合，完成后清理临时资源 |

控制请求经 `Op` 顺序录取。新任务安装前会取消并收敛旧任务；`SuspendTurnAndShutdown` 只允许无活跃子孙的 regular task，先 flush rollout，再把未完成 turn 交给新 runtime 恢复。rollback 是持久控制事件，恢复时累计应用，不能靠删除 SQLite 消息模拟。

## 6. Hook 交叉点

Hooks 位于 typed lifecycle 边界，而不是 Provider payload 的字符串后处理：

- `UserPromptSubmit` 可阻断或注入上下文；
- `PreToolUse` / `PermissionRequest` / `PostToolUse` 使用事件专属输入输出 schema；
- `PreCompact` / `PostCompact` 覆盖显式和运行中压缩；
- `Stop` 可要求继续 sampling，但由 continuation guard 防止无限循环；
- `Interrupt`、`SessionStart`、`SessionEnd`、`SubagentStart`、`SubagentStop` 绑定真实生命周期。

Command 与 MCP hook 的执行状态通过 `HookStarted` / `HookCompleted` 投影到 live UI；它们是瞬态观测事件，不进入 durable rollout。异步 hook 由 session runtime 所有，并在 shutdown 时取消、排空；不引入 executor-scoped plugin/request metadata。

## 7. 持久化和恢复

权威链路是：

```text
canonical state transition
  -> rollout policy
  -> append-only record
  -> live event delivery
```

`event_dispatch` 串行化持久化与 live 投递。`ItemCompleted`、turn 终态、usage、settings、rollback 与原生 `ResponseItem` 可恢复；delta、approval prompt、Hook run 状态和诊断错误是瞬态。Server 使用 rollout snapshot + live boundary 重建客户端，不使用 Core EventBus 或最后一条 SQLite message 猜测执行状态。

## 8. 代码事实源

| 契约 | 代码位置 |
| --- | --- |
| 原生历史类型 | `crates/agent-protocol/src/response_item.rs` |
| 提交协议 | `crates/agent-protocol/src/submission.rs` |
| Thread 与 submission loop | `crates/agent-core/src/runtime/astro_thread.rs`、`submission_loop.rs` |
| SessionTask 生命周期 | `crates/agent-core/src/tasks/` |
| Responses 请求组装 | `crates/agent-core/src/streaming/provider.rs`、`prompt/messages.rs` |
| Agent capability gate | `crates/agent-providers/src/dispatch.rs`、`profile.rs` |
| Responses 序列化 | `crates/agent-providers/src/compat/responses.rs` |
| Rollout 策略与恢复 | `crates/agent-rollout/src/policy.rs`、`reconstruction.rs` |
| Hook runtime | `crates/agent-hooks/src/lib.rs`、`command.rs`、`lifecycle_events.rs` |

## 9. 非目标

- 不为 Agent 链路恢复 Chat Completions fallback。
- 不以 DeepSeek 或其他兼容实现的特殊限制替代 Codex Responses 语义。
- 不把原生 `ResponseItem` 降级为 `Message` 作为 canonical storage。
- 不引入 executor-scoped plugin/request metadata；当前作用域是 process plugin bus、session-bound command/MCP runtime 和 turn/step typed context。
- 不把历史计划文档中的未实现接口描述为当前能力。
