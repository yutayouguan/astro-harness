# Usage Insights（洞察面板）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 侧边栏新增「洞察」页：用 SQLite 事件表记录今后的 tool/skill/mcp/cron/llm 用量，按月/季/年聚合展示调用、Token 与 LiteLLM 估算费用。

**Architecture:** `memory` 新增 `usage.db`（`usage_events`）与 `usage_pricing`（读 `~/.astro/litellm-model-meta.json` 估价）。现有 `record_usage_tool_call` 双写事件；MCP/cron/流式 FinalUsage 分别挂钩。Tauri 暴露 `get_usage_insights`；前端 `InsightsPanel` 用纯 CSS 柱状图（不引入图表库）。

**Tech Stack:** Rust / rusqlite / chrono / uuid、Tauri 2 invoke、React + 现有 i18n/CSS 体系

**Spec:** `docs/superpowers/specs/2026-07-13-usage-insights-design.md`

> **Plan status:** Completed on `main`（2026-07-13）。实现已落地；勿再按本计划重复开工。

---

## File map

| 文件 | 职责 |
|---|---|
| `crates/agent-memory/src/usage_db.rs` | SQLite 建库、insert、period 聚合查询 |
| `crates/agent-memory/src/usage_pricing.rs` | 从 LiteLLM 缓存 JSON 估算 `cost_usd` |
| `memory/tests/usage_db_test.rs` | DB + 聚合 + pricing 单测 |
| `crates/agent-memory/src/usage_stats.rs` | 双写：JSON 累计 + `usage_events`（tool/skill） |
| `crates/agent-memory/src/lib.rs` | 导出新模块 API |
| `crates/agent-core/src/loop_.rs` | MCP 调用额外写 `kind=mcp` |
| `crates/agent-core/src/streaming.rs` | FinalUsage 写 `kind=llm`（含估价） |
| `crates/agent-server/src/cron_runner.rs` | cron 执行完成写 `kind=cron` |
| `apps/desktop/src-tauri/src/config_commands.rs` | `get_usage_insights` 命令 |
| `apps/desktop/src-tauri/src/lib.rs` | 注册命令 |
| `apps/desktop/src-tauri/src/litellm_meta.rs` | 解析并保留 `input_cost_per_token` / `output_cost_per_token` |
| `apps/desktop/src/components/InsightsPanel.tsx` | 洞察 UI |
| `apps/desktop/src/styles/insights.css` | 样式 |
| `apps/desktop/src/components/NavIcons.tsx` | `IconInsights` |
| `apps/desktop/src/App.tsx` | NAV / PAGE_META / 挂载面板 |
| `apps/desktop/src/i18n/messages.ts` | 中英文案 |
| `apps/desktop/src/styles/index.css` | `@import` insights.css |

---

### Task 1: `UsageDb` 建库与 insert（TDD）

**Files:**
- Create: `crates/agent-memory/src/usage_db.rs`
- Create: `memory/tests/usage_db_test.rs`
- Modify: `crates/agent-memory/src/lib.rs`

- [x] **Step 1: 写失败测试（路径与 insert）**

```rust
//! usage.db 事件写入与聚合查询测试。

use memory::usage_db::{
    usage_db_path, NewUsageEvent, UsageDb, UsageInsightsQuery, UsagePeriod,
};
use tempfile::TempDir;

#[test]
fn usage_db_path_under_memory_dir() {
    let dir = TempDir::new().unwrap();
    std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
    assert_eq!(usage_db_path(), dir.path().join("usage.db"));
}

#[test]
fn insert_and_count_events() {
    let dir = TempDir::new().unwrap();
    let db = UsageDb::new(dir.path().join("usage.db")).unwrap();
    db.insert(NewUsageEvent {
        ts: "2026-07-13T02:00:00Z".into(),
        kind: "tool".into(),
        name: "terminal".into(),
        agent_id: "workspace".into(),
        session_id: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        cost_usd: 0.0,
        meta_json: None,
    })
    .unwrap();
    db.insert(NewUsageEvent {
        ts: "2026-07-13T03:00:00Z".into(),
        kind: "llm".into(),
        name: "gpt-4o-mini".into(),
        agent_id: "workspace".into(),
        session_id: Some("s1".into()),
        prompt_tokens: 100,
        completion_tokens: 50,
        total_tokens: 150,
        cost_usd: 0.001,
        meta_json: None,
    })
    .unwrap();
    let insights = db
        .query_insights(UsageInsightsQuery {
            period: UsagePeriod::Month,
            as_of: Some("2026-07-13T12:00:00Z".into()),
            agent_id: None,
        })
        .unwrap();
    assert_eq!(insights.kpis.calls, 2);
    assert_eq!(insights.kpis.tokens, 150);
    assert!((insights.kpis.cost_usd - 0.001).abs() < 1e-9);
    assert_eq!(insights.kpis.active_agents, 1);
}
```

- [x] **Step 2: 运行测试确认失败**

Run: `cargo test -p memory --test usage_db_test insert_and_count_events -- --nocapture`

Expected: 编译失败（模块/类型不存在）

- [x] **Step 3: 实现 `usage_db.rs`（最小可编译）**

