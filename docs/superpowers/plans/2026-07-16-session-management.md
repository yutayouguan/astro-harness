# Session Management Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为会话侧栏增加非破坏升级、归档/恢复、重命名和永久删除，并保证当前会话与运行时状态一致。

**Architecture:** SQLite `SessionStore` 继续作为权威数据源；Tauri command 负责跨 gRPC 释放运行时并调用 Store；React 会话列表通过独立 hook 和 `sessions-changed` 事件刷新。自动标题及辅助模型在配套计划 `2026-07-16-auxiliary-models.md` 中实现。

**Tech Stack:** Rust 2021、rusqlite/FTS5、tonic gRPC、Tauri 2、React 18、TypeScript、Node test runner

## Global Constraints

- schema v13 升 v14 必须保留已有会话、消息、FTS 和账单数据。
- 删除为二次确认后的永久删除，不提供回收站或恢复。
- 归档与 `ended_at/end_reason` 正交；归档会话仍可打开和继续对话。
- 删除不修改或删除关联产物文件。
- 只提交本计划相关文件，不包含现有 `tools/src/builtins/media/music_gen.rs` 改动。

---

## File Map

- Modify: `session/src/store/schema.rs` — v14 非破坏迁移。
- Modify: `session/src/store/mod.rs` — 会话列表过滤枚举与归档字段。
- Modify: `session/src/store/sessions.rs` — 标题、归档、删除 API。
- Modify: `session/src/store/search.rs` — active/archived 列表查询。
- Modify: `session/tests/session_store_test.rs` — Store 与迁移回归。
- Modify: `backend/src/grpc/astro_service.rs` — 幂等释放会话运行时。
- Modify: `frontend/src-tauri/src/commands.rs` — 会话 CRUD commands/DTO。
- Modify: `frontend/src-tauri/src/lib.rs` — command 注册。
- Modify: `frontend/src/types.ts` — DTO 类型。
- Create: `frontend/src/lib/chat/sessionManagement.ts` — invoke 封装与刷新事件。
- Create: `frontend/src/lib/chat/sessionManagement.test.ts` — 纯状态逻辑测试。
- Modify: `frontend/src/components/chat/ChatSessionList.tsx` — 页签、菜单与删除确认。
- Modify: `frontend/src/components/chat/ChatRightPanel.tsx` — 当前会话删除回调。
- Modify: `frontend/src/hooks/chat/useChatSession.ts` — 删除当前会话后的本地清理。
- Modify: `frontend/src/App.tsx` — 回调接线。
- Modify: `frontend/src/i18n/messages.ts` — 中英文文案。

---

### Task 1: 非破坏 schema v14 迁移

**Files:**
- Modify: `session/src/store/schema.rs`
- Modify: `session/src/store/mod.rs`
- Test: `session/tests/session_store_test.rs`

**Interfaces:**
- Produces: `SCHEMA_VERSION = 14`
- Produces: `StoredSession.archived_at: Option<f64>`
- Produces: `RecentSession.archived_at: Option<f64>`

- [ ] **Step 1: 将旧的删库测试改成数据保留测试**

在 `session/tests/session_store_test.rs` 将 `outdated_schema_discards_prior_chat_and_billing` 替换为 `v13_schema_migrates_to_v14_without_data_loss`。测试先创建当前库，写入会话、消息和 billing，再把版本改为 13、移除 `archived_at` 列的等价 v13 fixture，重新打开后断言：

```rust
assert_eq!(reopened.schema_version().unwrap(), 14);
assert_eq!(reopened.get_messages("legacy", 10).unwrap().len(), 1);
assert_eq!(
    reopened.get_session_billing("legacy").unwrap().unwrap().input_tokens,
    7
);
assert_eq!(
    reopened.get_session("legacy").unwrap().unwrap().archived_at,
    None
);
```

- [ ] **Step 2: 运行测试并确认失败**

Run: `cargo test -p session --test session_store_test v13_schema_migrates_to_v14_without_data_loss -- --nocapture`  
Expected: FAIL；当前 `open()` 删除旧数据库或 `archived_at` 不存在。

- [ ] **Step 3: 实现 v14 增量迁移**

在 `schema.rs`：

```rust
pub const SCHEMA_VERSION: i32 = 14;

fn column_exists(&self, table: &str, column: &str) -> Result<bool> {
    let sql = format!("PRAGMA table_info({table})");
    let mut stmt = self.conn.prepare(&sql)?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(names.iter().any(|name| name == column))
}
```

