use session::store::{BillingDelta, NewMessage, SessionListFilter, SessionStore, SCHEMA_VERSION};
use tempfile::TempDir;

async fn test_store() -> (TempDir, SessionStore) {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).await.unwrap();
    (dir, store)
}

#[tokio::test]
async fn opens_fresh_db_at_current_schema() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.db");
    let store = SessionStore::open(&path).await.unwrap();
    assert_eq!(store.schema_version().await.unwrap(), SCHEMA_VERSION);
    store
        .create_session("s1", "test", None, None, None)
        .await.unwrap();
    assert!(path.is_file());
}

#[tokio::test]
async fn session_store_impls_sqlite_store() {
    use types::SqliteStore;
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.db");
    let store = SessionStore::open(&path).await.unwrap();
    let _pool = SqliteStore::pool(&store);
    assert_eq!(store.schema_version().await.unwrap(), SCHEMA_VERSION);
}

#[tokio::test]
async fn append_and_reload_tool_calls_and_reasoning() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).await.unwrap();
    store
        .create_session("s1", "test", None, None, None)
        .await.unwrap();
    store
        .append_message(session::store::NewMessage {
            session_id: "s1",
            role: "assistant",
            content: Some("done"),
            compressed_content: None,
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
            media_json: None,
        })
        .await.unwrap();
    store
        .append_message(session::store::NewMessage {
            session_id: "s1",
            role: "tool",
            content: Some("ok"),
            compressed_content: None,
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
            media_json: None,
        })
        .await.unwrap();

    let msgs = store.get_messages("s1").await.unwrap();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].reasoning.as_deref(), Some("think"));
    assert!(msgs[0].tool_calls.is_some());
    assert_eq!(msgs[1].tool_call_id.as_deref(), Some("c1"));

    let conv = store.get_messages_as_conversation("s1").await.unwrap();
    assert_eq!(conv[0]["role"], "assistant");
    assert!(conv[0].get("tool_calls").is_some());
    assert_eq!(conv[0]["reasoning"], "think");
    assert_eq!(conv[1]["role"], "tool");
}

#[tokio::test]
async fn append_message_persists_compressed_content_in_the_initial_insert() {
    let (_dir, store) = test_store().await;
    store
        .create_session("mailbox", "tauri", None, None, None)
        .await.unwrap();
    let marker = "agent-mailbox-through:7";

    store
        .append_message(NewMessage {
            content: Some("atomic mailbox input"),
            finish_reason: Some(marker),
            compressed_content: Some(marker),
            ..NewMessage::empty("mailbox", "user")
        })
        .await.unwrap();

    let messages = store.get_messages("mailbox").await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content.as_deref(), Some("atomic mailbox input"));
    assert_eq!(messages[0].finish_reason.as_deref(), Some(marker));
    assert_eq!(messages[0].compressed_content.as_deref(), Some(marker));
}

#[tokio::test]
async fn search_messages_hits_tool_name_and_cjk() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).await.unwrap();
    store
        .create_session("s1", "tauri", None, None, None)
        .await.unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "tool",
            content: Some("已写入长期记忆"),
            tool_name: Some("memory"),
            tool_call_id: Some("c1"),
            ..NewMessage::empty("s1", "tool")
        })
        .await.unwrap();
    let hits = store.search_messages("memory", None, None, 10).await.unwrap();
    assert!(!hits.is_empty());
    let hits2 = store.search_messages("长期记忆", None, None, 10).await.unwrap();
    assert!(!hits2.is_empty());
}

#[tokio::test]
async fn build_chat_history_folds_tools_into_activities() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).await.unwrap();
    store
        .create_session("s1", "tauri", None, None, None)
        .await.unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "user",
            content: Some("hi"),
            ..NewMessage::empty("s1", "user")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "assistant",
            content: Some(""),
            reasoning: Some("plan"),
            tool_calls: Some(serde_json::json!([{
                "id": "c1", "name": "memory",
                "arguments": {"action": "add", "target": "memory", "content": "e"}
            }])),
            ..NewMessage::empty("s1", "assistant")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "tool",
            content: Some("ok"),
            tool_call_id: Some("c1"),
            tool_name: Some("memory"),
            ..NewMessage::empty("s1", "tool")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "assistant",
            content: Some("done"),
            ..NewMessage::empty("s1", "assistant")
        })
        .await.unwrap();

    let ui = store.build_chat_history("s1", 200).await.unwrap();
    assert_eq!(ui.len(), 2); // user + coalesced assistant(activity + final text)
    assert_eq!(ui[1].reasoning.as_deref(), Some("plan"));
    assert_eq!(ui[1].content, "done");
    assert_eq!(ui[1].activities.len(), 1);
    assert_eq!(ui[1].activities[0].title, "memory");
    assert_eq!(ui[1].activities[0].output.as_deref(), Some("ok"));
}

#[tokio::test]
async fn build_chat_history_restores_tool_media_json() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).await.unwrap();
    store
        .create_session("s1", "tauri", None, None, None)
        .await.unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "user",
            content: Some("画一只猫"),
            ..NewMessage::empty("s1", "user")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "assistant",
            content: Some(""),
            tool_calls: Some(serde_json::json!([{
                "id": "c1", "name": "image_gen",
                "arguments": {"prompt": "cat"}
            }])),
            ..NewMessage::empty("s1", "assistant")
        })
        .await.unwrap();
    let media = r#"[{"kind":"image","mime_type":"image/jpeg","reference":{"workspace_path":"generated/images/img-1.jpg"},"label":"图片已生成"}]"#;
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "tool",
            content: Some("图片已生成：generated/images/img-1.jpg"),
            tool_call_id: Some("c1"),
            tool_name: Some("image_gen"),
            media_json: Some(media),
            ..NewMessage::empty("s1", "tool")
        })
        .await.unwrap();

    let ui = store.build_chat_history("s1", 200).await.unwrap();
    assert_eq!(ui.len(), 2);
    assert_eq!(ui[1].activities.len(), 1);
    let media_val = ui[1].activities[0].media.as_ref().expect("media");
    let path = media_val[0]["reference"]["workspace_path"]
        .as_str()
        .unwrap();
    assert_eq!(path, "generated/images/img-1.jpg");
}

#[tokio::test]
async fn build_chat_history_restores_timeline() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).await.unwrap();
    store
        .create_session("s1", "tauri", None, None, None)
        .await.unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "user",
            content: Some("hi"),
            ..NewMessage::empty("s1", "user")
        })
        .await.unwrap();
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
                "id": "c1", "name": "present", "arguments": {}
            }])),
            reasoning_details: Some(details),
            ..NewMessage::empty("s1", "assistant")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "tool",
            content: Some("Presented info card"),
            tool_call_id: Some("c1"),
            tool_name: Some("present"),
            ..NewMessage::empty("s1", "tool")
        })
        .await.unwrap();

    let ui = store.build_chat_history("s1", 200).await.unwrap();
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
    assert_eq!(
        ui[1].activities[0].output.as_deref(),
        Some("Presented info card")
    );
}