在 `crates/agent-memory/src/usage_db.rs` 实现（对齐 `cron_run_db` / `artifact_db` 风格）：

```rust
//! 用量事件库：`~/.astro/usage.db` 记录 tool/skill/mcp/cron/llm 事件并按 period 聚合。

use anyhow::Context;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::workspace::default_memory_dir;

const DDL: &str = r#"
CREATE TABLE IF NOT EXISTS usage_events (
    id TEXT PRIMARY KEY,
    ts TEXT NOT NULL,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    session_id TEXT,
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    total_tokens INTEGER NOT NULL DEFAULT 0,
    cost_usd REAL NOT NULL DEFAULT 0,
    meta_json TEXT
);
CREATE INDEX IF NOT EXISTS idx_usage_ts ON usage_events(ts);
CREATE INDEX IF NOT EXISTS idx_usage_agent_ts ON usage_events(agent_id, ts);
CREATE INDEX IF NOT EXISTS idx_usage_kind_name_ts ON usage_events(kind, name, ts);
"#;

pub fn usage_db_path() -> PathBuf {
    default_memory_dir().join("usage.db")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UsagePeriod {
    Month,
    Quarter,
    Year,
}

#[derive(Debug, Clone)]
pub struct NewUsageEvent {
    pub ts: String,
    pub kind: String,
    pub name: String,
    pub agent_id: String,
    pub session_id: Option<String>,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub cost_usd: f64,
    pub meta_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageKpis {
    pub calls: u64,
    pub tokens: u64,
    pub cost_usd: f64,
    pub active_agents: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSeriesPoint {
    pub bucket: String,
    pub calls: u64,
    pub tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRankItem {
    pub kind: String,
    pub name: String,
    pub calls: u64,
    pub tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRankings {
    pub by_kind: Vec<UsageRankItem>,
    pub by_agent: Vec<UsageRankItem>,
    pub by_model: Vec<UsageRankItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInsights {
    pub kpis: UsageKpis,
    pub series: Vec<UsageSeriesPoint>,
    pub rankings: UsageRankings,
}

#[derive(Debug, Clone)]
pub struct UsageInsightsQuery {
    pub period: UsagePeriod,
    pub as_of: Option<String>,
    pub agent_id: Option<String>,
}

pub struct UsageDb {
    conn: Connection,
}

impl UsageDb {
    pub fn new(path: PathBuf) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path).context("open usage.db")?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(DDL)?;
        Ok(Self { conn })
    }

    pub fn open_default() -> anyhow::Result<Self> {
        Self::new(usage_db_path())
    }

    pub fn insert(&self, ev: NewUsageEvent) -> anyhow::Result<String> {
        let id = Uuid::new_v4().to_string();
        self.conn.execute(
            r#"INSERT INTO usage_events
            (id, ts, kind, name, agent_id, session_id, prompt_tokens, completion_tokens,
             total_tokens, cost_usd, meta_json)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)"#,
            params![
                id,
                ev.ts,
                ev.kind,
                ev.name,
                ev.agent_id,
                ev.session_id,
                ev.prompt_tokens,
                ev.completion_tokens,
                ev.total_tokens,
                ev.cost_usd,
                ev.meta_json,
            ],
        )?;
        Ok(id)
    }

    /// 尽力写入：打开/插入失败仅打日志风格由调用方 `let _ =` 忽略。
    pub fn try_record(ev: NewUsageEvent) {
        let Ok(db) = Self::open_default() else {
            return;
        };
        let _ = db.insert(ev);
    }

    pub fn query_insights(&self, q: UsageInsightsQuery) -> anyhow::Result<UsageInsights> {
        let (start, end, bucket_fmt) = period_bounds(q.period, q.as_of.as_deref())?;
        let agent = q
            .agent_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let kpis = self.query_kpis(&start, &end, agent)?;
        let series = self.query_series(&start, &end, agent, bucket_fmt)?;
        let by_kind = self.query_rank_by_kind(&start, &end, agent)?;
        let by_agent = self.query_rank_by_agent(&start, &end, agent)?;
        let by_model = self.query_rank_by_model(&start, &end, agent)?;

        Ok(UsageInsights {
            kpis,
            series,
            rankings: UsageRankings {
                by_kind,
                by_agent,
                by_model,
            },
        })
    }

    fn query_kpis(
        &self,
        start: &str,
        end: &str,
        agent: Option<&str>,
    ) -> anyhow::Result<UsageKpis> {
        // calls：tool/mcp/cron/llm（skill 为次级维度，不计入调用合计）
        let (calls, tokens, cost_usd, active_agents): (i64, i64, f64, i64) = if let Some(a) = agent {
            self.conn.query_row(
                r#"SELECT
                    COALESCE(SUM(CASE WHEN kind IN ('tool','mcp','cron','llm') THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(total_tokens),0),
                    COALESCE(SUM(cost_usd),0),
                    COUNT(DISTINCT agent_id)
                 FROM usage_events WHERE ts >= ?1 AND ts < ?2 AND agent_id = ?3"#,
                params![start, end, a],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?
        } else {
            self.conn.query_row(
                r#"SELECT
                    COALESCE(SUM(CASE WHEN kind IN ('tool','mcp','cron','llm') THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(total_tokens),0),
                    COALESCE(SUM(cost_usd),0),
                    COUNT(DISTINCT agent_id)
                 FROM usage_events WHERE ts >= ?1 AND ts < ?2"#,
                params![start, end],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?
        };
        Ok(UsageKpis {
            calls: calls as u64,
            tokens: tokens as u64,
            cost_usd,
            active_agents: active_agents as u64,
        })
    }

    fn query_series(
        &self,
        start: &str,
        end: &str,
        agent: Option<&str>,
        bucket_fmt: &str,
    ) -> anyhow::Result<Vec<UsageSeriesPoint>> {
        let sql = format!(
            r#"SELECT strftime('{bucket_fmt}', ts) AS bucket,
                      SUM(CASE WHEN kind IN ('tool','mcp','cron','llm') THEN 1 ELSE 0 END) AS calls,
                      COALESCE(SUM(total_tokens),0),
                      COALESCE(SUM(cost_usd),0)
               FROM usage_events
               WHERE ts >= ?1 AND ts < ?2 {agent_clause}
               GROUP BY bucket ORDER BY bucket"#,
            bucket_fmt = bucket_fmt,
            agent_clause = if agent.is_some() {
                "AND agent_id = ?3"
            } else {
                ""
            }
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let map_row = |r: &rusqlite::Row<'_>| {
            Ok(UsageSeriesPoint {
                bucket: r.get(0)?,
                calls: r.get::<_, i64>(1)? as u64,
                tokens: r.get::<_, i64>(2)? as u64,
                cost_usd: r.get(3)?,
            })
        };
        let rows = if let Some(a) = agent {
            stmt.query_map(params![start, end, a], map_row)?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map(params![start, end], map_row)?
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(rows)
    }

    fn query_rank_by_kind(
        &self,
        start: &str,
        end: &str,
        agent: Option<&str>,
    ) -> anyhow::Result<Vec<UsageRankItem>> {
        // tool/skill/mcp/cron；排除 llm（模型走 by_model）
        self.query_rank(
            start,
            end,
            agent,
            "kind IN ('tool','skill','mcp','cron')",
            "kind, name",
            true,
        )
    }

    fn query_rank_by_agent(
        &self,
        start: &str,
        end: &str,
        agent: Option<&str>,
    ) -> anyhow::Result<Vec<UsageRankItem>> {
        self.query_rank(
            start,
            end,
            agent,
            "kind IN ('tool','mcp','cron','llm')",
            "agent_id",
            false,
        )
    }

    fn query_rank_by_model(
        &self,
        start: &str,
        end: &str,
        agent: Option<&str>,
    ) -> anyhow::Result<Vec<UsageRankItem>> {
        self.query_rank(start, end, agent, "kind = 'llm'", "name", true)
    }

    fn query_rank(
        &self,
        start: &str,
        end: &str,
        agent: Option<&str>,
        kind_filter: &str,
        group_expr: &str,
        include_kind_col: bool,
    ) -> anyhow::Result<Vec<UsageRankItem>> {
        let (kind_select, name_select) = if include_kind_col && group_expr == "kind, name" {
            ("kind", "name")
        } else if group_expr == "agent_id" {
            ("'agent'", "agent_id")
        } else if group_expr == "name" {
            ("'llm'", "name")
        } else {
            ("kind", "name")
        };
        let sql = format!(
            r#"SELECT {kind_select} AS kind, {name_select} AS name,
                      COUNT(*) AS calls,
                      COALESCE(SUM(total_tokens),0),
                      COALESCE(SUM(cost_usd),0)
               FROM usage_events
               WHERE ts >= ?1 AND ts < ?2 AND ({kind_filter}) {agent_clause}
               GROUP BY {group_expr}
               ORDER BY calls DESC
               LIMIT 20"#,
            kind_select = kind_select,
            name_select = name_select,
            kind_filter = kind_filter,
            group_expr = group_expr,
            agent_clause = if agent.is_some() {
                "AND agent_id = ?3"
            } else {
                ""
            }
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let map_row = |r: &rusqlite::Row<'_>| {
            Ok(UsageRankItem {
                kind: r.get(0)?,
                name: r.get(1)?,
                calls: r.get::<_, i64>(2)? as u64,
                tokens: r.get::<_, i64>(3)? as u64,
                cost_usd: r.get(4)?,
            })
        };
        let rows = if let Some(a) = agent {
            stmt.query_map(params![start, end, a], map_row)?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map(params![start, end], map_row)?
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(rows)
    }
}

/// 返回 `[start, end)` ISO UTC 与 strftime 桶格式。
/// - month → 日桶 `%Y-%m-%d`
/// - quarter / year → 月桶 `%Y-%m`
fn period_bounds(
    period: UsagePeriod,
    as_of: Option<&str>,
) -> anyhow::Result<(String, String, &'static str)> {
    use chrono::{Datelike, TimeZone, Utc};
    let now = if let Some(s) = as_of {
        chrono::DateTime::parse_from_rfc3339(s)
            .map(|d| d.with_timezone(&Utc))
            .or_else(|_| {
                s.parse::<chrono::DateTime<Utc>>()
                    .map_err(|e| anyhow::anyhow!("invalid as_of: {e}"))
            })?
    } else {
        Utc::now()
    };
    let (start, end, fmt) = match period {
        UsagePeriod::Month => {
            let start = Utc
                .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
                .single()
                .ok_or_else(|| anyhow::anyhow!("bad month start"))?;
            let end = if now.month() == 12 {
                Utc.with_ymd_and_hms(now.year() + 1, 1, 1, 0, 0, 0)
                    .single()
            } else {
                Utc.with_ymd_and_hms(now.year(), now.month() + 1, 1, 0, 0, 0)
                    .single()
            }
            .ok_or_else(|| anyhow::anyhow!("bad month end"))?;
            (start, end, "%Y-%m-%d")
        }
        UsagePeriod::Quarter => {
            let q = ((now.month() - 1) / 3) * 3 + 1;
            let start = Utc
                .with_ymd_and_hms(now.year(), q, 1, 0, 0, 0)
                .single()
                .ok_or_else(|| anyhow::anyhow!("bad quarter start"))?;
            let end = if q + 3 > 12 {
                Utc.with_ymd_and_hms(now.year() + 1, 1, 1, 0, 0, 0)
                    .single()
            } else {
                Utc.with_ymd_and_hms(now.year(), q + 3, 1, 0, 0, 0)
                    .single()
            }
            .ok_or_else(|| anyhow::anyhow!("bad quarter end"))?;
            (start, end, "%Y-%m")
        }
        UsagePeriod::Year => {
            let start = Utc
                .with_ymd_and_hms(now.year(), 1, 1, 0, 0, 0)
                .single()
                .ok_or_else(|| anyhow::anyhow!("bad year start"))?;
            let end = Utc
                .with_ymd_and_hms(now.year() + 1, 1, 1, 0, 0, 0)
                .single()
                .ok_or_else(|| anyhow::anyhow!("bad year end"))?;
            (start, end, "%Y-%m")
        }
    };
    Ok((
        start.to_rfc3339(),
        end.to_rfc3339(),
        fmt,
    ))
}
```

