# Observability Alignment (S1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 贯通 `turn_id`、落地 `agent.log`/`errors.log` 分层，并在偏好设置提供按 `session_id`/`turn_id` 过滤的日志诊断面板。

**Architecture:** 将多轮流式入口已有的 `run_id` 作为本轮 `turn_id`（同源 UUID）；写入 UsageDb 新列与 tracing 字段；`init_logging` 双文件 sink；`query_agent_logs` 扫文件尾部过滤；Preferences「日志 / 诊断」调用 Tauri 命令。不引入 OTEL、不复活 EventBus、不做 Insights turn 下钻。

**Tech Stack:** Rust (`memory` / `agent` / Tauri)、`tracing` + `tracing-subscriber` + `tracing-appender`、rusqlite、React Preferences + i18n

**Spec:** [`docs/superpowers/specs/2026-07-14-observability-alignment-design.md`](../specs/2026-07-14-observability-alignment-design.md)

**Naming:** 代码与 UI **禁止**出现参考项目品牌字符串。

---

## File map

| Path | Responsibility |
|------|----------------|
| `memory/src/usage/db.rs` | `turn_id` 列、安全迁移、`NewUsageEvent`、insert |
| `memory/tests/usage_db_test.rs` | 迁移 / insert / 读回 `turn_id` |
| `memory/src/infra/log_query.rs`（新建） | `AgentLogQuery` / `query_agent_logs` |
| `memory/src/infra/logging.rs` | 双 sink：`agent.log` + `errors.log` |
| `memory/src/infra/mod.rs` / `lib.rs` | re-export |
| `agent/src/loop_.rs` | `current_turn_id` 字段；工具 usage 写入带 `turn_id` |
| `agent/src/streaming.rs` | `run_id` → 设为 `turn_id`；关键路径 `tracing` 带字段；`record_llm_usage` 贯通 |
| `agent/src/usage_record.rs` | `build_llm_usage_event` / dual_write 接受 `turn_id` |
| `memory/src/usage/stats.rs` 等 | `NewUsageEvent` 构造补 `turn_id: None`（或透传） |
| `frontend/src-tauri/src/config_commands.rs`（或 `diagnostics_commands.rs`） | `query_agent_logs` |
| `frontend/src-tauri/src/lib.rs` | 注册 command；`init_logging("agent")` |
| `frontend/src/components/PreferencesPanel.tsx` | 「日志 / 诊断」分区 |
| `frontend/src/i18n/messages.ts` | 文案键 |
| `frontend/src/styles/`（若需） | 诊断列表最小样式（复用 `prefs-*` 优先） |

---

## Task 1: UsageDb `turn_id` 列 + 安全迁移（TDD）

**Files:**
- Modify: `memory/src/usage/db.rs`
- Modify: `memory/tests/usage_db_test.rs`
- Modify: 所有 `NewUsageEvent { ... }` 构造处（见下方 Step 4 清单）

- [ ] **Step 1: Write failing migration + insert tests**

在 `memory/tests/usage_db_test.rs` 追加：

