# 子 Agent 派生详细设计

> 版本：v1.1 | 日期：2026-08-17 | 状态：已落地
> 对应需求：F-30 子 Agent 派生、F-04 Skills 系统、M-08 安全边界
> 补充文档：[05-子Agent派生.md](../../03-系统设计阶段/02-核心功能模块/05-子Agent派生.md)（系统设计层概览）、[02-agent-runtime详细设计.md](02-agent-runtime详细设计.md)（Supervisor 简述，Section 8）

---

## 0. Codex Agent Thread 对齐决策（v1.1）

本节是当前实现的规范基线；后文中以 `Supervisor` / `SubAgentResult` 为核心的 v1.0 设计保留为历史参考，不再作为实施契约。

### 0.1 核心模型

- 子 Agent 是独立的 **Agent Thread**，具有自己的会话上下文、模型调用、工具循环和持久化状态。
- 父 Agent 通过 `spawn / list / read / send / wait / interrupt / close` 管理线程，子线程返回精简摘要，不把中间日志灌入父上下文。
- `fork_turns = none | all | N` 显式控制初始上下文快照，不再强制“完全隔离且不可派生后追问”。
- 默认共享当前项目工作区，不隐式创建 git worktree；并行写任务必须由父 Agent 规划文件所有权。

### 0.2 运行方式与权限解耦

| 维度 | 可选值 | 职责 |
| --- | --- | --- |
| Agent 线程 | primary / spawned | 上下文与生命周期隔离 |
| 运行适配器 | foreground / background | 是否输出 UI 流、时间线和交互事件 |
| 权限策略 | read-only / workspace-write / danger-full-access | 文件、终端和网络的可用边界 |
| 审批策略 | user / auto-review / never | 风险操作的决策方式 |

`danger-full-access` 不等于 background，background 也不等于无权限约束。子 Agent 继承父任务已解析的权限上限，自定义 Agent 只能收紧，不能扩权。

### 0.3 统一运行内核

foreground 与 background 必须共享同一个多轮 round engine：Provider fallback、context maintenance、hooks、tool execution、iteration budget、usage 和 cancellation 只实现一次。差异仅由适配器表达：

```text
AgentThread
  └─ UnifiedRoundEngine
       ├─ ForegroundAdapter  -> token/timeline/HITL UI
       └─ BackgroundAdapter  -> summary/status/non-interactive result
```

Cron 和 SubAgent 使用 `BackgroundAdapter`；普通聊天使用 `ForegroundAdapter`。旧 `headless` 术语废弃，避免与 sandbox / approval 语义混淆。

当前实现以 `streaming::run_multi_turn_stream` 作为统一 round engine，
`exec::background` 仅消费事件并返回最终文本与 usage；它不再包含独立的 Provider、预算或工具执行循环。

---

## 1. 子 Agent 架构概述

### 1.1 设计动机

主 Agent 执行复杂任务时面临三重挑战：

1. **上下文窗口压力**：单次对话的 token 容量有限，大规模代码审查或多文件重构容易撞上 token 预算上限
2. **任务并行需求**：互不依赖的子任务（如同时搜索三个目录、并行审查多个文件）天然适合并发执行
3. **专项能力隔离**：安全审计、翻译、测试生成等子任务需要独立的系统提示和工具集，混合在一个上下文中会降低推理质量

子 Agent 派生系统通过 **Supervisor 模式** 解决上述问题：父 Agent 将子任务委派给独立的子 Agent 实例，每个子 Agent 拥有全新的 `AgentContext`（干净的消息历史）、受限的权限集和独立的 token 预算。子 Agent 完成后返回结构化摘要，父 Agent 据此继续推理。

### 1.2 核心原则

| 原则 | 说明 |
|------|------|
| 完全隔离 | 子 Agent 对父 Agent 的对话历史一无所知，仅通过 `goal + context` 获取必要信息 |
| 权限不可升级 | 子 Agent 的权限集 = 父 Agent 权限 $\cap$ 派生请求权限，绝不超出父 Agent |
| Token 高效 | 子 Agent 仅返回结构化摘要，不向父 Agent 回传完整对话历史 |
| 深度有界 | 最大嵌套深度硬编码为 3（可配置降低），防止无限递归 |
| 资源有限 | 每个子 Agent 有独立的 token 预算和执行超时，超限则优雅终止 |

### 1.3 Supervisor 模式总览

```text
                       AgentExecutor (depth=0)
                              │
                     ┌────────┴────────┐
                     │   Supervisor    │
                     │  semaphore(3)   │
                     │  max_depth=3    │
                     └───┬────┬────┬──┘
                         │    │    │
                    ┌────┘    │    └────┐
                    ▼         ▼         ▼
               Child A    Child B    Child C
              (depth=1)  (depth=1)  (depth=1)
                 │
                 ▼
           Grandchild A1
              (depth=2)
```

Supervisor 持有所有活跃子 Agent 的句柄，通过 Semaphore 控制并发、通过 depth 计数控制嵌套深度、通过 `CancellationToken` 实现级联取消。

---

## 2. Supervisor 详细设计

### 2.1 核心结构体

```rust
// crates/agent-runtime/src/supervisor.rs

use dashmap::DashMap;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

/// Supervisor 管理所有子 Agent 的生命周期
pub struct Supervisor {
    /// 当前活跃的子 Agent：child_id → ChildAgent
    children: DashMap<Uuid, ChildAgent>,
    /// 并发控制信号量（默认 3 个 permit）
    semaphore: Arc<Semaphore>,
    /// 最大嵌套深度（可在 workspace.yaml 中配置，上限 3）
    max_depth: u8,
    /// 父 Agent 的取消令牌（父被取消时级联取消所有子）
    parent_cancel: CancellationToken,
    /// 事件发射器（推送子 Agent 进度到前端）
    emitter: Arc<dyn EventEmitter>,
    /// 每个对话轮次最大可派生子 Agent 数
    max_children_per_turn: usize,
    /// 本轮已派生计数
    spawned_this_turn: AtomicUsize,
}

/// 单个子 Agent 的运行时句柄
pub struct ChildAgent {
    pub id: Uuid,
    pub role: String,
    pub state: ChildState,
    pub cancel_token: CancellationToken,
    pub task_handle: JoinHandle<Result<SubAgentResult, AgentError>>,
    pub spawned_at: Instant,
    pub depth: u8,
    /// 事件订阅通道（前端实时监控）
    pub event_tx: broadcast::Sender<AgentEvent>,
}

/// 子 Agent 生命周期状态
#[derive(Debug, Clone, PartialEq)]
pub enum ChildState {
    Queued,      // 等待 Semaphore permit
    Running,     // 正在执行 round_loop
    Completed,   // 正常完成
    Failed,      // 执行出错
    Cancelled,   // 被父 Agent 或用户取消
    TimedOut,    // 硬超时终止
}
```

