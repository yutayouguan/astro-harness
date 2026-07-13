use memory::session_store::{NewMessage, SessionStore};
use tempfile::TempDir;

#[test]
fn opens_fresh_db_at_schema_v11() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.db");
    let store = SessionStore::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), 11);
    store
        .create_session("s1", "test", None, None, None)
        .unwrap();
    assert!(path.is_file());
}

#[test]
fn append_and_reload_tool_calls_and_reasoning() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).unwrap();
    store.create_session("s1", "test", None, None, None).unwrap();
    store
        .append_message(memory::session_store::NewMessage {
            session_id: "s1",
            role: "assistant",
            content: Some("done"),
            tool_calls: Some(serde_json::json!([{
                "id": "c1",
                "name": "memory_add",
                "arguments": {"entry": "x"}
            }])),
            tool_call_id: None,
            tool_name: None,
            token_count: Some(12),
            finish_reason: Some("tool_calls"),
            reasoning: Some("think"),
            reasoning_content: None,
            reasoning_details: None,
            codex_reasoning_items: None,
            codex_message_items: None,
        })
        .unwrap();
    store
        .append_message(memory::session_store::NewMessage {
            session_id: "s1",
            role: "tool",
            content: Some("ok"),
            tool_calls: None,
            tool_call_id: Some("c1"),
            tool_name: Some("memory_add"),
            token_count: None,
            finish_reason: None,
            reasoning: None,
            reasoning_content: None,
            reasoning_details: None,
            codex_reasoning_items: None,
            codex_message_items: None,
        })
        .unwrap();

    let msgs = store.get_messages("s1").unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].reasoning.as_deref(), Some("think"));
    assert!(msgs[0].tool_calls.is_some());
    assert_eq!(msgs[1].tool_call_id.as_deref(), Some("c1"));

    let conv = store.get_messages_as_conversation("s1").unwrap();
    assert_eq!(conv[0]["role"], "assistant");
    assert!(conv[0].get("tool_calls").is_some());
    assert_eq!(conv[0]["reasoning"], "think");
    assert_eq!(conv[1]["role"], "tool");
}

#[test]
fn search_messages_hits_tool_name_and_cjk() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).unwrap();
    store.create_session("s1", "tauri", None, None, None).unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "tool",
            content: Some("已写入长期记忆"),
            tool_name: Some("memory_add"),
            tool_call_id: Some("c1"),
            ..NewMessage::empty("s1", "tool")
        })
        .unwrap();
    let hits = store.search_messages("memory_add", None, None, 10).unwrap();
    assert!(!hits.is_empty());
    let hits2 = store.search_messages("长期记忆", None, None, 10).unwrap();
    assert!(!hits2.is_empty());
}

#[test]
fn build_chat_history_folds_tools_into_activities() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).unwrap();
    store.create_session("s1", "tauri", None, None, None).unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "user",
            content: Some("hi"),
            ..NewMessage::empty("s1", "user")
        })
        .unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "assistant",
            content: Some(""),
            reasoning: Some("plan"),
            tool_calls: Some(serde_json::json!([{
                "id": "c1", "name": "memory_add",
                "arguments": {"entry": "e", "target": "project"}
            }])),
            ..NewMessage::empty("s1", "assistant")
        })
        .unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "tool",
            content: Some("ok"),
            tool_call_id: Some("c1"),
            tool_name: Some("memory_add"),
            ..NewMessage::empty("s1", "tool")
        })
        .unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "assistant",
            content: Some("done"),
            ..NewMessage::empty("s1", "assistant")
        })
        .unwrap();

    let ui = store.build_chat_history("s1", 200).unwrap();
    assert_eq!(ui.len(), 3); // user + assistant(with activity) + assistant(text)
    assert_eq!(ui[1].reasoning.as_deref(), Some("plan"));
    assert_eq!(ui[1].activities.len(), 1);
    assert_eq!(ui[1].activities[0].title, "memory_add");
    assert_eq!(ui[1].activities[0].output.as_deref(), Some("ok"));
}