把 `archived_at REAL` 加入新库 DDL，并在 `migrate_schema()` 中于 stamp 前执行：

```rust
if self.table_exists("sessions")? && !self.column_exists("sessions", "archived_at")? {
    self.conn
        .execute("ALTER TABLE sessions ADD COLUMN archived_at REAL", [])?;
}
```

在 `SessionStore::open` 删除版本落后时调用 `delete_sqlite_files` 的分支；保留损坏库打开失败的正常报错，不自动清库。更新模块注释。

在 `StoredSession`、`RecentSession` 增加：

```rust
pub archived_at: Option<f64>,
```

并同步 `get_session` SELECT 与 row index。

- [ ] **Step 4: 运行迁移与完整 Store 测试**

Run: `cargo test -p session --test session_store_test -- --nocapture`  
Expected: PASS，且旧的“删库”断言已不存在。

- [ ] **Step 5: Commit**

```bash
git add session/src/store/schema.rs session/src/store/mod.rs \
  session/src/store/sessions.rs session/tests/session_store_test.rs
git commit -m "feat(session): migrate archived state without data loss"
```

---

### Task 2: Store 层归档、列表、标题与永久删除

**Files:**
- Modify: `session/src/store/mod.rs`
- Modify: `session/src/store/sessions.rs`
- Modify: `session/src/store/search.rs`
- Test: `session/tests/session_store_test.rs`

**Interfaces:**
- Produces: `SessionListFilter::{Active, Archived}`
- Produces: `list_sessions(filter, limit)`
- Produces: `archive_session`, `unarchive_session`
- Produces: `set_session_title_if_empty`
- Produces: `delete_session_permanently`
- Produces: `first_turn_text`

- [ ] **Step 1: 写 Store 失败测试**

新增测试：

```rust
#[test]
fn archive_filters_and_restores_session() {
    let (_dir, store) = test_store();
    store.create_session("s1", "tauri", None, None, None).unwrap();
    store.create_session("s2", "tauri", None, None, None).unwrap();
    store.archive_session("s1").unwrap();
    assert_eq!(store.list_sessions(SessionListFilter::Active, 10).unwrap()[0].id, "s2");
    assert_eq!(store.list_sessions(SessionListFilter::Archived, 10).unwrap()[0].id, "s1");
    store.unarchive_session("s1").unwrap();
    assert_eq!(store.list_sessions(SessionListFilter::Archived, 10).unwrap().len(), 0);
}

#[test]
fn title_if_empty_never_overwrites_manual_title() {
    let (_dir, store) = test_store();
    store.create_session("s1", "tauri", None, None, None).unwrap();
    assert!(store.set_session_title_if_empty("s1", "Auto").unwrap());
    store.set_session_title("s1", "Manual").unwrap();
    assert!(!store.set_session_title_if_empty("s1", "Late").unwrap());
    assert_eq!(store.get_session("s1").unwrap().unwrap().title.as_deref(), Some("Manual"));
}

#[test]
fn permanent_delete_removes_messages_and_fts() {
    let (_dir, store) = test_store();
    store.create_session("s1", "tauri", None, None, None).unwrap();
    store.append_message(NewMessage {
        content: Some("unique-delete-token"),
        ..NewMessage::empty("s1", "user")
    }).unwrap();
    store.delete_session_permanently("s1").unwrap();
    assert!(store.get_session("s1").unwrap().is_none());
    assert!(store.search_messages("unique-delete-token", 10).unwrap().is_empty());
}
```

再为 `first_turn_text` 增加 user/tool/assistant 混排测试，期望仅返回首个非空 user 和 assistant 文本。

- [ ] **Step 2: 运行测试并确认缺少 API**

Run: `cargo test -p session --test session_store_test -- --nocapture`  
Expected: compile FAIL，方法与枚举尚不存在。

- [ ] **Step 3: 实现过滤枚举和归档 API**

在 `mod.rs`：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionListFilter {
    Active,
    Archived,
}
```

在 `sessions.rs`：

```rust
pub fn archive_session(&self, id: &str) -> Result<()> {
    let changed = self.conn.execute(
        "UPDATE sessions SET archived_at = ?1 WHERE id = ?2",
        params![now_epoch_secs()?, id],
    )?;
    anyhow::ensure!(changed == 1, "archive_session: session not found");
    Ok(())
}

