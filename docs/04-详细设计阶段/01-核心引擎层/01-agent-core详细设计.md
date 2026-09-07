# agent-core 详细设计

> **当前基线（2026-09-07）**：`agent-core`（package name: `agent`）是 Harness 编排中心，核心对象为 `Session`（`AgentLoop` 为兼容别名）、`AstroThread`、`SessionTask`、`TurnContext` 和 `StepContext`。旧独立 `agent-runtime` crate 已合并到本 crate 的 `runtime/`、`tasks/`、`streaming/` 子模块。入口为 `AstroThread -> submission_loop -> SessionTask::run -> streaming::multi_turn::run_turn`。以 [Agent Harness 执行外壳](14-Agent-Harness执行外壳详细设计.md) 和当前源码为准。

> 阶段：详细设计 | 状态：当前实现基线 | 说明：Crate 职责、模块组织、核心对象、事件管线、双重预算

## 1. Crate 职责边界

`agent-core`（package name `agent`）是整个项目的中央运行时 crate，提供以下能力：

- **Agent 运行时核心**（`runtime/`）：`Session` 生命周期、`AstroThread` 句柄、`submission_loop` 有序提交、`SessionState`、`SessionServices`、`TurnContext`/`StepContext` 层级上下文、`ToolRouter`/`ToolRuntime` 工具路由、turn lifecycle、context maintenance、recording、system prompt、`IterationBudget`（per-Thread 独立迭代预算）、`TurnState`（轮次与工具深度计数）
- **可恢复任务生命周期**（`tasks/`）：`SessionTask`、`ActiveTurn`、`TaskKind`（Regular/Compact/Review/UserShell）、spawn/cancel/terminal 事件保证
- **流式补全**（`streaming/`）：fallback、HITL bridge、多轮 streaming、provider 抽象、tool 执行、summary
- **执行域**（`exec/`）：`AgentControlDirectory`（根级 AgentControl 进程目录）、`AgentRuntimeManager`（活跃 turn 管理）、subagents（单 turn 运行器）、dispatch（V2 6 工具分发 + 桌面控制面）、cron、background、memory review、title generation
- **工具结果压缩**（`compression`）：原文保留，压缩视图给 provider
- **控制型运行时**（`control/`）：HITL gate、interrupt 状态机、schema 校验、smart approval（含 `SmartApprovalContext`）、`approval_cache`（会话级审批缓存）、network approval、guardian、trust model
- **提示词域**（`prompt/`）：上下文组装、hook 集成、消息变换、prompt builder、sanitization、context usage tracking

依赖它的 crate：`agent-server`（gRPC 服务端）、`apps/desktop/src-tauri`（Tauri 桌面 shell）。本 crate 同时依赖 `agent-providers`（多厂商 LLM Provider 层）。

---

## 2. 目录结构

