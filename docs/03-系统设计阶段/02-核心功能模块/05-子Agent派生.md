# 子 Agent 派生

> 文档状态：定稿 | 阶段：系统设计 | 拆分自：原 07-MCP与Skills与子Agent.md

---

## 一、设计哲学

子 Agent 是一个**全新的、完全隔离的 AgentCore 实例**，对父 Agent 的对话历史一无所知。父 Agent 必须通过 `context` 字段主动传入所有所需信息；子 Agent 只把最终结构化摘要回传给父 Agent，以此保持 Token 高效。

---

## 二、`delegate_task` 工具接口

```rust
pub struct DelegateTaskInput {
    pub goal: String,                          // 子任务目标（自然语言描述）
    pub context: String,                       // 父 Agent 主动传入的所有背景信息
    pub model: Option<String>,                 // 可指定更便宜的模型（如 claude-haiku-4-5）
    pub allowed_tools: Option<Vec<String>>,    // 可选白名单，限制子 Agent 可用工具
    pub timeout_secs: Option<u32>,             // 硬超时，默认 300s（5 分钟）
    pub max_iterations: Option<u32>,           // 最大循环次数，默认 50
}
```

**硬超时机制**：`timeout_secs`（默认 300s）为子 Agent 的绝对执行时间上限。超时后通过 `CancellationToken` 终止子 Agent，返回已完成的部分结果和 `timed_out: true` 标记。与 `max_iterations`（循环次数限制）互为补充——前者防止单次工具调用耗时过长导致的整体超时，后者防止无限循环。

支持两种调用形式：

- **单任务**：一次调用派生一个子 Agent
- **批量并行**：数组形式一次派生多个，并发执行，结果按索引排序回传

---

## 三、Supervisor

```rust
pub struct Supervisor {
    spawned: Vec<AgentHandle>,
    max_depth: u8,                   // 最大嵌套深度，可配置 1-3，默认 1（扁平）
    max_concurrent_children: usize,  // 最大并发子 Agent 数，默认 3
}

pub struct AgentHandle {
    pub id: Uuid,
    pub role: String,
    task_tx: mpsc::Sender<AgentTask>,
    result_rx: oneshot::Receiver<AgentResult>,
}

impl Supervisor {
    /// 派生子 Agent
    pub async fn spawn(
        &mut self,
        role: &str,
        task: AgentTask,
        config: SpawnConfig,
    ) -> anyhow::Result<AgentHandle>;

    /// 并行等待所有子 Agent 完成
    pub async fn join_all(&mut self) -> Vec<AgentResult>;

    /// 流式订阅子 Agent 事件（实时进度，/agents 监控面板数据来源）
    pub fn subscribe(&self, id: Uuid) -> broadcast::Receiver<AgentEvent>;
}
```

---

## 四、派生规则

- 子 Agent 共享 `Arc<dyn LlmClient>`（复用连接池，不重建 HTTP 连接）
- 子 Agent 有独立的 `short_term` memory（避免上下文污染）
- `AgentContext.depth` 每次 spawn 递增，超过阈值直接返回工具错误
- `SpawnConfig` 可限制子 Agent 可用的 Tool/Skill 白名单
- 超出 `max_concurrent_children` 时返回工具错误，不静默截断
- 每个子 Agent 最多执行 `max_iterations = 50` 次循环，防止无限递归

---

## 五、强制屏蔽的工具类别

子 Agent 继承父 Agent 工具集，但以下 7 类工具**强制屏蔽**以防副作用：

| 屏蔽原因 | 工具类型 |
| -------- | -------- |
| 防止子 Agent 再派生（除非 `role="orchestrator"`） | `delegate_task` |
| 防止意外发布 / 推送 | `git_push`、`publish` 类 |
| 防止跨会话写入持久记忆 | `memory_write` |
| 防止影响主 UI 状态 | UI 操作类 |
| 防止修改全局配置 | 配置写入类 |
| 子 Agent 启动的后台进程父 Agent 无法感知或清理 | `shell_bg`（后台进程启动） |
| 与 `memory_write` 同理，属于跨会话持久副作用 | `knowledge_manage`（知识库写入） |

