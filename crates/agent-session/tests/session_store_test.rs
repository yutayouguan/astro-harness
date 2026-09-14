use agent_protocol::{FunctionCallOutputPayload, ResponseItem};
use session::{NewResponseItem, SessionStore, SCHEMA_VERSION};
use tempfile::TempDir;
use types::SqliteStore;

async fn test_store() -> (TempDir, SessionStore) {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db"))
        .await
        .unwrap();
    (dir, store)
}

#[tokio::test]
async fn fresh_schema_persists_response_items_without_messages_table() {
    let (_dir, store) = test_store().await;
    assert_eq!(store.schema_version().await.unwrap(), SCHEMA_VERSION);
    let tables = agent_db::sqlx::query_scalar::<_, String>(
        "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
    )
    .fetch_all(store.pool())
    .await
    .unwrap();
    assert!(tables.iter().any(|table| table == "response_items"));
    assert!(!tables.iter().any(|table| table == "messages"));
}

#[tokio::test]
async fn v22_upgrade_preserves_history_billing_and_fts() {
    let (dir, store) = test_store().await;
    store.ensure_session("kept", "test").await.unwrap();
    store
        .append_response_item(NewResponseItem::new(
            "kept",
            &ResponseItem::user_text("migration evidence"),
        ))
        .await
        .unwrap();
    agent_db::sqlx::raw_sql("DROP TABLE thread_context; UPDATE schema_version SET version=22; UPDATE sessions SET input_tokens=123 WHERE id='kept';")
        .execute(store.pool()).await.unwrap();
    drop(store);
    let upgraded = SessionStore::open(&dir.path().join("state.db"))
        .await
        .unwrap();
    assert_eq!(upgraded.schema_version().await.unwrap(), SCHEMA_VERSION);
    assert_eq!(
        upgraded.get_response_items("kept").await.unwrap()[0].text(),
        "migration evidence"
    );
    let tokens: i64 =
        agent_db::sqlx::query_scalar("SELECT input_tokens FROM sessions WHERE id='kept'")
            .fetch_one(upgraded.pool())
            .await
            .unwrap();
    assert_eq!(tokens, 123);
    assert!(!upgraded
        .search_messages("migration", None, None, 5)
        .await
        .unwrap()
        .is_empty());
    upgraded
        .write_thread_notes("kept", "still here", 0)
        .await
        .unwrap();
    assert!(upgraded
        .list_thread_attachments("kept", None, 10)
        .await
        .unwrap()
        .data
        .is_empty());
}

#[tokio::test]
async fn v23_upgrade_adds_thread_attachments_without_rebuilding_sessions() {
    let (dir, store) = test_store().await;
    store.ensure_session("kept", "test").await.unwrap();
    agent_db::sqlx::raw_sql("DROP TABLE thread_attachments; UPDATE schema_version SET version=23;")
        .execute(store.pool())
        .await
        .unwrap();
    drop(store);

    let upgraded = SessionStore::open(&dir.path().join("state.db"))
        .await
        .unwrap();
    assert_eq!(upgraded.schema_version().await.unwrap(), SCHEMA_VERSION);
    assert!(upgraded.get_session("kept").await.unwrap().is_some());
    let added = upgraded
        .add_thread_attachment(
            "kept",
            "workspace_file",
            "notes.md",
            &serde_json::json!({"path":"notes.md"}),
        )
        .await
        .unwrap();
    assert_eq!(
        added.outcome,
        agent_protocol::ThreadAttachmentAddOutcome::Created
    );
}

#[tokio::test]
async fn opening_an_old_schema_destructively_rebuilds_it() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.db");
    let pool = agent_db::sqlx::SqlitePool::connect(&format!("sqlite:{}?mode=rwc", path.display()))
        .await
        .unwrap();
    agent_db::sqlx::raw_sql(
        "CREATE TABLE schema_version (version INTEGER NOT NULL);
         INSERT INTO schema_version VALUES (21);
         CREATE TABLE messages (id INTEGER PRIMARY KEY, content TEXT);
         INSERT INTO messages(content) VALUES ('obsolete');",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let store = SessionStore::open(&path).await.unwrap();
    assert_eq!(store.schema_version().await.unwrap(), SCHEMA_VERSION);
    let old_table: i64 = agent_db::sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='messages'",
    )
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(old_table, 0);
}