pub fn unarchive_session(&self, id: &str) -> Result<()> {
    let changed = self.conn.execute(
        "UPDATE sessions SET archived_at = NULL WHERE id = ?1",
        params![id],
    )?;
    anyhow::ensure!(changed == 1, "unarchive_session: session not found");
    Ok(())
}
```

将 `list_recent_sessions` 改为兼容 wrapper，并新增 `list_sessions`。使用两个静态 SQL 分支，避免把列名/条件拼接自外部输入：

```rust
pub fn list_recent_sessions(&self, limit: usize) -> Result<Vec<RecentSession>> {
    self.list_sessions(SessionListFilter::Active, limit)
}
```

active 条件为 `WHERE s.archived_at IS NULL`，archived 条件为 `WHERE s.archived_at IS NOT NULL`，SELECT 末尾加入 `s.archived_at`。

- [ ] **Step 4: 实现条件标题、首轮读取和删除**

```rust
pub fn set_session_title_if_empty(&self, id: &str, title: &str) -> Result<bool> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Ok(false);
    }
    let changed = self.conn.execute(
        "UPDATE sessions SET title = ?1
         WHERE id = ?2 AND (title IS NULL OR TRIM(title) = '')",
        params![trimmed, id],
    )?;
    Ok(changed == 1)
}

pub fn delete_session_permanently(&self, id: &str) -> Result<()> {
    let tx = self.conn.unchecked_transaction()?;
    tx.execute("DELETE FROM messages WHERE session_id = ?1", params![id])?;
    let changed = tx.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
    anyhow::ensure!(changed == 1, "delete_session_permanently: session not found");
    tx.commit()?;
    Ok(())
}
```

`first_turn_text` 用 `ORDER BY timestamp ASC, id ASC` 分别查询首个非空 `role='user'` 与 `role='assistant'` content；任一缺失则返回 `Ok(None)`。

- [ ] **Step 5: 运行完整 session 测试**

Run: `cargo test -p session -- --nocapture`  
Expected: PASS。

- [ ] **Step 6: Commit**

```bash
git add session/src/store/mod.rs session/src/store/sessions.rs \
  session/src/store/search.rs session/tests/session_store_test.rs
git commit -m "feat(session): add archive title and delete operations"
```

---

### Task 3: Backend 幂等释放会话运行时

**Files:**
- Modify: `backend/src/grpc/astro_service.rs`
- Test: existing backend unit test module in `backend/src/grpc/astro_service.rs`

**Interfaces:**
- Produces: chat control action `"release_session"`
- Guarantee: session 不在内存时返回成功。

- [ ] **Step 1: 写释放行为测试**

在现有 service 测试模块增加：创建测试 service，插入可取消的 session runtime，调用 `chat_control` action `release_session` 两次，断言两次均成功且 `sessions.get(session_id)` 为 `None`。

- [ ] **Step 2: 运行测试并确认 action 不支持**

Run: `cargo test -p backend release_session_runtime_is_idempotent -- --nocapture`  
Expected: FAIL，返回 unsupported action。

- [ ] **Step 3: 抽取释放 helper 并接入 chat control**

从 `release_session_for_new_chat` 抽取不含“创建新会话”语义的核心：

```rust
async fn release_session_runtime(&self, session_id: &str) {
    if let Some((_, control)) = self.pause_controls.remove(session_id) {
        control.cancel();
    }
    self.hitl_registry.cancel_and_remove(session_id);
    clear_interrupt_file(session_id);
    if let Some((_, agent)) = self.sessions.remove(session_id) {
        agent.cancel_signal().cancel();
    }
}
```

`new_chat` 继续先执行原 hooks，再调用 helper；新增 `"release_session"` 分支只调用 helper。缺少 runtime 时保持成功。

- [ ] **Step 4: 运行 backend 测试**

Run: `cargo test -p backend release_session_runtime_is_idempotent -- --nocapture`  
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add backend/src/grpc/astro_service.rs
git commit -m "feat(backend): release session runtime idempotently"
```

---

### Task 4: Tauri 会话管理 commands 与 DTO

**Files:**
- Modify: `frontend/src-tauri/src/commands.rs`
- Modify: `frontend/src-tauri/src/lib.rs`
- Modify: `frontend/src/types.ts`

**Interfaces:**
- Produces commands: `list_sessions`, `rename_session`, `archive_session`, `unarchive_session`, `delete_session_permanently`
- Produces `RecentSessionDto.archivedAt: string | null`

