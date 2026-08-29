use agent_db::sqlx;
use agent_db::SqlitePool;
use anyhow::{bail, Context};

pub(crate) const SCHEMA_VERSION: i32 = 5;

const DDL: &str = r#"
CREATE TABLE IF NOT EXISTS agent_threads (
    thread_id TEXT PRIMARY KEY,
    root_thread_id TEXT NOT NULL,
    parent_thread_id TEXT,
    canonical_path TEXT NOT NULL,
    task_name TEXT NOT NULL,
    agent_type TEXT NOT NULL,
    session_id TEXT NOT NULL,
    status_kind TEXT NOT NULL,
    status_payload TEXT NOT NULL,
    last_status_sequence INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(root_thread_id, canonical_path)
);
CREATE INDEX IF NOT EXISTS idx_agent_graph_root_path
    ON agent_threads(root_thread_id, canonical_path);

CREATE TABLE IF NOT EXISTS agent_spawn_edges (
    parent_thread_id TEXT NOT NULL,
    child_thread_id TEXT PRIMARY KEY,
    edge_state TEXT NOT NULL CHECK(edge_state IN ('open', 'closed')),
    created_at TEXT NOT NULL,
    closed_at TEXT,
    FOREIGN KEY(child_thread_id) REFERENCES agent_threads(thread_id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_agent_spawn_edges_parent
    ON agent_spawn_edges(parent_thread_id, edge_state, created_at);

CREATE TABLE IF NOT EXISTS agent_mailbox (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id TEXT NOT NULL UNIQUE,
    idempotency_key TEXT NOT NULL UNIQUE,
    sender_thread_id TEXT NOT NULL,
    recipient_thread_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    payload TEXT NOT NULL,
    trigger_turn INTEGER NOT NULL,
    delivery_state TEXT NOT NULL,
    created_at TEXT NOT NULL,
    delivered_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_agent_mailbox_recipient_delivery
    ON agent_mailbox(recipient_thread_id, delivery_state, sequence);

CREATE TABLE IF NOT EXISTS agent_status_events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    thread_id TEXT NOT NULL,
    event_kind TEXT NOT NULL,
    payload TEXT NOT NULL,
    source_turn_id TEXT,
    created_at TEXT NOT NULL,
    FOREIGN KEY(thread_id) REFERENCES agent_threads(thread_id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_agent_status_events_thread_sequence
    ON agent_status_events(thread_id, sequence);

CREATE TABLE IF NOT EXISTS agent_runtime_descriptors (
    thread_id TEXT PRIMARY KEY,
    model TEXT,
    reasoning_effort TEXT,
    FOREIGN KEY(thread_id) REFERENCES agent_threads(thread_id) ON DELETE CASCADE
);
"#;

pub(crate) async fn initialize(pool: &SqlitePool) -> anyhow::Result<()> {
    match read_schema_version(pool).await? {
        Some(SCHEMA_VERSION) => return Ok(()),
        Some(version) => {
            bail!("unsupported subagent graph schema version {version}; expected {SCHEMA_VERSION}")
        }
        None if has_domain_tables(pool).await? => {
            bail!("subagent graph database has no current schema marker")
        }
        None => {}
    }

    let mut tx = pool.begin().await?;
    sqlx::query(
        "CREATE TABLE schema_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::raw_sql(DDL).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO schema_meta(key, value) VALUES ('schema_version', ?1)")
        .bind(SCHEMA_VERSION.to_string())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn schema_version(pool: &SqlitePool) -> anyhow::Result<i32> {
    read_schema_version(pool)
        .await?
        .context("subagent graph schema_version is missing")
}

async fn read_schema_version(pool: &SqlitePool) -> anyhow::Result<Option<i32>> {
    if !table_exists(pool, "schema_meta").await? {
        return Ok(None);
    }
    let raw: Option<(String,)> =
        sqlx::query_as("SELECT value FROM schema_meta WHERE key = 'schema_version'")
            .fetch_optional(pool)
            .await?;
    raw.map(|(value,)| {
        value
            .parse::<i32>()
            .with_context(|| format!("invalid subagent graph schema version {value:?}"))
    })
    .transpose()
}

async fn has_domain_tables(pool: &SqlitePool) -> anyhow::Result<bool> {
    for table in [
        "agent_threads",
        "agent_spawn_edges",
        "agent_mailbox",
        "agent_status_events",
        "agent_runtime_descriptors",
    ] {
        if table_exists(pool, table).await? {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn table_exists(pool: &SqlitePool, table: &str) -> anyhow::Result<bool> {
    let (exists,): (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
    )
    .bind(table)
    .fetch_one(pool)
    .await?;
    Ok(exists)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_db::sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use agent_db::sqlx::ConnectOptions;

    async fn open_fixture_pool(path: &std::path::Path) -> SqlitePool {
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .log_statements(tracing::log::LevelFilter::Debug);
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn fresh_database_has_current_schema() {
        let dir = tempfile::tempdir().unwrap();
        let pool = open_fixture_pool(&dir.path().join("subagents-v2.db")).await;

        initialize(&pool).await.unwrap();

        assert_eq!(schema_version(&pool).await.unwrap(), SCHEMA_VERSION);
        for table in [
            "agent_threads",
            "agent_spawn_edges",
            "agent_mailbox",
            "agent_status_events",
            "agent_runtime_descriptors",
        ] {
            assert!(table_exists(&pool, table).await.unwrap(), "missing {table}");
        }
    }

    #[tokio::test]
    async fn rejects_unversioned_database_with_domain_tables() {
        let dir = tempfile::tempdir().unwrap();
        let pool = open_fixture_pool(&dir.path().join("subagents-v2.db")).await;
        sqlx::query("CREATE TABLE agent_threads (id TEXT PRIMARY KEY)")
            .execute(&pool)
            .await
            .unwrap();

        let error = initialize(&pool).await.unwrap_err().to_string();
        assert!(error.contains("no current schema marker"), "{error}");
    }

    #[tokio::test]
    async fn rejects_older_version() {
        let dir = tempfile::tempdir().unwrap();
        let pool = open_fixture_pool(&dir.path().join("subagents-v2.db")).await;
        sqlx::query("CREATE TABLE schema_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO schema_meta(key, value) VALUES ('schema_version', '4')")
            .execute(&pool)
            .await
            .unwrap();

        let error = initialize(&pool).await.unwrap_err().to_string();
        assert!(
            error.contains("unsupported subagent graph schema version 4"),
            "{error}"
        );
    }
}
