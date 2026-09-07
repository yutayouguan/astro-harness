# Agent 协作协议详细设计

> **Harness 当前基线（2026-09-07）**：已落地的协作面是 Codex V2 Agent Graph（`AgentControl` + `AgentGraphStore` + `AgentRegistry` + `ActivityBus`，位于 `crates/agent-subagents`）、durable mailbox（`subagents.db`）和六个模型工具（`spawn_agent`、`list_agents`、`send_message`、`followup_task`、`wait_agent`、`interrupt_agent`，位于 `crates/agent-core/src/exec/dispatch.rs`）。桌面控制面额外提供 `read_subagent_thread` 和 `close_subagent_thread`。资源配额：`max_threads=32`、`max_depth=8`、`max_running=8`（定义于 `agent_control_directory.rs`）。状态机：`PendingInit → Running → Completed { last_message } / Interrupted / Errored { message } → Shutdown`。Debate/MapReduce/Voting/MessageBus 等高层协议属于**目标设计**，codebase 中无对应实现。

> 版本：v1.0 | 日期：2026-08-12 | 状态：**目标设计（未实现）**
> 对应需求：F-30 子 Agent 派生（扩展）、F-31 多 Agent 协作
>
> **前置依赖**：
>
> - [Subagent 详细设计](06-Subagent详细设计.md)（`AgentControl` / `AgentGraphStore` / mailbox / 父子 Agent Thread）
> - [02-agent-runtime详细设计.md](02-agent-runtime详细设计.md)（`Session` / `submission_loop` / `SessionTask::run_turn`）
> - [03-MCP协议详细设计.md](../../04-详细设计阶段/04-工具与扩展生态/03-MCP协议详细设计.md)（MCP 通信层）

---

## 1. 协作架构概述

### 1.1 设计动机

现有 Codex V2 Agent Thread 系统（`crates/agent-subagents`）采用**父子树状模型**：父 Agent 通过 `spawn_agent` 派生子 Agent，子 Agent 完成后通过 `AgentGraphStore` mailbox 返回结果，兄弟节点之间可通过 `send_message` 通信。一个 `AgentControl` 共享于整个根会话的所有后代。这在单向任务委派和基本消息传递场景下足够高效，但无法覆盖以下需求：

1. **观点碰撞**：安全审计、代码评审等场景需要多个 Agent 从不同角度审视同一问题，通过辩论达成更全面的结论
2. **并行分治**：大规模文件重构、多语言翻译等场景需要将任务拆分给专业 Agent 并行处理，再合并结果
3. **质量仲裁**：关键决策场景需要多个 Agent 独立回答同一问题，通过投票或 LLM 裁判选出最优答案

### 1.2 三种协作模式

| 模式 | 适用场景 | Agent 关系 | 通信方式 |
|------|---------|-----------|---------|
| Hierarchical（层级式） | 简单子任务委派 | 父 → 子，单向 | 现有 V2 Agent Thread（`AgentControl` + 六工具） |
| Peer Discussion（对等讨论） | 辩论、评审、头脑风暴 | 平等对话，互相可见 | MessageBus 广播 |
| Division of Labor（分工协作） | 并行处理、MapReduce、投票 | 编排者 → 工作者，结果汇总 | MessageBus 定向 |

Hierarchical 模式完全复用现有 V2 Agent Thread 实现，本文档聚焦 Peer Discussion 和 Division of Labor 两种新模式。

### 1.3 与 V2 Agent Thread 的关系

协作协议**不替代**现有 V2 Agent Thread 系统，而是在其之上构建更高级的编排能力。`CollaborationOrchestrator`（目标设计）内部使用 `spawn_agent` 工具创建参与者 Agent（底层通过 `AgentRegistry` 预留路径和身份、`AgentGraphStore` 持久化线程图），但额外提供跨 Agent 消息路由和结果合成逻辑：

```text
Session (depth=0, 发起者)
    │
    ▼
CollaborationOrchestrator（目标设计）
    │
    ├─ 创建 CollaborationSession
    │
    ├─ spawn_agent × N  ← 复用现有 V2 派生机制
    │     ├─ Participant A (depth=1)
    │     ├─ Participant B (depth=1)
    │     └─ Participant C (depth=1)
    │     （受 AgentRegistry 配额约束：max_threads=32, max_running=8）
    │
    ├─ MessageBus 路由消息  ← 新增能力（目标设计）
    │     A ⇄ B ⇄ C（对等讨论模式）
    │     或 Orchestrator → A/B/C → Orchestrator（分工模式）
    │
    └─ SynthesisAgent (depth=1)  ← 可选：汇总合成
          └─ 读取所有参与者输出，生成最终结论
```

---

## 2. 协作会话模型

### 2.1 CollaborationSession

每次多 Agent 协作创建一个 `CollaborationSession`，持有参与者列表、共享目标和协议配置：

