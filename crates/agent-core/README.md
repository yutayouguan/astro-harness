# agent

Agent 运行时核心 crate：以 `AstroThread -> SessionTask -> TurnContext -> StepContext` 驱动 Responses-only 多轮执行、工具调用、typed hooks、持久化与恢复。

工具链与 Codex 的 Step-scoped tool plan 对齐：`CoreToolRuntime / ToolExecutor -> ToolRegistry -> build_tool_router / finalize_tool_router -> ToolRouter { registry, model_visible_specs } -> StepContext { tool_router } -> build_prompt() -> Prompt.tools -> ResponsesRequest -> ResponseItem -> ToolRouter::build_tool_call() -> ToolRegistry::dispatch`。

## 核心职责

- 维护原生 `ResponseItem` 会话历史、轮次预算与取消信号
- 每轮用户输入时召回记忆，组装 Codex 风格的三层 Prompt 契约：稳定基础指令、带角色动态上下文、独立原生工具 schema
- 统一路由内置工具与 MCP 工具，调用前后触发 typed Plugin/Command/MCP/Gateway/Shell hooks
- 通过 `StreamingCompletion` / `StreamingChat` / `StreamingPrompt` trait 暴露统一流式能力，生产 Agent 请求最终进入 Responses-only provider 入口
- 驱动「LLM 流式 → 工具执行 → 再请求」的多轮闭环
- 提供声明式 `AgentBuilder` 构建可运行 Agent 实例
- 管理工具结果压缩（原文保留，压缩视图给 provider）
- HITL 闸门、中断状态机、schema 校验、smart approval 审批
- 冻结 turn-scoped `ExtensionSnapshot`，reconcile 只把新快照排到下一 turn
- 恢复 durable Thread settings 与 `TokenUsageRecord` 累计/checkpoint
- Cron 定时任务执行、子 Agent 委派、记忆回顾、标题生成等辅助执行域

## 模块结构

