# agent-runtime 详细设计

> 本文档由原 02-agent-runtime详细设计 和 34-agent-runtime执行引擎 合并而成

---

## 1. Crate 职责边界

`agent-runtime` 是 Astro Agent 的执行核心与并发调度层，负责将用户意图转化为实际的 LLM 调用和工具执行序列，同时管理所有**有副作用的长期运行任务**。

| 层 | Crate | 职责 |
| --- | --- | --- |
| 接口契约 | `agent-core` | Tool/Skill/Agent trait、Permission、RiskLevel |
| 执行编排 | **`agent-runtime`** | 主循环、计划确认、HumanGuard、子 Agent 派生 |
| 模型调用 | `agent-providers` | LLM 调用、流式输出、多模态 |
| 桌面集成 | `apps/desktop/src-tauri` | Tauri commands、AppState、事件广播 |

核心职责：

- **主执行循环**：`round_loop` 驱动的 LLM 调用 → 工具执行迭代
- **自适应规划**：`submit_plan` 工具调用检测与用户确认流程
- **子 Agent 派生**：`Supervisor` 管理子 Agent 生命周期、depth 限制、资源配额
- **工具执行沙箱**：WASM 插件执行（`wasmtime`）、Rhai 脚本执行、Shell 命令隔离
- **HumanGuard 审批流**：拦截 L2/L3 风险工具调用，等待人工确认后再放行
- **后台任务调度**：`MediaTaskPoller`、`MemoryDistiller`、`ForgetScheduler`
- **并发 Agent 会话管理**：多窗口/多工作区并发对话，各自独立的 `AgentContext`
- **重试与限流**：Provider 调用失败时的指数退避、全局 Token 速率窗口

`agent-runtime` 是唯一同时依赖 `agent-core` 和 `agent-providers` 的 crate，通过 `EventEmitter` trait 抽象与前端通信，不直接依赖 `tauri::AppHandle`。

---

## 2. 模块结构

```text
crates/agent-runtime/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── event_emitter.rs        # EventEmitter trait + TauriEventEmitter / MockEventEmitter / TerminalEventEmitter
    ├── executor.rs             # AgentExecutor：主循环入口（round_loop）
    ├── planner.rs              # 自适应规划：submit_plan 工具调用检测与确认流程
    ├── supervisor.rs           # 子 Agent 生命周期、depth 限制、资源配额
    ├── orchestrator.rs         # SkillOrchestrator：Skill 内部 Tool 序列协调
    ├── pending_queue.rs        # PendingQueue：运行中追加消息队列（内存 only）
    ├── context.rs              # AgentContext 构建与生命周期管理
    ├── retry.rs                # 指数退避重试 + RateLimiter
    ├── session/
    │   ├── mod.rs
    │   ├── manager.rs          # SessionManager — 多会话并发管理
    │   └── handle.rs           # SessionHandle — 单会话操作入口
    ├── guard/
    │   ├── mod.rs
    │   ├── human_guard.rs      # HumanGuard：L1/L2/L3 + YOLO 门控
    │   ├── whitelist.rs        # 工具白名单（workspace.yaml 配置）
    │   └── state.rs            # GuardState 枚举（Running/Pending/Paused/Takeover）
    ├── sandbox/
    │   ├── mod.rs
    │   ├── wasm_sandbox.rs     # WASM 沙箱（复杂 Skill 隔离执行，wasmtime）
    │   ├── rhai_sandbox.rs     # Rhai 脚本沙箱（轻量动态工具）
    │   └── shell.rs            # Shell 执行（限时 + 输出截断）
    └── background/
        ├── mod.rs
        ├── media_poller.rs     # MediaTaskPoller — 视频/音乐轮询
        ├── distiller.rs        # MemoryDistiller — 对话蒸馏
        └── forget_scheduler.rs # ForgetScheduler — Weibull 衰减触发
```

---

## 3. AgentState 状态机

Agent 执行过程的完整状态枚举。`GuardState`（§9.3）描述的是 HumanGuard 审批子状态，`AgentState` 则是 Agent 整体生命周期的顶层状态机。

```rust
// crates/agent-runtime/src/executor.rs

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentState {
    Idle,                        // 空闲，等待用户输入
    StreamingResponse,           // 正在接收 LLM 流式响应
    WaitingForPlanConfirmation,  // 等待用户确认执行计划
    ExecutingTool,               // 正在执行工具调用
    WaitingForApproval,          // 等待用户审批 L2/L3 工具
    SpawningAgent,               // 正在派生子 Agent
    Paused,                      // 用户主动暂停
    Takeover,                    // 用户完全接管
    Done,                        // 任务完成
    Failed,                      // 任务失败
    Cancelled,                   // 用户取消
}
```

### 状态转移图

```text
Idle → StreamingResponse → ExecutingTool → [WaitingForApproval] → ExecutingTool
                         → WaitingForPlanConfirmation → ExecutingTool
                         → SpawningAgent → ExecutingTool
                         → Done
     → Paused → (resume) → StreamingResponse
     → Cancelled
Any → Failed
```

状态转移说明：

- **Idle → StreamingResponse**：用户提交消息后进入 LLM 流式调用阶段
- **StreamingResponse → ExecutingTool**：LLM 返回工具调用请求，开始执行工具
- **ExecutingTool → WaitingForApproval**：工具为 L2/L3 风险时进入审批等待（HumanGuard）
- **WaitingForApproval → ExecutingTool**：用户批准后继续执行
- **StreamingResponse → WaitingForPlanConfirmation**：LLM 调用 `submit_plan` 工具时进入计划确认
- **WaitingForPlanConfirmation → ExecutingTool**：用户确认计划后按步骤执行
- **ExecutingTool → SpawningAgent**：工具调用 `delegate_task` 时派生子 Agent
- **SpawningAgent → ExecutingTool**：子 Agent 完成后继续父 Agent 执行
- **StreamingResponse → Done**：LLM 无更多工具调用，任务完成
- **Any → Paused**：用户主动暂停，任意状态均可进入
- **Paused → StreamingResponse**：用户恢复后重新进入 LLM 调用
- **Any → Cancelled**：用户取消任务
- **Any → Failed**：任何阶段发生不可恢复错误