```rust
// crates/agent-runtime/src/collaboration/session.rs

use std::collections::HashMap;

/// 协作会话：一组 Agent 围绕共同目标协同工作
pub struct CollaborationSession {
    /// 会话唯一标识
    pub id: Uuid,
    /// 发起者的 conversation_id
    pub parent_conversation_id: String,
    /// 共同目标描述
    pub goal: String,
    /// 协作协议类型
    pub protocol: CollaborationProtocol,
    /// 参与者列表：participant_id → AgentRole
    pub participants: HashMap<Uuid, AgentRole>,
    /// 消息总线
    pub message_bus: Arc<MessageBus>,
    /// 会话状态
    pub state: CollaborationState,
    /// 创建时间
    pub created_at: Instant,
    /// 总预算上限（所有参与者共享）
    pub total_budget: TokenBudget,
    /// 最大协作轮次（防止无限讨论）
    pub max_rounds: u32,
    /// 当前轮次
    pub current_round: u32,
}

/// 协作协议枚举
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CollaborationProtocol {
    /// 辩论：多个 Agent 就同一问题展开讨论，每轮看到彼此回复
    Debate {
        rounds: u32,              // 辩论轮次，默认 3
        require_synthesis: bool,  // 是否需要合成 Agent 汇总
    },
    /// 轮询：Agent 按顺序依次发言，每个 Agent 看到前面所有人的回复
    RoundRobin {
        rounds: u32,
    },
    /// MapReduce：拆分任务 → 并行处理 → 合并结果
    MapReduce {
        subtask_count: usize,     // 子任务数量（若为 0 则由 split 阶段自动决定）
    },
    /// 投票：多个 Agent 独立回答同一问题，通过投票选出最优
    Voting {
        voter_count: usize,       // 投票 Agent 数量
        strategy: VotingStrategy, // 投票策略
    },
}

/// 投票策略
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VotingStrategy {
    /// 多数票胜出
    Majority,
    /// 加权投票（按 Agent 角色权重）
    Weighted,
    /// LLM 裁判：独立 Agent 作为裁判评判所有答案
    LlmJudge,
}

/// 协作会话生命周期状态
#[derive(Debug, Clone, PartialEq)]
pub enum CollaborationState {
    Initializing,   // 正在创建参与者
    Running,        // 协作进行中
    Synthesizing,   // 正在合成最终结果
    Completed,      // 协作完成
    Failed,         // 协作失败
    Cancelled,      // 被用户取消
}
```

### 2.2 与 AgentContext 的集成

协作会话中的每个参与者 Agent 仍通过 `Supervisor::spawn_child` 创建，拥有独立的 `AgentContext`。区别在于其系统提示中注入了角色定义和协作规则：

```rust
impl CollaborationSession {
    /// 为参与者构建系统提示
    fn build_participant_prompt(&self, role: &AgentRole) -> String {
        format!(
            "## 协作角色\n\
             你是一个 {persona}，专长领域：{expertise}。\n\n\
             ## 协作目标\n{goal}\n\n\
             ## 协作规则\n{rules}\n\n\
             ## 重要提醒\n\
             - 你的回复将被其他参与者看到\n\
             - 基于你的专业角色视角发表意见\n\
             - 如果不同意其他参与者的观点，请明确说明理由",
            persona = role.persona,
            expertise = role.expertise.join("、"),
            goal = self.goal,
            rules = self.protocol.rules_text(),
        )
    }
}
```

---

## 3. Debate 协议

### 3.1 设计概述

Debate（辩论）协议让两个或多个 Agent 就同一问题进行多轮讨论。每轮中每个 Agent 都能看到上一轮其他 Agent 的回复，从而实现观点碰撞和深度分析。适用场景包括代码评审、安全审计、技术方案评估等。

### 3.2 DebateOrchestrator

```rust
// crates/agent-runtime/src/collaboration/debate.rs

pub struct DebateOrchestrator {
    session: Arc<RwLock<CollaborationSession>>,
    supervisor: Arc<Mutex<Supervisor>>,
    emitter: Arc<dyn EventEmitter>,
}

impl DebateOrchestrator {
    /// 执行辩论协议
    ///
    /// 流程：
    /// 1. 为每个参与者创建子 Agent
    /// 2. 第一轮：所有 Agent 独立发表初始观点
    /// 3. 第 2..N 轮：每个 Agent 看到上一轮所有人的回复后再发表观点
    /// 4. 最终轮：合成 Agent 汇总所有观点，输出结论
    pub async fn execute(
        &self,
        parent_ctx: &AgentContext,
    ) -> Result<CollaborationResult, CollaborationError> {
        let session = self.session.read().await;
        let rounds = match &session.protocol {
            CollaborationProtocol::Debate { rounds, .. } => *rounds,
            _ => return Err(CollaborationError::ProtocolMismatch),
        };

        // ① 创建参与者 Agent（不启动 round_loop，仅初始化上下文）
        let mut participant_ids: Vec<Uuid> = Vec::new();
        for (_, role) in &session.participants {
            let config = SpawnConfig {
                goal: session.goal.clone(),
                context: String::new(),
                role: role.id.clone(),
                system_prompt_override: Some(
                    session.build_participant_prompt(role)
                ),
                token_budget: Some(
                    session.total_budget.allocate_equal_share(
                        session.participants.len()
                    )
                ),
                ..Default::default()
            };
            let id = self.supervisor.lock().await
                .spawn_child(config, parent_ctx).await?;
            participant_ids.push(id);
        }

        // ② 多轮辩论
        let mut round_history: Vec<DebateRound> = Vec::new();

        for round_num in 0..rounds {
            let mut round_responses: Vec<DebateResponse> = Vec::new();

            // 构建本轮上下文：包含前几轮所有人的回复
            let context_for_round = Self::build_round_context(
                &round_history, round_num
            );

            // 每个参与者依次发言（串行保证顺序一致性）
            for (idx, &participant_id) in participant_ids.iter().enumerate() {
                let role = session.participants.values()
                    .nth(idx).unwrap();

                // 向参与者注入本轮上下文
                let prompt = if round_num == 0 {
                    format!("请就以下问题发表你的观点：\n\n{}", session.goal)
                } else {
                    format!(
                        "以下是前 {} 轮的讨论记录：\n\n{}\n\n\
                         请基于以上讨论，发表你的第 {} 轮观点。\
                         你可以回应其他参与者的观点，补充新论据，或修正自己之前的看法。",
                        round_num, context_for_round, round_num + 1
                    )
                };

                // 通过 MessageBus 向参与者发送消息
                session.message_bus.send(CollaborationMessage {
                    from: Uuid::nil(), // 编排者
                    to: MessageTarget::Specific(participant_id),
                    content: prompt,
                    msg_type: MessageType::TaskAssignment,
                }).await;

                // 等待参与者回复
                let response = session.message_bus
                    .recv_from(participant_id, Duration::from_secs(120))
                    .await?;

                round_responses.push(DebateResponse {
                    participant_id,
                    role_name: role.persona.clone(),
                    content: response.content,
                });

                // 发射进度事件
                self.emitter.emit("collaboration_progress", serde_json::json!({
                    "session_id": session.id.to_string(),
                    "round": round_num + 1,
                    "total_rounds": rounds,
                    "participant": role.persona,
                    "status": "responded",
                })).await.ok();
            }

            round_history.push(DebateRound {
                round_num,
                responses: round_responses,
            });
        }

        // ③ 合成阶段
        let final_result = self.synthesize(
            &round_history, &session, parent_ctx
        ).await?;

        Ok(final_result)
    }

    /// 合成 Agent 汇总所有轮次观点
    async fn synthesize(
        &self,
        history: &[DebateRound],
        session: &CollaborationSession,
        parent_ctx: &AgentContext,
    ) -> Result<CollaborationResult, CollaborationError> {
        let synthesis_prompt = format!(
            "## 辩论汇总任务\n\n\
             以下是关于「{}」的 {} 轮辩论记录。\n\n\
             {}\n\n\
             请完成以下工作：\n\
             1. 总结各方核心观点\n\
             2. 识别共识和分歧\n\
             3. 给出综合结论和建议\n\
             4. 标注结论的置信度（高/中/低）",
            session.goal,
            history.len(),
            Self::format_debate_history(history),
        );

        let config = SpawnConfig {
            goal: synthesis_prompt,
            role: "synthesizer".into(),
            timeout_secs: 120,
            ..Default::default()
        };

        let mut supervisor = self.supervisor.lock().await;
        let synth_id = supervisor.spawn_child(config, parent_ctx).await?;
        let result = supervisor.join_one(synth_id).await?;

        Ok(CollaborationResult {
            session_id: session.id,
            protocol: session.protocol.clone(),
            synthesis: result.final_message,
            participant_outputs: history.iter()
                .flat_map(|r| r.responses.iter())
                .map(|r| ParticipantOutput {
                    role: r.role_name.clone(),
                    content: r.content.clone(),
                })
                .collect(),
            total_token_usage: result.token_usage,
        })
    }

    /// 构建轮次上下文
    fn build_round_context(
        history: &[DebateRound],
        _current_round: u32,
    ) -> String {
        history.iter().map(|round| {
            let responses = round.responses.iter()
                .map(|r| format!("**{}**：{}", r.role_name, r.content))
                .collect::<Vec<_>>()
                .join("\n\n");
            format!("### 第 {} 轮\n\n{}", round.round_num + 1, responses)
        }).collect::<Vec<_>>().join("\n\n---\n\n")
    }
}
```

