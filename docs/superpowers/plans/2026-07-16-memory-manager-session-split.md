# MemoryManager / Session Hard Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 硬切拆开 `MemoryManager` 与会话库：精炼记忆留在 `memory`，会话读写 / `session_search` / `format_recalled_context` 归 `session`。

**Architecture:** `AgentLoop` / `ToolContext` 并列持有 `MemoryManager` + `SessionStore`；`dispatch_memory_tool` 只处理 `memory`；`session::dispatch_session_tool` 处理 `session_search`；Tauri/backend 纯会话路径直接 `SessionStore::open_sessions_dir`。

**Tech Stack:** Rust workspace、`session` / `memory` / `agent` / `tools` / `astro-agent` / `backend`、rusqlite、现有集成测试。

**Spec:** `docs/superpowers/specs/2026-07-16-memory-manager-session-split-design.md`

## Global Constraints

- 硬切：不保留 `MemoryManager.session_store` 或会话方法兼容层。
- 不改 `sessions/state.db` schema。
- 不拆 `MemoryStore` / pending / dreaming / review。
- `session_search` 不得再进入 `dispatch_memory_tool`。
- 会话库路径保持 `{memory_dir}/sessions`（`SessionStore::open_sessions_dir`）。
- 每个 Task 结束可独立 `cargo test` / `cargo check` 验证；频繁提交。

## File Structure

| File | Responsibility |
|------|----------------|
| `crates/agent-session/src/format.rs`（新建） | `format_recalled_context`、`format_session_search_hits` |
| `crates/agent-session/src/tools.rs`（新建） | `dispatch_session_tool`、可选 `record_message` helper |
| `crates/agent-session/src/lib.rs` | 导出新模块 API |
| `session/Cargo.toml` | 若需要 `serde_json` 已有；无需新依赖 |
| `session/tests/dispatch_session_tool_test.rs`（新建） | 搜索分发 + format 单测 |
| `crates/agent-memory/src/session/manager.rs` | 删除会话字段/方法；收窄 `dispatch_memory_tool` |
| `crates/agent-memory/src/lib.rs` | 不再导出 `format_recalled_context`（若曾导出） |
| `memory/tests/memory_manager_session_test.rs` | 删除或改为纯记忆测；会话测迁 `session` |
| `tools/Cargo.toml` | 增加 `session` |
| `crates/agent-tools/src/engine/context.rs` | `sessions: &SessionStore` |
| `crates/agent-tools/src/engine/dispatch.rs` | `session_search` 分流 |
| `crates/agent-tools/src/builtin/memory/memory_tools.rs` | 仅 `memory`；搜索另调 session |
| `crates/agent-core/src/loop_.rs` | `sessions` 字段 + 召回/落盘改路径 |
| `crates/agent-core/src/streaming.rs` | `ToolContext` 构造补 `sessions` |
| `apps/desktop/src-tauri/src/commands.rs` 等 | 纯会话改 `SessionStore` |
| `apps/desktop/src-tauri/src/compaction_commands.rs` | 同上 |
| `crates/agent-server/src/grpc/astro_service.rs` | 同上 |
| 各 `tools`/`agent` 测试里的 `ToolContext { … }` | 补 `sessions` |

---

### Task 1: session — format + dispatch_session_tool

**Files:**
- Create: `crates/agent-session/src/format.rs`
- Create: `crates/agent-session/src/tools.rs`
- Create: `session/tests/dispatch_session_tool_test.rs`
- Modify: `crates/agent-session/src/lib.rs`

**Interfaces:**
- Consumes: `SessionStore::search_messages`, `SessionStore::ensure_session`, `SessionStore::append_message`, `ScrolledMessage`, `SearchHit`, `NewMessage`
- Produces:
  - `pub fn format_recalled_context(messages: &[ScrolledMessage]) -> String`
  - `pub fn dispatch_session_tool(store: &SessionStore, name: &str, args: &serde_json::Value) -> anyhow::Result<String>`
  - `pub fn record_message(store: &SessionStore, session_id: &str, role: &str, content: &str) -> anyhow::Result<i64>`（ensure `"tauri"` + append）

- [x] **Step 1: Write failing tests**

Create `session/tests/dispatch_session_tool_test.rs`:

