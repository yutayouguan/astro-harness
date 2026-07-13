//! 多 Agent 编排状态库：SQLite 持久化编排任务与步骤。
//!
//! 职责：
//! - 在 `~/.astro/orchestration.db` 记录编排（queued → running → done/failed/cancelled）
//! - 按 seq 串行步骤（pending → running → done/failed/skipped）
//! - `try_claim_running` 做 CAS，防止同 id 双 spawn
//!
//! 不变量：
//! - `output` 最长 64KB（UTF-8 安全截断）
//! - 使用 WAL 模式；`id` 为主键 UUID
//! - 时间戳为 ISO UTC（RFC3339，秒精度，与 usage period_window 一致）

use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::PathBuf;
use uuid::Uuid;

/// 建表 DDL（`orchestrations` + `orchestration_steps` 及常用索引）
const DDL: &str = r#"
CREATE TABLE IF NOT EXISTS orchestrations (
    id TEXT PRIMARY KEY,
    parent_agent_id TEXT NOT NULL,
    session_id TEXT,
    goal TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    finished_at TEXT,
    error TEXT,
    result_summary TEXT
);
CREATE INDEX IF NOT EXISTS idx_orchestrations_status_updated
    ON orchestrations(status, updated_at);

CREATE TABLE IF NOT EXISTS orchestration_steps (
    id TEXT PRIMARY KEY,
    orchestration_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    role TEXT NOT NULL,
    agent_id TEXT,
    prompt TEXT NOT NULL,
    status TEXT NOT NULL,
    output TEXT,
    error TEXT,
    started_at TEXT,
    finished_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_orchestration_steps_orch_seq
    ON orchestration_steps(orchestration_id, seq);
"#;

/// step `output` 字段最大字节数
const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// 编排整体状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrchestrationStatus {
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

impl OrchestrationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// 编排步骤状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    Running,
    Done,
    Failed,
    Skipped,
}

impl StepStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

/// 编排主表行
#[derive(Debug, Clone)]
pub struct OrchestrationRow {
    pub id: String,
    pub parent_agent_id: String,
    pub session_id: Option<String>,
    pub goal: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
    pub error: Option<String>,
    pub result_summary: Option<String>,
}

/// 编排步骤行
#[derive(Debug, Clone)]
pub struct StepRow {
    pub id: String,
    pub orchestration_id: String,
    pub seq: i64,
    pub role: String,
    pub agent_id: Option<String>,
    pub prompt: String,
    pub status: String,
    pub output: Option<String>,
    pub error: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

/// 新建编排时的步骤输入
#[derive(Debug, Clone)]
pub struct NewOrchestrationStep {
    pub role: String,
    pub agent_id: Option<String>,
    pub prompt: String,
}

/// 新建编排输入（不含 id，由 DB 生成）
#[derive(Debug, Clone)]
pub struct NewOrchestration {
    pub parent_agent_id: String,
    pub session_id: Option<String>,
    pub goal: String,
    pub steps: Vec<NewOrchestrationStep>,
}

/// 编排状态 SQLite 访问层
pub struct OrchestrationDb {
    conn: Connection,
}

/// 默认数据库路径：`{ASTRO_MEMORY_DIR|~/.astro}/orchestration.db`
pub fn orchestration_db_path() -> PathBuf {
    crate::workspace::default_memory_dir().join("orchestration.db")
}

/// 当前 UTC 时间 RFC3339（秒精度，与 `usage_db::period_window` / `fmt_utc_bound` 一致）
fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// 按 UTF-8 字符边界截断字符串至 `max_bytes`
fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

fn orchestration_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<OrchestrationRow> {
    Ok(OrchestrationRow {
        id: r.get(0)?,
        parent_agent_id: r.get(1)?,
        session_id: r.get(2)?,
        goal: r.get(3)?,
        status: r.get(4)?,
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
        finished_at: r.get(7)?,
        error: r.get(8)?,
        result_summary: r.get(9)?,
    })
}

fn step_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<StepRow> {
    Ok(StepRow {
        id: r.get(0)?,
        orchestration_id: r.get(1)?,
        seq: r.get(2)?,
        role: r.get(3)?,
        agent_id: r.get(4)?,
        prompt: r.get(5)?,
        status: r.get(6)?,
        output: r.get(7)?,
        error: r.get(8)?,
        started_at: r.get(9)?,
        finished_at: r.get(10)?,
    })
}

const ORCH_SELECT_COLS: &str = "id, parent_agent_id, session_id, goal, status, created_at, updated_at, finished_at, error, result_summary";
const STEP_SELECT_COLS: &str = "id, orchestration_id, seq, role, agent_id, prompt, status, output, error, started_at, finished_at";

impl OrchestrationDb {
    /// 打开或创建数据库并执行 DDL（WAL 模式）
    pub fn new(path: PathBuf) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(DDL)?;
        Ok(Self { conn })
    }