### 2.2 派生配置

```rust
/// spawn_child 的配置参数
pub struct SpawnConfig {
    /// 子任务目标描述（自然语言）
    pub goal: String,
    /// 父 Agent 主动传入的背景信息
    pub context: String,
    /// 子 Agent 的角色标识（影响工具屏蔽策略）
    pub role: String,
    /// 可选：指定更便宜的模型（如 claude-haiku-4-5）
    pub model_override: Option<String>,
    /// 工具白名单（若为 None 则继承父 Agent 的完整工具集，再减去强制屏蔽列表）
    pub allowed_tools: Option<Vec<String>>,
    /// 硬超时，默认 300s（5 分钟）
    pub timeout_secs: u32,
    /// 最大循环次数，默认 50
    pub max_iterations: u32,
    /// 子 Agent 的 token 预算（若为 None 则分配父 Agent 剩余预算的 1/3）
    pub token_budget: Option<TokenBudget>,
    /// 自定义系统提示（若为 None 则使用默认子 Agent 提示）
    pub system_prompt_override: Option<String>,
}

impl Default for SpawnConfig {
    fn default() -> Self {
        Self {
            goal: String::new(),
            context: String::new(),
            role: "worker".into(),
            model_override: None,
            allowed_tools: None,
            timeout_secs: 300,
            max_iterations: 50,
            token_budget: None,
            system_prompt_override: None,
        }
    }
}
```

### 2.3 spawn_child() 方法

```rust
impl Supervisor {
    /// 派生子 Agent
    ///
    /// 执行流程：
    /// 1. 检查深度限制
    /// 2. 检查本轮派生上限
    /// 3. 计算子 Agent 权限集（交集）
    /// 4. 构建子 AgentContext（干净历史 + 受限工具集）
    /// 5. 分配 token 预算
    /// 6. 在 Semaphore 控制下启动异步任务
    pub async fn spawn_child(
        &self,
        config: SpawnConfig,
        parent_ctx: &AgentContext,
    ) -> Result<Uuid, SupervisorError> {
        let child_depth = parent_ctx.depth + 1;

        // ① 深度检查（绝对上限）
        if child_depth > self.max_depth {
            return Err(SupervisorError::DepthExceeded {
                max: self.max_depth,
                current: child_depth,
            });
        }

        // ② 本轮派生上限检查（防止单轮爆炸式派生）
        let spawned = self.spawned_this_turn.fetch_add(1, Ordering::Relaxed);
        if spawned >= self.max_children_per_turn {
            self.spawned_this_turn.fetch_sub(1, Ordering::Relaxed);
            return Err(SupervisorError::TurnLimitExceeded {
                max: self.max_children_per_turn,
            });
        }

        // ③ 并发限制检查（不静默排队，直接返回错误）
        let permit = self.semaphore.clone().try_acquire_owned()
            .map_err(|_| SupervisorError::ConcurrencyExceeded {
                max: self.semaphore.available_permits(),
            })?;

        // ④ 权限交集计算
        let requested_perms = match &config.allowed_tools {
            Some(tools) => PermissionSet::from_tool_names(tools),
            None => parent_ctx.permissions.clone(),
        };
        let child_permissions = spawn_child_permissions(
            &parent_ctx.permissions, &requested_perms
        )?;

        // ⑤ 构建工具集（继承 - 强制屏蔽）
        let child_tools = build_child_tool_registry(
            &parent_ctx.tools,
            &config.allowed_tools,
            &config.role,
        );

        // ⑥ 分配 token 预算
        let child_budget = config.token_budget.unwrap_or_else(|| {
            parent_ctx.budget.allocate_child_budget()
        });

        // ⑦ 构建子 AgentContext（干净的消息历史）
        let child_ctx = AgentContext {
            conversation_id: format!("{}/child-{}", parent_ctx.conversation_id, Uuid::new_v4()),
            workspace_id: parent_ctx.workspace_id.clone(),
            depth: child_depth,
            permissions: child_permissions,
            tools: child_tools,
            llm_client: parent_ctx.llm_client.clone(),  // 共享连接池
            budget: Arc::new(child_budget),
            messages: Vec::new(),  // 干净历史
            model: config.model_override
                .unwrap_or_else(|| parent_ctx.model.clone()),
            system_prompt: config.system_prompt_override
                .unwrap_or_else(|| build_child_system_prompt(&config.role)),
            max_tool_rounds: config.max_iterations,
            ..Default::default()
        };

        // ⑧ 注入初始消息（goal + context）
        let initial_message = Message::user(
            format!(
                "## 任务目标\n{}\n\n## 背景信息\n{}",
                config.goal, config.context
            ),
            ContextSlot::Regular,
        );
        child_ctx.messages.push(initial_message);

        // ⑨ 创建子 Agent 并启动
        let child_id = Uuid::new_v4();
        let cancel_token = self.parent_cancel.child_token();
        let (event_tx, _) = broadcast::channel(64);
        let emitter = self.emitter.clone();

        let child_executor = AgentExecutor::new(child_ctx, emitter.clone());
        let cancel = cancel_token.clone();
        let timeout = Duration::from_secs(config.timeout_secs as u64);
        let event_tx_clone = event_tx.clone();
        let child_id_clone = child_id;

        let task_handle = tokio::spawn(async move {
            let _permit = permit;  // 持有 permit 直到任务完成

            // 发射子 Agent 启动事件
            event_tx_clone.send(AgentEvent::ChildSpawned {
                child_id: child_id_clone,
                role: config.role.clone(),
            }).ok();

            // 硬超时包装
            let result = tokio::time::timeout(timeout, async {
                tokio::select! {
                    result = child_executor.round_loop() => result,
                    _ = cancel.cancelled() => {
                        Err(AgentError::Cancelled)
                    }
                }
            }).await;

            match result {
                Ok(Ok(outcome)) => {
                    Ok(outcome.into_sub_agent_result())
                }
                Ok(Err(e)) => Err(e),
                Err(_elapsed) => {
                    // 硬超时：尝试优雅关闭
                    cancel.cancel();
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    Ok(SubAgentResult {
                        timed_out: true,
                        ..SubAgentResult::partial_from(&child_executor)
                    })
                }
            }
        });

        let child_agent = ChildAgent {
            id: child_id,
            role: config.role,
            state: ChildState::Running,
            cancel_token,
            task_handle,
            spawned_at: Instant::now(),
            depth: child_depth,
            event_tx,
        };

        self.children.insert(child_id, child_agent);

        // 发射事件通知前端
        self.emitter.emit("child_agent_spawned", serde_json::json!({
            "child_id": child_id.to_string(),
            "parent_conversation_id": parent_ctx.conversation_id,
            "role": config.role,
            "depth": child_depth,
        })).await.ok();

        Ok(child_id)
    }
}
```

