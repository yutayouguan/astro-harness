//! 用量事件库：SQLite 持久化与按月/季/年聚合查询。
//!
//! 职责：
//! - 在 `~/.astro/usage.db` 记录 tool / skill / mcp / cron / llm 用量事件
//! - 提供 insert、try_record 与按 period / agent 的洞察聚合
//!
//! 不变量：
//! - KPI `calls` 仅统计 `kind IN ('tool','mcp','cron','llm')`；`skill` 不计入 calls
//! - 时间窗为半开区间 `[start, end)`（UTC RFC3339）
//! - 使用 WAL 模式；`id` 为主键 UUID

use chrono::{Datelike, SecondsFormat, TimeZone, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// `usage.db` schema 版本；v3→v4 使用 ADD COLUMN 非破坏性迁移。
pub const USAGE_SCHEMA_VERSION: i32 = 4;

/// 建表 DDL（`usage_events` 及 ts / agent / kind 索引）
const DDL: &str = r#"
CREATE TABLE IF NOT EXISTS usage_events (
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
    meta_json TEXT,
    turn_id TEXT
);
CREATE INDEX IF NOT EXISTS idx_usage_ts ON usage_events(ts);
CREATE INDEX IF NOT EXISTS idx_usage_agent_ts ON usage_events(agent_id, ts);
CREATE INDEX IF NOT EXISTS idx_usage_kind_name_ts ON usage_events(kind, name, ts);
"#;

/// KPI `calls` 计入的 kind 集合（`skill` 除外）
const CALLS_KIND_SQL: &str =
    "CASE WHEN kind IN ('tool','mcp','cron','llm') THEN 1 ELSE 0 END";

/// KPI `cost_usd` 聚合：排除 `cost_status='unknown'`
const COST_SUM_SQL: &str =
    "CASE WHEN cost_status IS NULL OR cost_status IN ('estimated','included') THEN cost_usd ELSE 0 END";

/// 将 ISO8601（含 `T` / `Z`）规范为 SQLite `datetime` 可解析形式
const TS_NORM_SQL: &str = "replace(replace(ts, 'T', ' '), 'Z', '')";

/// 洞察时间粒度
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UsagePeriod {
    Month,
    Quarter,
    Year,
}

/// 插入用量事件时的输入（不含 id，由 DB 生成）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewUsageEvent {
    pub ts: String,
    pub kind: String,
    pub name: String,
    pub agent_id: String,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    pub total_tokens: i64,
    pub cost_usd: f64,
    pub cost_status: Option<String>,
    pub cost_source: Option<String>,
    pub pricing_version: Option<String>,
    pub billing_provider: Option<String>,
    pub billing_base_url: Option<String>,
    pub billing_mode: Option<String>,
    pub meta_json: Option<String>,
}

/// KPI 汇总
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageKpis {
    pub calls: i64,
    pub tokens: i64,
    pub cost_usd: f64,
    pub active_agents: i64,
}

/// 时间序列上的一个桶
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageSeriesPoint {
    pub bucket: String,
    pub calls: i64,
    pub tokens: i64,
    pub cost_usd: f64,
}

/// 排行榜单项
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageRankItem {
    pub kind: String,
    pub name: String,
    pub calls: i64,
    pub tokens: i64,
    pub cost_usd: f64,
}

/// 多维排行
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageRankings {
    pub by_kind: Vec<UsageRankItem>,
    pub by_agent: Vec<UsageRankItem>,
    pub by_model: Vec<UsageRankItem>,
}

/// Tracing：会话级聚合行
#[derive(Debug, Clone)]
pub struct TraceSessionRow {
    pub session_id: String,
    pub agent_id: String,
    pub started_at: String,
    pub ended_at: String,
    pub event_count: i64,
    pub tokens: i64,
    pub cost_usd: f64,
}