```rust
#[test]
fn migrate_v3_to_v4_keeps_rows_and_adds_turn_id() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("usage.db");

    // 手动建 v3 形状（无 turn_id）
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            r#"
            PRAGMA journal_mode=WAL;
            CREATE TABLE usage_events (
                id TEXT PRIMARY KEY,
                ts TEXT NOT NULL,
                kind TEXT NOT NULL,
                name TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                session_id TEXT,
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                cache_write_tokens INTEGER NOT NULL DEFAULT 0,
                reasoning_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                cost_usd REAL NOT NULL DEFAULT 0,
                cost_status TEXT,
                cost_source TEXT,
                pricing_version TEXT,
                billing_provider TEXT,
                billing_base_url TEXT,
                billing_mode TEXT,
                meta_json TEXT
            );
            PRAGMA user_version = 3;
            "#,
        )
        .unwrap();
        conn.execute(
            "INSERT INTO usage_events (id, ts, kind, name, agent_id, total_tokens, cost_usd)
             VALUES ('old1', '2026-01-01T00:00:00Z', 'llm', 'm', 'a', 10, 0.0)",
            [],
        )
        .unwrap();
    }

    let db = memory::UsageDb::new(path.clone()).unwrap();
    // 旧行仍在
    let n: i64 = {
        // 若无公开 count，用 list_trace 或临时 query；最小可：insert 新行后依赖公开 API
        let _ = &db;
        1
    };
    assert_eq!(n, 1);

    let id = db
        .insert(memory::NewUsageEvent {
            ts: "2026-07-14T00:00:00Z".into(),
            kind: "llm".into(),
            name: "m2".into(),
            agent_id: "a".into(),
            session_id: Some("s1".into()),
            turn_id: Some("turn-abc".into()),
            input_tokens: 1,
            output_tokens: 2,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 3,
            cost_usd: 0.0,
            cost_status: None,
            cost_source: None,
            pricing_version: None,
            billing_provider: None,
            billing_base_url: None,
            billing_mode: None,
            meta_json: None,
        })
        .unwrap();
    assert!(!id.is_empty());

    let conn = rusqlite::Connection::open(&path).unwrap();
    let ver: i32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(ver, memory::USAGE_SCHEMA_VERSION); // 应为 4
    let turn: Option<String> = conn
        .query_row(
            "SELECT turn_id FROM usage_events WHERE id = ?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(turn.as_deref(), Some("turn-abc"));
    let old_ok: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM usage_events WHERE id = 'old1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(old_ok, 1);
}
```

（若 `USAGE_SCHEMA_VERSION` 未 export，在 `memory/src/lib.rs` 增加 `pub use usage::db::USAGE_SCHEMA_VERSION;`。）

- [ ] **Step 2: Run test — expect FAIL**

```bash
cargo test -p memory migrate_v3_to_v4_keeps_rows_and_adds_turn_id -- --nocapture
```

Expected: compile error（无 `turn_id` 字段）或打开库时毁掉 old1。

- [ ] **Step 3: Implement schema + safe migrate**

在 `memory/src/usage/db.rs`：

1. `USAGE_SCHEMA_VERSION: i32 = 4`
2. DDL 的 `usage_events` 增加 `turn_id TEXT`
3. `NewUsageEvent` 增加 `pub turn_id: Option<String>`
4. **重写** `UsageDb::new` 迁移逻辑（破坏性 rebuild **仅**用于无法恢复的旧版本；v3→v4 必须 ADD COLUMN）：

```rust
pub fn new(path: PathBuf) -> anyhow::Result<Self> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let exists = path.exists();
    let version = if exists {
        let conn = Connection::open(&path)?;
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))?
    } else {
        0
    };

    if !exists || version == 0 {
        delete_usage_db_files(&path);
        let conn = open_and_init(&path)?;
        return Ok(Self { conn });
    }

    if version > USAGE_SCHEMA_VERSION {
        anyhow::bail!("usage.db schema version {version} is newer than supported {USAGE_SCHEMA_VERSION}");
    }

    if version < 3 {
        // 仅极旧库仍可 rebuild（与历史行为一致）；记录 warn
        tracing::warn!(version, "usage.db too old; rebuilding");
        delete_usage_db_files(&path);
        let conn = open_and_init(&path)?;
        return Ok(Self { conn });
    }

    let conn = Connection::open(&path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    if version < 4 {
        let has_turn: bool = {
            let mut stmt = conn.prepare("PRAGMA table_info(usage_events)")?;
            let names: Vec<String> = stmt
                .query_map([], |r| r.get::<_, String>(1))?
                .filter_map(|x| x.ok())
                .collect();
            names.iter().any(|n| n == "turn_id")
        };
        if !has_turn {
            conn.execute("ALTER TABLE usage_events ADD COLUMN turn_id TEXT", [])?;
        }
        conn.execute_batch(&format!("PRAGMA user_version = {USAGE_SCHEMA_VERSION};"))?;
    }
    // 确保索引等（幂等 DDL 中除建表外）
    Ok(Self { conn })
}
```

