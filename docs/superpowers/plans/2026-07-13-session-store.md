# Session Store Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将会话持久化升级为单库富消息 `SessionStore`（schema v11），使 UI/Agent 可完整恢复正文、工具活动与 reasoning；命名保持 Astro 中性（禁止 `hermes` 字样）。

**Architecture:** 新建 `memory/src/session_store.rs` 作为权威 SQLite（`~/.astro/sessions/state.db`）：`sessions` + `messages` + FTS + `schema_version`。`MemoryManager` 只持有 `SessionStore`；Agent 流式落盘带 `tool_calls`/`reasoning`；`get_chat_history` 组装成前端 `ChatMessage`（含 `activities`）。旧 `sessions.db` 一次性迁入后废弃写路径。

**Tech Stack:** Rust、rusqlite、serde_json、现有 agent streaming、Tauri commands、React 前端

**Spec:** `docs/superpowers/specs/2026-07-13-session-store-design.md`

---

## File Structure

| File | Responsibility |
|------|----------------|
| Create: `memory/src/session_store.rs` | Schema v11、迁移、`SessionStore` CRUD、FTS、history 组装 |
| Create: `memory/tests/session_store_test.rs` | 建表/迁移/append/search/history 单测 |
| Modify: `memory/src/lib.rs` | 导出 `SessionStore` 等；逐步停用旧 re-export 写路径 |
| Modify: `memory/src/manager.rs` | `session_store` 字段；`record_message` → `append_message`；`handle_session_search` |
| Modify: `memory/src/message_db.rs` | 薄兼容层委托 `SessionStore`，或删除并改全引用 |
| Modify: `memory/src/session_db.rs` | 仅保留迁移只读导入；迁完后删除调用方 |
| Modify: `memory/src/workspace.rs` | bootstrap 只建 `state.db`；触发旧库迁移 |
| Modify: `agent/src/loop_.rs` | 富字段落盘 API；ensure session |
| Modify: `agent/src/streaming.rs` | 累积 reasoning；assistant/tool append 带结构化字段 |
| Modify: `frontend/src-tauri/src/commands.rs` | 富 `ChatHistoryDto`；`list_recent_sessions` 改查 SessionStore |
| Modify: `frontend/src/App.tsx` | restore 消费 `reasoning`/`activities` |
| Modify: `frontend/src/types.ts` | history DTO 类型（若前端单独声明） |
| Modify: `tools/src/builtins/memory_tools.rs` | `session_search` 描述改为搜索历史消息 |
| Modify: `agent/src/cron_exec.rs` | 去掉 `SessionDb::save_session`，改 `create_session`/`set_title` |

---

### Task 1: `SessionStore` 空库 schema v11 + 打开路径

**Files:**
- Create: `memory/src/session_store.rs`
- Create: `memory/tests/session_store_test.rs`
- Modify: `memory/src/lib.rs`
- Modify: `memory/Cargo.toml`（若需 `[[test]]`；默认 `tests/` 即可）

- [x] **Step 1: Write the failing test**

```rust
// memory/tests/session_store_test.rs
use memory::session_store::SessionStore;
use tempfile::TempDir;

#[test]
fn opens_fresh_db_at_schema_v11() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.db");
    let store = SessionStore::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), 11);
    store
        .create_session("s1", "test", None, None, None)
        .unwrap();
    assert!(path.is_file());
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test -p memory --test session_store_test opens_fresh_db_at_schema_v11 -- --nocapture`  
Expected: FAIL（模块/类型不存在）

- [x] **Step 3: Implement minimal `SessionStore::open` + schema**

在 `session_store.rs` 实现：

- `SCHEMA_VERSION: i32 = 11`
- DDL：`schema_version`、`state_meta`、`sessions`（含 billing 列与 `parent_session_id`）、`messages`（富列）、`messages_fts` + `messages_fts_trigram` + 触发器（content/tool_name/tool_calls）
- `open(path)`：`create_dir_all`、WAL、跑 migrations 到 11
- `schema_version()` 读表
- `create_session(id, source, model, user_id, parent_session_id)` 最小插入

禁止任何标识符含 `hermes`。

- [x] **Step 4: Export from `lib.rs`**

```rust
pub mod session_store;
pub use session_store::{SessionStore, StoredMessage, SearchHit, ChatHistoryMessage};
```

（类型可在后续 task 再补全；本步至少导出 `SessionStore`。）

- [x] **Step 5: Run test to verify it passes**

Run: `cargo test -p memory --test session_store_test opens_fresh_db_at_schema_v11 -- --nocapture`  
Expected: PASS

