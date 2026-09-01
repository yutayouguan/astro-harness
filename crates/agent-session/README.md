# session

会话查询投影层：单库 `SessionStore`（SQLite WAL，schema v21）管理 sessions、messages、FTS5 全文检索与上下文召回。Agent 的 canonical Responses 历史和恢复事实源位于 rollout；本 crate 不拥有工具调用协议真相，也与精炼记忆（`MEMORY.md`）无关。

## 核心职责

- **消息 CRUD** -- 追加、查询、更新消息（含压缩视图 `compressed_content`、reasoning details 回写）
- **会话生命周期** -- 创建/获取/重命名/归档/删除会话，列表与筛选，账单累加与快照
- **FTS5 全文检索** -- 自动维护 FTS 索引，支持按关键词搜索历史消息（`SearchHit`）
- **上下文召回** -- `build_conversation_context()` 在 turn >= recent_turns 时触发 FTS 召回，返回 `ScrolledMessage` 序列
- **严格 Schema** -- 新库直接创建 v21；已有数据库必须与当前版本完全一致
- **抽象接口** -- `ConversationStore` trait 允许替换后端（Postgres / 远程 API / 内存 mock）
- **Rollout 投影** -- `rebuild_messages_from_rollout()` 从 rollout 数据重建消息序列

## 模块结构

| 文件 | 职责 |
|------|------|
| `lib.rs` | 公共 re-exports |
| `store/mod.rs` | `SessionStore` 主结构体、`BillingDelta` / `SessionBillingRow` / `NewMessage` / `StoredMessage` / `StoredSession` 等核心 DTO |
| `store/schema.rs` | 当前 schema 初始化与版本校验、DDL、索引、FTS5 虚表 |
| `store/messages.rs` | 消息 CRUD impl：append / get / update / patch / delete，FTS 同步 |
| `store/sessions.rs` | 会话 CRUD impl：create / get / list / rename / archive / billing |
| `store/search.rs` | FTS5 搜索实现：`search_messages()` / `SearchHit` / 相关性排序 |
| `store/rollout_projection.rs` | Rollout 投影：从备份/快照重建消息历史 |
| `message_db.rs` | `build_conversation_context()` -- 上下文窗口管理、FTS 召回、`ScrolledMessage` |
| `format.rs` | `format_recalled_context()` -- 召回结果格式化为 LLM 可消费文本 |
| `tools.rs` | `dispatch_session_tool()` / `record_message()` -- 工具层适配 |
| `traits.rs` | `ConversationStore` trait -- Agent 运行时与存储的抽象接口（Send，不要求 Sync） |

## 核心类型与 API

- `SessionStore` -- SQLite WAL 会话存储，实现 `ConversationStore`，数据路径由调用方传入
- `ConversationStore` trait -- 抽象接口：`append_message()` / `get_messages()` / `update_message_compressed_content()` / `create_session()` / `search_messages()` 等
- `NewMessage` -- 写入消息 DTO：session_id / role / content / tool_call_id / tool_calls / model 等
- `StoredMessage` -- 读取消息 DTO：含 id / timestamp / compressed_content
- `BillingDelta` -- 单次 LLM 调用的账单增量（token 计数 + 成本估算）
- `SearchHit` -- FTS 搜索命中：message_id / snippet / rank
- `ScrolledMessage` -- 上下文召回结果：消息 + 是否被召回标记
- `SCHEMA_VERSION` -- 当前 schema 版本（21）

## 设计要点

- **Non-Send 约束** -- `SessionStore` 内含 rusqlite `Connection`（非 Send），必须在同一线程使用；cron 通过 `spawn_blocking` 封装
- **压缩视图分离** -- `compressed_content` 存 Provider 视图，`content` 永远保留原文，FTS 索引仅基于原文
- **角色顺序校验** -- 相邻消息不得连续出现相同 role（user/user 或 assistant/assistant）
- **投影单向性** -- rollout `ResponseItem` 可投影为 Message 供查询/UI 使用；Message 不得反向替代 canonical Agent history

## Crate 关系

| 方向 | crate |
|------|-------|
| 依赖 | `types`（`Message` / `Role` 等共享类型）、`agent-protocol`、`agent-rollout` |
| 被依赖 | `agent`（核心运行时直接操作 SessionStore）、`server`（gRPC 服务层） |

## 测试

```bash
# 全部测试（含 3 个集成测试文件）
cargo test -p session

# 单个集成测试
cargo test -p session --test session_store_test
cargo test -p session --test dispatch_session_tool_test
cargo test -p session --test rollout_projection_test
```