### 2.4 结果收集与清理

```rust
impl Supervisor {
    /// 等待指定子 Agent 完成并收集结果
    pub async fn join_one(&mut self, child_id: Uuid) -> Result<SubAgentResult, SupervisorError> {
        let child = self.children.remove(&child_id)
            .ok_or(SupervisorError::ChildNotFound(child_id))?;
        let (_, child) = child;

        match child.task_handle.await {
            Ok(Ok(result)) => {
                self.emitter.emit("child_agent_completed", serde_json::json!({
                    "child_id": child_id.to_string(),
                    "timed_out": result.timed_out,
                    "token_usage": result.token_usage,
                })).await.ok();
                Ok(result)
            }
            Ok(Err(e)) => {
                self.emitter.emit("child_agent_failed", serde_json::json!({
                    "child_id": child_id.to_string(),
                    "error": e.to_string(),
                })).await.ok();
                Err(SupervisorError::ChildFailed(e))
            }
            Err(join_err) => Err(SupervisorError::JoinError(join_err)),
        }
    }

    /// 并行等待所有子 Agent 完成，按派生顺序返回结果
    pub async fn join_all(&mut self) -> Vec<Result<SubAgentResult, SupervisorError>> {
        let ids: Vec<Uuid> = self.children.iter().map(|e| *e.key()).collect();
        let mut results = Vec::with_capacity(ids.len());
        // 使用 FuturesOrdered 保证顺序
        let mut futures = FuturesOrdered::new();
        for id in ids {
            let child = self.children.remove(&id);
            if let Some((_, child)) = child {
                futures.push_back(async move {
                    (id, child.task_handle.await)
                });
            }
        }
        while let Some((id, join_result)) = futures.next().await {
            match join_result {
                Ok(Ok(result)) => results.push(Ok(result)),
                Ok(Err(e)) => results.push(Err(SupervisorError::ChildFailed(e))),
                Err(e) => results.push(Err(SupervisorError::JoinError(e))),
            }
        }
        results
    }

    /// 取消所有子 Agent（父 Agent 被取消时调用）
    pub fn cancel_all(&self) {
        for entry in self.children.iter() {
            entry.value().cancel_token.cancel();
        }
        // 不立即 remove：等 task_handle 自然结束后由 on_child_completed 清理
    }

    /// 子 Agent 完成后的清理回调
    fn on_child_completed(&self, child_id: Uuid) {
        self.children.remove(&child_id);
    }

    /// 流式订阅子 Agent 事件（前端 /agents 监控面板数据来源）
    pub fn subscribe(&self, child_id: Uuid) -> Option<broadcast::Receiver<AgentEvent>> {
        self.children.get(&child_id).map(|c| c.event_tx.subscribe())
    }

    /// 每轮开始时重置本轮派生计数
    pub fn reset_turn_counter(&self) {
        self.spawned_this_turn.store(0, Ordering::Relaxed);
    }
}
```

### 2.5 子 Agent 结果结构

```rust
/// 子 Agent 执行完成后的结构化摘要
pub struct SubAgentResult {
    pub actions: Vec<String>,          // 执行了哪些操作
    pub findings: Vec<String>,         // 发现了什么
    pub modified_files: Vec<String>,   // 修改了哪些文件
    pub issues: Vec<String>,           // 遇到了哪些问题
    pub token_usage: TokenUsage,       // Token 消耗统计
    pub timed_out: bool,               // 是否因超时终止
    pub final_message: String,         // 子 Agent 最后的文本输出
}

pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_cost_usd: f64,           // 由 BudgetManager 根据 Provider 定价计算
}

impl SubAgentResult {
    /// 从 round_loop 输出提取部分结果（超时场景）
    pub fn partial_from(executor: &AgentExecutor) -> Self {
        Self {
            actions: executor.context.extract_tool_call_summaries(),
            findings: Vec::new(),
            modified_files: executor.context.extract_modified_files(),
            issues: vec!["子 Agent 执行超时，返回部分结果".into()],
            token_usage: executor.context.budget.usage(),
            timed_out: true,
            final_message: String::new(),
        }
    }
}
```

---

## 3. 权限继承模型

### 3.1 核心原则：不可升级（Permission Intersection）

子 Agent 的权限集始终是父 Agent 权限集与派生请求权限集的**交集**。任何试图获取父 Agent 未持有权限的行为都会被拒绝。

```text
父 Agent 权限集:    { ReadFile, WriteFile, ExecuteBash, NetworkRead, SpawnAgent }
派生请求权限集:     { ReadFile, WriteFile, DeleteFile, NetworkRead }
                                            ^^^^^^^^
                                            父 Agent 没有此权限
子 Agent 实际权限:  { ReadFile, WriteFile, NetworkRead }
                    = 父 ∩ 请求（DeleteFile 被丢弃，不报错但记录审计日志）
```

### 3.2 权限交集计算

```rust
// crates/agent-core/src/security/permissions.rs

pub fn spawn_child_permissions(
    parent: &PermissionSet,
    requested: &PermissionSet,
) -> Result<PermissionSet, SecurityError> {
    let intersection: HashSet<Permission> =
        parent.0.intersection(&requested.0).cloned().collect();

    // 记录被剪裁掉的权限（审计用途，不阻断派生）
    let excess: Vec<&Permission> = requested.0.difference(&parent.0).collect();
    if !excess.is_empty() {
        tracing::warn!(
            "子 Agent 请求了父 Agent 未持有的权限，已自动剪裁: {:?}",
            excess
        );
    }

    // 交集为空时仍然允许派生（子 Agent 可能只做纯推理，不调用工具）
    Ok(PermissionSet(intersection))
}
```

### 3.3 工具集过滤

派生时，根据权限交集和强制屏蔽列表构建子 Agent 的工具注册表：

