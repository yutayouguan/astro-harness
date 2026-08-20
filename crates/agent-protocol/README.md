# agent-protocol

Core 领域事件协议 -- 运行时唯一的结构化事件格式，定义 Agent 会话中所有可观察事件、轮次 Item 与控制操作。

## 核心职责

1. **事件模型（Event / EventMsg）**：定义 28 种事件变体，覆盖 turn 生命周期、item 完成/增量、控制请求、token 统计、上下文用量等全部运行时可观察行为
2. **轮次 Item 类型（TurnItem）**：17 种 Item 变体统一表示用户消息、Agent 回复、工具执行、MCP 调用、子 Agent 活动等轮次内容
3. **提交操作（Op / Submission）**：定义 16 种控制操作枚举，从用户输入、中断、审批到线程回滚与关机，是前端→后端的完整控制面
4. **序列化契约**：所有类型实现 `Serialize` / `Deserialize`，使用 `tag + content` 模式的 serde JSON 序列化，确保跨进程可靠传输

## 模块结构

| 文件 | 职责 |
|------|------|
| `src/lib.rs` | 模块声明与 re-export（`pub use event::*` / `items::*` / `submission::*`） |
| `src/event.rs` | 事件模型：`Event`、`EventMsg`（28 变体）、`ItemEvent`、`DeltaEvent`、`ControlRequestEvent`、`TokenCountEvent`、`ContextUsageEvent` 等 |
| `src/items.rs` | 轮次 Item 类型：`TurnItem`（17 变体）、`TextItem`、`ToolItem`、`ExtensionItem`、`ToolStatus` |
| `src/submission.rs` | 控制操作：`Op`（16 变体）、`Submission`、`TurnInput`、`TurnInputRequest`、`TurnInputMode`、`TurnInputSubmission`、`TurnInputError` |

## 核心类型与 API

### 事件层（`event.rs`）

| 类型 | 说明 |
|------|------|
| `Event` | 顶层事件包装：`id: String` + `msg: EventMsg` |
| `EventMsg` | 28 变体 tagged enum（`#[serde(tag = "type", content = "data")]`） |
| `ItemEvent` | Item 事件载体：`turn_id` + `TurnItem` |
| `DeltaEvent` | 增量事件：`turn_id` + `item_id` + `delta: String` |
| `ControlRequestEvent` | 控制请求：`turn_id` + `item_id` + `request_id` + `payload: Value` |
| `TurnStartedEvent` | Turn 开始事件 |
| `TurnCompleteEvent` | Turn 完成事件（含可选 `last_agent_message` 和 `error`） |
| `TurnAbortedEvent` | Turn 中止事件（含 `TurnAbortReason`：Interrupted / Replaced / ReviewEnded / BudgetLimited） |
| `TokenCountEvent` | Token 统计：input / output / total / cache_read / cache_write / reasoning / request_count |
| `ContextUsageEvent` | 上下文用量：context_window、segments 分段统计、recommend_compact 标志 |
| `EventMsg::is_terminal()` | 判断事件是否为终态（`TurnComplete` 或 `TurnAborted`） |

### Item 层（`items.rs`）

| 类型 | 说明 |
|------|------|
| `TurnItem` | 17 变体 tagged enum：UserMessage / AgentMessage / Plan / Reasoning / CommandExecution / DynamicToolCall / McpToolCall / CollabAgentToolCall / SubAgentActivity / WebSearch / ImageView / ImageGeneration / FileChange / ContextCompaction / EnteredReviewMode / ExitedReviewMode / Extension / HookPrompt |
| `TextItem` | 文本类 Item：`id` + `content` |
| `ToolItem` | 工具类 Item：`id` + `name` + `arguments` + `output` + `media` + `status` |
| `ExtensionItem` | 扩展 Item：`id` + `namespace` + `payload` |
| `ToolStatus` | 工具状态枚举：InProgress / Completed / Failed |
| `TurnItem::id()` | 统一获取任意 Item 的 ID |

### 提交层（`submission.rs`）

| 类型 | 说明 |
|------|------|
| `Op` | 16 变体操作枚举：TurnInput / Interrupt / ThreadSettings / ExecApproval / PatchApproval / UserInputAnswer / RequestPermissionsResponse / DynamicToolResponse / RefreshMcpServers / ReloadUserConfig / Compact / ThreadRollback / Review / InterAgentCommunication / EmitExtension / Shutdown |
| `Submission` | 操作包装：`id: String` + `op: Op` |
| `TurnInput` | 用户输入：`content` + `image_data_urls` + 可选 `client_message_id` |
| `TurnInputRequest` | 输入请求：`input: Vec<TurnInput>`（支持批量） |
| `TurnInputMode` | 输入模式：StartOrSteer / StartIfIdle / Steer（含 expected_turn_id） |
| `TurnInputSubmission` | 输入结果：Started / Steered / NotSubmitted |
| `TurnInputError` | 输入错误：QueueClosed / ReplyClosed / Invalid |

## 与其他 crate 的关系

```
agent-protocol (本 crate)
  ├── 依赖 types (agent-types) → MediaAsset 用于 ToolItem.media
  ├── 被 agent-core 使用 → 运行时事件广播
  ├── 被 agent-rollout 使用 → 事件持久化策略
  ├── 被 agent-server 使用 → gRPC 事件转换
  └── 被 apps/desktop 前端对接 → 事件 JSON 解析
```

- **agent-types**：唯一依赖，提供 `MediaAsset` 类型
- **agent-core**：运行时产生 `EventMsg` 事件，通过 event_bus 广播
- **agent-rollout**：`policy.rs` 中 `should_persist_event_msg()` 按 `EventMsg` 变体决定持久化
- **agent-server**：将 `EventMsg` 转换为 proto `ThreadEvent` 推送给客户端

## 测试运行

```bash
# 运行全部单元测试
cargo test -p agent-protocol

# 运行特定测试
cargo test -p agent-protocol item_completed_roundtrips
cargo test -p agent-protocol only_complete_and_aborted_are_terminal
```

> 单元测试位于 `src/event.rs`（5 个测试），覆盖 JSON 往返序列化、终态判断与遗留变体拒绝。
