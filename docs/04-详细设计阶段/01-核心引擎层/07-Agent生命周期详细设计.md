# Agent 生命周期详细设计

> 版本：v2.28
> 日期：2026-08-19
> 状态：实施基线  
> 上游参考：[OpenAI Codex](https://github.com/openai/codex) `ede5247893a50297a47c9aa5038e6ab28312ff50`
> 适用范围：`agent-core`、`agent-tools`、`agent-sandbox`、`agent-network-proxy`、`agent-types`、`agent-subagents`、`agent-memory`、`agent-session`、`agent-hooks`、`agent-mcp`

---

## 1. 文档地位

本文档是 Astro Agent 生命周期、内部类型和内部命名的权威设计。其他文档中仍出现的
`AgentInstance`、`AgentSessionManager`、`AgentExecutor::round_loop`、`Supervisor`、
`delegate_task`、短生命周期且不持久化的 Child Agent 等旧设计，均以本文档为准逐步迁移。

本次对齐的目标不是复制 Codex 产品名称，而是复用其已经验证的运行时语义、生命周期边界和
源码命名，使阅读两套代码时无需重复建立概念映射。

对齐遵循以下规则：

1. 相同语义必须使用 Codex 的类型、函数和变量名称。
2. 不同语义不得仅为了表面一致而强行同名。
3. 带产品专名的类型只替换专名：Codex 的 `CodexThread` 在 Astro 中命名为 `AgentThread`。
4. 对外 RPC、数据库和前端字段通过兼容层迁移，禁止一次性破坏已有数据。
5. 新代码不得继续引入本文列入淘汰表的旧名称。

---

## 2. 核心结论

Astro 不再把 Agent 生命周期建模为一个巨大的 `AgentLoop`。目标架构与 Codex 一致，分为五层：

```text
ThreadManager
  └─ AgentThread
      └─ Session
          └─ SessionTask
              └─ TurnContext
                  └─ StepContext
                      ├─ Model sampling
                      └─ ToolCallRuntime
```

各层只拥有与自身生命周期一致的状态：

| 层级 | Codex 对齐名称 | 生命周期 | 主要职责 |
| --- | --- | --- | --- |
| 线程注册表 | `ThreadManager` | 进程级 | 创建、恢复、fork、关闭和查找线程 |
| Agent 线程 | `AgentThread` | 跨多个 Turn | 独立身份、历史、父子关系和持久化 |
| 会话运行时 | `Session` | 线程驻留期间 | 配置、服务、active task、事件和运行态 |
| 会话任务 | `SessionTask` | 一次后台任务 | regular、compact、review 等可取消任务 |
| 用户轮次 | `TurnContext` | 一次用户 Turn | 本轮固定配置、权限、模式、父子元数据 |
| 采样步骤 | `StepContext` | 一次模型请求 | 模型、工具、MCP、权限和环境的不可变快照 |
| 工具调用 | `ToolCallRuntime` | 单个 tool call | 并发门禁、取消、审批、沙箱和执行 |

### 2.1 关键不变量

- 一个 `Session` 同时最多存在一个 active `SessionTask`。
- 一个用户 Turn 只创建一个 `TurnContext`；模型 fallback 不得修改它。
- 每次模型 sampling 前必须创建新的 `StepContext`。
- 模型看到的工具集合与该 Step 随后可执行的工具集合必须来自同一个 `ToolRouter`。
- assistant tool call 必须先持久化，再开始执行工具。
- tool result 必须持久化后才能发起下一次 sampling。
- 前台、Cron 和 SubAgent 使用同一个 `run_turn`，后台代码只能做事件收集适配。
- SubAgent 是完整 `AgentThread`，拥有独立 thread id、状态、消息和 rollout。

---

## 3. 生命周期主链路

### 3.1 输入接纳

所有用户输入统一进入：

```rust
Session::start_or_steer_turn(TurnInputRequest)
    -> TurnInputSubmission
```

目标类型：

```rust
pub struct TurnInputRequest {
    pub items: Vec<UserInput>,
    pub mode: TurnInputMode,
    pub options: TurnStartOptions,
}

pub enum TurnInputMode {
    StartOrSteer,
    StartIfIdle,
    Steer,
}

pub enum TurnInputSubmission {
    Started { turn_id: String },
    Steered { turn_id: String },
    NotSubmitted { reason: TurnInputRejection },
}
```

`Start` 创建新的 `TurnContext` 和 `RegularTask`；`Steer` 只把输入投递到当前 active turn 的
mailbox，不重建本轮配置。

### 3.2 SessionTask

```rust
pub trait SessionTask: Send + Sync + 'static {
    fn kind(&self) -> TaskKind;
    fn span_name(&self) -> &'static str;
    fn run(
        self: Arc<Self>,
        session: Arc<Session>,
        ctx: Arc<TurnContext>,
        input: Vec<TurnInput>,
        cancellation_token: CancellationToken,
    ) -> impl Future<Output = SessionTaskResult> + Send;
    fn abort(
        &self,
        session: Arc<Session>,
        ctx: Arc<TurnContext>,
    ) -> impl Future<Output = ()> + Send;
}
```

与当前 Codex 源码一致，`TaskKind` 只有 `Regular`、`Review`、`Compact`。`UserShellCommandTask`
不新增第四个 kind，而是作为独立 task 实现处理。现有
foreground/background/subagent 三套入口最终都必须创建 `RegularTask`，而不是分别持有多轮循环。

Astro 与 Codex 一致只共享 `Arc<Session>`；会话状态、配置、任务注册表、记忆、工具注册表与
MCP Hub 分别在 `Session` 内部维护最小粒度同步边界，调用方不得再增加外层 session mutex。

`Session::spawn_task` 是唯一任务启动入口，负责替换旧任务、绑定 `TurnContext`、
登记 `RunningTask` 和统一收尾。`Session::abort_all_tasks` 使用与 Codex 一致的
`TurnAbortReason::{Interrupted, Replaced, ReviewEnded, BudgetLimited}`，先触发
`CancellationToken`，再等待 task 生命周期退出。

steer 输入不创建第二条流：`TurnInput::UserInput` 进入当前 `TurnContext` 的 pending
queue，`run_turn` 在下一次 sampling 前持久化并消费。最终输出前以同一把锁原子地
执行“取出 pending 或关闭 steer”，避免输入在 task 收尾窗口丢失。

### 3.3 run_turn

`run_turn` 是唯一的 LLM 与工具多轮循环。旧的 `AgentLoop::run_turn` 只负责准备用户输入，必须
拆分并最终移除，避免与 Codex 的 `run_turn` 同名异义。

```rust
pub async fn run_turn(
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
    input: Vec<TurnInput>,
    prewarmed_client_session: Option<ModelClientSession>,
    cancellation_token: CancellationToken,
) -> SessionTaskResult;
```

Astro 当前的 `RunTurnArgs` 仅是迁移 adapter；当 provider client session 下沉到 `Session`
后删除该结构，使 `run_turn` 收敛为上述 Codex 签名。

执行顺序：

```text
1. drain 上一轮异步 hook 结果
2. Turn 开始前检查 compaction
3. 解析用户输入、提及的 Skill/Plugin/MCP
4. 持久化输入和 TurnContext
5. 进入 sampling loop
   5.1 接收 mailbox/steer 输入
   5.2 capture_step_context
   5.3 构造 provider history
   5.4 run_sampling_request
   5.5 持久化 assistant item/tool calls
   5.6 执行工具并持久化 tool results
   5.7 根据 needs_follow_up 决定是否继续
6. 执行 turn-stop lifecycle
7. flush 持久化并进入 idle
```

### 3.4 Sampling Step

```rust
Session::capture_step_context(
    turn_context: Arc<TurnContext>,
    cancellation_token: &CancellationToken,
) -> Result<Arc<StepContext>>
```

```rust
pub struct StepContext {
    pub turn: Arc<TurnContext>,
    pub model_info: Arc<ModelInfo>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub approval_policy: AskForApproval,
    pub environments: TurnEnvironmentSnapshot,
    pub mcp: Arc<McpBinding>,
    pub tool_router: Arc<ToolRouter>,
}
```

Astro 可以分阶段补齐字段，但名称和所有权关系必须保持一致：`StepContext` 持有
`Arc<TurnContext>`，工具执行持有创建 tool call 时的同一个 `Arc<StepContext>`。

---

## 4. TurnContext 与 StepContext

### 4.1 TurnContext

`TurnContext` 只放本轮不应随 sampling 改变的状态：

```rust
pub struct TurnContext {
    pub sub_id: String,
    pub config: Arc<Config>,
    pub session_source: SessionSource,
    pub parent_thread_id: Option<ThreadId>,
    pub mode: InteractionMode,
    pub permission_profile: PermissionProfile,
    pub environments: TurnEnvironmentSnapshot,
    pub developer_instructions: Option<String>,
    pub extension_data: Arc<ExtensionData>,
}
```

命名要求：

- `turn_id` 在内部统一命名为 `sub_id`；协议边界仍可序列化为 `turn_id`。
- 局部变量必须写作 `turn_context`，禁止在生命周期代码中使用含义模糊的 `ctx`。
- `run_id` 若实际表示 Turn 标识，迁移为 `sub_id`；若表示 UI stream，保留 `stream_id`。

### 4.2 StepContext

以下值必须是 Step 级快照，禁止工具执行时重新从磁盘或全局状态解析：

- model 与 reasoning 配置
- approval policy/reviewer
- cwd、workspace roots 和 permission profile
- MCP 连接与工具目录
- 模型实际看到的 tool specs
- Skill/Plugin 对本 Step 的工具贡献

现有 `prepare_llm_context() -> (messages, tools)` 迁移为：

```rust
Session::capture_step_context(...) -> Arc<StepContext>
```

Provider history 属于 sampling request 输入，不属于工具注册表；目标接口为：

```rust
run_sampling_request(
    session: Arc<Session>,
    step_context: Arc<StepContext>,
    history: Vec<Message>,
    cancellation_token: CancellationToken,
) -> SamplingResult
```

---

## 5. 工具生命周期

### 5.1 类型边界

目标工具栈：

```text
ToolSpec                  模型可见 schema
ToolRouter                本 Step 最终工具计划
ToolRegistry              进程/Session 可用 runtime 注册表
ToolInvocation            单次调用参数
ToolCallRuntime           并发、取消和生命周期执行器
ToolOrchestrator          审批、沙箱、重试
ToolOutput                返回给模型的结构化结果
```

目标类型：

```rust
pub struct ToolInvocation {
    pub session: Arc<Session>,
    pub turn: Arc<TurnContext>,
    pub step_context: Arc<StepContext>,
    pub call_id: String,
    pub tool_name: ToolName,
    pub payload: ToolPayload,
    pub cancellation_token: CancellationToken,
}
```

### 5.2 审批、沙箱与并发

所有可产生副作用的工具统一经过 `ToolOrchestrator::run`：

```text
approval requirement
  -> permission-request hooks
  -> Guardian 或 User reviewer
  -> sandbox selection
  -> first attempt
  -> sandbox denial analysis
  -> retry approval（如需要）
  -> escalated attempt
```

禁止 terminal、code_exec、MCP、file_ops 各自解释“完全访问权限”。它们只能消费
`StepContext` 中已经解析完成的权限和 `ToolOrchestrator` 产生的本次 attempt。

`ToolCallRuntime` 使用读写门禁：普通工具获取共享读许可；`exclusive_access` 工具获取写许可；
取消后仍需清理的工具必须声明 cancellation semantics。

---

## 6. SubAgent / Agent Thread

SubAgent 是完整 `AgentThread`，不是临时 Child Agent。每个线程拥有独立 `ThreadId`、`Session`、
rollout/history、状态与消息邮箱，以及 `parent_thread_id`、`forked_from_thread_id` 和 agent path。

所有线程共享根树级 `AgentControl`：

```rust
pub struct AgentControl {
    session_id: SessionId,
    registry: Arc<AgentRegistry>,
    rollout_budget: Arc<RolloutBudget>,
    execution_limiter: Arc<AgentExecutionLimiter>,
}
```

模型只能使用六个公开工具：

- `spawn_agent`
- `list_agents`
- `send_message`
- `followup_task`
- `wait_agent`
- `interrupt_agent`

读取真实 Session 时间线和递归 close 只存在于 Desktop 控制面，不是模型工具。

旧名称 `delegate_task`、`delegate_async`、`Supervisor::spawn_child` 不再进入新代码。

子线程从父 `TurnContext`/`StepContext` 继承 cwd、workspace roots、permission profile、审批与
sandbox 策略、provider/model、Skill/MCP/Plugin 快照和 root/parent turn id。禁止隐式创建 git
worktree；worktree 是用户显式选择的任务隔离能力。

---

## 7. 记忆生命周期

记忆分为活跃上下文和长期记忆两个系统。

`ContextManager` 管理当前模型历史，包括 tool call/output 配对、media 能力过滤、token 估算、
prune、compaction、rollback 和 provider-visible history。

长期记忆采用两阶段后台流水线：

```text
Phase 1: Rollout Extraction
  rollout -> raw_memory + rollout_summary -> State DB

Phase 2: Global Consolidation
  DB selection -> memory workspace diff -> restricted consolidation Agent
  -> MEMORY.md + memory_summary.md + skills/
```

约束：

- 仅 root interactive thread 触发，SubAgent 不触发写流水线。
- Phase 1 使用 DB claim/lease/retry，支持并行但不重复消费。
- Phase 2 使用全局 lease，consolidation Agent 禁止网络和协作派生。
- memory read 与 memory write 分离。
- 模型引用长期记忆时必须保留来源并更新 usage。

现有 `MemoryManager` 保留为过渡 facade；目标名称为 `MemoryStore`、`MemoriesExtension`、
`start_memories_startup_task`、`phase1::run` 和 `phase2::run`。

---

## 8. Plugin 与 Extension 生命周期

`Plugin` 是分发单元，`Extension` 是运行时贡献接口，两者不得混称。

```rust
pub struct PluginManifest<Resource> {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub paths: PluginManifestPaths<Resource>,
    pub interface: Option<PluginManifestInterface<Resource>>,
}

pub struct PluginManifestPaths<Resource> {
    pub skills: Vec<Resource>,
    pub mcp_servers: Option<PluginManifestMcpServers<Resource>>,
    pub apps: Option<Resource>,
    pub hooks: Option<PluginManifestHooks<Resource>>,
}
```

目标 Extension contributors：

- `ThreadLifecycleContributor`
- `TurnLifecycleContributor`
- `ContextContributor`
- `ToolContributor`
- `ToolLifecycleContributor`
- `TurnInputContributor`
- `TurnItemContributor`
- `McpServerContributor`
- `ApprovalReviewContributor`
- `TokenUsageContributor`

扩展数据按 `session_store`、`thread_store`、`turn_store` 三个 `ExtensionData` 生命周期分层。
当前 Plugin/Gateway/Shell hook 总线保留为兼容 adapter，逐步迁移到 typed contributors。

---

## 9. 持久化

持久化采用双层模型：

```text
Rollout JSONL / append-only items
  └─ 耐久历史、审计、resume、fork、replay

State DB / SQLite projections
  └─ thread metadata、查询、队列、Agent graph、memory jobs、分页索引
```

目标接口统一为 `ThreadStore`，覆盖 create、resume、append、persist、flush、shutdown、
load history、prepare fork、revert、read 和 list。

现有 `SessionStore` 不立即删除；先成为 `LocalThreadStore` 的 SQLite projection adapter。任何
schema 和 RPC 字段改名都必须提供 migration 或 serde alias。

---

## 10. 命名对齐表

### 10.1 类型

| 当前 Astro | 目标名称 | 处理方式 |
| --- | --- | --- |
| `AgentLoop` | `Session` | 已更名为真实主类；`AgentLoop` 仅保留兼容 type alias |
| `AgentConfig` | `Config` | 已更名为真实主类；`AgentConfig` 仅保留兼容 type alias |
| `AgentThreadDispatch` | 模型工具边界 trait | V2 保留，只暴露六个模型工具 |
| `DefaultAgentThreadDispatch` | session-bound V2 dispatcher | V2 保留，共享 root-scoped `AgentControl` |
| `AgentGraphStore` | Agent graph projection | V2 已落地；只存 graph、mailbox、status event 与恢复元数据 |
| `AgentStatusV2` | `AgentStatus` | V2 严格六态；旧序列化值只由一次性迁移器识别 |
| `ToolContext` | `ToolInvocation` | 可变 memory facade 移出调用参数 |
| 无 | `TurnContext` | 已落地，Turn 级不可变状态 |
| 无 | `StepContext` | 已落地，sampling 级不可变快照 |
| 无 | `SessionTask` / `AnySessionTask` | 已落地，对齐 Codex 的静态 trait 与 object-safe adapter |
| 无 | `ActiveTurn` / `RunningTask` | 已落地，强制单 active task |
| 无 | `ToolCallRuntime` | 已落地，绑定 `Arc<StepContext>` |
| 无 | `ToolRouter` | 新增，绑定模型可见 spec 与 runtime |
| hooks 三总线 | `ExtensionRegistry` | 三总线作为 adapter |

### 10.2 函数

| 当前 Astro | 目标名称 |
| --- | --- |
| `AgentLoop::run_turn`（仅准备输入） | `Session::start_or_steer_turn`（已落地，旧名仅兼容） |
| `run_multi_turn_stream_inner` | `run_turn`（已落地） |
| `prepare_llm_context` | `capture_step_context`（已落地） |
| `stream_chat_with_hooks` | `run_sampling_request`（已落地） |
| `handle_tool_call_async_scoped` | `ToolCallRuntime::run` |
| `dispatch_named_tool` | `ToolRouter::dispatch_tool_call` |
| `AgentRuntimeManager::start_turn` | `ThreadManager::spawn_thread` + `RegularTask` |
| `run_background_multi_turn` | 删除；保留 event collector adapter |

### 10.3 局部变量

| 禁止/旧名称 | 统一名称 |
| --- | --- |
| `agent`（实际为 Session） | Turn/task 内使用 Codex 的 `sess`；边界/字段使用 `session` |
| 无语义的 `ctx` | 仅 `SessionTask` 签名保留 Codex 的 `ctx`；其余使用 `turn_context`、`step_context` |
| `calls` | `tool_calls` |
| `call` | `tool_call` |
| `tools`（模型 schema） | `tool_specs` |
| `tools`（执行器） | `tool_registry` 或 `tool_router` |
| `run_id`（实际为 turn id） | `sub_id` |
| `child` | `subagent` 或 `agent_thread` |
| `parent_session_id` | `parent_thread_id` |
| `conversation_id` | `thread_id`，协议迁移期除外 |

---

## 11. 分阶段迁移

### Phase A：上下文命名与快照

- [x] 新增 `TurnContext`、`StepContext`。
- [x] 将 `prepare_llm_context` 改为 `capture_step_context`。
- [x] sampling 和 tool execution 传递同一个 `Arc<StepContext>`。
- [x] 不改变 RPC、数据库和 UI 行为。

### Phase B：统一 SessionTask

- [x] 引入 `SessionTask`、`AnySessionTask`、`RegularTask`、`ActiveTurn` 和 `RunningTask`。
- [x] 把 `run_multi_turn_stream_inner` 提升为唯一 `run_turn`。
- [x] 将核心主类更名为 `Session` / `Config`，旧名保留 type alias。
- [x] 前台与 background adapter 真实经过 `RegularTask`。
- [x] foreground/background/Cron/SubAgent 的执行均经过 `Session::spawn_task`。
- [x] 实现 pending-input mailbox steer 和 `Session::abort_all_tasks`。
- [x] Chat 重入时优先 steer，不替换 `PauseControl`、不创建第二条流。
- [x] 将首次用户输入的持久化从 adapter 移入 `RegularTask::run`。
- [x] 将 `Arc<Mutex<Session>>` 内锁化为 Codex 的 `Arc<Session>`。
  - [x] `active_turn` 收敛为 Codex 同构的 `Mutex<Option<ActiveTurn>>`。
  - [x] 提取 Codex 同名 `SessionState`，集中轮次、压缩、注入上下文、交互模式与 Turn/Step 快照。
  - [x] 将 `Session.state` 升级为内部 `Mutex<SessionState>`。
  - [x] 将同步 `ConversationStore` 与 `CompressionPolicy` 收口到 Codex 同名
    `SessionServices`，并用编译期断言锁定 `Session: Send + Sync`。
  - [x] 将已由 `Session.state` 保护的轮次、压缩、注入上下文与交互模式 API
    收窄为 `&self`，并用 `Arc<Session>` 编译期契约锁定。
  - [x] 引入 Codex 同名异步 `Session::clone_history()` 快照边界，先迁移只读消费者。
  - [x] 将 `session_messages` 真实迁入 `SessionState.history`，并收口记录、压缩回写与测试夹具。
  - [x] 将消息记录、provider history、tool context maintenance 与 pending input 记录
    收窄为 `&Session`，并用 `Arc<Session>` 编译期契约锁定。
  - [x] 将 `ToolRegistry` 与 MCP instructions 内锁化，并把回合准备、Step 快照与 system prompt
    API 收窄为 `&Session`。
  - [x] 将 `MemoryManager` 内锁化，并把完整工具调用链收窄为 `&Session`；memory guard 不跨
    任意工具、MCP、SubAgent 或 provider `await`。
  - [x] 引入 Codex 同名 `SessionConfiguration`，把模型、凭证、权限、项目根、hooks、MCP 与
    skill override 内锁化；所有运行时 setter 均可通过 `Arc<Session>` 调用。
  - [x] 将 `SessionTask`、streaming、background 和 server 签名迁移为 `Arc<Session>`。

v2.3 落地说明：foreground、background、Cron 与 SubAgent 不再先调用
`start_or_steer_turn*` 获取预构建 system prompt，而是把 `Vec<TurnInput>` 直接交给
`Session::spawn_task`。`RegularTask::run` 在 active `TurnContext` 绑定后统一完成输入持久化、
FTS 召回、工具/MCP 热加载、system prompt 组装和首个 sampling；旧入口仅作为兼容 adapter
保留。测试专用 chat override 仍可注入预构建 prompt，生产入口不得使用该兼容路径。

v2.4 内锁化批次 1：先迁移 task registry。`steer_input`、`spawn_task`、
`abort_all_tasks` 和 task finish 只能通过 `Session.active_turn` 的内部异步锁访问运行任务；
Session 新建时该字段为 `None`，任务启动时创建 `ActiveTurn`，收尾时恢复 `None`。本批不移动
SQLite、消息历史、工具注册表和 provider 配置，避免把 non-Send 状态迁移与任务竞争控制混为
一次高风险改动。

v2.5 内锁化批次 2：先建立状态所有权边界，不同时改变同步语义。新增
`runtime::session_state::SessionState`，将 `CompressionState`、`TurnState`、
`pending_inject_context`、`pending_learning_nudge`、`interaction_mode`、
`current_turn_context` 和 `current_step_context` 从 `Session` 直属字段迁入 `Session.state`；
`active_turn` 继续作为独立内部锁，与 Codex 的字段布局一致。本批仍由现有 `&mut Session`
路径提供互斥，后续批次再把 `state` 包装为 `Mutex<SessionState>` 并逐层收敛到
`Arc<Session>`，因此不引入新的锁跨 `await` 行为。

v2.6 内锁化批次 3：`Session.state` 已升级为 Codex 同构的
`tokio::sync::Mutex<SessionState>`。轮次、压缩、注入上下文、交互模式与 Turn/Step 快照的
读写统一通过 `.lock().await`；`current_turn_id`、`recalled_context`、`mid_run_handoff` 等读取
返回拥有所有权的快照，禁止把 guard 引用泄漏到调用方。所有 provider、MCP、工具执行和 LLM
摘要 `await` 前均释放 state guard，避免锁跨外部 I/O。由于 `ConversationStore` 与
`CompressionPolicy` 尚未完成内部锁化，相关 async 访问器本批继续接收 `&mut Session`，以维持
`SessionTask` future 的 `Send` 约束；下一批再迁移剩余状态与 `Arc<Session>` 签名。

v2.7 内锁化批次 4：新增 Codex 同名 `SessionServices`，将会话级同步依赖从
`Session` 顶层字段收口。`SharedConversationStore` 仅在单次同步 SQLite 调用期间持有
`std::sync::Mutex`；`CompressionPolicy` 的互斥区仅包含 plan、fallback、recommend 与策略替换，
均不跨 provider、工具或其他 `await`。`session_is_send_and_sync` 以编译期断言固定
`Session: Send + Sync`。本批不移除迁移期的外层 `Arc<Mutex<Session>>`；下一批再逐层改为
`Arc<Session>` 并收窄可变接口。

v2.8 内锁化批次 5：将 `set_current_turn_id`、`create_turn_context`、
`increment_tool_round`、`take_inject_context`、`schemas_for_api` 等 22 个仅访问内部
`Session.state` 或只读会话配置的 API 从 `&mut self` 收窄为 `&self`。
`session_state_api_is_callable_through_arc` 以 `Arc<Session>` 直接构造这些 future，在编译期
阻止可变接口回退；streaming、tool execution 与 task registry 中因此多余的
17 处生产路径可变 guard 同步移除。本批仍保留外层 `Arc<Mutex<Session>>`：剩余阻塞集中在
conversation history、`MemoryManager`、`ToolRegistry` 和 MCP 热加载等真实可变状态，
后续批次须先继续收口这些所有权边界，再替换 `SessionTask` 与 server 的 handle 类型。

v2.9 history 内锁化批次 1：先固定 Codex 同名的异步 `Session::clone_history()`
契约，返回拥有所有权的 `Vec<Message>` 快照，不向调用方泄漏会话内部引用。
mid-run summary、background 输出提取与 memory review 等 5 个只读消费点已迁移，
`clone_history_returns_an_owned_snapshot_through_arc` 同时锁定 `Arc<Session>` 可调用性与
快照隔离性。本批刻意不移动 `session_messages` 字段：混合追加、压缩回写和集成测试
夹具将在下一批统一收口到 `SessionState.history`，而只读调用方无需再次改签名。

v2.10 history 内锁化批次 2：删除 `Session.session_messages`，由
`SessionState.history: Vec<Message>` 唯一持有运行时会话历史，并对齐 Codex 的
`SessionState::record_items / clone_history / replace_history` 与 `Session` 异步代理。
消息记录方法在 SQLite 成功追加后，仅短暂获取 state 锁写入内存镜像；provider history、
压缩计划和占用率计算先获取拥有所有权的快照，任何 Provider、工具、MCP 或辅助模型 I/O
都不持有 state guard。压缩回写只在更新单条 `compressed_content` 时持锁，测试夹具统一
通过 `record_items`、`clone_history` 与 `replace_history`，阻止重新暴露可变 history 字段。

v2.11 shared receiver 批次：将 `record_assistant_message*`、`record_user_message`、
`record_tool_result*`、`record_turn_input`、`provider_history`、`maintain_tool_context` 与
`compress_tool_results_if_needed` 从 `&mut Session` 收窄为 `&Session`。这些方法只写
`SessionServices` 的同步持久层或 `SessionState` 内锁状态，不再要求外层 session mutex 的
可变借用。`session_state_api_is_callable_through_arc` 直接从 `Arc<Session>` 构造上述 future，
作为编译期回归门；streaming 中因此移除 9 个无意义的 mutable guard。新增
`conversation_write_lock` 将 SQLite 持久化与 `SessionState.history` 镜像追加串成同一写序，
防止移除外锁后并发记录产生 DB/history 次序分叉；
`conversation_write_lock_serializes_persistence_and_history` 固定该契约。本批不提前双包装
background handle：background、streaming 与 `SessionTask` 必须在下一批共享同一个
`Arc<Session>`，避免 `Arc<Mutex<Session>>` 与 `Arc<Session>` 并存造成身份分裂。

v2.12 ToolRegistry/MCP 内锁化批次：`Session.tool_registry` 与 MCP instructions 快照改由
`RwLock` 持有；动态工具 handler 提升为拥有所有权的 `Arc` 快照，工具执行前从注册表克隆，
因此 registry guard 不跨工具、MCP 或 provider I/O。`begin_user_turn`、
`reload_tools_and_mcp`、`prepare_turn`、`capture_step_context`、system prompt 构建与分层统计
统一收窄为 `&Session`，`session_state_api_is_callable_through_arc` 锁定共享调用契约，
`dynamic_handler_snapshot_survives_registry_reload` 锁定热加载后在途 handler 仍可完成。本批仍不
替换外层 `Arc<Mutex<Session>>`：工具执行会把 `&mut MemoryManager` 借入 `ToolContext`，这是
下一批必须先内锁化的最后一类核心可变借用；完成后才能一次性迁移 SessionTask、streaming、
background 与 server handle，避免双重 session 身份。

v2.13 MemoryManager 内锁化批次：`Session.memory` 改由 `RwLock<MemoryManager>` 持有，
`ToolContext` 只保留该锁的共享引用。agent id、workspace 与 prompt snapshot 均先复制为拥有
所有权的值；`memory` 写入、`context_search` 读取和 `persona_create` 激活仅在同步临界区内
获取 guard，动态工具、MCP、终端、媒体与 SubAgent 的异步执行均不持有 memory guard。
`handle_tool_call*`、`dispatch_named_tool` 与 `finalize_tool_call_result` 因此统一收窄为
`&Session`。`session_state_api_is_callable_through_arc` 锁定 `Arc<Session>` 工具入口，
`awaiting_dynamic_tool_does_not_block_memory_reads` 用挂起的真实动态 handler 验证异步工具不会
阻塞记忆读取。外层 `Arc<Mutex<Session>>` 的真实可变状态阻塞已消除；下一批迁移 handle 前仍须
明确 `ToolContext.sessions` 与 built-in tool future 的 non-Send 执行契约，避免把 current-thread
运行时约束误改成跨线程 `tokio::spawn`。

v2.14 SessionConfiguration 内锁化批次：对照 Codex 的 `SessionConfiguration` 命名，将会话期
可更新的 `ModelContext`、采样参数、hook bus、project root、permission profile、MCP overlay 与
skill override 收口到同步 `RwLock<SessionConfiguration>`。setter 统一收窄为 `&Session`，读取端
只返回拥有所有权的快照，`session_runtime_settings_are_mutable_through_arc` 和
`session_runtime_settings_return_owned_snapshots` 分别固定 `Arc<Session>` 可调用性与快照隔离。
同步锁只用于复制轻量配置，不跨任何 provider、MCP、工具或 Tokio `await`。同时确认 built-in
tool future 保持 `!Send` 是当前线程工具执行器的明确契约；生产 `SessionTask` 通过同步工具桥接
进入独立 current-thread worker，任务 future 仍满足 `Send`，无需为迁移共享会话句柄而扩大工具
线程安全边界。下一批可直接将 SessionTask、streaming、background 与 server 的唯一句柄替换为
`Arc<Session>`。

v2.15 Arc<Session> 句柄收敛批次：`SessionTask::run/abort`、`AnySessionTask`、
`Session::spawn_task/abort_all_tasks`、foreground streaming、background/Cron、SubAgent runner 与
server `SessionHandle` 统一共享同一个 `Arc<Session>`。`spawn_task` 与 `abort_all_tasks` 改为
Codex 同构的 `self: &Arc<Self>` 方法；生产代码和测试夹具删除全部外层 session mutex 与无意义
guard/drop。`session_task_lifecycle_is_callable_through_arc_session` 以编译期契约固定任务 API，
全仓库残留扫描禁止 `Arc<Mutex<Session>>`、`Mutex::new(Session)` 和 session `.lock().await`
重新出现。会话竞争控制仍由 `active_turn`、`SessionState`、`SessionConfiguration` 等内部锁负责，
built-in tool 的 current-thread `!Send` 契约保持不变。

### Phase C：工具运行时

- [ ] 引入 `ToolRouter`、`ToolInvocation`、`ToolCallRuntime`、`ToolOrchestrator`。
  - [x] 以 Codex 同名字段引入 `ToolInvocation` 和 `ToolCallRuntime`，把并发调用绑定到
    产生该调用的 `StepContext` 与 turn cancellation token。
  - [x] 并发工具改用 Session 统一 dispatch，不再重开 `MemoryManager` / `SessionStore`
    或手工构造 `ToolContext`；动态工具、MCP、soft alias 与 tool hooks 因此共用同一语义。
  - [x] 将注册表运行时投影与模型可见 specs 收口为 `ToolRouter`。
  - [x] 引入 `ToolOrchestrator` 第一阶段，将 MCP、只读写入和进程内网络预检按固定顺序
    收口到单一入口，并统一聚合 one-shot grants 与 permission audit receipts。
  - [x] 由 `ToolOrchestrator::run` 统一执行 terminal 审批、step-bound dispatch、Applied
    审计与错误归档，返回 Codex 同名 `OrchestratorRunResult<Out>`。
  - [x] 引入 `ExecToolCallOutput`、`SandboxErr::Denied` 与 `run_attempt`，保留 sandbox
    denial 的结构化输出并排除普通非零退出。
  - [x] 引入 Codex 同名 `SandboxAttempt`；typed denial 要求 fresh approval，批准后仅以
    attempt-scoped policy override 重试一次，不持久化、不扩大网络授权。
  - [x] 将 escalation 的文件系统与网络策略解耦：文件系统扩大到 unrestricted，网络继续
    restricted，禁止用绕过平台沙箱的 `DangerFullAccess` 冒充文件系统单项授权。
  - [x] 将 `SandboxablePreference` 固定到 `ToolRouter` step snapshot，并由 orchestrator
    为初始 attempt 选择完整 sandbox policy；兼容入口仍可在 `ToolContext` 内回退解析。
  - [x] 与 Codex 对齐 denial analysis 边界：具体 `ToolCallRuntime` 将原始进程结果分类为
    typed `SandboxErr::Denied`，orchestrator 只负责审批、审计和单次 escalated retry。
  - [x] 引入 Codex 同名 `NetworkPolicyDecisionPayload` 并挂载到 `SandboxErr::Denied`；
    orchestrator 将结构化网络拒绝与文件系统拒绝分流，禁止误触发文件系统权限升级。
  - [x] 引入 `agent-network-proxy` 策略核心，对齐 Codex 的 host 规则、本地地址纵深防御、
    `NetworkPolicyRequest` / `NetworkDecision` / `NetworkPolicyDecider` 与 structured decision attribution。
  - [x] 实现 loopback-only HTTP/1 CONNECT listener，在上游 dial 前执行 host policy，并对实际解析的
    `SocketAddr` 做二次私网检查，防止 DNS rebinding 绕过。
  - [x] 将 CONNECT host enforcement 与 decision attribution 接入 `terminal` / `code_exec`
    attempt；结构化网络拒绝 fail closed，不猜测命令域名、不开放全网、不转成文件系统提权重试。
- [ ] 删除工具执行时重新加载权限/工具的路径。

v2.16 ToolInvocation / ToolCallRuntime 首批：新运行时保留 Codex 的 `session`、
`step_context`、`cancellation_token`、`call_id`、`tool_name` 与 `payload` 调用边界。
`execute_tools_concurrent` 不再维护第二套快照执行器，而是通过 Session 现有的动态/MCP/
built-in 路由和 pre/transform/post tool hooks 执行。显式 `StepContext` 防止并发调用读取后续
sampling step 的工具可见性；cancellation token 与 Session cancel 任一取消都会终止入场。
`tool_call_runtime_preserves_hardline_defense` 保留路由误判时的高危命令纵深防御，
`concurrent_tools_use_session_router_and_hooks` 锁定动态工具与 hook 的统一路径。

v2.17 ToolRouter 批次：`capture_step_context` 在同一次注册表读锁下将交互模式过滤后的
model-visible specs、动态 handler、MCP approval 与 confirmation/exclusive/stop-after 元数据
固定到 `Arc<ToolRouter>`，再交给 `StepContext`。sampling 之后的工具 gate 热更新、MCP
重连或同名 handler 覆盖只影响下一个 step，不改写已生成 tool call 的准入与路由语义。
streaming 的串/并发选择、MCP 预审批和 stop-after 判定也统一从该 router 读取。

v2.18 ToolOrchestrator 预检批次：串行工具执行不再分别内联解释 MCP policy、只读写入和
进程内网络权限，而是通过 `ToolOrchestrator::prepare` 依次执行
`Mcp -> WorkspaceWrite -> InProcessNetwork`。授权结果统一进入 `ToolRunContext`，其中包含
本次 workspace write grant、受限 host network grant 与待写入 Applied 事件的 audit receipts；
任一阶段拒绝即短路，取消或 HITL 通道断开仍沿用 `Option` 终止语义。本批保持 terminal
危险命令审批、实际 tool dispatch、sandbox selection 与 retry 原路径不变；后续批次再将它们
迁入 Codex 同构的 `ToolOrchestrator::run`。

v2.19 ToolOrchestrator run 批次：`execute_tools_serial_inner` 只保留 pause/cancel、
`StepContext` 准入和工具返回后的 Astro HITL 展示；参数错误、三段 permission preflight、
terminal hardline/allowlist/auto-review/user-review、one-shot grants、实际 dispatch、Applied
审计与 ToolFailure 归档全部由 `ToolOrchestrator::run` 驱动，并通过
`OrchestratorRunResult<Out>` 返回。实际调用改用
`handle_tool_invocation_with_once_grants`，因此串行工具也绑定到产生调用的
`Arc<StepContext>`，采样后的动态 handler 热替换只影响下一 step。旧的无 step
`handle_tool_call_with_once_grants` 入口已删除。sandbox selection、denial retry 和 escalation
仍由当前工具实现负责，留待下一批迁移。

v2.20 Sandbox denial contract 批次：`agent-sandbox` 引入与 Codex 同名的
`ExecToolCallOutput` 和 `SandboxErr::Denied`，仅当 attempt 处于受限 sandbox、退出码非零、
不属于 2/126/127 quick rejection 且 stdout/stderr 含已知 denial signal 时才分类为拒绝。
foreground terminal 与 code_exec 不再把这类结果压成普通文本，而是保留 exit/stdout/stderr
及经过 terminal transform 的 `aggregated_output`，穿过 anyhow 和
`ToolCallError::SandboxDenied` 到 `ToolOrchestrator::run_attempt`。orchestrator 当前仍执行
`transform_tool_result` / `post_tool_use` finalization，将可展示输出返回模型、写入
`sandbox_denied` Applied audit，且不记录为普通 ToolFailure。该批次仅建立 typed
boundary，未自动重试或提升权限；v2.21 在此边界上补齐审批与单次重试。

v2.21 Sandbox escalation 批次：`ToolOrchestrator` 以 Codex 同名 `SandboxAttempt`
表达初始与 escalated attempt。首次 `SandboxErr::Denied` 会构造
`PermissionReason::SandboxDenied` 的 fresh permission request，经当前 user/auto-reviewer
明确批准后，只对原 tool call 再执行一次。重试通过
attempt-scoped sandbox override 扩大文件系统访问；session permission profile 不改写，授权
不保留到下一调用，网络 grant 也不升级为 unrestricted。
用户拒绝、reviewer 不可用或 approval policy 禁止时不重试；第二次 attempt
无论成功、普通失败或再次 denial 都直接结束，不形成 escalation loop。
`tool_router_freezes_dynamic_handler_for_step` 通过采样后覆盖 Session 注册表并执行旧 step，
锁定不可变快照契约。无 `StepContext` 的兼容工具入口仍保留当前注册表路径，待后续删除。

v2.22 Sandbox policy separation 修正：复核真实 runner 后确认
`SandboxMode::DangerFullAccess` 会完全绕过平台沙箱，因而无法兑现 v2.21 的“网络授权不扩大”
约束。现改为传递完整的 attempt-scoped `sandbox_policy`：
`SandboxPolicy::unrestricted_file_system(execution_root, false)` 使用
`WorkspaceWrite + filesystem root` 表达 unrestricted 文件系统，同时继续经过平台 sandbox，
保留 workspace metadata deny，并保持 `network_access = false`。
`SandboxAttempt -> ToolExecutionGrants -> ToolContext` 全链路
传递不可变 policy，而不再只传 mode。该修正与 Codex 当前将 filesystem/network permission
正交建模的边界一致；managed subprocess network approval 仍留待后续批次迁入 orchestrator。

v2.23 Initial sandbox selection 批次：引入 Codex 同名
`SandboxablePreference::{Auto, Require, Forbid}`，并作为 `ToolEntry -> ToolRoute` 元数据固定到
生成当前调用的 `ToolRouter` snapshot。`terminal` 与 `code_exec` 标为 `Auto`；普通 built-in、
动态工具与 MCP 工具缺省 `Forbid`，不会因无关的 custom filesystem profile 提前失败。
`ToolOrchestrator::sandbox_policy_for_attempt` 在 dispatch 前根据该快照、turn permission profile、
execution root 与 one-shot workspace-write grant 选择完整 policy，再经
`SandboxAttempt -> ToolExecutionGrants -> ToolContext` 传递。`Require` 在 ambient full-access
profile 下仍强制使用平台 sandbox；escalated attempt 的显式 policy 优先于初始选择。
共享 helper 收到显式 attempt policy 时立即返回，不再重读或重新校验可能已热更新的 profile，
从而保持同一 step/attempt 的不可变权限语义。
无 `StepContext` 的兼容调用和常驻 MCP 启动仍保留 `ToolContext`/共享 helper 回退，等待后续
删除旧入口时一并收口。

v2.24 Sandbox denial ownership 复核：对照 Codex 当前
`core/src/tools/runtimes/shell/unix_escalation.rs::process_exec_tool_call`、
`core/src/tools/runtimes/apply_patch.rs::run` 与 `core/src/tools/orchestrator.rs::run`，拒绝信号的
识别属于产生 `ExecToolCallOutput` 的具体 runtime；runtime 负责把原始输出转换为 typed
`SandboxErr::Denied`，`ToolOrchestrator` 只匹配该结构化错误并决定审批、审计和最多一次重试。
因此 Astro 保持 `terminal` / `code_exec` runtime 的 classifier，不把 stdout/stderr 文本启发式
上移到 orchestrator，也不让 orchestrator 重新解释普通 `ToolOutput`。此前“将 denial analysis
迁入 orchestrator”的清单项属于职责边界误判，现已纠正。managed subprocess network approval
仍需以命令网络代理产生的结构化 network-policy decision 为前提；代理缺失时继续 fail closed，
不通过解析 `curl` / `wget` 命令文本推断 host，也不把一次网络批准降级为 unrestricted network。

v2.25 Structured network denial contract 批次：`agent-types::network_policy` 引入与 Codex
同名的 `NetworkPolicyDecision`、`NetworkDecisionSource`、`NetworkApprovalProtocol` 和
`NetworkPolicyDecisionPayload`，包括 proxy 使用的 HTTPS protocol aliases 与
`is_ask_from_decider()` 判定。`SandboxErr::Denied` 现同时保留原始 `ExecToolCallOutput` 和可选
`network_policy_decision`；现有 terminal / code_exec 文件系统拒绝显式写入 `None`，未来代理可在
同一 typed boundary 写入结构化 decision。`ToolOrchestrator::run` 看到该字段后不再调用
`review_sandbox_denial`，因此不会把网络 allowlist miss 错误展示成“完全文件系统访问”审批，也不会
用 filesystem escalation 重试网络请求；原始输出仍经过统一 tool finalization 返回。
本批刻意不实现伪 host 授权：在命令网络代理能够按 call 归因请求、执行 host 级规则并为重试注入
临时 policy 前，所有结构化网络拒绝继续 fail closed。后续批次再实现 proxy、即时/延迟审批、
session host cache 与受限 network retry。

v2.26 Network proxy policy core 批次：新增 workspace crate `agent-network-proxy`（package
`network-proxy`），依 Codex 职责边界将域名策略从 `agent-sandbox` 和 `ToolOrchestrator`
中独立出来。`NetworkProxyState::host_blocked` 实现 allowlist-first：精确 host、
`*.example.com`（仅子域）、`**.example.com`（根域与子域）和 allow-only `*`；
deny 先于任何 allow，`**.*` 等展开后等价的全局 deny 同样拒绝。
`NetworkProxyState::new` 仅接受 `network.enabled=true` 的策略，禁止将“代理未启用”降级为
可被 decider 覆盖的 allowlist miss。当 `allow_local_binding=false` 时，通配符不能放行 loopback、
link-local、私网、保留地址或解析失败的域名；只有精确写出的本地 host/IP 可作为
显式例外，与 Codex 当前语义一致。同时引入 Codex 同名 `NetworkProtocol`、
`NetworkPolicyRequest`、`NetworkDecision` 与 `NetworkPolicyDecider`；允许 allowlist miss 交给
decider，但显式 deny 和本地地址防御不可被覆盖。`NetworkDecision::to_policy_decision_payload`
将 protocol/host/port/reason/source 无损转换为 v2.25 的 sandbox denial payload。本批仍未
启动 HTTP/HTTPS CONNECT/SOCKS listener，也未修改子进程 proxy env；因此配置层继续不宣称
host 规则已对真实 socket 生效。下一批才把 listener、blocked-request observer 与
per-execution attribution 接到 `SandboxRunner`。

v2.27 Managed CONNECT listener 批次：`agent-network-proxy` 新增与 Codex 同名的
`NetworkProxyBuilder`、`NetworkProxy` 和 `NetworkProxyHandle`。builder 必须接收
`Arc<NetworkProxyState>`，仅允许 loopback bind，并在 `build` 阶段立即保留 listener，
避免暴露尚未实际绑定的端口。`run` 只能取走 listener 一次；handle 提供
`wait` / `shutdown`，accept loop 内的 `JoinSet` 在 listener task 取消时同步取消活跃 tunnel。
`wait` 保留 task handle 直到 await 结束，因此等待方被取消时 `Drop` 仍会 abort listener，
不会留下脱离所有权的后台代理。accept loop 以 semaphore 将同时处理的客户端限制为 256，
超额连接返回 503；瞬时 accept 错误会短暂退避后继续监听，不直接终止代理。

listener 只解析单个最大 32 KiB、五秒超时的 HTTP/1.0 或 HTTP/1.1 request head，
校验 header name 语法，且方括号 authority 只允许 IPv6 literal。当前仅接受
`CONNECT host:port`；malformed authority、超大 header 和普通 absolute-form HTTP 分别以
400/431/405 关闭，不把同一 keep-alive 连接盲目转发给第二个 host。请求通过
`NetworkPolicyRequest { protocol: HttpsConnect, host, port, client_addr, method }` 进入现有
policy/decider；deny 返回 403 及 decision/source 响应头，allow 才允许连接。DNS 解析后
代理只使用已检查的 `SocketAddr` dial；公网 hostname 即使第二次解析到私网地址也
不能利用 allowlist 绕过，只有 `allow_local_binding=true` 或已由策略接受的同一精确
IP/`localhost` 例外可连接 non-public address。这类实际地址拒绝保留为 typed policy denial，
返回带 `proxy_state` 归因的 403，而 DNS 或 dial 故障才返回 502。成功后用 `copy_bidirectional` 转发，并保留
与 request head 同包到达的首批 tunnel bytes。

本批仍不实现 plain HTTP forwarding、SOCKS、MITM、blocked-request 持久化、子进程 proxy env
注入或 orchestrator network retry。因此仅能声明 CONNECT listener 的 socket enforcement 已存在，
不能声明 permission profile 的本地命令网络已端到端生效。下一批应将代理地址作为
attempt-scoped 环境注入 `SandboxRunner`，并将 403 policy decision 还原为 typed
`SandboxErr::Denied`。

v2.28 Attempt-scoped managed network 端到端批次：前台 `terminal action=run` 与
`code_exec` 的真实执行链已收口为：

```text
ToolOrchestrator::run
  -> StartedNetworkProxy::start
  -> SandboxAttempt.managed_network
  -> SandboxPolicy.managed_network (exact loopback port)
  -> ToolExecutionGrants
  -> ToolContext
  -> terminal/code_exec prepared env
  -> BlockedRequest
  -> SandboxErr::Denied.network_policy_decision
```

`ToolOrchestrator::run` 在 permission preflight 和 terminal 危险命令审批之后、构造初始
`SandboxAttempt` 之前启动一个 `StartedNetworkProxy`。`Arc` lease 仅归属该 tool
attempt，经 `SandboxAttempt -> ToolExecutionGrants -> ToolContext` 传递；文件系统
escalation 仅 clone 同一 lease，不启动第二个 listener。`SandboxPolicy` 只放行该
listener 已绑定的精确 loopback port，不恢复旧的任意网络权限。

启用门禁是全局 `network_proxy.enabled` 与当前选中 custom profile 自身
`network.enabled` 的逻辑与。网络策略只读取选中 leaf profile 的自有字段，不合并
`extends` 父链；未知 profile、全局开关关闭、leaf 未开启或
`:danger-full-access` 均不创建 managed proxy。非进程工具、terminal 的
`list/status/poll/wait/kill`、进程内 HTTP、MCP 与 provider 路径不受影响。

managed terminal 以完整父环境为基础覆盖大小写 proxy keys，并通过
`env_clear().envs(...)` 防止旧代理变量遗留；`code_exec` 先执行既有 secret scrub，
再注入 managed proxy，不会把被删除的凭证复制回子进程。`background=true`
在 spawn 前 fail closed，后台 job 不获得或持有 attempt lease。

代理只将 host policy 或 DNS rebinding 防御产生的拒绝记入有界 `BlockedRequest`
队列。进程结束后 runtime 排空队列并选择最新一条，转换为
`SandboxErr::Denied { network_policy_decision: Some(...) }`。上游 DNS/dial 失败的
502 不是 policy denial。orchestrator 对结构化网络拒绝立即收尾，不发起文件系统
审批或重试。本批仍不实现 plain HTTP forwarding、SOCKS、MITM、host 审批、
network retry 或 session/global proxy 状态。

### Phase D：AgentRuntimeManager 与 AgentControl（V2 已替换旧实现）

- root/subagent 都由 V2 runtime manager 启动 turn。
- `AgentControl` 在一棵 Agent 树内共享。
- 当前投影库为 `subagents-v2.db`；旧 V1 库只在启动迁移/归档时读取，不再是可调用 runtime。

### Phase E：Rollout 与记忆

- append-only rollout 先双写，再成为 replay contract。
- SQLite 转为 projection/query store。
- 上线两阶段 memory pipeline 和引用追踪。

### Phase F：Plugin/Extension

- 增加 `PluginManifest` bundle。
- 增加 typed `ExtensionRegistry`。
- 将现有三类 hook 总线迁移为兼容 adapter。

---

## 12. 兼容与验收

兼容要求：

- Rust 内部改名允许使用短期 `#[deprecated]` alias，但新代码只能使用目标名称。
- Tauri/gRPC/JSON 字段先增加新字段或 serde alias，再迁移前端，最后删除旧字段。
- SQLite 表和列不得通过直接 rename 破坏旧库，必须提供 schema migration。
- tool name 改名必须至少保留一个版本的 soft alias。
- rollout item 一经发布不得原地改变语义，只能增加新版本 item。

每阶段至少运行：

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test -p agent
cargo test -p subagents
cargo test -p tools
cd apps/desktop && npx tsc --noEmit
cd apps/desktop && npm run build
```

只有同时满足以下条件才能称为“与 Codex 生命周期对齐”：

1. 前台、Cron、SubAgent 共用 `SessionTask -> run_turn`。
2. tool call 使用生成该调用时的同一个 `StepContext`。
3. SubAgent 是可恢复的完整 `AgentThread`。
4. rollout 可重放，SQLite 可由 rollout 修复或重建关键 projection。
5. memory write 为两阶段后台流水线，read path 为 Extension。
6. Plugin bundle 与 typed Extension API 已区分。
7. 旧名称仅存在于兼容层和迁移代码。

---

## 13. 相关设计

- [agent-core 详细设计](01-agent-core详细设计.md)
- [agent-runtime 详细设计](02-agent-runtime详细设计.md)
- [子 Agent 派生详细设计](06-子Agent派生详细设计.md)
- [Hooks 系统详细设计](08-Hooks系统详细设计.md)
- [Checkpoint 与状态快照详细设计](09-Checkpoint与状态快照详细设计.md)
- [记忆系统详细设计](../03-记忆与上下文/01-记忆系统详细设计.md)
- [工具系统详细设计](../04-工具与扩展生态/02-工具系统详细设计.md)
- [Plugin SDK 开发者文档](../04-工具与扩展生态/04-PluginSDK开发者文档.md)
- [存储层详细设计](../06-安全与基础设施/09-存储层详细设计.md)