```rust
/// 构建子 Agent 的工具注册表
fn build_child_tool_registry(
    parent_tools: &ToolRegistry,
    allowed_tools: &Option<Vec<String>>,
    role: &str,
) -> ToolRegistry {
    let mut child_registry = ToolRegistry::new();

    // 强制屏蔽的 7 类工具
    let blocked = blocked_tool_set(role);

    for (name, tool) in parent_tools.iter() {
        // 白名单过滤
        if let Some(allowed) = allowed_tools {
            if !allowed.contains(&name) {
                continue;
            }
        }
        // 强制屏蔽过滤
        if blocked.contains(&name) {
            continue;
        }
        child_registry.register(tool.clone());
    }

    child_registry
}

/// 返回指定角色下强制屏蔽的工具名集合
fn blocked_tool_set(role: &str) -> HashSet<String> {
    let mut blocked = HashSet::from([
        "git_push".into(),
        "publish".into(),
        "memory_write".into(),
        "knowledge_manage".into(),
        "shell_bg".into(),
        // UI 操作类和配置写入类通过前缀匹配
    ]);

    // 仅 role="orchestrator" 的子 Agent 允许再次派生
    if role != "orchestrator" {
        blocked.insert("delegate_task".into());
    }

    blocked
}
```

### 3.4 权限检查时序

```text
父 Agent 调用 delegate_task
    │
    ▼
Supervisor::spawn_child()
    │
    ├─ spawn_child_permissions(parent, requested) → child_perms
    │
    ├─ build_child_tool_registry(parent_tools, allowed, role)
    │     └─ 遍历父工具集 → 白名单过滤 → 强制屏蔽过滤 → 子工具集
    │
    └─ 子 Agent 执行工具调用时
          │
          ├─ ToolRegistry::execute_checked()
          │     └─ caller_permissions.check(&tool.required_permission())
          │
          └─ HumanGuard::check()  ← 子 Agent 共享父 Agent 的 HumanGuard 实例
                └─ 子 Agent 的 L2/L3 操作同样需要人工审批
```

---

## 4. 资源隔离

### 4.1 Token 预算隔离

每个子 Agent 分配独立的 `TokenBudget`，默认为父 Agent 剩余预算的 **1/3**：

```rust
impl TokenBudget {
    /// 为子 Agent 分配预算（从父预算中扣除）
    pub fn allocate_child_budget(&self) -> TokenBudget {
        let remaining = self.remaining();
        let child_allocation = remaining / 3;

        // 从父预算中预扣（子 Agent 实际可能用不完）
        self.reserve(child_allocation);

        TokenBudget::new(child_allocation)
    }

    /// 子 Agent 完成后退还未使用的预算
    pub fn return_unused(&self, child_budget: &TokenBudget) {
        let unused = child_budget.remaining();
        self.unreserve(unused);
    }

    /// 剩余可用 token 数
    pub fn remaining(&self) -> u64 {
        self.limit.saturating_sub(self.used.load(Ordering::Relaxed))
            .saturating_sub(self.reserved.load(Ordering::Relaxed))
    }
}
```

预算耗尽时子 Agent 的行为：

```rust
// AgentExecutor::round_loop() 中
self.budget_manager.check_before_call(&self.context.workspace_id, 0.0).await
    .map_err(|_| {
        // 预算耗尽：优雅停止，返回已完成的部分结果
        AgentError::BudgetExhausted {
            used: self.context.budget.used(),
            limit: self.context.budget.limit(),
        }
    })?;
```

### 4.2 执行超时

每个子 Agent 有独立的硬超时（默认 300 秒），通过 `tokio::time::timeout` 实现：

```rust
// spawn_child 内部的超时处理（见 §2.3）
let result = tokio::time::timeout(
    Duration::from_secs(config.timeout_secs as u64),
    child_executor.round_loop(),
).await;

match result {
    Ok(Ok(outcome)) => { /* 正常完成 */ }
    Ok(Err(e)) => { /* 执行错误 */ }
    Err(_elapsed) => {
        // 硬超时：先发 CancellationToken，等 5s 优雅关闭
        cancel_token.cancel();
        tokio::time::sleep(Duration::from_secs(5)).await;
        // 收集部分结果，标记 timed_out = true
    }
}
```

### 4.3 内存隔离

子 Agent 的 `AgentContext` 完全独立：

| 资源 | 父 Agent | 子 Agent | 共享方式 |
|------|---------|---------|---------|
| 消息历史 | 完整对话历史 | 空（仅 goal + context 作为初始消息） | 不共享 |
| 系统提示 | 全量系统提示 | 子 Agent 专用精简提示（或自定义） | 不共享 |
| LLM 客户端 | `Arc<dyn LlmClient>` | 同一 Arc 引用 | **共享**（复用连接池） |
| 工具注册表 | 完整工具集 | 过滤后的子集 | 不共享（独立副本） |
| Token 预算 | 原始预算（减去预留） | 从父预算分配的份额 | 独立实例 |
| HumanGuard | 审批状态机 | 同一 Arc 引用 | **共享**（子 Agent 操作同样受审批） |
| 长期记忆 | 可读写 | **只读**（`memory_write` 被屏蔽） | 读共享 |

---

## 5. 并发控制

### 5.1 Semaphore 限流

子 Agent 的并发数通过 `tokio::sync::Semaphore` 严格控制，默认最多 3 个同时运行：

```rust
impl Supervisor {
    pub fn new(
        max_concurrent: usize,
        max_depth: u8,
        parent_cancel: CancellationToken,
        emitter: Arc<dyn EventEmitter>,
    ) -> Self {
        Self {
            children: DashMap::new(),
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            max_depth: max_depth.min(3),  // 硬上限 3
            parent_cancel,
            emitter,
            max_children_per_turn: 10,
            spawned_this_turn: AtomicUsize::new(0),
        }
    }
}
```

### 5.2 并发超限行为

与系统设计保持一致：超出 `max_concurrent_children` 时**返回工具错误**，不静默截断或排队等待：

```rust
// spawn_child 中
let permit = self.semaphore.clone().try_acquire_owned()
    .map_err(|_| SupervisorError::ConcurrencyExceeded {
        max: 3, // Semaphore 初始 permit 数
    })?;
```

这一设计确保 LLM 能感知并发限制，调整派生策略（例如改为串行执行或减少子任务数量）。

### 5.3 背压机制

当并发达到上限时，错误信息会被注入父 Agent 的工具结果，LLM 据此决策：

```text
ToolResult {
    is_error: true,
    output: "无法派生子 Agent：已达最大并发数 3。请等待现有子 Agent 完成后再派生，
             或减少并行任务数量。当前活跃子 Agent:
             - child-abc (role: researcher, 已运行 45s)
             - child-def (role: reviewer, 已运行 30s)
             - child-ghi (role: tester, 已运行 12s)"
}
```

---

## 6. 深度限制

### 6.1 深度跟踪

通过 `AgentContext.depth` 字段追踪当前嵌套深度，每次 spawn 递增：

```text
depth=0  主 Agent（用户对话入口）
  └─ depth=1  子 Agent（默认最大层级，max_spawn_depth=1）
        └─ depth=2  需 role="orchestrator" 才允许
              └─ depth=3  绝对最大值
                    └─ depth=4  ← 直接返回 DepthExceeded 错误
```