#[tokio::test]
async fn build_chat_history_coalesces_consecutive_assistants() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).await.unwrap();
    store
        .create_session("s1", "tauri", None, None, None)
        .await.unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "user",
            content: Some("做首歌"),
            ..NewMessage::empty("s1", "user")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "assistant",
            content: Some("先生成"),
            tool_calls: Some(serde_json::json!([{
                "id": "c1", "name": "music_gen", "arguments": {}
            }])),
            reasoning_details: Some(serde_json::json!({
                "astro_timeline_v1": [
                    {"type": "reasoning", "id": "r1", "text": "t1", "at": 1},
                    {"type": "activity", "id": "c1", "at": 2}
                ]
            })),
            ..NewMessage::empty("s1", "assistant")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "tool",
            content: Some("audio ok"),
            tool_call_id: Some("c1"),
            tool_name: Some("music_gen"),
            ..NewMessage::empty("s1", "tool")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "assistant",
            content: Some("生成成功"),
            tool_calls: Some(serde_json::json!([{
                "id": "c2", "name": "present", "arguments": {}
            }])),
            reasoning_details: Some(serde_json::json!({
                "astro_timeline_v1": [
                    {"type": "reasoning", "id": "r1", "text": "t1", "at": 1},
                    {"type": "activity", "id": "c1", "at": 2},
                    {"type": "activity", "id": "c2", "at": 3},
                    {"type": "surface", "id": "surf-1", "at": 4}
                ],
                "astro_surfaces_v1": [{
                    "messageId": "surf-1",
                    "activityType": "a2ui-surface",
                    "operations": [],
                    "status": "active"
                }]
            })),
            ..NewMessage::empty("s1", "assistant")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "tool",
            content: Some("Presented info card"),
            tool_call_id: Some("c2"),
            tool_name: Some("present"),
            ..NewMessage::empty("s1", "tool")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "assistant",
            content: Some("搞定"),
            ..NewMessage::empty("s1", "assistant")
        })
        .await.unwrap();

    let ui = store.build_chat_history("s1", 200).await.unwrap();
    assert_eq!(ui.len(), 2);
    assert_eq!(ui[0].role, "user");
    assert_eq!(ui[1].role, "assistant");
    assert_eq!(ui[1].content, "先生成\n\n生成成功\n\n搞定");
    assert_eq!(ui[1].activities.len(), 2);
    let segs = ui[1].segments.as_ref().unwrap().as_array().unwrap();
    assert_eq!(segs.len(), 4);
    let surfaces = ui[1].ui_surfaces.as_ref().unwrap().as_array().unwrap();
    assert_eq!(surfaces.len(), 1);
}

#[tokio::test]
async fn patch_last_assistant_reasoning_details_merges_surfaces() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).await.unwrap();
    store
        .create_session("s1", "tauri", None, None, None)
        .await.unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "assistant",
            content: Some("calling"),
            reasoning_details: Some(serde_json::json!({
                "astro_timeline_v1": [{"type": "activity", "id": "c1", "at": 1}],
                "google_thought_signature": "sig"
            })),
            ..NewMessage::empty("s1", "assistant")
        })
        .await.unwrap();
    store
        .patch_last_assistant_reasoning_details(
            "s1",
            &serde_json::json!({
                "astro_timeline_v1": [
                    {"type": "activity", "id": "c1", "at": 1},
                    {"type": "surface", "id": "surf", "at": 2}
                ],
                "astro_surfaces_v1": [{"messageId": "surf"}]
            }),
        )
        .await.unwrap();
    let msgs = store.get_messages("s1").await.unwrap();
    let details = msgs[0].reasoning_details.as_ref().unwrap();
    assert_eq!(details["google_thought_signature"], "sig");
    assert_eq!(details["astro_surfaces_v1"][0]["messageId"], "surf");
    assert_eq!(details["astro_timeline_v1"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn patch_last_assistant_reasoning_details_handles_null_column() {
    // 既有 assistant 行的 reasoning_details 为 NULL 时，patch 不应报
    // "Invalid column type Null"，而应正常写入。
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).await.unwrap();
    store
        .create_session("s1", "tauri", None, None, None)
        .await.unwrap();
    store
        .append_message(NewMessage {
            session_id: "s1",
            role: "assistant",
            content: Some("no details yet"),
            reasoning_details: None,
            ..NewMessage::empty("s1", "assistant")
        })
        .await.unwrap();
    store
        .patch_last_assistant_reasoning_details(
            "s1",
            &serde_json::json!({
                "astro_surfaces_v1": [{"messageId": "surf"}]
            }),
        )
        .await.unwrap();
    let msgs = store.get_messages("s1").await.unwrap();
    let details = msgs[0].reasoning_details.as_ref().unwrap();
    assert_eq!(details["astro_surfaces_v1"][0]["messageId"], "surf");
}

#[tokio::test]
async fn v13_state_db_migrates_and_discards_sidecar_sessions_db() {
    let dir = TempDir::new().unwrap();
    let sessions_dir = dir.path().join("sessions");
    std::fs::create_dir_all(&sessions_dir).unwrap();

    // 旧 state.db：v13 但无 archived_at
    {
        let pool = agent_db::sqlx::SqlitePool::connect(&format!("sqlite:{}?mode=rwc", sessions_dir.join("state.db").display())).await.unwrap();
        agent_db::sqlx::raw_sql(
            "CREATE TABLE schema_version (version INTEGER NOT NULL);
             CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                user_id TEXT,
                model TEXT,
                model_config TEXT,
                system_prompt TEXT,
                parent_session_id TEXT,
                started_at REAL NOT NULL,
                ended_at REAL,
                end_reason TEXT,
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
                title TEXT,
                api_call_count INTEGER DEFAULT 0
             );
             CREATE TABLE messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT,
                tool_call_id TEXT,
                tool_calls TEXT,
                tool_name TEXT,
                timestamp REAL NOT NULL,
                token_count INTEGER,
                finish_reason TEXT,
                reasoning TEXT,
                reasoning_content TEXT,
                reasoning_details TEXT,
                codex_reasoning_items TEXT,
                codex_message_items TEXT
             );
             INSERT INTO schema_version (version) VALUES (13);
             INSERT INTO sessions (
                id, source, title, model, started_at, message_count, tool_call_count,
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                reasoning_tokens, billing_provider, billing_base_url, billing_mode,
                estimated_cost_usd, actual_cost_usd, cost_status, cost_source,
                pricing_version, api_call_count
             ) VALUES (
                'old', 'test', NULL, 'gpt', 1.0, 1, 0, 7, 3, 0, 0, 0,
                NULL, NULL, NULL, 0.42, NULL, NULL, NULL, NULL, 1
             );
             INSERT INTO messages (
                session_id, role, content, timestamp
             ) VALUES ('old', 'user', 'hello', 1.0);",
        )
        .execute(&pool).await.unwrap();
    }
    // 旧 sessions.db
    let legacy_path = sessions_dir.join("sessions.db");
    {
        let pool = agent_db::sqlx::SqlitePool::connect(&format!("sqlite:{}?mode=rwc", &legacy_path.display())).await.unwrap();
        agent_db::sqlx::raw_sql(
            "CREATE TABLE sessions (
                session_id TEXT PRIMARY KEY,
                summary TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             INSERT INTO sessions(session_id, summary) VALUES ('old','hello summary');",
        )
        .execute(&pool).await.unwrap();
    }

    let store = SessionStore::open_sessions_dir(&sessions_dir).await.unwrap();
    assert_eq!(store.schema_version().await.unwrap(), SCHEMA_VERSION);
    assert!(
        !legacy_path.exists(),
        "legacy sessions.db must be deleted, not imported"
    );
    assert!(
        !store.get_messages("old").await.unwrap().is_empty(),
        "state.db history must be preserved"
    );
    assert_eq!(store.get_session("old").await.unwrap().unwrap().archived_at, None);

    store
        .create_session("fresh", "test", None, None, None)
        .await.unwrap();
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
        .await.unwrap();
    let msgs = store.get_messages("fresh").await.unwrap();
    assert!(msgs.last().unwrap().content.is_none());

    let _ = SessionStore::open_sessions_dir(&sessions_dir).await.unwrap();
    assert_eq!(store.get_messages("fresh").await.unwrap().len(), 1);
}