#[tokio::test]
async fn native_response_items_round_trip_without_projection() {
    let (_dir, store) = test_store().await;
    store.ensure_session("s1", "test").await.unwrap();
    let items = vec![
        ResponseItem::user_text("hello"),
        ResponseItem::FunctionCall {
            id: Some("fc_1".into()),
            name: "exec_command".into(),
            namespace: None,
            arguments: r#"{"cmd":"pwd"}"#.into(),
            encrypted_function_args: None,
            call_id: "call_1".into(),
            internal_chat_message_metadata_passthrough: None,
        },
        ResponseItem::FunctionCallOutput {
            id: Some("out_1".into()),
            call_id: Some("call_1".into()),
            name: Some("exec_command".into()),
            namespace: None,
            output: FunctionCallOutputPayload::from_text("/tmp".into()),
            internal_chat_message_metadata_passthrough: Some(serde_json::json!({
                "custom": true
            })),
        },
        ResponseItem::assistant_text("done"),
    ];
    store.append_response_items("s1", &items).await.unwrap();

    let stored = store.get_response_items("s1").await.unwrap();
    assert_eq!(
        stored.iter().map(|entry| &entry.item).collect::<Vec<_>>(),
        items.iter().collect::<Vec<_>>()
    );
    assert_eq!(stored[1].call_id(), Some("call_1"));
    assert_eq!(stored[2].tool_name(), Some("exec_command"));
    assert!(stored[2].is_tool_output());
}

#[tokio::test]
async fn compression_changes_only_response_item_metadata() {
    let (_dir, store) = test_store().await;
    store.ensure_session("s1", "test").await.unwrap();
    let output = ResponseItem::FunctionCallOutput {
        id: None,
        call_id: Some("call_1".into()),
        name: Some("exec_command".into()),
        namespace: None,
        output: FunctionCallOutputPayload::from_text("full output".into()),
        internal_chat_message_metadata_passthrough: None,
    };
    let id = store
        .append_response_item(NewResponseItem::new("s1", &output))
        .await
        .unwrap();
    store
        .update_response_item_compressed_content(id, Some("short"))
        .await
        .unwrap();

    let stored = store.get_response_items("s1").await.unwrap().remove(0);
    assert_eq!(stored.text(), "full output");
    assert_eq!(stored.compressed_text(), Some("short"));
}

#[tokio::test]
async fn assistant_metadata_patch_targets_message_not_following_tool_call() {
    let (_dir, store) = test_store().await;
    store.ensure_session("s1", "test").await.unwrap();
    store
        .append_response_items(
            "s1",
            &[
                ResponseItem::assistant_text("working"),
                ResponseItem::FunctionCall {
                    id: None,
                    name: "exec_command".into(),
                    namespace: None,
                    arguments: "{}".into(),
                    encrypted_function_args: None,
                    call_id: "call_1".into(),
                    internal_chat_message_metadata_passthrough: None,
                },
            ],
        )
        .await
        .unwrap();

    store
        .patch_last_assistant_metadata("s1", &serde_json::json!({"astro_timeline": []}))
        .await
        .unwrap();
    let stored = store.get_response_items("s1").await.unwrap();
    assert!(stored[0]
        .item
        .metadata()
        .and_then(|metadata| metadata.get("astro_timeline"))
        .is_some());
    assert!(stored[1].item.metadata().is_none());
}

