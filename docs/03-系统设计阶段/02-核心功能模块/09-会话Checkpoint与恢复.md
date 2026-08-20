# 会话 Checkpoint 与恢复

> 文档状态：定稿 | 阶段：系统设计 | 关联：agent-runtime round_loop、崩溃恢复

---

## 一、设计目标

让用户可以在任何时刻安全地关闭应用、切换设备、甚至崩溃后，回来继续之前的 Agent 任务，而不丢失已完成的工作进度。

**核心原则**：Agent 的执行状态可恢复，但不追求精确重放（LLM 本质非确定性）。恢复的本质是"加载完整历史 + 告诉 LLM 从哪里继续"。

---

## 二、三种恢复场景

| 场景 | 触发条件 | 恢复能力 | 实现复杂度 |
| ---- | ---- | ---- | ---- |
| **暂停恢复** | 用户主动暂停（`/pause`）后恢复 | 完美恢复（内存状态完整） | 低（已实现） |
| **正常关闭恢复** | 用户关闭应用后重新打开 | 高（checkpoint 已写入） | 中 |
| **崩溃恢复** | 进程崩溃或强杀 | 中等（最后一轮可能不完整） | 中（已实现基础版） |

---

## 三、Checkpoint 数据模型

### 3.1 会话状态表（conversations 扩展）

复用现有 `conversations` 表，增加 checkpoint 相关字段：

```sql
ALTER TABLE conversations ADD COLUMN agent_state TEXT DEFAULT 'idle'
    CHECK(agent_state IN ('idle','running','paused','interrupted','done','failed'));
ALTER TABLE conversations ADD COLUMN last_checkpoint_at INTEGER;
ALTER TABLE conversations ADD COLUMN checkpoint_round INTEGER DEFAULT 0;
ALTER TABLE conversations ADD COLUMN checkpoint_meta TEXT DEFAULT '{}';
```

`checkpoint_meta` 存储 JSON：

```json
{
  "round": 15,
  "model": "claude-sonnet-4",
  "pending_tool_calls": ["call_abc123"],
  "active_sub_agents": ["uuid-1", "uuid-2"],
  "token_used": 45000,
  "cost_usd": 0.23,
  "last_tool_result_id": "msg_xyz789"
}
```

### 3.2 Checkpoint 写入时机

在 `round_loop` 的每个完整轮次结束后（所有工具执行完毕、结果已注入上下文）写入 checkpoint：

```rust
// round_loop 内部，每轮结束时
async fn write_checkpoint(&self, round: u32) {
    let meta = CheckpointMeta {
        round,
        model: self.turn_context.model.clone(),
        pending_tool_calls: vec![],  // 本轮已完成，无 pending
        active_sub_agents: self.supervisor.active_child_ids(),
        token_used: self.budget.used(),
        cost_usd: self.cost_tracker.total_usd(),
        last_tool_result_id: self.messages.last()
            .filter(|m| m.role == MessageRole::Tool)
            .map(|m| m.id.clone()),
    };
    
    sqlx::query(
        "UPDATE conversations SET 
            agent_state = 'running',
            checkpoint_round = ?1,
            checkpoint_meta = ?2,
            last_checkpoint_at = ?3
         WHERE id = ?4"
    )
    .bind(round as i32)
    .bind(serde_json::to_string(&meta)?)
    .bind(now_millis())
    .bind(&self.conversation_id)
    .execute(&self.pool).await?;
}
```

### 3.3 正常结束时清除 checkpoint

```rust
// Agent 正常完成（Done / Failed）
async fn clear_checkpoint(&self, final_state: &str) {
    sqlx::query(
        "UPDATE conversations SET agent_state = ?1, checkpoint_meta = '{}' WHERE id = ?2"
    )
    .bind(final_state)
    .bind(&self.conversation_id)
    .execute(&self.pool).await?;
}
```

---

## 四、恢复流程

### 4.1 应用启动时扫描

```rust
pub async fn scan_and_recover(pool: &SqlitePool, emitter: &dyn EventEmitter) -> Vec<RecoverableSession> {
    // 查找所有未完成的会话
    let sessions = sqlx::query_as::<_, RecoverableSession>(
        "SELECT id, workspace_id, title, agent_state, checkpoint_round, checkpoint_meta, last_checkpoint_at
         FROM conversations
         WHERE agent_state IN ('running', 'paused')
         ORDER BY last_checkpoint_at DESC"
    )
    .fetch_all(pool).await?;
    
    // 标记为 interrupted（区别于正常 running）
    for session in &sessions {
        if session.agent_state == "running" {
            sqlx::query("UPDATE conversations SET agent_state = 'interrupted' WHERE id = ?")
                .bind(&session.id)
                .execute(pool).await?;
        }
    }
    
    sessions
}
```

### 4.2 用户确认恢复

前端 UI 展示可恢复的会话列表：

