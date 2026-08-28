//! 定时任务执行记录：SQLite 持久化。
//!
//! 职责：
//! - 在 `~/.astro/cron/cron.db` 记录每次触发的运行状态（running / success / failure）
//! - 提供插入、完成、查询与按 job/agent/日期过滤列表
//!
//! 不变量：
//! - `summary` 最长 2KB、`output` 最长 512KB（UTF-8 安全截断）
//! - 运行中记录以 `status = 'running'` 标识；同一 job 可并发查询是否在跑
//! - 使用 WAL 模式；`id` 为主键 UUID

use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use types::{truncate_utf8, SqliteStore};
use uuid::Uuid;

/// 建表 DDL（`cron_runs` 及 fired/job/agent 索引）
const DDL: &str = r#"
CREATE TABLE IF NOT EXISTS cron_runs (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL,
    title TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    schedule TEXT NOT NULL,
    task TEXT NOT NULL,
    fired_at TEXT NOT NULL,
    finished_at TEXT,
    status TEXT NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    output TEXT NOT NULL DEFAULT '',
    error TEXT,
    session_id TEXT,
    trigger TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cron_runs_fired ON cron_runs(fired_at DESC);
CREATE INDEX IF NOT EXISTS idx_cron_runs_job ON cron_runs(job_id, fired_at DESC);
CREATE INDEX IF NOT EXISTS idx_cron_runs_agent ON cron_runs(agent_id, fired_at DESC);
CREATE INDEX IF NOT EXISTS idx_cron_runs_session ON cron_runs(session_id, fired_at DESC);
"#;

/// `summary` 字段最大字节数
const MAX_SUMMARY_BYTES: usize = 2 * 1024;
/// `output` 字段最大字节数
const MAX_OUTPUT_BYTES: usize = 512 * 1024;

/// 单次 cron 执行的完整记录行
#[derive(Debug, Clone)]
pub struct CronRunRow {
    /// 运行记录 UUID
    pub id: String,
    /// 关联的 `CronJob.id`
    pub job_id: String,
    /// 任务标题快照
    pub title: String,
    /// 执行时 Agent id
    pub agent_id: String,
    /// 调度表达式快照
    pub schedule: String,
    /// 任务指令快照
    pub task: String,
    /// 触发时刻（RFC3339）
    pub fired_at: String,
    /// 结束时刻；运行中为 null
    pub finished_at: Option<String>,
    /// `running` | `success` | `failure`
    pub status: String,
    /// 成功时的简短摘要（截断后存储）
    pub summary: String,
    /// Agent 完整输出（截断后存储）
    pub output: String,
    /// 失败时的错误信息
    pub error: Option<String>,
    /// 关联会话 id（若有）
    pub session_id: Option<String>,
    /// 触发来源：`due` | `manual` 等
    pub trigger: String,
}

/// 插入「运行中」记录时的输入（不含 id，由 DB 生成）
#[derive(Debug, Clone)]
pub struct NewCronRun {
    pub job_id: String,
    pub title: String,
    pub agent_id: String,
    pub schedule: String,
    pub task: String,
    pub fired_at: String,
    pub trigger: String,
    pub session_id: Option<String>,
}

/// 列表查询过滤条件；`limit == 0` 时默认 50
#[derive(Debug, Clone, Default)]
pub struct CronRunFilters {
    pub job_id: Option<String>,
    pub agent_id: Option<String>,
    /// `fired_at >= date_from`（RFC3339 字符串比较）
    pub date_from: Option<String>,
    /// `fired_at <= date_to`
    pub date_to: Option<String>,
    pub limit: u32,
}

/// Cron 执行记录 SQLite 访问层
pub struct CronRunDb {
    conn: Connection,
    path: PathBuf,
}

/// 默认数据库路径：`{cron_root}/cron.db`
pub fn cron_db_path(cron_root: &Path) -> PathBuf {
    cron_root.join("cron.db")
}

/// 从查询行映射为 [`CronRunRow`]
fn row_from_query(r: &rusqlite::Row<'_>) -> rusqlite::Result<CronRunRow> {
    Ok(CronRunRow {
        id: r.get(0)?,
        job_id: r.get(1)?,
        title: r.get(2)?,
        agent_id: r.get(3)?,
        schedule: r.get(4)?,
        task: r.get(5)?,
        fired_at: r.get(6)?,
        finished_at: r.get(7)?,
        status: r.get(8)?,
        summary: r.get(9)?,
        output: r.get(10)?,
        error: r.get(11)?,
        session_id: r.get(12)?,
        trigger: r.get(13)?,
    })
}

/// `SELECT` 列清单（与 [`row_from_query`] 列序一致）
const SELECT_COLS: &str = "id, job_id, title, agent_id, schedule, task, fired_at, finished_at, status, summary, output, error, session_id, trigger";

impl CronRunDb {
    /// 打开或创建数据库并执行 DDL（WAL 模式）
    pub fn new(path: PathBuf) -> anyhow::Result<Self> {
        let conn = types::open_wal(&path)?;
        let db = Self { conn, path };
        db.migrate()?;
        Ok(db)
    }

    /// 打开默认 `~/.astro/data/cron.db`
    pub fn open_default() -> anyhow::Result<Self> {
        let data = home::default_memory_dir().join("data");
        Self::new(cron_db_path(&data))
    }

    /// 数据库文件路径
    pub fn db_path(&self) -> &Path {
        &self.path
    }

    /// 插入一条 `status = running` 记录，返回新 id
    pub fn insert_running(&self, row: NewCronRun) -> anyhow::Result<String> {
        let id = Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO cron_runs (
                id, job_id, title, agent_id, schedule, task, fired_at, status, trigger, session_id
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'running', ?8, ?9)",
            params![
                id,
                row.job_id,
                row.title,
                row.agent_id,
                row.schedule,
                row.task,
                row.fired_at,
                row.trigger,
                row.session_id,
            ],
        )?;
        Ok(id)
    }

    /// 标记运行成功并写入 summary / output（自动截断）
    pub fn finish_success(
        &self,
        id: &str,
        summary: &str,
        output: &str,
        finished_at: &str,
    ) -> anyhow::Result<()> {
        let summary = truncate_utf8(summary, MAX_SUMMARY_BYTES);
        let output = truncate_utf8(output, MAX_OUTPUT_BYTES);
        let changed = self.conn.execute(
            "UPDATE cron_runs
             SET status = 'success', summary = ?2, output = ?3, finished_at = ?4, error = NULL
             WHERE id = ?1",
            params![id, summary, output, finished_at],
        )?;
        if changed == 0 {
            anyhow::bail!("cron run not found: {id}");
        }
        Ok(())
    }

    /// 标记运行失败并写入 error / output
    pub fn finish_failure(
        &self,
        id: &str,
        error: &str,
        output: &str,
        finished_at: &str,
    ) -> anyhow::Result<()> {
        let output = truncate_utf8(output, MAX_OUTPUT_BYTES);
        let changed = self.conn.execute(
            "UPDATE cron_runs
             SET status = 'failure', error = ?2, output = ?3, finished_at = ?4
             WHERE id = ?1",
            params![id, error, output, finished_at],
        )?;
        if changed == 0 {
            anyhow::bail!("cron run not found: {id}");
        }
        Ok(())
    }

    /// 按 id 查询单条记录
    pub fn get(&self, id: &str) -> anyhow::Result<Option<CronRunRow>> {
        let sql = format!("SELECT {SELECT_COLS} FROM cron_runs WHERE id = ?1");
        let mut stmt = self.conn.prepare(&sql)?;
        let row = stmt.query_row(params![id], row_from_query).optional()?;
        Ok(row)
    }

    /// 按关联会话查询最近一条运行记录，供聊天中的定时任务卡片恢复详情。
    pub fn get_by_session_id(&self, session_id: &str) -> anyhow::Result<Option<CronRunRow>> {
        let sql = format!(
            "SELECT {SELECT_COLS} FROM cron_runs
             WHERE session_id = ?1
             ORDER BY fired_at DESC
             LIMIT 1"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let row = stmt
            .query_row(params![session_id], row_from_query)
            .optional()?;
        Ok(row)
    }

    /// 删除单条运行记录；返回是否删到行。运行中的记录也可删（用于用户取消展示）。
    pub fn delete(&self, id: &str) -> anyhow::Result<bool> {
        let changed = self
            .conn
            .execute("DELETE FROM cron_runs WHERE id = ?1", params![id])?;
        Ok(changed > 0)
    }

    /// 该 job 是否仍有 `status = running` 的记录
    pub fn has_running_for_job(&self, job_id: &str) -> anyhow::Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM cron_runs WHERE job_id = ?1 AND status = 'running'",
            params![job_id],
            |r| r.get(0),
        )?;
        Ok(count > 0)
    }

    /// 列出全部仍为 `running` 的记录（进程崩溃后可能残留孤儿行）
    pub fn list_running(&self) -> anyhow::Result<Vec<CronRunRow>> {
        let sql = format!(
            "SELECT {SELECT_COLS} FROM cron_runs WHERE status = 'running' ORDER BY fired_at DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map([], row_from_query)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 列出因指定错误文案失败的记录（用于「应用退出中断」事后升级）
    pub fn list_failure_with_error(&self, error: &str) -> anyhow::Result<Vec<CronRunRow>> {
        let sql = format!(
            "SELECT {SELECT_COLS} FROM cron_runs WHERE status = 'failure' AND error = ?1 ORDER BY fired_at DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![error], row_from_query)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 按过滤条件列表，按 `fired_at` 降序
    pub fn list_filtered(&self, f: CronRunFilters) -> anyhow::Result<Vec<CronRunRow>> {
        let mut sql = format!("SELECT {SELECT_COLS} FROM cron_runs WHERE 1=1");
        let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if let Some(job_id) = f.job_id.filter(|s| !s.is_empty()) {
            sql.push_str(" AND job_id = ?");
            binds.push(Box::new(job_id));
        }
        if let Some(agent_id) = f.agent_id.filter(|s| !s.is_empty()) {
            sql.push_str(" AND agent_id = ?");
            binds.push(Box::new(agent_id));
        }
        if let Some(date_from) = f.date_from.filter(|s| !s.is_empty()) {
            sql.push_str(" AND fired_at >= ?");
            binds.push(Box::new(date_from));
        }
        if let Some(date_to) = f.date_to.filter(|s| !s.is_empty()) {
            sql.push_str(" AND fired_at <= ?");
            binds.push(Box::new(date_to));
        }

        let limit = if f.limit == 0 { 50 } else { f.limit };
        sql.push_str(" ORDER BY fired_at DESC LIMIT ?");
        binds.push(Box::new(limit as i64));

        let mut stmt = self.conn.prepare(&sql)?;
        let params_ref: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(params_ref.as_slice(), row_from_query)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

impl SqliteStore for CronRunDb {
    fn path(&self) -> &Path {
        &self.path
    }

    fn migrate(&self) -> anyhow::Result<()> {
        self.conn.execute_batch(DDL)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn finish_truncates_summary_and_output() {
        let dir = TempDir::new().unwrap();
        let db = CronRunDb::new(dir.path().join("cron.db")).unwrap();
        let id = db
            .insert_running(NewCronRun {
                job_id: "j".into(),
                title: "t".into(),
                agent_id: "workspace".into(),
                schedule: "every:1d".into(),
                task: "task".into(),
                fired_at: "2026-07-11T11:00:00+08:00".into(),
                trigger: "due".into(),
                session_id: None,
            })
            .unwrap();
        let long_summary = "x".repeat(MAX_SUMMARY_BYTES + 100);
        let long_output = "y".repeat(MAX_OUTPUT_BYTES + 100);
        db.finish_success(
            &id,
            &long_summary,
            &long_output,
            "2026-07-11T11:01:00+08:00",
        )
        .unwrap();
        let got = db.get(&id).unwrap().unwrap();
        assert!(got.summary.len() <= MAX_SUMMARY_BYTES);
        assert!(got.output.len() <= MAX_OUTPUT_BYTES);
    }

    #[test]
    fn delete_removes_run() {
        let dir = TempDir::new().unwrap();
        let db = CronRunDb::new(dir.path().join("cron.db")).unwrap();
        let id = db
            .insert_running(NewCronRun {
                job_id: "j".into(),
                title: "t".into(),
                agent_id: "workspace".into(),
                schedule: "every:1d".into(),
                task: "task".into(),
                fired_at: "2026-07-11T11:00:00+08:00".into(),
                trigger: "manual".into(),
                session_id: None,
            })
            .unwrap();
        assert!(db.delete(&id).unwrap());
        assert!(db.get(&id).unwrap().is_none());
        assert!(!db.delete(&id).unwrap());
    }

    #[test]
    fn get_by_session_id_returns_the_linked_run() {
        let dir = TempDir::new().unwrap();
        let db = CronRunDb::new(dir.path().join("cron.db")).unwrap();
        let id = db
            .insert_running(NewCronRun {
                job_id: "j".into(),
                title: "t".into(),
                agent_id: "workspace".into(),
                schedule: "every:1d".into(),
                task: "task".into(),
                fired_at: "2026-07-11T11:00:00+08:00".into(),
                trigger: "due".into(),
                session_id: Some("cron-session".into()),
            })
            .unwrap();

        let run = db.get_by_session_id("cron-session").unwrap().unwrap();
        assert_eq!(run.id, id);
        assert!(db.get_by_session_id("other-session").unwrap().is_none());
    }

    #[test]
    fn cron_run_db_impls_sqlite_store() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("cron.db");
        let db = CronRunDb::new(path.clone()).unwrap();
        assert_eq!(SqliteStore::path(&db), path.as_path());
        db.migrate().unwrap();
    }
}