```text
crates/agent-core/
├── Cargo.toml
└── src/
    ├── lib.rs                          # pub use 重导出
    ├── builder.rs                      # AgentBuilder — 声明式构建可运行 Agent
    ├── compression.rs                  # 工具结果压缩（原文保留 + provider 视图）
    ├── timeline.rs                     # 助手回合时间线（astro_timeline_v1）
    ├── runtime/
    │   ├── mod.rs                      # Session（AgentLoop 别名）、AgentConfig、AgentStatus
    │   ├── astro_thread.rs             # AstroThread — 提交队列 + 事件接收
    │   ├── submission_loop.rs          # submission_loop — Op 有序分发
    │   ├── session.rs                  # Session 创建
    │   ├── session_io.rs               # SessionIo — 通道绑定
    │   ├── session_state.rs            # SessionState — 运行时状态
    │   ├── session_services.rs         # SessionServices — 服务注入
    │   ├── turn_context.rs             # TurnContext — 每轮上下文
    │   ├── step_context.rs             # StepContext — 每次 sampling 上下文
    │   ├── turn_lifecycle.rs           # Turn 生命周期管理
    │   ├── turn_budget.rs              # TurnState — 轮次/工具深度计数
    │   ├── budget.rs                   # IterationBudget — per-Thread 迭代预算
    │   ├── tool_router.rs              # ToolRouter — wire name 到 handler/MCP 路由
    │   ├── tool_dispatch.rs            # 工具分发执行
    │   ├── tool_runtime.rs             # ToolRuntime — 工具运行时环境
    │   ├── context_maintenance.rs      # 上下文维护（压缩触发等）
    │   ├── compression_state.rs        # 压缩状态追踪
    │   ├── recording.rs                # Rollout 录制与 tool spill
    │   ├── system_prompt.rs            # 系统提示词构建
    │   ├── model_ctx.rs                # 模型上下文
    │   ├── code_mode.rs                # Code Mode（QuickJS）运行时
    │   ├── event_dispatch.rs           # 事件分发 guard
    │   ├── event_identity.rs           # 事件身份标识
    │   ├── history_control.rs          # 历史控制（rollback 等）
    │   ├── usage.rs                    # 用量归一化
    │   └── validate.rs                 # 消息顺序校验
    ├── tasks/
    │   ├── mod.rs                      # SessionTask、ActiveTurn、TaskKind
    │   ├── regular.rs                  # RegularTask — 常规 Responses sampling
    │   ├── compact.rs                  # CompactTask — 执行 compact
    │   ├── review.rs                   # ReviewTask — 只读代码审查
    │   └── user_shell.rs              # UserShellTask — 用户 login-shell 命令
    ├── streaming/
    │   ├── mod.rs                      # 流式模块入口
    │   ├── multi_turn.rs               # run_multi_turn_stream — 多轮 sampling 循环
    │   ├── lifecycle.rs                # 流式生命周期管理
    │   ├── maintenance.rs              # 运行中上下文维护
    │   ├── tools_exec.rs               # 流式工具执行
    │   ├── provider.rs                 # Provider 流式抽象
    │   ├── fallback.rs                 # Provider fallback 链
    │   ├── hitl_bridge.rs              # HITL 桥接
    │   ├── summary.rs                  # mid-run 摘要
    │   ├── run_state.rs                # 运行状态追踪
    │   ├── traits.rs                   # 流式 trait 定义
    │   └── types.rs                    # 流式类型定义
    ├── exec/
    │   ├── mod.rs                      # 执行域入口
    │   ├── dispatch.rs                 # Codex V2 6 工具分发 + 桌面控制面
    │   ├── dispatch/                   # 分发子模块
    │   ├── agent_control_directory.rs  # AgentControlDirectory（根级控制器目录）
    │   ├── agent_runtime.rs            # AgentRuntimeManager（活跃 turn 管理）
    │   ├── subagents.rs                # 子 Agent 单 turn 运行器
    │   ├── background.rs               # 后台任务执行
    │   ├── cron.rs                      # Cron 调度执行
    │   ├── memory_review.rs            # 记忆审查
    │   ├── mid_run_summary.rs          # 运行中摘要生成
    │   ├── title_generation.rs         # 标题自动生成
    │   └── tool_llm_compress.rs        # LLM 辅助工具压缩
    ├── control/
    │   ├── mod.rs                      # 控制模块入口
    │   ├── hitl.rs                     # HITL gate（人在回路）
    │   ├── interrupt.rs                # Interrupt 状态机
    │   ├── smart_approval.rs           # SmartApproval（含对话上下文）
    │   ├── approval_cache.rs           # 会话级审批缓存
    │   ├── guardian.rs                 # Guardian 高风险评估
    │   ├── network_approval.rs         # 网络审批
    │   ├── schema_validate.rs          # Schema 校验
    │   └── trust_model.rs              # 信任模型
    └── prompt/
        ├── mod.rs                      # 提示词模块入口
        ├── prompt_builder.rs           # PromptBuilder
        ├── contract.rs                 # PromptContract
        ├── context.rs                  # 上下文组装
        ├── context_source.rs           # 上下文来源
        ├── context_state.rs            # 上下文状态
        ├── context_usage.rs            # 上下文用量追踪
        ├── hooks.rs                    # Hook 集成
        ├── response_input.rs           # Response 输入转换
        └── sanitize.rs                 # Sanitization
```

---

## 3. 核心对象层级

Agent 运行分为六层：

```text
ThreadManager
  -> AstroThread
     -> Session
        -> SessionTask
           -> TurnContext
              -> StepContext
                 -> tool attempt
```

| 层级 | 生命周期 | 职责 |
| --- | --- | --- |
| Thread | 跨多个 turn | identity、submission queue、event receiver、rollout binding |
| Session | thread 驻留期 | services、配置、active task、history、event dispatch |
| Task | 一次可取消工作 | regular / compact / review / user shell、cancel token、join handle |
| Turn | 一条用户意图 | turn id、权限、交互模式、项目/父子上下文 |
| Step | 一次 model sampling | model target、工具/MCP 快照、prompt contract |
| Attempt | 一次工具执行 | approval、sandbox、managed network、hook 和结果 |

### 3.1 AstroThread

`AstroThread::spawn()` 将 `Session`、`SessionIo` 和 `RolloutRecorder` 绑定一次，并启动长期 `submission_loop`。外部状态变更必须作为 `Op` 顺序提交，不能绕过队列并发修改 Session。

### 3.2 Session

`Session`（`AgentLoop` 为兼容别名）保存会话级状态和服务。`SessionServices` 注入 MCP Hub、ToolRegistry、配置等服务引用。`SessionState` 维护运行时可变状态。

### 3.3 SessionTask

`SessionTask` 是可恢复任务抽象，当前有四种 `TaskKind`：

| Task | 行为 |
| --- | --- |
| `RegularTask` | 正常 Responses sampling 与工具循环 |
| `CompactTask` | 执行 compact、更新 canonical history、发送 compact 生命周期事件 |
| `ReviewTask` | 在隔离配置中运行只读代码审查，再回传结果并清理资源 |
| `UserShellTask` | 运行用户明确输入的 login-shell 命令，投影命令事件并参与取消收敛 |

`ActiveTurn` 最多持有一个 `RunningTask`，保存 task、kind、`CancellationToken`、`TurnContext`、完成信号和 handles。

### 3.4 TurnContext / StepContext

