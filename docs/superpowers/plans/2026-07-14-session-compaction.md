# Session Compaction（P1b）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在上下文逼近上限（或手动 `/compact`）时结束旧会话、新建子会话，写入摘要 + 近 K 轮气泡，并让前端无感切换 `sessionId` 续聊。

**Architecture:** `SessionStore::end_session` + `compact_and_split` 做原子拆分；Tauri `compact_chat_session` 负责 LLM/启发式摘要后调用拆分；前端 slash/菜单触发与自动阈值（默认 50%）+ 冷却；侧栏靠 `endReason` 标「已压实」。不采用同 `session_id` 原地压实。

**Tech Stack:** Rust（rusqlite / tempfile 单测）、Tauri command、providers `chat_stream`（复用 dreaming 的非流式拼装模式）、React / i18n

**Spec:** [`docs/superpowers/specs/2026-07-14-session-compaction-design.md`](../specs/2026-07-14-session-compaction-design.md)

**执行注意:**
- 代码 / UI / 路径禁止外部参考项目品牌字符串；用 `compact` / `compacted` / `CONTEXT COMPACTION`。
- `main` 上常有未提交前端 WIP；优先在干净 worktree 实现，或只改本计划列出的文件。
- MVP 阈值 / K / 冷却用常量，不强制接 `config.yaml`。

---

## File Structure

| File | Responsibility |
|------|----------------|
| Modify: `memory/src/session/store/sessions.rs` | `end_session`、`assert_session_writable` |
| Modify: `memory/src/session/store/messages.rs` | `start_inclusive_for_tail_bubbles`、`compact_and_split`；`append_message` 写保护 |
| Modify: `memory/src/session/store/search.rs` | `list_recent_sessions` 选出 `ended_at` / `end_reason` |
| Modify: `memory/src/session/store/mod.rs` | `RecentSession` 增加结束字段 |
| Modify: `memory/tests/session_store_test.rs` | end / compact / writable 单测 |
| Create: `frontend/src-tauri/src/compaction_commands.rs` | 摘要（LLM + 启发式）+ `compact_chat_session` |
| Modify: `frontend/src-tauri/src/lib.rs` | `mod` + register command |
| Modify: `frontend/src-tauri/src/commands.rs` | `RecentSessionDto` 加 `endReason`（若放在本文件） |
| Modify: `frontend/src/types.ts` | DTO 类型 |
| Modify: `frontend/src/lib/composerCommands.ts` | `/compact` slash |
| Modify: `frontend/src/App.tsx` | compact handler、自动阈值、切会话 |
| Modify: `frontend/src/components/ChatSessionList.tsx` | 「已压实」徽章 |
| Modify: `frontend/src/i18n/messages.ts` | 文案 |
| Modify: `docs/superpowers/specs/2026-07-14-session-compaction-design.md` | 实现后状态 → 已实现 |

**常量（Tauri / 前端共享语义，可各写一份）：**

```rust
const COMPACTION_THRESHOLD: f32 = 0.50;
const COMPACTION_KEEP_TAIL_BUBBLES: usize = 3;
const COMPACTION_MIN_BUBBLES: usize = 6;
const COMPACTION_COOLDOWN_SECS: u64 = 60;
const COMPACTION_SUMMARY_PREFIX: &str = "[CONTEXT COMPACTION]";
const COMPACTION_FALLBACK_PREFIX: &str = "[CONTEXT COMPACTION — fallback summary]";
```

---

### Task 1: `end_session` + `assert_session_writable`

**Files:**
- Modify: `memory/src/session/store/sessions.rs`
- Modify: `memory/src/session/store/messages.rs`（`append_message` 开头调用 guard）
- Modify: `memory/tests/session_store_test.rs`

- [ ] **Step 1: 写失败测试**

在 `memory/tests/session_store_test.rs` 追加：

```rust
#[test]
fn end_session_sets_ended_at_and_reason() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).unwrap();
    store
        .create_session("s1", "test", Some("gpt"), None, None)
        .unwrap();
    store.end_session("s1", "compacted").unwrap();
    let row = store.get_session("s1").unwrap().unwrap();
    assert!(row.ended_at.is_some());
    assert_eq!(row.end_reason.as_deref(), Some("compacted"));
}

#[test]
fn append_message_rejects_ended_session() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).unwrap();
    store.create_session("s1", "test", None, None, None).unwrap();
    store.end_session("s1", "compacted").unwrap();
    let err = store
        .append_message(NewMessage {
            content: Some("x"),
            ..NewMessage::empty("s1", "user")
        })
        .unwrap_err();
    assert!(
        err.to_string().contains("ended") || err.to_string().contains("writable"),
        "{err}"
    );
}
```

