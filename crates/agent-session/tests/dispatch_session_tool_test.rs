use serde_json::json;
use session::{
    dispatch_session_tool, format_recalled_context, record_message, NewResponseItem,
    ScrolledResponseItem, SessionStore,
};

#[test]
fn format_recalled_marks_anchors() {
    let items = vec![
        ScrolledResponseItem {
            id: 1,
            item: agent_protocol::ResponseItem::user_text("hi"),
            is_anchor: false,
        },
        ScrolledResponseItem {
            id: 2,
            item: agent_protocol::ResponseItem::assistant_text("yo"),
            is_anchor: true,
        },
    ];
    assert_eq!(
        format_recalled_context(&items),
        "[1] user: hi\n[2] assistant: yo [anchor]"
    );
}

#[tokio::test]
async fn dispatch_session_search_and_record_native_item() {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::open_sessions_dir(&dir.path().join("sessions"))
        .await
        .unwrap();
    store.ensure_session("s1", "test").await.unwrap();
    let item = agent_protocol::ResponseItem::user_text("alpha fact");
    store
        .append_response_item(NewResponseItem::new("s1", &item))
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

    let id = record_message(&store, "s1", "assistant", "reply")
        .await
        .unwrap();
    assert!(id > 0);
    assert_eq!(
        store.get_response_items("s1").await.unwrap()[1].item,
        agent_protocol::ResponseItem::assistant_text("reply")
    );
}
