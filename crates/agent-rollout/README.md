# agent-rollout

JSONL append-only 历史记录 -- Thread 历史的权威事实源，负责 rollout 文件的写入、读取、路径管理与持久化策略。

## 核心职责

1. **Rollout 写入（RolloutRecorder）**：异步 append-only 写入器，通过 unbounded channel + 后台 writer loop 实现无锁高吞吐 JSONL 追加，自动过滤瞬态事件
2. **持久化策略（policy）**：`should_persist_event_msg()` 按事件变体精确划分持久/瞬态边界 -- ItemCompleted、TurnStarted、TurnComplete 等为持久，Delta 增量和 Error 通知为瞬态
3. **路径管理（path）**：日期分区目录结构（`YYYY/MM/DD/rollout-{timestamp}-{encoded_thread_id}.jsonl`），thread_id 百分号编码防止路径穿越攻击
4. **历史重建（reconstruction）**：`read_rollout()` / `read_rollout_with_diagnostics()` 逐行解析 JSONL，容忍中间行损坏或末尾截断，保留有效前缀
5. **统一 Item 模型（RolloutItem）**：7 种变体（SessionMeta / ResponseItem / EventMsg / TurnContext / WorldState / Compacted / InterAgentCommunication）覆盖线程全生命周期数据

## 模块结构

| 文件 | 职责 |
|------|------|
| `src/lib.rs` | `RolloutItem` 枚举定义（7 变体）+ 模块声明与 re-export |
| `src/recorder.rs` | `RolloutRecorder` -- 异步写入器：`open()` / `record()` / `flush()` / `shutdown()`；后台 `run_writer_loop` 处理命令队列 |
| `src/policy.rs` | `should_persist_event_msg()` / `is_persisted_rollout_item()` -- 持久化策略函数 |
| `src/path.rs` | `new_rollout_path()` / `find_rollout()` -- 路径生成与按 thread_id 查找最新 rollout 文件 |
| `src/reconstruction.rs` | `read_rollout()` / `read_rollout_with_diagnostics()` / `RolloutRead` -- JSONL 文件读取与容错解析 |

## 核心类型与 API

### RolloutItem（`lib.rs`）

```rust
pub enum RolloutItem {
    SessionMeta(Value),               // 会话元数据
    ResponseItem(types::message::Message), // LLM 响应消息（含 tool_calls）
    EventMsg(agent_protocol::EventMsg),    // 领域事件
    TurnContext(Value),                // 轮次上下文快照
    WorldState(Value),                 // 世界状态快照
    Compacted(Value),                  // 压缩后的上下文
    InterAgentCommunication(Value),    // 跨 Agent 通信记录
}
```

### RolloutRecorder（`recorder.rs`）

| API | 说明 |
|-----|------|
| `RolloutRecorder::open(path)` | 异步打开/创建 JSONL 文件，启动后台 writer 协程 |
| `recorder.record(items)` | 写入 items（自动按 `is_persisted_rollout_item` 过滤瞬态项） |
| `recorder.flush()` | 刷盘等待 |
| `recorder.shutdown()` | 刷盘并终止 writer loop |
| `recorder.path()` | 获取 rollout 文件路径 |

### 持久化策略（`policy.rs`）

| 函数 | 说明 |
|------|------|
| `should_persist_event_msg(&EventMsg) -> bool` | 判断事件是否持久化（ItemCompleted / TurnStarted / TurnComplete / TurnAborted / TokenCount / ContextUsage / UserInputCommitted / ThreadSettingsApplied / ThreadRolledBack 为 true） |
| `is_persisted_rollout_item(&RolloutItem) -> bool` | 判断 rollout item 是否持久化（EventMsg 委托上述函数；其余 6 种 item 始终持久） |

### 路径管理（`path.rs`）

| 函数 | 说明 |
|------|------|
| `new_rollout_path(root, thread_id, now) -> PathBuf` | 生成日期分区路径：`{root}/YYYY/MM/DD/rollout-{timestamp}-{encoded_id}.jsonl` |
| `find_rollout(root, thread_id) -> Option<PathBuf>` | 遍历日期目录，按字典序返回最新匹配的 rollout 文件 |

### 历史重建（`reconstruction.rs`）

| 类型/函数 | 说明 |
|-----------|------|
| `RolloutRead` | 读取结果：`items: Vec<RolloutItem>` + `parse_errors: usize` |
| `read_rollout(path) -> Vec<RolloutItem>` | 便捷函数，忽略解析错误计数 |
| `read_rollout_with_diagnostics(path) -> RolloutRead` | 完整读取，返回有效 items 与错误数（容忍损坏行和截断尾部） |

## 与其他 crate 的关系

```
agent-rollout (本 crate)
  ├── 依赖 agent-protocol → EventMsg 用于持久化策略判断
  ├── 依赖 types (agent-types) → Message 类型用于 ResponseItem
  ├── 被 agent-core 使用 → 运行时写入 rollout 历史
  └── 被 agent-server 使用 → Thread resume 时重建历史
```

- **agent-protocol**：`EventMsg` 类型被 `policy.rs` 直接 match 判断持久化
- **agent-types**：`Message` 类型用于 `RolloutItem::ResponseItem`
- **agent-core**：运行时在每个 turn 结束时调用 `RolloutRecorder::record()` 写入事件
- **agent-server**：`ResumeThread` RPC 调用 `read_rollout()` 重建线程历史快照

## 测试运行

```bash
# 运行全部测试
cargo test -p agent-rollout

# 运行特定模块测试
cargo test -p agent-rollout find_rollout
cargo test -p agent-rollout record_then_flush
cargo test -p agent-rollout retains_valid_items
cargo test -p agent-rollout durable_policy
```

> 测试分布：`path.rs`（4 个）、`recorder.rs`（3 个）、`policy.rs`（6 个）、`reconstruction.rs`（3 个），共 16 个单元测试，覆盖路径安全、写入顺序、容错解析与持久化策略。
