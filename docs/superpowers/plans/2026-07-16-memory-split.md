# Memory 分步拆分计划

> **Goal:** 把臃肿的 `memory` crate 按域拆开；每步可编译、可测、`memory` 继续作 facade。  
> **Architecture:** `home`（无 SQLite）为叶子；有库域逐步独立；调用方短期仍 `use memory::…`。  
> **Tech Stack:** Rust workspace、rusqlite、现有集成测试不动语义。

---

## 总路线

| 步 | Crate | 迁入内容 | 留在 memory |
|----|-------|----------|-------------|
| **1 ✅** | `session` | `SessionStore` + `message_db` | `MemoryManager`、记忆等 |
| **2 ✅** | `usage` | usage db/pricing/stats/trace | — |
| **3 ✅** | `orchestration` | orchestration db/spawn/collab | 执行仍在 `agent` |
| **4 ✅** | `cron` | jobs.json + cron.db + tick/dispatch | 执行仍 backend/agent |
| **5 ✅** | `artifacts` + `delegate` | artifacts.db；委派类型/异步登记/worktree | store/pending/review/dreaming/config + workspace lifecycle + MemoryManager |

---

## 文件地图（Step 1）

| 操作 | 路径 |
|------|------|
| Create | `session/Cargo.toml`、`session/src/lib.rs` |
| Move | `memory/src/session/store/**` → `session/src/store/` |
| Move | `memory/src/session/message_db.rs` → `session/src/message_db.rs` |
| Move（可选） | `memory/tests/session_store_test.rs` → `session/tests/` |
| Keep | `memory/src/session/manager.rs` |
| Modify | `memory/src/session/mod.rs`：re-export + manager |
| Modify | 根 `Cargo.toml`、`memory/Cargo.toml` |
| Stub | `memory` 根级 `pub use session::…` 保持符号 |

`SessionStore::open` 已接受 `Path`，**不必**依赖 `home`。

---

### Task 1: 新建 `session` crate 并迁入 store + message_db

**Files:**
- Create: `session/Cargo.toml`
- Create: `session/src/lib.rs`
- Move: store + message_db
- Modify: store 内 `crate::message_db` → `crate::message_db`（同 crate 即可）

**依赖：** `anyhow`、`rusqlite`（bundled）、`serde`、`serde_json`；dev：`tempfile`。

**Verify：** `cargo test -p session`

---

### Task 2: `memory` 接 facade

**Files:**
- `memory/Cargo.toml`：`session = { path = "../session" }`
- `memory/src/session/mod.rs`：`pub use session::store as store;` 等 + `mod manager`
- `memory/src/lib.rs`：继续 `pub use session_store` / `message_db`（指向本包或 session）
- manager 的 `use crate::session_store` 保持有效

**Verify：** `cargo test -p memory --tests`；`cargo check -p agent -p tools -p backend -p astro-agent`

---

### Task 3: 提交

`refactor: extract session crate from memory`

---

## 本轮不做

- 不动 `MemoryManager`
- 不改调用方 import 路径
- 不拆 usage / orchestration