### 3.3 辩论轮次数据结构

```rust
/// 单轮辩论数据
pub struct DebateRound {
    pub round_num: u32,
    pub responses: Vec<DebateResponse>,
}

/// 单个参与者的辩论回复
pub struct DebateResponse {
    pub participant_id: Uuid,
    pub role_name: String,
    pub content: String,
}
```

---

## 4. MapReduce 协议

### 4.1 设计概述

MapReduce 协议将一个大型任务拆分为 N 个子任务，分发给专业 Agent 并行处理，最后由 Reduce Agent 合并所有结果。适用场景包括大规模代码重构、多文件翻译、批量数据处理等。

### 4.2 三阶段流程

```text
┌─────────────────────────────────────────────────────────┐
│                    MapReduce 协议                        │
│                                                         │
│  ┌─────────┐    ┌─────────────────────┐    ┌─────────┐ │
│  │  Split   │    │       Map           │    │ Reduce  │ │
│  │  阶段    │───►│       阶段          │───►│  阶段   │ │
│  │         │    │  ┌───┐ ┌───┐ ┌───┐  │    │         │ │
│  │ 任务拆分 │    │  │ A │ │ B │ │ C │  │    │ 结果合并 │ │
│  └─────────┘    │  └───┘ └───┘ └───┘  │    └─────────┘ │
│                 │    ↓      ↓      ↓    │               │
│                 │  结果A  结果B  结果C   │               │
│                 └─────────────────────┘                 │
└─────────────────────────────────────────────────────────┘
```

### 4.3 MapReduceOrchestrator

