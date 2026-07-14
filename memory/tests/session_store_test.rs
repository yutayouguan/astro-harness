use memory::session_store::{BillingDelta, NewMessage, SessionStore};
use tempfile::TempDir;

#[test]
fn opens_fresh_db_at_schema_v11() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.db");
    let store = SessionStore::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), 13);
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
                "name": "memory",
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
            tool_name: Some("memory"),
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
            tool_name: Some("memory"),
            tool_call_id: Some("c1"),
            ..NewMessage::empty("s1", "tool")
        })
        .unwrap();
    let hits = store.search_messages("memory", None, None, 10).unwrap();
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
                "id": "c1", "name": "memory",
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
            tool_name: Some("memory"),
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
    assert_eq!(ui[1].activities[0].title, "memory");
    assert_eq!(ui[1].activities[0].output.as_deref(), Some("ok"));
}

#[test]
fn build_chat_history_restores_timeline() {
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
    let details = serde_json::json!({
        "astro_timeline_v1": [
            {"type": "reasoning", "id": "r1", "text": "think", "at": 1},
            {"type": "activity", "id": "c1", "at": 2},
            {"type": "surface", "id": "a2ui-surface-c1", "at": 3}
        ],
        "astro_surfaces_v1": [{
            "messageId": "a2ui-surface-c1",
            "activityType": "a2ui-surface",
            "operations": [{"version": "v0.9"}],
            "status": "active"
        }]
    });
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "assistant",
            content: Some("card shown"),
            reasoning: Some("think"),
            tool_calls: Some(serde_json::json!([{
                "id": "c1", "name": "present_ui", "arguments": {}
            }])),
            reasoning_details: Some(details),
            ..NewMessage::empty("s1", "assistant")
        })
        .unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "tool",
            content: Some("Presented info card"),
            tool_call_id: Some("c1"),
            tool_name: Some("present_ui"),
            ..NewMessage::empty("s1", "tool")
        })
        .unwrap();

    let ui = store.build_chat_history("s1", 200).unwrap();
    assert_eq!(ui.len(), 2);
    let segs = ui[1].segments.as_ref().unwrap().as_array().unwrap();
    assert_eq!(segs.len(), 3);
    assert_eq!(segs[0]["type"], "reasoning");
    assert_eq!(segs[1]["type"], "activity");
    assert_eq!(segs[2]["type"], "surface");
    let surfaces = ui[1].ui_surfaces.as_ref().unwrap().as_array().unwrap();
    assert_eq!(surfaces.len(), 1);
    assert_eq!(surfaces[0]["messageId"], "a2ui-surface-c1");
    // tool 合并不应清掉 timeline
    assert_eq!(ui[1].activities.len(), 1);
    assert_eq!(ui[1].activities[0].output.as_deref(), Some("Presented info card"));
}