### 6.2 深度配置

```yaml
# workspace.yaml
agent:
  sub_agent:
    max_depth: 1          # 默认值：扁平结构，主 Agent 直接派生一层子 Agent
    max_concurrent: 3     # 每层最大并发子 Agent 数
    max_per_turn: 10      # 每个对话轮次最大派生总数
    default_timeout_secs: 300
```

深度限制的阶梯策略：

| 深度 | 许可条件 | 前端提示 |
|------|---------|---------|
| 0 → 1 | 默认允许 | 无特殊提示 |
| 1 → 2 | `role="orchestrator"` 或用户配置 `max_depth >= 2` | 显示深度警告图标 |
| 2 → 3 | 用户显式配置 `max_depth = 3` | 前端显示费用警告："最多 $3^3=27$ 个并发叶子节点" |
| > 3 | **绝对拒绝** | 工具错误：DepthExceeded |

### 6.3 深度检查实现

```rust
// spawn_child 中的深度校验
let child_depth = parent_ctx.depth + 1;

if child_depth > self.max_depth {
    return Err(SupervisorError::DepthExceeded {
        max: self.max_depth,
        current: child_depth,
    });
}

// max_depth 本身不能超过硬上限 3
// 在 Supervisor::new 中：max_depth: max_depth.min(3)
```

---

## 7. 跨 Agent 通信

### 7.1 通信模型

子 Agent 派生系统采用**单向树状通信**模型，不支持兄弟节点之间的直接通信：

```text
            父 Agent
           /    |    \
     spawn   spawn   spawn
         ↓      ↓      ↓
      Child A  Child B  Child C
         ↑      ↑      ↑
     result  result  result

  ← 父→子：spawn 时传入 goal + context + tools + system_prompt
  → 子→父：完成时返回 SubAgentResult（结构化摘要）
  ✗ 兄弟间：不允许直接通信
```

### 7.2 父 → 子：派生时传入

父 Agent 通过 `SpawnConfig` 向子 Agent 传递所有必要信息：

| 字段 | 说明 |
|------|------|
| `goal` | 子任务目标（自然语言） |
| `context` | 父 Agent 主动提供的背景信息（文件内容、搜索结果等） |
| `system_prompt_override` | 可选的专项系统提示 |
| `allowed_tools` | 可用工具白名单 |
| `model_override` | 可选的模型覆盖 |

子 Agent 对父 Agent 的对话历史**完全不可见**。父 Agent 必须在 `context` 字段中显式传入子 Agent 需要的所有信息——这确保了 token 高效（不传整个对话历史）和安全隔离（子 Agent 无法窥探父级上下文中的敏感信息）。

### 7.3 子 → 父：结果返回

子 Agent 完成后返回 `SubAgentResult` 结构（见 Section 2.5），父 Agent 将其注入工具结果上下文：

```rust
// delegate_task 工具执行完毕后的结果注入
fn inject_child_result(
    parent_ctx: &mut AgentContext,
    tool_call_id: &str,
    result: &SubAgentResult,
) {
    let summary = format!(
        "## 子 Agent 执行结果\n\
         \n### 执行的操作\n{}\
         \n### 发现\n{}\
         \n### 修改的文件\n{}\
         \n### 遇到的问题\n{}\
         \n### Token 消耗\n输入: {} / 输出: {} / 费用: ${:.4}\
         \n### 超时: {}",
        result.actions.iter().map(|a| format!("- {}", a)).collect::<Vec<_>>().join("\n"),
        result.findings.iter().map(|f| format!("- {}", f)).collect::<Vec<_>>().join("\n"),
        result.modified_files.iter().map(|f| format!("- `{}`", f)).collect::<Vec<_>>().join("\n"),
        result.issues.iter().map(|i| format!("- {}", i)).collect::<Vec<_>>().join("\n"),
        result.token_usage.input_tokens,
        result.token_usage.output_tokens,
        result.token_usage.total_cost_usd,
        result.timed_out,
    );

    parent_ctx.add_tool_result(
        tool_call_id.to_string(),
        ToolResult {
            tool_call_id: tool_call_id.into(),
            name: "delegate_task".into(),
            output: summary,
            is_error: false,
        },
    );
}
```

### 7.4 批量并行派生

支持一次调用派生多个子 Agent，并发执行后按索引排序回传：

```rust
/// 批量派生：数组形式传入多个子任务
pub async fn spawn_batch(
    &self,
    configs: Vec<SpawnConfig>,
    parent_ctx: &AgentContext,
) -> Vec<Result<Uuid, SupervisorError>> {
    let mut child_ids = Vec::with_capacity(configs.len());
    for config in configs {
        child_ids.push(self.spawn_child(config, parent_ctx).await);
    }
    child_ids
}
```

---

## 8. agent_spawn 工具设计

### 8.1 delegate_task Tool 定义

`delegate_task` 作为内置工具注册到 `ToolRegistry`，是父 Agent 派生子 Agent 的唯一入口：

```rust
// crates/agent-runtime/src/tools/delegate_task.rs

pub struct DelegateTaskTool {
    supervisor: Arc<Mutex<Supervisor>>,
}

#[async_trait]
impl Tool for DelegateTaskTool {
    fn name(&self) -> &str { "delegate_task" }

    fn description(&self) -> &str {
        "派生子 Agent 执行独立的子任务。子 Agent 拥有独立的上下文和受限的工具集。\
         适用于需要 LLM 推理、多步规划的复杂子任务。\
         对于机械性数据处理，请优先使用 shell_exec。"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "task_description": {
                    "type": "string",
                    "description": "子任务的详细描述（自然语言），包含目标和背景信息"
                },
                "context": {
                    "type": "string",
                    "description": "传递给子 Agent 的背景信息（文件内容、搜索结果等）。\
                                    子 Agent 无法看到父 Agent 的对话历史，必须在此显式传入所有需要的信息。"
                },
                "tools": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "子 Agent 可用的工具白名单（可选，默认继承父 Agent 工具集减去屏蔽列表）"
                },
                "model_override": {
                    "type": "string",
                    "description": "可选：指定子 Agent 使用的模型（如 claude-haiku-4-5 以降低成本）"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "子 Agent 的最大执行时间（秒），默认 300",
                    "default": 300,
                    "maximum": 600
                }
            },
            "required": ["task_description"]
        })
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::High  // 默认 L3，需要人工确认
    }

    fn required_permission(&self) -> Permission {
        Permission::SpawnAgent
    }

    async fn execute(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        let params: DelegateTaskInput = serde_json::from_value(input.parameters)
            .map_err(|e| ToolError::InvalidInput(e.to_string()))?;

        let config = SpawnConfig {
            goal: params.task_description,
            context: params.context.unwrap_or_default(),
            role: "worker".into(),
            model_override: params.model_override,
            allowed_tools: params.tools,
            timeout_secs: params.timeout_secs.unwrap_or(300),
            ..Default::default()
        };

        let mut supervisor = self.supervisor.lock().await;
        let child_id = supervisor.spawn_child(config, &input.caller_context).await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        // 同步等待子 Agent 完成（delegate_task 是阻塞式工具）
        let result = supervisor.join_one(child_id).await
            .map_err(|e| ToolError::Execution(e.to_string()))?;

        // 退还未使用的 token 预算
        input.caller_context.budget.return_unused(&result.token_usage);

        Ok(ToolOutput {
            content: vec![ToolContent::Text(format_sub_agent_result(&result))],
            is_error: false,
        })
    }
}

#[derive(Deserialize)]
struct DelegateTaskInput {
    task_description: String,
    context: Option<String>,
    tools: Option<Vec<String>>,
    model_override: Option<String>,
    timeout_secs: Option<u32>,
}
```