前端通过监听 `agent_state_changed` 事件同步 UI 状态（如按钮可用性、进度指示器）。`AgentExecutor` 在每次状态变迁时通过 `EventEmitter::emit("agent_state_changed", ...)` 广播。

---

## 4. 主执行循环（round_loop）

```text
用户请求到来
    │
    ▼
AgentExecutor::round_loop()
    │
    ├─── ① 注入 pending 队列消息（PendingQueue::drain()）
    │         → 追加为 ContextSlot::Regular 消息
    │
    ├─── ② 构建上下文
    │         → ContextBuilder 按 Pinned > Regular > Ephemeral 填充
    │         → 注入自适应规划 System Prompt 片段
    │         → TokenBudget 检查（超限则 downgrade/block）
    │
    ├─── ③ LLM 调用（流式）
    │         → BudgetManager::check_before_call()（超限则 block + 强制关闭 YOLO）
    │         → 流式 delta 逐步 emit "text_delta" 事件给前端
    │
    ├─── ④ 响应解析
    │         ├─ 含 submit_plan 工具调用？
    │         │   ├─ YES → Planner::request_confirmation()
    │         │   │         推送 plan_confirmation_required 事件
    │         │   │         等待用户"确认 / 修改后确认 / 取消"
    │         │   │         取消 → 终止本轮，返回 Cancelled
    │         │   └─ NO  → 直接进入工具执行
    │         │
    │         └─ 工具调用列表（ToolCall[]）
    │
    ├─── ⑤ 工具执行（每个 ToolCall 串行或并行）
    │         → HumanGuard::check()（YOLO / L1 / L2 / L3 判断）
    │         → ToolRegistry::execute()
    │         → 结果写回 context（ContextSlot::Ephemeral）
    │         → emit "tool_call" / "tool_result" 事件
    │
    │    > `round_loop` 的第 5 步「调用 LLM + 工具循环」实际委托给 `agent-core::run_agent_turn()`。
    │
    ├─── ⑥ 无更多工具调用？
    │         ├─ YES → 本轮结束，emit "done" 事件，退出循环
    │         └─ NO  → 回到 ①（下一轮，再次注入 pending 消息）
    │
    └─── ⑦ 反思（可选，后台异步）
              → 写 TaskTrace → 进化引擎异步消费
```

### Rust 核心结构

```rust
// crates/agent-runtime/src/executor.rs

pub struct AgentExecutor {
    context: AgentContext,
    pending_queue: Arc<PendingQueue>,
    human_guard: Arc<HumanGuard>,
    planner: Planner,
    token_budget: Arc<TokenBudget>,
    budget_manager: Arc<BudgetManager>,
    emitter: Arc<dyn EventEmitter>,
}

impl AgentExecutor {
    pub async fn round_loop(&self) -> Result<RoundOutcome> {
        // max_tool_rounds 默认 25，可通过 workspace.yaml 配置
        let max_tool_rounds = self.context.max_tool_rounds;
        let mut round = 0;

        loop {
            round += 1;

            // ⓪ max_tool_rounds 守卫：软着陆 + 硬上限
            if round >= max_tool_rounds - 5 {
                // 注入系统消息提醒 LLM 软着陆
                self.context.inject_system_message(
                    "你即将达到工具调用上限，请在接下来的调用中完成任务或向用户汇报进展"
                );
            }
            if round >= max_tool_rounds {
                return Err(AgentError::ToolRoundsExceeded { max: max_tool_rounds, reached: round });
            }

            // ① pending 注入
            for msg in self.pending_queue.drain().await {
                self.context.add_message(Message::user(msg.content, ContextSlot::Regular));
            }

            // ② 上下文构建 + ③ LLM 调用
            self.budget_manager.check_before_call(&self.context.workspace_id, 0.0).await?;
            let response = self.context.llm_call_stream(&self.emitter).await?;

            // ④ 自适应规划确认
            if let Some(plan) = self.planner.extract(&response) {
                match self.planner.request_confirmation(&plan, &self.emitter).await? {
                    PlanOutcome::Confirmed(plan) => { /* 继续 */ }
                    PlanOutcome::Cancelled => return Ok(RoundOutcome::Cancelled),
                }
            }

            // ⑤ 工具执行
            let tool_calls = response.tool_calls();
            if tool_calls.is_empty() {
                self.emitter.emit("done", serde_json::to_value(DoneMetrics::from(&response)).unwrap()).await.ok();
                return Ok(RoundOutcome::Done);
            }

            for call in tool_calls {
                let outcome = self.human_guard.check(&call.tool, &call.input).await?;

                // 拒绝反馈注入：将拒绝信息作为 ToolResult 回填到 LLM 上下文，
                // 避免 LLM 不知道操作被拒绝而陷入无限重试循环。
                if outcome == ApprovalOutcome::Rejected {
                    let rejection_result = ToolResult {
                        call_id: call.id.clone(),
                        is_error: true,
                        output: vec![ToolContent::Text(
                            format!("用户拒绝了此操作: {}。请调整策略或询问用户。", call.tool_name)
                        )],
                    };
                    self.context.messages.push(Message::tool_result(rejection_result));
                    continue;
                }

                let result = self.context.tools.execute(&call.tool_name, call.input).await;
                self.emitter.emit("tool_result", serde_json::to_value(&result).unwrap()).await.ok();
                self.context.add_tool_result(call.id, result);
            }

            // ⑤.1 重复工具调用检测
            self.detect_repeated_tool_calls(&tool_calls);
        }
    }
}
```