#[tokio::test]
async fn discards_legacy_sessions_db_without_importing_titles() {
    let dir = TempDir::new().unwrap();
    let sessions_dir = dir.path().join("sessions");
    std::fs::create_dir_all(&sessions_dir).unwrap();

    let legacy_path = sessions_dir.join("sessions.db");
    {
        let pool = agent_db::sqlx::SqlitePool::connect(&format!("sqlite:{}?mode=rwc", &legacy_path.display())).await.unwrap();
        agent_db::sqlx::raw_sql(
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
        .execute(&pool).await.unwrap();
    }

    let store = SessionStore::open_sessions_dir(&sessions_dir).await.unwrap();
    assert!(!legacy_path.exists());
    assert!(store.get_session("dup-a").await.unwrap().is_none());
    assert!(store.get_session("dup-b").await.unwrap().is_none());
}

#[tokio::test]
async fn open_repairs_legacy_fts_triggers_on_v11_db() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.db");
    {
        let store = SessionStore::open(&path).await.unwrap();
        store
            .create_session("s1", "test", None, None, None)
            .await.unwrap();
        store
            .append_message(NewMessage {
                content: Some("hello"),
                ..NewMessage::empty("s1", "user")
            })
            .await.unwrap();
    }
    // 模拟失效 FTS 触发器被重新挂上。
    {
        let pool = agent_db::sqlx::SqlitePool::connect(&format!("sqlite:{}?mode=rwc", &path.display())).await.unwrap();
        agent_db::sqlx::raw_sql(
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
        .execute(&pool).await.unwrap();
    }

    let store = SessionStore::open(&path).await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("world"),
            ..NewMessage::empty("s1", "user")
        })
        .await.unwrap();
    assert_eq!(store.get_messages("s1").await.unwrap().len(), 2);
}

#[tokio::test]
async fn open_repairs_broken_fts_delete_command_triggers() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.db");
    {
        let store = SessionStore::open(&path).await.unwrap();
        store
            .create_session("s1", "test", None, None, None)
            .await.unwrap();
        store
            .append_message(NewMessage {
                content: Some("keep"),
                ..NewMessage::empty("s1", "user")
            })
            .await.unwrap();
    }
    // 模拟旧 v11 DDL：contentful FTS 上使用 VALUES('delete', …)。
    {
        let pool = agent_db::sqlx::SqlitePool::connect(&format!("sqlite:{}?mode=rwc", &path.display())).await.unwrap();
        agent_db::sqlx::raw_sql(
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
        .execute(&pool).await.unwrap();
    }

    let id = {
        let store = SessionStore::open(&path).await.unwrap();
        store
            .append_message(NewMessage {
                content: Some("temp"),
                ..NewMessage::empty("s1", "user")
            })
            .await.unwrap()
    };
    // 打开后应已换成 DELETE FROM fts，消息可删。
    {
        let pool = agent_db::sqlx::SqlitePool::connect(&format!("sqlite:{}?mode=rwc", &path.display())).await.unwrap();
        agent_db::sqlx::query("DELETE FROM messages WHERE id = ?1")
            .bind(&id)
            .execute(&pool)
            .await
            .unwrap();
    }
    let store = SessionStore::open(&path).await.unwrap();
    assert_eq!(store.get_messages("s1").await.unwrap().len(), 1);
}

#[tokio::test]
async fn ensure_session_is_idempotent() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.db");
    let store = SessionStore::open(&path).await.unwrap();
    store.ensure_session("s1", "test").await.unwrap();
    store.ensure_session("s1", "other").await.unwrap();
    let s = store.get_session("s1").await.unwrap().unwrap();
    assert_eq!(s.source, "test"); // 冲突时不覆盖
}

#[tokio::test]
async fn open_backfills_sessions_from_orphan_messages() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.db");
    {
        let _ = SessionStore::open(&path).await.unwrap();
    }
    {
        let pool = agent_db::sqlx::SqlitePool::connect(&format!("sqlite:{}?mode=rwc", &path.display())).await.unwrap();
        agent_db::sqlx::raw_sql("PRAGMA foreign_keys=OFF;").execute(&pool).await.unwrap();
        agent_db::sqlx::query(
            "INSERT INTO messages (session_id, role, content, timestamp)
             VALUES ('orphan', 'user', 'hello orphan', 100.0)",
        )
        .execute(&pool)
        .await
        .unwrap();
        agent_db::sqlx::query(
            "INSERT INTO messages (session_id, role, content, timestamp)
             VALUES ('orphan', 'assistant', 'hi', 101.0)",
        )
        .execute(&pool)
        .await
        .unwrap();
    }
    let store = SessionStore::open(&path).await.unwrap();
    let s = store.get_session("orphan").await.unwrap().expect("backfilled");
    assert_eq!(s.source, "tauri");
    assert_eq!(s.message_count, 2);
    assert_eq!(s.started_at, 100.0);
}

#[tokio::test]
async fn update_session_billing_accumulates_and_unknown_skips_cost() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).await.unwrap();
    store
        .create_session("s1", "test", Some("gpt"), None, None)
        .await.unwrap();
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
        .await.unwrap();
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
        .await.unwrap();
    let row = store.get_session_billing("s1").await.unwrap().unwrap();
    assert_eq!(row.input_tokens, 11);
    assert!((row.estimated_cost_usd - 0.01).abs() < 1e-9);
    assert_eq!(row.cost_status.as_deref(), Some("unknown"));
    assert_eq!(row.api_call_count, 2);
}

