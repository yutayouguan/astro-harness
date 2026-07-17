//! 共享 SQLite 打开约定与 [`SqliteStore`] 协议（桌面多库对齐，不上 Postgres）。

use std::path::{Path, PathBuf};

use anyhow::Context;
use rusqlite::Connection;

/// 打开 SQLite 并启用 WAL；自动创建父目录。
pub fn open_wal(path: impl AsRef<Path>) -> anyhow::Result<Connection> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create {}", parent.display()))?;
    }
    let conn = Connection::open(path)
        .with_context(|| format!("open sqlite {}", path.display()))?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    Ok(conn)
}

/// 本地 SQLite 存储协议（session / usage / knowledge / … 形状对齐）。
pub trait SqliteStore {
    fn path(&self) -> &Path;
    fn migrate(&self) -> anyhow::Result<()>;
}

/// 示范实现：空库 + `user_version=1`（供其它库对齐接口）。
pub struct ExampleSqliteStore {
    path: PathBuf,
    conn: Connection,
}

impl ExampleSqliteStore {
    pub fn open(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let path = path.into();
        let conn = open_wal(&path)?;
        let store = Self { path, conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn user_version(&self) -> anyhow::Result<i32> {
        self.conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(Into::into)
    }
}

impl SqliteStore for ExampleSqliteStore {
    fn path(&self) -> &Path {
        &self.path
    }

    fn migrate(&self) -> anyhow::Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _astro_meta (k TEXT PRIMARY KEY, v TEXT);
             PRAGMA user_version = 1;",
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn open_wal_and_migrate_example() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("demo.db");
        let store = ExampleSqliteStore::open(&path).unwrap();
        assert_eq!(store.path(), path.as_path());
        assert_eq!(store.user_version().unwrap(), 1);
        let store2 = ExampleSqliteStore::open(&path).unwrap();
        assert_eq!(store2.user_version().unwrap(), 1);
    }
}
