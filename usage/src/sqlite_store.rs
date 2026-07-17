//! 兼容 re-export：实现已迁至 [`common::sqlite`]。

pub use common::sqlite::{open_wal, ExampleSqliteStore, SqliteStore};
