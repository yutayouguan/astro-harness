//! 统一 SQLite 基础层 — 所有 Astro DB 共用的文件级 PRAGMA 初始化、连接配置、池工厂和迁移。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{ConnectOptions, Connection, SqliteConnection};

pub use sqlx;
pub use sqlx::SqlitePool;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("SQLite 错误: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("迁移错误: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

pub type DbResult<T> = Result<T, DbError>;

#[derive(Debug, Clone)]
pub struct DbSpec {
    pub label: &'static str,
    pub filename: &'static str,
    pub max_connections: u32,
}

impl DbSpec {
    pub const fn new(label: &'static str, filename: &'static str) -> Self {
        Self {
            label,
            filename,
            max_connections: 4,
        }
    }

    pub const fn with_max_connections(mut self, n: u32) -> Self {
        self.max_connections = n;
        self
    }

    pub fn path(&self, home: &Path) -> PathBuf {
        home.join(self.filename)
    }
}

#[derive(Debug, Clone)]
pub struct AstroDb {
    home: PathBuf,
}

impl AstroDb {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self { home: home.into() }
    }

    pub fn from_default_home() -> Self {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string());
        Self::new(PathBuf::from(home).join(".astro"))
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub async fn open_pool(&self, spec: &DbSpec) -> DbResult<SqlitePool> {
        let path = spec.path(&self.home);
        self.open_pool_at_path(spec, &path).await
    }

    /// Open a pool for an explicit database path while retaining the shared
    /// connection policy and the spec's label/connection limit.
    ///
    /// Stores whose public constructor accepts a full path must use this
    /// method; deriving the filename from [`DbSpec`] would silently open a
    /// sibling database when the caller supplied a non-default filename.
    pub async fn open_pool_at_path(&self, spec: &DbSpec, path: &Path) -> DbResult<SqlitePool> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // journal_mode/auto_vacuum 是数据库文件级设置，不能放在池选项里。
        // SQLx 会在每条新连接上重放连接选项；多个池并发打开同一文件时，
        // 重复执行 journal_mode=WAL 会竞争排他锁并按 busy_timeout 串行等待。
        let init_lock = database_init_lock(path);
        let init_guard = init_lock.lock().await;
        initialize_database_file(path).await?;
        drop(init_guard);

        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5))
            .log_statements(tracing::log::LevelFilter::Debug)
            .log_slow_statements(tracing::log::LevelFilter::Warn, Duration::from_secs(1));

        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let pool = loop {
            match SqlitePoolOptions::new()
                .max_connections(spec.max_connections)
                .acquire_timeout(Duration::from_secs(10))
                .connect_with(opts.clone())
                .await
            {
                Ok(pool) => break pool,
                Err(error)
                    if is_sqlite_busy_or_locked(&error)
                        && tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                Err(error) => return Err(error.into()),
            }
        };

        tracing::debug!(db = spec.label, path = %path.display(), "SQLite 连接池已打开");
        Ok(pool)
    }

    pub async fn open_pool_with_ddl(
        &self,
        spec: &DbSpec,
        ddl: &'static str,
    ) -> DbResult<SqlitePool> {
        let pool = self.open_pool(spec).await?;
        sqlx::query(ddl).execute(&pool).await?;
        Ok(pool)
    }
}

fn database_init_lock(path: &Path) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Weak<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    let key = path
        .parent()
        .and_then(|parent| parent.canonicalize().ok())
        .and_then(|parent| path.file_name().map(|name| parent.join(name)))
        .unwrap_or_else(|| path.to_path_buf());
    let mut locks = LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    locks.insert(key, Arc::downgrade(&lock));
    lock
}