/// Tracing：单事件行
#[derive(Debug, Clone)]
pub struct TraceEventRow {
    pub id: String,
    pub ts: String,
    pub kind: String,
    pub name: String,
    pub agent_id: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub cost_usd: f64,
}

/// 洞察查询完整结果
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageInsights {
    pub kpis: UsageKpis,
    pub series: Vec<UsageSeriesPoint>,
    pub rankings: UsageRankings,
    /// `kind=llm` 且 `cost_status='unknown'` 的事件数
    pub unpriced_llm_events: i64,
}

/// 洞察查询参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInsightsQuery {
    pub period: UsagePeriod,
    /// 锚定时刻（RFC3339）；缺省为当前 UTC
    pub as_of: Option<String>,
    /// 按 Agent 过滤；`None` 表示全部
    pub agent_id: Option<String>,
}

/// 用量事件 SQLite 访问层
pub struct UsageDb {
    conn: Connection,
}

/// 默认数据库路径：`{ASTRO_MEMORY_DIR|~/.astro}/usage.db`
pub fn usage_db_path() -> PathBuf {
    crate::workspace::default_memory_dir().join("usage.db")
}

/// UTC 边界格式化为 `YYYY-MM-DDTHH:MM:SSZ`，便于与事件 `ts` 做字典序比较
fn fmt_utc_bound(dt: chrono::DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// 时间窗与分桶格式：`[start, end)` UTC RFC3339 + strftime 格式
fn period_bounds(period: UsagePeriod, as_of: &chrono::DateTime<Utc>) -> (String, String, &'static str) {
    let y = as_of.year();
    let m = as_of.month();
    match period {
        UsagePeriod::Month => {
            let start = Utc
                .with_ymd_and_hms(y, m, 1, 0, 0, 0)
                .single()
                .expect("valid month start");
            let end = if m == 12 {
                Utc.with_ymd_and_hms(y + 1, 1, 1, 0, 0, 0)
                    .single()
                    .expect("valid next year")
            } else {
                Utc.with_ymd_and_hms(y, m + 1, 1, 0, 0, 0)
                    .single()
                    .expect("valid next month")
            };
            (fmt_utc_bound(start), fmt_utc_bound(end), "%Y-%m-%d")
        }
        UsagePeriod::Quarter => {
            let q_start_month = ((m - 1) / 3) * 3 + 1;
            let start = Utc
                .with_ymd_and_hms(y, q_start_month, 1, 0, 0, 0)
                .single()
                .expect("valid quarter start");
            let end = if q_start_month + 3 > 12 {
                Utc.with_ymd_and_hms(y + 1, 1, 1, 0, 0, 0)
                    .single()
                    .expect("valid next year")
            } else {
                Utc.with_ymd_and_hms(y, q_start_month + 3, 1, 0, 0, 0)
                    .single()
                    .expect("valid next quarter")
            };
            (fmt_utc_bound(start), fmt_utc_bound(end), "%Y-%m")
        }
        UsagePeriod::Year => {
            let start = Utc
                .with_ymd_and_hms(y, 1, 1, 0, 0, 0)
                .single()
                .expect("valid year start");
            let end = Utc
                .with_ymd_and_hms(y + 1, 1, 1, 0, 0, 0)
                .single()
                .expect("valid next year");
            (fmt_utc_bound(start), fmt_utc_bound(end), "%Y-%m")
        }
    }
}

fn parse_as_of(as_of: Option<&str>) -> anyhow::Result<chrono::DateTime<Utc>> {
    match as_of {
        Some(s) if !s.is_empty() => chrono::DateTime::parse_from_rfc3339(s)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| anyhow::anyhow!("invalid as_of RFC3339: {e}")),
        _ => Ok(Utc::now()),
    }
}

/// 返回 period 半开区间 `[start, end)` 的 RFC3339 UTC 字符串（与 query_insights 一致）。
pub fn period_window(
    period: UsagePeriod,
    as_of: Option<&str>,
) -> anyhow::Result<(String, String)> {
    let as_of_dt = parse_as_of(as_of)?;
    let (start, end, _) = period_bounds(period, &as_of_dt);
    Ok((start, end))
}

