//! 文件空间（Artifact）索引：SQLite 登记 uploads 与工作区产出文件。
//!
//! 职责：
//! - 在 `~/.astro/sessions/artifacts.db` 按路径唯一索引文件元数据
//! - 按扩展名归类、过滤系统垃圾文件、支持按 Agent / 会话查询
//! - `reconcile` 扫描磁盘与索引对齐（补登记、标记 missing）
//!
//! 不变量：
//! - `path` 为唯一键；同路径重复 `register` 走 UPSERT 并清除 `missing`
//! - 系统垃圾文件（`.DS_Store`、`._*` 等）不入库且在列表中排除
//! - Agent 工作区内的核心模板 md 与 `SKILL.md` 不参与 reconcile 登记

use anyhow::Context;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// 建表 DDL（`artifacts` 及 session/category/name/agent 索引）
const DDL: &str = r#"
CREATE TABLE IF NOT EXISTS artifacts (
    id TEXT PRIMARY KEY,
    path TEXT UNIQUE NOT NULL,
    name TEXT NOT NULL,
    category TEXT NOT NULL,
    mime TEXT,
    size INTEGER NOT NULL DEFAULT 0,
    source TEXT NOT NULL,
    session_id TEXT,
    message_id TEXT,
    agent_id TEXT NOT NULL DEFAULT 'workspace',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    missing INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_artifacts_session ON artifacts(session_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_artifacts_category ON artifacts(category, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_artifacts_name ON artifacts(name);
CREATE INDEX IF NOT EXISTS idx_artifacts_agent ON artifacts(agent_id, created_at DESC);
"#;

/// Agent 工作区内不参与 reconcile 的核心模板文件名
const MEMORY_TEMPLATES: &[&str] = &[
    "AGENT.md", "IDENTITY.md", "USER.md", "SOUL.md", "AGENTS.md", "TOOLS.md", "MEMORY.md",
];

/// 文件登记来源
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactSource {
    /// Agent 工具写入
    AgentWrite,
    /// 用户上传
    UserUpload,
    /// 磁盘 reconcile 扫描发现
    Reconcile,
}

impl ArtifactSource {
    /// 持久化用的 source 字符串
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AgentWrite => "agent_write",
            Self::UserUpload => "user_upload",
            Self::Reconcile => "reconcile",
        }
    }
}

/// 索引表中的一行文件记录
#[derive(Debug, Clone)]
pub struct ArtifactRow {
    pub id: String,
    /// 规范化后的绝对路径（唯一键）
    pub path: String,
    pub name: String,
    /// 扩展名推断的分类：`doc` / `image` / `code` 等
    pub category: String,
    pub mime: Option<String>,
    pub size: i64,
    /// 来源字符串（见 [`ArtifactSource::as_str`]）
    pub source: String,
    pub session_id: Option<String>,
    pub message_id: Option<String>,
    pub agent_id: String,
    pub created_at: String,
    pub updated_at: String,
    /// 磁盘上已不存在时为 true
    pub missing: bool,
}

/// `reconcile` 扫描结果统计
#[derive(Debug, Clone, Default)]
pub struct ReconcileReport {
    /// 新登记的文件数
    pub added: u32,
    /// 标记为 missing 的文件数
    pub marked_missing: u32,
}

/// Artifact SQLite 访问层
pub struct ArtifactDb {
    conn: Connection,
}

/// 默认数据库路径：`{memory_dir}/sessions/artifacts.db`
pub fn artifacts_db_path(memory_dir: &Path) -> PathBuf {
    memory_dir.join("sessions").join("artifacts.db")
}

fn db_has_agent_id_column(path: &Path) -> anyhow::Result<bool> {
    let conn = Connection::open(path)?;
    let has_table: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='artifacts'",
        [],
        |row| row.get(0),
    )?;
    if !has_table {
        return Ok(true);
    }
    let has_col = conn
        .prepare("PRAGMA table_info(artifacts)")?
        .query_map([], |r| r.get::<_, String>(1))?
        .filter_map(|c| c.ok())
        .any(|name| name == "agent_id");
    Ok(has_col)
}

/// 打开默认 artifacts 数据库
pub fn open_default(memory_dir: &Path) -> anyhow::Result<ArtifactDb> {
    ArtifactDb::new(artifacts_db_path(memory_dir))
}

/// 路径存在时 canonicalize；否则转为绝对路径（相对路径基于当前工作目录）
pub fn normalize_artifact_path(path: &Path) -> PathBuf {
    if path.exists() {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    } else if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

/// 按文件名扩展名推断 UI 分类
pub fn category_from_name(name: &str) -> &'static str {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "md" | "txt" | "doc" | "docx" | "rtf" => "doc",
        "csv" | "xls" | "xlsx" | "tsv" => "sheet",
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg" | "heic" => "image",
        "mp4" | "mov" | "webm" | "mp3" | "wav" | "m4a" | "aac" | "flac" => "av",
        "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java" | "c" | "cpp" | "h"
        | "json" | "toml" | "yaml" | "yml" | "html" | "css" | "sh" => "code",
        "pdf" | "ppt" | "pptx" => "pdf_ppt",
        _ => "other",
    }
}

