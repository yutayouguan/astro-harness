use agent_db::sqlx::{self, Row};
use agent_db::{AstroDb, DbSpec, SqlitePool};
use anyhow::Context;
use std::path::{Path, PathBuf};
use types::SqliteStore;
use uuid::Uuid;

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
    agent_id TEXT NOT NULL DEFAULT 'default',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    missing INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_artifacts_session ON artifacts(session_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_artifacts_category ON artifacts(category, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_artifacts_name ON artifacts(name);
CREATE INDEX IF NOT EXISTS idx_artifacts_agent ON artifacts(agent_id, created_at DESC);
"#;

const DB_SPEC: DbSpec = DbSpec::new("artifacts", "artifacts.db");
const SCHEMA_VERSION: i32 = 1;

async fn has_user_tables(pool: &SqlitePool) -> anyhow::Result<bool> {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM sqlite_master
         WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_one(pool)
    .await?;
    Ok(count > 0)
}

async fn validate_current_schema(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        "SELECT id, path, name, category, mime, size, source, session_id,
                message_id, agent_id, created_at, updated_at, missing
         FROM artifacts LIMIT 0",
    )
    .execute(pool)
    .await
    .context("artifact database schema marker is current but artifacts table is incomplete")?;
    Ok(())
}

const MEMORY_TEMPLATES: &[&str] = &[
    "IDENTITY.md",
    "USER.md",
    "SOUL.md",
    "AGENTS.md",
    "TOOLS.md",
    "MEMORY.md",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactSource {
    AgentWrite,
    UserUpload,
    Reconcile,
}

impl ArtifactSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AgentWrite => "agent_write",
            Self::UserUpload => "user_upload",
            Self::Reconcile => "reconcile",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ArtifactRow {
    pub id: String,
    pub path: String,
    pub name: String,
    pub category: String,
    pub mime: Option<String>,
    pub size: i64,
    pub source: String,
    pub session_id: Option<String>,
    pub message_id: Option<String>,
    pub agent_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub missing: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ReconcileReport {
    pub added: u32,
    pub marked_missing: u32,
}

pub struct ArtifactDb {
    pool: SqlitePool,
    path: PathBuf,
}

pub fn artifacts_db_path(memory_dir: &Path) -> PathBuf {
    home::artifacts_db_path(memory_dir)
}

pub async fn open_default(memory_dir: &Path) -> anyhow::Result<ArtifactDb> {
    home::ensure_workspace_dirs(memory_dir)?;
    ArtifactDb::new(artifacts_db_path(memory_dir)).await
}

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
        "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java" | "c" | "cpp" | "h" | "json"
        | "toml" | "yaml" | "yml" | "html" | "css" | "sh" => "code",
        "pdf" | "ppt" | "pptx" => "pdf_ppt",
        _ => "other",
    }
}

pub fn is_junk_artifact_name(name: &str) -> bool {
    if name.starts_with("._") {
        return true;
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

const JUNK_NAME_SQL: &str = " AND lower(name) NOT IN (
    '.ds_store','thumbs.db','ehthumbs.db','desktop.ini','.localized',
    '.spotlight-v100','.trashes','.fseventsd','.temporaryitems','.volumeicon.icns'
) AND name NOT LIKE '._%'";

fn row_to_artifact(r: &sqlx::sqlite::SqliteRow) -> ArtifactRow {
    ArtifactRow {
        id: r.get("id"),
        path: r.get("path"),
        name: r.get("name"),
        category: r.get("category"),
        mime: r.get("mime"),
        size: r.get("size"),
        source: r.get("source"),
        session_id: r.get("session_id"),
        message_id: r.get("message_id"),
        agent_id: r.get("agent_id"),
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
        missing: r.get::<i64, _>("missing") != 0,
    }
}

impl ArtifactDb {
    pub async fn new(path: PathBuf) -> anyhow::Result<Self> {
        let db = AstroDb::new(path.parent().unwrap_or(Path::new(".")));
        let pool = db.open_pool_at_path(&DB_SPEC, &path).await?;
        let (version,): (i32,) = sqlx::query_as("PRAGMA user_version")
            .fetch_one(&pool)
            .await?;
        if version == SCHEMA_VERSION {
            validate_current_schema(&pool).await?;
            return Ok(Self { pool, path });
        }
        if version != 0 || has_user_tables(&pool).await? {
            anyhow::bail!(
                "unsupported artifacts.db schema version {version}; expected {SCHEMA_VERSION}"
            );
        }

        let mut tx = pool.begin().await?;
        sqlx::raw_sql(DDL).execute(&mut *tx).await?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "PRAGMA user_version = {SCHEMA_VERSION}"
        )))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        validate_current_schema(&pool).await?;
        Ok(Self { pool, path })
    }

    pub fn db_path(&self) -> &Path {
        &self.path
    }

    pub async fn register(
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
        let default_agent_id = home::DEFAULT_AGENT_ID;
        let mut agent_id = home::normalize_agent_id(agent_id.unwrap_or(default_agent_id));
        if agent_id.is_empty() {
            agent_id = default_agent_id.to_string();
        }

        sqlx::query(
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
                     WHEN excluded.agent_id != ?10 THEN excluded.agent_id
                     ELSE artifacts.agent_id
                 END,
                 missing = 0,
                 updated_at = datetime('now')",
        )
        .bind(&id)
        .bind(path)
        .bind(&name)
        .bind(&category)
        .bind(size)
        .bind(source.as_str())
        .bind(session_id)
        .bind(message_id)
        .bind(&agent_id)
        .bind(default_agent_id)
        .execute(&self.pool)
        .await?;

        self.get_by_path(path)
            .await?
            .with_context(|| format!("artifact missing after register: {path}"))
    }

    pub async fn get_by_path(&self, path: &str) -> anyhow::Result<Option<ArtifactRow>> {
        let row = sqlx::query(
            "SELECT id, path, name, category, mime, size, source, session_id, message_id,
                    agent_id, created_at, updated_at, missing
             FROM artifacts WHERE path = ?1",
        )
        .bind(path)
        .fetch_optional(&self.pool)
        .await?
        .map(|r| row_to_artifact(&r));
        Ok(row)
    }

    pub async fn unlinked_paths(&self) -> anyhow::Result<Vec<String>> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT path FROM artifacts WHERE session_id IS NULL AND missing = 0")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(|(p,)| p).collect())
    }

    pub async fn link_session_by_path(
        &self,
        path: &str,
        session_id: &str,
        message_id: Option<&str>,
    ) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "UPDATE artifacts
             SET session_id = ?2,
                 message_id = COALESCE(?3, message_id),
                 updated_at = datetime('now')
             WHERE path = ?1 AND session_id IS NULL",
        )
        .bind(path)
        .bind(session_id)
        .bind(message_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn remove_by_paths(&self, paths: &[String]) -> anyhow::Result<usize> {
        if paths.is_empty() {
            return Ok(0);
        }
        let mut n = 0usize;
        for path in paths {
            let result = sqlx::query("DELETE FROM artifacts WHERE path = ?1")
                .bind(path)
                .execute(&self.pool)
                .await?;
            n += result.rows_affected() as usize;
        }
        Ok(n)
    }

    pub async fn list(
        &self,
        category: Option<&str>,
        query: Option<&str>,
        recent_only: bool,
        limit: usize,
        include_missing: bool,
        agent_id: Option<&str>,
    ) -> anyhow::Result<Vec<ArtifactRow>> {
        let mut qb = sqlx::QueryBuilder::new(
            "SELECT id, path, name, category, mime, size, source, session_id, message_id,
                    agent_id, created_at, updated_at, missing
             FROM artifacts WHERE 1=1",
        );

        if !include_missing {
            qb.push(" AND missing = 0");
        }
        if let Some(cat) = category.filter(|c| !c.is_empty() && *c != "all") {
            qb.push(" AND category = ");
            qb.push_bind(cat.to_string());
        }
        if let Some(q) = query.map(str::trim).filter(|q| !q.is_empty()) {
            qb.push(" AND name LIKE ");
            qb.push_bind(format!("%{q}%"));
        }
        if recent_only {
            qb.push(" AND created_at >= datetime('now', '-7 days')");
        }
        if let Some(id) = agent_id.filter(|a| !a.is_empty()) {
            qb.push(" AND agent_id = ");
            qb.push_bind(id.to_string());
        }
        qb.push(JUNK_NAME_SQL);
        qb.push(" ORDER BY created_at DESC LIMIT ");
        qb.push_bind(limit as i64);

        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(row_to_artifact)
            .collect();
        Ok(rows)
    }

    pub async fn category_counts(
        &self,
        include_missing: bool,
        agent_id: Option<&str>,
    ) -> anyhow::Result<Vec<(String, i64)>> {
        let mut qb = sqlx::QueryBuilder::new("SELECT category, COUNT(*) FROM artifacts WHERE 1=1");
        if !include_missing {
            qb.push(" AND missing = 0");
        }
        qb.push(JUNK_NAME_SQL);
        if let Some(id) = agent_id.filter(|a| !a.is_empty()) {
            qb.push(" AND agent_id = ");
            qb.push_bind(id.to_string());
        }
        qb.push(" GROUP BY category");

        let rows = qb
            .build()
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(|r| (r.get::<String, _>(0), r.get::<i64, _>(1)))
            .collect();
        Ok(rows)
    }

    pub async fn reconcile(&self, memory_root: &Path) -> anyhow::Result<ReconcileReport> {
        let mut report = ReconcileReport::default();
        sqlx::query(
            "DELETE FROM artifacts WHERE lower(name) IN (
                '.ds_store','thumbs.db','ehthumbs.db','desktop.ini','.localized',
                '.spotlight-v100','.trashes','.fseventsd','.temporaryitems','.volumeicon.icns'
             ) OR name LIKE '._%'",
        )
        .execute(&self.pool)
        .await?;

        let mut roots: Vec<(PathBuf, Option<String>)> = vec![(memory_root.join("uploads"), None)];
        if let Ok(entries) = std::fs::read_dir(memory_root) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(agent_id) = home::agent_id_from_workspace_dir_name(&name) {
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
                let name = entry.file_name().and_then(|n| n.to_str()).unwrap_or("");
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
                if self.get_by_path(&path_str).await?.is_none() {
                    self.register(
                        &path_str,
                        ArtifactSource::Reconcile,
                        None,
                        None,
                        reconcile_agent,
                    )
                    .await?;
                    report.added += 1;
                } else {
                    sqlx::query(
                        "UPDATE artifacts SET missing = 0, size = ?2, updated_at = datetime('now')
                         WHERE path = ?1",
                    )
                    .bind(&path_str)
                    .bind(
                        std::fs::metadata(&entry)
                            .map(|m| m.len() as i64)
                            .unwrap_or(0),
                    )
                    .execute(&self.pool)
                    .await?;
                }
            }
        }

        let paths: Vec<(String,)> = sqlx::query_as("SELECT path FROM artifacts WHERE missing = 0")
            .fetch_all(&self.pool)
            .await?;
        for (path,) in paths {
            if !Path::new(&path).exists() {
                sqlx::query(
                    "UPDATE artifacts SET missing = 1, updated_at = datetime('now') WHERE path = ?1",
                )
                .bind(&path)
                .execute(&self.pool)
                .await?;
                report.marked_missing += 1;
            }
        }
        Ok(report)
    }
}

