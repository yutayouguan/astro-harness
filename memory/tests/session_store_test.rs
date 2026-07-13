use memory::session_store::SessionStore;
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