5. `insert` SQL 与 params 增加 `turn_id`（占位符顺延）

- [ ] **Step 4: Fix all `NewUsageEvent` call sites**

为每处结构体字面量补 `turn_id: None`（或有值则透传）：

- `memory/src/usage/stats.rs`（两处）
- `memory/src/usage/trace_insights.rs`（测试/helper）
- `memory/src/usage/db.rs`（测试 helper）
- `memory/tests/usage_db_test.rs`、`collab_insights_test.rs`
- `agent/src/usage_record.rs`
- `agent/src/loop_.rs`
- `agent/src/orchestration.rs`
- `agent/src/cron_exec.rs`

- [ ] **Step 5: Run tests — expect PASS**

```bash
cargo test -p memory --test usage_db_test
cargo test -p memory migrate_v3_to_v4
```

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add memory/src/usage/db.rs memory/tests/usage_db_test.rs memory/src/lib.rs \
  memory/src/usage/stats.rs memory/src/usage/trace_insights.rs \
  agent/src/usage_record.rs agent/src/loop_.rs agent/src/orchestration.rs agent/src/cron_exec.rs \
  memory/tests/collab_insights_test.rs
git commit -m "$(cat <<'EOF'
feat(usage): add turn_id column with non-destructive v4 migrate

Preserve existing usage.db rows when upgrading; stop wipe-on-version-bump for this change.
EOF
)"
```

---

## Task 2: `query_agent_logs`（TDD）

**Files:**
- Create: `memory/src/infra/log_query.rs`
- Modify: `memory/src/infra/mod.rs`
- Modify: `memory/src/lib.rs`
- Test: 单元测试放在 `log_query.rs` 的 `#[cfg(test)]`

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_lines(path: &std::path::Path, lines: &[&str]) {
        let mut f = std::fs::File::create(path).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
    }

    #[test]
    fn filters_by_session_and_turn_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let agent = dir.path().join("agent.log");
        write_lines(
            &agent,
            &[
                "INFO keep session_id=s1 turn_id=t1 hello",
                "INFO skip session_id=s2 turn_id=t9 other",
                "WARN err session_id=s1 turn_id=t1 boom",
            ],
        );
        let lines = query_agent_logs(AgentLogQuery {
            logs_dir: dir.path().to_path_buf(),
            session_id: Some("s1".into()),
            turn_id: Some("t1".into()),
            min_level: None,
            lines: 50,
            source: LogSource::Agent,
        })
        .unwrap();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].raw.contains("boom")); // newest first
        assert_eq!(lines[0].source, "agent");
    }

    #[test]
    fn errors_source_only_reads_errors_file() {
        let dir = tempfile::tempdir().unwrap();
        write_lines(&dir.path().join("agent.log"), &["INFO session_id=s1 a"]);
        write_lines(
            &dir.path().join("errors.log"),
            &["WARN session_id=s1 turn_id=t1 e"],
        );
        let lines = query_agent_logs(AgentLogQuery {
            logs_dir: dir.path().to_path_buf(),
            session_id: Some("s1".into()),
            turn_id: None,
            min_level: None,
            lines: 50,
            source: LogSource::Errors,
        })
        .unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].source, "errors");
    }
}
```

- [ ] **Step 2: Run — expect FAIL (module missing)**

```bash
cargo test -p memory filters_by_session_and_turn_newest_first -- --nocapture
```

- [ ] **Step 3: Implement `log_query.rs`**

```rust
//! 扫描 `~/.astro/logs` 下 agent/errors 日志尾部并按 session/turn 过滤。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogSource {
    Agent,
    Errors,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLogQuery {
    /// 测试可覆写；生产传 `logs_dir()`。
    #[serde(default)]
    pub logs_dir: PathBuf,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub min_level: Option<String>,
    pub lines: usize,
    pub source: LogSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLogLine {
    pub raw: String,
    pub source: String,
}

const MAX_LINES: usize = 500;

pub fn query_agent_logs(q: AgentLogQuery) -> anyhow::Result<Vec<AgentLogLine>> {
    let limit = q.lines.clamp(1, MAX_LINES);
    let mut out = Vec::new();
    let files: &[(&str, &str)] = match q.source {
        LogSource::Agent => &[("agent", "agent.log")],
        LogSource::Errors => &[("errors", "errors.log")],
        LogSource::Both => &[("agent", "agent.log"), ("errors", "errors.log")],
    };
    for (src, name) in files {
        let path = q.logs_dir.join(name);
        if !path.exists() {
            continue;
        }
        let raw = std::fs::read_to_string(&path)?;
        for line in raw.lines().rev() {
            if !line_matches(line, &q) {
                continue;
            }
            out.push(AgentLogLine {
                raw: line.to_string(),
                source: (*src).into(),
            });
            if out.len() >= limit {
                return Ok(out);
            }
        }
    }
    Ok(out)
}

fn line_matches(line: &str, q: &AgentLogQuery) -> bool {
    if let Some(ref sid) = q.session_id {
        if !sid.is_empty() && !line.contains(sid.as_str()) {
            return false;
        }
    }
    if let Some(ref tid) = q.turn_id {
        if !tid.is_empty() && !line.contains(tid.as_str()) {
            return false;
        }
    }
    if let Some(ref lvl) = q.min_level {
        if !level_ok(line, lvl) {
            return false;
        }
    }
    true
}

fn level_ok(line: &str, min: &str) -> bool {
    let order = |s: &str| match s.to_ascii_uppercase().as_str() {
        "DEBUG" => 0,
        "INFO" => 1,
        "WARN" | "WARNING" => 2,
        "ERROR" => 3,
        "CRITICAL" => 4,
        _ => -1,
    };
    let min_o = order(min);
    if min_o < 0 {
        return true;
    }
    for cand in ["CRITICAL", "ERROR", "WARNING", "WARN", "INFO", "DEBUG"] {
        if line.contains(cand) {
            let o = order(cand);
            if o < 0 {
                return true;
            }
            return o >= min_o;
        }
    }
    true // 无法解析则保留
}

pub fn default_agent_log_query() -> AgentLogQuery {
    AgentLogQuery {
        logs_dir: crate::logging::logs_dir(),
        session_id: None,
        turn_id: None,
        min_level: None,
        lines: 50,
        source: LogSource::Both,
    }
}
```

`infra/mod.rs`：`pub mod log_query;`  
`lib.rs`：`pub use log_query::{query_agent_logs, AgentLogLine, AgentLogQuery, LogSource};`

- [ ] **Step 4: Run tests — PASS**

```bash
cargo test -p memory filters_by_session_and_turn -- --nocapture
cargo test -p memory errors_source_only -- --nocapture
```

- [ ] **Step 5: Commit**

```bash
git add memory/src/infra/log_query.rs memory/src/infra/mod.rs memory/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(memory): add query_agent_logs tail filter by session/turn

EOF
)"
```

---

## Task 3: 双文件日志 sink（`agent.log` + `errors.log`）

**Files:**
- Modify: `memory/src/infra/logging.rs`
- Modify: `frontend/src-tauri/src/lib.rs`（`init_logging("agent")`）
- Modify: `backend/src/lib.rs`（`init_logging("agent")`，与桌面共用文件名语义；若 backend 独立部署可接受同名）

- [ ] **Step 1: Rewrite `init_logging`**

替换为双 non-blocking writer + 分层 level（示意，按现有 `OnceLock` 扩成存两个 guard 或元组）：

```rust
use tracing_subscriber::{filter::LevelFilter, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};
use tracing_subscriber::fmt;