（若现有 `NewMessage.content` 类型是 `Option<&'a str>`，用 `Some("x")`；若是 `Option<String>`，跟本文件其它测试一致。）

- [ ] **Step 2: 跑测确认失败**

```bash
cargo test -p memory --test session_store_test end_session_sets_ended_at -- --nocapture
```

Expected: FAIL（方法不存在）

- [ ] **Step 3: 实现**

在 `sessions.rs`：

```rust
/// 标记会话结束（幂等：已 ended 且 reason 相同则 Ok）。
pub fn end_session(&self, id: &str, end_reason: &str) -> Result<()> {
    if self.get_session(id)?.is_none() {
        anyhow::bail!("end_session: session not found");
    }
    let ended_at = now_epoch_secs()?;
    self.conn.execute(
        "UPDATE sessions SET ended_at = ?1, end_reason = ?2 WHERE id = ?3",
        params![ended_at, end_reason, id],
    )?;
    Ok(())
}

/// 会话存在且未结束。
pub fn assert_session_writable(&self, id: &str) -> Result<()> {
    let Some(s) = self.get_session(id)? else {
        anyhow::bail!("assert_session_writable: session not found");
    };
    if s.ended_at.is_some() {
        anyhow::bail!(
            "assert_session_writable: session ended ({})",
            s.end_reason.unwrap_or_else(|| "unknown".into())
        );
    }
    Ok(())
}
```

在 `append_message` 事务前调用 `self.assert_session_writable(msg.session_id)?;`。

注意：`ensure_session` / `create_session` 不受影响；`compact_and_split` 只往**新**会话 append/拷贝。

- [ ] **Step 4: 跑测通过**

```bash
cargo test -p memory --test session_store_test end_session_sets_ended_at -- --nocapture
cargo test -p memory --test session_store_test append_message_rejects_ended_session -- --nocapture
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add memory/src/session/store/sessions.rs memory/src/session/store/messages.rs memory/tests/session_store_test.rs
git commit -m "$(cat <<'EOF'
feat(session): end_session and reject writes on ended sessions

EOF
)"
```

---

### Task 2: `compact_and_split` + 尾部气泡 helper

**Files:**
- Modify: `memory/src/session/store/messages.rs`
- Modify: `memory/tests/session_store_test.rs`

- [ ] **Step 1: 写失败测试**

```rust
#[test]
fn compact_and_split_ends_old_and_seeds_new_with_summary_and_tail() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).unwrap();
    store
        .create_session("old", "test", Some("gpt"), None, None)
        .unwrap();
    store.set_session_title("old", "topic").unwrap();
    for (role, text) in [
        ("user", "u1"),
        ("assistant", "a1"),
        ("user", "u2"),
        ("assistant", "a2"),
        ("user", "u3"),
        ("assistant", "a3"),
    ] {
        store
            .append_message(NewMessage {
                content: Some(text),
                ..NewMessage::empty("old", role)
            })
            .unwrap();
    }
    // 给最后一条 assistant 挂 tool
    store
        .append_message(NewMessage {
            content: Some("tool-out"),
            tool_call_id: Some("c1"),
            tool_name: Some("x"),
            ..NewMessage::empty("old", "tool")
        })
        .unwrap();

    store
        .compact_and_split(
            "old",
            "new",
            "[CONTEXT COMPACTION]\nsummary body",
            2, // 保留 u3 + a3(+tool)
        )
        .unwrap();

    let old = store.get_session("old").unwrap().unwrap();
    assert!(old.ended_at.is_some());
    assert_eq!(old.end_reason.as_deref(), Some("compacted"));
    // 旧全文仍在
    assert!(store.get_messages("old").unwrap().len() >= 6);

    let neu = store.get_session("new").unwrap().unwrap();
    assert_eq!(neu.parent_session_id.as_deref(), Some("old"));
    assert_eq!(neu.model.as_deref(), Some("gpt"));
    assert!(neu.ended_at.is_none());
    assert!(neu.title.as_deref().unwrap_or("").contains("continued"));

    let msgs = store.get_messages("new").unwrap();
    assert_eq!(msgs[0].role, "user");
    assert!(msgs[0]
        .content
        .as_deref()
        .unwrap_or("")
        .starts_with("[CONTEXT COMPACTION]"));
    // 摘要 + u3 + a3 + tool
    assert_eq!(msgs.len(), 4);
    assert_eq!(msgs[1].content.as_deref(), Some("u3"));
    assert_eq!(msgs[2].role, "assistant");
    assert_eq!(msgs[3].role, "tool");
}

#[test]
fn compact_and_split_keep_zero_is_summary_only() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).unwrap();
    store.create_session("old", "test", None, None, None).unwrap();
    store
        .append_message(NewMessage {
            content: Some("u1"),
            ..NewMessage::empty("old", "user")
        })
        .unwrap();
    store
        .compact_and_split("old", "new", "[CONTEXT COMPACTION]\nx", 0)
        .unwrap();
    let msgs = store.get_messages("new").unwrap();
    assert_eq!(msgs.len(), 1);
}
```