注意：SQLite `strftime` 对带时区的 ISO 字符串解析不稳定。实现时把写入的 `ts` **统一存 UTC**（`chrono::Utc::now().to_rfc3339()`），且 `period_bounds` 返回的边界也用 UTC RFC3339；若实测 `strftime` 桶为空，改为在查询前把 `ts` 存成 `YYYY-MM-DDTHH:MM:SSZ` 无偏移形式，或用 `datetime(ts)` 包装。

在 `crates/agent-memory/src/lib.rs` 增加：

```rust
pub mod usage_db;
pub use usage_db::{
    usage_db_path, NewUsageEvent, UsageDb, UsageInsights, UsageInsightsQuery, UsagePeriod,
    UsageKpis, UsageRankItem, UsageRankings, UsageSeriesPoint,
};
```

- [x] **Step 4: 跑通 insert 测试**

Run: `cargo test -p memory --test usage_db_test -- --nocapture`

Expected: PASS

- [x] **Step 5: Commit**

```bash
git add memory/src/usage_db.rs memory/src/lib.rs memory/tests/usage_db_test.rs
git commit -m "$(cat <<'EOF'
feat(memory): add usage.db event store and insights query

EOF
)"
```

---

### Task 2: period 过滤与排行单测

**Files:**
- Modify: `memory/tests/usage_db_test.rs`