#[tokio::test]
async fn fork_and_truncate_preserve_complete_response_item_groups() {
    let (_dir, store) = test_store().await;
    store.ensure_session("source", "test").await.unwrap();
    let items = vec![
        ResponseItem::user_text("u1"),
        ResponseItem::FunctionCall {
            id: None,
            name: "exec_command".into(),
            namespace: None,
            arguments: "{}".into(),
            encrypted_function_args: None,
            call_id: "call_1".into(),
            internal_chat_message_metadata_passthrough: None,
        },
        ResponseItem::FunctionCallOutput {
            id: None,
            call_id: Some("call_1".into()),
            name: Some("exec_command".into()),
            namespace: None,
            output: FunctionCallOutputPayload::from_text("ok".into()),
            internal_chat_message_metadata_passthrough: None,
        },
        ResponseItem::assistant_text("a1"),
        ResponseItem::user_text("u2"),
        ResponseItem::assistant_text("a2"),
    ];
    store.append_response_items("source", &items).await.unwrap();

    store.fork_session("source", "branch", 2).await.unwrap();
    assert_eq!(store.get_response_items("branch").await.unwrap().len(), 4);
    store
        .truncate_session_to_bubbles("source", 2)
        .await
        .unwrap();
    assert_eq!(store.get_response_items("source").await.unwrap().len(), 4);
}

#[tokio::test]
async fn canonical_history_replacement_preserves_matching_prefix_metadata() {
    let (_dir, store) = test_store().await;
    store.ensure_session("source", "test").await.unwrap();
    let first = ResponseItem::user_text("u1");
    let second = ResponseItem::assistant_text("a1");
    let removed = ResponseItem::user_text("u2");
    store
        .append_response_item(NewResponseItem {
            session_id: "source",
            item: &first,
            token_count: Some(7),
            finish_reason: None,
        })
        .await
        .unwrap();
    store
        .append_response_items("source", &[second.clone(), removed])
        .await
        .unwrap();
    let original = store.get_response_items("source").await.unwrap();

    store
        .replace_response_items("source", &[first.clone(), second.clone()])
        .await
        .unwrap();
    let retained = store.get_response_items("source").await.unwrap();
    assert_eq!(retained.len(), 2);
    assert_eq!(retained[0].id, original[0].id);
    assert_eq!(retained[0].token_count, Some(7));

    let replacement = ResponseItem::developer_text("summary");
    store
        .replace_response_items("source", std::slice::from_ref(&replacement))
        .await
        .unwrap();
    let rebuilt = store.get_response_items("source").await.unwrap();
    assert_eq!(rebuilt.len(), 1);
    assert_eq!(rebuilt[0].item, replacement);
}

#[tokio::test]
async fn bubble_operations_coalesce_consecutive_assistant_response_items() {
    let (_dir, store) = test_store().await;
    store.ensure_session("source", "test").await.unwrap();
    let items = vec![
        ResponseItem::user_text("u1"),
        ResponseItem::FunctionCall {
            id: None,
            name: "exec_command".into(),
            namespace: None,
            arguments: "{}".into(),
            encrypted_function_args: None,
            call_id: "call_1".into(),
            internal_chat_message_metadata_passthrough: None,
        },
        ResponseItem::assistant_text("commentary"),
        ResponseItem::assistant_text("final"),
        ResponseItem::user_text("u2"),
        ResponseItem::assistant_text("a2"),
    ];
    store.append_response_items("source", &items).await.unwrap();

    store.fork_session("source", "branch", 2).await.unwrap();
    let branch = store.get_response_items("branch").await.unwrap();
    assert_eq!(branch.len(), 4);
    assert_eq!(branch.last().unwrap().text(), "final");

    store.remove_chat_bubbles("source", 1, 2).await.unwrap();
    let remaining = store.get_response_items("source").await.unwrap();
    assert_eq!(remaining.len(), 3);
    assert_eq!(remaining[0].text(), "u1");
    assert_eq!(remaining[1].text(), "u2");
    assert_eq!(remaining[2].text(), "a2");
}

#[tokio::test]
async fn fork_recent_turns_rolls_back_target_after_item_insert_failure() {
    let (_dir, store) = test_store().await;
    store.ensure_session("source", "test").await.unwrap();
    store
        .append_response_items(
            "source",
            &[
                ResponseItem::user_text("hello"),
                ResponseItem::assistant_text("answer"),
            ],
        )
        .await
        .unwrap();
    agent_db::sqlx::raw_sql(
        "CREATE TRIGGER fail_branch_response_item
         BEFORE INSERT ON response_items
         WHEN NEW.session_id = 'branch'
         BEGIN SELECT RAISE(ABORT, 'forced branch insert failure'); END;",
    )
    .execute(store.pool())
    .await
    .unwrap();

    assert!(store
        .fork_session_recent_turns("source", "branch", None)
        .await
        .is_err());
    assert!(store.get_session("branch").await.unwrap().is_none());
}