/// 系统/资源管理器垃圾文件，不进入文件空间索引。
pub fn is_junk_artifact_name(name: &str) -> bool {
    if name.starts_with("._") {
        return true; // AppleDouble
    }
    matches!(
        name.to_ascii_lowercase().as_str(),
        ".ds_store"
            | "thumbs.db"
            | "ehthumbs.db"
            | "desktop.ini"
            | ".localized"
            | ".spotlight-v100"
            | ".trashes"
            | ".fseventsd"
            | ".temporaryitems"
            | ".volumeicon.icns"
    )
}

/// 列表查询时排除系统垃圾文件的 SQL 片段
const JUNK_NAME_SQL: &str = " AND lower(name) NOT IN (
    '.ds_store','thumbs.db','ehthumbs.db','desktop.ini','.localized',
    '.spotlight-v100','.trashes','.fseventsd','.temporaryitems','.volumeicon.icns'
) AND name NOT LIKE '._%'";

impl ArtifactDb {
    /// 打开或创建数据库并执行 DDL；缺 `agent_id` 的旧库直接丢弃重建。
    pub fn new(path: PathBuf) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if path.exists() && !db_has_agent_id_column(&path)? {
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(format!("{}-wal", path.display()));
            let _ = std::fs::remove_file(format!("{}-shm", path.display()));
        }
        let conn = Connection::open(&path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch(DDL)?;
        Ok(Self { conn })
    }

    /// 登记或更新文件索引；垃圾文件名会拒绝；同 path UPSERT
    pub fn register(
        &self,
        path: &str,
        source: ArtifactSource,
        session_id: Option<&str>,
        message_id: Option<&str>,
        agent_id: Option<&str>,
    ) -> anyhow::Result<ArtifactRow> {
        let p = PathBuf::from(path);
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path)
            .to_string();
        if is_junk_artifact_name(&name) {
            anyhow::bail!("ignored system file: {name}");
        }
        let category = category_from_name(&name).to_string();
        let size = std::fs::metadata(&p).map(|m| m.len() as i64).unwrap_or(0);
        let id = Uuid::new_v4().to_string();
        let default_agent_id = crate::workspace::DEFAULT_AGENT_ID;
        let mut agent_id = crate::workspace::normalize_agent_id(
            agent_id.unwrap_or(default_agent_id),
        );
        if agent_id.is_empty() {
            agent_id = default_agent_id.to_string();
        }

        let sql = format!(
            "INSERT INTO artifacts (id, path, name, category, size, source, session_id, message_id, agent_id, missing)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0)
             ON CONFLICT(path) DO UPDATE SET
                 name = excluded.name,
                 category = excluded.category,
                 size = excluded.size,
                 source = CASE
                     WHEN artifacts.source = 'reconcile' THEN excluded.source
                     ELSE artifacts.source
                 END,
                 session_id = COALESCE(excluded.session_id, artifacts.session_id),
                 message_id = COALESCE(excluded.message_id, artifacts.message_id),
                 agent_id = CASE
                     WHEN excluded.agent_id != '{default_agent_id}' THEN excluded.agent_id
                     ELSE artifacts.agent_id
                 END,
                 missing = 0,
                 updated_at = datetime('now')"
        );

        self.conn.execute(
            &sql,
            params![
                id,
                path,
                name,
                category,
                size,
                source.as_str(),
                session_id,
                message_id,
                agent_id,
            ],
        )?;

