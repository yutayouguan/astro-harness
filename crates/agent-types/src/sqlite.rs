//! 共享 SQLite 打开约定与 [`SqliteStore`] 协议。
//!
//! 基于 sqlx（通过 `agent-db`），异步连接池 + WAL 模式。

use std::path::{Path, PathBuf};

pub use agent_db::{sqlx, AstroDb, DbError, DbResult, DbSpec, SqlitePool};

/// 删除 SQLite 主库及其 WAL/SHM 旁路文件（忽略缺失）。
pub fn delete_sqlite_files(path: &Path) {
    let base = path.to_string_lossy();
    for p in [
        path.to_path_buf(),
        PathBuf::from(format!("{base}-wal")),
        PathBuf::from(format!("{base}-shm")),
    ] {
        let _ = std::fs::remove_file(p);
    }
}

/// 本地 SQLite 存储协议（各库形状对齐，领域 CRUD 仍各库自管）。
#[async_trait::async_trait]
pub trait SqliteStore: Send + Sync {
    fn pool(&self) -> &SqlitePool;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn delete_sqlite_files_removes_sidecars() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("x.db");
        std::fs::write(&path, b"").unwrap();
        std::fs::write(format!("{}-wal", path.display()), b"").unwrap();
        std::fs::write(format!("{}-shm", path.display()), b"").unwrap();
        delete_sqlite_files(&path);
        assert!(!path.exists());
        assert!(!PathBuf::from(format!("{}-wal", path.display())).exists());
        assert!(!PathBuf::from(format!("{}-shm", path.display())).exists());
    }
}
