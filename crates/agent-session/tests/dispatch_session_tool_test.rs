use serde_json::json;
use session::{
    dispatch_session_tool, format_recalled_context, record_message, NewMessage, ScrolledMessage,
    SessionStore,
};
use tempfile::TempDir;

#[test]
fn format_recalled_marks_anchors() {
    let msgs = vec![
        ScrolledMessage {
            id: 1,
            role: "user".into(),
            content: "hi".into(),
            is_anchor: false,
        },
        ScrolledMessage {
            id: 2,
            role: "assistant".into(),
            content: "yo".into(),
            is_anchor: true,
        },
    ];
    let s = format_recalled_context(&msgs);
    assert_eq!(s, "[1] user: hi\n[2] assistant: yo [anchor]");
}

#[tokio::test]
async fn dispatch_session_search_and_record_message() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open_sessions_dir(&dir.path().join("sessions")).await.unwrap();
    store.ensure_session("s1", "test").await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("alpha fact"),
            ..NewMessage::empty("s1", "user")
        })
        .await
        .unwrap();

    let out = dispatch_session_tool(
        &store,
        "session_search",
        &json!({"query": "alpha", "limit": 5}),
    )
    .await
    .unwrap();
    assert!(out.contains("相关历史消息"));
    assert!(out.contains("alpha"));

    let id = record_message(&store, "s1", "assistant", "reply").await.unwrap();
    assert!(id > 0);
}
