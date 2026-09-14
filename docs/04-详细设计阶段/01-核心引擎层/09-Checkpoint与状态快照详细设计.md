# Checkpoint 与状态快照详细设计

> **当前基线（2026-09-14）**：当前恢复使用 `agent-rollout`（`crates/agent-rollout`）append-only JSONL 事实源（`RolloutRecorder`、`PersistencePolicy`、`reconstruct`）、稳定 item identity 和 snapshot + live boundary。SessionStore（`crates/agent-session`，`state.db` WAL SQLite，schema v24，FTS5）是可销毁重建的原生 `ResponseItem` 索引，同时保存线程附件。恢复协议：客户端先建立 `SubscribeThreadEvents` live stream → `ResumeThread(include_turns=true)` → Server 在 listener 内串行化 snapshot + 订阅 → 越过 live boundary 处理缓冲事件。`ThreadRollback` 是累计、可回放的 durable 控制事件。本文其余 checkpoint 表、快照覆盖和工作流断点方案若无当前源码对应，属于目标设计。

> 版本：v1.0 | 日期：2026-08-11 | 状态：草稿（大部分内容为目标设计）
> 对应需求：F-12 对话分支、F-01 Agent 核心执行（崩溃恢复/回滚）、F-22 工作流暂停恢复
> 前置文档：[07-Agent生命周期详细设计.md](07-Agent生命周期详细设计.md)（会话持久化与崩溃恢复）、[03-对话分支系统设计.md](../05-桌面端与交互/03-对话分支系统设计.md)（分支数据模型）、[05-工作流DAG执行引擎设计.md](../_v0.3规划/05-工作流DAG执行引擎设计.md)（workflow_runs 暂停恢复）、[01-数据库访问层详细设计.md](../06-安全与基础设施/01-数据库访问层详细设计.md)（Repository 模式、事务管理）

---

## 1. Checkpoint 架构概述

### 1.1 什么是 Checkpoint

Checkpoint 是某一时刻对话与 Agent 状态的冻结快照。它捕获该时刻的完整上下文——消息历史、系统提示、记忆快照、工具执行状态——使得用户或系统可以在任意未来时刻精确恢复到该状态，如同版本控制系统中的 commit。

与 [07-Agent生命周期详细设计.md](07-Agent生命周期详细设计.md) Section 11（事件与持久化）的崩溃恢复机制不同，Checkpoint 不仅仅用于故障恢复：它是一个主动的、可寻址的状态标记，支持用户驱动的回滚、状态复现和分支探索。

### 1.2 核心用例

| 用例 | 场景 | 触发方式 |
|------|------|---------|
| **回滚** | Agent 给出不满意的回复或执行了错误操作，用户希望回到之前的状态重新来过 | 手动选择 Checkpoint 恢复 |
| **工作流暂停恢复** | 长时间 DAG 工作流跨越应用重启，需从上次中断的节点继续 | 自动 Checkpoint（每个 DAG 节点边界） |
| **状态复现** | 调试场景：复现某次 Agent 行为需要精确还原当时的上下文（消息、记忆、SystemPrompt） | 手动 Checkpoint + 恢复 |
| **分支探索** | 从历史某个时间点创建新的对话分支，保留原对话不受影响（fork-on-restore） | Checkpoint → 创建分支 |
| **高风险操作保护** | 执行 `file_write`、`shell_exec` 等高风险工具前自动创建快照，失败后可一键回滚 | 自动 Checkpoint（风险触发） |

### 1.3 与现有机制的关系

```text
                  ┌──────────────────────────────────────────────────┐
                  │              Checkpoint 系统（本文档）             │
                  │                                                  │
                  │  冻结快照：messages + context + memory + tool     │
                  │  ► 支持精确恢复到任意历史时刻                      │
                  │  ► 支持 fork-on-restore（创建分支）               │
                  └──────────┬───────────────┬──────────────────┬────┘
                             │               │                  │
                    ┌────────▼─────┐  ┌──────▼────────┐  ┌─────▼──────────┐
                    │ 崩溃恢复     │  │ 对话分支       │  │ 工作流暂停恢复  │
                    │ (lifecycle   │  │ (branching     │  │ (workflow_runs  │
                    │  §11)        │  │  系统)         │  │  §7)           │
                    └──────────────┘  └───────────────┘  └────────────────┘
                    rollout snapshot     消息级分叉，        ExecutionContext
                    + live boundary     复制历史消息        序列化快照
                    ResponseItem 恢复
```

**崩溃恢复**（Agent 生命周期 Section 11 事件与持久化）处理的是应用意外退出时的"尽力而为"恢复，以 rollout JSONL 为权威历史、SessionStore（`response_items` 表）为可重建投影；**Checkpoint** 是主动创建的精确快照，包含完整的上下文环境（记忆、SystemPrompt 状态等），恢复精度更高。两者互补而非替代。

**对话分支**（branching 系统）通过复制消息历史创建新对话线；Checkpoint 的 fork-on-restore 模式在分支创建时额外注入完整的记忆和上下文快照，确保分支后的 Agent 状态与原始时刻完全一致。

---

## 2. Checkpoint 数据模型

### 2.1 DDL

```sql
-- migrations/0010_checkpoints.sql

-- 主 Checkpoint 元数据表
CREATE TABLE IF NOT EXISTS checkpoints (
    id              TEXT PRIMARY KEY,                       -- UUID
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    turn_index      INTEGER NOT NULL,                       -- 该 Checkpoint 对应的对话轮次序号
    label           TEXT,                                   -- 用户自定义标签（手动 Checkpoint 时填写）
    description     TEXT,                                   -- 用户自定义描述
    checkpoint_type TEXT NOT NULL DEFAULT 'auto'            -- auto / manual / workflow / pre_risk
                    CHECK(checkpoint_type IN ('auto','manual','workflow','pre_risk')),
    message_count   INTEGER NOT NULL DEFAULT 0,             -- 快照中的消息总数
    metadata_json   TEXT NOT NULL DEFAULT '{}',             -- 扩展元数据 JSON（model、token_usage 等）
    size_bytes      INTEGER NOT NULL DEFAULT 0,             -- 所有 checkpoint_data 的总字节数
    created_at      INTEGER NOT NULL DEFAULT (CAST(unixepoch('now','subsec') * 1000 AS INTEGER))
);

CREATE INDEX IF NOT EXISTS idx_checkpoints_conv
    ON checkpoints(conversation_id, turn_index DESC);

CREATE INDEX IF NOT EXISTS idx_checkpoints_workspace
    ON checkpoints(workspace_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_checkpoints_type
    ON checkpoints(conversation_id, checkpoint_type)
    WHERE checkpoint_type = 'manual';

-- Checkpoint 数据存储表（按 data_type 分片存储）
CREATE TABLE IF NOT EXISTS checkpoint_data (
    id              TEXT PRIMARY KEY,                       -- UUID
    checkpoint_id   TEXT NOT NULL REFERENCES checkpoints(id) ON DELETE CASCADE,
    data_type       TEXT NOT NULL                           -- messages / system_prompt / memory_snapshot
                    CHECK(data_type IN (                    -- / tool_state / pending_queue / skill_state
                        'messages', 'system_prompt', 'memory_snapshot',
                        'tool_state', 'pending_queue', 'skill_state'
                    )),
    data            BLOB NOT NULL,                          -- zstd 压缩后的 JSON 二进制
    uncompressed_size INTEGER NOT NULL DEFAULT 0,           -- 压缩前字节数（用于预估恢复开销）
    created_at      INTEGER NOT NULL DEFAULT (CAST(unixepoch('now','subsec') * 1000 AS INTEGER))
);

CREATE INDEX IF NOT EXISTS idx_checkpoint_data_parent
    ON checkpoint_data(checkpoint_id, data_type);

-- 工作流 Checkpoint 关联表（将 Checkpoint 与 DAG 节点绑定）
CREATE TABLE IF NOT EXISTS workflow_checkpoints (
    id              TEXT PRIMARY KEY,                       -- UUID
    checkpoint_id   TEXT NOT NULL REFERENCES checkpoints(id) ON DELETE CASCADE,
    workflow_run_id TEXT NOT NULL,                           -- 关联 workflow_runs.id
    node_id         TEXT NOT NULL,                           -- DAG 节点 ID
    node_status     TEXT NOT NULL,                           -- pending / running / done / failed / skipped
    context_json    TEXT,                                    -- ExecutionContext 的增量快照
    created_at      INTEGER NOT NULL DEFAULT (CAST(unixepoch('now','subsec') * 1000 AS INTEGER)),
    UNIQUE(workflow_run_id, node_id)                         -- 每个节点只保留最新 Checkpoint
);

CREATE INDEX IF NOT EXISTS idx_wf_checkpoints_run
    ON workflow_checkpoints(workflow_run_id, node_id);
```

### 2.2 Rust 数据结构