#[tokio::test]
async fn v13_schema_migrates_to_v14_without_data_loss() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.db");

    {
        let store = SessionStore::open(&path).await.unwrap();
        store
            .create_session("legacy", "test", Some("gpt"), None, None)
            .await.unwrap();
        store
            .append_message(NewMessage {
                session_id: "legacy",
                role: "user",
                content: Some("old chat migrationftsneedle"),
                ..NewMessage::empty("legacy", "user")
            })
            .await.unwrap();
        store
            .update_session_billing(
                "legacy",
                BillingDelta {
                    input_tokens: 7,
                    output_tokens: 3,
                    estimated_cost_usd: 0.42,
                    api_call_count: 1,
                    ..BillingDelta::default()
                },
            )
            .await.unwrap();
        let before = store.get_session("legacy").await.unwrap().unwrap();
        assert!(before.archived_at.is_none());
    }

    {
        let pool = agent_db::sqlx::SqlitePool::connect(&format!("sqlite:{}?mode=rwc", &path.display())).await.unwrap();
        agent_db::sqlx::raw_sql(
            "DROP TABLE IF EXISTS messages;
             DROP TABLE IF EXISTS sessions;
             DROP TABLE IF EXISTS schema_version;
             CREATE TABLE schema_version (version INTEGER NOT NULL);
             CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                user_id TEXT,
                model TEXT,
                model_config TEXT,
                system_prompt TEXT,
                parent_session_id TEXT,
                started_at REAL NOT NULL,
                ended_at REAL,
                end_reason TEXT,
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
                title TEXT,
                api_call_count INTEGER DEFAULT 0
             );
             CREATE TABLE messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT,
                tool_call_id TEXT,
                tool_calls TEXT,
                tool_name TEXT,
                timestamp REAL NOT NULL,
                token_count INTEGER,
                finish_reason TEXT,
                reasoning TEXT,
                reasoning_content TEXT,
                reasoning_details TEXT,
                codex_reasoning_items TEXT,
                codex_message_items TEXT
             );
             INSERT INTO schema_version (version) VALUES (13);
             INSERT INTO sessions (
                id, source, title, model, started_at, message_count, tool_call_count,
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                reasoning_tokens, billing_provider, billing_base_url, billing_mode,
                estimated_cost_usd, actual_cost_usd, cost_status, cost_source,
                pricing_version, api_call_count
             ) VALUES (
                'legacy', 'test', NULL, 'gpt', 1.0, 1, 0, 7, 3, 0, 0, 0,
                NULL, NULL, NULL, 0.42, NULL, NULL, NULL, NULL, 1
             );
             INSERT INTO messages (
                session_id, role, content, tool_call_id, tool_calls, tool_name, timestamp
             ) VALUES (
                'legacy', 'user', 'old chat migrationftsneedle', NULL, NULL, NULL, 1.0
             );",
        )
        .execute(&pool).await.unwrap();
    }

    let reopened = SessionStore::open(&path).await.unwrap();
    assert_eq!(reopened.schema_version().await.unwrap(), SCHEMA_VERSION);
    assert_eq!(reopened.get_messages("legacy").await.unwrap().len(), 1);
    assert_eq!(
        reopened
            .get_session_billing("legacy")
            .await.unwrap()
            .unwrap()
            .input_tokens,
        7
    );
    assert_eq!(
        reopened.get_session("legacy").await.unwrap().unwrap().archived_at,
        None
    );
    let hits = reopened
        .search_messages("migrationftsneedle", None, None, 10)
        .await.unwrap();
    assert!(
        hits.iter().any(|hit| hit.session_id == "legacy"),
        "v13 to v14 migration must preserve searchable FTS data"
    );
}

#[tokio::test]
async fn schema_v13_to_v14_preserves_chat_and_adds_compressed_content() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.db");
    {
        let pool = agent_db::sqlx::SqlitePool::connect(&format!("sqlite:{}?mode=rwc", &path.display())).await.unwrap();
        agent_db::sqlx::raw_sql(
            "CREATE TABLE schema_version (version INTEGER NOT NULL);
             CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                started_at REAL NOT NULL,
                message_count INTEGER DEFAULT 0,
                tool_call_count INTEGER DEFAULT 0
             );
             CREATE TABLE messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT,
                tool_call_id TEXT,
                tool_calls TEXT,
                tool_name TEXT,
                timestamp REAL NOT NULL,
                token_count INTEGER,
                finish_reason TEXT,
                reasoning TEXT,
                reasoning_content TEXT,
                reasoning_details TEXT,
                codex_reasoning_items TEXT,
                codex_message_items TEXT
             );
             INSERT INTO schema_version (version) VALUES (13);
             INSERT INTO sessions (id, source, started_at, message_count, tool_call_count)
             VALUES ('s1', 'test', 1.0, 1, 1);
             INSERT INTO messages (
                session_id, role, content, tool_call_id, tool_name, timestamp
             ) VALUES ('s1', 'tool', 'original tool output', 'c1', 'search', 1.0);",
        )
        .execute(&pool).await.unwrap();
    }

    let store = SessionStore::open(&path).await.unwrap();
    assert_eq!(store.schema_version().await.unwrap(), SCHEMA_VERSION);
    let msgs = store.get_messages("s1").await.unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].content.as_deref(), Some("original tool output"));
    assert!(msgs[0].compressed_content.is_none());

    store
        .update_message_compressed_content(msgs[0].id, Some("compressed view"))
        .await.unwrap();
    let msgs = store.get_messages("s1").await.unwrap();
    assert_eq!(msgs[0].content.as_deref(), Some("original tool output"));
    assert_eq!(
        msgs[0].compressed_content.as_deref(),
        Some("compressed view")
    );
}

/// 半迁移库：schema_version 已是 14，但 messages 缺 compressed_content（合并分支 stamp 竞态）。
#[tokio::test]
async fn stamped_v14_without_compressed_content_self_heals() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.db");
    {
        let pool = agent_db::sqlx::SqlitePool::connect(&format!("sqlite:{}?mode=rwc", &path.display())).await.unwrap();
        agent_db::sqlx::raw_sql(
            "CREATE TABLE schema_version (version INTEGER NOT NULL);
             CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                started_at REAL NOT NULL,
                message_count INTEGER DEFAULT 0,
                tool_call_count INTEGER DEFAULT 0,
                archived_at REAL
             );
             CREATE TABLE messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT,
                tool_call_id TEXT,
                tool_calls TEXT,
                tool_name TEXT,
                timestamp REAL NOT NULL,
                token_count INTEGER,
                finish_reason TEXT,
                reasoning TEXT,
                reasoning_content TEXT,
                reasoning_details TEXT,
                codex_reasoning_items TEXT,
                codex_message_items TEXT
             );
             INSERT INTO schema_version (version) VALUES (14);
             INSERT INTO sessions (id, source, started_at, message_count, tool_call_count)
             VALUES ('s1', 'test', 1.0, 1, 0);
             INSERT INTO messages (session_id, role, content, timestamp)
             VALUES ('s1', 'user', 'hello half-migrated', 1.0);",
        )
        .execute(&pool).await.unwrap();
    }

    let store = SessionStore::open(&path).await.unwrap();
    assert_eq!(store.schema_version().await.unwrap(), SCHEMA_VERSION);
    let msgs = store.get_messages("s1").await.unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].content.as_deref(), Some("hello half-migrated"));
    assert!(msgs[0].compressed_content.is_none());
}

