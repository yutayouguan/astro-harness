//! Knowledge Content DB：文档登记 + FTS5 正文检索（不做 embedding）。
//!
//! 库路径：`{sessions_dir}/knowledge.db`（与 artifacts.db 并列）。

use rusqlite::{params, Connection, OptionalExtension};
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

/// 内容处理状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentRow {
    pub id: String,
    pub title: String,
    pub path: String,
    pub status: String,
    pub created_at: String,
}

/// Knowledge Content 库。
pub struct KnowledgeDb {
    conn: Connection,
    path: PathBuf,
}

impl KnowledgeDb {
    pub fn open(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let path = path.into();
        let conn = types::open_wal(&path)?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let db = Self { conn, path };
        db.migrate()?;
        Ok(db)
    }

    pub fn open_default() -> anyhow::Result<Self> {
        let path = home::default_memory_dir()
            .join("data")
            .join("knowledge.db");
        Self::open(path)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn apply_migrate(conn: &Connection) -> anyhow::Result<()> {
        let ver: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap_or(0);
        if ver < SCHEMA_VERSION {
            conn.execute_batch(DDL)?;
            conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
        }
        Ok(())
    }

    /// 登记文档并写入 FTS 正文；同 path 则更新。
    pub fn register(
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

        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM contents WHERE path = ?1",
                params![path],
                |r| r.get(0),
            )
            .optional()?;

        let id = if let Some(id) = existing {
            self.conn.execute(
                "UPDATE contents SET title = ?1, status = ?2, updated_at = datetime('now') WHERE id = ?3",
                params![title, status, id],
            )?;
            self.conn.execute(
                "DELETE FROM contents_fts WHERE content_id = ?1",
                params![id],
            )?;
            self.conn.execute(
                "INSERT INTO contents_fts(title, body, content_id) VALUES (?1, ?2, ?3)",
                params![title, body, id],
            )?;
            id
        } else {
            let id = Uuid::new_v4().to_string();
            self.conn.execute(
                "INSERT INTO contents(id, title, path, status) VALUES (?1, ?2, ?3, ?4)",
                params![id, title, path, status],
            )?;
            self.conn.execute(
                "INSERT INTO contents_fts(title, body, content_id) VALUES (?1, ?2, ?3)",
                params![title, body, id],
            )?;
            id
        };

        self.get(&id)?
            .ok_or_else(|| anyhow::anyhow!("register 后读回失败"))
    }

    pub fn get(&self, id: &str) -> anyhow::Result<Option<ContentRow>> {
        self.conn
            .query_row(
                "SELECT id, title, path, status, created_at FROM contents WHERE id = ?1",
                params![id],
                |r| {
                    Ok(ContentRow {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        path: r.get(2)?,
                        status: r.get(3)?,
                        created_at: r.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list(&self, limit: usize) -> anyhow::Result<Vec<ContentRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, path, status, created_at FROM contents
             ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit as i64], |r| {
                Ok(ContentRow {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    path: r.get(2)?,
                    status: r.get(3)?,
                    created_at: r.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// FTS 检索；返回匹配的内容元数据（citation 用 path/title）。
    ///
    /// `MATCH` 失败（特殊字符等）时回退为 title/path `LIKE` 子串搜索。
    pub fn search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<ContentRow>> {
        let q = query.trim();
        if q.is_empty() {
            return self.list(limit);
        }
        let fts = (|| -> anyhow::Result<Vec<ContentRow>> {
            let mut stmt = self.conn.prepare(
                "SELECT c.id, c.title, c.path, c.status, c.created_at
                 FROM contents_fts f
                 JOIN contents c ON c.id = f.content_id
                 WHERE contents_fts MATCH ?1
                 LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![q, limit as i64], |r| {
                    Ok(ContentRow {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        path: r.get(2)?,
                        status: r.get(3)?,
                        created_at: r.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })();
        match fts {
            Ok(rows) => Ok(rows),
            Err(_) => self.search_like(q, limit),
        }
    }

    fn search_like(&self, query: &str, limit: usize) -> anyhow::Result<Vec<ContentRow>> {
        let pattern = format!("%{query}%");
        let mut stmt = self.conn.prepare(
            "SELECT id, title, path, status, created_at FROM contents
             WHERE title LIKE ?1 OR path LIKE ?1
             ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![pattern, limit as i64], |r| {
                Ok(ContentRow {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    path: r.get(2)?,
                    status: r.get(3)?,
                    created_at: r.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 删除登记与 FTS 行。
    pub fn delete(&self, id: &str) -> anyhow::Result<bool> {
        self.conn.execute(
            "DELETE FROM contents_fts WHERE content_id = ?1",
            params![id],
        )?;
        let n = self
            .conn
            .execute("DELETE FROM contents WHERE id = ?1", params![id])?;
        Ok(n > 0)
    }
}

impl SqliteStore for KnowledgeDb {
    fn path(&self) -> &Path {
        &self.path
    }

    fn migrate(&self) -> anyhow::Result<()> {
        Self::apply_migrate(&self.conn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn register_search_delete() {
        let dir = TempDir::new().unwrap();
        let db = KnowledgeDb::open(dir.path().join("knowledge.db")).unwrap();
        let row = db
            .register(
                "Rust Guide",
                "/docs/rust.md",
                "Rust ownership and borrowing are core.",
                "ready",
            )
            .unwrap();
        assert_eq!(row.title, "Rust Guide");
        let hits = db.search("ownership", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "/docs/rust.md");
        assert!(db.delete(&row.id).unwrap());
        assert!(db.search("ownership", 10).unwrap().is_empty());
        assert!(db.list(10).unwrap().is_empty());
    }

    #[test]
    fn search_falls_back_on_bad_fts_query() {
        let dir = TempDir::new().unwrap();
        let db = KnowledgeDb::open(dir.path().join("knowledge.db")).unwrap();
        db.register("Guide", "/docs/a.md", "plain body text", "ready")
            .unwrap();
        // FTS 特殊字符常导致 MATCH 语法错误；应回退 LIKE（可能空结果但不 panic）
        let _ = db.search("a AND OR \"", 10).unwrap();
        let hits = db.search("Guide", 10).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn knowledge_db_impls_sqlite_store() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("knowledge.db");
        let db = KnowledgeDb::open(&path).unwrap();
        assert_eq!(SqliteStore::path(&db), path.as_path());
        db.migrate().unwrap();
        let ver: i32 = db
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ver, SCHEMA_VERSION);
    }
}