```rust
// crates/agent-core/src/checkpoint/model.rs

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Checkpoint 类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointType {
    /// 系统自动创建（轮次边界、高风险工具执行前）
    Auto,
    /// 用户手动创建（/checkpoint 命令或 UI 操作）
    Manual,
    /// 工作流 DAG 节点边界自动创建
    Workflow,
    /// 高风险工具执行前自动创建
    PreRisk,
}

/// Checkpoint 数据分片类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointDataType {
    /// 完整消息历史（截至该 Checkpoint 的所有消息）
    Messages,
    /// SystemPrompt 完整内容（8 槽位拼接结果）
    SystemPrompt,
    /// 记忆快照（L3 MEMORY.md + L5 USER.md 内容）
    MemorySnapshot,
    /// 工具执行上下文（注册的工具列表 + 白名单状态）
    ToolState,
    /// PendingQueue 待处理消息队列
    PendingQueue,
    /// 活跃 Skill 状态（已加载的 Skill 列表 + 触发索引）
    SkillState,
}

/// Checkpoint 元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub conversation_id: String,
    pub workspace_id: String,
    pub turn_index: u32,
    pub label: Option<String>,
    pub description: Option<String>,
    pub checkpoint_type: CheckpointType,
    pub message_count: u32,
    pub metadata: CheckpointMetadata,
    pub size_bytes: u64,
    pub created_at: DateTime<Utc>,
}

/// 扩展元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointMetadata {
    /// 创建 Checkpoint 时使用的 model
    pub model: String,
    /// 截至该 Checkpoint 的累计 token 用量
    pub token_usage: TokenUsageSummary,
    /// 最后一条消息的角色和摘要
    pub last_message_role: String,
    pub last_message_preview: String,
    /// 若为 pre_risk 类型，记录触发的工具名和风险等级
    pub risk_tool: Option<String>,
    pub risk_level: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageSummary {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// Checkpoint 数据分片
#[derive(Debug, Clone)]
pub struct CheckpointData {
    pub id: String,
    pub checkpoint_id: String,
    pub data_type: CheckpointDataType,
    /// zstd 压缩后的数据
    pub data: Vec<u8>,
    pub uncompressed_size: u64,
}
```

### 2.3 与现有表的关系

```text
workspaces ──< checkpoints ──< checkpoint_data（按 data_type 分片）
           │        │
           │        └──< workflow_checkpoints（工作流节点级 Checkpoint）
           │                    │
           │                    └──> workflow_runs（关联执行上下文）
           │
           └──< conversations ──< checkpoints
                               ──< messages（Checkpoint 捕获的历史数据源）
```

---

## 3. 自动 Checkpoint

### 3.1 触发条件

自动 Checkpoint 由系统在关键时刻自动创建，用户无需手动干预。触发条件通过 `AutoCheckpointPolicy` 配置：

```rust
// crates/agent-core/src/checkpoint/policy.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoCheckpointPolicy {
    /// 是否启用自动 Checkpoint
    pub enabled: bool,
    /// 每 N 轮对话自动创建一个 Checkpoint（默认 5）
    pub turn_interval: u32,
    /// 高风险工具执行前是否自动创建（默认 true）
    pub before_high_risk_tool: bool,
    /// 工作流节点边界是否自动创建（默认 true）
    pub at_workflow_node_boundary: bool,
    /// 最大自动 Checkpoint 保留数（默认 20）
    pub max_auto_checkpoints: usize,
    /// 自动 Checkpoint 最大单个大小限制（字节，默认 10 MiB）
    pub max_checkpoint_size: u64,
}

impl Default for AutoCheckpointPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            turn_interval: 5,
            before_high_risk_tool: true,
            at_workflow_node_boundary: true,
            max_auto_checkpoints: 20,
            max_checkpoint_size: 10 * 1024 * 1024,
        }
    }
}
```

### 3.2 触发点集成

自动 Checkpoint 在 `AgentExecutor::round_loop` 的关键位置插入，与现有执行流程无缝集成：

```rust
// crates/agent-runtime/src/executor.rs（round_loop 内部增强）

impl AgentExecutor {
    async fn round_loop_with_checkpoints(
        &self,
        ctx: &mut AgentContext,
        checkpoint_mgr: &CheckpointManager,
    ) -> Result<RoundOutcome, AgentError> {
        let mut round: u32 = 0;

        loop {
            // ---- 轮次间隔 Checkpoint ----
            if round > 0
                && round % self.policy.turn_interval == 0
                && self.policy.enabled
            {
                checkpoint_mgr.create_auto(
                    &ctx.conversation_id,
                    &ctx.workspace_id,
                    round,
                    ctx,
                ).await.ok(); // 自动 Checkpoint 失败不阻断主流程
            }

            // 1. 调用 LLM
            let response = self.call_llm(ctx).await?;

            // 2. 解析 tool_calls
            if let Some(tool_calls) = &response.tool_calls {
                for tc in tool_calls {
                    // ---- 高风险工具 Checkpoint ----
                    let risk = self.tool_registry.risk_level(&tc.name);
                    if self.policy.before_high_risk_tool
                        && matches!(risk, RiskLevel::High | RiskLevel::Critical)
                    {
                        checkpoint_mgr.create_pre_risk(
                            &ctx.conversation_id,
                            &ctx.workspace_id,
                            round,
                            &tc.name,
                            &risk,
                            ctx,
                        ).await.ok();
                    }

                    // 执行工具调用...
                }
            }

            round += 1;

            if response.stop_reason == StopReason::EndTurn {
                break;
            }
        }

        Ok(RoundOutcome::Done)
    }
}
```

### 3.3 保留与清理策略

自动 Checkpoint 的数量受 `max_auto_checkpoints` 限制。超出时按 FIFO 策略清理最旧的自动 Checkpoint，手动 Checkpoint 不受此限制：

```rust
impl CheckpointManager {
    /// 清理超出保留限制的自动 Checkpoint
    async fn prune_auto_checkpoints(
        &self,
        conversation_id: &str,
    ) -> Result<usize, CheckpointError> {
        let max = self.policy.max_auto_checkpoints as i64;

        // 查找需要删除的自动 Checkpoint（保留最新的 max 个）
        let to_delete = sqlx::query_scalar!(
            r#"SELECT id FROM checkpoints
               WHERE conversation_id = ?
                 AND checkpoint_type IN ('auto', 'pre_risk')
               ORDER BY created_at DESC
               LIMIT -1 OFFSET ?"#,
            conversation_id,
            max,
        )
        .fetch_all(&self.pool)
        .await?;

        if to_delete.is_empty() {
            return Ok(0);
        }

        let count = to_delete.len();

        // 级联删除 checkpoint_data（外键 ON DELETE CASCADE）
        for id in &to_delete {
            sqlx::query!("DELETE FROM checkpoints WHERE id = ?", id)
                .execute(&self.pool)
                .await?;
        }

        tracing::debug!(
            conversation_id,
            pruned = count,
            "自动 Checkpoint 清理完成"
        );

        Ok(count)
    }
}
```

---

## 4. 手动 Checkpoint

### 4.1 创建方式

用户通过以下两种方式创建手动 Checkpoint：

1. **UI 操作**：在对话界面点击"创建快照"按钮，弹出对话框输入标签和描述
2. **斜杠命令**：在对话输入框中输入 `/checkpoint [label] [--desc "description"]`

手动 Checkpoint 具有以下特性：
- 必须提供 `label`（标签名），不可为空
- `description` 可选
- 持久保留，不受自动清理策略影响，直到用户显式删除
- 在 CheckpointTimeline 组件中以醒目标记展示

### 4.2 创建流程

```rust
impl CheckpointManager {
    /// 创建手动 Checkpoint
    pub async fn create_manual(
        &self,
        conversation_id: &str,
        workspace_id: &str,
        label: &str,
        description: Option<&str>,
        ctx: &AgentContext,
    ) -> Result<Checkpoint, CheckpointError> {
        // 校验 label 唯一性（同一对话内不允许重名）
        let exists = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count: i64" FROM checkpoints
               WHERE conversation_id = ? AND label = ?"#,
            conversation_id,
            label,
        )
        .fetch_one(&self.pool)
        .await?;

        if exists > 0 {
            return Err(CheckpointError::DuplicateLabel(label.to_string()));
        }

        // 计算当前轮次序号
        let turn_index = self.count_turns(conversation_id).await?;

        // 捕获完整状态快照
        let checkpoint = self.capture_snapshot(
            conversation_id,
            workspace_id,
            turn_index,
            CheckpointType::Manual,
            Some(label),
            description,
            ctx,
        ).await?;

        tracing::info!(
            checkpoint_id = %checkpoint.id,
            label,
            turn_index,
            "手动 Checkpoint 已创建"
        );

        Ok(checkpoint)
    }
}
```

---

## 5. Checkpoint 内容

### 5.1 捕获的状态

Checkpoint 按 `CheckpointDataType` 分片捕获以下状态，每种类型独立压缩存储：