- [ ] **Step 2: 跑测确认失败**

```bash
cargo test -p memory --test session_store_test compact_and_split -- --nocapture
```

Expected: FAIL

- [ ] **Step 3: 实现 helper + `compact_and_split`**

在 `messages.rs`（`end_inclusive_for_bubbles` 旁）：

```rust
/// 返回保留尾部 `keep` 个 user/assistant 气泡（含 assistant 后连续 tool）的**起始下标**。
fn start_inclusive_for_tail_bubbles(messages: &[StoredMessage], keep: usize) -> Option<usize> {
    if keep == 0 || messages.is_empty() {
        return None;
    }
    let mut bubble_starts = Vec::new();
    for (i, m) in messages.iter().enumerate() {
        if m.role == "user" || m.role == "assistant" {
            bubble_starts.push(i);
        }
    }
    if bubble_starts.is_empty() {
        return None;
    }
    let skip = bubble_starts.len().saturating_sub(keep);
    Some(bubble_starts[skip])
}
```

```rust
/// 结束旧会话并拆出子会话：摘要消息 + 最近 `keep_tail_bubbles` 轮（含 tool）。
pub fn compact_and_split(
    &self,
    old_id: &str,
    new_id: &str,
    summary_text: &str,
    keep_tail_bubbles: usize,
) -> Result<()> {
    if old_id == new_id {
        anyhow::bail!("compact_and_split: session ids must differ");
    }
    if self.get_session(new_id)?.is_some() {
        anyhow::bail!("compact_and_split: target session already exists");
    }
    let parent = self
        .get_session(old_id)?
        .ok_or_else(|| anyhow::anyhow!("compact_and_split: source session not found"))?;
    if parent.ended_at.is_some() {
        anyhow::bail!("compact_and_split: source session already ended");
    }

    self.end_session(old_id, "compacted")?;

    let model = parent.model.clone();
    let source = parent.source.clone();
    self.create_session(
        new_id,
        &source,
        model.as_deref(),
        None,
        Some(old_id),
    )?;

    self.append_message(NewMessage {
        content: Some(summary_text),
        ..NewMessage::empty(new_id, "user")
    })?;

    if keep_tail_bubbles > 0 {
        let messages = self.get_messages(old_id)?;
        if let Some(start) = start_inclusive_for_tail_bubbles(&messages, keep_tail_bubbles) {
            let tx = self.conn.unchecked_transaction()?;
            let mut message_count = 1i64; // 已有摘要
            let mut tool_call_count = 0i64;
            for m in &messages[start..] {
                let tool_calls = json_to_db(&m.tool_calls)?;
                let reasoning_details = json_to_db(&m.reasoning_details)?;
                let codex_reasoning_items = json_to_db(&m.codex_reasoning_items)?;
                let codex_message_items = json_to_db(&m.codex_message_items)?;
                tx.execute(
                    "INSERT INTO messages (
                        session_id, role, content, tool_call_id, tool_calls, tool_name,
                        timestamp, token_count, finish_reason,
                        reasoning, reasoning_content, reasoning_details,
                        codex_reasoning_items, codex_message_items
                     ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6,
                        ?7, ?8, ?9,
                        ?10, ?11, ?12,
                        ?13, ?14
                     )",
                    params![
                        new_id,
                        m.role,
                        m.content,
                        m.tool_call_id,
                        tool_calls,
                        m.tool_name,
                        m.timestamp,
                        m.token_count,
                        m.finish_reason,
                        m.reasoning,
                        m.reasoning_content,
                        reasoning_details,
                        codex_reasoning_items,
                        codex_message_items,
                    ],
                )?;
                message_count += 1;
                if m.role == "tool" {
                    tool_call_count += 1;
                }
            }
            tx.execute(
                "UPDATE sessions
                 SET message_count = ?1, tool_call_count = ?2
                 WHERE id = ?3",
                params![message_count, tool_call_count, new_id],
            )?;
            tx.commit()?;
        }
    }

    if let Some(title) = parent.title.filter(|t| !t.trim().is_empty()) {
        let continued = format!("{title} · continued");
        let _ = self.set_session_title(new_id, &continued);
    }

    Ok(())
}
```

