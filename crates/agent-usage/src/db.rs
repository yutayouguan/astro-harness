use chrono::{Datelike, SecondsFormat, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use agent_db::sqlx::{self, AssertSqlSafe, Row};
use agent_db::{AstroDb, DbSpec, SqlitePool};

pub const USAGE_SCHEMA_VERSION: i32 = 4;

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

const DB_SPEC: DbSpec = DbSpec::new("usage", "usage.db");

const CALLS_KIND_SQL: &str = "CASE WHEN kind IN ('tool','mcp','cron','llm') THEN 1 ELSE 0 END";

const COST_SUM_SQL: &str =
    "CASE WHEN cost_status IS NULL OR cost_status IN ('estimated','included') THEN cost_usd ELSE 0.0 END";

const TS_NORM_SQL: &str = "replace(replace(ts, 'T', ' '), 'Z', '')";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UsagePeriod {
    Month,
    Quarter,
    Year,
}

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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageKpis {
    pub calls: i64,
    pub tokens: i64,
    pub cost_usd: f64,
    pub active_agents: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageSeriesPoint {
    pub bucket: String,
    pub calls: i64,
    pub tokens: i64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageRankItem {
    pub kind: String,
    pub name: String,
    pub calls: i64,
    pub tokens: i64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageRankings {
    pub by_kind: Vec<UsageRankItem>,
    pub by_agent: Vec<UsageRankItem>,
    pub by_model: Vec<UsageRankItem>,
}

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
    pub turn_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UsageInsights {
    pub kpis: UsageKpis,
    pub series: Vec<UsageSeriesPoint>,
    pub rankings: UsageRankings,
    pub unpriced_llm_events: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInsightsQuery {
    pub period: UsagePeriod,
    pub as_of: Option<String>,
    pub agent_id: Option<String>,
}

pub struct UsageDb {
    pool: SqlitePool,
    path: PathBuf,
}

pub fn usage_db_path() -> PathBuf {
    home::usage_db_path(&home::default_memory_dir())
}

fn fmt_utc_bound(dt: chrono::DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn period_bounds(
    period: UsagePeriod,
    as_of: &chrono::DateTime<Utc>,
) -> (String, String, &'static str) {
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

pub fn period_window(period: UsagePeriod, as_of: Option<&str>) -> anyhow::Result<(String, String)> {
    let as_of_dt = parse_as_of(as_of)?;
    let (start, end, _) = period_bounds(period, &as_of_dt);
    Ok((start, end))
}

fn normalize_event_ts(ts: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(ts) {
        Ok(dt) => dt
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Secs, true),
        Err(_) => ts.to_string(),
    }
}

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

impl types::SqliteStore for UsageDb {
    fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

impl UsageDb {
    pub async fn new(path: PathBuf) -> anyhow::Result<Self> {
        let parent = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let exists = path.exists();
        let db = AstroDb::new(&parent);
        let pool = db.open_pool(&DB_SPEC).await?;
        if !exists {
            sqlx::query(DDL).execute(&pool).await?;
            sqlx::raw_sql(AssertSqlSafe(format!(
                "PRAGMA user_version = {USAGE_SCHEMA_VERSION}"
            )))
            .execute(&pool)
            .await?;
            return Ok(Self { pool, path });
        }

        let (version,): (i32,) = sqlx::query_as("PRAGMA user_version")
            .fetch_one(&pool)
            .await?;
        if version != USAGE_SCHEMA_VERSION {
            anyhow::bail!(
                "unsupported usage.db schema version {version}; expected {USAGE_SCHEMA_VERSION}"
            );
        }
        Ok(Self { pool, path })
    }

    pub async fn open_default() -> anyhow::Result<Self> {
        home::ensure_default_workspace_dirs()?;
        Self::new(usage_db_path()).await
    }

    pub fn db_path(&self) -> &Path {
        &self.path
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn insert(&self, row: NewUsageEvent) -> anyhow::Result<String> {
        let id = Uuid::new_v4().to_string();
        let ts = normalize_event_ts(&row.ts);
        sqlx::query(
            "INSERT INTO usage_events (
                id, ts, kind, name, agent_id, session_id, turn_id,
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                reasoning_tokens, total_tokens, cost_usd,
                cost_status, cost_source, pricing_version,
                billing_provider, billing_base_url, billing_mode, meta_json
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
        )
        .bind(&id)
        .bind(&ts)
        .bind(&row.kind)
        .bind(&row.name)
        .bind(&row.agent_id)
        .bind(&row.session_id)
        .bind(&row.turn_id)
        .bind(row.input_tokens)
        .bind(row.output_tokens)
        .bind(row.cache_read_tokens)
        .bind(row.cache_write_tokens)
        .bind(row.reasoning_tokens)
        .bind(row.total_tokens)
        .bind(row.cost_usd)
        .bind(&row.cost_status)
        .bind(&row.cost_source)
        .bind(&row.pricing_version)
        .bind(&row.billing_provider)
        .bind(&row.billing_base_url)
        .bind(&row.billing_mode)
        .bind(&row.meta_json)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn try_record(row: NewUsageEvent) {
        let result = async {
            let db = Self::open_default().await?;
            db.insert(row).await?;
            Ok::<(), anyhow::Error>(())
        }
        .await;
        if let Err(e) = result {
            tracing::warn!("usage_db try_record failed: {e:#}");
        }
    }

    pub async fn query_insights(&self, q: UsageInsightsQuery) -> anyhow::Result<UsageInsights> {
        let as_of = parse_as_of(q.as_of.as_deref())?;
        let (start, end, bucket_fmt) = period_bounds(q.period, &as_of);
        let agent_id = q.agent_id.filter(|s| !s.is_empty());
        Ok(UsageInsights {
            kpis: self.query_kpis(&start, &end, agent_id.as_deref()).await?,
            series: self
                .query_series(&start, &end, bucket_fmt, agent_id.as_deref())
                .await?,
            rankings: UsageRankings {
                by_kind: self
                    .query_rank_by_kind(&start, &end, agent_id.as_deref())
                    .await?,
                by_agent: self
                    .query_rank_by_agent(&start, &end, agent_id.as_deref())
                    .await?,
                by_model: self
                    .query_rank_by_model(&start, &end, agent_id.as_deref())
                    .await?,
            },
            unpriced_llm_events: self
                .query_unpriced_llm_events(&start, &end, agent_id.as_deref())
                .await?,
        })
    }

    fn agent_filter_sql(agent_id: Option<&str>) -> &'static str {
        if agent_id.is_some() {
            " AND agent_id = ?3"
        } else {
            ""
        }
    }

    async fn query_kpis(
        &self,
        start: &str,
        end: &str,
        agent_id: Option<&str>,
    ) -> anyhow::Result<UsageKpis> {
        let agent_clause = Self::agent_filter_sql(agent_id);
        let sql = format!(
            "SELECT
                COALESCE(SUM({CALLS_KIND_SQL}), 0),
                COALESCE(SUM(total_tokens), 0),
                COALESCE(SUM({COST_SUM_SQL}), 0.0),
                COUNT(DISTINCT agent_id)
             FROM usage_events
             WHERE ts >= ?1 AND ts < ?2{agent_clause}"
        );
        let mut query = sqlx::query(AssertSqlSafe(sql)).bind(start).bind(end);
        if let Some(aid) = agent_id {
            query = query.bind(aid);
        }
        let row = query.fetch_one(&self.pool).await?;
        Ok(UsageKpis {
            calls: row.get(0),
            tokens: row.get(1),
            cost_usd: row.get(2),
            active_agents: row.get(3),
        })
    }

    async fn query_series(
        &self,
        start: &str,
        end: &str,
        bucket_fmt: &str,
        agent_id: Option<&str>,
    ) -> anyhow::Result<Vec<UsageSeriesPoint>> {
        let agent_clause = Self::agent_filter_sql(agent_id);
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
        let mut query = sqlx::query(AssertSqlSafe(sql)).bind(start).bind(end);
        if let Some(aid) = agent_id {
            query = query.bind(aid);
        }
        let rows = query.fetch_all(&self.pool).await?;
        let db_rows: Vec<(String, i64, i64, f64)> = rows
            .iter()
            .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3)))
            .collect();

        let mut by_bucket: std::collections::BTreeMap<String, (i64, i64, f64)> =
            std::collections::BTreeMap::new();
        for (bucket, calls, tokens, cost) in db_rows {
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

    async fn query_rank_by_kind(
        &self,
        start: &str,
        end: &str,
        agent_id: Option<&str>,
    ) -> anyhow::Result<Vec<UsageRankItem>> {
        let agent_clause = Self::agent_filter_sql(agent_id);
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
        self.query_rank_items(sql, start, end, agent_id).await
    }

    async fn query_rank_by_agent(
        &self,
        start: &str,
        end: &str,
        agent_id: Option<&str>,
    ) -> anyhow::Result<Vec<UsageRankItem>> {
        let agent_clause = Self::agent_filter_sql(agent_id);
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
        self.query_rank_items(sql, start, end, agent_id).await
    }

    async fn query_rank_by_model(
        &self,
        start: &str,
        end: &str,
        agent_id: Option<&str>,
    ) -> anyhow::Result<Vec<UsageRankItem>> {
        let agent_clause = Self::agent_filter_sql(agent_id);
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
        self.query_rank_items(sql, start, end, agent_id).await
    }

    async fn query_unpriced_llm_events(
        &self,
        start: &str,
        end: &str,
        agent_id: Option<&str>,
    ) -> anyhow::Result<i64> {
        let agent_clause = Self::agent_filter_sql(agent_id);
        let sql = format!(
            "SELECT COUNT(*)
             FROM usage_events
             WHERE ts >= ?1 AND ts < ?2
               AND kind = 'llm'
               AND cost_status = 'unknown'
               {agent_clause}"
        );
        let mut query = sqlx::query_as::<_, (i64,)>(AssertSqlSafe(sql))
            .bind(start)
            .bind(end);
        if let Some(aid) = agent_id {
            query = query.bind(aid);
        }
        let (count,) = query.fetch_one(&self.pool).await?;
        Ok(count)
    }

    pub async fn list_trace_sessions(
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
        let mut query = sqlx::query(AssertSqlSafe(sql)).bind(start).bind(end);
        if let Some(aid) = agent_id {
            query = query.bind(aid);
        }
        let rows = query.fetch_all(&self.pool).await?;
        Ok(rows
            .iter()
            .map(|r| TraceSessionRow {
                session_id: r.get(0),
                agent_id: r.get(1),
                started_at: r.get(2),
                ended_at: r.get(3),
                event_count: r.get(4),
                tokens: r.get(5),
                cost_usd: r.get(6),
            })
            .collect())
    }

    pub async fn list_trace_events(
        &self,
        session_id: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<TraceEventRow>> {
        let sql = format!(
            "SELECT id, ts, kind, name, agent_id,
                    input_tokens, output_tokens, total_tokens, cost_usd, turn_id
             FROM usage_events
             WHERE session_id = ?1
             ORDER BY ts ASC, rowid ASC
             LIMIT {limit}"
        );
        let rows = sqlx::query(AssertSqlSafe(sql))
            .bind(session_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .iter()
            .map(|r| TraceEventRow {
                id: r.get(0),
                ts: r.get(1),
                kind: r.get(2),
                name: r.get(3),
                agent_id: r.get(4),
                input_tokens: r.get(5),
                output_tokens: r.get(6),
                total_tokens: r.get(7),
                cost_usd: r.get(8),
                turn_id: r.get(9),
            })
            .collect())
    }

    async fn query_rank_items(
        &self,
        sql: String,
        start: &str,
        end: &str,
        agent_id: Option<&str>,
    ) -> anyhow::Result<Vec<UsageRankItem>> {
        let mut query = sqlx::query(AssertSqlSafe(sql)).bind(start).bind(end);
        if let Some(aid) = agent_id {
            query = query.bind(aid);
        }
        let rows = query.fetch_all(&self.pool).await?;
        Ok(rows
            .iter()
            .map(|r| UsageRankItem {
                kind: r.get(0),
                name: r.get(1),
                calls: r.get(2),
                tokens: r.get(3),
                cost_usd: r.get(4),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn usage_db_impls_sqlite_store() {
        use types::SqliteStore;
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("usage.db");
        let db = UsageDb::new(path).await.unwrap();
        let pool = SqliteStore::pool(&db);
        let (ver,): (i32,) = sqlx::query_as("PRAGMA user_version")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(ver, USAGE_SCHEMA_VERSION);
    }

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

    #[tokio::test]
    async fn skill_rows_do_not_inflate_calls() {
        let dir = TempDir::new().unwrap();
        let db = UsageDb::new(dir.path().join("usage.db")).await.unwrap();
        db.insert(zero_event(
            "2026-07-13T02:00:00Z",
            "tool",
            "skills",
            "workspace",
            0,
        ))
        .await
        .unwrap();
        db.insert(zero_event(
            "2026-07-13T02:00:01Z",
            "skill",
            "create-agent",
            "workspace",
            0,
        ))
        .await
        .unwrap();
        let insights = db
            .query_insights(UsageInsightsQuery {
                period: UsagePeriod::Month,
                as_of: Some("2026-07-13T12:00:00Z".into()),
                agent_id: None,
            })
            .await
            .unwrap();
        assert_eq!(insights.kpis.calls, 1);
        assert_eq!(insights.rankings.by_kind.len(), 2);
        assert!(insights.rankings.by_agent.is_empty());
    }

    #[tokio::test]
    async fn normalize_offset_ts_on_insert_counts_in_z_bounds() {
        assert_eq!(
            normalize_event_ts("2026-07-01T00:00:00+00:00"),
            "2026-07-01T00:00:00Z"
        );
        let dir = TempDir::new().unwrap();
        let db = UsageDb::new(dir.path().join("usage.db")).await.unwrap();
        db.insert(zero_event(
            "2026-07-01T00:00:00+00:00",
            "tool",
            "terminal",
            "workspace",
            0,
        ))
        .await
        .unwrap();
        let insights = db
            .query_insights(UsageInsightsQuery {
                period: UsagePeriod::Month,
                as_of: Some("2026-07-13T12:00:00Z".into()),
                agent_id: None,
            })
            .await
            .unwrap();
        assert_eq!(insights.kpis.calls, 1);
    }

    #[tokio::test]
    async fn by_agent_only_counts_llm() {
        let dir = TempDir::new().unwrap();
        let db = UsageDb::new(dir.path().join("usage.db")).await.unwrap();
        db.insert(zero_event(
            "2026-07-13T02:00:00Z",
            "skill",
            "only-skill",
            "skill-only",
            10,
        ))
        .await
        .unwrap();
        db.insert(zero_event(
            "2026-07-13T02:00:01Z",
            "tool",
            "terminal",
            "workspace",
            0,
        ))
        .await
        .unwrap();
        db.insert(NewUsageEvent {
            ts: "2026-07-13T02:00:02Z".into(),
            kind: "llm".into(),
            name: "gpt-5.6".into(),
            agent_id: "default".into(),
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
        .await
        .unwrap();
        let insights = db
            .query_insights(UsageInsightsQuery {
                period: UsagePeriod::Month,
                as_of: Some("2026-07-13T12:00:00Z".into()),
                agent_id: None,
            })
            .await
            .unwrap();
        assert_eq!(insights.rankings.by_agent.len(), 1);
        assert_eq!(insights.rankings.by_agent[0].name, "default");
        assert_eq!(insights.rankings.by_agent[0].calls, 1);
        assert_eq!(insights.rankings.by_agent[0].tokens, 15);
    }
}