- [x] **Step 1: 追加测试**

```rust
#[test]
fn filters_by_agent_and_excludes_out_of_range() {
    let dir = TempDir::new().unwrap();
    let db = UsageDb::new(dir.path().join("usage.db")).unwrap();
    for (ts, agent, kind, name) in [
        ("2026-07-01T10:00:00Z", "workspace", "tool", "terminal"),
        ("2026-07-02T10:00:00Z", "research", "tool", "web_search"),
        ("2026-06-01T10:00:00Z", "workspace", "tool", "terminal"), // 上月
    ] {
        db.insert(NewUsageEvent {
            ts: ts.into(),
            kind: kind.into(),
            name: name.into(),
            agent_id: agent.into(),
            session_id: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            cost_usd: 0.0,
            meta_json: None,
        })
        .unwrap();
    }
    let all = db
        .query_insights(UsageInsightsQuery {
            period: UsagePeriod::Month,
            as_of: Some("2026-07-15T00:00:00Z".into()),
            agent_id: None,
        })
        .unwrap();
    assert_eq!(all.kpis.calls, 2);
    assert_eq!(all.kpis.active_agents, 2);

    let one = db
        .query_insights(UsageInsightsQuery {
            period: UsagePeriod::Month,
            as_of: Some("2026-07-15T00:00:00Z".into()),
            agent_id: Some("workspace".into()),
        })
        .unwrap();
    assert_eq!(one.kpis.calls, 1);
    assert_eq!(one.rankings.by_kind[0].name, "terminal");
}

#[test]
fn skill_events_do_not_inflate_kpi_calls() {
    let dir = TempDir::new().unwrap();
    let db = UsageDb::new(dir.path().join("usage.db")).unwrap();
    db.insert(NewUsageEvent {
        ts: "2026-07-13T01:00:00Z".into(),
        kind: "tool".into(),
        name: "skills".into(),
        agent_id: "workspace".into(),
        session_id: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        cost_usd: 0.0,
        meta_json: None,
    })
    .unwrap();
    db.insert(NewUsageEvent {
        ts: "2026-07-13T01:00:01Z".into(),
        kind: "skill".into(),
        name: "demo".into(),
        agent_id: "workspace".into(),
        session_id: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        cost_usd: 0.0,
        meta_json: None,
    })
    .unwrap();
    let insights = db
        .query_insights(UsageInsightsQuery {
            period: UsagePeriod::Month,
            as_of: Some("2026-07-13T12:00:00Z".into()),
            agent_id: None,
        })
        .unwrap();
    assert_eq!(insights.kpis.calls, 1);
    assert!(insights
        .rankings
        .by_kind
        .iter()
        .any(|r| r.kind == "skill" && r.name == "demo"));
}
```