### 8.2 风险等级与审批

`delegate_task` 默认风险等级为 `High`（L3），需要人工确认才能执行。但可以通过白名单规则快速放行：

```yaml
# workspace.yaml
human_guard:
  whitelist:
    - tool_name: delegate_task
      # 仅当子任务只使用只读工具时自动放行
      args_pattern: '"tools":\s*\["file_read",\s*"memory_search"'
```

### 8.3 批量派生接口

LLM 可在单次 tool_call 中传入数组形式的子任务，实现一次性派生多个子 Agent：

```rust
pub struct DelegateTaskBatchTool {
    supervisor: Arc<Mutex<Supervisor>>,
}

#[async_trait]
impl Tool for DelegateTaskBatchTool {
    fn name(&self) -> &str { "delegate_task_batch" }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "tasks": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "task_description": { "type": "string" },
                            "context": { "type": "string" },
                            "tools": { "type": "array", "items": { "type": "string" } },
                            "model_override": { "type": "string" }
                        },
                        "required": ["task_description"]
                    },
                    "maxItems": 5,
                    "description": "批量子任务列表，最多 5 个，并发执行"
                }
            },
            "required": ["tasks"]
        })
    }

    // ... execute 内部调用 spawn_batch + join_all
}
```

---

## 9. 子 Agent 执行流程

### 9.1 完整序列图

```text
    父 AgentExecutor          Supervisor           子 AgentExecutor         前端 UI
         │                       │                       │                    │
         │  round_loop 中        │                       │                    │
         │  LLM 返回 tool_call:  │                       │                    │
         │  delegate_task        │                       │                    │
         │                       │                       │                    │
         ├──[1] execute ────────►│                       │                    │
         │   (DelegateTaskTool)  │                       │                    │
         │                       │                       │                    │
         │                       ├─[2] depth check       │                    │
         │                       ├─[3] perm intersect    │                    │
         │                       ├─[4] tool filter       │                    │
         │                       ├─[5] budget allocate   │                    │
         │                       │                       │                    │
         │                       ├─[6] spawn_child ─────►│                    │
         │                       │    (tokio::spawn)     │                    │
         │                       │                       │                    │
         │                       ├──[7] emit ──────────────────────────────►│
         │                       │   child_agent_spawned │                    │
         │                       │                       │                    │
         │  (阻塞等待子 Agent)   │                       ├─[8] round_loop    │
         │                       │                       │  ├─ LLM call      │
         │                       │                       │  ├─ tool exec     │
         │                       │                       │  ├─ LLM call      │
         │                       │                       │  └─ ... 循环      │
         │                       │                       │                    │
         │                       │                       │                    │
         │                       │                       ├──[9] emit ────────►│
         │                       │                       │  progress events   │
         │                       │                       │                    │
         │                       │                       │──[10] Done         │
         │                       │                       │                    │
         │                       │◄─[11] result ─────────┤                    │
         │                       │  (SubAgentResult)     │                    │
         │                       │                       │                    │
         │                       ├─[12] cleanup child    │                    │
         │                       │  remove from DashMap  │                    │
         │                       │                       │                    │
         │                       ├──[13] emit ─────────────────────────────►│
         │                       │   child_agent_completed                   │
         │                       │                       │                    │
         │◄─[14] ToolResult ─────┤                       │                    │
         │  (格式化的 SubAgentResult)                     │                    │
         │                       │                       │                    │
         ├─[15] inject result    │                       │                    │
         │  into parent context  │                       │                    │
         │                       │                       │                    │
         ├─[16] 继续 round_loop  │                       │                    │
         │  (下一轮 LLM 调用)    │                       │                    │
```

### 9.2 关键步骤说明

1. **[1-5] 预检与资源分配**：Supervisor 在实际派生前完成所有校验（深度、并发、权限、预算），任一失败则以 ToolError 形式回传父 Agent
2. **[6] 异步派生**：子 Agent 在独立的 tokio task 中运行，持有 Semaphore permit
3. **[7] 前端通知**：派生事件推送到前端，UI 可展示子 Agent 卡片
4. **[8-9] 独立执行**：子 Agent 运行自己的 `round_loop`，可调用工具、接受 HumanGuard 审批
5. **[10-14] 结果收集**：子 Agent 完成后，结果转换为父 Agent 的 ToolResult
6. **[15-16] 继续推理**：父 Agent 将子 Agent 结果作为上下文输入下一轮 LLM 调用

---

## 10. 错误处理

### 10.1 错误类型枚举

```rust
#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    #[error("嵌套深度超限: 当前 {current}，最大 {max}")]
    DepthExceeded { max: u8, current: u8 },

    #[error("并发子 Agent 数超限: 最大 {max}")]
    ConcurrencyExceeded { max: usize },

    #[error("本轮派生数超限: 最大 {max}")]
    TurnLimitExceeded { max: usize },

    #[error("子 Agent 未找到: {0}")]
    ChildNotFound(Uuid),

    #[error("子 Agent 执行失败: {0}")]
    ChildFailed(AgentError),

    #[error("等待子 Agent 时 join 错误: {0}")]
    JoinError(tokio::task::JoinError),

    #[error("权限升级被拒绝: {0}")]
    PermissionEscalation(String),
}
```

### 10.2 失败模式与处理策略