static LOG_GUARDS: OnceLock<(WorkerGuard, WorkerGuard)> = OnceLock::new();

pub fn init_logging(_component: &str) -> anyhow::Result<()> {
    let _ = ensure_default_workspace()?;
    let log_dir = default_memory_dir().join("logs");
    std::fs::create_dir_all(&log_dir)?;

    let agent_appender = tracing_appender::rolling::daily(&log_dir, "agent.log");
    let (agent_nb, g1) = tracing_appender::non_blocking(agent_appender);
    let err_appender = tracing_appender::rolling::daily(&log_dir, "errors.log");
    let (err_nb, g2) = tracing_appender::non_blocking(err_appender);
    let _ = LOG_GUARDS.set((g1, g2));

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,memory=info,agent=info"));

    let agent_file = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_writer(agent_nb);

    let errors_file = fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_writer(err_nb)
        .with_filter(LevelFilter::WARN);

    let result = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true))
        .with(agent_file)
        .with(errors_file)
        .try_init();

    if result.is_ok() {
        tracing::info!(
            path = %log_dir.join("agent.log").display(),
            "file logging enabled (agent + errors)"
        );
    }
    Ok(())
}
```

注意：`Layer` 的 `with_filter` 需要 `tracing-subscriber` 的 `registry` feature（通常已开）。若编译失败，改用 `filter::filter_fn` 包一层。

- [ ] **Step 2: 统一调用方文件语义**

- `frontend/src-tauri/src/lib.rs`：`memory::init_logging("agent")`
- `backend/src/lib.rs`：`init_logging("agent")`

- [ ] **Step 3: Smoke**

```bash
cargo check -p memory -p frontend/src-tauri 2>/dev/null || cargo check -p memory
# 从仓库根：
cargo check -p memory
```

手动：启动一次 app 后确认 `~/.astro/logs/agent.log` 与 `errors.log` 出现（errors 可在触发 warn 后出现内容）。

- [ ] **Step 4: Commit**

```bash
git add memory/src/infra/logging.rs frontend/src-tauri/src/lib.rs backend/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(logging): split agent.log and errors.log sinks