- [x] **Step 2: 运行测试**

Run: `cargo test -p memory --test usage_db_test -- --nocapture`

Expected: PASS（若 period 边界或 strftime 失败则修 `period_bounds` / ts 格式）

- [x] **Step 3: Commit**

```bash
git add memory/tests/usage_db_test.rs memory/src/usage_db.rs
git commit -m "$(cat <<'EOF'
test(memory): cover usage insights filters and skill kpi

EOF
)"
```

---

### Task 3: LiteLLM 估价 + 保留单价字段

**Files:**
- Create: `crates/agent-memory/src/usage_pricing.rs`
- Modify: `crates/agent-memory/src/lib.rs`
- Modify: `memory/tests/usage_db_test.rs`
- Modify: `apps/desktop/src-tauri/src/litellm_meta.rs`

- [x] **Step 1: 写 pricing 失败测试**

```rust
#[test]
fn estimate_cost_from_litellm_fixture() {
    let dir = TempDir::new().unwrap();
    std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
    std::fs::write(
        dir.path().join("litellm-model-meta.json"),
        r#"{
          "gpt-4o-mini": {
            "input_cost_per_token": 0.00000015,
            "output_cost_per_token": 0.0000006,
            "max_input_tokens": 128000
          }
        }"#,
    )
    .unwrap();
    let cost = memory::estimate_llm_cost("gpt-4o-mini", 1_000_000, 1_000_000);
    // 0.15 + 0.6 = 0.75
    assert!((cost - 0.75).abs() < 1e-9);
    assert_eq!(memory::estimate_llm_cost("unknown-model", 100, 100), 0.0);
}
```

- [x] **Step 2: 实现 `usage_pricing.rs`**

```rust
//! 从 `~/.astro/litellm-model-meta.json` 读取单价并估算 LLM 费用（USD）。

use serde_json::Value;
use std::fs;
use std::sync::Mutex;

use crate::workspace::default_memory_dir;

fn cache_path() -> std::path::PathBuf {
    default_memory_dir().join("litellm-model-meta.json")
}

/// 估算：`prompt * input_cost_per_token + completion * output_cost_per_token`。
/// 未知模型或缺字段返回 `0.0`。
pub fn estimate_llm_cost(model: &str, prompt_tokens: u32, completion_tokens: u32) -> f64 {
    let model = model.trim();
    if model.is_empty() {
        return 0.0;
    }
    let Ok(raw) = fs::read_to_string(cache_path()) else {
        return 0.0;
    };
    let Ok(v): Result<Value, _> = serde_json::from_str(&raw) else {
        return 0.0;
    };
    let Some(obj) = v.as_object() else {
        return 0.0;
    };
    // 精确键或后缀匹配（provider/model）
    let entry = obj.get(model).or_else(|| {
        obj.iter()
            .find(|(k, _)| k.ends_with(&format!("/{model}")) || *k == model)
            .map(|(_, v)| v)
    });
    let Some(entry) = entry else {
        return 0.0;
    };
    let input = entry
        .get("input_cost_per_token")
        .and_then(|x| x.as_f64())
        .unwrap_or(0.0);
    let output = entry
        .get("output_cost_per_token")
        .and_then(|x| x.as_f64())
        .unwrap_or(0.0);
    input * f64::from(prompt_tokens) + output * f64::from(completion_tokens)
}
```

导出：`pub use usage_pricing::estimate_llm_cost;`

- [x] **Step 3: 扩展 `litellm_meta.rs` 的 `RawEntry`**

在 `RawEntry` 增加：

```rust
#[serde(default)]
input_cost_per_token: Option<f64>,
#[serde(default)]
output_cost_per_token: Option<f64>,
```

在 `LiteLlmEntry` 增加同名字段；`into_entry` 映射它们。

修改 `parse_map` 保留条件：若存在 `input_cost_per_token` 或 `output_cost_per_token` 也保留条目（不要再跳过「纯定价」项）。

- [x] **Step 4: 跑测试**