#[tokio::test]
async fn fork_session_copies_bubbles_and_trailing_tools() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).await.unwrap();
    store
        .create_session("src", "test", Some("gpt"), None, None)
        .await.unwrap();
    store.set_session_title("src", "hello").await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("u1"),
            ..NewMessage::empty("src", "user")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("a1"),
            tool_calls: Some(serde_json::json!([{ "id": "c1", "name": "x", "arguments": {} }])),
            ..NewMessage::empty("src", "assistant")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("tool-out"),
            tool_call_id: Some("c1"),
            tool_name: Some("x"),
            ..NewMessage::empty("src", "tool")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("a1b"),
            ..NewMessage::empty("src", "assistant")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("u2"),
            ..NewMessage::empty("src", "user")
        })
        .await.unwrap();

    // keep 2 bubbles = user + assistant(+trailing tools until next non-tool)
    // After first assistant with tools, we include tool rows then stop before next assistant?
    // Looking at impl: when bubble count hits keep, it includes following tool rows only.
    // So keep=2: u1, a1(+tool). Not a1b.
    store.fork_session("src", "dst", 2).await.unwrap();
    let dst = store.get_messages("dst").await.unwrap();
    assert_eq!(dst.len(), 3);
    assert_eq!(dst[0].content.as_deref(), Some("u1"));
    assert_eq!(dst[1].role, "assistant");
    assert_eq!(dst[2].role, "tool");
    let meta = store.get_session("dst").await.unwrap().unwrap();
    assert_eq!(meta.parent_session_id.as_deref(), Some("src"));
    assert!(meta.title.as_deref().unwrap_or("").contains("branch"));
}

#[tokio::test]
async fn fork_session_recent_turns_preserves_complete_rows_and_turn_boundaries() {
    let (_dir, store) = test_store().await;
    store
        .create_session("source", "tauri", Some("model-a"), None, None)
        .await.unwrap();

    store
        .append_message(NewMessage {
            session_id: "source",
            role: "user",
            content: Some("first question"),
            media_json: Some(
                r#"[{"kind":"image","mime_type":"image/png","reference":{"remote_url":"https://example.invalid/one.png"}}]"#,
            ),
            ..NewMessage::empty("source", "user")
        })
        .await.unwrap();
    let assistant_id = store
        .append_message(NewMessage {
            session_id: "source",
            role: "assistant",
            content: Some("calling tool"),
            tool_calls: Some(serde_json::json!([{
                "id": "call-1",
                "name": "inspect",
                "arguments": {"path": "a.rs"}
            }])),
            token_count: Some(17),
            finish_reason: Some("tool_calls"),
            reasoning: Some("visible reasoning"),
            reasoning_content: Some("provider reasoning"),
            reasoning_details: Some(serde_json::json!({"detail": true})),
            codex_reasoning_items: Some(serde_json::json!([{"type": "reasoning"}])),
            codex_message_items: Some(serde_json::json!([{"type": "message"}])),
            ..NewMessage::empty("source", "assistant")
        })
        .await.unwrap();
    store
        .update_message_compressed_content(assistant_id, Some("compressed assistant"))
        .await.unwrap();
    let tool_id = store
        .append_message(NewMessage {
            session_id: "source",
            role: "tool",
            content: Some("tool result"),
            tool_call_id: Some("call-1"),
            tool_name: Some("inspect"),
            media_json: Some(
                r#"[{"kind":"image","mime_type":"image/png","reference":{"workspace_path":"out.png"}}]"#,
            ),
            ..NewMessage::empty("source", "tool")
        })
        .await.unwrap();
    store
        .update_message_compressed_content(tool_id, Some("compressed tool"))
        .await.unwrap();
    store
        .append_message(NewMessage {
            session_id: "source",
            role: "assistant",
            content: Some("first answer"),
            ..NewMessage::empty("source", "assistant")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            session_id: "source",
            role: "user",
            content: Some("second question"),
            ..NewMessage::empty("source", "user")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            session_id: "source",
            role: "assistant",
            content: Some("second answer"),
            ..NewMessage::empty("source", "assistant")
        })
        .await.unwrap();

    store
        .fork_session_recent_turns("source", "all", None)
        .await.unwrap();
    store
        .fork_session_recent_turns("source", "none", Some(0))
        .await.unwrap();
    store
        .fork_session_recent_turns("source", "last", Some(1))
        .await.unwrap();
    store
        .fork_session_recent_turns("source", "last-two", Some(2))
        .await.unwrap();

    let source = store.get_messages("source").await.unwrap();
    let all = store.get_messages("all").await.unwrap();
    assert_eq!(all.len(), source.len());
    for (expected, actual) in source.iter().zip(&all) {
        assert_eq!(actual.role, expected.role);
        assert_eq!(actual.content, expected.content);
        assert_eq!(actual.compressed_content, expected.compressed_content);
        assert_eq!(actual.tool_call_id, expected.tool_call_id);
        assert_eq!(actual.tool_calls, expected.tool_calls);
        assert_eq!(actual.tool_name, expected.tool_name);
        assert_eq!(actual.timestamp, expected.timestamp);
        assert_eq!(actual.token_count, expected.token_count);
        assert_eq!(actual.finish_reason, expected.finish_reason);
        assert_eq!(actual.reasoning, expected.reasoning);
        assert_eq!(actual.reasoning_content, expected.reasoning_content);
        assert_eq!(actual.reasoning_details, expected.reasoning_details);
        assert_eq!(actual.codex_reasoning_items, expected.codex_reasoning_items);
        assert_eq!(actual.codex_message_items, expected.codex_message_items);
        assert_eq!(actual.media_json, expected.media_json);
    }
    assert!(store.get_messages("none").await.unwrap().is_empty());
    assert_eq!(
        store
            .get_messages("last")
            .await.unwrap()
            .iter()
            .map(|message| (message.role.as_str(), message.content.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            ("user", Some("second question")),
            ("assistant", Some("second answer")),
        ]
    );
    assert_eq!(store.get_messages("last-two").await.unwrap().len(), source.len());

    for fork_id in ["all", "none", "last", "last-two"] {
        let fork = store.get_session(fork_id).await.unwrap().unwrap();
        assert_eq!(fork.parent_session_id.as_deref(), Some("source"));
        assert_eq!(fork.model.as_deref(), Some("model-a"));
    }
}

#[tokio::test]
async fn fork_recent_turns_rejects_missing_source_without_creating_target() {
    let (_dir, store) = test_store().await;

    let error = store
        .fork_session_recent_turns("missing", "target", None)
        .await.unwrap_err();

    assert!(error.to_string().contains("source session not found"));
    assert!(store.get_session("target").await.unwrap().is_none());
}

#[tokio::test]
async fn fork_recent_turns_rolls_back_target_and_retries_after_insert_failure() {
    let (dir, store) = test_store().await;
    store
        .create_session("source", "tauri", Some("model-a"), None, None)
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("question"),
            ..NewMessage::empty("source", "user")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("answer"),
            ..NewMessage::empty("source", "assistant")
        })
        .await.unwrap();
    let pool = agent_db::sqlx::SqlitePool::connect(&format!("sqlite:{}?mode=rwc", dir.path().join("state.db").display())).await.unwrap();
    agent_db::sqlx::raw_sql(
        "CREATE TRIGGER fail_recent_fork
         BEFORE INSERT ON messages
         WHEN NEW.session_id = 'target'
         BEGIN
           SELECT RAISE(ABORT, 'injected fork insert failure');
         END;",
    )
    .execute(&pool)
    .await
    .unwrap();

    let error = store
        .fork_session_recent_turns("source", "target", None)
        .await.unwrap_err();
    assert!(error.to_string().contains("injected fork insert failure"));
    assert!(store.get_session("target").await.unwrap().is_none());
    assert!(store.get_messages("target").await.unwrap().is_empty());

    agent_db::sqlx::raw_sql("DROP TRIGGER fail_recent_fork;").execute(&pool).await.unwrap();
    store
        .fork_session_recent_turns("source", "target", None)
        .await.unwrap();
    let target = store.get_session("target").await.unwrap().unwrap();
    assert_eq!(target.parent_session_id.as_deref(), Some("source"));
    assert_eq!(target.message_count, 2);
    assert_eq!(store.get_messages("target").await.unwrap().len(), 2);
    assert!(store
        .search_messages("question", None, None, 10)
        .await.unwrap()
        .iter()
        .any(|hit| hit.session_id == "target"));
}