### 重复工具调用检测

在 round_loop 中维护最近调用记录，检测 LLM 是否陷入推理死循环：

```rust
// 在 round_loop 外部初始化
let mut recent_calls: VecDeque<u64> = VecDeque::with_capacity(6);

// 每轮工具调用后记录
for call in &tool_calls {
    let call_hash = hash(&(call.name.clone(), call.arguments.to_string()));
    recent_calls.push_back(call_hash);
    if recent_calls.len() > 6 { recent_calls.pop_front(); }
}

// 检测连续 3 次相同调用
if recent_calls.len() >= 3 {
    let last = recent_calls.back().unwrap();
    let repeated = recent_calls.iter().rev().take(3).all(|h| h == last);
    if repeated {
        context.inject_system_message(
            "检测到你连续 3 次调用了相同的工具和参数。请改变策略：尝试不同的方法、询问用户或直接给出当前已有的结果。"
        );
    }
}
```

> 这避免了 LLM 反复读同一文件或反复搜索相同关键词的 token 浪费场景。

### 暂停感知的主循环

Agent 主循环中 LLM 调用处需挂 `tokio::select!` 监听暂停信号，实现即时响应：

```rust
loop {
    // LLM 流式调用：暂停信号到来时立即 abort
    let response = tokio::select! {
        result = provider.chat_stream(&messages, &mut event_tx) => result?,
        _ = guard.paused() => {
            // 通知前端清空 streaming buffer（消息尚未提交到 DB，状态干净）
            event_tx.send(AgentEvent::StreamAborted).await.ok();
            // 阻塞等待恢复
            guard.resumed().await;
            continue;  // 重新发起 LLM 调用，history 未变
        }
    };

    // 工具执行：逐个挂 select!
    for tool_call in &response.tool_calls {
        let result = tokio::select! {
            r = execute_tool(tool_call, ctx) => r,
            _ = guard.paused() => {
                // 可取消工具（shell/http）在 execute_tool 内部响应 cancel_token
                // 不可取消工具（DB 写）等其自然完成，inject cancelled 结果
                ToolResult::cancelled(&tool_call.id)
            }
        };
        append_tool_result(ctx, result).await;
    }
}
```

---

## 5. 多会话管理

### 5.1 SessionManager

持有所有活跃会话，并提供跨会话查询与取消能力：

```rust
pub struct SessionManager {
    sessions: RwLock<HashMap<String, SessionHandle>>,  // conversation_id → handle
    pool: SqlitePool,
    provider_registry: Arc<ProviderRegistry>,
}

impl SessionManager {
    /// 开启新会话，返回 SessionHandle
    pub async fn new_session(&self, conv_id: &str, workspace_id: &str) -> SessionHandle { ... }

    /// 取消正在运行的会话（发送 CancellationToken）
    pub async fn cancel(&self, conv_id: &str) -> bool { ... }

    /// 查询当前活跃会话数
    pub fn active_count(&self) -> usize { ... }
}
```

### 5.2 SessionHandle

代表单个对话会话，封装 `AgentContext` 与 Tokio task 句柄：

```rust
pub struct SessionHandle {
    pub conv_id: String,
    cancel_token: CancellationToken,
    task: JoinHandle<anyhow::Result<()>>,
    event_tx: mpsc::Sender<AgentEvent>,
    pub pending_user_messages: Arc<Mutex<VecDeque<PendingMessage>>>,
}

impl SessionHandle {
    /// 向运行中的 Agent 注入新用户消息（追加到 message_history）
    pub async fn send_message(&self, msg: Message) -> anyhow::Result<()> { ... }

    /// 中止当前工具调用轮次（不关闭整个会话）
    pub async fn interrupt(&self) { ... }
}
```

并发限制：单工作区最多 3 个并发对话；超过时新请求排队，队满（>10）拒绝并返回 `-32000`。

---

## 6. 自适应规划（Planner）

### 6.1 触发机制

> **架构决策**：放弃 `<agent_plan>` XML 标签的文本解析方案（存在代码块误触发、UTF-8 偏移 panic、嵌套标签等边缘问题），改为通过 `submit_plan` 内置工具提交计划。LLM 使用结构化的 tool_call 提交计划，消除了文本解析的脆弱性。详见 `01-agent-core详细设计.md` Section 4 的 `submit_plan` 工具定义。

LLM 通过调用 `submit_plan` 工具触发计划确认流程。该工具始终注册在可用工具列表中，触发条件（由 LLM 自判断）：

- 预计工具调用步骤 > 3 步
- 涉及不可逆操作（DeleteFile、NetworkWrite 等）
- 跨多个工作区或调用子 Agent

### 6.2 System Prompt 注入

AgentContext 初始化时注入的 System Prompt 追加片段：

```text
当任务预计需要 3 步以上工具调用、或涉及不可逆操作（写文件、执行代码、
发送外部请求、删除数据）时，请先调用 submit_plan 工具提交执行计划，
等待用户确认后再开始执行。
简单的查询或单步只读操作直接执行，无需提交计划。
```

### 6.3 确认流程