- [ ] **Step 1: 扩展 DTO 与列表 command**

Rust DTO 增加：

```rust
pub archived_at: Option<String>,
```

TypeScript 增加：

```typescript
archivedAt?: string | null;
```

新增：

```rust
#[tauri::command]
pub async fn list_sessions(
    filter: String,
    limit: Option<i32>,
) -> Result<Vec<RecentSessionDto>, String>
```

只接受 `"active"` 与 `"archived"`，其它值返回 `invalid session filter`。保留 `list_recent_sessions` 并让它调用 active 查询，避免破坏旧调用方。

- [ ] **Step 2: 新增 rename/archive/unarchive commands**

参数统一使用 snake_case Rust 名；Tauri 前端传 camelCase：

```rust
#[tauri::command]
pub async fn rename_session(session_id: String, title: String) -> Result<(), String>

#[tauri::command]
pub async fn archive_session(session_id: String) -> Result<(), String>

#[tauri::command]
pub async fn unarchive_session(session_id: String) -> Result<(), String>
```

title trim 后为空返回错误；其余直接调用 Store。

- [ ] **Step 3: 新增安全删除编排**

```rust
#[tauri::command]
pub async fn delete_session_permanently(session_id: String) -> Result<(), String> {
    chat_control(session_id.clone(), "release_session".into()).await?;
    open_sessions()?
        .delete_session_permanently(&session_id)
        .map_err(|e| e.to_string())
}
```

运行时释放必须先于 DB 删除；release action 幂等，因此冷会话也能删除。

- [ ] **Step 4: 注册并编译**

在 `generate_handler!` 注册五个新命令。

Run: `cargo check -p astro-agent`  
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add frontend/src-tauri/src/commands.rs frontend/src-tauri/src/lib.rs \
  frontend/src/types.ts
git commit -m "feat(tauri): expose session management commands"
```

---

### Task 5: 前端会话管理状态与当前会话清理

**Files:**
- Create: `frontend/src/lib/chat/sessionManagement.ts`
- Create: `frontend/src/lib/chat/sessionManagement.test.ts`
- Modify: `frontend/src/hooks/chat/useChatSession.ts`
- Modify: `frontend/src/components/chat/ChatRightPanel.tsx`
- Modify: `frontend/src/App.tsx`

**Interfaces:**
- Produces: `SessionListKind = "active" | "archived"`
- Produces: `dispatchSessionsChanged()`, `subscribeSessionsChanged(listener)`
- Produces: `deleteManagedSession(sessionId, activeSessionId, clearActive)`

- [ ] **Step 1: 写纯逻辑失败测试**

```typescript
test("deleting active session clears local chat after backend success", async () => {
  const calls: string[] = [];
  await deleteManagedSession(
    "s1",
    "s1",
    async () => calls.push("invoke"),
    async () => calls.push("clear"),
  );
  assert.deepEqual(calls, ["invoke", "clear"]);
});

test("failed delete does not clear active chat", async () => {
  let cleared = false;
  await assert.rejects(() =>
    deleteManagedSession("s1", "s1", async () => { throw new Error("db"); }, async () => {
      cleared = true;
    }),
  );
  assert.equal(cleared, false);
});
```

- [ ] **Step 2: 运行并确认模块不存在**

Run: `node --import tsx --test frontend/src/lib/chat/sessionManagement.test.ts`  
Expected: FAIL，无法导入模块。

- [ ] **Step 3: 实现 invoke 封装与 DOM 事件**

```typescript
export type SessionListKind = "active" | "archived";
export const SESSIONS_CHANGED_EVENT = "astro:sessions-changed";

export function dispatchSessionsChanged(): void {
  window.dispatchEvent(new CustomEvent(SESSIONS_CHANGED_EVENT));
}

export function subscribeSessionsChanged(listener: () => void): () => void {
  window.addEventListener(SESSIONS_CHANGED_EVENT, listener);
  return () => window.removeEventListener(SESSIONS_CHANGED_EVENT, listener);
}