| 数据类型 | 内容 | 来源 |
|---------|------|------|
| `Messages` | 截至该 Checkpoint 时刻的完整消息历史（user/assistant/tool/system） | `messages` 表 |
| `SystemPrompt` | 8 槽位 SystemPrompt 的拼接结果 | `AgentSession.system_prompt` |
| `MemorySnapshot` | L3 MEMORY.md 和 L5 USER.md 的内容快照 | 文件系统 + `memory_entries` 活跃版本 |
| `ToolState` | 已注册工具列表 + HumanGuard 白名单状态 + YOLO 开关 | `ToolRegistry` + `HumanGuard` |
| `PendingQueue` | 待处理的用户消息队列 | `PendingQueue` 内存状态 |
| `SkillState` | 已加载的 Skill 列表、触发索引快照 | `SkillRegistry` |

### 5.2 不捕获的状态

| 数据 | 原因 |
|------|------|
| 进行中的 LLM 流式响应 | 流式响应是瞬态数据，未完成前不具备一致性 |
| Provider 连接状态 | 网络连接不可序列化，恢复时重新建立 |
| CancellationToken 状态 | 运行时信号，恢复时创建新的 token |
| Subagent 的活跃 tokio task / cancellation handle | 运行时资源不进入本 Checkpoint；Agent Graph、mailbox、状态事件和 runtime descriptor 由 `subagents-v2.db` 独立持久化 |
| Subagent Session 时间线 | 不重复写入本 Checkpoint；真实 `ResponseItem` 由 `state.db` 持久化，并通过 `AgentThreadV2.session_id` 关联 |

### 5.3 状态捕获实现

```rust
// crates/agent-core/src/checkpoint/capture.rs

use zstd;

impl CheckpointManager {
    /// 捕获完整状态快照
    async fn capture_snapshot(
        &self,
        conversation_id: &str,
        workspace_id: &str,
        turn_index: u32,
        checkpoint_type: CheckpointType,
        label: Option<&str>,
        description: Option<&str>,
        ctx: &AgentContext,
    ) -> Result<Checkpoint, CheckpointError> {
        let checkpoint_id = uuid();
        let mut total_size: u64 = 0;

        // 开启事务保证原子性
        let mut tx = self.pool.begin().await?;

        // [1] 捕获消息历史
        let messages = sqlx::query!(
            "SELECT * FROM messages WHERE conversation_id = ? ORDER BY created_at ASC",
            conversation_id,
        )
        .fetch_all(&mut *tx)
        .await?;

        let messages_json = serde_json::to_vec(&messages)?;
        let messages_compressed = zstd::encode_all(&messages_json[..], 3)?;
        total_size += self.store_data_in_tx(
            &mut tx,
            &checkpoint_id,
            CheckpointDataType::Messages,
            &messages_compressed,
            messages_json.len() as u64,
        ).await?;

        // [2] 捕获 SystemPrompt
        let system_prompt = ctx.system_prompt_snapshot();
        let prompt_json = serde_json::to_vec(&system_prompt)?;
        let prompt_compressed = zstd::encode_all(&prompt_json[..], 3)?;
        total_size += self.store_data_in_tx(
            &mut tx,
            &checkpoint_id,
            CheckpointDataType::SystemPrompt,
            &prompt_compressed,
            prompt_json.len() as u64,
        ).await?;

        // [3] 捕获记忆快照
        let memory = self.capture_memory_snapshot(workspace_id).await?;
        let memory_json = serde_json::to_vec(&memory)?;
        let memory_compressed = zstd::encode_all(&memory_json[..], 3)?;
        total_size += self.store_data_in_tx(
            &mut tx,
            &checkpoint_id,
            CheckpointDataType::MemorySnapshot,
            &memory_compressed,
            memory_json.len() as u64,
        ).await?;

        // [4] 捕获工具状态
        let tool_state = ctx.tool_state_snapshot();
        let tool_json = serde_json::to_vec(&tool_state)?;
        let tool_compressed = zstd::encode_all(&tool_json[..], 3)?;
        total_size += self.store_data_in_tx(
            &mut tx,
            &checkpoint_id,
            CheckpointDataType::ToolState,
            &tool_compressed,
            tool_json.len() as u64,
        ).await?;

        // [5] 捕获 PendingQueue
        let pending = ctx.pending_queue_snapshot();
        if !pending.is_empty() {
            let pending_json = serde_json::to_vec(&pending)?;
            let pending_compressed = zstd::encode_all(&pending_json[..], 3)?;
            total_size += self.store_data_in_tx(
                &mut tx,
                &checkpoint_id,
                CheckpointDataType::PendingQueue,
                &pending_compressed,
                pending_json.len() as u64,
            ).await?;
        }

        // [6] 捕获 Skill 状态
        let skill_state = ctx.skill_state_snapshot();
        let skill_json = serde_json::to_vec(&skill_state)?;
        let skill_compressed = zstd::encode_all(&skill_json[..], 3)?;
        total_size += self.store_data_in_tx(
            &mut tx,
            &checkpoint_id,
            CheckpointDataType::SkillState,
            &skill_compressed,
            skill_json.len() as u64,
        ).await?;

        // 构建元数据
        let last_msg = messages.last();
        let metadata = CheckpointMetadata {
            model: ctx.model().to_string(),
            token_usage: ctx.token_usage_summary(),
            last_message_role: last_msg.map_or("none".into(), |m| m.role.clone()),
            last_message_preview: last_msg
                .map_or(String::new(), |m| truncate(&m.content, 100)),
            risk_tool: None,
            risk_level: None,
        };

        let metadata_json = serde_json::to_string(&metadata)?;

        // 写入 Checkpoint 主记录
        let now_ms = now_ms();
        sqlx::query!(
            r#"INSERT INTO checkpoints
               (id, conversation_id, workspace_id, turn_index, label,
                description, checkpoint_type, message_count, metadata_json,
                size_bytes, created_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
            checkpoint_id,
            conversation_id,
            workspace_id,
            turn_index,
            label,
            description,
            checkpoint_type.as_str(),
            messages.len() as i64,
            metadata_json,
            total_size as i64,
            now_ms,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(Checkpoint {
            id: checkpoint_id,
            conversation_id: conversation_id.to_string(),
            workspace_id: workspace_id.to_string(),
            turn_index,
            label: label.map(String::from),
            description: description.map(String::from),
            checkpoint_type,
            message_count: messages.len() as u32,
            metadata,
            size_bytes: total_size,
            created_at: Utc::now(),
        })
    }

    /// 存储单个数据分片到事务中
    async fn store_data_in_tx(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        checkpoint_id: &str,
        data_type: CheckpointDataType,
        compressed: &[u8],
        uncompressed_size: u64,
    ) -> Result<u64, CheckpointError> {
        let id = uuid();
        let data_type_str = data_type.as_str();
        sqlx::query!(
            r#"INSERT INTO checkpoint_data
               (id, checkpoint_id, data_type, data, uncompressed_size)
               VALUES (?, ?, ?, ?, ?)"#,
            id,
            checkpoint_id,
            data_type_str,
            compressed,
            uncompressed_size as i64,
        )
        .execute(&mut **tx)
        .await?;

        Ok(compressed.len() as u64)
    }
}
```

### 5.4 记忆快照捕获

```rust
/// 记忆快照结构
#[derive(Debug, Serialize, Deserialize)]
pub struct MemorySnapshotData {
    /// L3 MEMORY.md 文件内容
    pub memory_md: String,
    /// L5 USER.md 文件内容
    pub user_md: String,
    /// 活跃 memory_entries 列表（valid_to IS NULL）
    pub active_entries: Vec<MemoryEntrySnapshot>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MemoryEntrySnapshot {
    pub id: String,
    pub category: String,
    pub key: String,
    pub value: String,
    pub importance: f64,
    pub retrieval_count: i64,
}

impl CheckpointManager {
    async fn capture_memory_snapshot(
        &self,
        workspace_id: &str,
    ) -> Result<MemorySnapshotData, CheckpointError> {
        // 读取 MEMORY.md
        let memory_path = format!(
            "{}/.astro/workspaces/{}/MEMORY.md",
            dirs::home_dir().unwrap().display(),
            workspace_id
        );
        let memory_md = tokio::fs::read_to_string(&memory_path)
            .await
            .unwrap_or_default();

        // 读取 USER.md
        let user_path = format!(
            "{}/.astro/USER.md",
            dirs::home_dir().unwrap().display()
        );
        let user_md = tokio::fs::read_to_string(&user_path)
            .await
            .unwrap_or_default();

        // 查询活跃记忆条目
        let entries = sqlx::query_as!(
            MemoryEntrySnapshot,
            r#"SELECT id, category, key, value, importance as "importance: f64",
                      retrieval_count as "retrieval_count: i64"
               FROM memory_entries
               WHERE workspace_id = ? AND valid_to IS NULL
               ORDER BY category, key"#,
            workspace_id,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(MemorySnapshotData {
            memory_md,
            user_md,
            active_entries: entries,
        })
    }
}
```

---

## 6. 状态恢复（Restore）

### 6.1 恢复流程