```text
LLM 响应含 submit_plan tool_call
    │
    ▼
Planner::request_confirmation()
    → 从 submit_plan 的结构化参数中提取计划步骤
    → emit plan_confirmation_required {steps, estimated_tools, risk_summary}
    │
    ▼
前端渲染计划确认对话框
    ├─ [确认] → 继续执行（步骤按原计划）
    ├─ [修改后确认] → 用户编辑计划文本 → 重新注入 LLM context 后继续
    └─ [取消] → emit plan_cancelled → 本轮终止
```

### 6.4 Rust 侧检测与等待确认

```rust
// round_loop 中，parse_response 检测到 submit_plan 工具调用后触发确认流程
if parsed.has_plan {
    // submit_plan 的结构化参数中已包含 plan 数组和 reasoning
    let plan_call = parsed.tool_calls.iter()
        .find(|tc| tc.name == "submit_plan")
        .expect("has_plan=true implies submit_plan exists");

    let (tx, rx) = oneshot::channel::<PlanDecision>();
    let request_id = Uuid::new_v4();

    // 注册等待句柄，推送计划确认事件到前端
    self.pending_plans.insert(request_id, tx);
    app.emit("plan_confirmation_required", PlanConfirmationEvent {
        request_id,
        plan: serde_json::from_value(plan_call.arguments.clone())?,
        conversation_id: context.conversation_id.clone(),
    }).ok();

    // 阻塞等待用户决定（同样受 pause 信号监听）
    let decision = tokio::select! {
        d = rx => d?,
        _ = pause_changed() => return Ok(RoundResult::Paused),
    };

    match decision {
        PlanDecision::Confirmed { edited_plan } => {
            // 将用户确认（含可能的编辑）注入下一轮 context
            context.push_user(format!(
                "[计划已确认，请严格按以下步骤执行]\n{edited_plan}"
            ));
        }
        PlanDecision::Cancelled => {
            return Ok(RoundResult::Cancelled);
        }
    }
}
```

### 6.5 Tauri Commands

```rust
#[tauri::command]
pub async fn confirm_plan(
    state: State<'_, AppState>,
    request_id: Uuid,
    edited_plan: String,
) -> Result<(), AppError> {
    if let Some((_, tx)) = state.runtime.pending_plans.remove(&request_id) {
        let _ = tx.send(PlanDecision::Confirmed { edited_plan });
    }
    Ok(())
}

#[tauri::command]
pub async fn cancel_plan(
    state: State<'_, AppState>,
    request_id: Uuid,
) -> Result<(), AppError> {
    if let Some((_, tx)) = state.runtime.pending_plans.remove(&request_id) {
        let _ = tx.send(PlanDecision::Cancelled);
    }
    Ok(())
}
```

### 6.6 与 HumanGuard 的区别

计划确认在**工具执行前**，粒度为"整轮任务"；HumanGuard 在**每个工具调用时**，粒度为"单次操作"。两者正交，不互相替代（见 04-人工接管设计.md Section 1.3）。

---

## 7. 运行中 pending 消息队列（PendingQueue）

用户在 Agent 运行期间发送的消息暂存于内存队列，当前轮次（本轮 LLM 回复 + 全部工具调用）结束后统一注入下一轮 context，LLM 据此调整后续行为。

### 数据结构

挂载在 SessionHandle 上：

```rust
pub struct PendingMessage {
    pub content: String,
    pub queued_at: u64,   // ms timestamp
}
```

### 注入时机

每轮开始前调用：

```rust
async fn drain_pending_into_context(&self, context: &mut AgentContext) {
    let messages: Vec<PendingMessage> = {
        let mut q = self.pending_user_messages.lock().await;
        q.drain(..).collect()
    };
    if messages.is_empty() { return; }

    let combined = messages.iter()
        .map(|m| m.content.as_str())
        .collect::<Vec<_>>()
        .join("\n---\n");
    context.push_user(format!(
        "[用户在上一轮执行中追加的补充内容]\n{combined}"
    ));
}
```

### Tauri Command（前端消息入队）

```rust
#[tauri::command]
pub async fn append_pending_message(
    state: State<'_, AppState>,
    conversation_id: String,
    content: String,
) -> Result<(), AppError> {
    let session = state.runtime.get_session(&conversation_id)
        .ok_or(AppError::SessionNotFound)?;
    session.pending_user_messages.lock().await.push_back(PendingMessage {
        content,
        queued_at: now_ms(),
    });
    Ok(())
}
```

### 与暂停的交互

用户点击"暂停"时，pending 队列消息**立即**注入 context（不等轮次结束），确保暂停后 LLM 重新规划时能感知用户追加的意图：

```rust
pub async fn pause(&self, context: &mut AgentContext) {
    // 1. 广播暂停信号
    self.state_tx.send(GuardState::Paused).ok();
    // 2. 立即将 pending 消息注入 context
    self.drain_pending_into_context(context).await;
}
```

---

## 8. 子 Agent 派生（Supervisor）

```rust
// crates/agent-runtime/src/supervisor.rs

pub struct Supervisor {
    /// 当前活跃的子 Agent
    children: DashMap<Uuid, Arc<AgentExecutor>>,
    /// 全局并发限制
    semaphore: Arc<Semaphore>,
}

impl Supervisor {
    /// 派生子 Agent（depth 超限则拒绝）
    pub async fn spawn(
        &self,
        role: String,
        task: String,
        parent_ctx: &AgentContext,
    ) -> Result<Uuid, SupervisorError> {
        let child_depth = parent_ctx.depth + 1;

        // depth > 3 绝对拒绝（见 06-安全边界.md）
        if child_depth > 3 {
            return Err(SupervisorError::DepthExceeded { max: 3, current: child_depth });
        }

        // 子 Agent 权限 = 父级权限集的交集
        let child_permissions = parent_ctx.permissions.intersect(
            &PermissionSet::from_role(&role)
        );

        let child_ctx = parent_ctx.derive_child(child_depth, child_permissions);
        let child = Arc::new(AgentExecutor::new(child_ctx, role, task));
        let id = Uuid::new_v4();
        self.children.insert(id, child.clone());

        // 在 semaphore 控制下并发执行
        let sem = self.semaphore.clone();
        tokio::spawn(async move {
            let _permit = sem.acquire().await;
            child.round_loop().await
        });

        Ok(id)
    }
}
```