---

## 六、嵌套深度与并发限制

```text
depth=0  主 Agent
  └─ depth=1  子 Agent（默认最大层级，max_spawn_depth=1）
        └─ depth=2  需 role="orchestrator" 才允许
              └─ depth=3  最大值；前端配置界面强制显示费用警告
                            最多 3³=27 个并发叶子节点
                    └─ 直接返回错误，不再递归
```

---

## 七、子 Agent 生命周期与监控

```text
父 Agent 调用 delegate_task
        │
        ▼
Supervisor::spawn()
  ├─ 检查 depth（超限返回错误）
  ├─ 检查 max_concurrent_children（超限返回错误）
  └─ 创建新 AgentCore 实例
        │
        ▼
子 AgentCore 执行主循环
  ├─ 以 goal + context 作为初始输入
  ├─ 正常调用 Tool / Skill / MCP 工具
  └─ 心跳检测：无响应超时 → 写诊断日志
        │
        ▼
子 Agent 完成 → 返回结构化摘要（SubAgentResult）
        │
        ▼
父 Agent 按任务索引排序合并摘要 → 继续主任务
```

---

## 八、子 Agent 结果结构

```rust
pub struct SubAgentResult {
    pub actions: Vec<String>,          // 执行了哪些操作
    pub findings: Vec<String>,         // 发现了什么
    pub modified_files: Vec<String>,   // 修改了哪些文件
    pub issues: Vec<String>,           // 遇到了哪些问题
    pub token_usage: TokenUsage,       // Token 消耗统计
    pub timed_out: bool,               // 是否因超时终止
}

pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_cost_usd: f64,
}
```

父 Agent 收到 `SubAgentResult` 后可据 `token_usage` 决定后续子任务是否降级模型（如从 claude-sonnet-4 切换到 claude-haiku-4-5），实现自适应成本控制。

---

## 九、子 Agent 清理机制

子 Agent 完成后自动从 `Supervisor.children` DashMap 中移除。父 Agent 取消时通过 `CancellationToken` 级联取消所有子 Agent。

```rust
impl Supervisor {
    /// 取消所有子 Agent（父 Agent 被取消时调用）
    pub fn cancel_all(&self) {
        for entry in self.children.iter() {
            entry.value().cancel_token.cancel();
        }
        self.children.clear();
    }

    /// 子 Agent 完成后的清理（由子 Agent 自身触发）
    fn on_child_completed(&self, child_id: Uuid) {
        self.children.remove(&child_id);
    }
}
```

硬超时通过 `tokio::time::timeout` 包装子 Agent 的 `round_loop`，超时后先发送 `CancellationToken`，等待 5s 优雅关闭，再强制 abort：

```rust
let result = tokio::time::timeout(
    Duration::from_secs(timeout_secs as u64),
    child.round_loop(),
).await;

match result {
    Ok(Ok(outcome)) => { /* 正常完成 */ }
    Ok(Err(e)) => { /* 执行错误 */ }
    Err(_) => {
        // 硬超时：级联取消 + 收集部分结果
        child.cancel_token.cancel();
        tokio::time::sleep(Duration::from_secs(5)).await;
        child.task.abort();
        // 返回 timed_out = true 的部分结果
    }
}
```

完成事件持久化写入 SQLite，应用重启后可恢复路由；前端 `/agents` 监控面板通过 `broadcast::Receiver<AgentEvent>` 实时接收进度，并支持回放历史。

---

## 十、`delegate_task` vs `execute_code` 决策原则

| 场景 | 选择 | 原因 |
| ---- | ---- | ---- |
| 需要推理、多步规划、调用 LLM | `delegate_task` | 灵活但 Token 较高 |
| 机械性数据处理、脚本执行 | `execute_code` | Token 极低，速度快 |

---

## 相关文档

- [03-MCP集成.md](03-MCP集成.md) — MCP 集成设计
- [04-Skills系统.md](04-Skills系统.md) — Skills 系统设计
- `04-详细设计阶段/01-核心引擎层/02-agent-runtime详细设计.md` — Supervisor 详细实现