```text
┌─────────────────────────────────────────────────────────┐
│  🔄 检测到上次未完成的任务                                │
├─────────────────────────────────────────────────────────┤
│                                                         │
│  📋 "帮我重构 auth 模块" — 已完成 15 轮，消耗 $0.23      │
│     中断时间：2 小时前 | 模型：claude-sonnet-4            │
│     [继续任务]  [查看对话]  [放弃]                        │
│                                                         │
│  📋 "分析竞品报告" — 已完成 8 轮，消耗 $0.12              │
│     中断时间：昨天 | 模型：gpt-4o                         │
│     [继续任务]  [查看对话]  [放弃]                        │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

### 4.3 恢复执行

```rust
pub async fn resume_session(
    pool: &SqlitePool,
    conversation_id: &str,
    session: &AgentSession,
    emitter: &dyn EventEmitter,
) -> Result<()> {
    // 1. 加载完整消息历史
    let messages = MessageRepo::new(pool)
        .list_by_conversation(conversation_id).await?;
    
    // 2. 修复断裂的工具调用（崩溃场景）
    let messages = repair_dangling_tool_calls(messages);
    
    // 3. 加载 checkpoint 元数据
    let meta: CheckpointMeta = sqlx::query_scalar(
        "SELECT checkpoint_meta FROM conversations WHERE id = ?"
    )
    .bind(conversation_id)
    .fetch_one(pool).await
    .and_then(|s: String| serde_json::from_str(&s).ok())
    .unwrap_or_default();
    
    // 4. 注入恢复提示
    let resume_prompt = format!(
        "[系统提示] 上一次执行因应用关闭而中断（已完成 {} 轮）。\
         以上是所有已完成的步骤和结果。请评估当前进度，继续执行未完成的任务。\
         如果任务已基本完成，请给出最终总结。",
        meta.round
    );
    
    // 5. 构建 TurnContext 并恢复 round_loop
    let mut turn_context = TurnContext {
        conversation_id: conversation_id.to_string(),
        model: meta.model,
        messages,
        round: meta.round,  // 从断点轮次继续计数
        ..TurnContext::default()
    };
    turn_context.inject_system_message(&resume_prompt);
    
    // 6. 更新状态为 running
    sqlx::query("UPDATE conversations SET agent_state = 'running' WHERE id = ?")
        .bind(conversation_id)
        .execute(pool).await?;
    
    // 7. 进入 round_loop
    let mut executor = AgentExecutor::new(session, turn_context, emitter);
    executor.round_loop().await
}
```

---

## 五、子 Agent Checkpoint

### 5.1 子 Agent 进度持久化

子 Agent 的执行进度通过 `task_traces` 表持久化：

```sql
-- task_traces 已有字段足以支撑子 Agent 恢复
-- id, conversation_id, parent_task_id, depth, status, input_json, output_json, started_at, ended_at
```

每个子 Agent 完成时 `output_json` 写入 `SubAgentResult`。崩溃时 `status = 'running'` 的子 Agent 被标记为 `failed`，其已完成的部分结果注入父 Agent 上下文。

### 5.2 恢复策略

子 Agent 不支持断点续跑（上下文独立、短生命周期），崩溃后的处理：

1. 已完成的子 Agent：`output_json` 已持久化，直接复用
2. 未完成的子 Agent：标记为 `failed`，注入父 Agent 上下文："子任务 '{goal}' 因中断未完成，请决定是否重新委派"
3. 父 Agent 收到恢复提示后，LLM 自行决定是否重新 `delegate_task`

---

## 六、Checkpoint 与 Prompt Cache 的兼容

Checkpoint 恢复时完整加载 messages 历史，system prompt 的 Slot 1-4（冻结前缀）在会话内不变。这意味着 Anthropic Prompt Cache 在恢复后仍然有效——恢复的请求与中断前的请求共享相同的 system prompt 前缀。

---

## 七、数据安全

### 7.1 Checkpoint 不存储敏感数据

`checkpoint_meta` 只存储结构化元数据（round 计数、model 名、cost 数值），不存储消息内容或工具结果。完整内容已在 `messages` 表中。

### 7.2 "放弃"操作

用户选择"放弃"未完成任务时：
- `agent_state` 设为 `'failed'`
- `checkpoint_meta` 清空
- 消息历史保留（用户可以查看但不继续）

### 7.3 自动过期

超过 7 天的 `interrupted` 状态会话自动降级为 `failed`（不再提示恢复）：

```sql
UPDATE conversations 
SET agent_state = 'failed', checkpoint_meta = '{}'
WHERE agent_state = 'interrupted' 
  AND last_checkpoint_at < (unixepoch('now') - 7*86400) * 1000;
```

---

## 八、与现有设计的关系

| 现有机制 | Checkpoint 的增强 |
| ---- | ---- |
| `repair_dangling_tool_calls` | 保留，作为崩溃恢复的第一步（修复断裂的工具调用） |
| `PendingQueue`（内存） | 不恢复（用户重新输入），UI 提示 |
| `YOLO` 开关 | 不恢复（安全原则，重置为 OFF） |
| `TokenBudget` | 从 `checkpoint_meta.token_used` 恢复 |
| 子 Agent 进度 | 通过 `task_traces` 表恢复已完成的结果 |
| `AgentState` 状态机 | 新增 `Interrupted` 状态（区别于 `Paused` 和 `Failed`） |

---

## 相关文档

- `04-详细设计阶段/01-核心引擎层/02-agent-runtime详细设计.md` § 15 — 崩溃恢复策略
- `04-详细设计阶段/06-安全与基础设施/02-人工接管与授权详细设计.md` § 11 — 三种恢复场景
- `03-系统设计阶段/04-数据库设计/01-Schema设计.md` — conversations 表、task_traces 表