| 文件/目录 | 职责 |
|-----------|------|
| `builder.rs` | 声明式 `AgentBuilder` / `BuiltAgentSpec` 构建器 |
| `compression.rs` | 工具结果压缩：保留原始 content，给 provider 发送压缩视图 |
| `control/hitl.rs` | HITL 闸门：`HitlGate` / `HitlRegistry` / `HitlRequest` / `HitlResolution` |
| `control/interrupt.rs` | 中断状态机：`Interrupt` / `InterruptPending` / `ResumeItem` |
| `control/schema_validate.rs` | 工具参数 JSON Schema 校验 |
| `control/smart_approval.rs` | LLM 辅模型智能审批（`AuxiliaryTask::SmartApproval`） |
| `control/network_approval.rs` | 网络访问审批协议 |
| `exec/cron.rs` | Cron 定时任务执行入口（`execute_job` → `run_agent_job`） |
| `exec/subagents.rs` | 子 Agent 线程生命周期管理 |
| `exec/dispatch.rs` | `DefaultAgentThreadDispatch` 实现 |
| `exec/agent_runtime.rs` | `AgentRuntimeManager` 与封装的 `RuntimeState` 代际转换 |
| `exec/background.rs` | 后台 Agent 线程执行 |
| `exec/memory_review.rs` | 记忆回顾与审批 |
| `exec/mid_run_summary.rs` | 中途摘要生成 |
| `exec/title_generation.rs` | 会话标题自动生成 |
| `exec/tool_llm_compress.rs` | 工具结果 LLM 压缩 |
| `prompt/context.rs` | 静态/动态上下文组装（`StaticContext`） |
| `prompt/context_source.rs` | 上下文来源抽象 |
| `prompt/context_usage.rs` | 上下文预算与用量追踪 |
| `prompt/contract.rs` | `PromptContract` 三层边界与 developer/user 角色分层 |
| `prompt/prompt_builder.rs` | System prompt 分层构建器 |
| `prompt/hooks.rs` | Hook 集成与 `CancelSignal` |
| `prompt/response_input.rs` | 原生 `ResponseItem` 输入变换与注入 |
| `prompt/sanitize.rs` | Prompt 清洗与安全处理 |
| `runtime/mod.rs` | `Session`（原 `AgentLoop`）核心结构体、`Config`、`TurnResult` |
| `runtime/session_state.rs` | `SessionState` — 会话级可变运行时状态 |
| `runtime/session_services.rs` | `SessionServices` — 会话级服务注册表 |
| `runtime/session_io.rs` | `AgentStatus` 状态枚举与 I/O 绑定 |
| `runtime/astro_thread.rs` | `AstroThread` — Session 的事件流句柄 |
| `runtime/event_dispatch.rs` | durable event 的 rollout-first 持久化与 live 投递 |
| `runtime/response_journal.rs` | canonical `ResponseItem` 追加与 SQLite 投影边界 |
| `runtime/model_ctx.rs` | `ModelContext` — LLM 凭证、model_targets/fallback 链 |
| `runtime/turn_budget.rs` | `TurnState` — turn_id、轮次/深度计数、`MaxDepthError` |
| `runtime/turn_lifecycle.rs` | 轮次生命周期 — `begin_user_turn` / `run_turn` / `prepare_llm_context` |
| `runtime/turn_context.rs` | `TurnContext` — 单轮上下文快照 |
| `runtime/step_context.rs` | `StepContext` — 单步（工具调用）上下文 |
| `runtime/compression_state.rs` | `CompressionState` — mid-run 摘要、compact 建议、召回上下文 |
| `runtime/context_maintenance.rs` | 上下文维护 — `maintain_tool_context` / `provider_history` |
| `runtime/recording.rs` | `ResponseItem` 记录 — `record_assistant_*` / `record_tool_result_*` |
| `runtime/tool_dispatch.rs` | 工具调度 — `handle_tool_call_async` / `finalize_tool_call_result` |
| `runtime/tool_router.rs` | `build_tool_router` / `finalize_tool_router` / `ToolRouter` — 冻结 Registry、模型可见 schema 与结构化路由 |
| `runtime/system_prompt.rs` | Prompt 契约构建 — `build_prompt_contract` |
| `runtime/submission_loop.rs` | 有序提交循环 |
| `runtime/history_control.rs` | compact replacement、rollback、suspend/recover 历史控制 |
| `runtime/validate.rs` | `validate_message_order` 消息角色顺序校验 |
| `streaming/multi_turn.rs` | 多轮工具循环编排（核心流式主循环） |
| `streaming/traits.rs` | 三层 Streaming trait 定义 |
| `streaming/provider.rs` | `Prompt` / `build_prompt` / `ProviderStreamer` — 冻结 `instructions + input + tools` 并接入 fallback |
| `streaming/fallback.rs` | Agent Responses 主模型首包前故障切换 |
| `streaming/tools_exec.rs` | 单轮工具调用执行（串行 HITL / 并发普通） |
| `streaming/hitl_bridge.rs` | `astro_hitl` 解析与会话 park/resume 桥 |
| `streaming/summary.rs` | 迭代预算耗尽后的强制总结轮 |
| `streaming/types.rs` | `StreamedAssistantContent` 流式内容类型 |
| `tasks/mod.rs` | `ActiveTurn` — Codex 风格单活跃任务注册 |
| `timeline.rs` | 助手回合时间线（`astro_timeline_v1`） |

## 核心类型与 API

- `Session`（兼容别名 `AgentLoop`）— 会话运行时，拥有 canonical `ResponseItem` 历史、记忆、工具注册表与 provider 凭证
- `Config`（别名 `AgentConfig`）— 运行时配置：轮次预算、记忆路径、soul、温度、上下文预算
- `AstroThread` — Session 的事件流句柄，提供 `next_event()` / `status()` 接口
- `TurnResult` — 单轮结果枚举：`Continue` / `Steered` / `ToolCalls` / `Finished` / `BudgetExhausted` / `MaxDepth` / `Interrupted`
- `TurnContext` — 单轮快照：turn_id、轮次序号、交互模式、权限配置、项目根
- `AgentBuilder` / `BuiltAgentSpec` — 声明式构建可运行 Agent
- `HitlGate` / `HitlRequest` / `HitlResolution` — 人机交互闸门
- `Interrupt` / `InterruptPending` — 中断状态机
- `ProviderStreamer` — 流式补全实现，含 fallback 切换
- `StreamingResponses` — Agent 原生 Responses 流式 trait
- `ResponsesOverride` / `ResponsesOverrideInput` — 保留 instructions 与 Items 边界的测试注入接缝
- `CancelSignal` — 可克隆取消信号，供 UI 或上层触发中断

