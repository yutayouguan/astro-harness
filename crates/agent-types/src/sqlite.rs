//! 共享 SQLite 打开约定与 [`SqliteStore`] 协议。
//!
//! 基于 sqlx（通过 `agent-db`），异步连接池 + WAL 模式。

pub use agent_db::{sqlx, AstroDb, DbError, DbResult, DbSpec, SqlitePool};

/// 本地 SQLite 存储协议（各库形状对齐，领域 CRUD 仍各库自管）。
#[async_trait::async_trait]
pub trait SqliteStore: Send + Sync {
    fn pool(&self) -> &SqlitePool;
}
