use anyhow::{bail, Context};
use agent_db::sqlx::{self, Row};
use agent_db::SqlitePool;
use serde::{Deserialize, Serialize};

pub(crate) const SCHEMA_VERSION: i32 = 4;

const V2_DDL: &str = r#"
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
    recovery_state TEXT NOT NULL DEFAULT 'available'
        CHECK(recovery_state IN ('available', 'legacy_unavailable')),
    FOREIGN KEY(thread_id) REFERENCES agent_threads(thread_id) ON DELETE CASCADE
);
"#;

const V4_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS agent_runtime_descriptors (
    thread_id TEXT PRIMARY KEY,
    model TEXT,
    reasoning_effort TEXT,
    recovery_state TEXT NOT NULL DEFAULT 'available'
        CHECK(recovery_state IN ('available', 'legacy_unavailable')),
    FOREIGN KEY(thread_id) REFERENCES agent_threads(thread_id) ON DELETE CASCADE
);
"#;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoricalAgentThread {
    pub id: String,
    pub parent_session_id: String,
    pub parent_agent_id: String,
    pub agent_name: String,
    pub task: String,
    pub status: String,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub model: Option<String>,
    pub model_reasoning_effort: Option<String>,
    pub sandbox_mode: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
    pub closed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoricalAgentMessage {
    pub id: i64,
    pub thread_id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

pub(crate) async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    let existing_version = read_schema_version(pool).await?;
    match existing_version {
        Some(SCHEMA_VERSION) => return Ok(()),
        Some(3) => {
            let mut tx = pool.begin().await?;
            ensure_runtime_descriptor_recovery_state(&mut tx).await?;
            sqlx::query("UPDATE schema_meta SET value = ?1 WHERE key = 'schema_version'")
                .bind(SCHEMA_VERSION.to_string())
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(());
        }
        Some(2) => {
            let mut tx = pool.begin().await?;
            sqlx::query(V4_DDL).execute(&mut *tx).await?;
            ensure_runtime_descriptor_recovery_state(&mut tx).await?;
            sqlx::query("UPDATE schema_meta SET value = ?1 WHERE key = 'schema_version'")
                .bind(SCHEMA_VERSION.to_string())
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(());
        }
        Some(version) => {
            bail!("unsupported subagent graph schema version {version}; expected {SCHEMA_VERSION}")
        }
        None => {}
    }

    let mut tx = pool.begin().await?;
    archive_legacy_v1_tables(&mut tx).await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS schema_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query(V2_DDL).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO schema_meta(key, value) VALUES ('schema_version', ?1)")
        .bind(SCHEMA_VERSION.to_string())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn schema_version(pool: &SqlitePool) -> anyhow::Result<i32> {
    read_schema_version(pool).await?.context("subagent graph schema_version is missing")
}

pub(crate) async fn list_historical_threads(
    pool: &SqlitePool,
) -> anyhow::Result<Vec<HistoricalAgentThread>> {
    if !table_exists_on(pool, "historical_agent_threads_v1").await? {
        return Ok(Vec::new());
    }
    let rows = sqlx::query(
        "SELECT id, parent_session_id, parent_agent_id, agent_name, task, status,
                summary, error, model, model_reasoning_effort, sandbox_mode,
                created_at, updated_at, finished_at, closed_at
         FROM historical_agent_threads_v1
         ORDER BY created_at, id",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|row| HistoricalAgentThread {
            id: row.get("id"),
            parent_session_id: row.get("parent_session_id"),
            parent_agent_id: row.get("parent_agent_id"),
            agent_name: row.get("agent_name"),
            task: row.get("task"),
            status: row.get("status"),
            summary: row.get("summary"),
            error: row.get("error"),
            model: row.get("model"),
            model_reasoning_effort: row.get("model_reasoning_effort"),
            sandbox_mode: row.get("sandbox_mode"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            finished_at: row.get("finished_at"),
            closed_at: row.get("closed_at"),
        })
        .collect())
}

pub(crate) async fn list_historical_messages(
    pool: &SqlitePool,
    legacy_thread_id: &str,
) -> anyhow::Result<Vec<HistoricalAgentMessage>> {
    if !table_exists_on(pool, "historical_agent_messages_v1").await? {
        return Ok(Vec::new());
    }
    let rows = sqlx::query(
        "SELECT id, thread_id, role, content, created_at
         FROM historical_agent_messages_v1
         WHERE thread_id = ?1
         ORDER BY id",
    )
    .bind(legacy_thread_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|row| HistoricalAgentMessage {
            id: row.get("id"),
            thread_id: row.get("thread_id"),
            role: row.get("role"),
            content: row.get("content"),
            created_at: row.get("created_at"),
        })
        .collect())
}