实现细节：
- 摘要用 `append_message`（会维护计数）；随后直接 SQL 批量插入尾部时**覆盖** `message_count` / `tool_call_count`（与 `fork_session` 一致）。
- 不要在事务里再调 `append_message`（避免嵌套事务）。
- 不把父会话 billing 拷到子会话（新行默认 0/NULL）。

- [ ] **Step 4: 跑测通过**

```bash
cargo test -p memory --test session_store_test compact_and_split -- --nocapture
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add memory/src/session/store/messages.rs memory/tests/session_store_test.rs
git commit -m "$(cat <<'EOF'
feat(session): compact_and_split with summary and tail bubbles

EOF
)"
```

---

### Task 3: 侧栏 DTO 暴露 `endReason`

**Files:**
- Modify: `memory/src/session/store/mod.rs`（`RecentSession`）
- Modify: `memory/src/session/store/search.rs`（`list_recent_sessions` SELECT）
- Modify: `frontend/src-tauri/src/commands.rs`（`RecentSessionDto` + map）
- Modify: `frontend/src/types.ts`

- [ ] **Step 1: 扩展 `RecentSession`**

```rust
pub struct RecentSession {
    pub id: String,
    pub title: Option<String>,
    pub started_at: f64,
    pub preview: Option<String>,
    pub ended_at: Option<f64>,
    pub end_reason: Option<String>,
}
```

`list_recent_sessions` SQL 增加 `s.ended_at, s.end_reason` 并填入结构体。

- [ ] **Step 2: DTO**

```rust
// RecentSessionDto
pub end_reason: Option<String>, // serde rename_all = camelCase → endReason
```

```ts
export type RecentSessionDto = {
  sessionId: string;
  summary: string;
  createdAt: string | null;
  endReason?: string | null;
};
```

- [ ] **Step 3: 编译检查**

```bash
cargo check -p memory
cargo check -p frontend-lib 2>/dev/null || cargo check --manifest-path frontend/src-tauri/Cargo.toml
```

Expected: 无错误（按仓库实际 package 名调整）

- [ ] **Step 4: Commit**

```bash
git add memory/src/session/store/mod.rs memory/src/session/store/search.rs \
  frontend/src-tauri/src/commands.rs frontend/src/types.ts
git commit -m "$(cat <<'EOF'
feat(session): expose end_reason on recent session list

EOF
)"
```

---

### Task 4: Tauri `compact_chat_session`（摘要 + 拆分）

**Files:**
- Create: `frontend/src-tauri/src/compaction_commands.rs`
- Modify: `frontend/src-tauri/src/lib.rs`

- [ ] **Step 1: 模块骨架与 DTO**

```rust
//! 会话压实：LLM/启发式摘要 + SessionStore 拆分。

use futures::StreamExt;
use serde::Serialize;
use uuid::Uuid;

use memory::{default_memory_dir, MemoryManager};
use providers::registry::ProviderRegistry;
use providers::trait_::{ChatMessage, ProviderConfig};

use crate::providers_commands::{self, resolve_api_key, ProviderConfig as UiProvider};

const KEEP_TAIL_DEFAULT: usize = 3;
const SUMMARY_PREFIX: &str = "[CONTEXT COMPACTION]";
const FALLBACK_PREFIX: &str = "[CONTEXT COMPACTION — fallback summary]";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactChatResultDto {
    pub new_session_id: String,
    pub summary_preview: String,
    pub degraded: bool,
}
```