Checkpoint 恢复支持两种模式：

| 模式 | 行为 | 适用场景 |
|------|------|---------|
| **覆写恢复** | 在当前对话中回退到 Checkpoint 状态，后续消息被删除 | 简单回滚，不需要保留后续历史 |
| **分支恢复（fork-on-restore）** | 从 Checkpoint 创建新对话分支，原对话保持不变 | 探索不同路径，保留完整历史 |

```rust
// crates/agent-core/src/checkpoint/restore.rs

/// 恢复选项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreOptions {
    /// 恢复模式
    pub mode: RestoreMode,
    /// 是否恢复记忆状态（默认 true）
    pub restore_memory: bool,
    /// 是否恢复工具状态（默认 true）
    pub restore_tool_state: bool,
    /// 是否恢复 PendingQueue（默认 false，通常不需要）
    pub restore_pending_queue: bool,
    /// 分支模式下的新对话标题（可选）
    pub branch_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RestoreMode {
    /// 覆写当前对话
    Overwrite,
    /// 创建新分支
    Fork,
}

/// 恢复结果
#[derive(Debug, Serialize)]
pub struct RestoreResult {
    /// 恢复目标的 conversation_id（覆写模式为原 ID，分支模式为新 ID）
    pub conversation_id: String,
    /// 恢复的消息数
    pub restored_message_count: u32,
    /// 是否恢复了记忆
    pub memory_restored: bool,
    /// 是否创建了新分支
    pub forked: bool,
}
```

### 6.2 覆写恢复实现

```rust
impl CheckpointManager {
    /// 恢复 Checkpoint（覆写模式）
    async fn restore_overwrite(
        &self,
        checkpoint: &Checkpoint,
        options: &RestoreOptions,
        instance: &mut AgentInstance,
    ) -> Result<RestoreResult, CheckpointError> {
        // Agent 必须处于 Ready 状态
        if !matches!(instance.status, AgentLifecycle::Ready) {
            return Err(CheckpointError::AgentNotReady);
        }

        let mut tx = self.pool.begin().await?;

        // [1] 删除 Checkpoint 之后的消息
        sqlx::query!(
            r#"DELETE FROM messages
               WHERE conversation_id = ?
                 AND created_at > (
                     SELECT MAX(created_at) FROM messages
                     WHERE conversation_id = ?
                     ORDER BY created_at ASC
                     LIMIT 1 OFFSET ? - 1
                 )"#,
            checkpoint.conversation_id,
            checkpoint.conversation_id,
            checkpoint.message_count as i64,
        )
        .execute(&mut *tx)
        .await?;

        // [2] 加载并恢复 SystemPrompt
        let prompt_data = self.load_data(
            &mut tx,
            &checkpoint.id,
            CheckpointDataType::SystemPrompt,
        ).await?;
        if let Some(data) = prompt_data {
            let prompt: String = decompress_and_parse(&data)?;
            instance.session.set_system_prompt(prompt);
        }

        // [3] 恢复记忆状态（可选）
        if options.restore_memory {
            self.restore_memory_state(&mut tx, &checkpoint.id, &checkpoint.workspace_id)
                .await?;
        }

        // [4] 恢复工具状态（可选）
        if options.restore_tool_state {
            let tool_data = self.load_data(
                &mut tx,
                &checkpoint.id,
                CheckpointDataType::ToolState,
            ).await?;
            if let Some(data) = tool_data {
                let state: ToolStateSnapshot = decompress_and_parse(&data)?;
                instance.session.restore_tool_state(state);
            }
        }

        // [5] 恢复 PendingQueue（可选）
        if options.restore_pending_queue {
            let pending_data = self.load_data(
                &mut tx,
                &checkpoint.id,
                CheckpointDataType::PendingQueue,
            ).await?;
            if let Some(data) = pending_data {
                let queue: Vec<PendingMessage> = decompress_and_parse(&data)?;
                instance.executor.as_mut()
                    .ok_or(CheckpointError::ExecutorNotReady)?
                    .restore_pending_queue(queue);
            }
        }

        // [6] 清理 Checkpoint 之后创建的自动 Checkpoint
        sqlx::query!(
            r#"DELETE FROM checkpoints
               WHERE conversation_id = ?
                 AND created_at > ?
                 AND checkpoint_type IN ('auto', 'pre_risk')"#,
            checkpoint.conversation_id,
            checkpoint.created_at.timestamp_millis(),
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        // [7] 转入 Ready 状态（确保 AgentInstance 就绪）
        tracing::info!(
            checkpoint_id = %checkpoint.id,
            conversation_id = %checkpoint.conversation_id,
            turn_index = checkpoint.turn_index,
            "Checkpoint 覆写恢复完成"
        );

        Ok(RestoreResult {
            conversation_id: checkpoint.conversation_id.clone(),
            restored_message_count: checkpoint.message_count,
            memory_restored: options.restore_memory,
            forked: false,
        })
    }
}
```

### 6.3 分支恢复实现（fork-on-restore）

分支恢复复用对话分支系统的 `create_branch` 能力，但额外注入 Checkpoint 中捕获的记忆和上下文快照：

```rust
impl CheckpointManager {
    /// 恢复 Checkpoint（分支模式）
    async fn restore_fork(
        &self,
        checkpoint: &Checkpoint,
        options: &RestoreOptions,
        instance: &mut AgentInstance,
    ) -> Result<RestoreResult, CheckpointError> {
        let mut tx = self.pool.begin().await?;

        // [1] 找到 Checkpoint 时刻的最后一条消息 ID
        let branch_point_msg = sqlx::query_scalar!(
            r#"SELECT id FROM messages
               WHERE conversation_id = ?
               ORDER BY created_at ASC
               LIMIT 1 OFFSET ? - 1"#,
            checkpoint.conversation_id,
            checkpoint.message_count as i64,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(CheckpointError::CorruptedData("无法定位分支点消息".into()))?;

        // [2] 创建新的对话分支（复用 branching 系统逻辑）
        let branch_title = options.branch_title.as_deref()
            .unwrap_or("从快照恢复的分支");
        let new_conv_id = uuid();

        sqlx::query!(
            r#"INSERT INTO conversations
               (id, workspace_id, parent_conversation_id,
                branch_point_message_id, title, model, created_at, updated_at)
               SELECT ?, workspace_id, ?, ?, ?, model,
                      CAST(unixepoch('now','subsec') * 1000 AS INTEGER),
                      CAST(unixepoch('now','subsec') * 1000 AS INTEGER)
               FROM conversations WHERE id = ?"#,
            new_conv_id,
            checkpoint.conversation_id,
            branch_point_msg,
            branch_title,
            checkpoint.conversation_id,
        )
        .execute(&mut *tx)
        .await?;

        // [3] 从 Checkpoint 数据恢复消息（而非从 messages 表复制）
        let msg_data = self.load_data(
            &mut tx,
            &checkpoint.id,
            CheckpointDataType::Messages,
        ).await?;

        if let Some(data) = msg_data {
            let messages: Vec<StoredMessage> = decompress_and_parse(&data)?;
            for msg in &messages {
                let new_msg_id = uuid();
                sqlx::query!(
                    r#"INSERT INTO messages
                       (id, conversation_id, role, content, reasoning_content,
                        tool_call_id, tool_name, token_count, latency_ms,
                        is_pinned, created_at)
                       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
                    new_msg_id,
                    new_conv_id,
                    msg.role,
                    msg.content,
                    msg.reasoning_content,
                    msg.tool_call_id,
                    msg.tool_name,
                    msg.token_count,
                    msg.latency_ms,
                    msg.is_pinned,
                    msg.created_at,
                )
                .execute(&mut *tx)
                .await?;
            }
        }

        // [4] 恢复记忆状态到新分支上下文（可选）
        if options.restore_memory {
            self.restore_memory_state(&mut tx, &checkpoint.id, &checkpoint.workspace_id)
                .await?;
        }

        tx.commit().await?;

        tracing::info!(
            checkpoint_id = %checkpoint.id,
            new_conversation_id = %new_conv_id,
            "Checkpoint 分支恢复完成"
        );

        Ok(RestoreResult {
            conversation_id: new_conv_id,
            restored_message_count: checkpoint.message_count,
            memory_restored: options.restore_memory,
            forked: true,
        })
    }

    /// 恢复记忆状态
    async fn restore_memory_state(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        checkpoint_id: &str,
        workspace_id: &str,
    ) -> Result<(), CheckpointError> {
        let mem_data = self.load_data_in_tx(
            tx,
            checkpoint_id,
            CheckpointDataType::MemorySnapshot,
        ).await?;

        if let Some(data) = mem_data {
            let snapshot: MemorySnapshotData = decompress_and_parse(&data)?;

            // 恢复 MEMORY.md 文件
            let memory_path = format!(
                "{}/.astro/workspaces/{}/MEMORY.md",
                dirs::home_dir().unwrap().display(),
                workspace_id
            );
            tokio::fs::write(&memory_path, &snapshot.memory_md).await?;

            // USER.md 不做覆写（全局资源，恢复可能影响其他工作区）
            // 仅记录日志提示用户差异
            tracing::info!("记忆快照恢复完成，USER.md 未覆写（全局资源保护）");
        }

        Ok(())
    }
}

/// 解压缩并反序列化
fn decompress_and_parse<T: serde::de::DeserializeOwned>(
    compressed: &[u8],
) -> Result<T, CheckpointError> {
    let decompressed = zstd::decode_all(compressed)?;
    let parsed = serde_json::from_slice(&decompressed)?;
    Ok(parsed)
}
```