#[test]
fn discards_legacy_messages_and_sessions_db() {
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
    let legacy_path = sessions_dir.join("sessions.db");
    {
        let conn = rusqlite::Connection::open(&legacy_path).unwrap();
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

    let store = SessionStore::open_sessions_dir(&sessions_dir).unwrap();
    assert_eq!(store.schema_version().unwrap(), 13);
    assert!(
        !legacy_path.exists(),
        "legacy sessions.db must be deleted, not imported"
    );
    assert!(
        store.get_messages("old").unwrap().is_empty(),
        "old chat history must be discarded"
    );
    assert!(store.get_session("old").unwrap().is_none());

    store.create_session("fresh", "test", None, None, None).unwrap();
    store
        .append_message(NewMessage {
            session_id: "fresh",
            role: "assistant",
            content: None,
            tool_calls: Some(serde_json::json!([{
                "id": "c1",
                "name": "noop",
                "arguments": {}
            }])),
            ..NewMessage::empty("fresh", "assistant")
        })
        .unwrap();
    let msgs = store.get_messages("fresh").unwrap();
    assert!(msgs.last().unwrap().content.is_none());

    let _ = SessionStore::open_sessions_dir(&sessions_dir).unwrap();
    assert_eq!(store.get_messages("fresh").unwrap().len(), 1);
}

#[test]
fn discards_legacy_sessions_db_without_importing_titles() {
    let dir = TempDir::new().unwrap();
    let sessions_dir = dir.path().join("sessions");
    std::fs::create_dir_all(&sessions_dir).unwrap();

    let legacy_path = sessions_dir.join("sessions.db");
    {
        let conn = rusqlite::Connection::open(&legacy_path).unwrap();
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

    let store = SessionStore::open_sessions_dir(&sessions_dir).unwrap();
    assert!(!legacy_path.exists());
    assert!(store.get_session("dup-a").unwrap().is_none());
    assert!(store.get_session("dup-b").unwrap().is_none());
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

#[test]
fn open_repairs_legacy_fts_triggers_on_v11_db() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.db");
    {
        let store = SessionStore::open(&path).unwrap();
        store.create_session("s1", "test", None, None, None).unwrap();
        store
            .append_message(NewMessage {
                content: Some("hello"),
                ..NewMessage::empty("s1", "user")
            })
            .unwrap();
    }
    // 模拟失效 FTS 触发器被重新挂上。
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TRIGGER sync_messages_to_fts AFTER INSERT ON messages
            BEGIN
                INSERT INTO messages_fts(message_id, content) VALUES (new.id, new.content);
            END;
            CREATE TRIGGER sync_messages_fts_update AFTER UPDATE ON messages
            BEGIN
                UPDATE messages_fts SET content = new.content WHERE message_id = old.id;
            END;
            CREATE TRIGGER sync_messages_fts_delete AFTER DELETE ON messages
            BEGIN
                DELETE FROM messages_fts WHERE message_id = old.id;
            END;
            "#,
        )
        .unwrap();
    }

    let store = SessionStore::open(&path).unwrap();
    store
        .append_message(NewMessage {
            content: Some("world"),
            ..NewMessage::empty("s1", "user")
        })
        .unwrap();
    assert_eq!(store.get_messages("s1").unwrap().len(), 2);
}

#[test]
fn open_repairs_broken_fts_delete_command_triggers() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.db");
    {
        let store = SessionStore::open(&path).unwrap();
        store.create_session("s1", "test", None, None, None).unwrap();
        store
            .append_message(NewMessage {
                content: Some("keep"),
                ..NewMessage::empty("s1", "user")
            })
            .unwrap();
    }
    // 模拟旧 v11 DDL：contentful FTS 上使用 VALUES('delete', …)。
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            r#"
            DROP TRIGGER IF EXISTS messages_fts_delete;
            DROP TRIGGER IF EXISTS messages_fts_update;
            CREATE TRIGGER messages_fts_delete AFTER DELETE ON messages BEGIN
                INSERT INTO messages_fts(messages_fts, rowid, content, tool_name, tool_calls)
                    VALUES ('delete', old.id, old.content, old.tool_name, old.tool_calls);
                INSERT INTO messages_fts_trigram(messages_fts_trigram, rowid, content, tool_name, tool_calls)
                    VALUES ('delete', old.id, old.content, old.tool_name, old.tool_calls);
            END;
            CREATE TRIGGER messages_fts_update AFTER UPDATE ON messages BEGIN
                INSERT INTO messages_fts(messages_fts, rowid, content, tool_name, tool_calls)
                    VALUES ('delete', old.id, old.content, old.tool_name, old.tool_calls);
                INSERT INTO messages_fts(rowid, content, tool_name, tool_calls)
                    VALUES (new.id, new.content, new.tool_name, new.tool_calls);
            END;
            "#,
        )
        .unwrap();
    }

    let id = {
        let store = SessionStore::open(&path).unwrap();
        store
            .append_message(NewMessage {
                content: Some("temp"),
                ..NewMessage::empty("s1", "user")
            })
            .unwrap()
    };
    // 打开后应已换成 DELETE FROM fts，消息可删。
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute("DELETE FROM messages WHERE id = ?1", rusqlite::params![id])
            .unwrap();
    }
    let store = SessionStore::open(&path).unwrap();
    assert_eq!(store.get_messages("s1").unwrap().len(), 1);
}