async fn archive_legacy_v1_tables(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> anyhow::Result<()> {
    let had_legacy_threads = table_exists(&mut **tx, "agent_threads").await?;
    if had_legacy_threads {
        if !column_exists(&mut **tx, "agent_threads", "id").await?
            || column_exists(&mut **tx, "agent_threads", "thread_id").await?
        {
            bail!("agent_threads exists without a V1 marker; refusing destructive migration");
        }
        if table_exists(&mut **tx, "historical_agent_threads_v1").await? {
            bail!("historical_agent_threads_v1 already exists; refusing to overwrite archive");
        }
        sqlx::query("ALTER TABLE agent_threads RENAME TO historical_agent_threads_v1")
            .execute(&mut **tx)
            .await?;
    }

    if table_exists(&mut **tx, "agent_thread_messages").await? {
        if table_exists(&mut **tx, "historical_agent_messages_v1").await? {
            bail!("historical_agent_messages_v1 already exists; refusing to overwrite archive");
        }
        if had_legacy_threads {
            sqlx::query(
                "ALTER TABLE agent_thread_messages RENAME TO historical_agent_messages_v1",
            )
            .execute(&mut **tx)
            .await?;
        } else {
            sqlx::query(
                "CREATE TABLE historical_agent_messages_v1 (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    thread_id TEXT NOT NULL,
                    role TEXT NOT NULL,
                    content TEXT NOT NULL,
                    created_at TEXT NOT NULL
                )",
            )
            .execute(&mut **tx)
            .await?;
            sqlx::query(
                "INSERT INTO historical_agent_messages_v1(id, thread_id, role, content, created_at)
                    SELECT id, thread_id, role, content, created_at
                    FROM agent_thread_messages",
            )
            .execute(&mut **tx)
            .await?;
            sqlx::query("DROP TABLE agent_thread_messages")
                .execute(&mut **tx)
                .await?;
        }
    }
    Ok(())
}

async fn read_schema_version(pool: &SqlitePool) -> anyhow::Result<Option<i32>> {
    if !table_exists_on(pool, "schema_meta").await? {
        return Ok(None);
    }
    let raw: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM schema_meta WHERE key = 'schema_version'",
    )
    .fetch_optional(pool)
    .await?;
    raw.map(|(value,)| {
        value
            .parse::<i32>()
            .with_context(|| format!("invalid subagent graph schema version {value:?}"))
    })
    .transpose()
}

async fn table_exists_on(pool: &SqlitePool, table: &str) -> anyhow::Result<bool> {
    let (exists,): (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
    )
    .bind(table)
    .fetch_one(pool)
    .await?;
    Ok(exists)
}

async fn table_exists(
    conn: &mut sqlx::SqliteConnection,
    table: &str,
) -> anyhow::Result<bool> {
    let (exists,): (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
    )
    .bind(table)
    .fetch_one(&mut *conn)
    .await?;
    Ok(exists)
}

async fn column_exists(
    conn: &mut sqlx::SqliteConnection,
    table: &str,
    column: &str,
) -> anyhow::Result<bool> {
    let sql = format!("PRAGMA table_info({table})");
    let rows = sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows.iter().any(|row| {
        let name: String = row.get(1);
        name == column
    }))
}