### 6.4 恢复入口

```rust
impl CheckpointManager {
    /// 统一恢复入口
    pub async fn restore(
        &self,
        checkpoint_id: &str,
        options: RestoreOptions,
        instance: &mut AgentInstance,
    ) -> Result<RestoreResult, CheckpointError> {
        let checkpoint = self.get_checkpoint(checkpoint_id).await?
            .ok_or(CheckpointError::NotFound(checkpoint_id.to_string()))?;

        match options.mode {
            RestoreMode::Overwrite => {
                self.restore_overwrite(&checkpoint, &options, instance).await
            }
            RestoreMode::Fork => {
                self.restore_fork(&checkpoint, &options, instance).await
            }
        }
    }
}
```

---

## 7. 工作流 Checkpoint

### 7.1 概述

工作流 DAG 执行引擎（见 [05-工作流DAG执行引擎设计.md](../_v0.3规划/05-工作流DAG执行引擎设计.md)）中，每个节点边界自动创建 Checkpoint。这使得长时间工作流可以跨应用重启恢复，且失败时可以从最后成功的节点继续。

工作流 Checkpoint 在通用 Checkpoint 基础上，通过 `workflow_checkpoints` 关联表追加了 DAG 节点级的元数据。

### 7.2 节点边界 Checkpoint

```rust
// crates/agent-runtime/src/workflow/executor.rs（增强）

impl WorkflowExecutor {
    async fn execute_node_with_checkpoint(
        &self,
        node: &WorkflowNode,
        ctx: &ExecutionContext,
        checkpoint_mgr: &CheckpointManager,
        run_id: &str,
    ) -> anyhow::Result<Value> {
        // 节点执行前创建 Checkpoint
        let checkpoint = checkpoint_mgr.create_workflow_checkpoint(
            run_id,
            &node.id,
            "pending",
            ctx,
        ).await?;

        // 执行节点
        let result = self.execute_node(node, &ctx.snapshot()).await;

        // 更新 Checkpoint 中的节点状态
        match &result {
            Ok(output) => {
                checkpoint_mgr.update_workflow_checkpoint(
                    &checkpoint.id,
                    &node.id,
                    "done",
                    Some(output),
                ).await?;
            }
            Err(e) => {
                checkpoint_mgr.update_workflow_checkpoint(
                    &checkpoint.id,
                    &node.id,
                    "failed",
                    None,
                ).await?;
            }
        }

        result
    }
}
```

### 7.3 工作流 Checkpoint 管理

```rust
impl CheckpointManager {
    /// 创建工作流节点级 Checkpoint
    pub async fn create_workflow_checkpoint(
        &self,
        workflow_run_id: &str,
        node_id: &str,
        node_status: &str,
        exec_ctx: &ExecutionContext,
    ) -> Result<Checkpoint, CheckpointError> {
        // 查找关联的 conversation_id 和 workspace_id
        let run = sqlx::query!(
            "SELECT workspace_id FROM workflow_runs WHERE id = ?",
            workflow_run_id,
        )
        .fetch_one(&self.pool)
        .await?;

        let checkpoint_id = uuid();
        let turn_index = 0; // 工作流 Checkpoint 不基于对话轮次

        // 序列化 ExecutionContext 增量快照
        let context_json = serde_json::to_string(&exec_ctx.incremental_snapshot())?;

        let mut tx = self.pool.begin().await?;

        // 写入 Checkpoint 主记录
        let metadata_json = serde_json::to_string(&serde_json::json!({
            "workflow_run_id": workflow_run_id,
            "node_id": node_id,
        }))?;

        sqlx::query!(
            r#"INSERT INTO checkpoints
               (id, conversation_id, workspace_id, turn_index,
                checkpoint_type, metadata_json, created_at)
               VALUES (?, '', ?, ?, 'workflow', ?, ?)"#,
            checkpoint_id,
            run.workspace_id,
            turn_index,
            metadata_json,
            now_ms(),
        )
        .execute(&mut *tx)
        .await?;

        // 写入工作流 Checkpoint 关联记录
        let wf_cp_id = uuid();
        sqlx::query!(
            r#"INSERT INTO workflow_checkpoints
               (id, checkpoint_id, workflow_run_id, node_id,
                node_status, context_json, created_at)
               VALUES (?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT(workflow_run_id, node_id) DO UPDATE SET
                   checkpoint_id = excluded.checkpoint_id,
                   node_status = excluded.node_status,
                   context_json = excluded.context_json,
                   created_at = excluded.created_at"#,
            wf_cp_id,
            checkpoint_id,
            workflow_run_id,
            node_id,
            node_status,
            context_json,
            now_ms(),
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(Checkpoint {
            id: checkpoint_id,
            conversation_id: String::new(),
            workspace_id: run.workspace_id,
            turn_index: 0,
            label: None,
            description: None,
            checkpoint_type: CheckpointType::Workflow,
            message_count: 0,
            metadata: CheckpointMetadata::default(),
            size_bytes: context_json.len() as u64,
            created_at: Utc::now(),
        })
    }

    /// 从工作流 Checkpoint 恢复 DAG 执行
    pub async fn resume_workflow_from_checkpoint(
        &self,
        workflow_run_id: &str,
    ) -> Result<WorkflowResumeState, CheckpointError> {
        // 查找所有已完成的节点 Checkpoint
        let completed_nodes = sqlx::query!(
            r#"SELECT node_id, node_status, context_json
               FROM workflow_checkpoints
               WHERE workflow_run_id = ?
                 AND node_status = 'done'
               ORDER BY created_at ASC"#,
            workflow_run_id,
        )
        .fetch_all(&self.pool)
        .await?;

        // 构建恢复状态：已完成节点集合 + 最后的 ExecutionContext
        let last_ctx = completed_nodes.last()
            .and_then(|n| n.context_json.as_deref())
            .and_then(|s| serde_json::from_str::<ExecutionContextSnapshot>(s).ok());

        Ok(WorkflowResumeState {
            completed_node_ids: completed_nodes.iter()
                .map(|n| n.node_id.clone())
                .collect(),
            last_context: last_ctx,
        })
    }
}

#[derive(Debug)]
pub struct WorkflowResumeState {
    pub completed_node_ids: Vec<String>,
    pub last_context: Option<ExecutionContextSnapshot>,
}
```

---

## 8. Checkpoint 存储优化

### 8.1 增量快照

连续的自动 Checkpoint 之间通常只有少量消息差异。增量快照仅存储与前一个 Checkpoint 的差量（delta），大幅减少存储开销：

```rust
// crates/agent-core/src/checkpoint/incremental.rs

#[derive(Debug, Serialize, Deserialize)]
pub struct IncrementalSnapshot {
    /// 基准 Checkpoint ID（NULL 表示完整快照）
    pub base_checkpoint_id: Option<String>,
    /// 新增的消息（仅 delta）
    pub new_messages: Vec<StoredMessage>,
    /// 记忆变更列表（新增/修改/软删除）
    pub memory_changes: Vec<MemoryChange>,
    /// SystemPrompt 是否有变化（通常不变）
    pub prompt_changed: bool,
    /// 若 prompt_changed 为 true，存储新的 SystemPrompt
    pub new_prompt: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum MemoryChange {
    Added(MemoryEntrySnapshot),
    Updated { id: String, new_value: String, new_importance: f64 },
    Expired { id: String },
}

impl CheckpointManager {
    /// 创建增量 Checkpoint（仅存储与上一个 Checkpoint 的差量）
    async fn create_incremental(
        &self,
        conversation_id: &str,
        workspace_id: &str,
        turn_index: u32,
        ctx: &AgentContext,
    ) -> Result<Checkpoint, CheckpointError> {
        // 查找最近的完整 Checkpoint 或增量 Checkpoint
        let base = sqlx::query!(
            r#"SELECT id, message_count, created_at FROM checkpoints
               WHERE conversation_id = ?
               ORDER BY created_at DESC LIMIT 1"#,
            conversation_id,
        )
        .fetch_optional(&self.pool)
        .await?;

        match base {
            Some(base_cp) => {
                // 仅捕获 base_cp 之后新增的消息
                let new_messages = sqlx::query_as!(
                    StoredMessage,
                    r#"SELECT * FROM messages
                       WHERE conversation_id = ?
                         AND created_at > ?
                       ORDER BY created_at ASC"#,
                    conversation_id,
                    base_cp.created_at,
                )
                .fetch_all(&self.pool)
                .await?;

                let delta = IncrementalSnapshot {
                    base_checkpoint_id: Some(base_cp.id.clone()),
                    new_messages,
                    memory_changes: self.diff_memory(workspace_id, &base_cp.id).await?,
                    prompt_changed: false,
                    new_prompt: None,
                };

                // 如果增量数据量超过完整快照的 50%，退化为完整快照
                let delta_json = serde_json::to_vec(&delta)?;
                if delta_json.len() as u64 > self.policy.max_checkpoint_size / 2 {
                    return self.capture_snapshot(
                        conversation_id, workspace_id, turn_index,
                        CheckpointType::Auto, None, None, ctx,
                    ).await;
                }

                let compressed = zstd::encode_all(&delta_json[..], 3)?;
                // 存储增量快照...
                self.store_incremental(
                    conversation_id, workspace_id, turn_index,
                    &base_cp.id, &compressed, delta_json.len() as u64,
                ).await
            }
            None => {
                // 无基准 Checkpoint，创建完整快照
                self.capture_snapshot(
                    conversation_id, workspace_id, turn_index,
                    CheckpointType::Auto, None, None, ctx,
                ).await
            }
        }
    }
}
```