- [x] **Step 6: Commit**

```bash
git add memory/src/session_store.rs memory/src/lib.rs memory/tests/session_store_test.rs
git commit -m "$(cat <<'EOF'
feat(memory): add SessionStore schema v11 skeleton

EOF
)"
```

---

### Task 2: `append_message` / `get_messages` / conversation 重建

**Files:**
- Modify: `memory/src/session_store.rs`
- Modify: `memory/tests/session_store_test.rs`

- [x] **Step 1: Write the failing test**

```rust
#[test]
fn append_and_reload_tool_calls_and_reasoning() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(dir.path().join("state.db")).unwrap();
    store.create_session("s1", "test", None, None, None).unwrap();
    store
        .append_message(memory::session_store::NewMessage {
            session_id: "s1",
            role: "assistant",
            content: Some("done"),
            tool_calls: Some(serde_json::json!([{
                "id": "c1",
                "name": "memory_add",
                "arguments": {"entry": "x"}
            }])),
            tool_call_id: None,
            tool_name: None,
            token_count: Some(12),
            finish_reason: Some("tool_calls"),
            reasoning: Some("think"),
            reasoning_content: None,
            reasoning_details: None,
            codex_reasoning_items: None,
            codex_message_items: None,
        })
        .unwrap();
    store
        .append_message(memory::session_store::NewMessage {
            session_id: "s1",
            role: "tool",
            content: Some("ok"),
            tool_calls: None,
            tool_call_id: Some("c1"),
            tool_name: Some("memory_add"),
            token_count: None,
            finish_reason: None,
            reasoning: None,
            reasoning_content: None,
            reasoning_details: None,
            codex_reasoning_items: None,
            codex_message_items: None,
        })
        .unwrap();

    let msgs = store.get_messages("s1").unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].reasoning.as_deref(), Some("think"));
    assert!(msgs[0].tool_calls.is_some());
    assert_eq!(msgs[1].tool_call_id.as_deref(), Some("c1"));

    let conv = store.get_messages_as_conversation("s1").unwrap();
    assert_eq!(conv[0]["role"], "assistant");
    assert!(conv[0].get("tool_calls").is_some());
    assert_eq!(conv[0]["reasoning"], "think");
    assert_eq!(conv[1]["role"], "tool");
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test -p memory --test session_store_test append_and_reload -- --nocapture`  
Expected: FAIL

- [x] **Step 3: Implement `NewMessage`, `StoredMessage`, append/get APIs**

- `append_message`：插入 messages；递增 sessions.`message_count`（tool 行同时 `tool_call_count++`）
- JSON 字段 `serde_json::to_string`
- `timestamp`：`SystemTime` → f64 epoch seconds
- `get_messages_as_conversation`：OpenAI 形状；assistant 带 `tool_calls`/`reasoning*`

- [x] **Step 4: Run test to verify it passes**

Run: `cargo test -p memory --test session_store_test append_and_reload -- --nocapture`  
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add memory/src/session_store.rs memory/tests/session_store_test.rs
git commit -m "$(cat <<'EOF'
feat(memory): SessionStore append/get with tool and reasoning fields

EOF
)"
```

---

### Task 3: FTS 搜索 + UI history 组装

**Files:**
- Modify: `memory/src/session_store.rs`
- Modify: `memory/tests/session_store_test.rs`

- [x] **Step 1: Write the failing tests**

```rust
#[test]
fn search_messages_hits_tool_name_and_cjk() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(dir.path().join("state.db")).unwrap();
    store.create_session("s1", "tauri", None, None, None).unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "tool",
            content: Some("已写入长期记忆"),
            tool_name: Some("memory_add"),
            tool_call_id: Some("c1"),
            ..NewMessage::empty("s1", "tool")
        })
        .unwrap();
    let hits = store.search_messages("memory_add", None, None, 10).unwrap();
    assert!(!hits.is_empty());
    let hits2 = store.search_messages("长期记忆", None, None, 10).unwrap();
    assert!(!hits2.is_empty());
}