impl SqliteStore for ArtifactDb {
    fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

fn walkdir_files(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
        for e in std::fs::read_dir(dir)? {
            let e = e?;
            let name = e.file_name().to_string_lossy().to_string();
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

    #[tokio::test]
    async fn rejects_unversioned_database() {
        let root = TempDir::new().unwrap();
        let db_path = artifacts_db_path(root.path());
        let database_dir = db_path.parent().unwrap();
        fs::create_dir_all(database_dir).unwrap();

        {
            let old_db = AstroDb::new(database_dir);
            let pool = old_db.open_pool(&DB_SPEC).await.unwrap();
            sqlx::query(
                "CREATE TABLE artifacts (
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
                )",
            )
            .execute(&pool)
            .await
            .unwrap();
            pool.close().await;
        }

        let error = ArtifactDb::new(db_path)
            .await
            .err()
            .expect("unversioned database must be rejected")
            .to_string();
        assert!(error.contains("unsupported artifacts.db schema version 0"));
    }

    #[tokio::test]
    async fn initializes_precreated_empty_database_file() {
        let root = TempDir::new().unwrap();
        let db_path = artifacts_db_path(root.path());
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        std::fs::File::create(&db_path).unwrap();

        let db = ArtifactDb::new(db_path).await.unwrap();
        let (version,): (i32,) = sqlx::query_as("PRAGMA user_version")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[tokio::test]
    async fn rejects_current_marker_with_incomplete_schema() {
        let root = TempDir::new().unwrap();
        let db_path = artifacts_db_path(root.path());
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}?mode=rwc", db_path.display()))
            .await
            .unwrap();
        sqlx::query("CREATE TABLE artifacts (id TEXT PRIMARY KEY)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "PRAGMA user_version = {SCHEMA_VERSION}"
        )))
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;