## Crate 关系

| 方向 | crate | 说明 |
|------|-------|------|
| 依赖 | `agent-protocol` | `Op`、`EventMsg`、`TurnItem` 与 canonical `ResponseItem` |
| 依赖 | `types` | 通用 DTO：ModelTarget、ToolEntry、InteractionMode；`ModelTarget` 是模型路由目标名，不代表 Chat Completions 协议 |
| 依赖 | `providers` | LLM 流式调用、fallback、media 生成 |
| 依赖 | `tools` | 工具注册表、分发、审批、ToolContext |
| 依赖 | `memory` | MemoryManager、配置加载、workspace 引导 |
| 依赖 | `session` | SessionStore / ConversationStore 消息持久化 |
| 依赖 | `sandbox` | 子 Agent 进程沙箱策略 |
| 依赖 | `home` | 路径约定、agent config、tool gates |
| 依赖 | `hooks` | typed lifecycle 与 Plugin/Command/MCP/Gateway/Shell hook runtime |
| 依赖 | `mcp` | MCP 客户端连接池与工具发现 |
| 依赖 | `skills` | Skill 加载与管理 |
| 依赖 | `subagents` | AgentControl / AgentPath / AgentGraph |
| 依赖 | `cron` | Cron job 持久化与运行记录 |
| 依赖 | `a2ui` | AG-UI 声明式组件 |
| 依赖 | `artifacts` | 文件空间索引 |
| 依赖 | `usage` | 用量事件与成本估算 |
| 被依赖 | `agent-server` | gRPC 服务端通过本 crate 驱动 Agent 循环 |
| 被依赖 | `astro-agent`（Tauri） | 桌面应用通过本 crate 构建 Session |

## 关键不变量

1. **原生历史**：Agent sampling、SessionStore、rollout 和 Desktop history RPC 都使用 `ResponseItem`；UI 只在渲染边界生成 `ConversationEntry`
2. **原生命名空间**：模型工具以分离的 `namespace + name` 命中 `ToolRouter`；展平字符串只能作为展示或内部执行键
3. **工具深度**：`tool_rounds` 在每条用户消息开始时归零；单条用户消息内上限 `multi_turn`（默认 90）；`increment_tool_round()` 超限返回 `MaxDepthError`
4. **streaming 不变量**：每轮 assistant 回复必须先写入 history 再执行工具；usage 覆盖式累加，并以 rollout `TokenUsageRecord` 保存 resume/compaction 累计基线
5. **取消信号**：`CancelSignal` 在工具调用前后均检查，已取消则立即中断
6. **Session 是 Send + Sync**：所有可变状态封装在 `StdMutex` / `TokioMutex` 中，无裸 `RefCell`
7. **事件有序性**：`event_dispatch` 锁保证 rollout 持久化与 live 投递严格有序
8. **Provider 边界**：primary、fallback 和 Agent 辅助任务都必须支持 Responses，不回退 Chat Completions
9. **Persistent 门禁**：只有 OpenAI backend 且模型目录存在非空 persistent instructions 时可启用；wire effort 为 `disabled`
10. **Extension 冻结**：当前 turn 的 snapshot 不可替换，pending reconcile 只能在下一 turn 发布
11. **Usage 恢复**：resume 使用最后一条 cumulative checkpoint，fork 不继承父 Thread 累计值
12. **Runtime 转换封装**：`RuntimeState` 不暴露 `HashMap` 的 `DerefMut`，slot 声明周期只能通过显式转换方法修改

## 测试

```bash
cargo test -p agent
```