EOF
)"
```

---

## Task 4: Agent 主路径贯通 `turn_id`

**Files:**
- Modify: `agent/src/loop_.rs`
- Modify: `agent/src/streaming.rs`
- Modify: `agent/src/usage_record.rs`
- Modify: 工具 usage 的 `NewUsageEvent`（`loop_.rs` 内 tool 记录处）

**约定：** `run_multi_turn_stream` 内已有 `run_id` —— **本轮 `turn_id` 与其相同**（同一 UUID）。`RunStarted.run_id` 值不变；Usage/logs 字段名用 `turn_id`。

- [ ] **Step 1: `AgentLoop` 持有当前回合**

在 `AgentLoop` 增加：

```rust
current_turn_id: Option<String>,
```

构造处置 `None`。增加：

```rust
pub fn set_current_turn_id(&mut self, turn_id: impl Into<String>) {
    self.current_turn_id = Some(turn_id.into());
}
pub fn clear_current_turn_id(&mut self) {
    self.current_turn_id = None;
}
pub fn current_turn_id(&self) -> Option<&str> {
    self.current_turn_id.as_deref()
}
```

- [ ] **Step 2: streaming 入口设置并打日志**

在 `run_multi_turn_stream` 生成 `run_id` 之后：

```rust
let turn_id = run_id.clone();
{
    let mut agent = session.lock().await;
    agent.set_current_turn_id(&turn_id);
}
tracing::info!(
    session_id = %session_id,
    turn_id = %turn_id,
    "turn started"
);
```

在 `run_multi_turn_stream_inner` **返回前**（成功/错误/`Done` 路径都覆盖）：

```rust
tracing::info!(session_id = %session_id /* 用 thread_id */, turn_id = %run_id, "turn finished");
{
    let mut agent = session.lock().await;
    agent.clear_current_turn_id();
}
```

对已有 `tracing::warn!` 的 LLM/tool 失败处，尽量补 `session_id` / `turn_id` 字段。

- [ ] **Step 3: `usage_record` 贯通**

```rust
pub(crate) fn build_llm_usage_event(
    agent_id: &str,
    session_id: Option<&str>,
    turn_id: Option<&str>,
    model: &str,
    usage: &Usage,
    provider: &str,
    base_url: &str,
    api_key: &str,
) -> (NewUsageEvent, BillingDelta) {
    // ...
    let event = NewUsageEvent {
        // ...
        session_id: session_id.map(str::to_string),
        turn_id: turn_id.map(str::to_string),
        // ...
    };
}
```

`apply_llm_usage_dual_write` 同样增加 `turn_id: Option<&str>` 并下传。

`streaming.rs` 的 `record_llm_usage`：

```rust
let turn_id = agent.current_turn_id().map(str::to_string);
apply_llm_usage_dual_write(
    &agent_id,
    Some(&session_id),
    turn_id.as_deref(),
    model,
    usage,
    ...
);
```

- [ ] **Step 4: `loop_.rs` 工具 / 其它 `NewUsageEvent`**

凡写入 UsageDb 的路径：`turn_id: self.current_turn_id.clone()`。

- [ ] **Step 5: 单元测试 usage_record**

更新 `build_llm_usage_event_*` 测试调用签名；可选断言 `event.turn_id == Some(...)`。

```bash
cargo test -p agent build_llm_usage_event -- --nocapture
cargo check -p agent
```

- [ ] **Step 6: Commit**

```bash
git add agent/src/loop_.rs agent/src/streaming.rs agent/src/usage_record.rs
git commit -m "$(cat <<'EOF'
feat(agent): bind turn_id to run_id across usage and tracing

