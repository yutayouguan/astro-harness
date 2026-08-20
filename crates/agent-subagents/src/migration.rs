//! 子 Agent 图数据库 schema 迁移：v1 归档、v2→v4 增量升级。

use anyhow::{bail, Context};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
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

/// 从 v1 归档的历史线程记录（只读）。
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

/// 从 v1 归档的历史消息记录（只读）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoricalAgentMessage {
    pub id: i64,
    pub thread_id: String,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

/// 执行 schema 迁移：检测当前版本并逐步升级到 v4。
pub(crate) fn migrate(conn: &mut Connection) -> anyhow::Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let existing_version = read_schema_version(&tx)?;
    match existing_version {
        Some(SCHEMA_VERSION) => {
            tx.commit()?;
            return Ok(());
        }
        Some(3) => {
            ensure_runtime_descriptor_recovery_state(&tx)?;
            tx.execute(
                "UPDATE schema_meta SET value = ?1 WHERE key = 'schema_version'",
                [SCHEMA_VERSION.to_string()],
            )?;
            tx.commit()?;
            return Ok(());
        }
        Some(2) => {
            tx.execute_batch(V4_DDL)?;
            ensure_runtime_descriptor_recovery_state(&tx)?;
            tx.execute(
                "UPDATE schema_meta SET value = ?1 WHERE key = 'schema_version'",
                [SCHEMA_VERSION.to_string()],
            )?;
            tx.commit()?;
            return Ok(());
        }
        Some(version) => {
            bail!("unsupported subagent graph schema version {version}; expected {SCHEMA_VERSION}")
        }
        None => {}
    }

    archive_legacy_v1_tables(&tx)?;
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )?;
    tx.execute_batch(V2_DDL)?;
    tx.execute(
        "INSERT INTO schema_meta(key, value) VALUES ('schema_version', ?1)",
        [SCHEMA_VERSION.to_string()],
    )?;
    tx.commit()?;
    Ok(())
}

pub(crate) fn schema_version(conn: &Connection) -> anyhow::Result<i32> {
    read_schema_version(conn)?.context("subagent graph schema_version is missing")
}