#[test]
fn ensure_session_is_idempotent() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.db");
    let store = SessionStore::open(&path).unwrap();
    store.ensure_session("s1", "test").unwrap();
    store.ensure_session("s1", "other").unwrap();
    let s = store.get_session("s1").unwrap().unwrap();
    assert_eq!(s.source, "test"); // 冲突时不覆盖
}

#[test]
fn open_backfills_sessions_from_orphan_messages() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.db");
    {
        let _ = SessionStore::open(&path).unwrap();
    }
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        conn.execute(
            "INSERT INTO messages (session_id, role, content, timestamp)
             VALUES ('orphan', 'user', 'hello orphan', 100.0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO messages (session_id, role, content, timestamp)
             VALUES ('orphan', 'assistant', 'hi', 101.0)",
            [],
        )
        .unwrap();
    }
    let store = SessionStore::open(&path).unwrap();
    let s = store.get_session("orphan").unwrap().expect("backfilled");
    assert_eq!(s.source, "tauri");
    assert_eq!(s.message_count, 2);
    assert_eq!(s.started_at, 100.0);
}

#[test]
fn update_session_billing_accumulates_and_unknown_skips_cost() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).unwrap();
    store
        .create_session("s1", "test", Some("gpt"), None, None)
        .unwrap();
    store
        .update_session_billing(
            "s1",
            BillingDelta {
                input_tokens: 10,
                output_tokens: 5,
                cache_read_tokens: 2,
                cache_write_tokens: 1,
                reasoning_tokens: 3,
                estimated_cost_usd: 0.01,
                api_call_count: 1,
                billing_provider: Some("openai".into()),
                billing_base_url: Some("https://api.openai.com/v1".into()),
                billing_mode: Some("official_docs_snapshot".into()),
                cost_status: Some("estimated".into()),
                cost_source: Some("official_docs_snapshot".into()),
                pricing_version: Some("v1".into()),
                model: Some("gpt".into()),
            },
        )
        .unwrap();
    store
        .update_session_billing(
            "s1",
            BillingDelta {
                input_tokens: 1,
                output_tokens: 1,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                reasoning_tokens: 0,
                estimated_cost_usd: 0.0,
                api_call_count: 1,
                billing_provider: None,
                billing_base_url: None,
                billing_mode: None,
                cost_status: Some("unknown".into()),
                cost_source: Some("none".into()),
                pricing_version: None,
                model: None,
            },
        )
        .unwrap();
    let row = store.get_session_billing("s1").unwrap().unwrap();
    assert_eq!(row.input_tokens, 11);
    assert!((row.estimated_cost_usd - 0.01).abs() < 1e-9);
    assert_eq!(row.cost_status.as_deref(), Some("unknown"));
    assert_eq!(row.api_call_count, 2);
}