#[tokio::test]
async fn truncate_session_to_bubbles_drops_tail_and_trailing_tools() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).await.unwrap();
    store
        .create_session("s1", "test", Some("gpt"), None, None)
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("u1"),
            ..NewMessage::empty("s1", "user")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("a1"),
            tool_calls: Some(serde_json::json!([{ "id": "c1", "name": "x", "arguments": {} }])),
            ..NewMessage::empty("s1", "assistant")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("tool-out"),
            tool_call_id: Some("c1"),
            tool_name: Some("x"),
            ..NewMessage::empty("s1", "tool")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("u2"),
            ..NewMessage::empty("s1", "user")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("a2"),
            ..NewMessage::empty("s1", "assistant")
        })
        .await.unwrap();

    // keep 1 bubble = only first user；后续 assistant/tool/u2/a2 全删
    store.truncate_session_to_bubbles("s1", 1).await.unwrap();
    let left = store.get_messages("s1").await.unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].content.as_deref(), Some("u1"));
    let meta = store.get_session("s1").await.unwrap().unwrap();
    assert_eq!(meta.message_count, 1);
    assert_eq!(meta.tool_call_count, 0);

    // keep 0 → 清空
    store
        .append_message(NewMessage {
            content: Some("again"),
            ..NewMessage::empty("s1", "user")
        })
        .await.unwrap();
    store.truncate_session_to_bubbles("s1", 0).await.unwrap();
    assert!(store.get_messages("s1").await.unwrap().is_empty());
    let meta = store.get_session("s1").await.unwrap().unwrap();
    assert_eq!(meta.message_count, 0);
}

#[tokio::test]
async fn remove_chat_bubbles_splices_middle_user_and_tools() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).await.unwrap();
    store
        .create_session("s1", "test", Some("gpt"), None, None)
        .await.unwrap();
    // u0 a0(tool) u1 a1 u2
    store
        .append_message(NewMessage {
            content: Some("u0"),
            ..NewMessage::empty("s1", "user")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("a0"),
            tool_calls: Some(serde_json::json!([{ "id": "c0", "name": "x", "arguments": {} }])),
            ..NewMessage::empty("s1", "assistant")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("tool0"),
            tool_call_id: Some("c0"),
            tool_name: Some("x"),
            ..NewMessage::empty("s1", "tool")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("u1"),
            ..NewMessage::empty("s1", "user")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("a1"),
            ..NewMessage::empty("s1", "assistant")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("u2"),
            ..NewMessage::empty("s1", "user")
        })
        .await.unwrap();

    // 删除气泡 [2,4) = u1 + a1（0=u0,1=a0），保留 u0/a0/tool0 + u2
    store.remove_chat_bubbles("s1", 2, 4).await.unwrap();
    let left = store.get_messages("s1").await.unwrap();
    assert_eq!(left.len(), 4);
    assert_eq!(left[0].content.as_deref(), Some("u0"));
    assert_eq!(left[1].role, "assistant");
    assert_eq!(left[2].role, "tool");
    assert_eq!(left[3].content.as_deref(), Some("u2"));
    let meta = store.get_session("s1").await.unwrap().unwrap();
    assert_eq!(meta.message_count, 4);
    assert_eq!(meta.tool_call_count, 1);
}

#[tokio::test]
async fn end_session_sets_ended_at_and_reason() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).await.unwrap();
    store
        .create_session("s1", "test", Some("gpt"), None, None)
        .await.unwrap();
    store.end_session("s1", "compacted").await.unwrap();
    let row = store.get_session("s1").await.unwrap().unwrap();
    assert!(row.ended_at.is_some());
    assert_eq!(row.end_reason.as_deref(), Some("compacted"));
}

#[tokio::test]
async fn append_message_rejects_ended_session() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).await.unwrap();
    store
        .create_session("s1", "test", None, None, None)
        .await.unwrap();
    store.end_session("s1", "compacted").await.unwrap();
    let err = store
        .append_message(NewMessage {
            content: Some("x"),
            ..NewMessage::empty("s1", "user")
        })
        .await.unwrap_err();
    assert!(
        err.to_string().contains("ended") || err.to_string().contains("writable"),
        "{err}"
    );
}

#[tokio::test]
async fn compact_and_split_ends_old_and_seeds_new_with_summary_and_tail() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).await.unwrap();
    store
        .create_session("old", "test", Some("gpt"), None, None)
        .await.unwrap();
    store.set_session_title("old", "topic").await.unwrap();
    for (role, text) in [
        ("user", "u1"),
        ("assistant", "a1"),
        ("user", "u2"),
        ("assistant", "a2"),
        ("user", "u3"),
        ("assistant", "a3"),
    ] {
        store
            .append_message(NewMessage {
                content: Some(text),
                ..NewMessage::empty("old", role)
            })
            .await.unwrap();
    }
    // 给最后一条 assistant 挂 tool
    store
        .append_message(NewMessage {
            content: Some("tool-out"),
            tool_call_id: Some("c1"),
            tool_name: Some("x"),
            ..NewMessage::empty("old", "tool")
        })
        .await.unwrap();

    store
        .compact_and_split(
            "old",
            "new",
            "[CONTEXT COMPACTION]\nsummary body",
            2, // 保留 u3 + a3(+tool)
        )
        .await.unwrap();

    let old = store.get_session("old").await.unwrap().unwrap();
    assert!(old.ended_at.is_some());
    assert_eq!(old.end_reason.as_deref(), Some("compacted"));
    // 旧全文仍在
    assert!(store.get_messages("old").await.unwrap().len() >= 6);

    let neu = store.get_session("new").await.unwrap().unwrap();
    assert_eq!(neu.parent_session_id.as_deref(), Some("old"));
    assert_eq!(neu.model.as_deref(), Some("gpt"));
    assert!(neu.ended_at.is_none());
    assert!(neu.title.as_deref().unwrap_or("").contains("continued"));

    let msgs = store.get_messages("new").await.unwrap();
    assert_eq!(msgs[0].role, "user");
    assert!(msgs[0]
        .content
        .as_deref()
        .unwrap_or("")
        .starts_with("[CONTEXT COMPACTION]"));
    // 摘要 + u3 + a3 + tool
    assert_eq!(msgs.len(), 4);
    assert_eq!(msgs[1].content.as_deref(), Some("u3"));
    assert_eq!(msgs[2].role, "assistant");
    assert_eq!(msgs[3].role, "tool");
}