EOF
)"
```

---

## Task 5: Tauri `query_agent_logs`

**Files:**
- Modify: `frontend/src-tauri/src/config_commands.rs`（或新建 `diagnostics_commands.rs` 并在 `lib.rs` mod）
- Modify: `frontend/src-tauri/src/lib.rs`（`generate_handler!`）

- [ ] **Step 1: Command**

```rust
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryAgentLogsArgs {
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub min_level: Option<String>,
    pub lines: Option<usize>,
    /// "agent" | "errors" | "both"
    pub source: Option<String>,
}

#[tauri::command]
pub async fn query_agent_logs(args: QueryAgentLogsArgs) -> Result<Vec<memory::AgentLogLine>, String> {
    let source = match args.source.as_deref() {
        Some("agent") => memory::LogSource::Agent,
        Some("errors") => memory::LogSource::Errors,
        _ => memory::LogSource::Both,
    };
    let q = memory::AgentLogQuery {
        logs_dir: memory::logs_dir(),
        session_id: args.session_id,
        turn_id: args.turn_id,
        min_level: args.min_level,
        lines: args.lines.unwrap_or(50),
        source,
    };
    memory::query_agent_logs(q).map_err(|e| e.to_string())
}
```

注册到 `lib.rs` 的 `invoke_handler`。

- [ ] **Step 2: Check**

```bash
cargo check -p astro-ui 2>/dev/null || cargo check --manifest-path frontend/src-tauri/Cargo.toml
```

- [ ] **Step 3: Commit**

```bash
git add frontend/src-tauri/src/config_commands.rs frontend/src-tauri/src/lib.rs
# 若新建 diagnostics_commands.rs 一并 add
git commit -m "$(cat <<'EOF'
feat(tauri): expose query_agent_logs for diagnostics UI