Run: `cargo test -p memory --test usage_db_test estimate_cost_from_litellm_fixture -- --nocapture`

Expected: PASS

- [x] **Step 5: Commit**

```bash
git add memory/src/usage_pricing.rs memory/src/lib.rs memory/tests/usage_db_test.rs apps/desktop/src-tauri/src/litellm_meta.rs
git commit -m "$(cat <<'EOF'
feat: estimate LLM cost from LiteLLM price cache

EOF
)"
```

---

### Task 4: 双写 tool / skill 事件

**Files:**
- Modify: `crates/agent-memory/src/usage_stats.rs`

- [x] **Step 1: 在 `record_tool_call` 成功写 JSON 后追加事件**

在 `save_usage_stats(...)?` 之前或之后（推荐 save 成功后）调用：

```rust
use crate::usage_db::{NewUsageEvent, UsageDb};
use crate::tools_enabled::tool_name_to_toolset;

// 在 save_usage_stats 成功后：
let ts = chrono::Utc::now().to_rfc3339();
let toolset = tool_name_to_toolset(tool_name).to_string();
UsageDb::try_record(NewUsageEvent {
    ts: ts.clone(),
    kind: "tool".into(),
    name: toolset,
    agent_id: id.clone(),
    session_id: None,
    prompt_tokens: 0,
    completion_tokens: 0,
    total_tokens: 0,
    cost_usd: 0.0,
    meta_json: Some(serde_json::json!({ "tool": tool_name }).to_string()),
});
if tool_name == "skills" {
    if let Some(skill_id) = args
        .get("skill_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        UsageDb::try_record(NewUsageEvent {
            ts,
            kind: "skill".into(),
            name: skill_id.to_string(),
            agent_id: id,
            session_id: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            cost_usd: 0.0,
            meta_json: None,
        });
    }
}
```

保留原有 JSON 逻辑与返回值不变。

- [x] **Step 2: 扩展 `usage_stats` 现有测试**（设 `ASTRO_MEMORY_DIR` 后 `record_tool_call`，再 `UsageDb::open_default().query_insights` 断言有 tool/skill 行）

- [x] **Step 3: 运行**

Run: `cargo test -p memory --lib records_toolset_and_skill_counts -- --nocapture`

Expected: PASS

- [x] **Step 4: Commit**

```bash
git add memory/src/usage_stats.rs
git commit -m "$(cat <<'EOF'
feat(memory): dual-write usage events for tools and skills

EOF
)"
```

---

### Task 5: MCP + cron 写入钩子

**Files:**
- Modify: `crates/agent-core/src/loop_.rs`（MCP 分支，约 353–362 行）
- Modify: `crates/agent-server/src/cron_runner.rs`

- [x] **Step 1: MCP — 在已有 `record_usage_tool_call` 旁追加**

```rust
if is_mcp_tool_name(name) {
    // ... existing allow checks ...
    let agent_id = self.memory.agent_id.clone();
    let _ = memory::record_tool_call(&agent_id, name, args);
    let _ = memory::record_usage_tool_call(&agent_id, name, args);
    memory::UsageDb::try_record(memory::NewUsageEvent {
        ts: chrono::Utc::now().to_rfc3339(),
        kind: "mcp".into(),
        name: name.to_string(),
        agent_id,
        session_id: self.session_id.clone(),
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        cost_usd: 0.0,
        meta_json: None,
    });
    return self.mcp_hub.call_tool(name, args).await;
}
```

说明：`record_usage_tool_call` 已写 `kind=tool name=mcp`；此处额外写 `kind=mcp` 用完整工具名，供排行。KPI calls 会计 tool+mcp 各 1——若不想双计，改为 MCP 路径**跳过** `record_usage_tool_call` 的 tool 事件、只 bump JSON，或让 `usage_stats` 对 `mcp__` 前缀不写 tool 事件、只写 JSON。**推荐：** 在 `usage_stats::record_tool_call` 里若 `tool_name.starts_with("mcp__")` 则**只更新 JSON、不写 usage_events tool 行**，由 loop 写 `kind=mcp` 事件。

- [x] **Step 2: 按推荐改 `usage_stats`**

```rust
let is_mcp = tool_name.starts_with("mcp__");
// ... JSON bump 照旧 ...
save_usage_stats(...)?;
if !is_mcp {
    // try_record tool (+ skill) as in Task 4
}
Ok(())
```

- [x] **Step 3: cron — 在 `tick_and_execute` 成功分支**

```rust
Ok(row) => {
    memory::UsageDb::try_record(memory::NewUsageEvent {
        ts: chrono::Utc::now().to_rfc3339(),
        kind: "cron".into(),
        name: job.id.clone(),
        agent_id: job.agent_id.clone(),
        session_id: row.session_id.clone(),
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        cost_usd: 0.0,
        meta_json: Some(
            serde_json::json!({ "title": job.title, "trigger": "due" }).to_string(),
        ),
    });
    tracing::info!(...);
}
```

确认 `CronJob` 有 `agent_id` / `title` 字段；若字段名不同则按实际结构调整。

- [x] **Step 4: `cargo check -p agent -p backend`**

Expected: 成功

- [x] **Step 5: Commit**