/// 将事件时间戳规范为 `…Z`（秒精度），以便与 `period_bounds` 做字典序比较。
/// 解析失败时保留原字符串。
fn normalize_event_ts(ts: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(ts) {
        Ok(dt) => dt
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Secs, true),
        Err(_) => ts.to_string(),
    }
}

/// 生成 `[start, end)` 内全部时间桶标签（日或月）
fn all_buckets(start: &str, end: &str, fmt: &str) -> anyhow::Result<Vec<String>> {
    let start_dt = chrono::DateTime::parse_from_rfc3339(start)?.with_timezone(&Utc);
    let end_dt = chrono::DateTime::parse_from_rfc3339(end)?.with_timezone(&Utc);
    let mut out = Vec::new();
    match fmt {
        "%Y-%m-%d" => {
            let mut d = start_dt.date_naive();
            let end_d = end_dt.date_naive();
            while d < end_d {
                out.push(d.format("%Y-%m-%d").to_string());
                d = d
                    .succ_opt()
                    .ok_or_else(|| anyhow::anyhow!("date overflow filling day buckets"))?;
            }
        }
        "%Y-%m" => {
            let mut y = start_dt.year();
            let mut m = start_dt.month();
            let end_y = end_dt.year();
            let end_m = end_dt.month();
            while y < end_y || (y == end_y && m < end_m) {
                out.push(format!("{y:04}-{m:02}"));
                if m == 12 {
                    y += 1;
                    m = 1;
                } else {
                    m += 1;
                }
            }
        }
        other => anyhow::bail!("unsupported bucket format: {other}"),
    }
    Ok(out)
}

fn delete_usage_db_files(path: &Path) {
    let base = path.to_string_lossy();
    for p in [
        path.to_path_buf(),
        PathBuf::from(format!("{base}-wal")),
        PathBuf::from(format!("{base}-shm")),
    ] {
        let _ = std::fs::remove_file(p);
    }
}

fn open_and_init(path: &Path) -> anyhow::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch(DDL)?;
    conn.execute_batch(&format!(
        "PRAGMA user_version = {USAGE_SCHEMA_VERSION};"
    ))?;
    Ok(conn)
}