```rust
// crates/agent-runtime/src/collaboration/map_reduce.rs

pub struct MapReduceOrchestrator {
    session: Arc<RwLock<CollaborationSession>>,
    supervisor: Arc<Mutex<Supervisor>>,
    emitter: Arc<dyn EventEmitter>,
}

/// 子任务定义（Split 阶段输出）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtask {
    pub index: usize,
    pub description: String,
    pub context: String,
    pub role_hint: String, // 建议的 Agent 角色
}

/// MapReduce 配置
pub struct MapReduceConfig {
    /// 拆分提示词（指导 Split Agent 如何拆分任务）
    pub split_prompt: Option<String>,
    /// 合并提示词（指导 Reduce Agent 如何合并结果）
    pub reduce_prompt: Option<String>,
    /// 最大并行 Map Agent 数量
    pub max_parallel: usize,
    /// 单个 Map Agent 超时
    pub map_timeout_secs: u32,
}

impl MapReduceOrchestrator {
    /// 执行 MapReduce 协议
    pub async fn execute(
        &self,
        config: MapReduceConfig,
        parent_ctx: &AgentContext,
    ) -> Result<CollaborationResult, CollaborationError> {
        let session = self.session.read().await;

        // ① Split 阶段：将大任务拆分为子任务
        let subtasks = self.split_phase(
            &session.goal,
            &config,
            parent_ctx,
        ).await?;

        self.emitter.emit("collaboration_progress", serde_json::json!({
            "session_id": session.id.to_string(),
            "phase": "split_complete",
            "subtask_count": subtasks.len(),
        })).await.ok();

        // ② Map 阶段：并行执行所有子任务
        let map_results = self.map_phase(
            &subtasks,
            &config,
            parent_ctx,
        ).await?;

        self.emitter.emit("collaboration_progress", serde_json::json!({
            "session_id": session.id.to_string(),
            "phase": "map_complete",
            "completed": map_results.len(),
            "total": subtasks.len(),
        })).await.ok();

        // ③ Reduce 阶段：合并所有子任务结果
        let final_result = self.reduce_phase(
            &subtasks,
            &map_results,
            &config,
            parent_ctx,
        ).await?;

        Ok(final_result)
    }

    /// Split 阶段：使用 LLM 将任务拆分为子任务
    async fn split_phase(
        &self,
        goal: &str,
        config: &MapReduceConfig,
        parent_ctx: &AgentContext,
    ) -> Result<Vec<Subtask>, CollaborationError> {
        let split_prompt = config.split_prompt.clone().unwrap_or_else(|| {
            format!(
                "请将以下任务拆分为独立的子任务，每个子任务可以由不同的专业 Agent 独立完成。\n\n\
                 ## 任务\n{}\n\n\
                 ## 输出格式\n\
                 请以 JSON 数组格式输出子任务列表，每个子任务包含：\n\
                 - description: 子任务描述\n\
                 - context: 子任务所需的上下文信息\n\
                 - role_hint: 建议的执行角色（如 coder / reviewer / researcher）",
                goal
            )
        });

        let config_spawn = SpawnConfig {
            goal: split_prompt,
            role: "splitter".into(),
            timeout_secs: 60,
            ..Default::default()
        };

        let mut supervisor = self.supervisor.lock().await;
        let split_id = supervisor.spawn_child(config_spawn, parent_ctx).await?;
        let result = supervisor.join_one(split_id).await?;

        // 从 final_message 中解析 JSON 子任务列表
        parse_subtasks(&result.final_message)
    }

    /// Map 阶段：并行执行子任务（受 Supervisor semaphore 控制）
    async fn map_phase(
        &self,
        subtasks: &[Subtask],
        config: &MapReduceConfig,
        parent_ctx: &AgentContext,
    ) -> Result<Vec<SubAgentResult>, CollaborationError> {
        let mut child_ids = Vec::with_capacity(subtasks.len());

        // 批量派生（Supervisor 的 semaphore 自动限流）
        for subtask in subtasks {
            let spawn_config = SpawnConfig {
                goal: subtask.description.clone(),
                context: subtask.context.clone(),
                role: subtask.role_hint.clone(),
                timeout_secs: config.map_timeout_secs,
                ..Default::default()
            };
            let mut supervisor = self.supervisor.lock().await;
            let id = supervisor.spawn_child(spawn_config, parent_ctx).await?;
            child_ids.push(id);
        }

        // 等待所有子任务完成
        let mut supervisor = self.supervisor.lock().await;
        let results = supervisor.join_all().await;

        // 收集成功的结果，失败的子任务标记错误但不阻断整体流程
        let mut map_results = Vec::new();
        for result in results {
            match result {
                Ok(r) => map_results.push(r),
                Err(e) => {
                    tracing::warn!("MapReduce: 子任务执行失败: {}", e);
                    map_results.push(SubAgentResult {
                        findings: vec![format!("子任务失败: {}", e)],
                        issues: vec![e.to_string()],
                        ..SubAgentResult::default()
                    });
                }
            }
        }

        Ok(map_results)
    }

    /// Reduce 阶段：合并所有子任务结果
    async fn reduce_phase(
        &self,
        subtasks: &[Subtask],
        results: &[SubAgentResult],
        config: &MapReduceConfig,
        parent_ctx: &AgentContext,
    ) -> Result<CollaborationResult, CollaborationError> {
        let reduce_input = subtasks.iter().zip(results.iter())
            .enumerate()
            .map(|(i, (task, result))| {
                format!(
                    "### 子任务 {} — {}\n**结果**：{}\n**修改文件**：{}\n**问题**：{}",
                    i + 1,
                    task.description,
                    result.final_message,
                    result.modified_files.join(", "),
                    result.issues.join("; "),
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");

        let reduce_prompt = config.reduce_prompt.clone().unwrap_or_else(|| {
            "请合并以下子任务的执行结果，生成统一的最终报告。\
             检查子任务之间是否存在冲突或遗漏，并给出整体评估。".into()
        });

        let spawn_config = SpawnConfig {
            goal: format!("{}\n\n{}", reduce_prompt, reduce_input),
            role: "reducer".into(),
            timeout_secs: 120,
            ..Default::default()
        };

        let mut supervisor = self.supervisor.lock().await;
        let reduce_id = supervisor.spawn_child(spawn_config, parent_ctx).await?;
        let result = supervisor.join_one(reduce_id).await?;
        let session = self.session.read().await;

        Ok(CollaborationResult {
            session_id: session.id,
            protocol: session.protocol.clone(),
            synthesis: result.final_message,
            participant_outputs: results.iter()
                .enumerate()
                .map(|(i, r)| ParticipantOutput {
                    role: format!("mapper_{}", i),
                    content: r.final_message.clone(),
                })
                .collect(),
            total_token_usage: aggregate_token_usage(results, &result.token_usage),
        })
    }
}
```

---

## 5. Voting 协议

### 5.1 设计概述

Voting（投票）协议让 N 个 Agent 独立回答同一问题（互相不可见），然后通过投票策略选出最优答案。适用于需要高可靠性输出的场景，如关键代码生成、重要决策判断。

### 5.2 VotingOrchestrator