**TurnContext**：每轮创建一次，包含 turn identity、交互模式、输入准入与事件归属。Fallback 不改变它。

**StepContext**：每次 Provider sampling 重新创建。冻结本次可见工具、MCP、路由、权限和工作目录；热加载只影响下一 step。

---

## 4. 事件管线（单一事实链）

```text
AstroThread::submit(Op)
  → bounded(512) submission channel
  → Session::submission_loop — 有序 Op 分发
  → SessionTask::run_turn — 模型/工具循环
  → EventMsg — Core 单一事件格式
  → rollout policy + JSONL append（权威历史）
  → Core event queue（单 receiver）
  → Server ThreadListener（单 listener per Thread）
  → ThreadHistoryBuilder（活跃 Turn 快照）
  → per-connection bounded(128) queues
  → Tauri ThreadEventsBridge / exec / external Thread clients
```

### 4.1 持久化先于实时交付

`Session::send_event` 先规范化 event identity，再在串行 `event_dispatch` guard 内按 rollout policy 执行 `RolloutRecorder::record`，最后进入 Core event queue。durable event 先落 rollout，再 live 投递。

### 4.2 核心事件类型

稳定生命周期由 `TurnStarted`、`ItemStarted`、`ItemCompleted`、`TurnComplete` 和 `TurnAborted` 表达。消息、reasoning、exec、patch、approval、MCP、Hook、Subagent、usage 和 compaction 都映射到同一 EventMsg/TurnItem 模型。

---

## 5. 双重迭代预算

两套独立机制并行生效：

### 5.1 TurnState.tool_rounds

每条用户消息归零，上限 `config.multi_turn`（默认 `DEFAULT_MAX_ITERATIONS` = 90），`increment_tool_round()` 耗尽时返回 `MaxDepthError`。

### 5.2 IterationBudget

每 Agent Thread 独立预算（默认 90），`code_exec` 类型轮次可通过 `refund()` 退还。

```rust
pub const DEFAULT_MAX_ITERATIONS: usize = 90;

pub struct IterationBudget {
    // 仅在单个 async 任务内顺序访问，使用 Cell 而非 Mutex
}

impl IterationBudget {
    pub fn new(max_total: usize) -> Self;
    pub fn consume(&self) -> bool;     // 消费一次迭代
    pub fn refund(&self);              // 退还一次（code_exec 等廉价轮次）
    pub fn remaining(&self) -> usize;
}
```

---

## 6. 工具路由与压缩

### 6.1 ToolRouter

`ToolRouter` 将 wire name 解析为已注册 handler 或 MCP 工具。`ToolExposure` 六级暴露：Direct / DirectModelOnly / Deferred / DeferredModelOnly / CodeModeOnly / Hidden。Deferred 工具被 `tool_search` 激活后仍需经过授权和沙箱。

### 6.2 工具结果压缩

`maintain_tool_context()` 三阶段：prune（截断超大 tool 结果）→ LLM 辅模型摘要（`AuxiliaryTask::Compaction`）→ head/tail fallback。`compressed_content` 字段存 Provider 视图；`content` 字段永远保留原文。

### 6.3 Tool spill

tool 结果 >= `DEFAULT_SPILL_THRESHOLD_BYTES` 时落盘，provider history 用 stub。

---

## 7. 关键不变量

1. **原生历史**：Agent、rollout、SQLite 和 Desktop history RPC 都使用 `ResponseItem`。相邻 user/assistant message item 不得重复角色，由 `validate_message_order()` 强制。
2. **streaming 不变量**：每轮 assistant 回复先写入再执行工具；usage 覆盖式累加（兼容 Google 累计式 usageMetadata）。
3. **Tool spill**：tool 结果 >= `DEFAULT_SPILL_THRESHOLD_BYTES` 时落盘，provider history 用 stub。
4. **Skill soft-alias**：模型把 skill 名当工具调用时，自动改写为 `skills(action=load, skill_id=...)`。
5. **MCP 工具名**：`mcp__{server_id}__{tool_name}` 前缀。
6. **交互模式**：`interaction_mode` 经 ChatRequest 下传；行为说明只进 system prompt。
7. **单一事件事实链**：`EventMsg` 是 Core 唯一事件格式。SessionStore 为可重建投影；rollout JSONL 为权威历史。
8. **Agent Thread 资源守恒**：每次 spawn 失败释放路径和身份预留；每次 turn 退出释放执行槽位；completed/interrupted/errored 线程保持可寻址。

---

## 8. 相关文档

- [02-agent-runtime详细设计.md](02-agent-runtime详细设计.md) — 旧独立 crate 设计（已合并到 agent-core）
- [07-Agent生命周期详细设计.md](07-Agent生命周期详细设计.md) — 完整 Turn/Task/Hook 生命周期
- [08-Hooks系统详细设计.md](08-Hooks系统详细设计.md) — 三总线 Hook 系统
- [12-Agent事件与恢复详细设计.md](12-Agent事件与恢复详细设计.md) — EventMsg 单一事实链、恢复协议
- [14-Agent-Harness执行外壳详细设计.md](14-Agent-Harness执行外壳详细设计.md) — Harness 当前实现基线