### 8.2 压缩策略

所有 Checkpoint 数据使用 zstd 压缩（level 3），在压缩率和速度之间取得平衡：

| 数据类型 | 典型未压缩大小 | 压缩后大小 | 压缩率 |
|---------|--------------|-----------|--------|
| Messages（50 轮对话） | ~200 KiB | ~40 KiB | ~80% |
| SystemPrompt | ~8 KiB | ~3 KiB | ~62% |
| MemorySnapshot | ~20 KiB | ~5 KiB | ~75% |
| ToolState | ~4 KiB | ~1.5 KiB | ~62% |

### 8.3 存储预算与清理

```rust
/// 工作区存储预算配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointStorageBudget {
    /// 每个工作区的 Checkpoint 存储上限（字节，默认 100 MiB）
    pub max_workspace_bytes: u64,
    /// 每个对话的 Checkpoint 存储上限（字节，默认 20 MiB）
    pub max_conversation_bytes: u64,
    /// 自动清理触发阈值（已用存储 / 上限 >= 此值时触发清理）
    pub cleanup_threshold: f64,
}

impl Default for CheckpointStorageBudget {
    fn default() -> Self {
        Self {
            max_workspace_bytes: 100 * 1024 * 1024,    // 100 MiB
            max_conversation_bytes: 20 * 1024 * 1024,  // 20 MiB
            cleanup_threshold: 0.8,
        }
    }
}

impl CheckpointManager {
    /// 检查并执行存储清理
    pub async fn enforce_storage_budget(
        &self,
        workspace_id: &str,
    ) -> Result<CleanupReport, CheckpointError> {
        let total_used = sqlx::query_scalar!(
            r#"SELECT COALESCE(SUM(size_bytes), 0) as "total: i64"
               FROM checkpoints WHERE workspace_id = ?"#,
            workspace_id,
        )
        .fetch_one(&self.pool)
        .await? as u64;

        let budget = &self.storage_budget;
        let threshold = (budget.max_workspace_bytes as f64 * budget.cleanup_threshold) as u64;

        if total_used < threshold {
            return Ok(CleanupReport { freed_bytes: 0, deleted_count: 0 });
        }

        // 按优先级清理：auto（最旧优先） → pre_risk（最旧优先）
        // 手动和工作流 Checkpoint 不参与自动清理
        let mut freed: u64 = 0;
        let mut deleted: usize = 0;

        let candidates = sqlx::query!(
            r#"SELECT id, size_bytes FROM checkpoints
               WHERE workspace_id = ?
                 AND checkpoint_type IN ('auto', 'pre_risk')
               ORDER BY created_at ASC"#,
            workspace_id,
        )
        .fetch_all(&self.pool)
        .await?;

        for cp in candidates {
            if total_used - freed <= budget.max_workspace_bytes / 2 {
                break; // 清理到 50% 以下即停
            }
            sqlx::query!("DELETE FROM checkpoints WHERE id = ?", cp.id)
                .execute(&self.pool)
                .await?;
            freed += cp.size_bytes as u64;
            deleted += 1;
        }

        tracing::info!(
            workspace_id,
            freed_bytes = freed,
            deleted_count = deleted,
            "Checkpoint 存储清理完成"
        );

        Ok(CleanupReport { freed_bytes: freed, deleted_count: deleted })
    }
}

#[derive(Debug, Serialize)]
pub struct CleanupReport {
    pub freed_bytes: u64,
    pub deleted_count: usize,
}
```

---

## 9. Checkpoint 比较与回放

### 9.1 Checkpoint 比较（Diff）

两个 Checkpoint 之间的差异包括：消息增减、记忆变更、SystemPrompt 变更。Diff 结果用于 UI 展示和调试：

```rust
// crates/agent-core/src/checkpoint/diff.rs

#[derive(Debug, Serialize)]
pub struct CheckpointDiff {
    /// 两个 Checkpoint 的基本信息
    pub from: CheckpointSummary,
    pub to: CheckpointSummary,
    /// 消息差异
    pub message_diff: MessageDiff,
    /// 记忆差异
    pub memory_diff: MemoryDiff,
    /// SystemPrompt 是否变更
    pub prompt_changed: bool,
}

#[derive(Debug, Serialize)]
pub struct MessageDiff {
    /// 新增的消息
    pub added: Vec<MessageSummary>,
    /// 仅在 from 中存在的消息（被删除或回滚的）
    pub removed: Vec<MessageSummary>,
    /// 共同消息数
    pub common_count: usize,
}

#[derive(Debug, Serialize)]
pub struct MemoryDiff {
    pub added: Vec<MemoryEntrySnapshot>,
    pub removed: Vec<MemoryEntrySnapshot>,
    pub modified: Vec<MemoryEntryModification>,
}

#[derive(Debug, Serialize)]
pub struct MemoryEntryModification {
    pub key: String,
    pub category: String,
    pub old_value: String,
    pub new_value: String,
    pub old_importance: f64,
    pub new_importance: f64,
}

#[derive(Debug, Serialize)]
pub struct MessageSummary {
    pub role: String,
    pub preview: String,
    pub created_at: i64,
}

#[derive(Debug, Serialize)]
pub struct CheckpointSummary {
    pub id: String,
    pub label: Option<String>,
    pub turn_index: u32,
    pub message_count: u32,
    pub created_at: i64,
}

impl CheckpointManager {
    /// 比较两个 Checkpoint 的差异
    pub async fn compare(
        &self,
        from_id: &str,
        to_id: &str,
    ) -> Result<CheckpointDiff, CheckpointError> {
        // 加载两个 Checkpoint 的消息数据
        let from_msgs: Vec<StoredMessage> = self.load_and_parse(
            from_id, CheckpointDataType::Messages,
        ).await?;
        let to_msgs: Vec<StoredMessage> = self.load_and_parse(
            to_id, CheckpointDataType::Messages,
        ).await?;

        // 按 created_at 对齐，计算消息差异
        let from_set: HashSet<i64> = from_msgs.iter().map(|m| m.created_at).collect();
        let to_set: HashSet<i64> = to_msgs.iter().map(|m| m.created_at).collect();

        let added: Vec<MessageSummary> = to_msgs.iter()
            .filter(|m| !from_set.contains(&m.created_at))
            .map(|m| MessageSummary {
                role: m.role.clone(),
                preview: truncate(&m.content, 80),
                created_at: m.created_at,
            })
            .collect();

        let removed: Vec<MessageSummary> = from_msgs.iter()
            .filter(|m| !to_set.contains(&m.created_at))
            .map(|m| MessageSummary {
                role: m.role.clone(),
                preview: truncate(&m.content, 80),
                created_at: m.created_at,
            })
            .collect();

        // 加载并比较记忆快照
        let from_mem: MemorySnapshotData = self.load_and_parse(
            from_id, CheckpointDataType::MemorySnapshot,
        ).await?;
        let to_mem: MemorySnapshotData = self.load_and_parse(
            to_id, CheckpointDataType::MemorySnapshot,
        ).await?;

        let memory_diff = diff_memory_snapshots(&from_mem, &to_mem);

        // 比较 SystemPrompt
        let from_prompt: String = self.load_and_parse(
            from_id, CheckpointDataType::SystemPrompt,
        ).await.unwrap_or_default();
        let to_prompt: String = self.load_and_parse(
            to_id, CheckpointDataType::SystemPrompt,
        ).await.unwrap_or_default();

        let from_cp = self.get_checkpoint(from_id).await?.unwrap();
        let to_cp = self.get_checkpoint(to_id).await?.unwrap();

        Ok(CheckpointDiff {
            from: CheckpointSummary {
                id: from_cp.id,
                label: from_cp.label,
                turn_index: from_cp.turn_index,
                message_count: from_cp.message_count,
                created_at: from_cp.created_at.timestamp_millis(),
            },
            to: CheckpointSummary {
                id: to_cp.id,
                label: to_cp.label,
                turn_index: to_cp.turn_index,
                message_count: to_cp.message_count,
                created_at: to_cp.created_at.timestamp_millis(),
            },
            message_diff: MessageDiff {
                added,
                removed,
                common_count: from_set.intersection(&to_set).count(),
            },
            memory_diff,
            prompt_changed: from_prompt != to_prompt,
        })
    }
}
```