```rust
// crates/agent-runtime/src/collaboration/voting.rs

pub struct VotingOrchestrator {
    session: Arc<RwLock<CollaborationSession>>,
    supervisor: Arc<Mutex<Supervisor>>,
    emitter: Arc<dyn EventEmitter>,
}

impl VotingOrchestrator {
    /// 执行投票协议
    pub async fn execute(
        &self,
        parent_ctx: &AgentContext,
    ) -> Result<CollaborationResult, CollaborationError> {
        let session = self.session.read().await;
        let (voter_count, strategy) = match &session.protocol {
            CollaborationProtocol::Voting { voter_count, strategy } => {
                (*voter_count, strategy.clone())
            }
            _ => return Err(CollaborationError::ProtocolMismatch),
        };

        // ① 并行派生投票 Agent（互相不可见）
        let mut child_ids = Vec::with_capacity(voter_count);
        for i in 0..voter_count {
            let config = SpawnConfig {
                goal: session.goal.clone(),
                context: format!(
                    "你是第 {} 个独立评审者。请独立回答以下问题，\
                     不要参考其他评审者的意见。给出你的答案和详细推理过程。",
                    i + 1
                ),
                role: "voter".into(),
                timeout_secs: 180,
                token_budget: Some(
                    session.total_budget.allocate_equal_share(voter_count + 1)
                ),
                ..Default::default()
            };
            let mut supervisor = self.supervisor.lock().await;
            let id = supervisor.spawn_child(config, parent_ctx).await?;
            child_ids.push(id);
        }

        // ② 收集所有投票结果
        let mut supervisor = self.supervisor.lock().await;
        let vote_results = supervisor.join_all().await;
        drop(supervisor);

        let votes: Vec<VoteEntry> = vote_results.into_iter()
            .enumerate()
            .filter_map(|(i, r)| {
                r.ok().map(|result| VoteEntry {
                    voter_index: i,
                    answer: result.final_message,
                    token_usage: result.token_usage,
                })
            })
            .collect();

        // ③ 根据策略决定最终结果
        let winner = match strategy {
            VotingStrategy::Majority => {
                self.majority_vote(&votes)
            }
            VotingStrategy::Weighted => {
                self.weighted_vote(&votes, &session.participants)
            }
            VotingStrategy::LlmJudge => {
                self.llm_judge_vote(&votes, &session, parent_ctx).await?
            }
        };

        Ok(CollaborationResult {
            session_id: session.id,
            protocol: session.protocol.clone(),
            synthesis: winner.answer,
            participant_outputs: votes.iter()
                .map(|v| ParticipantOutput {
                    role: format!("voter_{}", v.voter_index),
                    content: v.answer.clone(),
                })
                .collect(),
            total_token_usage: votes.iter()
                .fold(TokenUsage::default(), |acc, v| acc.merge(&v.token_usage)),
        })
    }

    /// 多数票：按语义相似度聚类后选择最大簇的代表
    fn majority_vote(&self, votes: &[VoteEntry]) -> VoteEntry {
        // 简化实现：选择与其他答案平均相似度最高的答案
        // 生产环境可接入嵌入模型做语义聚类
        votes.first().cloned().unwrap_or_default()
    }

    /// 加权投票：按角色权重加权
    fn weighted_vote(
        &self,
        votes: &[VoteEntry],
        participants: &HashMap<Uuid, AgentRole>,
    ) -> VoteEntry {
        // 根据 AgentRole.weight 对每个投票加权，选择最高分答案
        votes.first().cloned().unwrap_or_default()
    }

    /// LLM 裁判：独立 Agent 评判所有答案
    async fn llm_judge_vote(
        &self,
        votes: &[VoteEntry],
        session: &CollaborationSession,
        parent_ctx: &AgentContext,
    ) -> Result<VoteEntry, CollaborationError> {
        let candidates = votes.iter()
            .map(|v| format!(
                "### 候选答案 {} \n{}",
                v.voter_index + 1, v.answer
            ))
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");

        let judge_prompt = format!(
            "## 裁判任务\n\n\
             问题：{}\n\n\
             以下是 {} 个独立评审者给出的答案：\n\n{}\n\n\
             请评判哪个答案最优，考虑以下维度：\n\
             1. 正确性\n2. 完整性\n3. 推理质量\n4. 实用性\n\n\
             输出格式：先给出分析，最后一行写「最优答案：X」（X 为候选编号）",
            session.goal,
            votes.len(),
            candidates,
        );

        let config = SpawnConfig {
            goal: judge_prompt,
            role: "judge".into(),
            timeout_secs: 120,
            ..Default::default()
        };

        let mut supervisor = self.supervisor.lock().await;
        let judge_id = supervisor.spawn_child(config, parent_ctx).await?;
        let result = supervisor.join_one(judge_id).await?;

        // 从裁判结果中解析最优答案编号
        let winner_idx = parse_judge_winner(&result.final_message)
            .unwrap_or(0);
        let winner = votes.get(winner_idx)
            .cloned()
            .unwrap_or_else(|| votes[0].clone());

        Ok(winner)
    }
}

/// 单个投票条目
#[derive(Debug, Clone, Default)]
pub struct VoteEntry {
    pub voter_index: usize,
    pub answer: String,
    pub token_usage: TokenUsage,
}
```

---

## 6. Agent 角色定义

### 6.1 AgentRole 结构

每个协作参与者通过 `AgentRole` 定义其身份、专业领域和可用工具：