---

## 9. HumanGuard 审批流

### 9.1 风险分级

| 级别 | 场景 | 需要审批 |
| --- | --- | --- |
| L1 低风险 | file_read, memory_search, web_fetch（只读） | 无（自动执行） |
| L2 中风险 | file_write, file_edit, shell_exec（白名单命令） | 需确认（30s 超时自动拒绝，跳过该步骤继续） |
| L3 高风险 | shell_exec（任意命令）、file_delete | 必须手动确认，无超时 |

### 9.2 EventEmitter 抽象

> **架构决策**：agent-runtime 通过 `EventEmitter` trait 抽象与外部环境的事件通信，不直接依赖 `tauri::AppHandle`。这样 agent-runtime 不再依赖 `tauri::AppHandle`，可在 CLI 模式（`TerminalEventEmitter`）、单元测试（`MockEventEmitter`）和未来 Server 部署中复用。

```rust
// crates/agent-runtime/src/event_emitter.rs

/// 事件发射器抽象：解耦 agent-runtime 与 Tauri 框架
/// Tauri 环境使用 TauriEventEmitter，测试使用 MockEventEmitter，CLI 使用 TerminalEventEmitter
#[async_trait]
pub trait EventEmitter: Send + Sync {
    /// 发射通用事件
    async fn emit(&self, event: &str, payload: serde_json::Value) -> Result<()>;
    /// 请求用户审批（阻塞直到用户响应或超时）
    async fn request_approval(&self, req: ApprovalRequest) -> Result<ApprovalResponse>;
    /// 发射流式 Token
    async fn emit_token(&self, conversation_id: &str, token: &str) -> Result<()>;
}

// --- Tauri 实现 ---
pub struct TauriEventEmitter {
    app: tauri::AppHandle,
}

#[async_trait]
impl EventEmitter for TauriEventEmitter {
    async fn emit(&self, event: &str, payload: serde_json::Value) -> Result<()> {
        self.app.emit(event, payload).map_err(Into::into)
    }
    async fn request_approval(&self, req: ApprovalRequest) -> Result<ApprovalResponse> {
        // 通过 Tauri 事件推送审批弹窗，等待前端回调
        self.app.emit("approval_required", &req)?;
        req.wait_response().await
    }
    async fn emit_token(&self, conversation_id: &str, token: &str) -> Result<()> {
        self.app.emit("text_delta", serde_json::json!({
            "conversation_id": conversation_id, "delta": token
        })).map_err(Into::into)
    }
}

// --- 测试用 Mock ---
pub struct MockEventEmitter {
    pub events: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
    pub auto_approve: bool,
}

// --- CLI 终端实现 ---
pub struct TerminalEventEmitter { /* 通过 stdin/stdout 与用户交互 */ }
```

### 9.3 HumanGuard 结构

> 规范结构定义，与 `04-人工接管设计.md` Section 4.1 和 [`02-人工接管与授权详细设计.md`](../06-安全与基础设施/02-人工接管与授权详细设计.md) Section 4.3 保持一致。

```rust
// crates/agent-runtime/src/guard/state.rs

#[derive(Clone, Debug, PartialEq)]
pub enum GuardState {
    Running,      // 正常执行
    Pending,      // 等待用户审批
    Paused,       // 用户主动暂停
    Takeover,     // 人工完全接管
}
```

```rust
// crates/agent-runtime/src/guard/human_guard.rs
pub struct HumanGuard {
    state: Arc<RwLock<GuardState>>,
    state_tx: watch::Sender<GuardState>,
    state_rx: watch::Receiver<GuardState>,
    config: HumanGuardConfig,
    whitelist: Whitelist,
    /// per-request oneshot：key = request_id，value = 审批结果 Sender
    pending_decisions: DashMap<Uuid, oneshot::Sender<ApprovalResponse>>,
    /// 事件发射器（替代原 Tauri AppHandle，实现环境解耦）
    emitter: Arc<dyn EventEmitter>,
    /// SQLite 连接池，写入 audit_logs 表
    pool: SqlitePool,
}

impl HumanGuard {
    /// L1 直通，L2 弹窗 30s 超时自动拒绝，L3 弹窗无超时必须手动确认
    pub async fn check(&self, tool: &dyn Tool, input: &ToolInput)
        -> Result<ApprovalOutcome, GuardError>;

    /// 切换暂停状态，通过 state_tx 广播（LLM 调用 + 工具执行 select! 监听）
    pub fn set_paused(&self, paused: bool) {
        let new_state = if paused { GuardState::Paused } else { GuardState::Running };
        *self.state.write().unwrap() = new_state.clone();
        let _ = self.state_tx.send(new_state);
    }

    /// 等待暂停信号的 Future，供 select! 使用
    pub fn paused(&self) -> impl Future<Output = ()> + '_ {
        let mut rx = self.state_rx.clone();
        async move {
            let _ = rx.wait_for(|s| *s == GuardState::Paused).await;
        }
    }

    /// 等待恢复信号的 Future
    pub fn resumed(&self) -> impl Future<Output = ()> + '_ {
        let mut rx = self.state_rx.clone();
        async move {
            let _ = rx.wait_for(|s| *s == GuardState::Running).await;
        }
    }

    /// 前端审批回调（approve_action / reject_action Tauri Command 调用）
    pub fn resolve(&self, request_id: Uuid, response: ApprovalResponse) {
        if let Some((_, tx)) = self.pending_decisions.remove(&request_id) {
            let _ = tx.send(response);
        }
    }
}
```