/// 查询 v1 归档的所有历史线程。
pub(crate) fn list_historical_threads(
    conn: &Connection,
) -> anyhow::Result<Vec<HistoricalAgentThread>> {
    if !table_exists(conn, "historical_agent_threads_v1")? {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT id, parent_session_id, parent_agent_id, agent_name, task, status,
                summary, error, model, model_reasoning_effort, sandbox_mode,
                created_at, updated_at, finished_at, closed_at
         FROM historical_agent_threads_v1
         ORDER BY created_at, id",
    )?;
    let threads = stmt
        .query_map([], |row| {
            Ok(HistoricalAgentThread {
                id: row.get(0)?,
                parent_session_id: row.get(1)?,
                parent_agent_id: row.get(2)?,
                agent_name: row.get(3)?,
                task: row.get(4)?,
                status: row.get(5)?,
                summary: row.get(6)?,
                error: row.get(7)?,
                model: row.get(8)?,
                model_reasoning_effort: row.get(9)?,
                sandbox_mode: row.get(10)?,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
                finished_at: row.get(13)?,
                closed_at: row.get(14)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(threads)
}

/// 查询指定 v1 历史线程的所有消息。
pub(crate) fn list_historical_messages(
    conn: &Connection,
    legacy_thread_id: &str,
) -> anyhow::Result<Vec<HistoricalAgentMessage>> {
    if !table_exists(conn, "historical_agent_messages_v1")? {
        return Ok(Vec::new());
    }
    let mut stmt = conn.prepare(
        "SELECT id, thread_id, role, content, created_at
         FROM historical_agent_messages_v1
         WHERE thread_id = ?1
         ORDER BY id",
    )?;
    let messages = stmt
        .query_map([legacy_thread_id], |row| {
            Ok(HistoricalAgentMessage {
                id: row.get(0)?,
                thread_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(messages)
}

fn archive_legacy_v1_tables(tx: &Transaction<'_>) -> anyhow::Result<()> {
    let had_legacy_threads = table_exists(tx, "agent_threads")?;
    if had_legacy_threads {
        if !column_exists(tx, "agent_threads", "id")?
            || column_exists(tx, "agent_threads", "thread_id")?
        {
            bail!("agent_threads exists without a V1 marker; refusing destructive migration");
        }
        if table_exists(tx, "historical_agent_threads_v1")? {
            bail!("historical_agent_threads_v1 already exists; refusing to overwrite archive");
        }
        tx.execute_batch("ALTER TABLE agent_threads RENAME TO historical_agent_threads_v1;")?;
    }

    if table_exists(tx, "agent_thread_messages")? {
        if table_exists(tx, "historical_agent_messages_v1")? {
            bail!("historical_agent_messages_v1 already exists; refusing to overwrite archive");
        }
        if had_legacy_threads {
            tx.execute_batch(
                "ALTER TABLE agent_thread_messages RENAME TO historical_agent_messages_v1;",
            )?;
        } else {
            // With no legacy parent table, renaming would leave the archived
            // foreign key pointing at the new V2 `agent_threads`. Rebuild the
            // read-only archive without that unsafe cross-schema constraint.
            tx.execute_batch(
                "CREATE TABLE historical_agent_messages_v1 (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    thread_id TEXT NOT NULL,
                    role TEXT NOT NULL,
                    content TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                INSERT INTO historical_agent_messages_v1(id, thread_id, role, content, created_at)
                    SELECT id, thread_id, role, content, created_at
                    FROM agent_thread_messages;
                DROP TABLE agent_thread_messages;",
            )?;
        }
    }
    Ok(())
}

fn read_schema_version(conn: &Connection) -> anyhow::Result<Option<i32>> {
    if !table_exists(conn, "schema_meta")? {
        return Ok(None);
    }
    let raw = conn
        .query_row(
            "SELECT value FROM schema_meta WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    raw.map(|value| {
        value
            .parse::<i32>()
            .with_context(|| format!("invalid subagent graph schema version {value:?}"))
    })
    .transpose()
}

fn table_exists(conn: &Connection, table: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1
        )",
        [table],
        |row| row.get(0),
    )
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(columns.iter().any(|candidate| candidate == column))
}

fn ensure_runtime_descriptor_recovery_state(conn: &Connection) -> anyhow::Result<()> {
    if !column_exists(conn, "agent_runtime_descriptors", "recovery_state")? {
        conn.execute(
            "ALTER TABLE agent_runtime_descriptors
             ADD COLUMN recovery_state TEXT NOT NULL DEFAULT 'available'
             CHECK(recovery_state IN ('available', 'legacy_unavailable'))",
            [],
        )?;
    }
    conn.execute(
        "INSERT OR IGNORE INTO agent_runtime_descriptors (
             thread_id, model, reasoning_effort, recovery_state
         )
         SELECT thread_id, NULL, NULL, 'legacy_unavailable'
         FROM agent_threads
         WHERE parent_thread_id IS NOT NULL",
        [],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rusqlite::{params, Connection};

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

    fn create_v1_fixture(path: &std::path::Path, include_messages_table: bool) {
        let conn = Connection::open(path).unwrap();
        if include_messages_table {
            conn.execute_batch(LEGACY_DDL).unwrap();
        } else {
            conn.execute_batch(
                LEGACY_DDL
                    .split("CREATE TABLE agent_thread_messages")
                    .next()
                    .unwrap(),
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO agent_threads (
                id, parent_session_id, parent_agent_id, agent_name, task, status,
                summary, error, model, model_reasoning_effort, sandbox_mode,
                created_at, updated_at, finished_at, closed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, ?9, ?10, ?11, ?12, ?13, NULL)",
            params![
                "legacy-thread",
                "legacy-session",
                "astro",
                "explorer",
                "inspect history",
                "completed",
                "found it",
                "gpt-5",
                "high",
                "read-only",
                "2026-08-17T00:00:00Z",
                "2026-08-17T00:01:00Z",
                "2026-08-17T00:01:00Z",
            ],
        )
        .unwrap();
        if include_messages_table {
            conn.execute(
                "INSERT INTO agent_thread_messages(thread_id, role, content, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    "legacy-thread",
                    "assistant",
                    "historical answer",
                    "2026-08-17T00:01:00Z"
                ],
            )
            .unwrap();
        }
    }

    #[test]
    fn migrates_v1_rows_to_read_only_archive_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subagents.db");
        create_v1_fixture(&path, true);

        let store = AgentGraphStore::open(path.clone()).unwrap();
        assert_eq!(store.schema_version().unwrap(), 4);
        let historical_threads = store.list_historical_threads().unwrap();
        assert_eq!(historical_threads.len(), 1);
        assert_eq!(historical_threads[0].id, "legacy-thread");
        let historical_messages = store.list_historical_messages("legacy-thread").unwrap();
        assert_eq!(historical_messages.len(), 1);
        assert_eq!(historical_messages[0].content, "historical answer");
        assert!(store.get_thread("legacy-thread").unwrap().is_none());
        assert!(store.snapshot("legacy-thread").unwrap().threads.is_empty());
        drop(store);

        let reopened = AgentGraphStore::open(path).unwrap();
        assert_eq!(reopened.list_historical_threads().unwrap().len(), 1);
        assert_eq!(
            reopened
                .list_historical_messages("legacy-thread")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn fresh_database_has_v4_schema_without_history() {
        let dir = tempfile::tempdir().unwrap();
        let store = AgentGraphStore::open(dir.path().join("subagents.db")).unwrap();

        assert_eq!(store.schema_version().unwrap(), 4);
        assert!(store.list_historical_threads().unwrap().is_empty());
        assert!(store
            .list_historical_messages("missing")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn migrates_v2_schema_additively_and_preserves_threads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subagents-v2.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
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
        .unwrap();
        drop(conn);

        let store = AgentGraphStore::open(path.clone()).unwrap();
        assert_eq!(store.schema_version().unwrap(), 4);
        assert!(store.get_thread("root").unwrap().is_some());
        drop(store);

        let conn = Connection::open(path).unwrap();
        let descriptor_table: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'table' AND name = 'agent_runtime_descriptors'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(descriptor_table, 1);
        let legacy_markers: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM agent_runtime_descriptors
                 WHERE recovery_state = 'legacy_unavailable'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(legacy_markers, 1);
    }

    #[test]
    fn self_heals_early_v3_descriptor_table_and_marks_missing_children() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subagents-v2.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
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
        .unwrap();
        drop(conn);

        let store = AgentGraphStore::open(path.clone()).unwrap();
        let descriptor = store.runtime_descriptor("child").unwrap().unwrap();
        assert_eq!(descriptor.model.as_deref(), Some("openai:trusted-v3-model"));
        assert_eq!(descriptor.reasoning_effort.as_deref(), Some("high"));
        let error = store.runtime_descriptor("missing").unwrap_err();
        assert!(error
            .downcast_ref::<crate::LegacyRuntimeDescriptorUnavailable>()
            .is_some());
        drop(store);
        let conn = Connection::open(path).unwrap();
        let states = conn
            .prepare(
                "SELECT thread_id, recovery_state FROM agent_runtime_descriptors
                 ORDER BY thread_id",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(
            states,
            vec![
                ("child".into(), "available".into()),
                ("missing".into(), "legacy_unavailable".into()),
            ]
        );
    }

    #[test]
    fn migration_tolerates_missing_legacy_messages_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subagents.db");
        create_v1_fixture(&path, false);

        let store = AgentGraphStore::open(path).unwrap();
        assert_eq!(store.list_historical_threads().unwrap().len(), 1);
        assert!(store
            .list_historical_messages("legacy-thread")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn migration_archives_orphaned_legacy_messages_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subagents.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
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
        .unwrap();
        drop(conn);

        let store = AgentGraphStore::open(path.clone()).unwrap();
        assert_eq!(store.schema_version().unwrap(), 4);
        let archived = store.list_historical_messages("orphan-thread").unwrap();
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].content, "preserve me");

        let conn = Connection::open(path).unwrap();
        assert!(!super::table_exists(&conn, "agent_thread_messages").unwrap());
        assert!(super::table_exists(&conn, "historical_agent_messages_v1").unwrap());
        let foreign_key_targets = conn
            .prepare("PRAGMA foreign_key_list(historical_agent_messages_v1)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(2))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert!(!foreign_key_targets
            .iter()
            .any(|table| table == "agent_threads"));
    }
}