```rust
// crates/agent-runtime/src/collaboration/role.rs

/// Agent 协作角色定义
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRole {
    /// 角色唯一标识（如 "security_reviewer"）
    pub id: String,
    /// 角色人设描述（注入系统提示）
    pub persona: String,
    /// 专业领域列表
    pub expertise: Vec<String>,
    /// 允许使用的工具白名单（为空则继承父 Agent 工具集）
    pub allowed_tools: Vec<String>,
    /// 建议使用的模型（可选，默认继承发起者模型）
    pub model_hint: Option<String>,
    /// 在加权投票中的权重（0.0 - 1.0）
    pub weight: f64,
}

impl AgentRole {
    /// 预置角色模板
    pub fn template(name: &str) -> Option<Self> {
        match name {
            "coder" => Some(Self {
                id: "coder".into(),
                persona: "资深软件工程师".into(),
                expertise: vec![
                    "代码编写".into(),
                    "架构设计".into(),
                    "重构优化".into(),
                ],
                allowed_tools: vec![
                    "file_read".into(),
                    "file_write".into(),
                    "file_edit".into(),
                    "shell_exec".into(),
                    "memory_search".into(),
                ],
                model_hint: None,
                weight: 1.0,
            }),
            "reviewer" => Some(Self {
                id: "reviewer".into(),
                persona: "严格的代码审查专家".into(),
                expertise: vec![
                    "代码质量".into(),
                    "最佳实践".into(),
                    "性能分析".into(),
                    "安全漏洞检测".into(),
                ],
                allowed_tools: vec![
                    "file_read".into(),
                    "memory_search".into(),
                ],
                model_hint: None,
                weight: 1.0,
            }),
            "researcher" => Some(Self {
                id: "researcher".into(),
                persona: "技术调研员".into(),
                expertise: vec![
                    "信息检索".into(),
                    "文档分析".into(),
                    "技术对比".into(),
                ],
                allowed_tools: vec![
                    "file_read".into(),
                    "memory_search".into(),
                    "web_fetch".into(),
                    "http_request".into(),
                ],
                model_hint: Some("claude-haiku-4-5".into()),
                weight: 0.8,
            }),
            "writer" => Some(Self {
                id: "writer".into(),
                persona: "技术文档撰写专家".into(),
                expertise: vec![
                    "技术写作".into(),
                    "文档组织".into(),
                    "用户友好的表达".into(),
                ],
                allowed_tools: vec![
                    "file_read".into(),
                    "file_write".into(),
                    "memory_search".into(),
                ],
                model_hint: None,
                weight: 0.8,
            }),
            "critic" => Some(Self {
                id: "critic".into(),
                persona: "批判性思维专家，专注于发现问题和盲点".into(),
                expertise: vec![
                    "逻辑分析".into(),
                    "风险识别".into(),
                    "反面论证".into(),
                ],
                allowed_tools: vec![
                    "file_read".into(),
                    "memory_search".into(),
                ],
                model_hint: None,
                weight: 0.9,
            }),
            _ => None,
        }
    }
}
```

### 6.2 角色与工具集的关系

角色定义中的 `allowed_tools` 与 Supervisor 的权限交集模型协同工作。参与者实际可用的工具集 = 发起者权限 $\cap$ 角色 allowed_tools $-$ 强制屏蔽列表：

```text
发起者（depth=0）权限集:  { ReadFile, WriteFile, ExecuteBash, NetworkRead, SpawnAgent }
角色 allowed_tools:       { file_read, file_write, web_fetch }
强制屏蔽列表（depth>0）:  { git_push, publish, memory_write, ... }

参与者实际工具集:  { file_read, file_write, web_fetch }
                  （web_fetch 需要 NetworkRead 权限，发起者持有，故保留）
```

---

## 7. 跨 Agent 通信

### 7.1 MessageBus

MessageBus 是协作会话内部的消息路由层，提供参与者之间的类型化通信通道。所有消息传递通过 MessageBus 中转，不允许参与者直接访问彼此的 `AgentContext`（无共享可变状态）。

```rust
// crates/agent-runtime/src/collaboration/message_bus.rs

use tokio::sync::mpsc;

/// 协作消息总线
pub struct MessageBus {
    /// 每个参与者的接收通道
    channels: DashMap<Uuid, mpsc::Sender<CollaborationMessage>>,
    /// 广播通道（所有参与者可见）
    broadcast_tx: broadcast::Sender<CollaborationMessage>,
    /// 消息历史（供合成 Agent 使用）
    history: RwLock<Vec<CollaborationMessage>>,
}

/// 协作消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationMessage {
    /// 发送者 ID（Uuid::nil() 表示编排者）
    pub from: Uuid,
    /// 接收者
    pub to: MessageTarget,
    /// 消息内容
    pub content: String,
    /// 消息类型
    pub msg_type: MessageType,
}

/// 消息目标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageTarget {
    /// 指定参与者
    Specific(Uuid),
    /// 广播给所有参与者
    Broadcast,
    /// 发送给编排者（结果回传）
    Orchestrator,
}

/// 消息类型枚举
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    /// 任务分配（编排者 → 参与者）
    TaskAssignment,
    /// 任务响应（参与者 → 编排者）
    Response,
    /// 反馈意见（参与者 → 参与者，Debate 模式）
    Feedback,
    /// 投票（参与者 → 编排者，Voting 模式）
    Vote,
    /// 合成请求（编排者 → 合成 Agent）
    SynthesisRequest,
}

impl MessageBus {
    pub fn new(capacity: usize) -> Self {
        let (broadcast_tx, _) = broadcast::channel(capacity);
        Self {
            channels: DashMap::new(),
            broadcast_tx,
            history: RwLock::new(Vec::new()),
        }
    }

    /// 注册参与者，返回其接收端
    pub fn register(&self, participant_id: Uuid) -> mpsc::Receiver<CollaborationMessage> {
        let (tx, rx) = mpsc::channel(32);
        self.channels.insert(participant_id, tx);
        rx
    }

    /// 发送消息
    pub async fn send(&self, msg: CollaborationMessage) -> Result<(), CollaborationError> {
        // 记录到历史
        self.history.write().await.push(msg.clone());

        match &msg.to {
            MessageTarget::Specific(id) => {
                if let Some(tx) = self.channels.get(id) {
                    tx.send(msg).await
                        .map_err(|_| CollaborationError::ChannelClosed)?;
                }
            }
            MessageTarget::Broadcast => {
                self.broadcast_tx.send(msg)
                    .map_err(|_| CollaborationError::ChannelClosed)?;
            }
            MessageTarget::Orchestrator => {
                // 编排者通过 recv_from 直接接收
            }
        }
        Ok(())
    }

    /// 从指定参与者接收下一条消息（带超时）
    pub async fn recv_from(
        &self,
        participant_id: Uuid,
        timeout: Duration,
    ) -> Result<CollaborationMessage, CollaborationError> {
        // 编排者从参与者的 Response 消息中接收
        // 实际实现通过 oneshot 或 watch channel 完成
        tokio::time::timeout(timeout, async {
            // 等待指定参与者发送 Response 类型消息
            loop {
                let history = self.history.read().await;
                if let Some(msg) = history.iter().rev().find(|m| {
                    m.from == participant_id
                        && matches!(m.msg_type, MessageType::Response)
                }) {
                    return Ok(msg.clone());
                }
                drop(history);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .map_err(|_| CollaborationError::Timeout)?
    }

    /// 获取完整消息历史（供合成 Agent 使用）
    pub async fn get_history(&self) -> Vec<CollaborationMessage> {
        self.history.read().await.clone()
    }
}
```