#[tokio::test]
async fn compact_and_split_keep_zero_is_summary_only() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).await.unwrap();
    store
        .create_session("old", "test", None, None, None)
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("u1"),
            ..NewMessage::empty("old", "user")
        })
        .await.unwrap();
    store
        .compact_and_split("old", "new", "[CONTEXT COMPACTION]\nx", 0)
        .await.unwrap();
    let msgs = store.get_messages("new").await.unwrap();
    assert_eq!(msgs.len(), 1);
}

#[tokio::test]
async fn compact_and_split_rejects_changed_source_without_partial_state() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).await.unwrap();
    store
        .create_session("old", "test", None, None, None)
        .await.unwrap();
    let snapshot_id = store
        .append_message(NewMessage {
            content: Some("before summary"),
            ..NewMessage::empty("old", "user")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("arrived while summarizing"),
            ..NewMessage::empty("old", "assistant")
        })
        .await.unwrap();

    let err = store
        .compact_and_split_if_unchanged(
            "old",
            "new",
            "[CONTEXT COMPACTION]\nstale",
            0,
            Some(snapshot_id),
        )
        .await.unwrap_err();

    assert!(err.to_string().contains("changed while summarizing"));
    assert!(store
        .get_session("old")
        .await.unwrap()
        .unwrap()
        .ended_at
        .is_none());
    assert!(store.get_session("new").await.unwrap().is_none());
}

#[tokio::test]
async fn archive_filters_and_restores_session() {
    let (_dir, store) = test_store().await;
    store
        .create_session("s1", "tauri", None, None, None)
        .await.unwrap();
    store
        .create_session("s2", "tauri", None, None, None)
        .await.unwrap();

    store.archive_session("s1").await.unwrap();

    let active = store.list_sessions(SessionListFilter::Active, 10).await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, "s2");

    let archived = store
        .list_sessions(SessionListFilter::Archived, 10)
        .await.unwrap();
    assert_eq!(archived.len(), 1);
    assert_eq!(archived[0].id, "s1");

    store.unarchive_session("s1").await.unwrap();
    assert_eq!(
        store
            .list_sessions(SessionListFilter::Archived, 10)
            .await.unwrap()
            .len(),
        0
    );
}

#[tokio::test]
async fn v15_schema_migrates_to_v16_without_data_loss() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.db");

    {
        let store = SessionStore::open(&path).await.unwrap();
        store
            .create_session("keep", "tauri", None, None, None)
            .await.unwrap();
        store
            .append_message(NewMessage {
                session_id: "keep",
                role: "user",
                content: Some("keep me after v16"),
                ..NewMessage::empty("keep", "user")
            })
            .await.unwrap();
        store.set_session_title("keep", "History").await.unwrap();
    }

    {
        let pool = agent_db::sqlx::SqlitePool::connect(&format!("sqlite:{}?mode=rwc", &path.display())).await.unwrap();
        agent_db::sqlx::query("UPDATE schema_version SET version = 15")
            .execute(&pool)
            .await
            .unwrap();
    }

    let reopened = SessionStore::open(&path).await.unwrap();
    assert_eq!(reopened.schema_version().await.unwrap(), SCHEMA_VERSION);

    let sessions = reopened
        .list_sessions(SessionListFilter::Active, 10)
        .await.unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "keep");
    assert_eq!(sessions[0].title.as_deref(), Some("History"));

    let msgs = reopened.get_messages("keep").await.unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].content.as_deref(), Some("keep me after v16"));
}

#[tokio::test]
async fn v16_to_v17_strips_legacy_chat_mode_hint() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.db");

    {
        let store = SessionStore::open(&path).await.unwrap();
        store
            .create_session("s1", "tauri", None, None, None)
            .await.unwrap();
        store
            .append_message(NewMessage {
                session_id: "s1",
                role: "user",
                content: Some(
                    "帮我写个脚本\n\n---\n[Mode: Agent] 可执行工具。复杂多步任务可先 switch_mode。",
                ),
                ..NewMessage::empty("s1", "user")
            })
            .await.unwrap();
        store
            .append_message(NewMessage {
                session_id: "s1",
                role: "user",
                content: Some("干净消息，无 Mode 后缀"),
                ..NewMessage::empty("s1", "user")
            })
            .await.unwrap();
        store
            .append_message(NewMessage {
                session_id: "s1",
                role: "assistant",
                content: Some("ok\n\n---\n[Mode: Agent] should stay on assistant"),
                ..NewMessage::empty("s1", "assistant")
            })
            .await.unwrap();
    }

    {
        let pool = agent_db::sqlx::SqlitePool::connect(&format!("sqlite:{}?mode=rwc", &path.display())).await.unwrap();
        agent_db::sqlx::query("UPDATE schema_version SET version = 16")
            .execute(&pool)
            .await
            .unwrap();
    }

    let reopened = SessionStore::open(&path).await.unwrap();
    assert_eq!(reopened.schema_version().await.unwrap(), SCHEMA_VERSION);

    let msgs = reopened.get_messages("s1").await.unwrap();
    assert_eq!(msgs.len(), 3);
    assert_eq!(msgs[0].content.as_deref(), Some("帮我写个脚本"));
    assert_eq!(msgs[1].content.as_deref(), Some("干净消息，无 Mode 后缀"));
    assert_eq!(
        msgs[2].content.as_deref(),
        Some("ok\n\n---\n[Mode: Agent] should stay on assistant")
    );
}

#[tokio::test]
async fn pin_session_sorts_before_unpinned() {
    let (_dir, store) = test_store().await;
    store
        .create_session("older", "tauri", None, None, None)
        .await.unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    store
        .create_session("newer", "tauri", None, None, None)
        .await.unwrap();

    let before = store.list_sessions(SessionListFilter::Active, 10).await.unwrap();
    assert_eq!(before[0].id, "newer");

    store.pin_session("older").await.unwrap();
    let pinned = store.list_sessions(SessionListFilter::Active, 10).await.unwrap();
    assert_eq!(pinned[0].id, "older");
    assert!(pinned[0].pinned_at.is_some());
    assert!(pinned[1].pinned_at.is_none());

    store.unpin_session("older").await.unwrap();
    let after = store.list_sessions(SessionListFilter::Active, 10).await.unwrap();
    assert_eq!(after[0].id, "newer");
    assert!(after.iter().all(|s| s.pinned_at.is_none()));
}

