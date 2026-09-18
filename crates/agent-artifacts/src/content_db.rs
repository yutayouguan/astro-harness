use agent_db::sqlx::{self, Row};
use agent_db::{AstroDb, DbSpec, SqlitePool};
use anyhow::Context;
use std::path::{Path, PathBuf};
use types::SqliteStore;
use uuid::Uuid;

const SCHEMA_VERSION: i32 = 2;

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
    sqlx::query("SELECT id, title, path, status, created_at, updated_at FROM contents LIMIT 0")
        .execute(pool)
        .await
        .context("knowledge database schema marker is current but contents table is incomplete")?;
    sqlx::query("SELECT title, body, content_id FROM contents_fts LIMIT 0")
        .execute(pool)
        .await
        .context("knowledge database schema marker is current but contents_fts is incomplete")?;
    Ok(())
}

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
        let db = AstroDb::new(path.parent().unwrap_or(Path::new(".")));
        let pool = db.open_pool_at_path(&DB_SPEC, &path).await?;
        sqlx::query("PRAGMA foreign_keys=ON").execute(&pool).await?;
        let kdb = Self { pool, path };
        kdb.initialize_schema().await?;
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

    async fn initialize_schema(&self) -> anyhow::Result<()> {
        let (ver,): (i32,) = sqlx::query_as("PRAGMA user_version")
            .fetch_one(&self.pool)
            .await?;
        if ver == SCHEMA_VERSION {
            return validate_current_schema(&self.pool).await;
        }
        if ver == 1 {
            return self.migrate_v1_to_v2().await;
        }
        if ver != 0 || has_user_tables(&self.pool).await? {
            anyhow::bail!(
                "unsupported knowledge.db schema version {ver}; expected {SCHEMA_VERSION}"
            );
        }

        let mut tx = self.pool.begin().await?;
        sqlx::raw_sql(DDL).execute(&mut *tx).await?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "PRAGMA user_version = {SCHEMA_VERSION}"
        )))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        validate_current_schema(&self.pool).await
    }

    /// v1 -> v2：检索文本改为按字切分（中文短查询此前完全命中不了）。
    ///
    /// 正文只存在于 FTS 表内，所以先建新表并回填，再把旧表换掉——中途退出只会留下
    /// 一张待清理的临时表，不会丢正文。
    async fn migrate_v1_to_v2(&self) -> anyhow::Result<()> {
        sqlx::query("DROP TABLE IF EXISTS contents_fts_v2")
            .execute(&self.pool)
            .await?;
        sqlx::raw_sql(
            "CREATE VIRTUAL TABLE contents_fts_v2 USING fts5(
                 title, body, content_id UNINDEXED, tokenize = 'unicode61'
             );",
        )
        .execute(&self.pool)
        .await?;

        let rows = sqlx::query("SELECT title, body, content_id FROM contents_fts")
            .fetch_all(&self.pool)
            .await?;
        let mut tx = self.pool.begin().await?;
        for row in &rows {
            let title: String = row.get(0);
            let body: String = row.get(1);
            let content_id: String = row.get(2);
            sqlx::query("INSERT INTO contents_fts_v2(title, body, content_id) VALUES (?1, ?2, ?3)")
                .bind(types::search_text::segment_for_index(&title))
                .bind(types::search_text::segment_for_index(&body))
                .bind(content_id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;

        let mut tx = self.pool.begin().await?;
        sqlx::raw_sql(
            "DROP TABLE contents_fts;
             ALTER TABLE contents_fts_v2 RENAME TO contents_fts;",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
            "PRAGMA user_version = {SCHEMA_VERSION}"
        )))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        validate_current_schema(&self.pool).await
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
                .bind(types::search_text::segment_for_index(title))
                .bind(types::search_text::segment_for_index(body))
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
                .bind(types::search_text::segment_for_index(title))
                .bind(types::search_text::segment_for_index(body))
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
        // 查询串与索引写入使用同一套按字切分；纯标点等无法构成 token 的输入直接走 LIKE。
        let Some(fts_query) = types::search_text::match_query(q) else {
            return self.search_like(q, limit).await;
        };
        let fts = async {
            let rows = sqlx::query(
                "SELECT c.id, c.title, c.path, c.status, c.created_at
                 FROM contents_fts f
                 JOIN contents c ON c.id = f.content_id
                 WHERE contents_fts MATCH ?1
                 LIMIT ?2",
            )
            .bind(&fts_query)
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
#[allow(clippy::disallowed_methods)] // 测试直接开原始 pool，生产路径必须走 agent-db
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
    async fn search_tolerates_operator_like_input() {
        let dir = TempDir::new().unwrap();
        let db = KnowledgeDb::open(dir.path().join("knowledge.db"))
            .await
            .unwrap();
        db.register("Guide", "/docs/a.md", "plain body text", "ready")
            .await
            .unwrap();
        // 运算符样式输入被整体包成短语，不再作为 FTS 语法执行。
        let _ = db.search("a AND OR \"", 10).await.unwrap();
        // 无法构成 token 的输入退回 LIKE。
        let _ = db.search("，。", 10).await.unwrap();
        let hits = db.search("Guide", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
    }

    /// 知识库与会话库共用同一套按字切分：中文短查询必须命中。
    #[tokio::test]
    async fn search_recalls_short_chinese_queries_in_body() {
        let dir = TempDir::new().unwrap();
        let db = KnowledgeDb::open(dir.path().join("knowledge.db"))
            .await
            .unwrap();
        db.register(
            "会话设计",
            "/docs/session.md",
            "会话列表需要支持中文检索与上下文压缩。",
            "ready",
        )
        .await
        .unwrap();

        for query in ["会", "会话", "会话列表", "中文检索", "话列"] {
            let hits = db.search(query, 10).await.unwrap();
            assert_eq!(hits.len(), 1, "查询 {query:?} 应当命中知识库正文");
        }
        assert!(db.search("不存在的片段", 10).await.unwrap().is_empty());
    }

    /// v1 -> v2：中文索引重建后正文不丢，短查询由搜不到变为可命中。
    #[tokio::test]
    async fn v1_upgrade_rebuilds_chinese_index() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("knowledge.db");
        let db = KnowledgeDb::open(&path).await.unwrap();
        db.register(
            "会话设计",
            "/docs/session.md",
            "会话列表的中文检索",
            "ready",
        )
        .await
        .unwrap();
        // 还原成 v1 形态：正文按原文入索引、版本号回退。
        sqlx::raw_sql(
            "DELETE FROM contents_fts;
             INSERT INTO contents_fts(title, body, content_id)
                 SELECT title, '会话列表的中文检索', id FROM contents;
             PRAGMA user_version = 1;",
        )
        .execute(&db.pool)
        .await
        .unwrap();
        drop(db);

        let upgraded = KnowledgeDb::open(&path).await.unwrap();
        let (version,): (i32,) = sqlx::query_as("PRAGMA user_version")
            .fetch_one(&upgraded.pool)
            .await
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(upgraded.list(10).await.unwrap().len(), 1);
        assert_eq!(upgraded.search("会话", 10).await.unwrap().len(), 1);
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

    #[tokio::test]
    async fn initializes_precreated_empty_database_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("knowledge.db");
        std::fs::File::create(&path).unwrap();

        let db = KnowledgeDb::open(&path).await.unwrap();
        let (version,): (i32,) = sqlx::query_as("PRAGMA user_version")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[tokio::test]
    async fn rejects_current_marker_with_incomplete_schema() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("knowledge.db");
        let pool = sqlx::SqlitePool::connect(&format!("sqlite:{}?mode=rwc", path.display()))
            .await
            .unwrap();
        sqlx::query("CREATE TABLE contents (id TEXT PRIMARY KEY)")
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

        let error = KnowledgeDb::open(path)
            .await
            .err()
            .expect("incomplete current schema must be rejected")
            .to_string();
        assert!(error.contains("contents table is incomplete"), "{error}");
    }
}