async fn initialize_database_file(path: &Path) -> DbResult<()> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .busy_timeout(Duration::from_secs(5))
        .log_statements(tracing::log::LevelFilter::Debug)
        .log_slow_statements(tracing::log::LevelFilter::Warn, Duration::from_secs(1));
    let mut connection = SqliteConnection::connect_with(&opts).await?;

    let auto_vacuum: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
        .fetch_one(&mut connection)
        .await?;
    let user_table_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_one(&mut connection)
    .await?;
    if auto_vacuum != 2 && user_table_count == 0 {
        sqlx::query("PRAGMA auto_vacuum = INCREMENTAL")
            .execute(&mut connection)
            .await?;
    }

    let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(&mut connection)
        .await?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        loop {
            match sqlx::query_scalar::<_, String>("PRAGMA journal_mode = WAL")
                .fetch_one(&mut connection)
                .await
            {
                Ok(_) => break,
                Err(error)
                    if is_sqlite_busy_or_locked(&error)
                        && tokio::time::Instant::now() < deadline =>
                {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
    connection.close().await?;
    Ok(())
}

fn is_sqlite_busy_or_locked(error: &sqlx::Error) -> bool {
    matches!(
        error,
        sqlx::Error::Database(database)
            if matches!(database.code().as_deref(), Some("5" | "6"))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_and_query() {
        let dir = std::env::temp_dir().join("astro-db-test");
        let _ = std::fs::create_dir_all(&dir);
        let db = AstroDb::new(&dir);
        let spec = DbSpec::new("test", "test_v1.db");
        let pool = db.open_pool(&spec).await.unwrap();

        sqlx::query("CREATE TABLE IF NOT EXISTS t(id INTEGER PRIMARY KEY, name TEXT)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO t(name) VALUES('hello')")
            .execute(&pool)
            .await
            .unwrap();
        let row: (String,) = sqlx::query_as("SELECT name FROM t WHERE id = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.0, "hello");

        pool.close().await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn fts5_compatibility() {
        let dir = std::env::temp_dir().join("astro-db-fts5-test");
        let _ = std::fs::create_dir_all(&dir);
        let db = AstroDb::new(&dir);
        let spec = DbSpec::new("fts5", "fts5_v1.db");
        let pool = db.open_pool(&spec).await.unwrap();

        sqlx::query(
            "CREATE VIRTUAL TABLE IF NOT EXISTS docs USING fts5(title, body, tokenize='unicode61')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO docs(rowid, title, body) VALUES (1, 'Rust', 'Systems programming')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let rows: Vec<(String,)> = sqlx::query_as("SELECT title FROM docs WHERE docs MATCH 'rust'")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "Rust");

        pool.close().await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn open_pool_at_path_uses_the_exact_filename() {
        let dir = std::env::temp_dir().join(format!("astro-db-exact-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db = AstroDb::new(&dir);
        let spec = DbSpec::new("custom", "default.db");
        let exact = dir.join("custom.db");

        let pool = db.open_pool_at_path(&spec, &exact).await.unwrap();
        sqlx::query("CREATE TABLE marker(id INTEGER PRIMARY KEY)")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        assert!(exact.is_file());
        assert!(!dir.join(spec.filename).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn concurrent_pools_initialize_file_pragmas_once_without_lock_storm() {
        let dir = std::env::temp_dir().join(format!(
            "astro-db-concurrent-open-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp database directory");
        let db = AstroDb::new(&dir);
        let spec = DbSpec::new("concurrent", "shared.db");

        let (first, second, third, fourth) = tokio::time::timeout(Duration::from_secs(8), async {
            tokio::join!(
                db.open_pool(&spec),
                db.open_pool(&spec),
                db.open_pool(&spec),
                db.open_pool(&spec),
            )
        })
        .await
        .expect("concurrent pool initialization must remain bounded");
        let pools = [
            first.expect("first pool"),
            second.expect("second pool"),
            third.expect("third pool"),
            fourth.expect("fourth pool"),
        ];

        for pool in &pools {
            let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
                .fetch_one(pool)
                .await
                .expect("journal mode");
            let auto_vacuum: i64 = sqlx::query_scalar("PRAGMA auto_vacuum")
                .fetch_one(pool)
                .await
                .expect("auto vacuum");
            let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
                .fetch_one(pool)
                .await
                .expect("foreign keys");
            let synchronous: i64 = sqlx::query_scalar("PRAGMA synchronous")
                .fetch_one(pool)
                .await
                .expect("synchronous");
            assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
            assert_eq!(auto_vacuum, 2);
            assert_eq!(foreign_keys, 1);
            assert_eq!(synchronous, 1);
        }
        for pool in pools {
            pool.close().await;
        }
        std::fs::remove_dir_all(&dir).expect("remove temp database directory");
    }
}