```rust
use session::{
    dispatch_session_tool, format_recalled_context, record_message, NewMessage, ScrolledMessage,
    SessionStore,
};
use serde_json::json;
use tempfile::TempDir;

#[test]
fn format_recalled_marks_anchors() {
    let msgs = vec![
        ScrolledMessage {
            id: 1,
            role: "user".into(),
            content: "hi".into(),
            is_anchor: false,
        },
        ScrolledMessage {
            id: 2,
            role: "assistant".into(),
            content: "yo".into(),
            is_anchor: true,
        },
    ];
    let s = format_recalled_context(&msgs);
    assert_eq!(s, "[1] user: hi\n[2] assistant: yo [anchor]");
}

#[test]
fn dispatch_session_search_and_record_message() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open_sessions_dir(&dir.path().join("sessions")).unwrap();
    store.ensure_session("s1", "test").unwrap();
    store
        .append_message(NewMessage {
            content: Some("alpha fact"),
            ..NewMessage::empty("s1", "user")
        })
        .unwrap();

    let out = dispatch_session_tool(
        &store,
        "session_search",
        &json!({"query": "alpha", "limit": 5}),
    )
    .unwrap();
    assert!(out.contains("相关历史消息"));
    assert!(out.contains("alpha"));

    let id = record_message(&store, "s1", "assistant", "reply").unwrap();
    assert!(id > 0);
}
```

若 `ScrolledMessage` 字段名与现实现不一致，以 `crates/agent-session/src/message_db.rs` 为准调整测试结构体初始化。

- [x] **Step 2: Run tests — expect FAIL**

```bash
cargo test -p session --test dispatch_session_tool_test
```

Expected: FAIL（`dispatch_session_tool` / `format_recalled_context` / `record_message` 未定义）

- [x] **Step 3: Implement `crates/agent-session/src/format.rs`**

从 `crates/agent-memory/src/session/manager.rs` 原样迁入 `format_session_search_hits`（`pub(crate)`）与 `format_recalled_context`（`pub`）。`use crate::{SearchHit, ScrolledMessage};`

- [x] **Step 4: Implement `crates/agent-session/src/tools.rs`**

```rust
use anyhow::anyhow;
use serde_json::Value;

use crate::format::{format_session_search_hits, /* not needed for record */};
use crate::{NewMessage, SessionStore};

pub fn record_message(
    store: &SessionStore,
    session_id: &str,
    role: &str,
    content: &str,
) -> anyhow::Result<i64> {
    store.ensure_session(session_id, "tauri")?;
    store.append_message(NewMessage {
        content: Some(content),
        ..NewMessage::empty(session_id, role)
    })
}

pub fn dispatch_session_tool(
    store: &SessionStore,
    name: &str,
    args: &Value,
) -> anyhow::Result<String> {
    match name {
        "session_search" => {
            let query = args["query"]
                .as_str()
                .ok_or_else(|| anyhow!("缺少 query 参数"))?;
            let limit = args["limit"].as_u64().unwrap_or(5).clamp(1, 10) as usize;
            let hits = store.search_messages(query, None, None, limit as i64)?;
            Ok(crate::format::format_session_search_hits(&hits))
        }
        other => anyhow::bail!("未知会话工具: {other}"),
    }
}
```

- [x] **Step 5: Wire `crates/agent-session/src/lib.rs`**

```rust
pub mod format;
pub mod message_db;
pub mod store;
pub mod tools;

pub use format::format_recalled_context;
pub use message_db::{build_conversation_context, ScrolledMessage};
pub use store::{ /* 保持现有 */ };
pub use tools::{dispatch_session_tool, record_message};
```

- [x] **Step 6: Run tests — expect PASS**

```bash
cargo test -p session --test dispatch_session_tool_test
cargo test -p session
```

Expected: PASS

- [x] **Step 7: Commit**

```bash
git add session/src/format.rs session/src/tools.rs session/src/lib.rs session/tests/dispatch_session_tool_test.rs
git commit -m "$(cat <<'EOF'
feat(session): add format_recalled and session_search dispatch

Move recalled-context formatting and session_search tool handling into
the session crate ahead of MemoryManager hard split.
EOF
)"
```

---

### Task 2: memory — strip session from MemoryManager