| 失败模式 | 触发条件 | 处理策略 | 父 Agent 感知 |
|---------|---------|---------|-------------|
| 超时 | `timeout_secs` 到期 | `CancellationToken` 取消 → 5s 优雅关闭 → 收集部分结果 | ToolResult 含 `timed_out: true` + 部分结果 |
| Token 预算耗尽 | `BudgetManager::check_before_call` 失败 | 优雅停止当前轮次，返回已完成操作 | ToolResult 含已完成操作 + 预算耗尽提示 |
| 工具执行错误 | 子 Agent 内部工具调用失败 | 子 Agent 内部 LLM 自行处理重试/调整 | 仅当子 Agent 整体失败时向父传播 |
| 深度超限 | `parent_ctx.depth + 1 > max_depth` | 立即拒绝，不创建子 Agent | ToolError: DepthExceeded |
| 并发超限 | Semaphore 无可用 permit | 立即拒绝，返回当前活跃子 Agent 信息 | ToolError: ConcurrencyExceeded |
| 父 Agent 取消 | 用户取消或父 Agent 超时 | `CancellationToken` 级联取消所有子 Agent | 不需要（父自身也在终止） |
| 进程崩溃 | 应用意外退出 | 重启后仅恢复根任务（depth=0），子任务由父编排逻辑重新 spawn | 崩溃恢复注入占位消息 |

### 10.3 超时处理详细流程

```rust
/// 超时后的优雅关闭序列
async fn handle_child_timeout(
    child: &ChildAgent,
    executor: &AgentExecutor,
) -> SubAgentResult {
    // Phase 1：发送取消信号
    child.cancel_token.cancel();

    // Phase 2：等待 5s 让子 Agent 完成当前原子操作
    let grace_result = tokio::time::timeout(
        Duration::from_secs(5),
        child.task_handle,  // 等待 task 自然结束
    ).await;

    match grace_result {
        Ok(Ok(Ok(result))) => {
            // 子 Agent 在优雅期内完成
            SubAgentResult { timed_out: true, ..result }
        }
        _ => {
            // Phase 3：强制 abort
            child.task_handle.abort();
            SubAgentResult::partial_from(executor)
        }
    }
}
```

### 10.4 错误传播规则

子 Agent 的错误**不会导致父 Agent 崩溃**。所有子 Agent 错误都被封装为 `ToolResult`（可能 `is_error = true`），由父 Agent 的 LLM 决定如何处理：

```rust
// delegate_task execute 中的错误处理
match supervisor.join_one(child_id).await {
    Ok(result) => {
        Ok(ToolOutput {
            content: vec![ToolContent::Text(format_sub_agent_result(&result))],
            is_error: result.timed_out || !result.issues.is_empty(),
        })
    }
    Err(SupervisorError::ChildFailed(e)) => {
        Ok(ToolOutput {
            content: vec![ToolContent::Text(format!(
                "子 Agent 执行失败: {}。请考虑调整策略或自行完成此子任务。", e
            ))],
            is_error: true,
        })
    }
    Err(e) => Err(ToolError::Execution(e.to_string())),
}
```

---

## 11. 与 Skill 编排的关系

### 11.1 SkillOrchestrator 内部使用 Supervisor

`SkillOrchestrator`（定义在 `agent-runtime/src/orchestrator.rs`）在执行需要子 Agent 的 Skill 时，内部委托给 `Supervisor`：

```rust
// crates/agent-runtime/src/orchestrator.rs

pub struct SkillOrchestrator {
    supervisor: Arc<Mutex<Supervisor>>,
    tool_registry: Arc<ToolRegistry>,
}

impl SkillOrchestrator {
    /// 执行一个 Skill，若 Skill 声明 can_spawn_agents=true 则通过 Supervisor 派生子 Agent
    pub async fn execute_skill(
        &self,
        skill: &dyn Skill,
        ctx: &mut AgentContext,
        input: SkillInput,
    ) -> anyhow::Result<SkillOutput> {
        let manifest = skill.manifest();

        if manifest.can_spawn_agents {
            // Skill 声明需要派生子 Agent：通过 Supervisor 执行
            let config = SpawnConfig {
                goal: format!("执行 Skill: {}\n{}", manifest.name, manifest.description),
                context: serde_json::to_string(&input)?,
                role: "skill_worker".into(),
                allowed_tools: Some(manifest.allowed_tools.clone()),
                ..Default::default()
            };

            let mut supervisor = self.supervisor.lock().await;
            let child_id = supervisor.spawn_child(config, ctx).await?;
            let result = supervisor.join_one(child_id).await?;

            Ok(SkillOutput::from_sub_agent_result(result))
        } else {
            // 普通 Skill：在当前上下文中直接执行
            skill.execute(ctx, input).await
        }
    }
}
```

### 11.2 Skill Manifest 中的声明

Skill 通过 `SKILL.md` frontmatter 中的 `can_spawn_agents` 字段声明是否需要子 Agent：

```yaml
---
name: deep-research
description: 对给定主题进行多源深度研究
can_spawn_agents: true          # 声明需要派生子 Agent
allowed_tools: [http_request, file_read, browser_fetch]
parameters:
  topic:
    type: string
    description: 研究主题
    required: true
---
```

当 `can_spawn_agents: true` 时，Skill 的执行权限要求会自动包含 `Permission::SpawnAgent`。

### 11.3 调用关系

```text
AgentExecutor::round_loop()
    │
    ├─ LLM 选择了 Skill 调用
    │
    ▼
SkillOrchestrator::execute_skill()
    │
    ├─ skill.manifest().can_spawn_agents == true?
    │   │
    │   ├─ YES → Supervisor::spawn_child()
    │   │         └─ 子 AgentExecutor 独立运行
    │   │              └─ 子 Agent 使用 Skill 声明的 allowed_tools
    │   │
    │   └─ NO  → skill.execute() 在当前上下文中直接执行
    │
    └─ 结果回传 AgentExecutor
```

---

## 12. Tauri 集成

### 12.1 前端事件

子 Agent 系统通过 `EventEmitter` 推送以下事件到前端：

| 事件名 | 触发时机 | Payload |
|--------|---------|---------|
| `child_agent_spawned` | 子 Agent 被创建 | `{ child_id, parent_conversation_id, role, depth }` |
| `child_agent_progress` | 子 Agent 执行工具调用 | `{ child_id, tool_name, status }` |
| `child_agent_completed` | 子 Agent 正常完成 | `{ child_id, timed_out, token_usage, duration_ms }` |
| `child_agent_failed` | 子 Agent 执行失败 | `{ child_id, error }` |
| `child_agent_cancelled` | 子 Agent 被取消 | `{ child_id, reason }` |

### 12.2 Tauri Commands