#[test]
fn build_chat_history_folds_tools_into_activities() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(dir.path().join("state.db")).unwrap();
    store.create_session("s1", "tauri", None, None, None).unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "user",
            content: Some("hi"),
            ..NewMessage::empty("s1", "user")
        })
        .unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "assistant",
            content: Some(""),
            reasoning: Some("plan"),
            tool_calls: Some(serde_json::json!([{
                "id": "c1", "name": "memory_add",
                "arguments": {"entry": "e", "target": "project"}
            }])),
            ..NewMessage::empty("s1", "assistant")
        })
        .unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "tool",
            content: Some("ok"),
            tool_call_id: Some("c1"),
            tool_name: Some("memory_add"),
            ..NewMessage::empty("s1", "tool")
        })
        .unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "assistant",
            content: Some("done"),
            ..NewMessage::empty("s1", "assistant")
        })
        .unwrap();

    let ui = store.build_chat_history("s1", 200).unwrap();
    assert_eq!(ui.len(), 3); // user + assistant(with activity) + assistant(text)
    assert_eq!(ui[1].reasoning.as_deref(), Some("plan"));
    assert_eq!(ui[1].activities.len(), 1);
    assert_eq!(ui[1].activities[0].title, "memory_add");
    assert_eq!(ui[1].activities[0].output.as_deref(), Some("ok"));
}
```

（按需实现 `NewMessage::empty` helper。）

- [x] **Step 2: Run tests to verify they fail**

Run: `cargo test -p memory --test session_store_test search_messages_hits -- --nocapture`  
Expected: FAIL

- [x] **Step 3: Implement `search_messages` + `build_chat_history`**

- `SearchHit`：id/session_id/role/snippet/context/…  
- FTS：优先 `messages_fts`；CJK 可回退/并用 `messages_fts_trigram`  
- `ChatHistoryMessage`：`id, role, content, reasoning, activities: Vec<ChatActivityStored { id, kind, title, input, output, status }>`  
- 组装规则见 spec

- [x] **Step 4: Run tests to verify they pass**

Run: `cargo test -p memory --test session_store_test -- --nocapture`  
Expected: 相关测试 PASS

- [x] **Step 5: Commit**

```bash
git add memory/src/session_store.rs memory/tests/session_store_test.rs
git commit -m "$(cat <<'EOF'
feat(memory): SessionStore FTS search and chat history folding

