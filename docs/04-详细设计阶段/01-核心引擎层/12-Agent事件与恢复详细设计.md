# Agent 事件与恢复详细设计

> **Harness 当前基线（2026-08-30）**：Core 产生 `agent-protocol::EventMsg/TurnItem`，Session 在 `event_dispatch` 中序列化状态归约、rollout 持久化和 live 交付。Server listener 投影到 gRPC/Tauri，恢复使用 rollout snapshot + live boundary。Core EventBus、SessionEventHub 及独立转换链仅是已被取代的历史架构。

> 版本：v1.0
> 日期：2026-08-20
> 状态：已实现
> 适用范围：`agent-protocol`、`agent-rollout`、`agent-core`、`agent-server`、
> `agent-session`、`astro-agent`（Tauri）

## 1. 文档地位

本文档是 Astro Thread 事件、持久历史、订阅、恢复和桌面生命周期收敛的权威设计。实现以
`EventMsg` 与 rollout 为单一事实链；gRPC `ThreadEvent` 和 Tauri 本地 UI 事件只能从该事实链
投影，不能反向成为 Core 状态或恢复事实源。

已删除的运行时路径包括：

- `SessionEventHub` 及 `SubscribeSessionEvents`/Tauri session-events bridge；
- Core `EventBus` / `AgentEvent`；
- Core `MultiTurnStreamItem`、`RunFinished`、`Done`；
- `stream_id` / `after_event_id` cursor replay；
- Server `multi_turn_to_chat_event` 独立转换路径。
- gRPC `Chat` streaming RPC、`ChatEvent` protobuf 与 Server Chat adapter。

Provider 自身的 stream chunk 类型不属于 Core 事件系统，继续保留。`Done` 只存在于 desktop
本地 UI 投影，不是 backend 传输或恢复协议的一部分。

## 2. 唯一事件主链

```text
AstroThread::submit(Op)
  → async_channel::bounded(512)
  → Session::submission_loop
  → SessionTask::run_turn
  → Event { id, msg: EventMsg }
  → rollout policy + append
  → Core async_channel::unbounded event queue
  → one Server listener per loaded Thread
  → ThreadHistoryBuilder
  → ConnectionRegistry queues bounded(128)
  → Tauri / exec / external Thread clients
```

### 2.1 提交与执行顺序

每个 `AstroThread` 持有一个 `Arc<Session>` 和一个 `SessionIo`。submission queue 容量固定为
512；`send().await` 在满时施加背压，不丢 Op。长期 `submission_loop` 顺序处理 TurnInput、
Interrupt、approval/control、`EmitExtension` 与 Shutdown。TurnInput 的 reply 只确认 started、
steered 或 not-submitted，不等待 Turn 结束。

`SessionTask` 是 Turn 终态的唯一所有者。每个 `TurnStarted` 必须恰好对应一个
`TurnComplete` 或 `TurnAborted`；Error 只是诊断事件，不能代替终态。

### 2.2 持久化先于实时交付

`Session::send_event` 先规范化 event identity，再在串行 dispatch guard 内按 rollout policy
执行 `RolloutRecorder::record`，最后进入 Core event queue。append 失败会记录告警并继续 live
交付，但该事件不能被声明为已持久化；恢复只认实际 rollout 内容。

Server 对每个加载 Thread 启动一个 listener。listener 先用 `ThreadHistoryBuilder::track` 更新
状态，再把同一映射结果 fan-out；连接之间观察相同顺序。每连接内部队列容量 128，fan-out
使用非阻塞发送。某连接满时只取消该 generation，Session、listener 和其他连接继续运行。

## 3. EventMsg 与客户端投影

稳定生命周期由 `TurnStarted`、`ItemStarted`、`ItemCompleted`、`TurnComplete` 和
`TurnAborted` 表达。消息、reasoning、exec、patch、approval、MCP、Hook、Subagent、usage 和
compaction 都映射到同一 EventMsg/TurnItem 模型。token、reasoning 和 stdout delta 服务于
实时体验，通常不持久化。

Server listener 是 Core → proto 的唯一映射层。Tauri、exec 和外部客户端只接收同一个
`ThreadEvent`。desktop 可在进程内把 terminal 投影成 `ChatStreamEvent::Done` 供现有 React 状态机
消费，但该 UI 类型不进入 gRPC、rollout 或恢复链，也不拥有独立 history、terminal state 或
emitter。

