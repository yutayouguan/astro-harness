# 会话存储（Session Store）设计

**日期:** 2026-07-13  
**状态:** 已批准 / 已实现（P0）  
**范围:** 将会话持久化升级为单库富消息模型（schema 目标版本 11）  
**外部参考（仅设计借鉴，不引入其品牌命名）：** [NousResearch session storage 文档](https://hermes-agent.nousresearch.com/docs/developer-guide/session-storage) 所描述的 SQLite 会话架构

## 命名约束

- **代码、模块、类型、文件、用户可见文案、路径中不得出现 `hermes` / `Hermes` 字样。**
- 推荐命名：`SessionStore`、`session_store.rs`、`StoredMessage`、`~/.astro/sessions/state.db`。
- 本文档仅在「外部参考」处提及上游项目一次；正文用「参考架构 / 目标 schema」表述。

## 目标

Astro 的会话真相源改为：

- 单库 SQLite（WAL）
- 富 `messages` 行（含 tool / reasoning）
- 带元数据的 `sessions`
- FTS5（含 trigram）
- `schema_version` 迁移

刷新或清掉 localStorage 后，UI 仍能完整回放正文、工具活动与思考内容；Agent 可按 OpenAI conversation 形状从 DB 重建上下文。

## 分期路线图

**原则：** 最终能力集与参考架构对齐；下列能力本期不做完整实现，但 schema / API **必须预留**，后续按同一语义落地，禁止另起一套。

| 阶段 | 能力 | 本期 | 后续行为 |
|------|------|------|----------|
| **P0（本期）** | 单库 `state.db`、富 messages、FTS、UI/Agent 完整恢复 | **做** | — |
| **P1（后续）** | 自动 **compaction + session split** | 只预留 `parent_session_id`、compaction 相关 API 形状 | 压缩旧上下文为持久化摘要、新 session 挂 `parent_session_id`、lineage 查询 |
| **P1（后续）** | **会话计费实算流水** | **列全量对齐**，数值可空；轮次结束可可选回填累计字段 | → **计费实现 spec：** [`2026-07-13-route-aware-usage-pricing-design.md`](./2026-07-13-route-aware-usage-pricing-design.md)（路由定价 + 事件/会话双写；已批准，待实现）。旧 P1a 草案 [已取代](./2026-07-13-session-store-p1a-billing-design.md) |
| **P2（更后）** | **多通道 Gateway 路由** | 不做 | 路由索引（如 `sessions.json`）、source 平台过滤、跨通道 sessionKey 解析 |

本期明确 **不做**：

- JSONL transcript 路线（坚持 SQLite 单库）
- Gateway 路由实现本体（P2）
- Compaction / split 自动触发逻辑（P1，仅预留）
- Billing 实算与对账流水（P1，仅列与可选累计回填）

## 决策：单库富消息，不做「摘要双库」折中

| 现 Astro | 目标 |
|----------|------|
| `sessions/state.db` + `sessions/sessions.db` | **仅** `~/.astro/sessions/state.db` |
| `messages(role, content)` | 富 `messages` 全列（见下） |
| `sessions.db` 存 `summary` + FTS | `sessions` 元数据 + **消息级** FTS |
| `get_chat_history` 只要 user/assistant 文本 | 从 DB 组装完整 UI / conversation |

**`session_search` 工具：** 改为对 `messages_fts` / `messages_fts_trigram` 做跨会话检索（对齐参考架构的 `search_messages`），不再依赖独立 `sessions.db` 摘要表。侧栏「近期会话」改为查 `sessions`（preview 取首条 user content）。

旧 `sessions.db`：启动时一次性迁入新库后标记废弃（文件可保留备份，代码路径不再打开）。

## 库路径与模块

- 路径：`{ASTRO_MEMORY_DIR|~/.astro}/sessions/state.db`（WAL）
- 模块：
  - `memory/src/message_db.rs` → 扩展或演进为 `session_store.rs` 中的 `SessionStore`（sessions + messages + FTS + migrations）
  - `memory/src/session_db.rs`：迁移期保留只读导入；迁完后删除或变为 thin deprecated wrapper
- `MemoryManager` 只持有一个 `SessionStore`

## Schema（目标版本 11）

### `schema_version`

单行整数；声明式缺列 `ALTER TABLE ADD COLUMN`；版本门控负责 FTS 重建等不可声明式变更。空库直接建到最新；旧库逐步 migrate。

### `sessions`

- 身份：`id` PK, `source`, `user_id`, `title`
- 模型：`model`, `model_config`, `system_prompt`
- 谱系：`parent_session_id`（**P1 compaction/split 必需**，本期只建列与 FK/索引）
- 时间：`started_at`, `ended_at`, `end_reason`
- 计数：`message_count`, `tool_call_count`, `api_call_count`
- Token / billing 列（**P1 计费流水必需，本期列齐、值可空**）：`input_tokens`, `output_tokens`, `cache_read_tokens`, `cache_write_tokens`, `reasoning_tokens`, `billing_provider`, `billing_base_url`, `billing_mode`, `estimated_cost_usd`, `actual_cost_usd`, `cost_status`, `cost_source`, `pricing_version`

索引：`source`, `parent_session_id`, `started_at DESC`；`title` 非空唯一。

`source` 取值约定：`tauri` | `cron` | `orchestration` | `test`（日后多通道再扩展，如 messaging 平台 id）。

**不**再增加专用 `summary` 列；预览与搜索一律走 messages / title。

### `messages`

```
id, session_id, role, content,
tool_call_id, tool_calls, tool_name,
timestamp, token_count, finish_reason,
reasoning, reasoning_content, reasoning_details,
codex_reasoning_items, codex_message_items
```

- `tool_calls` / `reasoning_details` / `codex_*`：JSON 文本
- `timestamp`：Unix epoch float（写入用 `SystemTime`）
- `content` 允许 NULL（纯 tool_calls 的 assistant 行）

### FTS

1. `messages_fts`：inline 模式，字段覆盖 `content`、`tool_name`、`tool_calls`
2. `messages_fts_trigram`：trigram tokenizer，服务 CJK / 子串
3. INSERT/UPDATE/DELETE 触发器保持与主表同步；迁移时 drop 旧 external-content FTS 并 backfill

## 写入 API

```text
create_session(id, source, model?, user_id?, parent_session_id?, …)
end_session(id, end_reason)
reopen_session(id)
append_message(session_id, role, content?, tool_calls?, tool_call_id?, tool_name?,
               token_count?, finish_reason?, reasoning?, reasoning_content?,
               reasoning_details?, …) -> id
get_messages(session_id) -> Vec<StoredMessage>
get_messages_as_conversation(session_id) -> Vec<Value>  // OpenAI 形状，含 reasoning 回放字段
search_messages(query, source_filter?, role_filter?) -> Vec<SearchHit>
# P1 预留（本期可 stub / 不调用）：
# split_session_after_compaction(old_id, new_id, summary_message…)
# update_session_billing(id, token deltas, cost fields…)
```

写竞争：短 timeout + 有限次 jitter 重试 + 可选 `BEGIN IMMEDIATE`（单进程 Tauri 可先实现重试骨架）。

### Agent / 流式接线

| 事件 | 落盘 |
|------|------|
| 用户开轮 | `ensure/create_session`；`append_message(role=user, …)` |
| 模型流结束 | `append_message(role=assistant, content, tool_calls?, reasoning?, token_count?, finish_reason?)` |
| 工具结果 | `append_message(role=tool, content=result, tool_call_id, tool_name)` |
| 会话结束 / 取消 | 可选 `end_session` |

运行时 `AgentLoop.session_messages` 仍可作热缓存；权威以 DB 为准。`record_message` 旧三参 API 改为委托 `append_message`（缺省字段 `None`）。

本轮流式需把 **reasoning 文本** 累积到落盘点（今天未写入的缺口一并关闭）。

## 读取与 UI 恢复

### `get_chat_history`

返回富 DTO（字段名可 serde camelCase）：

- `sessionId`
- `messages[]`：已折叠为前端 `ChatMessage` 形状  
  - `user` → 一条气泡  
  - `assistant` → 一条气泡：`content`, `reasoning`, `activities[]`（由本条 `tool_calls` + 后续匹配的 `tool` 行填充 `input`/`output`）, 可选 `usage`

组装规则（与当前直播 UI 一致，tool **不**单独成气泡）：

1. 按 `(timestamp, id)` 扫描  
2. 遇 `assistant` 开新助手消息；解析 `tool_calls` JSON 生成 activity 骨架  
3. 遇 `tool`：按 `tool_call_id`（或顺序）挂到最近助手消息的 activity `output`  
4. 忽略无法展示的扩展角色（若有）

前端 `restoreChatHistory`：优先 localStorage；否则消费上述富 DTO，**删除**「只 map role+content」逻辑。

### 侧栏 `list_recent_sessions`

改为查 `sessions`：`id/title/started_at` + 首条 user `content` 截断为 preview。无 title 时 preview 可作展示名。

### `session_search` 工具

调用 `search_messages`；返回 snippet + 邻接上下文；描述文案改为「搜索历史消息」而非「搜索会话摘要」。

## 迁移

1. 打开 `state.db`，跑到 schema v11  
2. 若存在旧 messages 表（仅 role/content）：`ADD COLUMN` 补齐；旧 FTS 按 v11 重建  
3. 若存在 `sessions.db`：  
   - 每行 `session_id/summary/created_at` → `sessions(id, title=NULL, source='tauri', started_at=解析 created_at, …)`  
   - 推荐：`title` 取 summary 前 80 字，不造假消息  
4. 写入 `state_meta` 键 `migrated_from_sessions_db=1`，避免重复导入  
5. 代码不再 `SessionDb::new(sessions.db)`；文档注明旧文件可手动删除

## 与 Usage Insights 的关系

- **本期：** Insights 仍以 `usage.db` 事件为准；sessions 上 billing/token 列可空，允许轮次结束 **可选回填** 累计字段，便于 P1 对接。  
- **P1（会话计费实算）：** 写入 `estimated_cost_usd` / `actual_cost_usd` / `cost_status` 等；另开 spec 决定 Insights 是继续双写 `usage.db`、还是收敛到 sessions 计费列 + 导出视图——**不允许**再发明第三套计费 schema。

## 测试计划

- memory：空库建表 v11；旧库列迁移；`sessions.db` 导入幂等  
- `append_message` 富字段读写；`get_messages_as_conversation` 含 tool_calls + reasoning  
- history 组装：user → assistant(tool_calls) → tool → assistant(text) → 正确 activities  
- FTS：英文 + 中文 trigram 命中 tool_name/content  
- agent/streaming：一轮带 reasoning + tool 的落盘集成（可用 ScriptedProvider）  
- 前端类型 / restore 路径编译与最小单测（若已有 history 相关测试则扩展）

## 验收标准

1. 清 localStorage 后重进聊天：工具活动条与思考内容仍在  
2. 仅保留一份 `sessions/state.db` 为权威；无代码依赖 `sessions.db`  
3. `session_search` 基于消息 FTS 可用  
4. 旧数据可迁移，不丢会话 id 与历史正文  
5. 仓库内新增代码 / 模块 / 类型名 **不含** `hermes` 字样  

## 实现顺序建议

1. `SessionStore` schema + migrations + 单测  
2. 扩展 `append_message` / Manager / Agent 写入（含 reasoning）  
3. `get_chat_history` + 前端 restore  
4. `list_recent_sessions` + `session_search` 切新 API  
5. 移除 / 废弃 `session_db.rs` 写路径；文档与 workspace bootstrap 更新  