#[test]
fn outdated_schema_discards_prior_chat_and_billing() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE schema_version (version INTEGER NOT NULL);
             CREATE TABLE state_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                model TEXT,
                started_at REAL NOT NULL,
                message_count INTEGER DEFAULT 0,
                tool_call_count INTEGER DEFAULT 0,
                input_tokens INTEGER DEFAULT 0,
                output_tokens INTEGER DEFAULT 0,
                cache_read_tokens INTEGER DEFAULT 0,
                cache_write_tokens INTEGER DEFAULT 0,
                reasoning_tokens INTEGER DEFAULT 0,
                billing_provider TEXT,
                billing_base_url TEXT,
                billing_mode TEXT,
                estimated_cost_usd REAL,
                actual_cost_usd REAL,
                cost_status TEXT,
                cost_source TEXT,
                pricing_version TEXT,
                api_call_count INTEGER DEFAULT 0
             );
             CREATE TABLE messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT,
                timestamp REAL NOT NULL
             );
             INSERT INTO schema_version (version) VALUES (11);
             INSERT INTO sessions (
                id, source, started_at, input_tokens, output_tokens,
                estimated_cost_usd, actual_cost_usd, cost_status, api_call_count,
                billing_provider
             ) VALUES (
                's1', 'test', 1.0, 500, 200, 9.99, 8.88, 'estimated', 7, 'openai'
             );
             INSERT INTO messages (session_id, role, content, timestamp)
             VALUES ('s1', 'user', 'old chat', 1.0);",
        )
        .unwrap();
    }

    let store = SessionStore::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), 13);
    assert!(store.get_session("s1").unwrap().is_none());
    assert!(store.get_messages("s1").unwrap().is_empty());
}



#[test]
fn fork_session_copies_bubbles_and_trailing_tools() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).unwrap();
    store
        .create_session("src", "test", Some("gpt"), None, None)
        .unwrap();
    store.set_session_title("src", "hello").unwrap();
    store
        .append_message(NewMessage {
            content: Some("u1".into()),
            ..NewMessage::empty("src", "user")
        })
        .unwrap();
    store
        .append_message(NewMessage {
            content: Some("a1".into()),
            tool_calls: Some(serde_json::json!([{ "id": "c1", "name": "x", "arguments": {} }])),
            ..NewMessage::empty("src", "assistant")
        })
        .unwrap();
    store
        .append_message(NewMessage {
            content: Some("tool-out".into()),
            tool_call_id: Some("c1".into()),
            tool_name: Some("x".into()),
            ..NewMessage::empty("src", "tool")
        })
        .unwrap();
    store
        .append_message(NewMessage {
            content: Some("a1b".into()),
            ..NewMessage::empty("src", "assistant")
        })
        .unwrap();
    store
        .append_message(NewMessage {
            content: Some("u2".into()),
            ..NewMessage::empty("src", "user")
        })
        .unwrap();

    // keep 2 bubbles = user + assistant(+trailing tools until next non-tool)
    // After first assistant with tools, we include tool rows then stop before next assistant? 
    // Looking at impl: when bubble count hits keep, it includes following tool rows only.
    // So keep=2: u1, a1(+tool). Not a1b.
    store.fork_session("src", "dst", 2).unwrap();
    let dst = store.get_messages("dst").unwrap();
    assert_eq!(dst.len(), 3);
    assert_eq!(dst[0].content.as_deref(), Some("u1"));
    assert_eq!(dst[1].role, "assistant");
    assert_eq!(dst[2].role, "tool");
    let meta = store.get_session("dst").unwrap().unwrap();
    assert_eq!(meta.parent_session_id.as_deref(), Some("src"));
    assert!(meta.title.as_deref().unwrap_or("").contains("branch"));
}