        let error = ArtifactDb::new(db_path)
            .await
            .err()
            .expect("incomplete current schema must be rejected")
            .to_string();
        assert!(error.contains("artifacts table is incomplete"), "{error}");
    }

    #[tokio::test]
    async fn register_reconcile_and_list_by_category() {
        let root = TempDir::new().unwrap();
        let uploads = root.path().join("uploads");
        fs::create_dir_all(&uploads).unwrap();
        fs::create_dir_all(home::data_dir(root.path())).unwrap();

        let upload_path = uploads.join("photo.png");
        fs::write(&upload_path, b"fake-png").unwrap();

        let db = ArtifactDb::new(artifacts_db_path(root.path()))
            .await
            .unwrap();
        let registered = db
            .register(
                upload_path.to_str().unwrap(),
                ArtifactSource::UserUpload,
                Some("sess-1"),
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(registered.category, "image");
        assert_eq!(registered.source, "user_upload");
        assert!(!registered.missing);

        let extra = uploads.join("notes.md");
        fs::write(&extra, b"# notes").unwrap();
        let report = db.reconcile(root.path()).await.unwrap();
        assert_eq!(report.added, 1);
        assert_eq!(report.marked_missing, 0);

        let images = db
            .list(Some("image"), None, false, 50, false, None)
            .await
            .unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].name, "photo.png");

        let docs = db
            .list(Some("doc"), None, false, 50, false, None)
            .await
            .unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].name, "notes.md");
        assert_eq!(docs[0].source, "reconcile");

        let counts = db.category_counts(false, None).await.unwrap();
        assert!(counts.iter().any(|(c, n)| c == "image" && *n == 1));
        assert!(counts.iter().any(|(c, n)| c == "doc" && *n == 1));
    }

    #[tokio::test]
    async fn artifact_db_impls_sqlite_store() {
        let root = TempDir::new().unwrap();
        fs::create_dir_all(home::data_dir(root.path())).unwrap();
        let path = artifacts_db_path(root.path());
        let db = ArtifactDb::new(path).await.unwrap();
        let _pool: &SqlitePool = SqliteStore::pool(&db);
    }
}
