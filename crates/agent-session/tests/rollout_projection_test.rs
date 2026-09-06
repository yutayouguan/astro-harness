use agent_protocol::{FunctionCallOutputPayload, ResponseItem};
use agent_rollout::RolloutItem;
use session::SessionStore;

#[tokio::test]
async fn rollout_rebuild_preserves_native_items_exactly_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db"))
        .await
        .unwrap();
    let expected = vec![
        ResponseItem::user_text("hello"),
        ResponseItem::FunctionCall {
            id: Some("fc_1".into()),
            name: "exec_command".into(),
            namespace: None,
            arguments: "{}".into(),
            encrypted_function_args: None,
            call_id: "call_1".into(),
            internal_chat_message_metadata_passthrough: Some(serde_json::json!({"x": 1})),
        },
        ResponseItem::FunctionCallOutput {
            id: Some("out_1".into()),
            call_id: Some("call_1".into()),
            name: Some("exec_command".into()),
            namespace: None,
            output: FunctionCallOutputPayload::from_text("ok".into()),
            internal_chat_message_metadata_passthrough: None,
        },
    ];
    let rollout = std::iter::once(RolloutItem::SessionMeta(serde_json::json!({"id": "s1"})))
        .chain(expected.iter().cloned().map(RolloutItem::ResponseItem))
        .chain(std::iter::once(RolloutItem::TurnContext(
            serde_json::json!({"turn": 1}),
        )))
        .collect::<Vec<_>>();

    session::store::rebuild_response_items_from_rollout(&store, "s1", &rollout)
        .await
        .unwrap();
    session::store::rebuild_response_items_from_rollout(&store, "s1", &rollout)
        .await
        .unwrap();

    let actual = store
        .get_response_items("s1")
        .await
        .unwrap()
        .into_iter()
        .map(|stored| stored.item)
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn rollout_rebuild_replaces_existing_index_rows() {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db"))
        .await
        .unwrap();
    session::store::rebuild_response_items_from_rollout(
        &store,
        "s1",
        &[RolloutItem::ResponseItem(ResponseItem::user_text("first"))],
    )
    .await
    .unwrap();
    session::store::rebuild_response_items_from_rollout(
        &store,
        "s1",
        &[RolloutItem::ResponseItem(ResponseItem::user_text(
            "replacement",
        ))],
    )
    .await
    .unwrap();

    let items = store.get_response_items("s1").await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].text(), "replacement");
}

#[tokio::test]
async fn rollout_rebuild_applies_rollback_before_indexing() {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db"))
        .await
        .unwrap();
    let rollout = vec![
        RolloutItem::ResponseItem(ResponseItem::user_text("keep")),
        RolloutItem::ResponseItem(ResponseItem::assistant_text("kept answer")),
        RolloutItem::ResponseItem(ResponseItem::user_text("remove")),
        RolloutItem::ResponseItem(ResponseItem::assistant_text("removed answer")),
        RolloutItem::EventMsg(agent_protocol::EventMsg::ThreadRolledBack(
            agent_protocol::ThreadRolledBackEvent {
                num_turns: 1,
                keep_chat_bubbles: None,
            },
        )),
    ];

    session::store::rebuild_response_items_from_rollout(&store, "s1", &rollout)
        .await
        .unwrap();

    let items = store.get_response_items("s1").await.unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].text(), "keep");
    assert_eq!(items[1].text(), "kept answer");
}