```rust
/// 查询当前所有活跃子 Agent
#[tauri::command]
pub async fn list_child_agents(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<Vec<ChildAgentInfo>, AppError> {
    let session = state.runtime.get_session(&conversation_id)
        .ok_or(AppError::SessionNotFound)?;
    let supervisor = session.supervisor.lock().await;
    Ok(supervisor.children.iter().map(|entry| {
        let child = entry.value();
        ChildAgentInfo {
            id: child.id.to_string(),
            role: child.role.clone(),
            state: format!("{:?}", child.state),
            depth: child.depth,
            elapsed_secs: child.spawned_at.elapsed().as_secs(),
        }
    }).collect())
}

/// 取消指定子 Agent
#[tauri::command]
pub async fn cancel_child_agent(
    state: State<'_, AppState>,
    conversation_id: String,
    child_id: String,
) -> Result<(), AppError> {
    let id = Uuid::parse_str(&child_id)?;
    let session = state.runtime.get_session(&conversation_id)
        .ok_or(AppError::SessionNotFound)?;
    let supervisor = session.supervisor.lock().await;
    if let Some(child) = supervisor.children.get(&id) {
        child.cancel_token.cancel();
        Ok(())
    } else {
        Err(AppError::NotFound(format!("子 Agent {} 不存在", child_id)))
    }
}

/// 订阅子 Agent 事件流
#[tauri::command]
pub async fn subscribe_child_agent(
    state: State<'_, AppState>,
    conversation_id: String,
    child_id: String,
) -> Result<(), AppError> {
    // 通过 EventEmitter 转发子 Agent 的 broadcast::Receiver 到前端
    let id = Uuid::parse_str(&child_id)?;
    let session = state.runtime.get_session(&conversation_id)
        .ok_or(AppError::SessionNotFound)?;
    let supervisor = session.supervisor.lock().await;
    if let Some(rx) = supervisor.subscribe(id) {
        tokio::spawn(forward_child_events(rx, state.emitter.clone(), child_id));
    }
    Ok(())
}
```

### 12.3 前端子 Agent 卡片

```typescript
// apps/desktop/src/components/ChildAgentCard.tsx

interface ChildAgentCardProps {
  childId: string;
  role: string;
  state: string;
  depth: number;
  elapsedSecs: number;
  onCancel: (childId: string) => void;
}

function ChildAgentCard({ childId, role, state, depth, elapsedSecs, onCancel }: ChildAgentCardProps) {
  return (
    <div className={`child-agent-card depth-${depth}`}>
      <div className="header">
        <span className="role">{role}</span>
        <span className={`state ${state.toLowerCase()}`}>{state}</span>
        <span className="depth">Depth {depth}</span>
      </div>
      <div className="meta">
        <span>ID: {childId.slice(0, 8)}...</span>
        <span>已运行: {elapsedSecs}s</span>
      </div>
      {state === 'Running' && (
        <button className="cancel-btn" onClick={() => onCancel(childId)}>
          取消
        </button>
      )}
    </div>
  );
}
```

前端在对话视图的侧边栏展示活跃子 Agent 列表，支持实时状态更新和取消操作。通过监听 `child_agent_*` 事件维护本地状态。

---

## 13. 设计约束

### 13.1 派生数量限制

| 约束 | 默认值 | 可配置 | 说明 |
|------|--------|-------|------|
| 单轮最大派生数 | 10 | 是（`max_per_turn`） | 防止 LLM 在一轮中爆炸式派生 |
| 最大并发子 Agent | 3 | 是（`max_concurrent`） | Semaphore 控制 |
| 最大嵌套深度 | 1（可配至 3） | 是（`max_depth`，硬上限 3） | 防止无限递归 |
| 单个子 Agent 最大迭代次数 | 50 | 是（`max_iterations`） | 防止单个子 Agent 无限循环 |
| 单个子 Agent 最大执行时间 | 300s | 是（`timeout_secs`，上限 600s） | 硬超时 |

### 13.2 禁止行为

- **禁止递归自派生**：非 `orchestrator` 角色的子 Agent 无法调用 `delegate_task`（该工具被强制屏蔽）
- **禁止子 Agent 写入父 Agent 记忆**：`memory_write` 和 `knowledge_manage` 被强制屏蔽
- **禁止子 Agent 修改全局配置**：配置写入类工具被强制屏蔽
- **禁止子 Agent 推送代码**：`git_push`、`publish` 类工具被强制屏蔽
- **禁止子 Agent 启动后台进程**：`shell_bg` 被强制屏蔽（父 Agent 无法感知/清理子 Agent 启动的后台进程）

### 13.3 安全不变量

```text
INVARIANT 1: child.permissions ⊆ parent.permissions
             子 Agent 权限永远不超出父 Agent

INVARIANT 2: child.depth == parent.depth + 1
             深度严格递增

INVARIANT 3: child.depth <= 3
             绝对深度上限

INVARIANT 4: child.token_budget <= parent.remaining_budget
             子 Agent 预算不超过父 Agent 剩余预算

INVARIANT 5: parent.cancel => all_children.cancel
             父取消级联到所有子 Agent
```

### 13.4 成本控制

父 Agent 收到 `SubAgentResult` 后可据 `token_usage.total_cost_usd` 决策后续子任务是否降级模型：

```rust
// 自适应成本控制示例
if result.token_usage.total_cost_usd > 0.10 {
    // 后续子任务自动切换到更便宜的模型
    next_config.model_override = Some("claude-haiku-4-5".into());
    tracing::info!(
        "子 Agent 费用 ${:.4}，后续任务降级为 claude-haiku-4-5",
        result.token_usage.total_cost_usd
    );
}
```

---

## 14. 相关文档

- [05-子Agent派生.md](../../03-系统设计阶段/02-核心功能模块/05-子Agent派生.md) -- 子 Agent 派生的系统设计层概览
- [02-agent-runtime详细设计.md](02-agent-runtime详细设计.md) -- Supervisor 简述（Section 8）、AgentExecutor 主循环
- [01-agent-core详细设计.md](01-agent-core详细设计.md) -- AgentContext / TurnContext 数据结构、Agent 纯函数
- [02-人工接管与授权详细设计.md](../06-安全与基础设施/02-人工接管与授权详细设计.md) -- Permission / PermissionSet / HumanGuard / 子 Agent 权限继承规则
- [01-Skills系统详细设计.md](../04-工具与扩展生态/01-Skills系统详细设计.md) -- SkillOrchestrator / can_spawn_agents 声明
- [06-安全边界.md](../../03-系统设计阶段/03-基础设施/06-安全边界.md) -- 深度限制、强制屏蔽工具列表
- [08-成本预算控制.md](../../03-系统设计阶段/03-基础设施/08-成本预算控制.md) -- BudgetManager / TokenBudget