#[test]
fn truncate_session_to_bubbles_drops_tail_and_trailing_tools() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).unwrap();
    store
        .create_session("s1", "test", Some("gpt"), None, None)
        .unwrap();
    store
        .append_message(NewMessage {
            content: Some("u1".into()),
            ..NewMessage::empty("s1", "user")
        })
        .unwrap();
    store
        .append_message(NewMessage {
            content: Some("a1".into()),
            tool_calls: Some(serde_json::json!([{ "id": "c1", "name": "x", "arguments": {} }])),
            ..NewMessage::empty("s1", "assistant")
        })
        .unwrap();
    store
        .append_message(NewMessage {
            content: Some("tool-out".into()),
            tool_call_id: Some("c1".into()),
            tool_name: Some("x".into()),
            ..NewMessage::empty("s1", "tool")
        })
        .unwrap();
    store
        .append_message(NewMessage {
            content: Some("u2".into()),
            ..NewMessage::empty("s1", "user")
        })
        .unwrap();
    store
        .append_message(NewMessage {
            content: Some("a2".into()),
            ..NewMessage::empty("s1", "assistant")
        })
        .unwrap();

    // keep 1 bubble = only first user；后续 assistant/tool/u2/a2 全删
    store.truncate_session_to_bubbles("s1", 1).unwrap();
    let left = store.get_messages("s1").unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].content.as_deref(), Some("u1"));
    let meta = store.get_session("s1").unwrap().unwrap();
    assert_eq!(meta.message_count, 1);
    assert_eq!(meta.tool_call_count, 0);

    // keep 0 → 清空
    store
        .append_message(NewMessage {
            content: Some("again".into()),
            ..NewMessage::empty("s1", "user")
        })
        .unwrap();
    store.truncate_session_to_bubbles("s1", 0).unwrap();
    assert!(store.get_messages("s1").unwrap().is_empty());
    let meta = store.get_session("s1").unwrap().unwrap();
    assert_eq!(meta.message_count, 0);
}

#[test]
fn remove_chat_bubbles_splices_middle_user_and_tools() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).unwrap();
    store
        .create_session("s1", "test", Some("gpt"), None, None)
        .unwrap();
    // u0 a0(tool) u1 a1 u2
    store
        .append_message(NewMessage {
            content: Some("u0".into()),
            ..NewMessage::empty("s1", "user")
        })
        .unwrap();
    store
        .append_message(NewMessage {
            content: Some("a0".into()),
            tool_calls: Some(serde_json::json!([{ "id": "c0", "name": "x", "arguments": {} }])),
            ..NewMessage::empty("s1", "assistant")
        })
        .unwrap();
    store
        .append_message(NewMessage {
            content: Some("tool0".into()),
            tool_call_id: Some("c0".into()),
            tool_name: Some("x".into()),
            ..NewMessage::empty("s1", "tool")
        })
        .unwrap();
    store
        .append_message(NewMessage {
            content: Some("u1".into()),
            ..NewMessage::empty("s1", "user")
        })
        .unwrap();
    store
        .append_message(NewMessage {
            content: Some("a1".into()),
            ..NewMessage::empty("s1", "assistant")
        })
        .unwrap();
    store
        .append_message(NewMessage {
            content: Some("u2".into()),
            ..NewMessage::empty("s1", "user")
        })
        .unwrap();

    // 删除气泡 [2,4) = u1 + a1（0=u0,1=a0），保留 u0/a0/tool0 + u2
    store.remove_chat_bubbles("s1", 2, 4).unwrap();
    let left = store.get_messages("s1").unwrap();
    assert_eq!(left.len(), 4);
    assert_eq!(left[0].content.as_deref(), Some("u0"));
    assert_eq!(left[1].role, "assistant");
    assert_eq!(left[2].role, "tool");
    assert_eq!(left[3].content.as_deref(), Some("u2"));
    let meta = store.get_session("s1").unwrap().unwrap();
    assert_eq!(meta.message_count, 4);
    assert_eq!(meta.tool_call_count, 1);
}

#[test]
fn end_session_sets_ended_at_and_reason() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).unwrap();
    store
        .create_session("s1", "test", Some("gpt"), None, None)
        .unwrap();
    store.end_session("s1", "compacted").unwrap();
    let row = store.get_session("s1").unwrap().unwrap();
    assert!(row.ended_at.is_some());
    assert_eq!(row.end_reason.as_deref(), Some("compacted"));
}

#[test]
fn append_message_rejects_ended_session() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).unwrap();
    store.create_session("s1", "test", None, None, None).unwrap();
    store.end_session("s1", "compacted").unwrap();
    let err = store
        .append_message(NewMessage {
            content: Some("x"),
            ..NewMessage::empty("s1", "user")
        })
        .unwrap_err();
    assert!(
        err.to_string().contains("ended") || err.to_string().contains("writable"),
        "{err}"
    );
}