```bash
git add agent/src/loop_.rs backend/src/cron_runner.rs memory/src/usage_stats.rs
git commit -m "$(cat <<'EOF'
feat: record mcp and cron usage events

EOF
)"
```

---

### Task 6: LLM FinalUsage 写入

**Files:**
- Modify: `crates/agent-core/src/streaming.rs`
- Modify: `crates/agent-core/src/loop_.rs`（新增 `agent_id()`）

- [x] **Step 1: 在 `AgentLoop` 增加只读访问器（`memory` / `session_id` 均为私有）**

在 `crates/agent-core/src/loop_.rs`：

```rust
pub fn agent_id(&self) -> &str {
    &self.memory.agent_id
}
```

（已有 `session_id(&self) -> &str`。）

- [x] **Step 2: 在 `finish_usage_and_done` 调用前记录**

在 `run_multi_turn_stream` 中，每次准备 `finish_usage_and_done(&tx, saw_usage.then_some(total_usage))` 之前（含正常结束与取消路径），若 `saw_usage`：

```rust
{
    let agent = session.lock().await;
    let agent_id = agent.agent_id().to_string();
    let session_id = Some(agent.session_id().to_string());
    drop(agent);
    let model = config.model.clone();
    let prompt = total_usage.prompt_tokens;
    let completion = total_usage.completion_tokens;
    let total = total_usage.total_tokens;
    let cost = memory::estimate_llm_cost(&model, prompt, completion);
    memory::UsageDb::try_record(memory::NewUsageEvent {
        ts: chrono::Utc::now().to_rfc3339(),
        kind: "llm".into(),
        name: model,
        agent_id,
        session_id,
        prompt_tokens: i64::from(prompt),
        completion_tokens: i64::from(completion),
        total_tokens: i64::from(total),
        cost_usd: cost,
        meta_json: None,
    });
}
```

`ProviderConfig.model` 已存在。只在流真正结束时记**一次累计** usage（不要每轮 `round_usage` 都记）。

- [x] **Step 3: `cargo test -p agent --test streaming_test`**

Expected: PASS（行为不变，仅旁路记账）

- [x] **Step 4: Commit**

```bash
git add agent/src/streaming.rs agent/src/loop_.rs
git commit -m "$(cat <<'EOF'
feat(agent): record llm usage events with estimated cost

EOF
)"
```

---

### Task 7: Tauri `get_usage_insights`

**Files:**
- Modify: `apps/desktop/src-tauri/src/config_commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

- [x] **Step 1: 添加命令**

```rust
#[derive(Debug, Deserialize)]
pub struct UsageInsightsArgs {
    pub period: String, // month | quarter | year
    pub as_of: Option<String>,
    pub agent_id: Option<String>,
}

#[tauri::command]
pub async fn get_usage_insights(
    args: UsageInsightsArgs,
) -> Result<memory::UsageInsights, String> {
    let period = match args.period.to_lowercase().as_str() {
        "month" => memory::UsagePeriod::Month,
        "quarter" => memory::UsagePeriod::Quarter,
        "year" => memory::UsagePeriod::Year,
        other => return Err(format!("invalid period: {other}")),
    };
    let agent_id = normalize_agent_id(args.agent_id);
    let db = memory::UsageDb::open_default().map_err(|e| e.to_string())?;
    db.query_insights(memory::UsageInsightsQuery {
        period,
        as_of: args.as_of,
        agent_id,
    })
    .map_err(|e| e.to_string())
}
```

在 `lib.rs` 的 `invoke_handler` 注册 `get_usage_insights`。

- [x] **Step 2: `cargo check -p astro-frontend`（或 workspace 中 tauri crate 名）**

Expected: 成功

- [x] **Step 3: Commit**

```bash
git add apps/desktop/src-tauri/src/config_commands.rs apps/desktop/src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(tauri): expose get_usage_insights command

EOF
)"
```

---

### Task 8: 前端 InsightsPanel + 导航

**Files:**
- Create: `apps/desktop/src/components/InsightsPanel.tsx`
- Create: `apps/desktop/src/styles/insights.css`
- Modify: `apps/desktop/src/styles/index.css`
- Modify: `apps/desktop/src/components/NavIcons.tsx`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/i18n/messages.ts`

- [x] **Step 1: 图标**

在 `NavIcons.tsx` 增加柱状图风格 `IconInsights`（与现有 `IconBase` 一致）。

- [x] **Step 2: i18n（中英）**

```ts
"nav.insights": "洞察",
"page.insights.title": "用量洞察",
"page.insights.sub": "调用次数、Token 与费用估算",
"insights.period.month": "月",
"insights.period.quarter": "季",
"insights.period.year": "年",
"insights.kpi.calls": "调用合计",
"insights.kpi.tokens": "Tokens",
"insights.kpi.cost": "费用（估）",
"insights.kpi.agents": "活跃 Agent",
"insights.rank.kind": "工具 / 技能 / MCP / 定时",
"insights.rank.agent": "按 Agent",
"insights.rank.model": "按模型",
"insights.empty": "上线后开始累计。新的工具调用与对话用量会出现在这里。",
"insights.unpriced": "部分模型未计价",
"insights.allAgents": "全部 Agent",
// English 对应键同名
```