- [ ] **Step 2: 启发式摘要**

```rust
fn heuristic_summary(messages: &[memory::session_store::StoredMessage], max_chars: usize) -> String {
    let mut parts = Vec::new();
    for m in messages.iter().rev() {
        if m.role != "user" && m.role != "assistant" {
            continue;
        }
        let text = m.content.as_deref().unwrap_or("").trim();
        if text.is_empty() {
            continue;
        }
        parts.push(format!("{}: {}", m.role, text.chars().take(400).collect::<String>()));
        if parts.len() >= 8 {
            break;
        }
    }
    parts.reverse();
    let body = parts.join("\n");
    let clipped: String = body.chars().take(max_chars).collect();
    format!("{FALLBACK_PREFIX}\n{clipped}")
}
```

（`StoredMessage` 导出路径以 `memory` 实际 `pub use` 为准；若不便导出，则 `get_messages_as_conversation` 拼字符串，避免放大 API。）

- [ ] **Step 3: LLM 摘要（对齐 dreaming `complete_chat`）**

```rust
async fn summarize_with_llm(transcript: &str) -> Result<String, String> {
    let state = providers_commands::get_providers_state()?;
    let id = state
        .active_provider_id
        .or_else(|| state.providers.first().map(|p| p.id.clone()))
        .ok_or_else(|| "no provider".to_string())?;
    let ui: UiProvider = providers_commands::find_provider(&id)?;
    let api_key = resolve_api_key(&ui)?;
    let registry = ProviderRegistry::default();
    let provider = registry
        .get(&ui.backend_id)
        .ok_or_else(|| format!("unsupported backend {}", ui.backend_id))?;
    let config = ProviderConfig {
        api_key,
        base_url: ui.base_url.filter(|u| !u.trim().is_empty()),
        model: ui.model.clone(),
        temperature: 0.2,
        max_tokens: 2048,
        thinking_enabled: false,
        reasoning_effort: "high".to_string(),
        additional_params: serde_json::Value::Null,
    };
    let system = "You compress a chat transcript into a compact handoff note. \
Cover: goals, constraints, done, in-progress, key paths/decisions, next steps. \
Reply in the same language as the transcript. No preamble.";
    let user = format!("Transcript:\n\n{transcript}");
    let messages = vec![
        ChatMessage::text("system", system),
        ChatMessage::text("user", user),
    ];
    let mut stream = provider
        .chat_stream(messages, vec![], &config)
        .await
        .map_err(|e| e.to_string())?;
    let mut out = String::new();
    while let Some(item) = stream.next().await {
        let chunk = item.map_err(|e| e.to_string())?;
        if let Some(tok) = chunk.token {
            out.push_str(&tok);
        }
    }
    let trimmed = out.trim().to_string();
    if trimmed.is_empty() {
        return Err("empty summary".into());
    }
    Ok(format!("{SUMMARY_PREFIX}\n{trimmed}"))
}
```

字段名（`backend_id` / `model`）必须与 `providers_commands::ProviderConfig` 实际字段对齐——打开该结构体抄写，勿猜。

- [ ] **Step 4: 命令**

```rust
#[tauri::command]
pub async fn compact_chat_session(
    session_id: String,
    keep_tail_bubbles: Option<i32>,
    _focus: Option<String>,
) -> Result<CompactChatResultDto, String> {
    let sid = session_id.trim();
    if sid.is_empty() {
        return Err("session_id 不能为空".into());
    }
    let keep = keep_tail_bubbles
        .map(|k| k.max(0) as usize)
        .unwrap_or(KEEP_TAIL_DEFAULT);

    let mgr = MemoryManager::new(default_memory_dir()).map_err(|e| e.to_string())?;
    let store = &mgr.session_store;
    let meta = store
        .get_session(sid)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "会话不存在".to_string())?;
    if meta.ended_at.is_some() {
        return Err("会话已结束，无法压实".into());
    }

    let messages = store.get_messages(sid).map_err(|e| e.to_string())?;
    let bubble_count = messages
        .iter()
        .filter(|m| m.role == "user" || m.role == "assistant")
        .count();
    if bubble_count < 2 {
        return Err("消息过少，无需压实".into());
    }

    let transcript: String = messages
        .iter()
        .filter(|m| m.role == "user" || m.role == "assistant")
        .map(|m| {
            format!(
                "{}: {}",
                m.role,
                m.content.as_deref().unwrap_or("").chars().take(2000).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    // 控制 prompt 体积
    let transcript: String = transcript.chars().take(60_000).collect();

    let (summary, degraded) = match summarize_with_llm(&transcript).await {
        Ok(s) => (s, false),
        Err(_) => (heuristic_summary(&messages, 6_000), true),
    };

    let new_id = Uuid::new_v4().to_string();
    store
        .compact_and_split(sid, &new_id, &summary, keep)
        .map_err(|e| e.to_string())?;

    let preview: String = summary.chars().take(160).collect();
    Ok(CompactChatResultDto {
        new_session_id: new_id,
        summary_preview: preview,
        degraded,
    })
}
```