export async function deleteManagedSession(
  sessionId: string,
  activeSessionId: string | null,
  invokeDelete: () => Promise<void>,
  clearActive: () => Promise<void>,
): Promise<void> {
  await invokeDelete();
  if (sessionId === activeSessionId) await clearActive();
  dispatchSessionsChanged();
}
```

- [ ] **Step 4: 暴露无二次 chat_control 的本地清理方法**

在 `useChatSession` 增加 `clearDeletedCurrentSession`：调用 `clearChatSession()`，清 `sessionId/messages/pendingInterrupts/readOnly`，但不再调用 `chat_control new_chat`，因为 Tauri 删除 command 已释放 runtime。经 `ChatRightPanel` 和 `App` 传给列表。

- [ ] **Step 5: 运行测试与前端构建**

Run: `node --import tsx --test frontend/src/lib/chat/sessionManagement.test.ts`  
Expected: PASS。

Run: `cd frontend && npm run build`  
Expected: PASS。

- [ ] **Step 6: Commit**

```bash
git add frontend/src/lib/chat/sessionManagement.ts \
  frontend/src/lib/chat/sessionManagement.test.ts \
  frontend/src/hooks/chat/useChatSession.ts \
  frontend/src/components/chat/ChatRightPanel.tsx frontend/src/App.tsx
git commit -m "feat(chat): coordinate session deletion state"
```

---

### Task 6: 会话侧栏页签、菜单、确认与文案

**Files:**
- Modify: `frontend/src/components/chat/ChatSessionList.tsx`
- Modify: `frontend/src/i18n/messages.ts`
- Modify: `frontend/src/styles/features/chat/right-panel.css`

**Interfaces:**
- Consumes: Task 4 commands and Task 5 events.
- Produces: active/archived tabs and per-session action menu.

- [ ] **Step 1: 将列表加载改为按页签调用**

增加状态：

```typescript
const [listKind, setListKind] = useState<SessionListKind>("active");
const [menuSessionId, setMenuSessionId] = useState<string | null>(null);
const [busySessionId, setBusySessionId] = useState<string | null>(null);
```

`loadSessions` 调用：

```typescript
invoke<RecentSessionDto[]>("list_sessions", { filter: listKind, limit: 50 })
```

effect 依赖 `listKind`，并订阅 `subscribeSessionsChanged(loadSessions)`。

- [ ] **Step 2: 实现菜单操作**

普通列表菜单：重命名、重新生成标题（先以 disabled + i18n tooltip 标记“辅助模型计划接入”，在配套计划启用）、归档、永久删除。归档列表将“归档”替换为“取消归档”。

rename 使用 `window.prompt`，默认值为当前标题；取消或 trim 后为空时不提交。提交后：

```typescript
await invoke("rename_session", { sessionId: item.sessionId, title });
dispatchSessionsChanged();
```

archive/unarchive 同样成功后 dispatch。

- [ ] **Step 3: 实现永久删除二次确认**

确认文案必须包含标题和“不可恢复”。只有确认后调用 `deleteManagedSession`。busy 时禁用该卡片所有菜单动作。

- [ ] **Step 4: 增加中英文 i18n**

至少新增：`sessions.active`、`sessions.archived`、`sessions.rename`、`sessions.regenerateTitle`、`sessions.archive`、`sessions.unarchive`、`sessions.deletePermanently`、`sessions.deleteConfirm`、`sessions.deleteIrreversible`、`sessions.actionFailed`。

- [ ] **Step 5: 构建与手工冒烟**

Run: `cd frontend && npm run build`  
Expected: PASS。

手工检查：创建 → 重命名 → 归档 → 归档页查看/继续 → 取消归档 → 删除 → 刷新不恢复。确认归档不改变 `ended_at`，删除不影响产物文件。

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/chat/ChatSessionList.tsx \
  frontend/src/i18n/messages.ts frontend/src/styles/features/chat/right-panel.css
git commit -m "feat(chat): add session archive and delete controls"
```

---

### Task 7: 最终回归与文档状态

**Files:**
- Modify: `docs/superpowers/specs/2026-07-16-session-management-auxiliary-models-design.md`

- [ ] **Step 1: 运行相关回归**

```bash
cargo test -p session -- --nocapture
cargo test -p backend -- --nocapture
cargo check -p astro-agent
node --import tsx --test frontend/src/lib/chat/sessionManagement.test.ts
cd frontend && npm run build
```

Expected: 全部 PASS。

- [ ] **Step 2: 更新 spec 实现状态**

将会话管理相关范围标记为已实现，并注明自动标题仍由 `2026-07-16-auxiliary-models.md` 跟进；不得把整个联合 spec 提前标成全部完成。

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/specs/2026-07-16-session-management-auxiliary-models-design.md
git commit -m "docs: record session management implementation"
```