#[tokio::test]
async fn title_if_empty_never_overwrites_manual_title() {
    let (_dir, store) = test_store().await;
    store
        .create_session("s1", "tauri", None, None, None)
        .await.unwrap();

    assert!(store.set_session_title_if_empty("s1", "Auto").await.unwrap());
    store.set_session_title("s1", "Manual").await.unwrap();

    assert!(!store.set_session_title_if_empty("s1", "Late").await.unwrap());
    assert_eq!(
        store.get_session("s1").await.unwrap().unwrap().title.as_deref(),
        Some("Manual")
    );
}

#[tokio::test]
async fn title_if_empty_suffixes_duplicate_generated_title() {
    let (_dir, store) = test_store().await;
    store
        .create_session("session-alpha", "tauri", None, None, None)
        .await.unwrap();
    store
        .create_session("session-beta", "tauri", None, None, None)
        .await.unwrap();

    assert!(store
        .set_session_title_if_empty("session-alpha", "Shared title")
        .await.unwrap());
    assert!(store
        .set_session_title_if_empty("session-beta", "Shared title")
        .await.unwrap());

    assert_eq!(
        store
            .get_session("session-alpha")
            .await.unwrap()
            .unwrap()
            .title
            .as_deref(),
        Some("Shared title")
    );
    assert_eq!(
        store
            .get_session("session-beta")
            .await.unwrap()
            .unwrap()
            .title
            .as_deref(),
        Some("Shared title · session-")
    );
}

#[tokio::test]
async fn permanent_delete_removes_messages_and_fts() {
    let (_dir, store) = test_store().await;
    store
        .create_session("s1", "tauri", None, None, None)
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("unique-delete-token"),
            ..NewMessage::empty("s1", "user")
        })
        .await.unwrap();

    store.delete_session_permanently("s1").await.unwrap();

    assert!(store.get_session("s1").await.unwrap().is_none());
    assert!(store
        .search_messages("unique-delete-token", None, None, 10)
        .await.unwrap()
        .is_empty());
}

#[tokio::test]
async fn permanent_delete_detaches_child_branches_before_removing_parent() {
    let (_dir, store) = test_store().await;
    store
        .create_session("parent", "tauri", None, None, None)
        .await.unwrap();
    store
        .create_session("child", "tauri", None, None, Some("parent"))
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("keep-child"),
            ..NewMessage::empty("child", "user")
        })
        .await.unwrap();

    store.delete_session_permanently("parent").await.unwrap();

    assert!(store.get_session("parent").await.unwrap().is_none());
    let child = store.get_session("child").await.unwrap().unwrap();
    assert!(child.parent_session_id.is_none());
    assert_eq!(
        store.get_messages("child").await.unwrap()[0].content.as_deref(),
        Some("keep-child")
    );
}

#[tokio::test]
async fn first_turn_text_returns_first_non_empty_user_and_assistant() {
    let (_dir, store) = test_store().await;
    store
        .create_session("s1", "tauri", None, None, None)
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("   "),
            ..NewMessage::empty("s1", "user")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("tool noise"),
            ..NewMessage::empty("s1", "tool")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("hello"),
            ..NewMessage::empty("s1", "user")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some(""),
            ..NewMessage::empty("s1", "assistant")
        })
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("world"),
            ..NewMessage::empty("s1", "assistant")
        })
        .await.unwrap();

    let first = store.first_turn_text("s1").await.unwrap();
    assert_eq!(
        first
            .as_ref()
            .map(|(user, assistant)| (user.as_str(), assistant.as_str())),
        Some(("hello", "world"))
    );
}

#[tokio::test]
async fn first_turn_text_returns_earliest_completed_user_assistant_pair() {
    let (_dir, store) = test_store().await;
    store
        .create_session("s1", "tauri", None, None, None)
        .await.unwrap();
    for (role, content) in [
        ("assistant", "orphan assistant"),
        ("user", "superseded user"),
        ("user", "paired user"),
        ("assistant", "paired assistant"),
    ] {
        store
            .append_message(NewMessage {
                content: Some(content),
                ..NewMessage::empty("s1", role)
            })
            .await.unwrap();
    }

    let first = store.first_turn_text("s1").await.unwrap();
    assert_eq!(
        first
            .as_ref()
            .map(|(user, assistant)| (user.as_str(), assistant.as_str())),
        Some(("paired user", "paired assistant"))
    );
}

#[tokio::test]
async fn first_turn_text_returns_none_without_assistant() {
    let (_dir, store) = test_store().await;
    store
        .create_session("s1", "tauri", None, None, None)
        .await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("user only"),
            ..NewMessage::empty("s1", "user")
        })
        .await.unwrap();

    assert!(store.first_turn_text("s1").await.unwrap().is_none());
}

#[tokio::test]
async fn append_and_reload_media_json() {
    let (_dir, store) = test_store().await;
    store
        .create_session("s1", "test", None, None, None)
        .await.unwrap();
    let media = r#"[{"kind":"image","mime_type":"image/png","reference":{"data_url":"data:image/png;base64,abc"}}]"#;
    store
        .append_message(NewMessage {
            content: Some("see pic"),
            media_json: Some(media),
            ..NewMessage::empty("s1", "user")
        })
        .await.unwrap();
    let msgs = store.get_messages("s1").await.unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].media_json.as_deref(), Some(media));
}

#[tokio::test]
async fn v14_to_v15_adds_media_json_without_data_loss() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.db");
    {
        let pool = agent_db::sqlx::SqlitePool::connect(&format!("sqlite:{}?mode=rwc", &path.display())).await.unwrap();
        agent_db::sqlx::raw_sql(
            "CREATE TABLE schema_version (version INTEGER NOT NULL);
             CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                started_at REAL NOT NULL,
                message_count INTEGER DEFAULT 0,
                tool_call_count INTEGER DEFAULT 0,
                archived_at REAL
             );
             CREATE TABLE messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT,
                compressed_content TEXT,
                tool_call_id TEXT,
                tool_calls TEXT,
                tool_name TEXT,
                timestamp REAL NOT NULL,
                token_count INTEGER,
                finish_reason TEXT,
                reasoning TEXT,
                reasoning_content TEXT,
                reasoning_details TEXT,
                codex_reasoning_items TEXT,
                codex_message_items TEXT
             );
             INSERT INTO schema_version (version) VALUES (14);
             INSERT INTO sessions (id, source, started_at, message_count, tool_call_count)
             VALUES ('s1', 'test', 1.0, 1, 0);
             INSERT INTO messages (session_id, role, content, timestamp)
             VALUES ('s1', 'user', 'hello v14', 1.0);",
        )
        .execute(&pool).await.unwrap();
    }
    let store = SessionStore::open(&path).await.unwrap();
    assert_eq!(store.schema_version().await.unwrap(), SCHEMA_VERSION);
    let msgs = store.get_messages("s1").await.unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].content.as_deref(), Some("hello v14"));
    assert!(msgs[0].media_json.is_none());
}
