# agent-protocol

Agent runtime 的稳定控制、事件、item 与原生模型历史协议。本 crate 不执行业务逻辑，也不负责传输。

## 协议层

| 模块 | 核心类型 | 职责 |
| --- | --- | --- |
| `submission.rs` | `Op`、`Submission`、`TurnInputRequest`、`TurnInputMode` | Thread 的有序控制面 |
| `event.rs` | `Event`、`EventMsg` | runtime 可观察事件和终态 |
| `items.rs` | `TurnItem` | UI/客户端稳定完成项 |
| `response_item.rs` | `ResponseItem`、`ContentItem` | Agent 与 Responses API 的 canonical history |
| `control.rs` | approval、settings、review、协作 DTO | typed 控制请求与响应 |
| `thread_attachment.rs` | `ThreadAttachment`、分页与 mutation DTO | 线程级有界 JSON 状态 |

## `ResponseItem`

`ResponseItem` 直接表达 message、reasoning、local shell、function/custom/tool-search call 及 output。它保留 item id、call id、namespace、phase 和内部 metadata，使 Agent history 可以跨 sampling、rollout 与进程重启保持协议身份。`qualified_tool_name()` 只为索引/展示派生点号形式，不改写原生字段。

它不是通用 `Message` 的序列化包装。Agent 请求、SQLite 和 rollout 直接使用 `Vec<ResponseItem>`；Desktop 也返回原生 item，UI 只在渲染边界生成 `ConversationEntry`。

工具结果的 host-only metadata 保存在 output item 的
`internal_chat_message_metadata_passthrough.astro_tool_result_metadata_v1`。它有独立字节上限，
由 Provider wire encoder 整体剥离，不得拼入模型可见 `output`。

## `Op`

`AstroThread` 通过一个有界队列接收 `Op`。当前控制面包括：

- turn start/steer/recover/suspend；
- interrupt 和后台 terminal 清理；
- thread settings；
- exec/patch/permission/user-input/dynamic-tool 响应；
- MCP/config refresh；
- compact、review、rollback；
- inter-agent communication、extension 和 shutdown。

`TurnInputSubmission` 只确认录取结果，不表示 turn 已完成。

## `EventMsg` 与 `TurnItem`

`EventMsg` 同时包含 durable state transition 和 transient live telemetry。是否持久化由 `agent-rollout::should_persist_event_msg()` 决定，不由事件消费者猜测。

- durable：completed item、turn start/terminal、committed user input、usage/context、thread settings、rollback；
- transient：delta、approval prompt、MCP begin/end、Hook started/completed、diagnostic error、shutdown notification。

每个 `TurnStarted` 应收敛为一个 `TurnComplete` 或 `TurnAborted`；suspend/recover handoff 是唯一故意不写 terminal 的控制路径。

## Crate 关系

```text
agent-core -> produces Op outcomes, ResponseItem and EventMsg
agent-rollout -> persists canonical ResponseItem and selected EventMsg
agent-server -> projects EventMsg to gRPC/Tauri
agent-session -> builds query/search projections
```

Core 不经 EventBus 广播权威事件。`Session::send_event` 先按 rollout policy 持久化，再进入 live queue。

## 兼容规则

1. 已发布 variant 不原地改变语义；新增字段使用 serde default/alias 完成迁移。
2. call/output identity 不得在 transport 投影中重建。
3. transient event 不可成为恢复所需的唯一状态。
4. UI/protobuf 可以投影协议，但不能反向成为 runtime 事实源。

## 验证

```bash
cargo test -p agent-protocol
```