    /// 打开默认 `~/.astro/orchestration.db`
    pub fn open_default() -> anyhow::Result<Self> {
        Self::new(orchestration_db_path())
    }

    /// 插入编排（queued）及步骤（pending），返回 orchestration id
    pub fn create(&self, input: NewOrchestration) -> anyhow::Result<String> {
        let id = Uuid::new_v4().to_string();
        let now = now_rfc3339();
        let tx = self.conn.unchecked_transaction()?;

        tx.execute(
            "INSERT INTO orchestrations (
                id, parent_agent_id, session_id, goal, status,
                created_at, updated_at, finished_at, error, result_summary
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, NULL)",
            params![
                id,
                input.parent_agent_id,
                input.session_id,
                input.goal,
                OrchestrationStatus::Queued.as_str(),
                now,
                now,
            ],
        )?;

        for (seq, step) in input.steps.into_iter().enumerate() {
            let step_id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO orchestration_steps (
                    id, orchestration_id, seq, role, agent_id, prompt, status,
                    output, error, started_at, finished_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, NULL, NULL)",
                params![
                    step_id,
                    id,
                    seq as i64,
                    step.role,
                    step.agent_id,
                    step.prompt,
                    StepStatus::Pending.as_str(),
                ],
            )?;
        }

        tx.commit()?;
        Ok(id)
    }

    /// 按 id 查询编排
    pub fn get(&self, id: &str) -> anyhow::Result<Option<OrchestrationRow>> {
        let sql = format!("SELECT {ORCH_SELECT_COLS} FROM orchestrations WHERE id = ?1");
        let mut stmt = self.conn.prepare(&sql)?;
        let row = stmt
            .query_row(params![id], orchestration_from_row)
            .optional()?;
        Ok(row)
    }

    /// 列出编排步骤，按 seq 升序
    pub fn list_steps(&self, orchestration_id: &str) -> anyhow::Result<Vec<StepRow>> {
        let sql = format!(
            "SELECT {STEP_SELECT_COLS} FROM orchestration_steps WHERE orchestration_id = ?1 ORDER BY seq ASC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![orchestration_id], step_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 更新编排状态；终态（done/failed/cancelled）会写 `finished_at`
    pub fn set_orchestration_status(
        &self,
        id: &str,
        status: OrchestrationStatus,
        error: Option<&str>,
        result_summary: Option<&str>,
    ) -> anyhow::Result<()> {
        let now = now_rfc3339();
        let finished_at = match status {
            OrchestrationStatus::Done
            | OrchestrationStatus::Failed
            | OrchestrationStatus::Cancelled => Some(now.clone()),
            _ => None,
        };
        let changed = self.conn.execute(
            "UPDATE orchestrations
             SET status = ?2, updated_at = ?3, error = ?4, result_summary = ?5,
                 finished_at = COALESCE(?6, finished_at)
             WHERE id = ?1",
            params![
                id,
                status.as_str(),
                now,
                error,
                result_summary,
                finished_at,
            ],
        )?;
        if changed == 0 {
            anyhow::bail!("orchestration not found: {id}");
        }
        Ok(())
    }

    /// 将步骤标为 running，并写 `started_at`
    pub fn set_step_running(&self, step_id: &str) -> anyhow::Result<()> {
        let now = now_rfc3339();
        let changed = self.conn.execute(
            "UPDATE orchestration_steps
             SET status = ?2, started_at = ?3, error = NULL
             WHERE id = ?1",
            params![step_id, StepStatus::Running.as_str(), now],
        )?;
        if changed == 0 {
            anyhow::bail!("orchestration step not found: {step_id}");
        }
        Ok(())
    }

    /// 将步骤标为 done，写入截断后的 output
    pub fn set_step_done(&self, step_id: &str, output: &str) -> anyhow::Result<()> {
        let now = now_rfc3339();
        let output = truncate_utf8(output, MAX_OUTPUT_BYTES);
        let changed = self.conn.execute(
            "UPDATE orchestration_steps
             SET status = ?2, output = ?3, error = NULL, finished_at = ?4
             WHERE id = ?1",
            params![step_id, StepStatus::Done.as_str(), output, now],
        )?;
        if changed == 0 {
            anyhow::bail!("orchestration step not found: {step_id}");
        }
        Ok(())
    }

    /// 将步骤标为 failed，写入 error
    pub fn set_step_failed(&self, step_id: &str, error: &str) -> anyhow::Result<()> {
        let now = now_rfc3339();
        let changed = self.conn.execute(
            "UPDATE orchestration_steps
             SET status = ?2, error = ?3, finished_at = ?4
             WHERE id = ?1",
            params![step_id, StepStatus::Failed.as_str(), error, now],
        )?;
        if changed == 0 {
            anyhow::bail!("orchestration step not found: {step_id}");
        }
        Ok(())
    }

    /// 将 `seq > failed_seq` 且仍为 pending 的步骤标为 skipped（失败/超时后跳过后续步）。
    pub fn skip_pending_steps_after(
        &self,
        orchestration_id: &str,
        failed_seq: i64,
    ) -> anyhow::Result<usize> {
        let now = now_rfc3339();
        let changed = self.conn.execute(
            "UPDATE orchestration_steps
             SET status = ?3, finished_at = ?4
             WHERE orchestration_id = ?1 AND seq > ?2 AND status = ?5",
            params![
                orchestration_id,
                failed_seq,
                StepStatus::Skipped.as_str(),
                now,
                StepStatus::Pending.as_str(),
            ],
        )?;
        Ok(changed)
    }

    /// CAS：仅当 status 为 `queued` 时改为 `running`，防双 spawn
    pub fn try_claim_running(&self, id: &str) -> anyhow::Result<bool> {
        let now = now_rfc3339();
        let changed = self.conn.execute(
            "UPDATE orchestrations
             SET status = ?2, updated_at = ?3
             WHERE id = ?1 AND status = ?4",
            params![
                id,
                OrchestrationStatus::Running.as_str(),
                now,
                OrchestrationStatus::Queued.as_str(),
            ],
        )?;
        Ok(changed > 0)
    }

    /// 按 created_at ∈ [start, end) 列出编排，可选 parent_agent_id，按 created_at DESC，limit（0→50）。
    pub fn list_in_period(
        &self,
        start: &str,
        end: &str,
        parent_agent_id: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<OrchestrationRow>> {
        let limit = if limit == 0 { 50usize } else { limit };
        let limit_i = limit as i64;
        let parent = parent_agent_id.filter(|s| !s.is_empty());

        if let Some(aid) = parent {
            let sql = format!(
                "SELECT {ORCH_SELECT_COLS} FROM orchestrations
                 WHERE created_at >= ?1 AND created_at < ?2 AND parent_agent_id = ?3
                 ORDER BY created_at DESC
                 LIMIT ?4"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt
                .query_map(params![start, end, aid, limit_i], orchestration_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        } else {
            let sql = format!(
                "SELECT {ORCH_SELECT_COLS} FROM orchestrations
                 WHERE created_at >= ?1 AND created_at < ?2
                 ORDER BY created_at DESC
                 LIMIT ?3"
            );
            let mut stmt = self.conn.prepare(&sql)?;
            let rows = stmt
                .query_map(params![start, end, limit_i], orchestration_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        }
    }

    /// 测试辅助：覆写 created_at / updated_at（集成测试无法使用 `#[cfg(test)]` 库方法）。
    pub fn set_created_at_for_test(&self, id: &str, created_at: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE orchestrations SET created_at = ?2, updated_at = ?2 WHERE id = ?1",
            params![id, created_at],
        )?;
        Ok(())
    }
}