EOF
)"
```

---

## Task 6: Preferences「日志 / 诊断」UI + i18n

**Files:**
- Modify: `frontend/src/components/PreferencesPanel.tsx`
- Modify: `frontend/src/i18n/messages.ts`
- Optional: `frontend/src/styles/` 下 prefs 相关 CSS（优先复用 `prefs-card`）

- [ ] **Step 1: i18n keys**

在 `messages.ts` 英/中（及现有其它 locale，若有对称结构）增加：

```ts
"prefs.diag.title": "Logs / Diagnostics",
"prefs.diag.sub": "Filter recent agent logs by session or turn",
"prefs.diag.session": "Session ID",
"prefs.diag.turn": "Turn ID",
"prefs.diag.source": "Source",
"prefs.diag.source.agent": "agent.log",
"prefs.diag.source.errors": "errors.log",
"prefs.diag.source.both": "Both",
"prefs.diag.lines": "Lines",
"prefs.diag.refresh": "Refresh",
"prefs.diag.copy": "Copy all",
"prefs.diag.empty": "No matching log lines",
// zh:
"prefs.diag.title": "日志 / 诊断",
"prefs.diag.sub": "按会话或回合过滤最近 Agent 日志",
// ...
```

- [ ] **Step 2: Panel section**

在 `PreferencesPanel` 聊天偏好与语言之间（或语言之后）插入一节：

- `useState`：`sessionId`, `turnId`, `source` (`both`|`agent`|`errors`), `lines` (50), `rows`, `busy`, `error`
- Props 可选：`activeSessionId?: string` —— 若 App 能传入则初始填入；**没有则手动粘贴也可验收**
- `invoke<AgentLogLine[]>("query_agent_logs", { sessionId, turnId, source, lines })`（注意 Tauri camelCase）
- 列表：`<pre className="prefs-diag-log">` 每行 `[{source}] {raw}`
- 复制：`navigator.clipboard.writeText(rows.map(...).join("\n"))`

类型：

```ts
type AgentLogLine = { raw: string; source: string };
```

- [ ] **Step 3: 样式（最小）**

```css
.prefs-diag-log {
  max-height: 240px;
  overflow: auto;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: 12px;
  white-space: pre-wrap;
  margin: 0;
}
```

放入已有 prefs CSS 文件（搜 `.prefs-card` 所在 stylesheet）。

- [ ] **Step 4: 手测清单**

1. 跑一轮聊天 → `~/.astro/logs/agent.log` 出现 `session_id=` 与 `turn_id=`  
2. 偏好设置粘贴该 session → 刷新看到行  
3. 故意触发 warn（或看 errors）→ source=errors 有行  
4. Usage：抽查 `usage.db` 新 llm 行 `turn_id` 非空  

- [ ] **Step 5: Commit**

```bash
git add frontend/src/components/PreferencesPanel.tsx frontend/src/i18n/messages.ts frontend/src/styles/*.css
git commit -m "$(cat <<'EOF'
feat(ui): preferences log diagnostics filter by session/turn

EOF
)"
```

---

## Task 7: 端到端验收 + 收尾

- [ ] **Step 1: 回归测试**

```bash
cargo test -p memory --test usage_db_test
cargo test -p memory filters_by_session
cargo test -p agent build_llm_usage
```

- [ ] **Step 2: Spec 勾对**

对照 [`2026-07-14-observability-alignment-design.md`](../specs/2026-07-14-observability-alignment-design.md) S1：turn_id、双日志、query API、Preferences —— 均已落地；Insights 下钻未做。

- [ ] **Step 3: 更新 spec 状态行（可选）**

将 spec 头部 `状态: 已批准（待实现）` 改为 `状态: 已批准 / S1 已实现`（若本批完成）。

```bash
git add docs/superpowers/specs/2026-07-14-observability-alignment-design.md
git commit -m "$(cat <<'EOF'
docs: mark observability S1 implemented

EOF
)"
```

---

## Spec coverage self-check

| Spec 要求 | Task |
|-----------|------|
| `turn_id` 贯通 streaming/Usage/tracing | Task 4（+ Task 1 列） |
| `agent.log` / `errors.log` | Task 3 |
| Usage 安全 ADD COLUMN | Task 1 |
| `query_agent_logs` | Task 2 + 5 |
| Preferences 诊断 UI | Task 6 |
| 不做 Insights 下钻 / OTEL / EventBus | 全计划未包含 |
| 命名禁品牌串 | File map + commits |

**Placeholder scan:** 无 TBD；`NewUsageEvent` 调用点在 Task 1 Step 4 列全。

**Type consistency:** `turn_id: Option<String>`；`LogSource` / `AgentLogQuery` / `AgentLogLine`；Tauri args camelCase ↔ Rust serde；streaming `run_id` ≡ `turn_id`。