### 7.2 通信不变量

```text
INVARIANT 1: 参与者之间不共享 AgentContext
             所有信息交换通过 MessageBus 消息传递

INVARIANT 2: MessageBus 消息不可变
             发送后不可修改，仅追加

INVARIANT 3: Debate 模式下消息可见性规则
             参与者仅在当前轮次开始时看到上一轮所有人的回复
             不会看到同轮次中其他参与者的实时回复

INVARIANT 4: Voting 模式下消息隔离
             投票 Agent 之间完全隔离，互不可见
```

---

## 8. 资源与预算管理

### 8.1 预算分配策略

协作会话的总预算从发起者（父 Agent）的剩余预算中分配。不同协议的预算分配方式：

```rust
// crates/agent-runtime/src/collaboration/budget.rs

impl TokenBudget {
    /// 为协作会话分配总预算
    pub fn allocate_collaboration_budget(
        &self,
        protocol: &CollaborationProtocol,
    ) -> TokenBudget {
        let remaining = self.remaining();

        // 协作预算最多占父 Agent 剩余预算的 2/3
        let collab_budget = (remaining * 2) / 3;
        self.reserve(collab_budget);

        TokenBudget::new(collab_budget)
    }

    /// 按参与者数量平均分配
    pub fn allocate_equal_share(&self, participant_count: usize) -> TokenBudget {
        let share = self.remaining() / participant_count as u64;
        self.reserve(share);
        TokenBudget::new(share)
    }
}
```

### 8.2 各协议的预算分配明细

| 协议 | 参与者预算 | 额外预算 | 说明 |
|------|-----------|---------|------|
| Debate（3 轮 / 2 人） | 总预算的 40% / 人 | 20% 给合成 Agent | 轮次越多单轮预算越少 |
| MapReduce | 总预算的 10% 给 Split | 70% 平分给 Map Agent | 20% 给 Reduce Agent |
| Voting（3 人） | 总预算的 25% / 人 | 25% 给 LLM Judge | 加权投票无需 Judge 预算 |

### 8.3 并发限制

协作会话的并发 Agent 数量受 Supervisor 的 Semaphore 控制。由于参与者本质上是子 Agent，它们共享 Supervisor 的 `max_concurrent` 限制（默认 3）：

```rust
impl CollaborationOrchestrator {
    /// 检查协作是否可行（参与者数量不超过并发限制）
    fn check_feasibility(
        &self,
        protocol: &CollaborationProtocol,
        supervisor: &Supervisor,
    ) -> Result<(), CollaborationError> {
        let required = match protocol {
            CollaborationProtocol::Debate { .. } => {
                // Debate 串行执行，每次只需 1 个 permit
                1
            }
            CollaborationProtocol::MapReduce { subtask_count, .. } => {
                // MapReduce 并行执行，需要 min(subtask_count, max_concurrent) 个 permit
                (*subtask_count).min(supervisor.max_concurrent())
            }
            CollaborationProtocol::Voting { voter_count, .. } => {
                // Voting 并行执行
                (*voter_count).min(supervisor.max_concurrent())
            }
            _ => 1,
        };

        let available = supervisor.available_permits();
        if required > available {
            return Err(CollaborationError::InsufficientCapacity {
                required,
                available,
            });
        }
        Ok(())
    }
}
```

### 8.4 超时控制

每个协作协议有独立的总超时限制，防止无限运行：

| 协议 | 单轮超时 | 总超时 | 超时行为 |
|------|---------|-------|---------|
| Debate | 120s / 参与者 / 轮 | rounds * participants * 120s | 跳过超时参与者，其他人继续 |
| MapReduce | 300s / Map Agent | 600s（含 Split + Reduce） | 超时的 Map 任务标记为失败，Reduce 基于已有结果合并 |
| Voting | 180s / 投票 Agent | 360s（含 Judge） | 丢弃超时投票，基于已收集的投票决策 |

---

## 9. Tauri Commands + 前端

### 9.1 Tauri Commands

```rust
// apps/desktop/src-tauri/src/commands/collaboration.rs

/// 创建协作会话
#[tauri::command]
pub async fn create_collaboration(
    state: State<'_, AppState>,
    conversation_id: String,
    goal: String,
    protocol: CollaborationProtocol,
    roles: Vec<AgentRole>,
) -> Result<CollaborationSessionInfo, AppError> {
    let session = state.runtime.get_session(&conversation_id)
        .ok_or(AppError::SessionNotFound)?;

    let collab = state.runtime.collaboration_orchestrator()
        .create_session(
            &conversation_id,
            goal,
            protocol,
            roles,
            &session.context,
        )
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(CollaborationSessionInfo {
        id: collab.id.to_string(),
        goal: collab.goal,
        protocol: format!("{:?}", collab.protocol),
        participant_count: collab.participants.len(),
        state: format!("{:?}", collab.state),
    })
}

/// 查询协作会话中的参与 Agent 列表
#[tauri::command]
pub async fn list_collaboration_agents(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<CollaborationAgentInfo>, AppError> {
    let id = Uuid::parse_str(&session_id)?;
    let orchestrator = state.runtime.collaboration_orchestrator();
    let session = orchestrator.get_session(id)
        .ok_or(AppError::NotFound("协作会话不存在".into()))?;

    let session = session.read().await;
    Ok(session.participants.iter().map(|(id, role)| {
        CollaborationAgentInfo {
            id: id.to_string(),
            role_id: role.id.clone(),
            persona: role.persona.clone(),
            expertise: role.expertise.clone(),
            weight: role.weight,
        }
    }).collect())
}

/// 获取协作会话的最终结果
#[tauri::command]
pub async fn get_collaboration_result(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<CollaborationResultInfo, AppError> {
    let id = Uuid::parse_str(&session_id)?;
    let orchestrator = state.runtime.collaboration_orchestrator();
    let result = orchestrator.get_result(id)
        .ok_or(AppError::NotFound("协作尚未完成".into()))?;

    Ok(CollaborationResultInfo {
        session_id: result.session_id.to_string(),
        protocol: format!("{:?}", result.protocol),
        synthesis: result.synthesis,
        participant_outputs: result.participant_outputs.iter()
            .map(|p| ParticipantOutputInfo {
                role: p.role.clone(),
                content: p.content.clone(),
            })
            .collect(),
        total_tokens: result.total_token_usage.input_tokens
            + result.total_token_usage.output_tokens,
        total_cost_usd: result.total_token_usage.total_cost_usd,
    })
}
```

