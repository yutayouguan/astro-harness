use agent_db::sqlx::{self, Row};
use agent_db::{AstroDb, DbSpec, SqlitePool};
use std::path::{Path, PathBuf};
use types::SqliteStore;
use uuid::Uuid;

const SCHEMA_VERSION: i32 = 1;

const DDL: &str = r#"
CREATE TABLE IF NOT EXISTS contents (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'ready',
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE VIRTUAL TABLE IF NOT EXISTS contents_fts USING fts5(
    title,
    body,
    content_id UNINDEXED,
    tokenize = 'unicode61'
);
"#;

const DB_SPEC: DbSpec = DbSpec::new("knowledge", "knowledge.db");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentRow {
    pub id: String,
    pub title: String,
    pub path: String,
    pub status: String,
    pub created_at: String,
}

pub struct KnowledgeDb {
    pool: SqlitePool,
    path: PathBuf,
}

fn row_to_content(r: &sqlx::sqlite::SqliteRow) -> ContentRow {
    ContentRow {
        id: r.get("id"),
        title: r.get("title"),
        path: r.get("path"),
        status: r.get("status"),
        created_at: r.get("created_at"),
    }
}

impl KnowledgeDb {
    pub async fn open(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let path = path.into();
        let exists = path.exists();
        let db = AstroDb::new(path.parent().unwrap_or(Path::new(".")));
        let pool = db.open_pool(&DB_SPEC).await?;
        sqlx::query("PRAGMA foreign_keys=ON").execute(&pool).await?;
        let kdb = Self { pool, path };
        kdb.initialize_schema(exists).await?;
        Ok(kdb)
    }

    pub async fn open_default() -> anyhow::Result<Self> {
        let base = home::default_memory_dir();
        home::ensure_workspace_dirs(&base)?;
        let path = home::knowledge_db_path(&base);
        Self::open(path).await
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    async fn initialize_schema(&self, exists: bool) -> anyhow::Result<()> {
        let (ver,): (i32,) = sqlx::query_as("PRAGMA user_version")
            .fetch_one(&self.pool)
            .await
            .unwrap_or((0,));
        if exists {
            if ver != SCHEMA_VERSION {
                anyhow::bail!(
                    "unsupported knowledge.db schema version {ver}; expected {SCHEMA_VERSION}"
                );
            }
            return Ok(());
        }

        sqlx::query(DDL).execute(&self.pool).await?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "PRAGMA user_version = {SCHEMA_VERSION}"
        )))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn register(
        &self,
        title: &str,
        path: &str,
        body: &str,
        status: &str,
    ) -> anyhow::Result<ContentRow> {
        let title = title.trim();
        let path = path.trim();
        if title.is_empty() || path.is_empty() {
            anyhow::bail!("title/path 不能为空");
        }
        let status = if status.trim().is_empty() {
            "ready"
        } else {
            status.trim()
        };

        let existing: Option<(String,)> = sqlx::query_as("SELECT id FROM contents WHERE path = ?1")
            .bind(path)
            .fetch_optional(&self.pool)
            .await?;

        let id = if let Some((id,)) = existing {
            sqlx::query(
                "UPDATE contents SET title = ?1, status = ?2, updated_at = datetime('now') WHERE id = ?3",
            )
            .bind(title)
            .bind(status)
            .bind(&id)
            .execute(&self.pool)
            .await?;
            sqlx::query("DELETE FROM contents_fts WHERE content_id = ?1")
                .bind(&id)
                .execute(&self.pool)
                .await?;
            sqlx::query("INSERT INTO contents_fts(title, body, content_id) VALUES (?1, ?2, ?3)")
                .bind(title)
                .bind(body)
                .bind(&id)
                .execute(&self.pool)
                .await?;
            id
        } else {
            let id = Uuid::new_v4().to_string();
            sqlx::query("INSERT INTO contents(id, title, path, status) VALUES (?1, ?2, ?3, ?4)")
                .bind(&id)
                .bind(title)
                .bind(path)
                .bind(status)
                .execute(&self.pool)
                .await?;
            sqlx::query("INSERT INTO contents_fts(title, body, content_id) VALUES (?1, ?2, ?3)")
                .bind(title)
                .bind(body)
                .bind(&id)
                .execute(&self.pool)
                .await?;
            id
        };

        self.get(&id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("register 后读回失败"))
    }

    pub async fn get(&self, id: &str) -> anyhow::Result<Option<ContentRow>> {
        let row =
            sqlx::query("SELECT id, title, path, status, created_at FROM contents WHERE id = ?1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?
                .map(|r| row_to_content(&r));
        Ok(row)
    }

    pub async fn list(&self, limit: usize) -> anyhow::Result<Vec<ContentRow>> {
        let rows = sqlx::query(
            "SELECT id, title, path, status, created_at FROM contents
             ORDER BY created_at DESC LIMIT ?1",
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(row_to_content)
        .collect();
        Ok(rows)
    }

    pub async fn search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<ContentRow>> {
        let q = query.trim();
        if q.is_empty() {
            return self.list(limit).await;
        }
        let fts = async {
            let rows = sqlx::query(
                "SELECT c.id, c.title, c.path, c.status, c.created_at
                 FROM contents_fts f
                 JOIN contents c ON c.id = f.content_id
                 WHERE contents_fts MATCH ?1
                 LIMIT ?2",
            )
            .bind(q)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(row_to_content)
            .collect();
            Ok::<Vec<ContentRow>, anyhow::Error>(rows)
        }
        .await;
        match fts {
            Ok(rows) => Ok(rows),
            Err(_) => self.search_like(q, limit).await,
        }
    }

    async fn search_like(&self, query: &str, limit: usize) -> anyhow::Result<Vec<ContentRow>> {
        let pattern = format!("%{query}%");
        let rows = sqlx::query(
            "SELECT id, title, path, status, created_at FROM contents
             WHERE title LIKE ?1 OR path LIKE ?1
             ORDER BY created_at DESC LIMIT ?2",
        )
        .bind(&pattern)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(row_to_content)
        .collect();
        Ok(rows)
    }

    pub async fn delete(&self, id: &str) -> anyhow::Result<bool> {
        sqlx::query("DELETE FROM contents_fts WHERE content_id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        let result = sqlx::query("DELETE FROM contents WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

impl SqliteStore for KnowledgeDb {
    fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn register_search_delete() {
        let dir = TempDir::new().unwrap();
        let db = KnowledgeDb::open(dir.path().join("knowledge.db"))
            .await
            .unwrap();
        let row = db
            .register(
                "Rust Guide",
                "/docs/rust.md",
                "Rust ownership and borrowing are core.",
                "ready",
            )
            .await
            .unwrap();
        assert_eq!(row.title, "Rust Guide");
        let hits = db.search("ownership", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "/docs/rust.md");
        assert!(db.delete(&row.id).await.unwrap());
        assert!(db.search("ownership", 10).await.unwrap().is_empty());
        assert!(db.list(10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn search_falls_back_on_bad_fts_query() {
        let dir = TempDir::new().unwrap();
        let db = KnowledgeDb::open(dir.path().join("knowledge.db"))
            .await
            .unwrap();
        db.register("Guide", "/docs/a.md", "plain body text", "ready")
            .await
            .unwrap();
        let _ = db.search("a AND OR \"", 10).await.unwrap();
        let hits = db.search("Guide", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[tokio::test]
    async fn knowledge_db_impls_sqlite_store() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("knowledge.db");
        let db = KnowledgeDb::open(&path).await.unwrap();
        let _pool: &SqlitePool = SqliteStore::pool(&db);
        let (ver,): (i32,) = sqlx::query_as("PRAGMA user_version")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(ver, SCHEMA_VERSION);
    }
}