EOF
)"
```

---

### Task 4: 旧库迁移（messages 列 + sessions.db 导入）

**Files:**
- Modify: `memory/src/session_store.rs`
- Modify: `memory/tests/session_store_test.rs`
- Modify: `memory/src/workspace.rs`（bootstrap 调用 migrate）

- [x] **Step 1: Write the failing test**

```rust
#[test]
fn migrates_legacy_messages_and_sessions_db() {
    let dir = TempDir::new().unwrap();
    let sessions_dir = dir.path().join("sessions");
    std::fs::create_dir_all(&sessions_dir).unwrap();

    // 旧 state.db：瘦 messages
    {
        let conn = rusqlite::Connection::open(sessions_dir.join("state.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
             );
             INSERT INTO messages(session_id, role, content) VALUES ('old','user','hello');",
        )
        .unwrap();
    }
    // 旧 sessions.db
    {
        let conn = rusqlite::Connection::open(sessions_dir.join("sessions.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (
                session_id TEXT PRIMARY KEY,
                summary TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             INSERT INTO sessions(session_id, summary) VALUES ('old','hello summary');",
        )
        .unwrap();
    }

    let store = SessionStore::open_with_legacy_migration(&sessions_dir).unwrap();
    assert_eq!(store.schema_version().unwrap(), 11);
    let msgs = store.get_messages("old").unwrap();
    assert_eq!(msgs[0].content.as_deref(), Some("hello"));
    let sess = store.get_session("old").unwrap().unwrap();
    assert!(sess.title.as_deref().unwrap_or("").contains("hello"));
    // 幂等
    let _ = SessionStore::open_with_legacy_migration(&sessions_dir).unwrap();
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test -p memory --test session_store_test migrates_legacy -- --nocapture`  
Expected: FAIL

- [x] **Step 3: Implement migration**

- `open(path)`：对已有 messages 声明式 `ADD COLUMN`；重建 FTS 到 v11  
- `open_with_legacy_migration(sessions_dir)`：打开 `state.db`；若 `state_meta.migrated_from_sessions_db` 未设，从旁路 `sessions.db` 导入 sessions 行（title=summary 截断 80）；写 meta 标记  
- `workspace` bootstrap：`SessionStore::open_with_legacy_migration(base.join("sessions"))`

- [x] **Step 4: Run test to verify it passes**

Run: `cargo test -p memory --test session_store_test migrates_legacy -- --nocapture`  
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add memory/src/session_store.rs memory/tests/session_store_test.rs memory/src/workspace.rs
git commit -m "$(cat <<'EOF'
feat(memory): migrate legacy state.db columns and sessions.db

EOF
)"
```

---

### Task 5: `MemoryManager` 切到 `SessionStore`

**Files:**
- Modify: `memory/src/manager.rs`
- Modify: `memory/src/lib.rs`
- Modify: `memory/src/message_db.rs`（兼容：`MessageDb` 改为 type alias / 包装，或更新所有引用）

- [x] **Step 1: Write/adjust failing compile or unit test**

在 `manager` 相关测试或 `session_store_test` 旁加：

```rust
#[test]
fn memory_manager_record_message_uses_session_store() {
    let dir = TempDir::new().unwrap();
    let mut mgr = memory::MemoryManager::new(dir.path()).unwrap();
    mgr.ensure_session("s1", "test").unwrap();
    let id = mgr
        .record_message_ex(
            "s1",
            memory::session_store::NewMessage {
                session_id: "s1",
                role: "user",
                content: Some("hi"),
                ..memory::session_store::NewMessage::empty("s1", "user")
            },
        )
        .unwrap();
    assert!(id > 0);
}
```

保留旧 `record_message(session, role, content)` 为委托到 `NewMessage` 的薄封装。

- [x] **Step 2: Run to verify fail/compile errors**

Run: `cargo test -p memory memory_manager_record_message -- --nocapture`

- [x] **Step 3: Wire manager**

- 字段：`pub session_store: SessionStore`（可暂时保留 `message_db` 作为 `session_store` 的别名访问器，避免大爆炸；优先一次切完）
- `build_session_context` / `recent_messages` / `latest_session_id` 改走 store
- `handle_session_search` 改用 `search_messages`，Markdown 标题改为「相关历史消息」
- `list_recent` 类 API：`session_store.list_recent_sessions(limit)`（preview SQL）

- [x] **Step 4: Fix all crate compile + tests**

Run: `cargo test -p memory -- --nocapture`  
Expected: PASS（更新任何仍引用旧 API 的测试）

- [x] **Step 5: Commit**

```bash
git add memory/src/manager.rs memory/src/lib.rs memory/src/message_db.rs memory/src/session_db.rs
git commit -m "$(cat <<'EOF'
feat(memory): route MemoryManager through SessionStore

EOF
)"
```

---

### Task 6: Agent 流式落盘（reasoning + tool_calls）

**Files:**
- Modify: `agent/src/loop_.rs`
- Modify: `agent/src/streaming.rs`
- Modify: `agent/tests/streaming_test.rs`（或新建 persistence 断言）

- [x] **Step 1: Write the failing test**

扩展 streaming 测试：挂真实 `MemoryManager` temp dir，跑一轮带 reasoning chunk + tool_call 的 ScriptedProvider，然后：

```rust
let store = SessionStore::open(dir.path().join("sessions/state.db")).unwrap();
let hist = store.build_chat_history(&session_id, 50).unwrap();
assert!(hist.iter().any(|m| m.reasoning.as_deref() == Some(...)));
assert!(hist.iter().any(|m| !m.activities.is_empty()));
```

（按现有 workspace 布局调整路径。）

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test -p agent --test streaming_test <new_test_name> -- --nocapture`  
Expected: FAIL（reasoning/activities 未落盘）

- [x] **Step 3: Implement wiring**

- `run_turn` / 开聊：`ensure_session(session_id, "tauri")`
- `record_assistant_message_with_tools`：改为 `append_message`，传入 `tool_calls` JSON、可选 `reasoning`
- streaming：在流循环累积 `full_reasoning`；在 `notify_completion` 前后把 reasoning 传入 record API
- `record_tool_result_with_id`：写 `tool_call_id` + `tool_name`（从 call 传入）
- 工具执行处把 `call.name` 传入 record

- [x] **Step 4: Run tests**

Run: `cargo test -p agent --test streaming_test -- --nocapture`  
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add agent/src/loop_.rs agent/src/streaming.rs agent/tests/streaming_test.rs
git commit -m "$(cat <<'EOF'
feat(agent): persist reasoning and structured tool calls to SessionStore

EOF
)"
```

---

### Task 7: Tauri `get_chat_history` + 前端 restore

**Files:**
- Modify: `frontend/src-tauri/src/commands.rs`
- Modify: `frontend/src/App.tsx`
- Modify: `frontend/src/types.ts`（如需要）

- [x] **Step 1: Expand DTOs**

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatHistoryActivityDto {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub input: Option<String>,
    pub output: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatHistoryMessageDto {
    pub id: String,
    pub role: String,
    pub content: String,
    pub reasoning: Option<String>,
    pub activities: Option<Vec<ChatHistoryActivityDto>>,
}
```

`get_chat_history`：调用 `mgr.session_store.build_chat_history`；**去掉** `filter user|assistant` 纯文本逻辑（组装已折叠）。

- [x] **Step 2: Frontend restore**

`App.tsx` `restoreChatHistory`：

```ts
const restored: ChatMessage[] = history.messages.map((m) => ({
  id: m.id,
  role: m.role as "user" | "assistant",
  content: m.content ?? "",
  reasoning: m.reasoning ?? undefined,
  activities: m.activities?.map((a) => ({
    id: a.id,
    kind: (a.kind as ChatActivity["kind"]) || "tool",
    title: a.title,
    input: a.input,
    output: a.output,
    status: (a.status as ChatActivity["status"]) || "done",
  })),
}));
```

- [x] **Step 3: Manual/compile check**

Run: `cargo check -p astro-agent`（或 workspace tauri crate 名）  
Run: `cd frontend && npx tsc --noEmit`（若项目惯用）

- [x] **Step 4: Commit**

```bash
git add frontend/src-tauri/src/commands.rs frontend/src/App.tsx frontend/src/types.ts
git commit -m "$(cat <<'EOF'
feat(chat): restore rich history with activities and reasoning

EOF
)"
```

---

### Task 8: `list_recent_sessions` + `session_search` + cron/artifacts 去旧 SessionDb

**Files:**
- Modify: `frontend/src-tauri/src/commands.rs`
- Modify: `frontend/src-tauri/src/artifacts_commands.rs`
- Modify: `agent/src/cron_exec.rs`
- Modify: `tools/src/builtins/memory_tools.rs`
- Modify: `memory/src/session_db.rs`（删除或标 deprecated；无调用后可删）

- [x] **Step 1: Update `list_recent_sessions`**

改用 `MemoryManager` / `SessionStore::list_recent_sessions`：

DTO 可保持 `sessionId` + 用 `preview` 填原 `summary` 字段（避免前端大改），或扩展 `title`/`preview`——优先 **兼容字段**：`summary` ← `title.or(preview)`。

- [x] **Step 2: Update tool description**

`session_search` description → 搜索历史**消息**（非会话摘要）。

- [x] **Step 3: Replace cron `SessionDb::save_session`**

```rust
store.ensure_session(sid, "cron")?;
store.set_session_title(sid, &summary)?; // 或 create + title
```

- [x] **Step 4: Grep cleanup**

Run: `rg 'SessionDb|sessions\\.db|message_db::MessageDb' --glob '*.rs'`  
Expected: 无生产写路径（测试迁移除外）

- [x] **Step 5: Full test**

Run: `cargo test -p memory -p agent -p tools -- --nocapture`  
Expected: PASS

- [x] **Step 6: Commit**

```bash
git add -u memory agent tools frontend/src-tauri
git commit -m "$(cat <<'EOF'
refactor: retire dual sessions.db; route search and sidebar via SessionStore

EOF
)"
```

---

### Task 9: Spec 覆盖自检 + 命名扫描

- [x] **Step 1: Spec coverage checklist**

对照 `docs/superpowers/specs/2026-07-13-session-store-design.md`：

- [x] 单库 state.db  
- [x] 富 messages 列  
- [x] FTS + trigram  
- [x] schema_version  
- [x] UI activities + reasoning 恢复  
- [x] session_search 消息级  
- [x] 旧库迁移  
- [x] P1 列预留（parent_session_id / billing）存在  
- [x] 无 Gateway / compaction 实现（仅预留）

- [x] **Step 2: Naming scan**

Run: `rg -i hermes --glob '!docs/superpowers/specs/**' --glob '!.git/**'`  
Expected: 代码与用户文案无匹配（spec 外部参考句可保留一处）

- [x] **Step 3: Final commit if docs tweaks**

```bash
git add docs/superpowers/specs/2026-07-13-session-store-design.md
git commit -m "$(cat <<'EOF'
docs: mark session store spec approved after P0 implementation

EOF
)"
```

---

## Spec coverage (plan self-review)

| Spec 项 | Task |
|---------|------|
| SessionStore / schema v11 | 1 |
| append/get/conversation | 2 |
| FTS + history fold | 3 |
| legacy migration | 4 |
| MemoryManager | 5 |
| Agent reasoning/tools persist | 6 |
| get_chat_history + frontend | 7 |
| sidebar + session_search + remove sessions.db | 8 |
| naming ban + acceptance | 9 |
| P1/P2 deferred only | 文档分期；Task 1 建列 |

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-07-13-session-store.md`.

**两种执行方式：**

1. **Subagent-Driven（推荐）** — 每任务新开子代理，任务间复审  
2. **Inline Execution** — 本会话按 executing-plans 连续执行并设检查点  

选哪一种？