## 4. Snapshot + live 恢复

`TokenCount` 和 `ContextUsage` 都持久化到 rollout。`TokenCount` 是 turn aggregate，包含总 input、未缓存 input、cache read/write、output、reasoning、request count、Provider 原始 total 及报告状态。`ContextUsage` 是 step snapshot，包含 `provider_reported | provider_recomputed | local_estimate` 来源、Provider 明细和本地分层估算。回放时客户端必须展示事件中的来源，不得将未上报字段填充为“Provider 报告 0”。

恢复协议不提供 transient replay cursor：

1. 客户端先建立并读取 `SubscribeThreadEvents` live stream；
2. 对 workspace、active 和 background-pending Thread 调用 `ResumeThread(include_turns=true)`；
3. Server 将“加入订阅集合 + 读取 ThreadHistoryBuilder snapshot”串行放进 listener command；
4. Tauri 先投影所有 snapshot，再越过 live boundary 处理缓冲事件；
5. 后续只消费 live ThreadEvent。

因此不存在 snapshot 已取出但订阅尚未生效的窗口。snapshot 提供 durable completed turns、当前
active turn 和 pending background 集合；token、reasoning、stdout 等 transient delta 不承诺
补发。稳定 item 以 `turn_id + item_id` 去重，最终内容由 snapshot 或后续完成事件收敛。

连接 identity 分为 logical `connection_id` 与 registration generation。旧 generation 的 cleanup
不能删除同 id 新 generation；subscriber 投递按 generation 隔离，background retained sink 则
按 logical id 在 replacement 后解析当前 generation。

## 5. Background Extension 与恢复收敛

终态后的副作用统一提交 `Op::EmitExtension`，生成 `ItemCompleted(Extension)` 并经过 rollout：

| namespace | thread | payload |
| --- | --- | --- |
| `astro.memory` | 原 session thread | `{ source, target, summary, live_written }` |
| `astro.session_metadata` | 原 session thread | `{ title }` |
| `astro.pending` | `astro-workspace-events` | `{ pending_count, reason }` |
| `astro.background_complete` | 原 session thread | `{}` |
| `astro.background_expired` | 原 session thread | `{ turn_id }` |

Tauri 每次连接都 Resume workspace event thread。成功前台终态后，Server 为该 turn 保留
background extension sink，desktop 把 turn 从 active 移到独立 `background_pending`，不会把它
继续算作 UI active turn。memory/title 可在前台 terminal unsubscribe 之后通过 retained logical
sink live 到达；断线期间到达的稳定 extension 则由下一次 Resume snapshot 恢复。

side-effect supervisor 对整个 post-turn 阶段设置有界总超时，并在成功、错误、超时和 panic
收敛后发送 `astro.background_complete`；turn id 由 ThreadEvent envelope 承载。marker 失败或
sink retention 到期时，Server 对正式
subscriber 和 retained logical ids 投递 `astro.background_expired`、稳定 expired item id 与
`{ turn_id }`。release/cancel 路径直接 expire 捕获的旧 listener，不等待 marker，也绝不
`get_or_create` 复活已删除 Session。

`ThreadSnapshot.pending_background_turn_ids` 始终是 Server 当前 sink 的权威集合。Desktop 每次
snapshot 都做 subtractive reconcile：清除本地有而 Server 没有的 turn。集合支持同 Thread
多个 pending turn；协议不再提供旧 backend presence capability 分支。

## 6. Desktop provisional ACK barrier

Tauri `ThreadEventsBridge` 用 activation generation 线性化 SubmitTurn ACK 与可能抢先到达的 live
事件。SubmitTurn 尚在 awaiting 集合时：

- extension 仍按独立 background lifecycle 立即处理；
- terminal 先 defer，不提前清 active；
- 所有其他映射事件按 `(thread_id, turn_id, activation)` FIFO 缓冲，不 emit，也不更新正文、
  reasoning 或 pending error projection。

每个 `(turn_id, activation)` provisional buffer 的上限为 128；同一 activation 下不同 turn key
分别计数。进入同一个 key 的第 129 个事件对全局 pump 施加等待，直到 ACK、replacement、
failure 或 forget 释放容量；不静默丢合法事件。matching ACK 原子取出对应 buffer，先投影 FIFO
事件，再拼接 matching deferred terminal。mismatched ACK 删除该 activation 的其他 turn buffer、
provisional epoch、marker 和 deferred terminal，防止旧 turn 污染新 turn。