**Files:**
- Modify: `crates/agent-memory/src/session/manager.rs`
- Modify: `crates/agent-memory/src/lib.rs`（若仍 `pub use format_recalled_context` 则删除）
- Modify: `memory/tests/memory_manager_session_test.rs`（删除文件，或改为只测记忆；会话断言已在 Task 1）
- Modify: `crates/agent-memory/src/session/mod.rs` 文档注释

**Interfaces:**
- Consumes: Task 1 APIs（本 task 的 memory 不再调用）
- Produces: `MemoryManager` 无 `session_store`；`dispatch_memory_tool` 仅 `memory` + 旧名错误

- [x] **Step 1: Delete or rewrite `memory/tests/memory_manager_session_test.rs`**

删除该文件（覆盖已迁至 `session/tests/dispatch_session_tool_test.rs`）。

若保留文件，改为仅断言 `MemoryManager::for_agent` 打开 MEMORY/USER，**禁止**调用 `ensure_session` / `session_store`。

- [x] **Step 2: Edit `MemoryManager` struct and `for_agent`**

删除字段 `pub session_store: SessionStore`。

`for_agent` 中删除：

```rust
session_store: SessionStore::open_sessions_dir(&sessions_dir)?,
```

以及不再需要的 `sessions_dir` 局部变量（若仅用于开库）。

删除 imports：`session::{NewMessage, RecentSession, ScrolledMessage, SearchHit, SessionStore}`、`session::build_conversation_context`（若仅会话用）。

保留 `ensure_workspace` 调用（仍初始化会话库文件）。

- [x] **Step 3: Delete session methods from impl**

删除整个方法：`ensure_session`、`record_message`、`record_message_ex`、`build_session_context`、`list_recent_sessions`、`handle_session_search`。

删除文件底部 `format_session_search_hits`、`format_recalled_context` 函数。

- [x] **Step 4: Narrow `dispatch_memory_tool`**

```rust
pub fn dispatch_memory_tool(
    memory: &mut MemoryManager,
    name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    match name {
        "memory" => { /* 保持现有 */ }
        "memory_add" | "memory_replace" | "memory_remove" => {
            anyhow::bail!("工具已迁移为 memory(action,target)；请使用 action=add|replace|remove")
        }
        _ => anyhow::bail!("未知记忆工具: {name}"),
    }
}
```

更新文档注释：不再声称支持 `session_search`。

- [x] **Step 5: Verify memory compiles in isolation**

```bash
cargo test -p memory
```

Expected: PASS（若其他 crate 尚未改会失败——此时可先 `cargo test -p memory --lib` 与 `cargo test -p memory --tests`；若 workspace 因 agent 引用旧 API 导致 `-p memory` 仍过，因 agent 是下游。优先保证 memory 包自身测试通过。）

若全 workspace 因下游未改而无法只测 memory，用：

```bash
cargo test -p memory --tests
```

- [x] **Step 6: Commit**

```bash
git add memory/src/session/manager.rs memory/src/lib.rs memory/src/session/mod.rs memory/tests/memory_manager_session_test.rs
git commit -m "$(cat <<'EOF'
refactor(memory): remove SessionStore from MemoryManager

Hard-cut session APIs out of MemoryManager; session_search and
format_recalled live in the session crate.
EOF
)"
```

---

### Task 3: tools — ToolContext + dispatch split

**Files:**
- Modify: `tools/Cargo.toml`（加 `session = { path = "../session" }`）
- Modify: `crates/agent-tools/src/engine/context.rs`
- Modify: `crates/agent-tools/src/engine/dispatch.rs`
- Modify: `crates/agent-tools/src/builtin/memory/memory_tools.rs`
- Modify: 所有构造 `ToolContext {` 的测试/内测（见 File Structure 列表）

**Interfaces:**
- Consumes: `session::SessionStore`, `session::dispatch_session_tool`
- Produces: `ToolContext.sessions: &'a SessionStore`

- [x] **Step 1: Add dependency**

在 `tools/Cargo.toml` `[dependencies]` 增加：

```toml
session = { path = "../session" }
```

- [x] **Step 2: Add field to `ToolContext`**

```rust
use session::SessionStore;

pub struct ToolContext<'a> {
    pub memory: &'a mut MemoryManager,
    /// 共享会话库（`{memory_dir}/sessions`）。
    pub sessions: &'a SessionStore,
    // …其余不变
}
```