### 9.4 YOLO 开关（v0.3 预留）

YOLO 开关叠加在自适应执行上，临时跳过 L2/L3 审批弹窗，实现零打断执行。

设计约束：

- 首次启用需前端弹出免责确认，用户勾选"我了解跳过审批的风险"后才生效
- 费用预算超限时后端强制关闭，写入 `AppState.yolo_mode = false` 并推送 `yolo_force_disabled` 事件通知前端
- 不跨会话持久化：每次应用启动默认 `false`

```rust
// AppState 中
pub yolo_mode: AtomicBool,

// HumanGuard::check() 中，risk_level 判断前插入：
// Critical 风险不可跳过，即使 YOLO 模式
if risk_level == RiskLevel::Critical {
    // 走正常审批流程
} else if self.app_state.yolo_mode.load(Ordering::Relaxed) {
    return Ok(ApprovalDecision::Approved { modified_args: None });
}
```

YOLO 与 BudgetManager 的联动：`BudgetManager::check_before_call()` 检测到费用超限时，会通过 `HumanGuard` 强制关闭 YOLO 模式（见集成关系图）。

---

## 10. 工具执行沙箱

### 10.1 WASM 沙箱

使用 `wasmtime` 执行第三方插件，内存上限 64MB，CPU 时间上限 10s（通过 `Epoch` 机制实现）：

```rust
pub struct WasmSandbox {
    engine: Engine,    // 全局共享，跨会话复用
}

impl WasmSandbox {
    pub async fn execute(&self, wasm_bytes: &[u8], input: &str) -> anyhow::Result<String> {
        let mut store = Store::new(&self.engine, ());
        store.set_epoch_deadline(10);          // 10 epoch ticks = ~10s
        store.limiter(|_| StoreLimiter::new(64 * 1024 * 1024, u32::MAX));

        let module = Module::new(&self.engine, wasm_bytes)?;
        // 仅导出 WASI preview2 + 自定义 astro_host 接口，禁止任意文件系统访问
        let linker = build_restricted_linker(&self.engine);
        let instance = linker.instantiate_async(&mut store, &module).await?;
        let run = instance.get_typed_func::<(), ()>(&mut store, "run")?;
        run.call_async(&mut store, ()).await?;
        Ok(read_output(&mut store))
    }
}
```

### 10.2 Shell 沙箱

```rust
pub async fn exec_shell(cmd: &str, timeout_ms: u64) -> anyhow::Result<ShellOutput> {
    let mut child = Command::new("sh")
        .arg("-c").arg(cmd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let result = timeout(
        Duration::from_millis(timeout_ms.max(30_000)),
        child.wait_with_output(),
    ).await;

    match result {
        Ok(Ok(output)) => Ok(ShellOutput {
            stdout: truncate_utf8(output.stdout, 1_000_000),  // 1MB 截断
            stderr: truncate_utf8(output.stderr, 100_000),
            exit_code: output.status.code().unwrap_or(-1),
        }),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => {
            let _ = child.kill().await;
            Err(anyhow!("Shell 命令超时（{}ms）", timeout_ms))
        }
    }
}
```

支持可取消的 Shell 执行（用于暂停场景）：

```rust
pub async fn exec_shell_cancellable(
    cmd: &str,
    timeout_ms: u64,
    cancel: CancellationToken,
) -> anyhow::Result<ShellOutput> {
    let mut child = Command::new("sh").arg("-c").arg(cmd)
        .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;

    tokio::select! {
        result = child.wait_with_output() => { /* 正常完成 */ Ok(result?.into()) }
        _ = tokio::time::sleep(Duration::from_millis(timeout_ms)) => {
            let _ = child.kill().await;
            Err(anyhow!("Shell 命令超时"))
        }
        _ = cancel.cancelled() => {
            let _ = child.kill().await;
            Err(anyhow!("Shell 命令已取消"))
        }
    }
}
```

---

## 11. 暂停与网络容错

### 11.1 暂停即时响应

暂停机制的完整实现见 §4（消费端 `select!` 循环）和 §9.3（`HumanGuard` 的 `set_paused`/`paused`/`resumed` 方法）。

### 11.2 流中断与前端状态同步

LLM 调用被 abort 时，已推送给前端的 `token_chunk` 形成"半条消息"。前端监听 `stream_aborted` 事件后清空 streaming buffer，避免与下一次完整响应冲突：

```typescript
// ConversationView.tsx
useEffect(() => {
    const unlisten = listen<{ convId: string }>('stream_aborted', ({ payload }) => {
        if (payload.convId !== conversationId) return;
        // 清空未提交的 streaming buffer
        useConversationStore.getState().clearStreamBuffer(conversationId);
        useConversationStore.getState().clearThinkingBuffer(conversationId);
    });
    return () => { unlisten.then(f => f()); };
}, [conversationId]);
```

**Rust 侧保证**：`provider.chat_stream()` 被 abort 时，assistant 消息**不写入** `messages` 表（写入发生在 stream 完整结束后），因此 abort 后 history 状态干净，重新发起 LLM 调用不会产生历史冗余。

### 11.3 网络失败分级处理

网络恢复对齐 Codex，并分为互不混用的两层：

1. `agent-providers` request 层：默认在首次请求后最多重试 4 次，200ms 指数退避并带
   `0.9..1.1` jitter；只处理 connection/timeout/network/5xx。
