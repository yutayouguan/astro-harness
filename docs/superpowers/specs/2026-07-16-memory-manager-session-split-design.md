# MemoryManager / Session 硬切拆分设计

**日期:** 2026-07-16  
**状态:** 已定稿（待实现计划）  
**前置:** memory 域拆分完成（`home` / `session` / `usage` / …）；本设计只解耦仍绑在 `MemoryManager` 上的会话职责。

## 目标与成功标准

- **边界清晰：** `MemoryManager` 不再持有或转发会话 API；调用方直接使用 `session::SessionStore`。
- **依赖可瘦：** 纯会话路径（Tauri 会话命令、compaction、backend 会话查询等）代码上经 `SessionStore` 打开库，不必再 `MemoryManager::new` 仅为拿到 `session_store`。
- **硬切：** 一次改完 agent / tools / tauri / backend；不保留兼容 facade 或过渡双路径。
- **工具归属：** `session_search` 迁到 `session` 侧分发，不再经 `dispatch_memory_tool`。

非目标：不改 `sessions/state.db` schema；不拆 `MemoryStore` / pending / dreaming / review；不强制本轮从所有 crate 的 `Cargo.toml` 删除 `memory` 依赖（仅要求调用路径正确；能删则删）。

## 架构

```
home          路径 / Agent 脚手架
session       SessionStore + message_db
              + format_recalled_context
              + dispatch_session_tool (session_search)
              + 可选 record_message 薄 helper
memory        MemoryManager（MEMORY / USER / 日记 / prompt / memory 工具）
              + ensure_workspace（仍编排 SessionStore 初始化 + skills）
agent         AgentLoop { memory: MemoryManager, sessions: SessionStore, … }
tools         ToolContext { memory, sessions, … }；按工具名分流
tauri/backend 会话 → SessionStore；精炼记忆 → MemoryManager
```

依赖方向：`memory` → `session`（仅 bootstrap）；`agent` / `tools` / `tauri` / `backend` 对会话直连 `session`。

## 组件与 API

### MemoryManager（`memory`）

**保留**

- 构造：`new` / `for_agent`（确保工作区与 Agent 空间；**不再** `SessionStore::open`）
- 字段：`base_dir`、`agent_id`、`workspace_dir`、`memory`、`user`、`config`
- 日记：`today_daily_path`、`daily_content_today`、`append_daily`
- Prompt：`prompt_content`、`prompt_content_with_daily`、`refresh_memory_snapshot`、`reload_memory_config`
- 工具：`handle_memory_op` / `handle_memory_op_with_source`、`dispatch_memory_tool`（仅 `memory` 与旧名迁移错误）

**删除**

- 字段：`session_store`
- 方法：`ensure_session`、`record_message`、`record_message_ex`、`build_session_context`、`list_recent_sessions`、`handle_session_search`
- 模块错位：`format_recalled_context`、会话搜索格式化函数从 `memory` 根导出中移除

构造语义：`ensure_workspace` 仍会创建/打开会话库文件；`MemoryManager` 自身不持有连接。

### session crate

**迁入 / 新增**

- `format_recalled_context(messages: &[ScrolledMessage]) -> String`（从 `memory` manager 迁入）
- `format_session_search_hits`（私有或 `pub(crate)`，供分发使用）
- `dispatch_session_tool(store: &SessionStore, name: &str, args: &Value) -> Result<String>`  
  - 处理 `session_search`（query / limit 钳制与今日一致）  
  - 未知工具名报错
- 可选：`record_message(store, session_id, role, content)` = `ensure_session(..., "tauri")` + `append_message`，供 agent 少写样板

**已有、继续使用**

- `SessionStore::open_sessions_dir`、`ensure_session`、`append_message`、`search_messages`、`build_conversation_context`、`list_recent_sessions` 等

### AgentLoop / ToolContext

- 并列字段：`memory: MemoryManager` + `sessions: SessionStore`
- 打开：`SessionStore::open_sessions_dir(config.memory_dir.join("sessions"))`（与今日路径一致）
- 回合：会话 ensure / 落盘 / 召回全部走 `sessions` + `session::build_conversation_context` + `format_recalled_context`
- Prompt 组装仍读 `memory`
- `ToolContext`：`memory: &mut MemoryManager`、`sessions: &SessionStore`（搜索只读即可；若未来需要写会话再放宽）
- 分发：`session_search` → `session::dispatch_session_tool`；`memory` → `memory::dispatch_memory_tool`

### Tauri / backend

- 凡今日「`MemoryManager::new` 后只用 `.session_store` / `ensure_session`」的命令，改为直接 `SessionStore::open_sessions_dir`
- 精炼记忆命令（refresh snapshot、pending 等）继续 `MemoryManager`

## 数据流

1. `sessions.ensure_session(id, source)`
2. `sessions.append_message`（或 session helper）
3. 召回：`build_conversation_context(&sessions, …)` → `format_recalled_context`
4. Prompt：`memory.prompt_content*`
5. 工具：`session_search` 仅用 `sessions`；`memory` 仅用 `memory`

错误语义与今日一致（会话失败与记忆失败各自 `anyhow`）；本设计不改变产品行为，只改调用路径。

## 测试与验收

- 迁移 `memory` 内依赖会话 API 的单测（如 `memory_manager_session_test`）到 `session` 或改为只测记忆面
- `cargo test -p session -p memory -p agent -p tools`
- `cargo check -p backend -p astro-agent`
- 验收清单：
  - [ ] `MemoryManager` 无 `session_store` 字段与会话方法
  - [ ] `session_search` 不经过 `dispatch_memory_tool`
  - [ ] 纯会话 Tauri/backend 路径不经 `MemoryManager` 开库
  - [ ] `format_recalled_context` 仅从 `session` 导出

## 实现顺序（建议）

1. `session`：迁入格式化与 `dispatch_session_tool`（+ 可选 record helper）；单测  
2. `memory`：删会话字段/方法与根导出；收窄 `dispatch_memory_tool`  
3. `agent` / `tools`：双字段 + 分发分流  
4. `tauri` / `backend`：会话命令直连 `SessionStore`  
5. 全量 check / test，提交