- [x] **Step 3: Split dispatch**

`crates/agent-tools/src/engine/dispatch.rs`：

```rust
match name {
    "memory" | "memory_add" | "memory_replace" | "memory_remove" => {
        crate::memory_tools::dispatch(ctx, name, args)
    }
    "session_search" => crate::memory_tools::dispatch_session_search(ctx, args),
    // …
}
```

`memory_tools.rs`：

```rust
pub fn dispatch(ctx: &mut ToolContext<'_>, name: &str, args: &serde_json::Value) -> anyhow::Result<String> {
    memory::dispatch_memory_tool(ctx.memory, name, args)
}

pub fn dispatch_session_search(
    ctx: &ToolContext<'_>,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    session::dispatch_session_tool(ctx.sessions, "session_search", args)
}
```

更新模块顶注释：`session_search` 走 session crate。

- [x] **Step 4: Fix every `ToolContext {` site**

模式（在已有 `MemoryManager::new` / `for_agent` 之后，因 `ensure_workspace` 已创建 sessions 目录）：

```rust
let sessions = session::SessionStore::open_sessions_dir(
    &memory.base_dir.join("sessions"), // 或 dir.path().join("sessions")
).unwrap();
let mut ctx = ToolContext {
    memory: &mut memory,
    sessions: &sessions,
    // …
};
```

需改文件至少包括：
- `crates/agent-core/src/loop_.rs`（可与 Task 4 一起做；若本 task 先改 tools 测试）
- `tools/tests/*.rs`、`crates/agent-tools/src/builtin/shell/{terminal,code_exec}.rs`、`image_gen.rs` 内测
- `crates/agent-core/src/streaming.rs`

本 Task 至少让 `cargo test -p tools` 能编过；agent 可留到 Task 4，但若 tools 不依赖 agent，先修 tools 内所有构造点。

- [x] **Step 5: Verify**

```bash
cargo test -p tools
```

Expected: PASS

- [x] **Step 6: Commit**

```bash
git add tools/Cargo.toml tools/src/engine/context.rs tools/src/engine/dispatch.rs tools/src/builtin/memory/memory_tools.rs tools/tests tools/src/builtin/system tools/src/builtin/media/image_gen.rs
git commit -m "$(cat <<'EOF'
refactor(tools): give ToolContext its own SessionStore

Route session_search through session::dispatch_session_tool instead of
memory facade.
EOF
)"
```

---

### Task 4: agent — dual fields on AgentLoop

**Files:**
- Modify: `crates/agent-core/src/loop_.rs`
- Modify: `crates/agent-core/src/streaming.rs`
- Modify: `crates/agent-core/src/usage_record.rs`（若仍经 memory 取 SessionStore——改为 `session::SessionStore`）
- 检查：`crates/agent-core/src/cron_exec.rs`、`delegate_exec.rs`、`orchestration.rs` 是否经 MemoryManager 写会话

**Interfaces:**
- Consumes: `session::{SessionStore, build_conversation_context, format_recalled_context, record_message, NewMessage}`
- Produces: `AgentLoop { memory, sessions, … }`

- [x] **Step 1: Add `sessions` to `AgentLoop`**

在 struct 中与 `memory: MemoryManager` 并列：

```rust
sessions: SessionStore,
```

在 `new` / `for_agent` / 接受 `memory` 的构造路径中：

```rust
let sessions = SessionStore::open_sessions_dir(&config.memory_dir.join("sessions"))?;
```

（`MemoryManager::new` 已 `ensure_workspace`，目录应存在；若构造顺序是先开 sessions，可先 `memory::ensure_workspace(&config.memory_dir)?`。）

- [x] **Step 2: Replace session call sites in `loop_.rs`**

| 旧 | 新 |
|----|----|
| `self.memory.ensure_session(...)` | `self.sessions.ensure_session(...)` |
| `self.memory.record_message_ex(...)` | `self.sessions.append_message(...)` |
| `self.memory.record_message(...)` | `session::record_message(&self.sessions, ...)` |
| `self.memory.build_session_context(...)` | `session::build_conversation_context(&self.sessions, ...)` |
| `memory::format_recalled_context` | `session::format_recalled_context` |
| `memory.session_store.get_messages` | `self.sessions.get_messages` |