在 `lib.rs`：`mod compaction_commands;` 并 `.invoke_handler` 注册 `compaction_commands::compact_chat_session`。

- [ ] **Step 5: 编译**

```bash
cargo check --manifest-path frontend/src-tauri/Cargo.toml
```

Expected: ok

- [ ] **Step 6: Commit**

```bash
git add frontend/src-tauri/src/compaction_commands.rs frontend/src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(tauri): add compact_chat_session with LLM/heuristic summary

EOF
)"
```

---

### Task 5: 前端手动 `/compact` + 无感切换

**Files:**
- Modify: `frontend/src/lib/composerCommands.ts`
- Modify: `frontend/src/i18n/messages.ts`
- Modify: `frontend/src/App.tsx`
- Modify: `frontend/src/components/ChatSessionList.tsx`（徽章可本任务一并做）

- [ ] **Step 1: Slash + i18n**

`SlashAction` 增加 `"compact"`；`BUILTIN_SLASH_COMMANDS` 增加：

```ts
{
  name: "compact",
  aliases: ["compress"],
  descKey: "chat.slashCompact",
  action: "compact",
  icon: "↯",
},
```

中英文案：

- `chat.slashCompact`: 「压实并继续」 / `Compact and continue`
- `chat.compactDone`: 「已压实并切换到新会话」 / `Compacted — switched to new session`
- `chat.compactDegraded`: 「摘要降级为剪贴文本，已切换新会话」 / …
- `chat.compactFailed`: 「压实失败：{error}」
- `chat.compactBlockedStreaming`: 「生成中无法压实」
- `chat.compactBlockedInterrupt`: 「请先处理待确认操作」
- `chat.sessionCompactedBadge`: 「已压实」 / `Compacted`

- [ ] **Step 2: `runCompactSession` in App**

模式对齐 `branchMessage`：

```ts
const lastCompactAtRef = useRef(0);

const runCompactSession = useCallback(async () => {
  if (streaming) {
    showTransientToast(t("chat.compactBlockedStreaming"));
    return;
  }
  if (sessionPendingInterrupts.length > 0) {
    showTransientToast(t("chat.compactBlockedInterrupt"));
    return;
  }
  if (!sessionId) {
    showTransientToast(t("chat.compactFailed", { error: "no session" }));
    return;
  }
  try {
    const res = await invoke<{
      newSessionId: string;
      summaryPreview: string;
      degraded: boolean;
    }>("compact_chat_session", {
      sessionId,
      keepTailBubbles: 3,
      focus: null,
    });
    const history = await invoke<{
      sessionId: string | null;
      messages: ChatHistoryMessageDto[]; /* 用 App 现有 DTO 类型 */
    }>("get_chat_history", { sessionId: res.newSessionId, limit: 200 });
    const restored = /* 复用现有 get_chat_history → ChatMessage 映射函数 */;
    unlistenRef.current?.();
    unlistenRef.current = null;
    clearStreamBuffers();
    setSessionPendingInterrupts([]);
    setStreaming(false);
    setStreamPaused(false);
    setFocusMessageId(null);
    currentRunIdRef.current = null;
    setCurrentTurnId(null);
    setSessionId(res.newSessionId);
    setMessages(restored);
    saveChatSession(res.newSessionId, restored, []);
    lastCompactAtRef.current = Date.now();
    showTransientToast(
      res.degraded ? t("chat.compactDegraded") : t("chat.compactDone"),
    );
  } catch (e) {
    showTransientToast(
      t("chat.compactFailed", {
        error: e instanceof Error ? e.message : String(e ?? "error"),
      }),
    );
  }
}, [/* deps */]);
```