2. `agent-core::streaming` sampling 层：request retry 耗尽后，前台交互式 Turn 对明确的
   `ConnectionFailed` 按 5/10/20/40/60 秒持续等待，60 秒封顶，直到恢复或用户中断。

各错误类型采用以下策略：

| 错误类型 | 处理策略 |
| --- | --- |
| DNS/TCP/TLS/CONNECT | 前台持续重连；后台有界重试；不切换 Provider |
| 请求超时 | request 层有限重试，随后进入有界 stream retry |
| 500 / 503 等 5xx | request 层有限重试；耗尽后可按显式 fallback 链切换 |
| 429 | 不进入通用 request retry；按服务端提示和显式 fallback 产品策略处理 |
| 400 / 上下文 / 内容拒绝 | 不重试、不 fallback，立即失败 |
| 401 / 403 | 不重试；保留显式 fallback 产品策略 |
| 用户取消 | 立即 `TurnAborted` |

前台断网时发送专用、非终态 `StreamError`：

```rust
StreamErrorEvent {
    message: "Reconnecting... waiting for network".into(),
    codex_error_info: Some(CodexErrorInfo::ResponseStreamDisconnected {
        http_status_code: None,
    }),
    additional_details: Some(connection_error.to_string()),
}
```

`StreamError` 不写入 assistant 内容、不结束 Turn、不切换模型，也不显示永久错误 Toast。
对外通知使用 `ErrorNotification.will_retry=true`，前端更新当前 Turn 的单条瞬时状态；
网络恢复后自动继续同一 `turn_id`。等待实现必须同时监听
Turn cancellation 和 pause/interrupt，不能使用不可取消的裸 `sleep`。

完整契约见
[Codex 网络恢复对齐设计](../../superpowers/specs/2026-08-28-codex-network-recovery-alignment-design.md)。

### 11.4 工具幂等性保护

网络重试或 Agent 恢复时，同一个 `tool_call_id` 可能被再次执行。对有副作用的工具（`shell_exec`、`http_request`、`file_write`），执行前先查 `messages` 表：

```rust
pub async fn execute_idempotent(
    tool_call: &ToolCall,
    pool: &SqlitePool,
    ctx: &AgentContext,
) -> ToolResult {
    // 同一 tool_call_id 已有结果：直接返回，不重复执行
    if let Ok(Some(existing)) = MessageRepo::new(pool)
        .find_tool_result(&tool_call.id).await
    {
        tracing::debug!("工具 {} 幂等命中，跳过执行", tool_call.id);
        return existing.into();
    }

    execute_tool(tool_call, ctx).await
}
```

`find_tool_result` 查询：

```sql
SELECT content FROM messages
WHERE tool_call_id = ? AND role = 'tool'
LIMIT 1
```

只读工具（`file_read`、`memory_search`）不需要幂等保护，重复执行无副作用。

### 11.5 容错行为汇总

| 场景 | 设计行为 | 章节 |
| --- | --- | --- |
| 暂停信号 | `watch::Sender<GuardState>` 广播，LLM 调用与工具执行通过 `select!` 即时响应 | §4, §9.3 |
| LLM 流式中断 | 发送 `StreamAborted` 事件，前端清空 streaming buffer；assistant 消息不写入数据库 | §11.2 |
| 明确连接失败 | request 层有限重试；前台持续等待网络恢复，后台有界失败；不进入 Provider fallback | §11.3 |
| 5xx/超时/普通流错误 | request/stream 两层有界重试；符合显式策略的 5xx 可进入 fallback | §11.3 |
| 不可重试错误（400/401/403） | 立即失败，不重试 | §11.3 |
| 工具重复执行 | 执行前查 `messages` 表，同 `tool_call_id` 已有结果则跳过 | §11.4 |

---

## 12. 后台任务调度

所有后台任务在 Tauri 主进程启动时通过 `tokio::spawn` 启动，持有 `SqlitePool` 和 `AppHandle` 引用。

### 12.1 MediaTaskPoller

轮询 `media_tasks` 表中 `status = 'pending' OR 'processing'` 的任务，调用对应 Provider 的 `poll` 接口：

```rust
pub async fn run_media_poller(pool: SqlitePool, registry: Arc<ProviderRegistry>, app: AppHandle) {
    let mut interval = tokio::time::interval(Duration::from_secs(3));
    loop {
        interval.tick().await;
        let tasks = MediaTaskRepo::new(&pool).list_pending().await.unwrap_or_default();
        for task in tasks {
            let pool = pool.clone();
            let registry = registry.clone();
            let app = app.clone();
            tokio::spawn(async move {
                if let Ok(result) = registry.video_client(&task.provider)
                    .poll(&task.task_id).await
                {
                    MediaTaskRepo::new(&pool).update_status(&task.task_id, &result).await.ok();
                    app.emit("media_task_update", MediaTaskEvent::from(result)).ok();
                }
            });
        }
    }
}
```

### 12.2 MemoryDistiller

每次对话结束后，将本轮消息蒸馏为记忆条目。采用批处理（每次处理 20 条消息）：

- 调用 LLM 提炼关键事件/模式，噪声过滤后写入 `memory_entries`
- 生成嵌入向量写入 `embeddings` 虚拟表
- 更新 USER.md diff（偏好/习惯类记忆）

### 12.3 ForgetScheduler

每小时运行一次，扫描 Weibull 分数低于阈值的记忆条目并软删除：