        self.get_by_path(path)?
            .with_context(|| format!("artifact missing after register: {path}"))
    }

    /// 按规范化 path 查询单条记录
    pub fn get_by_path(&self, path: &str) -> anyhow::Result<Option<ArtifactRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, name, category, mime, size, source, session_id, message_id,
                    agent_id, created_at, updated_at, missing
             FROM artifacts WHERE path = ?1",
        )?;
        let row = stmt
            .query_row(params![path], |r| {
                Ok(ArtifactRow {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    name: r.get(2)?,
                    category: r.get(3)?,
                    mime: r.get(4)?,
                    size: r.get(5)?,
                    source: r.get(6)?,
                    session_id: r.get(7)?,
                    message_id: r.get(8)?,
                    agent_id: r.get(9)?,
                    created_at: r.get(10)?,
                    updated_at: r.get(11)?,
                    missing: r.get::<_, i64>(12)? != 0,
                })
            })
            .optional()?;
        Ok(row)
    }

    /// 按路径批量删除索引行，返回删除条数
    pub fn remove_by_paths(&self, paths: &[String]) -> anyhow::Result<usize> {
        if paths.is_empty() {
            return Ok(0);
        }
        let mut n = 0usize;
        for path in paths {
            let changed = self
                .conn
                .execute("DELETE FROM artifacts WHERE path = ?1", [path])?;
            n += changed;
        }
        Ok(n)
    }

    /// 按分类、关键词、时间、Agent 过滤列表（默认排除 missing 与垃圾文件）
    pub fn list(
        &self,
        category: Option<&str>,
        query: Option<&str>,
        recent_only: bool,
        limit: usize,
        include_missing: bool,
        agent_id: Option<&str>,
    ) -> anyhow::Result<Vec<ArtifactRow>> {
        let mut sql = String::from(
            "SELECT id, path, name, category, mime, size, source, session_id, message_id,
                    agent_id, created_at, updated_at, missing
             FROM artifacts WHERE 1=1",
        );
        let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if !include_missing {
            sql.push_str(" AND missing = 0");
        }
        if let Some(cat) = category.filter(|c| !c.is_empty() && *c != "all") {
            sql.push_str(" AND category = ?");
            binds.push(Box::new(cat.to_string()));
        }
        if let Some(q) = query.map(str::trim).filter(|q| !q.is_empty()) {
            sql.push_str(" AND name LIKE ?");
            binds.push(Box::new(format!("%{q}%")));
        }
        if recent_only {
            sql.push_str(" AND created_at >= datetime('now', '-7 days')");
        }
        if let Some(id) = agent_id.filter(|a| !a.is_empty()) {
            sql.push_str(" AND agent_id = ?");
            binds.push(Box::new(id.to_string()));
        }
        sql.push_str(JUNK_NAME_SQL);
        sql.push_str(" ORDER BY created_at DESC LIMIT ?");
        binds.push(Box::new(limit as i64));

        let mut stmt = self.conn.prepare(&sql)?;
        let params_ref: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(params_ref.as_slice(), |r| {
                Ok(ArtifactRow {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    name: r.get(2)?,
                    category: r.get(3)?,
                    mime: r.get(4)?,
                    size: r.get(5)?,
                    source: r.get(6)?,
                    session_id: r.get(7)?,
                    message_id: r.get(8)?,
                    agent_id: r.get(9)?,
                    created_at: r.get(10)?,
                    updated_at: r.get(11)?,
                    missing: r.get::<_, i64>(12)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 按分类统计数量（可选包含 missing、按 Agent 过滤）
    pub fn category_counts(
        &self,
        include_missing: bool,
        agent_id: Option<&str>,
    ) -> anyhow::Result<Vec<(String, i64)>> {
        let mut sql = String::from("SELECT category, COUNT(*) FROM artifacts WHERE 1=1");
        let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if !include_missing {
            sql.push_str(" AND missing = 0");
        }
        sql.push_str(JUNK_NAME_SQL);
        if let Some(id) = agent_id.filter(|a| !a.is_empty()) {
            sql.push_str(" AND agent_id = ?");
            binds.push(Box::new(id.to_string()));
        }
        sql.push_str(" GROUP BY category");

        let mut stmt = self.conn.prepare(&sql)?;
        let params_ref: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(params_ref.as_slice(), |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 扫描 `uploads` 与各 Agent 工作区，与索引对齐并标记已删文件
    pub fn reconcile(&self, memory_root: &Path) -> anyhow::Result<ReconcileReport> {
        let mut report = ReconcileReport::default();
        // 清掉历史上已入库的系统垃圾文件
        self.conn.execute(
            "DELETE FROM artifacts WHERE lower(name) IN (
                '.ds_store','thumbs.db','ehthumbs.db','desktop.ini','.localized',
                '.spotlight-v100','.trashes','.fseventsd','.temporaryitems','.volumeicon.icns'
             ) OR name LIKE '._%'",
            [],
        )?;
        let mut roots: Vec<(PathBuf, Option<String>)> =
            vec![(memory_root.join("uploads"), None)];
        // 所有 Agent 工作区：workspace + workspace-*
        if let Ok(entries) = std::fs::read_dir(memory_root) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(agent_id) = crate::workspace::agent_id_from_workspace_dir_name(&name) {
                    if entry.path().is_dir() {
                        roots.push((entry.path(), Some(agent_id)));
                    }
                }
            }
        }
        for (root, agent_id) in &roots {
            if !root.exists() {
                continue;
            }
            for entry in walkdir_files(root)? {
                let name = entry
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                if is_junk_artifact_name(name) {
                    continue;
                }
                let is_agent_space = roots.iter().skip(1).any(|(r, _)| entry.starts_with(r));
                if is_agent_space && MEMORY_TEMPLATES.contains(&name) {
                    continue;
                }
                if is_agent_space && name == "SKILL.md" {
                    continue;
                }
                let path_str = entry.to_string_lossy().to_string();
                let reconcile_agent = if is_agent_space {
                    agent_id.as_deref()
                } else {
                    None
                };
                if self.get_by_path(&path_str)?.is_none() {
                    self.register(
                        &path_str,
                        ArtifactSource::Reconcile,
                        None,
                        None,
                        reconcile_agent,
                    )?;
                    report.added += 1;
                } else {
                    self.conn.execute(
                        "UPDATE artifacts SET missing = 0, size = ?2, updated_at = datetime('now')
                         WHERE path = ?1",
                        params![
                            path_str,
                            std::fs::metadata(&entry).map(|m| m.len() as i64).unwrap_or(0)
                        ],
                    )?;
                }
            }
        }

        let mut stmt = self.conn.prepare(
            "SELECT path FROM artifacts WHERE missing = 0",
        )?;
        let paths: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for path in paths {
            if !Path::new(&path).exists() {
                self.conn.execute(
                    "UPDATE artifacts SET missing = 1, updated_at = datetime('now') WHERE path = ?1",
                    params![path],
                )?;
                report.marked_missing += 1;
            }
        }
        Ok(report)
    }
}

/// 递归收集目录下所有普通文件（跳过 `__MACOSX` 与垃圾文件名）
fn walkdir_files(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
        for e in std::fs::read_dir(dir)? {
            let e = e?;
            let name = e.file_name().to_string_lossy().to_string();
            // 跳过系统目录与垃圾文件名
            if name == "__MACOSX" || is_junk_artifact_name(&name) {
                continue;
            }
            let p = e.path();
            if p.is_dir() {
                walk(&p, out)?;
            } else if p.is_file() {
                out.push(p);
            }
        }
        Ok(())
    }
    walk(root, &mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn discards_db_without_agent_id_column() {
        let root = TempDir::new().unwrap();
        let sessions = root.path().join("sessions");
        fs::create_dir_all(&sessions).unwrap();
        let db_path = artifacts_db_path(root.path());

        // 模拟旧库：无 agent_id 列
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                r#"
                CREATE TABLE artifacts (
                    id TEXT PRIMARY KEY,
                    path TEXT UNIQUE NOT NULL,
                    name TEXT NOT NULL,
                    category TEXT NOT NULL,
                    mime TEXT,
                    size INTEGER NOT NULL DEFAULT 0,
                    source TEXT NOT NULL,
                    session_id TEXT,
                    message_id TEXT,
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                    missing INTEGER NOT NULL DEFAULT 0
                );
                INSERT INTO artifacts (id, path, name, category, size, source)
                VALUES ('old', '/tmp/old.txt', 'old.txt', 'doc', 1, 'reconcile');
                "#,
            )
            .unwrap();
        }

        let db = ArtifactDb::new(db_path).unwrap();
        assert!(db.get_by_path("/tmp/old.txt").unwrap().is_none());
        let row = db
            .register(
                root.path().join("uploads").join("a.txt").to_str().unwrap(),
                ArtifactSource::Reconcile,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(row.agent_id, "workspace");
    }

    #[test]
    fn register_reconcile_and_list_by_category() {
        let root = TempDir::new().unwrap();
        let uploads = root.path().join("uploads");
        fs::create_dir_all(&uploads).unwrap();
        fs::create_dir_all(root.path().join("sessions")).unwrap();

        let upload_path = uploads.join("photo.png");
        fs::write(&upload_path, b"fake-png").unwrap();

        let db = ArtifactDb::new(artifacts_db_path(root.path())).unwrap();
        let registered = db
            .register(
                upload_path.to_str().unwrap(),
                ArtifactSource::UserUpload,
                Some("sess-1"),
                None,
                None,
            )
            .unwrap();
        assert_eq!(registered.category, "image");
        assert_eq!(registered.source, "user_upload");
        assert!(!registered.missing);

        // Untracked file under uploads — reconcile should pick it up.
        let extra = uploads.join("notes.md");
        fs::write(&extra, b"# notes").unwrap();
        let report = db.reconcile(root.path()).unwrap();
        assert_eq!(report.added, 1);
        assert_eq!(report.marked_missing, 0);

        let images = db
            .list(Some("image"), None, false, 50, false, None)
            .unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].name, "photo.png");

        let docs = db
            .list(Some("doc"), None, false, 50, false, None)
            .unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].name, "notes.md");
        assert_eq!(docs[0].source, "reconcile");

        let counts = db.category_counts(false, None).unwrap();
        assert!(counts.iter().any(|(c, n)| c == "image" && *n == 1));
        assert!(counts.iter().any(|(c, n)| c == "doc" && *n == 1));
    }
}