在 `handleSlashAction`：`case "compact": void runCompactSession(); break;`

映射历史时优先复用 `applyRestoredHistory` / hydrate 路径，避免复制一半字段丢 activities。

- [ ] **Step 3: 侧栏徽章**

`ChatSessionList`：若 `s.endReason === "compacted"`，在标题旁加 `<span className="chat-session-badge">{t("chat.sessionCompactedBadge")}</span>`。只读打开仍走 `onOpenSession`（`get_chat_history` 可读 ended 会话）。

- [ ] **Step 4: 手动冒烟**

启动 app → 造 ≥2 轮对话 → `/compact` → 确认 `sessionId` 变化、气泡含摘要、侧栏旧会话有徽章、旧会话可打开看全文。

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/composerCommands.ts frontend/src/i18n/messages.ts \
  frontend/src/App.tsx frontend/src/components/ChatSessionList.tsx frontend/src/styles/layout.css
git commit -m "$(cat <<'EOF'
feat(chat): /compact switches to split session after summary

EOF
)"
```

---

### Task 6: 自动阈值 + 冷却

**Files:**
- Modify: `frontend/src/App.tsx`

- [ ] **Step 1: 在流结束 / 一轮完成处挂检查**

复用 `App` 里已有 `contextUsagePercent` / `tokenUsage` 计算（约 `contextUsageFallback` 与 `tokenUsage.totalTokens / 128_000`）。

在「assistant 一轮完全结束且 `streaming` 变为 false」的已有 effect / finally 路径中调用：

```ts
const maybeAutoCompact = useCallback(() => {
  if (streaming) return;
  if (sessionPendingInterrupts.length > 0) return;
  if (!sessionId) return;
  const bubbles = messages.filter((m) => m.id !== "welcome" && (m.role === "user" || m.role === "assistant")).length;
  if (bubbles < 6) return;
  if (Date.now() - lastCompactAtRef.current < 60_000) return;
  const pct =
    tokenUsage && tokenUsage.totalTokens > 0
      ? tokenUsage.totalTokens / 128_000
      : /* 与 contextUsageFallback 同口径 */ 0;
  if (pct < 0.5) return;
  void runCompactSession();
}, [/* deps */]);
```

注意：自动路径失败不要死循环——`runCompactSession` 失败时也应更新 `lastCompactAtRef`（或单独 `lastAutoCompactAttemptRef`），满足 spec 冷却。

发送下一用户消息前可再调用一次 `maybeAutoCompact`（可选二次门闩）；MVP 只做「轮次结束」即可。

- [ ] **Step 2: 冒烟**

临时把阈值改成 `0.01` 或 mock `tokenUsage`，确认一轮后自动压实；再确认流式中不触发。

- [ ] **Step 3: Commit**

```bash
git add frontend/src/App.tsx
git commit -m "$(cat <<'EOF'
feat(chat): auto-compact when context usage passes threshold

EOF
)"
```

---

### Task 7: Spec 收尾

**Files:**
- Modify: `docs/superpowers/specs/2026-07-14-session-compaction-design.md`

- [ ] **Step 1:** 状态改为 **已实现**；可选加「实现说明」表（store / Tauri / 前端入口）。

- [ ] **Step 2: Commit**

```bash
git add docs/superpowers/specs/2026-07-14-session-compaction-design.md
git commit -m "$(cat <<'EOF'
docs(session): mark compaction P1b implemented

EOF
)"
```

---

## Spec coverage（自检）

| Spec 项 | Task |
|---------|------|
| 拆新会话 + `parent` + `end_reason=compacted` | 1–2 |
| 摘要 + 尾 K（含 tool） | 2、4 |
| LLM 失败仍拆 + degraded | 4–5 |
| 手动 `/compact` | 5 |
| 自动 ~50% + 冷却 | 6 |
| 流式 / interrupt 禁止 | 5–6 |
| 侧栏旧会话只读展示 | 3、5 |
| 计费不拷贝 | 2（create 新行） |
| 命名无外部品牌 | 全程 |
| 非目标：原地压实 / OTEL / HITL 迁移 | 不做 |

## Placeholder scan

无 TBD；Provider 字段名要求打开源文件对齐（Task 4 Step 3）。