#[tokio::test]
async fn fork_recent_turns_keeps_the_complete_last_user_turn() {
    let (_dir, store) = test_store().await;
    store.ensure_session("source", "test").await.unwrap();
    store
        .append_response_items(
            "source",
            &[
                ResponseItem::user_text("u1"),
                ResponseItem::assistant_text("a1"),
                ResponseItem::user_text("u2"),
                ResponseItem::FunctionCall {
                    id: None,
                    name: "exec_command".into(),
                    namespace: None,
                    arguments: "{}".into(),
                    encrypted_function_args: None,
                    call_id: "call_2".into(),
                    internal_chat_message_metadata_passthrough: None,
                },
                ResponseItem::assistant_text("a2"),
            ],
        )
        .await
        .unwrap();

    store
        .fork_session_recent_turns("source", "branch", Some(1))
        .await
        .unwrap();
    let branch = store.get_response_items("branch").await.unwrap();
    assert_eq!(branch.len(), 3);
    assert_eq!(branch[0].text(), "u2");
    assert_eq!(branch[2].text(), "a2");
}

#[tokio::test]
async fn compact_keeps_summary_and_tail_counts_consistent() {
    let (_dir, store) = test_store().await;
    store.ensure_session("old", "test").await.unwrap();
    store
        .append_response_items(
            "old",
            &[
                ResponseItem::user_text("first"),
                ResponseItem::assistant_text("answer"),
                ResponseItem::user_text("last"),
                ResponseItem::assistant_text("last answer"),
            ],
        )
        .await
        .unwrap();
    store
        .compact_and_split("old", "new", "summary", 2)
        .await
        .unwrap();

    let items = store.get_response_items("new").await.unwrap();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].text(), "summary");
    assert_eq!(
        store
            .get_session("new")
            .await
            .unwrap()
            .unwrap()
            .message_count,
        3
    );
}

#[tokio::test]
async fn compact_rolls_back_source_and_target_after_item_insert_failure() {
    let (_dir, store) = test_store().await;
    store.ensure_session("old", "test").await.unwrap();
    store
        .append_response_items(
            "old",
            &[
                ResponseItem::user_text("hello"),
                ResponseItem::assistant_text("answer"),
            ],
        )
        .await
        .unwrap();
    agent_db::sqlx::raw_sql(
        "CREATE TRIGGER fail_compacted_response_item
         BEFORE INSERT ON response_items
         WHEN NEW.session_id = 'new'
         BEGIN SELECT RAISE(ABORT, 'forced compaction insert failure'); END;",
    )
    .execute(store.pool())
    .await
    .unwrap();

    assert!(store
        .compact_and_split("old", "new", "summary", 1)
        .await
        .is_err());
    assert!(store.get_session("new").await.unwrap().is_none());
    assert!(store
        .get_session("old")
        .await
        .unwrap()
        .unwrap()
        .ended_at
        .is_none());
}

#[tokio::test]
async fn fts_indexes_native_item_text_and_tool_name() {
    let (_dir, store) = test_store().await;
    store.ensure_session("s1", "test").await.unwrap();
    let call = ResponseItem::FunctionCall {
        id: None,
        name: "terminal".into(),
        namespace: None,
        arguments: r#"{"cmd":"中文检索"}"#.into(),
        encrypted_function_args: None,
        call_id: "call_1".into(),
        internal_chat_message_metadata_passthrough: None,
    };
    store
        .append_response_item(NewResponseItem::new("s1", &call))
        .await
        .unwrap();
    assert!(!store
        .search_messages("中文检索", None, None, 5)
        .await
        .unwrap()
        .is_empty());
    assert!(!store
        .search_messages("terminal", None, None, 5)
        .await
        .unwrap()
        .is_empty());
}