```rust
pub async fn run_forget_scheduler(pool: SqlitePool) {
    let mut interval = tokio::time::interval(Duration::from_secs(3600));
    loop {
        interval.tick().await;
        let repo = MemoryRepo::new(&pool);
        // 扫描所有工作区的活跃记忆
        let candidates = repo.scan_for_forgetting_all(WEIBULL_THRESHOLD).await
            .unwrap_or_default();
        for entry in candidates {
            if weibull_score(&entry) < WEIBULL_THRESHOLD {
                repo.soft_expire(&entry.id).await.ok();
            }
        }
    }
}

// Weibull 衰减分数：score = exp(-(age_days / half_life)^1.5) * frequency_boost
fn weibull_score(entry: &MemoryEntry) -> f64 {
    let age_days = (now_ms() - entry.valid_from) as f64 / 86_400_000.0;
    let effective_half_life = entry.half_life_days as f64
        * (1.0 + (entry.retrieval_count as f64).ln().max(0.0) / 3.0).min(3.0);
    (-(age_days / effective_half_life).powf(1.5)).exp()
}
```

---

## 13. 重试与限流

重试实现不再由通用 `with_retry<anyhow::Error>` 猜测错误文本，而由 typed
`ProviderError` 驱动：

- request retry：默认 4 次重试，base 200ms、factor 2、jitter `0.9..1.1`；
- stream retry：默认 5 次；
- foreground connection retry：5 秒起步、倍增至 60 秒后保持，无次数上限；
- background connection retry：必须有界；
- 所有等待均可取消；
- Google 等单一 Provider 不得再维护私有 retry helper。

全局 Token 速率窗口仍按 Provider 分别限速；Rate Limit 与本机断网是不同错误类别，不进入
无限网络恢复循环。

---

## 14. 集成关系

```text
AppState（Tauri）
    ├── AgentExecutor（agent-runtime）
    │       ├── HumanGuard → Arc<dyn EventEmitter>（事件推送，解耦 Tauri）
    │       ├── PendingQueue（内存）
    │       ├── Planner → Arc<dyn EventEmitter>（计划确认事件）
    │       ├── TokenBudget（agent-core/context）
    │       ├── BudgetManager（agent-core/budget）
    │       │       └── HumanGuard（YOLO 强制关闭）
    │       └── Supervisor
    │               └── 子 AgentExecutor（递归，depth ≤ 3）
    ├── SessionManager（agent-runtime/session）
    │       └── SessionHandle × N（conversation_id → handle）
    ├── ToolRegistry（agent-core）
    ├── ProviderRegistry（agent-providers）
    └── Store / SqlitePool（agent-core/storage）
```

---

## 15. 崩溃恢复策略：Resume（非 Replay）

> **架构决策**：放弃"从 checkpoint 重放"概念（LLM 调用本质非确定性，重放不保证相同结果），改为"中断恢复"策略。

**启动时恢复流程**：

1. 扫描 `conversations` 表中 `status = 'running'` 的记录
2. 标记为 `interrupted`
3. 调用 `repair_dangling_tool_calls`：检查 messages 表中是否有未闭合的 tool_call（有 assistant 的 tool_call 但无对应 tool_result），注入占位 ToolResult `{ is_error: true, output: "操作因应用重启而中断" }`
4. UI 顶部显示 Banner："检测到上次中断的对话，是否继续？"
5. 用户点击"继续"后，注入系统消息："[系统提示: 上一次执行因应用退出而中断。以上是已完成的步骤和结果，请从中断处继续执行任务。]"，然后正常进入 `round_loop`

**不恢复的内容**：

- PendingQueue（内存丢失，UI 提示用户重新输入）
- YOLO 开关（重置为 OFF）
- 会话 TokenBudget（重置为 0）

---

## 16. 模型切换时的紧急压缩

当用户在对话中途切换到上下文窗口更小的模型时（如从 Claude 200K 切换到 Ollama 8K），触发紧急上下文压缩：

```rust
pub async fn handle_model_switch(&mut self, new_model: &str) {
    let new_window = self.providers.max_context_tokens(new_model);
    let current_tokens = self.estimate_current_tokens();
    
    if current_tokens > (new_window as f32 * 0.9) as u32 {
        // 重新计算 TokenBudget（按新模型窗口）
        self.budget = TokenBudget::for_model_window(new_window);
        
        // 执行 Emergency 压缩
        let compressed = self.context_manager
            .compress_emergency(&self.messages, &self.budget);
        self.messages = compressed;
        
        // 通知用户
        self.emitter.emit("model_switch_compressed", json!({
            "old_tokens": current_tokens,
            "new_limit": new_window,
            "retained_messages": self.messages.len(),
            "message": "切换到较小模型，已压缩对话历史以适应新的上下文窗口"
        })).await.ok();
    }
}
```

> 此机制确保模型切换不会因 ContextTooLong 而直接失败，而是优雅地压缩历史并告知用户。

---

## 17. 相关文档

- [01-架构总览.md](../../03-系统设计阶段/01-架构设计/01-架构总览.md) — 核心能力层次流程图（高层视角）
- [03-Crate结构.md](../../03-系统设计阶段/01-架构设计/03-Crate结构.md) — agent-runtime 目录结构
- [07-上下文管理.md](../../03-系统设计阶段/02-核心功能模块/07-上下文管理.md) — ContextBuilder / TokenBudget / PendingQueue
- [04-人工接管设计.md](../../03-系统设计阶段/03-基础设施/04-人工接管设计.md) — HumanGuard 完整设计
- [02-人工接管与授权详细设计.md](../06-安全与基础设施/02-人工接管与授权详细设计.md) — GuardState / 接管授权详细设计
- [06-安全边界.md](../../03-系统设计阶段/03-基础设施/06-安全边界.md) — Permission / YOLO 开关
- [08-成本预算控制.md](../../03-系统设计阶段/03-基础设施/08-成本预算控制.md) — BudgetManager / YOLO 强制关闭