- [x] **Step 3: `InsightsPanel.tsx` 骨架**

```tsx
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { useLocale } from "../i18n/LocaleContext";
// list agents: 复用现有 invoke list_agents 或父组件传入

type Period = "month" | "quarter" | "year";

type UsageInsights = {
  kpis: { calls: number; tokens: number; cost_usd: number; active_agents: number };
  series: { bucket: string; calls: number; tokens: number; cost_usd: number }[];
  rankings: {
    by_kind: { kind: string; name: string; calls: number; tokens: number; cost_usd: number }[];
    by_agent: { kind: string; name: string; calls: number; tokens: number; cost_usd: number }[];
    by_model: { kind: string; name: string; calls: number; tokens: number; cost_usd: number }[];
  };
};

export default function InsightsPanel({ active }: { active: boolean }) {
  const { t } = useLocale();
  const [period, setPeriod] = useState<Period>("month");
  const [agentId, setAgentId] = useState<string | null>(null);
  const [data, setData] = useState<UsageInsights | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    (async () => {
      try {
        const res = await invoke<UsageInsights>("get_usage_insights", {
          args: { period, as_of: null, agent_id: agentId },
        });
        if (!cancelled) {
          setData(res);
          setError(null);
        }
      } catch (e) {
        if (!cancelled) setError(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [active, period, agentId]);

  const maxCalls = Math.max(1, ...(data?.series.map((s) => s.calls) ?? [1]));
  const empty = data && data.kpis.calls === 0 && data.kpis.tokens === 0;

  return (
    <div className="insights-panel">
      {/* period tabs + agent select */}
      {/* KPI cards */}
      {/* CSS bar chart from data.series */}
      {/* ranking lists */}
      {empty && <p className="insights-empty">{t("insights.empty")}</p>}
      {error && <p className="insights-error">{error}</p>}
    </div>
  );
}
```

样式对齐 `tools.css` / `cron.css` 的面板间距与卡片；柱状图用 `flex` + 百分比 `height`，**不**加 chart 依赖。

- [x] **Step 4: 接入 App**

- `NavId` 增加 `"insights"`
- `NAV` 在 `tools` 与 `cron` 之间插入 insights（tone: `"amber"`）
- `PAGE_META` 增加对应键
- 主内容区：`{nav === "insights" && <InsightsPanel active={nav === "insights"} />}`
- import 图标与面板；`index.css` `@import "./insights.css"`

- [x] **Step 5: 类型检查**

Run: `cd frontend && npx tsc -b --pretty false`

Expected: 无错误

- [x] **Step 6: Commit**

```bash
git add apps/desktop/src/components/InsightsPanel.tsx apps/desktop/src/styles/insights.css \
  apps/desktop/src/styles/index.css apps/desktop/src/components/NavIcons.tsx \
  apps/desktop/src/App.tsx apps/desktop/src/i18n/messages.ts
git commit -m "$(cat <<'EOF'
feat(frontend): add Insights sidebar panel

EOF
)"
```

---

### Task 9: 端到端冒烟与验收对照

**Files:** 无新文件（必要时微调）

- [x] **Step 1: 单元回归**

Run:

```bash
cargo test -p memory --test usage_db_test
cargo check -p agent -p backend -p memory
cd frontend && npx tsc -b
```

Expected: 全绿

- [x] **Step 2: 手工冒烟（可选，有桌面环境时）**

1. 启动 App，打开「洞察」→ 见空态文案  
2. 聊天触发工具 / 一轮对话 → 刷新洞察 → KPI / 排行出现数据  
3. 切换 月/季/年 → series 桶粒度变化  
4. Tools 面板累计 chip 仍可用  

- [x] **Step 3: 对照 spec 验收清单勾选**

- [x] 侧边栏洞察 + period 切换  
- [x] tool/skill/mcp/cron/llm 新事件可见  
- [x] 费用标注「估」；未知模型不炸  
- [x] 不回填；旧 JSON 保留  

- [x] **Step 4: 若有小修，单独 commit；否则完成**

---

## Self-review (plan vs spec)

| Spec 要求 | Task |
|---|---|
| SQLite `usage_events` + 索引 | Task 1 |
| 月/季/年聚合、Agent 筛选 | Task 1–2 |
| 只记今后、不回填 | 全程无回填逻辑 |
| 双写 JSON + events | Task 4 |
| tool/skill/mcp/cron/llm 写入 | Task 4–6 |
| LiteLLM 估价 | Task 3 + 6 |
| `get_usage_insights` | Task 7 |
| Insights UI + 空态 | Task 8 |
| 无 3D / 无自定义单价 / 无协作图 | 未列入 |
| 测试要点 | Task 1–2、3、9 |

**类型一致性：** `UsagePeriod` / `UsageInsights` / `NewUsageEvent` / `get_usage_insights` args 在 Task 1 与 7–8 对齐。

**KPI 口径：** `calls` 不含 `skill` 行，避免与 tool 双计；MCP 推荐只写 `kind=mcp` 事件（JSON 仍 bump）。