async fn ensure_runtime_descriptor_recovery_state(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> anyhow::Result<()> {
    if !column_exists(&mut **tx, "agent_runtime_descriptors", "recovery_state").await? {
        sqlx::query(
            "ALTER TABLE agent_runtime_descriptors
             ADD COLUMN recovery_state TEXT NOT NULL DEFAULT 'available'
             CHECK(recovery_state IN ('available', 'legacy_unavailable'))",
        )
        .execute(&mut **tx)
        .await?;
    }
    sqlx::query(
        "INSERT OR IGNORE INTO agent_runtime_descriptors (
             thread_id, model, reasoning_effort, recovery_state
         )
         SELECT thread_id, NULL, NULL, 'legacy_unavailable'
         FROM agent_threads
         WHERE parent_thread_id IS NOT NULL",
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use agent_db::sqlx::{self, Row};
    use agent_db::SqlitePool;

    use crate::AgentGraphStore;

    const LEGACY_DDL: &str = r#"
        CREATE TABLE agent_threads (
            id TEXT PRIMARY KEY,
            parent_session_id TEXT NOT NULL,
            parent_agent_id TEXT NOT NULL,
            agent_name TEXT NOT NULL,
            task TEXT NOT NULL,
            status TEXT NOT NULL,
            summary TEXT,
            error TEXT,
            model TEXT,
            model_reasoning_effort TEXT,
            sandbox_mode TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            finished_at TEXT,
            closed_at TEXT
        );
        CREATE TABLE agent_thread_messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            thread_id TEXT NOT NULL,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at TEXT NOT NULL,
            FOREIGN KEY(thread_id) REFERENCES agent_threads(id) ON DELETE CASCADE
        );
    "#;

    async fn open_fixture_pool(path: &std::path::Path) -> SqlitePool {
        use agent_db::sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        use agent_db::sqlx::ConnectOptions;
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

    async fn create_v1_fixture(path: &std::path::Path, include_messages_table: bool) {
        let pool = open_fixture_pool(path).await;
        if include_messages_table {
            sqlx::query(LEGACY_DDL).execute(&pool).await.unwrap();
        } else {
            sqlx::query(
                LEGACY_DDL
                    .split("CREATE TABLE agent_thread_messages")
                    .next()
                    .unwrap(),
            )
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query(
            "INSERT INTO agent_threads (
                id, parent_session_id, parent_agent_id, agent_name, task, status,
                summary, error, model, model_reasoning_effort, sandbox_mode,
                created_at, updated_at, finished_at, closed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, ?9, ?10, ?11, ?12, ?13, NULL)",
        )
        .bind("legacy-thread")
        .bind("legacy-session")
        .bind("astro")
        .bind("explorer")
        .bind("inspect history")
        .bind("completed")
        .bind("found it")
        .bind("gpt-5")
        .bind("high")
        .bind("read-only")
        .bind("2026-08-17T00:00:00Z")
        .bind("2026-08-17T00:01:00Z")
        .bind("2026-08-17T00:01:00Z")
        .execute(&pool)
        .await
        .unwrap();
        if include_messages_table {
            sqlx::query(
                "INSERT INTO agent_thread_messages(thread_id, role, content, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .bind("legacy-thread")
            .bind("assistant")
            .bind("historical answer")
            .bind("2026-08-17T00:01:00Z")
            .execute(&pool)
            .await
            .unwrap();
        }
        pool.close().await;
    }

    #[tokio::test]
    async fn migrates_v1_rows_to_read_only_archive_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subagents.db");
        create_v1_fixture(&path, true).await;

        let store = AgentGraphStore::open(path.clone()).await.unwrap();
        assert_eq!(store.schema_version().await.unwrap(), 4);
        let historical_threads = store.list_historical_threads().await.unwrap();
        assert_eq!(historical_threads.len(), 1);
        assert_eq!(historical_threads[0].id, "legacy-thread");
        let historical_messages = store.list_historical_messages("legacy-thread").await.unwrap();
        assert_eq!(historical_messages.len(), 1);
        assert_eq!(historical_messages[0].content, "historical answer");
        assert!(store.get_thread("legacy-thread").await.unwrap().is_none());
        assert!(store.snapshot("legacy-thread").await.unwrap().threads.is_empty());
        drop(store);

        let reopened = AgentGraphStore::open(path).await.unwrap();
        assert_eq!(reopened.list_historical_threads().await.unwrap().len(), 1);
        assert_eq!(
            reopened
                .list_historical_messages("legacy-thread")
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn fresh_database_has_v4_schema_without_history() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents.db")).await.unwrap();

        assert_eq!(store.schema_version().await.unwrap(), 4);
        assert!(store.list_historical_threads().await.unwrap().is_empty());
        assert!(store
            .list_historical_messages("missing")
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn migrates_v2_schema_additively_and_preserves_threads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subagents-v2.db");
        let pool = open_fixture_pool(&path).await;
        sqlx::query(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE schema_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO schema_meta(key, value) VALUES ('schema_version', '2');
             CREATE TABLE agent_threads (
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
             INSERT INTO agent_threads VALUES (
                 'root', 'root', NULL, '/root', 'root', 'root', 'root',
                 'running', '{\"kind\":\"running\"}', 0,
                 '2026-08-19T00:00:00Z', '2026-08-19T00:00:00Z'
             );
             INSERT INTO agent_threads VALUES (
                 'child', 'root', 'root', '/root/child', 'child', 'default', 'child-session',
                 'interrupted', '{\"kind\":\"interrupted\",\"reason\":\"restart\"}', 0,
                 '2026-08-19T00:00:00Z', '2026-08-19T00:00:00Z'
             );",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;

        let store = AgentGraphStore::open(path.clone()).await.unwrap();
        assert_eq!(store.schema_version().await.unwrap(), 4);
        assert!(store.get_thread("root").await.unwrap().is_some());
        drop(store);

        let pool = open_fixture_pool(&path).await;
        let (descriptor_table,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name = 'agent_runtime_descriptors'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(descriptor_table, 1);
        let (legacy_markers,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM agent_runtime_descriptors
             WHERE recovery_state = 'legacy_unavailable'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(legacy_markers, 1);
    }

    #[tokio::test]
    async fn self_heals_early_v3_descriptor_table_and_marks_missing_children() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subagents-v2.db");
        let pool = open_fixture_pool(&path).await;
        sqlx::query(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE schema_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             INSERT INTO schema_meta(key, value) VALUES ('schema_version', '3');
             CREATE TABLE agent_threads (
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
             INSERT INTO agent_threads VALUES (
                 'root', 'root', NULL, '/root', 'root', 'root', 'root',
                 'running', '{\"kind\":\"running\"}', 0,
                 '2026-08-19T00:00:00Z', '2026-08-19T00:00:00Z'
             );
             INSERT INTO agent_threads VALUES (
                 'child', 'root', 'root', '/root/child', 'child', 'default', 'child-session',
                 'interrupted', '{\"kind\":\"interrupted\",\"reason\":\"restart\"}', 0,
                 '2026-08-19T00:00:00Z', '2026-08-19T00:00:00Z'
             );
             INSERT INTO agent_threads VALUES (
                 'missing', 'root', 'root', '/root/missing', 'missing', 'default', 'missing-session',
                 'interrupted', '{\"kind\":\"interrupted\",\"reason\":\"restart\"}', 0,
                 '2026-08-19T00:00:00Z', '2026-08-19T00:00:00Z'
             );
             CREATE TABLE agent_runtime_descriptors (
                 thread_id TEXT PRIMARY KEY,
                 model TEXT,
                 reasoning_effort TEXT,
                 FOREIGN KEY(thread_id) REFERENCES agent_threads(thread_id) ON DELETE CASCADE
             );
             INSERT INTO agent_runtime_descriptors VALUES (
                 'child', 'openai:trusted-v3-model', 'high'
             );",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;

        let store = AgentGraphStore::open(path.clone()).await.unwrap();
        let descriptor = store.runtime_descriptor("child").await.unwrap().unwrap();
        assert_eq!(descriptor.model.as_deref(), Some("openai:trusted-v3-model"));
        assert_eq!(descriptor.reasoning_effort.as_deref(), Some("high"));
        let error = store.runtime_descriptor("missing").await.unwrap_err();
        assert!(error
            .downcast_ref::<crate::LegacyRuntimeDescriptorUnavailable>()
            .is_some());
        drop(store);
        let pool = open_fixture_pool(&path).await;
        let states = sqlx::query("SELECT thread_id, recovery_state FROM agent_runtime_descriptors ORDER BY thread_id")
            .fetch_all(&pool)
            .await
            .unwrap()
            .iter()
            .map(|row| (row.get::<String, _>("thread_id"), row.get::<String, _>("recovery_state")))
            .collect::<Vec<_>>();
        assert_eq!(
            states,
            vec![
                ("child".into(), "available".into()),
                ("missing".into(), "legacy_unavailable".into()),
            ]
        );
    }

    #[tokio::test]
    async fn migration_tolerates_missing_legacy_messages_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subagents.db");
        create_v1_fixture(&path, false).await;

        let store = AgentGraphStore::open(path).await.unwrap();
        assert_eq!(store.list_historical_threads().await.unwrap().len(), 1);
        assert!(store
            .list_historical_messages("legacy-thread")
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn migration_archives_orphaned_legacy_messages_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subagents.db");
        let pool = open_fixture_pool(&path).await;
        sqlx::query(
            "PRAGMA foreign_keys=OFF;
            CREATE TABLE agent_thread_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                thread_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY(thread_id) REFERENCES agent_threads(id) ON DELETE CASCADE
            );
            INSERT INTO agent_thread_messages(thread_id, role, content, created_at)
            VALUES ('orphan-thread', 'assistant', 'preserve me', '2026-08-18T00:00:00Z');",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;

        let store = AgentGraphStore::open(path.clone()).await.unwrap();
        assert_eq!(store.schema_version().await.unwrap(), 4);
        let archived = store.list_historical_messages("orphan-thread").await.unwrap();
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].content, "preserve me");

        let pool = open_fixture_pool(&path).await;
        let (has_old,): (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'agent_thread_messages')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(!has_old);
        let (has_archive,): (bool,) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'historical_agent_messages_v1')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(has_archive);
        let fk_rows = sqlx::query("PRAGMA foreign_key_list(historical_agent_messages_v1)")
            .fetch_all(&pool)
            .await
            .unwrap();
        assert!(!fk_rows.iter().any(|row| {
            let table: String = row.get(2);
            table == "agent_threads"
        }));
    }
}
