# session

`SessionStore` 是 Agent 会话的 SQLite 索引与查询层。schema v24 直接保存
`agent_protocol::ResponseItem` JSON；它不再保存或返回第二套泛化消息模型。append-only rollout
仍是可恢复事件的事实源，SQLite 可由 rollout 重建。

## 核心职责

- **原生 item 持久化** -- `response_items.item_json` 无损保存 message、reasoning、call 和 output item。
- **会话生命周期** -- 创建、重命名、归档、分支、截断、压缩和账单累加。
- **FTS5 检索** -- 从原生 item 提取 `role` / `search_text` / `tool_name` 作为可重建索引列。
- **上下文召回** -- 返回带原生 `ResponseItem` 的检索结果，不生成 Chat DTO。
- **Rollout 重建** -- `rebuild_response_items_from_rollout()` 按原顺序重建 SQLite 索引。

## Schema v24

`response_items` 的权威内容是 `item_json` 中的 `ResponseItem`。`role`、`search_text`和
`tool_name` 只是查询索引，不参与恢复原生 item。旧 `messages` schema 不做兼容迁移：
v22 → v23 只增量添加线程检查点表；v23 → v24 增量添加 `thread_attachments`，两条升级路径都保留会话、账单、item 和 FTS。更早的旧 messages 库仍沿用重建策略，后续可从 rollout 回填。

`thread_context` 保存线程私有 notes/revision、回退后的 notes_stale、绑定 turn 的压缩请求状态。notes 使用 CAS 原子替换；不复制到 fork，也不混入长期记忆。该表是工作状态，不是可从 rollout 重建的对话查询投影。

`thread_attachments` 保存与线程独立关联的有界 JSON 状态，使用
`(session_id, attachment_type, identity_key)` 保证幂等；支持 keyset 分页和事务删除。
它不替代 `agent-artifacts` 的文件索引，也不隐式删除磁盘文件。

`NewResponseItem` 与 `StoredResponseItem` 都持有原生 item；工具调用和输出不合并到
assistant/tool message，call id、namespace、encrypted arguments、reasoning 和 metadata 均保持原样。
`tool_name` 索引列可使用 `namespace.name` 便于 FTS/用量归类，但 `item_json` 仍是唯一权威内容。

## 模块

| 文件 | 职责 |
| --- | --- |
| `store/mod.rs` | `SessionStore`、`NewResponseItem`、`StoredResponseItem`、会话与账单 DTO |
| `store/schema.rs` | v24 DDL、v22/v23 增量迁移、旧库重建、索引和 FTS5 |
| `store/thread_context.rs` | 线程检查点 CAS 与 turn-scoped 压缩请求 |
| `store/thread_attachments.rs` | 线程附件的幂等 CRUD、边界校验与分页 |
| `store/messages.rs` | 原生 item 追加、查询、分支、截断与压缩 |
| `store/rollout_projection.rs` | rollout → `response_items` 幂等重建 |
| `store/search.rs` | FTS5 搜索与召回 |
| `traits.rs` | 以 `ResponseItem` 为边界的 `ConversationStore` |

## 不变量

1. 读取依据 `item_json`，不从索引列反向组装 item。
2. 压缩视图写在 item metadata，原文不被覆盖。
3. call/output 是独立 item，分支、截断和压缩必须保持边界。
4. Desktop RPC 直接返回 `StoredResponseItem`；气泡只在 React 渲染边界临时派生。

## 验证

```bash
cargo test -p session
cargo test -p session --test rollout_projection_test
```