### 9.2 步进回放

从 Checkpoint A 到 Checkpoint B 的步进回放，逐条消息重放以辅助调试：

```rust
impl CheckpointManager {
    /// 步进回放：从 from_checkpoint 逐步播放到 to_checkpoint
    pub async fn replay_steps(
        &self,
        from_id: &str,
        to_id: &str,
    ) -> Result<Vec<ReplayStep>, CheckpointError> {
        let from_msgs: Vec<StoredMessage> = self.load_and_parse(
            from_id, CheckpointDataType::Messages,
        ).await?;
        let to_msgs: Vec<StoredMessage> = self.load_and_parse(
            to_id, CheckpointDataType::Messages,
        ).await?;

        // 找出 to 中相对于 from 新增的消息（按时间排序）
        let from_count = from_msgs.len();
        let steps: Vec<ReplayStep> = to_msgs.iter()
            .skip(from_count)
            .enumerate()
            .map(|(i, msg)| ReplayStep {
                step_index: i as u32,
                role: msg.role.clone(),
                content_preview: truncate(&msg.content, 200),
                tool_name: msg.tool_name.clone(),
                tool_call_id: msg.tool_call_id.clone(),
                token_count: msg.token_count,
                created_at: msg.created_at,
            })
            .collect();

        Ok(steps)
    }
}

#[derive(Debug, Serialize)]
pub struct ReplayStep {
    pub step_index: u32,
    pub role: String,
    pub content_preview: String,
    pub tool_name: Option<String>,
    pub tool_call_id: Option<String>,
    pub token_count: i64,
    pub created_at: i64,
}
```

---

## 10. Tauri Commands

### 10.1 create_checkpoint

```rust
/// 创建手动 Checkpoint
#[tauri::command]
pub async fn create_checkpoint(
    state: State<'_, AppState>,
    conversation_id: String,
    label: String,
    description: Option<String>,
) -> Result<CheckpointResponse, AppError> {
    let workspace_id = state.active_workspace_id().await?;

    let instance = state.agent_manager
        .get_by_workspace(&workspace_id).await
        .ok_or(AppError::NotFound("活跃 Agent 实例不存在".into()))?;

    let inst = instance.read().await;
    let ctx = inst.executor.as_ref()
        .ok_or(AppError::Tool("Executor 未就绪".into()))?
        .agent_context();

    let checkpoint = state.checkpoint_manager
        .create_manual(
            &conversation_id,
            &workspace_id,
            &label,
            description.as_deref(),
            &ctx,
        )
        .await
        .map_err(|e| AppError::Tool(e.to_string()))?;

    Ok(CheckpointResponse::from(checkpoint))
}

#[derive(Serialize)]
pub struct CheckpointResponse {
    pub id: String,
    pub conversation_id: String,
    pub turn_index: u32,
    pub label: Option<String>,
    pub description: Option<String>,
    pub checkpoint_type: String,
    pub message_count: u32,
    pub size_bytes: u64,
    pub created_at: i64,
    pub metadata: CheckpointMetadata,
}
```

### 10.2 list_checkpoints

```rust
/// 列出对话的所有 Checkpoint
#[tauri::command]
pub async fn list_checkpoints(
    state: State<'_, AppState>,
    conversation_id: String,
    checkpoint_type: Option<String>,
) -> Result<Vec<CheckpointResponse>, AppError> {
    let checkpoints = state.checkpoint_manager
        .list(&conversation_id, checkpoint_type.as_deref())
        .await
        .map_err(|e| AppError::Tool(e.to_string()))?;

    Ok(checkpoints.into_iter().map(CheckpointResponse::from).collect())
}
```

### 10.3 restore_checkpoint

```rust
/// 恢复 Checkpoint
#[tauri::command]
pub async fn restore_checkpoint(
    state: State<'_, AppState>,
    checkpoint_id: String,
    mode: String,             // "overwrite" | "fork"
    restore_memory: Option<bool>,
    branch_title: Option<String>,
) -> Result<RestoreResult, AppError> {
    let workspace_id = state.active_workspace_id().await?;

    let instance = state.agent_manager
        .get_by_workspace(&workspace_id).await
        .ok_or(AppError::NotFound("活跃 Agent 实例不存在".into()))?;

    let mut inst = instance.write().await;

    // Agent 必须处于 Ready 状态
    if !matches!(inst.status, AgentLifecycle::Ready) {
        return Err(AppError::Validation(
            "请等待当前操作完成后再恢复 Checkpoint".into()
        ));
    }

    let options = RestoreOptions {
        mode: match mode.as_str() {
            "fork" => RestoreMode::Fork,
            _ => RestoreMode::Overwrite,
        },
        restore_memory: restore_memory.unwrap_or(true),
        restore_tool_state: true,
        restore_pending_queue: false,
        branch_title,
    };

    let result = state.checkpoint_manager
        .restore(&checkpoint_id, options, &mut inst)
        .await
        .map_err(|e| AppError::Tool(e.to_string()))?;

    // 通知前端对话状态变更
    state.emitter.emit("checkpoint_restored", serde_json::json!({
        "checkpoint_id": checkpoint_id,
        "conversation_id": result.conversation_id,
        "forked": result.forked,
    })).await.ok();

    Ok(result)
}
```

### 10.4 delete_checkpoint

```rust
/// 删除 Checkpoint
#[tauri::command]
pub async fn delete_checkpoint(
    state: State<'_, AppState>,
    checkpoint_id: String,
) -> Result<(), AppError> {
    // 级联删除 checkpoint_data（外键 ON DELETE CASCADE）
    let deleted = sqlx::query!(
        "DELETE FROM checkpoints WHERE id = ?",
        checkpoint_id,
    )
    .execute(&state.pool)
    .await
    .map_err(|e| AppError::Tool(e.to_string()))?;

    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound(
            format!("Checkpoint {} 不存在", checkpoint_id)
        ));
    }

    Ok(())
}
```

### 10.5 compare_checkpoints

```rust
/// 比较两个 Checkpoint 的差异
#[tauri::command]
pub async fn compare_checkpoints(
    state: State<'_, AppState>,
    from_id: String,
    to_id: String,
) -> Result<CheckpointDiff, AppError> {
    state.checkpoint_manager
        .compare(&from_id, &to_id)
        .await
        .map_err(|e| AppError::Tool(e.to_string()))
}
```

### 10.6 get_checkpoint_detail

```rust
/// 获取 Checkpoint 详情（含数据分片摘要，不含完整数据）
#[tauri::command]
pub async fn get_checkpoint_detail(
    state: State<'_, AppState>,
    checkpoint_id: String,
) -> Result<CheckpointDetail, AppError> {
    let checkpoint = state.checkpoint_manager
        .get_checkpoint(&checkpoint_id)
        .await
        .map_err(|e| AppError::Tool(e.to_string()))?
        .ok_or(AppError::NotFound(format!("Checkpoint {} 不存在", checkpoint_id)))?;

    // 查询各数据分片的大小信息
    let data_parts = sqlx::query!(
        r#"SELECT data_type, LENGTH(data) as "compressed_size: i64",
                  uncompressed_size as "uncompressed_size: i64"
           FROM checkpoint_data
           WHERE checkpoint_id = ?"#,
        checkpoint_id,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AppError::Tool(e.to_string()))?;

    Ok(CheckpointDetail {
        checkpoint: CheckpointResponse::from(checkpoint),
        data_parts: data_parts.into_iter().map(|p| DataPartInfo {
            data_type: p.data_type,
            compressed_size: p.compressed_size as u64,
            uncompressed_size: p.uncompressed_size as u64,
        }).collect(),
    })
}

#[derive(Serialize)]
pub struct CheckpointDetail {
    pub checkpoint: CheckpointResponse,
    pub data_parts: Vec<DataPartInfo>,
}

#[derive(Serialize)]
pub struct DataPartInfo {
    pub data_type: String,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
}
```

---

## 11. 前端组件

### 11.1 CheckpointTimeline

在对话消息流中嵌入的可视化时间线，标记各 Checkpoint 的位置：