impl UsageDb {
    /// 打开或创建数据库；v3→v4 非破坏性迁移，仅 version<3 或新库时重建
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
            anyhow::bail!(
                "usage.db schema version {version} is newer than supported {USAGE_SCHEMA_VERSION}"
            );
        }

        if version < 3 {
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
        Ok(Self { conn })
    }

    /// 打开默认 `~/.astro/usage.db`
    pub fn open_default() -> anyhow::Result<Self> {
        Self::new(usage_db_path())
    }

    /// 插入一条用量事件，返回新 id
    pub fn insert(&self, row: NewUsageEvent) -> anyhow::Result<String> {
        let id = Uuid::new_v4().to_string();
        let ts = normalize_event_ts(&row.ts);
        self.conn.execute(
            "INSERT INTO usage_events (
                id, ts, kind, name, agent_id, session_id, turn_id,
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                reasoning_tokens, total_tokens, cost_usd,
                cost_status, cost_source, pricing_version,
                billing_provider, billing_base_url, billing_mode, meta_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
            params![
                id,
                ts,
                row.kind,
                row.name,
                row.agent_id,
                row.session_id,
                row.turn_id,
                row.input_tokens,
                row.output_tokens,
                row.cache_read_tokens,
                row.cache_write_tokens,
                row.reasoning_tokens,
                row.total_tokens,
                row.cost_usd,
                row.cost_status,
                row.cost_source,
                row.pricing_version,
                row.billing_provider,
                row.billing_base_url,
                row.billing_mode,
                row.meta_json,
            ],
        )?;
        Ok(id)
    }

    /// 列出时间窗内 `kind=orchestration` 事件的 `meta_json`（供协作图聚合）
    pub fn list_orchestration_meta(
        &self,
        start: &str,
        end: &str,
    ) -> anyhow::Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT meta_json FROM usage_events
             WHERE kind = 'orchestration'
               AND ts >= ?1 AND ts < ?2
               AND meta_json IS NOT NULL",
        )?;
        let rows = stmt
            .query_map(params![start, end], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 尽力写入：打开默认库并 insert；失败只记日志，不向上抛
    pub fn try_record(row: NewUsageEvent) {
        match Self::open_default().and_then(|db| db.insert(row).map(|_| ())) {
            Ok(()) => {}
            Err(e) => tracing::warn!("usage_db try_record failed: {e:#}"),
        }
    }

    /// 按 period / agent 聚合洞察（KPI + 序列 + 排行）
    pub fn query_insights(&self, q: UsageInsightsQuery) -> anyhow::Result<UsageInsights> {
        let as_of = parse_as_of(q.as_of.as_deref())?;
        let (start, end, bucket_fmt) = period_bounds(q.period, &as_of);
        let agent_id = q.agent_id.filter(|s| !s.is_empty());
        Ok(UsageInsights {
            kpis: self.query_kpis(&start, &end, agent_id.as_deref())?,
            series: self.query_series(&start, &end, bucket_fmt, agent_id.as_deref())?,
            rankings: UsageRankings {
                by_kind: self.query_rank_by_kind(&start, &end, agent_id.as_deref())?,
                by_agent: self.query_rank_by_agent(&start, &end, agent_id.as_deref())?,
                by_model: self.query_rank_by_model(&start, &end, agent_id.as_deref())?,
            },
            unpriced_llm_events: self.query_unpriced_llm_events(&start, &end, agent_id.as_deref())?,
        })
    }

    fn agent_filter_sql(agent_id: Option<&str>) -> (&'static str, Option<&str>) {
        if agent_id.is_some() {
            (" AND agent_id = ?3", agent_id)
        } else {
            ("", None)
        }
    }

    fn query_kpis(
        &self,
        start: &str,
        end: &str,
        agent_id: Option<&str>,
    ) -> anyhow::Result<UsageKpis> {
        let (agent_clause, _) = Self::agent_filter_sql(agent_id);
        let sql = format!(
            "SELECT
                COALESCE(SUM({CALLS_KIND_SQL}), 0),
                COALESCE(SUM(total_tokens), 0),
                COALESCE(SUM({COST_SUM_SQL}), 0.0),
                COUNT(DISTINCT agent_id)
             FROM usage_events
             WHERE ts >= ?1 AND ts < ?2{agent_clause}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let kpis = if let Some(aid) = agent_id {
            stmt.query_row(params![start, end, aid], |r| {
                Ok(UsageKpis {
                    calls: r.get(0)?,
                    tokens: r.get(1)?,
                    cost_usd: r.get(2)?,
                    active_agents: r.get(3)?,
                })
            })?
        } else {
            stmt.query_row(params![start, end], |r| {
                Ok(UsageKpis {
                    calls: r.get(0)?,
                    tokens: r.get(1)?,
                    cost_usd: r.get(2)?,
                    active_agents: r.get(3)?,
                })
            })?
        };
        Ok(kpis)
    }

    fn query_series(
        &self,
        start: &str,
        end: &str,
        bucket_fmt: &str,
        agent_id: Option<&str>,
    ) -> anyhow::Result<Vec<UsageSeriesPoint>> {
        let (agent_clause, _) = Self::agent_filter_sql(agent_id);
        let sql = format!(
            "SELECT
                strftime('{bucket_fmt}', {TS_NORM_SQL}) AS bucket,
                COALESCE(SUM({CALLS_KIND_SQL}), 0),
                COALESCE(SUM(total_tokens), 0),
                COALESCE(SUM({COST_SUM_SQL}), 0.0)
             FROM usage_events
             WHERE ts >= ?1 AND ts < ?2{agent_clause}
             GROUP BY bucket
             ORDER BY bucket"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let map_row = |r: &rusqlite::Row<'_>| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, f64>(3)?,
            ))
        };
        let rows: Vec<(String, i64, i64, f64)> = if let Some(aid) = agent_id {
            stmt.query_map(params![start, end, aid], map_row)?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map(params![start, end], map_row)?
                .collect::<Result<Vec<_>, _>>()?
        };

        let mut by_bucket: std::collections::BTreeMap<String, (i64, i64, f64)> =
            std::collections::BTreeMap::new();
        for (bucket, calls, tokens, cost) in rows {
            if !bucket.is_empty() {
                by_bucket.insert(bucket, (calls, tokens, cost));
            }
        }

        let mut series = Vec::new();
        for bucket in all_buckets(start, end, bucket_fmt)? {
            let (calls, tokens, cost_usd) = by_bucket.get(&bucket).copied().unwrap_or((0, 0, 0.0));
            series.push(UsageSeriesPoint {
                bucket,
                calls,
                tokens,
                cost_usd,
            });
        }
        Ok(series)
    }

    /// 统一维度排行：tool / skill / mcp / cron
    fn query_rank_by_kind(
        &self,
        start: &str,
        end: &str,
        agent_id: Option<&str>,
    ) -> anyhow::Result<Vec<UsageRankItem>> {
        let (agent_clause, _) = Self::agent_filter_sql(agent_id);
        let sql = format!(
            "SELECT kind, name,
                COUNT(*) AS calls,
                COALESCE(SUM(total_tokens), 0),
                COALESCE(SUM({COST_SUM_SQL}), 0.0)
             FROM usage_events
             WHERE ts >= ?1 AND ts < ?2
               AND kind IN ('tool','skill','mcp','cron')
               {agent_clause}
             GROUP BY kind, name
             ORDER BY calls DESC, kind, name
             LIMIT 50"
        );
        self.query_rank_items(&sql, start, end, agent_id)
    }

    /// 按 Agent 排行（仅 `kind = llm`，用于模型用量洞察）
    fn query_rank_by_agent(
        &self,
        start: &str,
        end: &str,
        agent_id: Option<&str>,
    ) -> anyhow::Result<Vec<UsageRankItem>> {
        let (agent_clause, _) = Self::agent_filter_sql(agent_id);
        let sql = format!(
            "SELECT 'agent' AS kind, agent_id AS name,
                COUNT(*) AS calls,
                COALESCE(SUM(total_tokens), 0),
                COALESCE(SUM({COST_SUM_SQL}), 0.0)
             FROM usage_events
             WHERE ts >= ?1 AND ts < ?2
               AND kind = 'llm'
               {agent_clause}
             GROUP BY agent_id
             ORDER BY calls DESC, name
             LIMIT 50"
        );
        self.query_rank_items(&sql, start, end, agent_id)
    }

    /// 按模型排行（`kind = llm`）
    fn query_rank_by_model(
        &self,
        start: &str,
        end: &str,
        agent_id: Option<&str>,
    ) -> anyhow::Result<Vec<UsageRankItem>> {
        let (agent_clause, _) = Self::agent_filter_sql(agent_id);
        let sql = format!(
            "SELECT kind, name,
                COUNT(*) AS calls,
                COALESCE(SUM(total_tokens), 0),
                COALESCE(SUM({COST_SUM_SQL}), 0.0)
             FROM usage_events
             WHERE ts >= ?1 AND ts < ?2
               AND kind = 'llm'
               {agent_clause}
             GROUP BY kind, name
             ORDER BY calls DESC, name
             LIMIT 50"
        );
        self.query_rank_items(&sql, start, end, agent_id)
    }

    fn query_unpriced_llm_events(
        &self,
        start: &str,
        end: &str,
        agent_id: Option<&str>,
    ) -> anyhow::Result<i64> {
        let (agent_clause, _) = Self::agent_filter_sql(agent_id);
        let sql = format!(
            "SELECT COUNT(*)
             FROM usage_events
             WHERE ts >= ?1 AND ts < ?2
               AND kind = 'llm'
               AND cost_status = 'unknown'
               {agent_clause}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let count = if let Some(aid) = agent_id {
            stmt.query_row(params![start, end, aid], |r| r.get(0))?
        } else {
            stmt.query_row(params![start, end], |r| r.get(0))?
        };
        Ok(count)
    }

    /// 按 session 聚合近期 Trace 摘要
    pub fn list_trace_sessions(
        &self,
        start: &str,
        end: &str,
        agent_id: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<TraceSessionRow>> {
        let agent_clause = if agent_id.is_some() {
            " AND agent_id = ?3"
        } else {
            ""
        };
        let sql = format!(
            "SELECT
                session_id,
                MIN(agent_id) AS agent_id,
                MIN(ts) AS started_at,
                MAX(ts) AS ended_at,
                COUNT(*) AS event_count,
                COALESCE(SUM(total_tokens), 0) AS tokens,
                COALESCE(SUM(cost_usd), 0.0) AS cost_usd
             FROM usage_events
             WHERE ts >= ?1 AND ts < ?2
               AND session_id IS NOT NULL
               AND TRIM(session_id) != ''
               {agent_clause}
             GROUP BY session_id
             ORDER BY MAX(ts) DESC
             LIMIT {limit}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let map = |r: &rusqlite::Row<'_>| {
            Ok(TraceSessionRow {
                session_id: r.get(0)?,
                agent_id: r.get(1)?,
                started_at: r.get(2)?,
                ended_at: r.get(3)?,
                event_count: r.get(4)?,
                tokens: r.get(5)?,
                cost_usd: r.get(6)?,
            })
        };
        let rows = if let Some(aid) = agent_id {
            stmt.query_map(params![start, end, aid], map)?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map(params![start, end], map)?
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(rows)
    }

    /// 单会话事件时间线
    pub fn list_trace_events(
        &self,
        session_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<TraceEventRow>> {
        let sql = format!(
            "SELECT id, ts, kind, name, agent_id,
                    input_tokens, output_tokens, total_tokens, cost_usd
             FROM usage_events
             WHERE session_id = ?1
             ORDER BY ts ASC, rowid ASC
             LIMIT {limit}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![session_id], |r| {
                Ok(TraceEventRow {
                    id: r.get(0)?,
                    ts: r.get(1)?,
                    kind: r.get(2)?,
                    name: r.get(3)?,
                    agent_id: r.get(4)?,
                    input_tokens: r.get(5)?,
                    output_tokens: r.get(6)?,
                    total_tokens: r.get(7)?,
                    cost_usd: r.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn query_rank_items(
        &self,
        sql: &str,
        start: &str,
        end: &str,
        agent_id: Option<&str>,
    ) -> anyhow::Result<Vec<UsageRankItem>> {
        let mut stmt = self.conn.prepare(sql)?;
        let map_row = |r: &rusqlite::Row<'_>| {
            Ok(UsageRankItem {
                kind: r.get(0)?,
                name: r.get(1)?,
                calls: r.get(2)?,
                tokens: r.get(3)?,
                cost_usd: r.get(4)?,
            })
        };
        let rows = if let Some(aid) = agent_id {
            stmt.query_map(params![start, end, aid], map_row)?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map(params![start, end], map_row)?
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn period_bounds_month_half_open() {
        let as_of = chrono::DateTime::parse_from_rfc3339("2026-07-13T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let (start, end, fmt) = period_bounds(UsagePeriod::Month, &as_of);
        assert_eq!(start, "2026-07-01T00:00:00Z");
        assert_eq!(end, "2026-08-01T00:00:00Z");
        assert_eq!(fmt, "%Y-%m-%d");
    }

    fn zero_event(
        ts: &str,
        kind: &str,
        name: &str,
        agent_id: &str,
        total_tokens: i64,
    ) -> NewUsageEvent {
        NewUsageEvent {
            ts: ts.into(),
            kind: kind.into(),
            name: name.into(),
            agent_id: agent_id.into(),
            session_id: None,
            turn_id: None,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            total_tokens,
            cost_usd: 0.0,
            cost_status: None,
            cost_source: None,
            pricing_version: None,
            billing_provider: None,
            billing_base_url: None,
            billing_mode: None,
            meta_json: None,
        }
    }

    #[test]
    fn skill_rows_do_not_inflate_calls() {
        let dir = TempDir::new().unwrap();
        let db = UsageDb::new(dir.path().join("usage.db")).unwrap();
        db.insert(zero_event(
            "2026-07-13T02:00:00Z",
            "tool",
            "skills",
            "workspace",
            0,
        ))
        .unwrap();
        db.insert(zero_event(
            "2026-07-13T02:00:01Z",
            "skill",
            "create-agent",
            "workspace",
            0,
        ))
        .unwrap();
        let insights = db
            .query_insights(UsageInsightsQuery {
                period: UsagePeriod::Month,
                as_of: Some("2026-07-13T12:00:00Z".into()),
                agent_id: None,
            })
            .unwrap();
        assert_eq!(insights.kpis.calls, 1);
        assert_eq!(insights.rankings.by_kind.len(), 2);
        // 仅 llm 进入 by_agent；此处只有 tool/skill
        assert!(insights.rankings.by_agent.is_empty());
    }

    #[test]
    fn normalize_offset_ts_on_insert_counts_in_z_bounds() {
        assert_eq!(
            normalize_event_ts("2026-07-01T00:00:00+00:00"),
            "2026-07-01T00:00:00Z"
        );
        let dir = TempDir::new().unwrap();
        let db = UsageDb::new(dir.path().join("usage.db")).unwrap();
        db.insert(zero_event(
            "2026-07-01T00:00:00+00:00",
            "tool",
            "terminal",
            "workspace",
            0,
        ))
        .unwrap();
        let insights = db
            .query_insights(UsageInsightsQuery {
                period: UsagePeriod::Month,
                as_of: Some("2026-07-13T12:00:00Z".into()),
                agent_id: None,
            })
            .unwrap();
        assert_eq!(insights.kpis.calls, 1);
    }

    #[test]
    fn by_agent_only_counts_llm() {
        let dir = TempDir::new().unwrap();
        let db = UsageDb::new(dir.path().join("usage.db")).unwrap();
        db.insert(zero_event(
            "2026-07-13T02:00:00Z",
            "skill",
            "only-skill",
            "skill-only",
            10,
        ))
        .unwrap();
        db.insert(zero_event(
            "2026-07-13T02:00:01Z",
            "tool",
            "terminal",
            "workspace",
            0,
        ))
        .unwrap();
        db.insert(NewUsageEvent {
            ts: "2026-07-13T02:00:02Z".into(),
            kind: "llm".into(),
            name: "gpt-5.6".into(),
            agent_id: "workspace".into(),
            session_id: None,
            turn_id: None,
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 15,
            cost_usd: 0.01,
            cost_status: Some("estimated".into()),
            cost_source: None,
            pricing_version: None,
            billing_provider: None,
            billing_base_url: None,
            billing_mode: None,
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
        assert_eq!(insights.rankings.by_agent.len(), 1);
        assert_eq!(insights.rankings.by_agent[0].name, "workspace");
        assert_eq!(insights.rankings.by_agent[0].calls, 1);
        assert_eq!(insights.rankings.by_agent[0].tokens, 15);
    }
}