#[test]
fn migrates_legacy_messages_and_sessions_db() {
    let dir = TempDir::new().unwrap();
    let sessions_dir = dir.path().join("sessions");
    std::fs::create_dir_all(&sessions_dir).unwrap();

    // 旧 state.db：瘦 messages
    {
        let conn = rusqlite::Connection::open(sessions_dir.join("state.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
             );
             INSERT INTO messages(session_id, role, content) VALUES ('old','user','hello');",
        )
        .unwrap();
    }
    // 旧 sessions.db
    {
        let conn = rusqlite::Connection::open(sessions_dir.join("sessions.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (
                session_id TEXT PRIMARY KEY,
                summary TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             INSERT INTO sessions(session_id, summary) VALUES ('old','hello summary');",
        )
        .unwrap();
    }

    let store = SessionStore::open_with_legacy_migration(&sessions_dir).unwrap();
    assert_eq!(store.schema_version().unwrap(), 11);
    let msgs = store.get_messages("old").unwrap();
    assert_eq!(msgs[0].content.as_deref(), Some("hello"));
    let sess = store.get_session("old").unwrap().unwrap();
    assert!(sess.title.as_deref().unwrap_or("").contains("hello"));

    // v11：content 必须可空（旧表为 NOT NULL，迁移应整表重建）。
    store
        .append_message(NewMessage {
            session_id: "old",
            role: "assistant",
            content: None,
            tool_calls: Some(serde_json::json!([{
                "id": "c1",
                "name": "noop",
                "arguments": {}
            }])),
            ..NewMessage::empty("old", "assistant")
        })
        .unwrap();
    let msgs = store.get_messages("old").unwrap();
    assert!(msgs.last().unwrap().content.is_none());

    // 幂等
    let _ = SessionStore::open_with_legacy_migration(&sessions_dir).unwrap();
}

#[test]
fn legacy_migration_allows_duplicate_session_summaries() {
    let dir = TempDir::new().unwrap();
    let sessions_dir = dir.path().join("sessions");
    std::fs::create_dir_all(&sessions_dir).unwrap();

    {
        let conn = rusqlite::Connection::open(sessions_dir.join("sessions.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE sessions (
                session_id TEXT PRIMARY KEY,
                summary TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             INSERT INTO sessions(session_id, summary) VALUES
               ('dup-a', 'shared summary'),
               ('dup-b', 'shared summary');",
        )
        .unwrap();
    }

    let store = SessionStore::open_with_legacy_migration(&sessions_dir).unwrap();
    let a = store.get_session("dup-a").unwrap().unwrap();
    let b = store.get_session("dup-b").unwrap().unwrap();
    let titled = [a.title.as_deref(), b.title.as_deref()];
    assert_eq!(
        titled.iter().filter(|t| t.is_some()).count(),
        1,
        "exactly one session keeps the shared title"
    );
    assert_eq!(
        titled.iter().filter(|t| t.is_none()).count(),
        1,
        "the other session falls back to NULL title"
    );
    assert!(titled.iter().any(|t| *t == Some("shared summary")));
}

#[test]
fn memory_manager_record_message_uses_session_store() {
    let dir = TempDir::new().unwrap();
    let mgr = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
    mgr.ensure_session("s1", "test").unwrap();
    let id = mgr
        .record_message_ex(
            "s1",
            memory::session_store::NewMessage {
                content: Some("hi"),
                ..memory::session_store::NewMessage::empty("s1", "user")
            },
        )
        .unwrap();
    assert!(id > 0);

    let msgs = mgr.session_store.get_messages("s1").unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].content.as_deref(), Some("hi"));

    let thin = mgr.record_message("s1", "assistant", "yo").unwrap();
    assert!(thin > id);

    let hits = mgr.handle_session_search("hi", 5).unwrap();
    assert!(hits.contains("相关历史消息"));
    assert!(hits.contains("hi"));
}