### 9.2 前端事件

协作系统通过 `EventEmitter` 推送以下事件到前端：

| 事件名 | 触发时机 | Payload |
|--------|---------|---------|
| `collaboration_created` | 协作会话创建 | `{ session_id, goal, protocol, participant_count }` |
| `collaboration_progress` | 协作进度更新 | `{ session_id, phase, round, participant, status }` |
| `collaboration_completed` | 协作完成 | `{ session_id, total_tokens, total_cost_usd }` |
| `collaboration_failed` | 协作失败 | `{ session_id, error }` |

### 9.3 CollaborationView 前端组件

```typescript
// apps/desktop/src/components/CollaborationView.tsx

import { useEffect, useState } from 'react';
import { listen, invoke } from '@tauri-apps/api';

interface CollaborationViewProps {
  sessionId: string;
}

interface ParticipantThread {
  role: string;
  persona: string;
  messages: Array<{
    round: number;
    content: string;
    timestamp: number;
  }>;
}

function CollaborationView({ sessionId }: CollaborationViewProps) {
  const [threads, setThreads] = useState<ParticipantThread[]>([]);
  const [synthesis, setSynthesis] = useState<string | null>(null);
  const [phase, setPhase] = useState<string>('initializing');

  useEffect(() => {
    // 监听协作进度事件
    const unlisten = listen<{
      session_id: string;
      phase: string;
      round?: number;
      participant?: string;
      content?: string;
    }>('collaboration_progress', ({ payload }) => {
      if (payload.session_id !== sessionId) return;
      setPhase(payload.phase);

      if (payload.participant && payload.content) {
        setThreads(prev => {
          const updated = [...prev];
          let thread = updated.find(t => t.persona === payload.participant);
          if (!thread) {
            thread = { role: '', persona: payload.participant!, messages: [] };
            updated.push(thread);
          }
          thread.messages.push({
            round: payload.round ?? 0,
            content: payload.content!,
            timestamp: Date.now(),
          });
          return updated;
        });
      }
    });

    return () => { unlisten.then(f => f()); };
  }, [sessionId]);

  useEffect(() => {
    // 监听协作完成事件
    const unlisten = listen<{ session_id: string }>('collaboration_completed', async ({ payload }) => {
      if (payload.session_id !== sessionId) return;
      const result = await invoke<{ synthesis: string }>('get_collaboration_result', {
        sessionId,
      });
      setSynthesis(result.synthesis);
    });
    return () => { unlisten.then(f => f()); };
  }, [sessionId]);

  return (
    <div className="collaboration-view">
      <div className="phase-indicator">
        <span className={`phase ${phase}`}>{phaseLabel(phase)}</span>
      </div>

      <div className="thread-container">
        {threads.map((thread, idx) => (
          <div key={idx} className="participant-thread">
            <div className="thread-header">
              <span className="persona">{thread.persona}</span>
              <span className="message-count">
                {thread.messages.length} 条发言
              </span>
            </div>
            <div className="thread-messages">
              {thread.messages.map((msg, msgIdx) => (
                <div key={msgIdx} className="message">
                  <span className="round-badge">R{msg.round + 1}</span>
                  <p>{msg.content}</p>
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>

      {synthesis && (
        <div className="synthesis-panel">
          <h3>综合结论</h3>
          <div className="synthesis-content">{synthesis}</div>
        </div>
      )}
    </div>
  );
}

function phaseLabel(phase: string): string {
  const labels: Record<string, string> = {
    initializing: '初始化参与者',
    split_complete: '任务拆分完成',
    map_complete: '并行处理完成',
    running: '协作进行中',
    synthesizing: '合成结论',
    completed: '已完成',
  };
  return labels[phase] ?? phase;
}
```

---

## 10. 相关文档

- [Subagent 详细设计](06-Subagent详细设计.md) -- AgentControl、AgentGraphStore、mailbox、运行时恢复与权限收窄
- [02-agent-runtime详细设计.md](02-agent-runtime详细设计.md) -- AgentExecutor / round_loop / SessionManager
- [03-MCP协议详细设计.md](../../04-详细设计阶段/04-工具与扩展生态/03-MCP协议详细设计.md) -- MCP 客户端/服务端，跨进程工具通信
- [01-agent-core详细设计.md](01-agent-core详细设计.md) -- AgentContext / ToolRegistry / Permission
- [06-安全边界.md](../../03-系统设计阶段/03-基础设施/06-安全边界.md) -- 深度限制、强制屏蔽工具列表
- [08-成本预算控制.md](../../03-系统设计阶段/03-基础设施/08-成本预算控制.md) -- BudgetManager / TokenBudget
- [02-人工接管与授权详细设计.md](../06-安全与基础设施/02-人工接管与授权详细设计.md) -- HumanGuard、子 Agent 权限继承