```typescript
// apps/desktop/src/components/checkpoint/CheckpointTimeline.tsx

import { useEffect, useState } from "react";
import { invoke, listen } from "@tauri-apps/api";

interface CheckpointMarker {
  id: string;
  turnIndex: number;
  label: string | null;
  checkpointType: "auto" | "manual" | "workflow" | "pre_risk";
  messageCount: number;
  createdAt: number;
}

export function CheckpointTimeline({
  conversationId,
}: {
  conversationId: string;
}) {
  const [markers, setMarkers] = useState<CheckpointMarker[]>([]);

  useEffect(() => {
    invoke<CheckpointMarker[]>("list_checkpoints", { conversationId })
      .then(setMarkers);

    const unlisten = listen("checkpoint_restored", () => {
      invoke<CheckpointMarker[]>("list_checkpoints", { conversationId })
        .then(setMarkers);
    });

    return () => { unlisten.then(f => f()); };
  }, [conversationId]);

  return (
    <div className="checkpoint-timeline">
      {markers.map((marker) => (
        <CheckpointDot
          key={marker.id}
          marker={marker}
          onRestore={(id) => handleRestore(id)}
          onDelete={(id) => handleDelete(id)}
        />
      ))}
    </div>
  );
}

function CheckpointDot({
  marker,
  onRestore,
  onDelete,
}: {
  marker: CheckpointMarker;
  onRestore: (id: string) => void;
  onDelete: (id: string) => void;
}) {
  const isManual = marker.checkpointType === "manual";

  return (
    <div
      className={`checkpoint-dot ${isManual ? "manual" : "auto"}`}
      title={marker.label ?? `自动快照 #${marker.turnIndex}`}
    >
      {/* 手动 Checkpoint 显示标签名，自动 Checkpoint 显示圆点 */}
      {isManual ? (
        <span className="checkpoint-label">{marker.label}</span>
      ) : (
        <span className="checkpoint-auto-dot" />
      )}
      <div className="checkpoint-actions">
        <button onClick={() => onRestore(marker.id)}>恢复</button>
        {isManual && (
          <button onClick={() => onDelete(marker.id)}>删除</button>
        )}
      </div>
    </div>
  );
}
```

### 11.2 RestoreDialog

恢复确认对话框，展示 Checkpoint 详情并让用户选择恢复模式：

```typescript
// apps/desktop/src/components/checkpoint/RestoreDialog.tsx

interface RestoreDialogProps {
  checkpointId: string;
  onConfirm: (options: RestoreOptions) => void;
  onCancel: () => void;
}

interface RestoreOptions {
  mode: "overwrite" | "fork";
  restoreMemory: boolean;
  branchTitle?: string;
}

export function RestoreDialog({
  checkpointId,
  onConfirm,
  onCancel,
}: RestoreDialogProps) {
  const [detail, setDetail] = useState<CheckpointDetail | null>(null);
  const [mode, setMode] = useState<"overwrite" | "fork">("fork");
  const [restoreMemory, setRestoreMemory] = useState(true);
  const [branchTitle, setBranchTitle] = useState("");

  useEffect(() => {
    invoke<CheckpointDetail>("get_checkpoint_detail", { checkpointId })
      .then(setDetail);
  }, [checkpointId]);

  if (!detail) return <div>加载中...</div>;

  const cp = detail.checkpoint;

  return (
    <div className="restore-dialog-overlay">
      <div className="restore-dialog">
        <h3>恢复快照</h3>

        {/* Checkpoint 摘要 */}
        <div className="checkpoint-summary">
          <p><strong>标签：</strong>{cp.label ?? "(自动快照)"}</p>
          <p><strong>轮次：</strong>{cp.turnIndex}</p>
          <p><strong>消息数：</strong>{cp.messageCount}</p>
          <p><strong>创建时间：</strong>{formatTime(cp.createdAt)}</p>
          <p><strong>快照大小：</strong>{formatBytes(cp.sizeBytes)}</p>
        </div>

        {/* 数据分片详情 */}
        <div className="data-parts">
          <h4>包含的数据：</h4>
          {detail.dataParts.map((part) => (
            <div key={part.dataType} className="data-part-row">
              <span>{dataTypeLabel(part.dataType)}</span>
              <span>{formatBytes(part.compressedSize)}</span>
            </div>
          ))}
        </div>

        {/* 恢复模式选择 */}
        <div className="restore-mode">
          <label>
            <input
              type="radio"
              checked={mode === "fork"}
              onChange={() => setMode("fork")}
            />
            创建新分支（保留当前对话）
          </label>
          <label>
            <input
              type="radio"
              checked={mode === "overwrite"}
              onChange={() => setMode("overwrite")}
            />
            覆写当前对话（不可撤销）
          </label>
        </div>

        {mode === "fork" && (
          <input
            placeholder="分支标题（可选）"
            value={branchTitle}
            onChange={(e) => setBranchTitle(e.target.value)}
          />
        )}

        <label>
          <input
            type="checkbox"
            checked={restoreMemory}
            onChange={(e) => setRestoreMemory(e.target.checked)}
          />
          同时恢复记忆状态（MEMORY.md）
        </label>

        {/* 操作按钮 */}
        <div className="dialog-actions">
          <button onClick={onCancel}>取消</button>
          <button
            className="primary"
            onClick={() =>
              onConfirm({
                mode,
                restoreMemory,
                branchTitle: branchTitle || undefined,
              })
            }
          >
            确认恢复
          </button>
        </div>
      </div>
    </div>
  );
}
```

### 11.3 CheckpointManager 面板

侧边栏中的 Checkpoint 管理面板，展示所有 Checkpoint 并支持批量操作：

```typescript
// apps/desktop/src/components/checkpoint/CheckpointManager.tsx

export function CheckpointManagerPanel({
  conversationId,
}: {
  conversationId: string;
}) {
  const [checkpoints, setCheckpoints] = useState<CheckpointResponse[]>([]);
  const [filter, setFilter] = useState<string | null>(null);
  const [compareMode, setCompareMode] = useState(false);
  const [selectedPair, setSelectedPair] = useState<[string?, string?]>([]);

  useEffect(() => {
    loadCheckpoints();
  }, [conversationId, filter]);

  async function loadCheckpoints() {
    const list = await invoke<CheckpointResponse[]>("list_checkpoints", {
      conversationId,
      checkpointType: filter,
    });
    setCheckpoints(list);
  }

  async function handleCompare() {
    const [fromId, toId] = selectedPair;
    if (!fromId || !toId) return;
    const diff = await invoke<CheckpointDiff>("compare_checkpoints", {
      fromId,
      toId,
    });
    // 展示 diff 视图...
  }

  return (
    <div className="checkpoint-manager">
      <div className="manager-header">
        <h3>状态快照</h3>
        <div className="filter-tabs">
          <button
            className={filter === null ? "active" : ""}
            onClick={() => setFilter(null)}
          >
            全部
          </button>
          <button
            className={filter === "manual" ? "active" : ""}
            onClick={() => setFilter("manual")}
          >
            手动
          </button>
          <button
            className={filter === "auto" ? "active" : ""}
            onClick={() => setFilter("auto")}
          >
            自动
          </button>
        </div>
      </div>

      <div className="checkpoint-list">
        {checkpoints.map((cp) => (
          <CheckpointCard
            key={cp.id}
            checkpoint={cp}
            compareMode={compareMode}
            selected={selectedPair.includes(cp.id)}
            onSelect={(id) => handleSelect(id)}
            onRestore={(id) => openRestoreDialog(id)}
            onDelete={(id) => handleDelete(id)}
          />
        ))}
      </div>

      <div className="manager-footer">
        <button onClick={() => setCompareMode(!compareMode)}>
          {compareMode ? "退出比较" : "比较模式"}
        </button>
        {compareMode && selectedPair[0] && selectedPair[1] && (
          <button onClick={handleCompare}>查看差异</button>
        )}
      </div>
    </div>
  );
}
```

---

## 12. 相关文档

| 文档 | 关联内容 |
|------|---------|
| [07-Agent生命周期详细设计.md](07-Agent生命周期详细设计.md) | Compact 与历史控制（Section 6）、事件与持久化（Section 11）、AstroThread / Session 生命周期 |
| [03-对话分支系统设计.md](../05-桌面端与交互/03-对话分支系统设计.md) | `create_branch` 分支创建逻辑、消息复制策略、分支树查询 |
| [05-工作流DAG执行引擎设计.md](../_v0.3规划/05-工作流DAG执行引擎设计.md) | `workflow_runs` 暂停/恢复（Section 7）、ExecutionContext 序列化 |
| [01-数据库访问层详细设计.md](../06-安全与基础设施/01-数据库访问层详细设计.md) | Repository 模式、事务管理规范、TaskTraceRepo |
| [01-Schema设计.md](../../03-系统设计阶段/04-数据库设计/01-Schema设计.md) | conversations / messages / memory_entries 表 DDL |
| [02-agent-runtime详细设计.md](02-agent-runtime详细设计.md) | 运行时执行模型（注：代码中 AgentExecutor / round_loop / PendingQueue / HumanGuard 已不存在，当前为 SessionTask / turn_lifecycle / submission_loop） |
| [03-交互执行模式设计.md](03-交互执行模式设计.md) | RiskLevel 分级、高风险工具列表 |
| [01-Tauri桌面端详细设计.md](../05-桌面端与交互/01-Tauri桌面端详细设计.md) | AppState 设计、Tauri Command 模式、EventEmitter |