`use` 改为：

```rust
use session::{
    build_conversation_context, format_recalled_context, record_message, NewMessage, SessionStore,
};
use memory::MemoryManager;
```

- [x] **Step 3: ToolContext construction in loop_ / streaming**

传入 `sessions: &self.sessions`（注意生命周期：构造 ctx 时 `self.sessions` 与 `self.memory` 同时借用——若 borrow checker 冲突，对 `sessions` 用不可变借用、对 `memory` 可变借用，通常可行）。

- [x] **Step 4: Verify**

```bash
cargo test -p agent
cargo check -p agent
```

Expected: PASS / Finished

- [x] **Step 5: Commit**

```bash
git add agent/src/loop_.rs agent/src/streaming.rs
git commit -m "$(cat <<'EOF'
refactor(agent): hold SessionStore beside MemoryManager

Persist and recall chat history via session crate APIs only.
EOF
)"
```

---

### Task 5: tauri + backend — open SessionStore directly

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands.rs`（所有 `MemoryManager::new` 后只用 session 的块）
- Modify: `apps/desktop/src-tauri/src/compaction_commands.rs`
- Modify: `crates/agent-server/src/grpc/astro_service.rs`（约 Memory 查询旁的 session 路径）
- 确认 `apps/desktop/src-tauri/Cargo.toml` 已有 `session`（先前拆分已加）

**Helper pattern（可内联，不必新文件）：**

```rust
fn open_sessions() -> Result<session::SessionStore, String> {
    let root = home::default_memory_dir();
    let _ = memory::ensure_workspace(&root).map_err(|e| e.to_string())?;
    session::SessionStore::open_sessions_dir(&root.join("sessions")).map_err(|e| e.to_string())
}
```

纯会话命令用 `open_sessions()`；记忆命令继续 `MemoryManager`。

- [x] **Step 1: Migrate `commands.rs` session-only sites**

把 `mgr.session_store` / `mgr.ensure_session` 换成 `store` / `store.ensure_session`。

- [x] **Step 2: Migrate `compaction_commands.rs`**

同上，删除仅为开库而创建的 `MemoryManager`。

- [x] **Step 3: Migrate `astro_service.rs` session query path**

约 1209 行附近：改为 `SessionStore::open_sessions_dir`。

- [x] **Step 4: Verify**

```bash
cargo check -p backend -p astro-agent
```

Expected: Finished

- [x] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/src/compaction_commands.rs backend/src/grpc/astro_service.rs
git commit -m "$(cat <<'EOF'
refactor: open SessionStore directly in tauri/backend session paths

Stop constructing MemoryManager just to reach the chat database.
EOF
)"
```

---

### Task 6: Full verification + acceptance

**Files:** none（或修漏网 `rg` 命中）

- [x] **Step 1: Grep for leftovers**

```bash
rg -n 'session_store|MemoryManager::.*ensure_session|handle_session_search|format_recalled_context|build_session_context|record_message_ex' \
  --glob '*.rs' -g '!.worktrees/**' -g '!target/**'
```

Expected: `format_recalled_context` 仅出现在 `session/` 与调用方 `session::`；`MemoryManager` 无 `session_store`；无 `handle_session_search`。

- [x] **Step 2: Full tests**

```bash
cargo test -p session -p memory -p agent -p tools
cargo check -p backend -p astro-agent
```

Expected: 全部通过

- [x] **Step 3: Final commit if any stray fixes**

```bash
git status -sb
# 若有漏网修复则 commit
```

- [x] **Step 4: Mark spec status**

可选：将 `docs/superpowers/specs/2026-07-16-memory-manager-session-split-design.md` 状态改为「已实现」，并勾验收清单。

---

## Spec coverage (self-review)

| Spec 要求 | Task |
|-----------|------|
| MemoryManager 去 session 字段/方法 | Task 2 |
| format_recalled → session | Task 1 |
| dispatch_session_tool / session_search | Task 1 + 3 |
| record_message helper | Task 1 |
| AgentLoop / ToolContext 双字段 | Task 3–4 |
| Tauri/backend 直连 SessionStore | Task 5 |
| 硬切、schema 不动 | Global + 全程 |
| 验收 grep + tests | Task 6 |

无 TBD / 占位实现步骤。