ACK-drained batch 还有一层 delivery barrier：bridge 在 bind 返回后立即启动自有 detached worker，
worker 等待 oneshot acceptance。正常 UI 同步 emit 完成后 sender 成功；调用者在发送前 drop 或
task abort 时 receiver error 也会释放 barrier。这样终态不会越过尚未被 UI 接受的 batch，同时
取消也不会永久造成 global head-of-line block。

lifecycle RPC 与新 activation 使用 per-thread terminal gate：forget 在 gate 内清旧代并持锁跨
RPC，activate 只能在 RPC 完成后建立新代。RPC 成功和错误都会释放 gate；旧代 event 在无 active
窗口不进入新 projection。永久 delete 可以清除所有本地恢复目标。

## 7. SQLite projection 与冷启动

`SessionStore::rebuild_messages_from_rollout` 先在事务外完成过滤与角色顺序验证，再在单个 SQLite
事务中：创建缺失目标 session、删除该 session 的 message rows、按 rollout 顺序插入投影、更新
目标计数并 commit。它不会全库删除；验证/插入失败会 rollback，其他 session 永不受影响；重复
rebuild 结果幂等。

投影完整保留 user/assistant/tool、content、compressed content、tool_call_id、assistant
tool_calls、reasoning 和 reasoning details。tool name 使用 per-id FIFO 队列按 rollout 单遍归属，
重复 id 的 A/tool/B/tool 仍分别匹配 A/B。Message Parts 中 image/audio/video URL 转成
`MediaAsset`，与 `message.media` 合并并按稳定 identity 去重；data URL 保留 MIME，HTTP(S)
使用 `RemoteUri`，无法表达的 URL 显式报错。

冷启动 `hydrate_history` 从 `media_json` 恢复完整 `Vec<MediaAsset>`。只有 Image + DataUrl 同时
构造 legacy image content parts；Audio/Video 的 kind、reference、MIME、label 和 id 保持原值，
不会被 `user_with_images` 降格。

## 8. 故障与清理不变量

- slow connection 只移除自身 generation；同 id replacement 不受 stale cleanup 影响；
- `WaitForExtension` 使用 opaque waiter id，submission/await 取消时显式撤销，不泄漏 waiter；
- active、background pending、dedupe、provisional buffer 和 lifecycle ownership 都随
  replacement/failure/forget/clear 清理；
- release 绑定原 `ManagedThread`/Session，cancel observer 后快速返回，不等待 30 秒 marker；
- 正常 success/error/总超时仍尝试 marker，失败后有界 expire；
- 空闲卸载只在无 active、无 subscriber、无 background sink 时开始计时。

## 9. 验证门

本设计由以下 focused suites 锁定：

```bash
cargo fmt --all -- --check
cargo test -p agent-protocol
cargo test -p agent-rollout
cargo test -p agent --test thread_event_lifecycle_test
cargo test -p agent --test streaming_test
cargo test -p server --test thread_events_test
cargo test -p session --test rollout_projection_test
```

另以 workspace all-target tests/clippy、desktop TypeScript/build、legacy deletion rg 和 EventMsg
coverage rg 做最终回归。2026-08-20 focused suites 全部通过，事件与恢复文档及本轮事件对齐设计
据此标记为已实现；生命周期总文档仍保持“部分实现”。workspace all-target tests（1478 passed，
2 ignored）、TypeScript 和生产 build 也通过。

全 workspace clippy 的既有 baseline 仍未清零：`agent-types` 的
`items_after_test_module`、`agent-skills` 的 `unnecessary_sort_by`、`agent-providers` tests 的
81 个 `unwrap_used`（首个 `src/compat/messages.rs:148`），以及 `agent-workflow` tests 的 36 个
`unwrap_used`（首个 `src/engine/dag.rs:265`）。这些告警来自本批未修改的 Rust 文件，故不得把
`cargo clippy --workspace --all-targets -- -D warnings` 记为通过。legacy deletion rg 为 0 命中，
EventMsg coverage rg 为 217 命中，`git diff --check` 通过。

## 10. 相关设计

- [Agent 生命周期详细设计](07-Agent生命周期详细设计.md)
- [Agent Loop Codex 架构对齐设计](../../superpowers/specs/2026-08-18-agent-loop-codex-alignment-design.md)
