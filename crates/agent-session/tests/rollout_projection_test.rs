use agent_rollout::RolloutItem;
use session::store::{NewMessage, SessionStore};
use types::message::{ContentPart, Message, MessageContent, Role, ToolCall};
use types::{MediaAsset, MediaKind, MediaRef};

fn response_items(message: &Message, tool_name: Option<&str>) -> Vec<RolloutItem> {
    agent_rollout::response_items_from_message(message, tool_name)
        .unwrap()
        .into_iter()
        .map(RolloutItem::ResponseItem)
        .collect()
}

fn response_item(message: Message) -> RolloutItem {
    let mut items = response_items(&message, None);
    assert_eq!(items.len(), 1);
    items.remove(0)
}

fn rich_rollout() -> Vec<RolloutItem> {
    let image_url = "data:image/png;base64,aGVsbG8=".to_string();
    let mut user = Message::user_with_images("hello", &[image_url]);
    user.compressed_content = Some("hello compressed".into());

    let mut assistant = Message::assistant_with_tools(
        "world",
        vec![ToolCall {
            id: "call-1".into(),
            name: "exec_command".into(),
            arguments: serde_json::json!({"command": "pwd"}),
            signature: Some("tool-signature".into()),
        }],
    );
    assistant.compressed_content = Some("world compressed".into());
    assistant.reasoning = Some("careful reasoning".into());
    assistant.thought_signature = Some("thought-signature".into());

    let mut tool = Message::tool_with_media(
        "call-1",
        "tool output",
        vec![MediaAsset::workspace(
            MediaKind::Image,
            "generated/result.png",
            "image/png",
        )],
    );
    tool.compressed_content = Some("tool compressed".into());

    let mut items = vec![RolloutItem::SessionMeta(
        serde_json::json!({"id": "thread-1"}),
    )];
    items.extend(response_items(&user, None));
    items.extend(response_items(&assistant, None));
    items.extend(response_items(&tool, Some("exec_command")));
    items.push(RolloutItem::TurnContext(
        serde_json::json!({"turn_id": "turn-1"}),
    ));
    items
}

#[tokio::test]
async fn repeated_rebuild_is_idempotent_for_native_response_items() {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db"))
        .await
        .unwrap();
    let items = rich_rollout();

    session::store::rebuild_messages_from_rollout(&store, "thread-1", &items)
        .await
        .unwrap();
    let first_timestamps = store
        .get_messages("thread-1")
        .await
        .unwrap()
        .into_iter()
        .map(|message| message.timestamp)
        .collect::<Vec<_>>();
    std::thread::sleep(std::time::Duration::from_millis(2));
    session::store::rebuild_messages_from_rollout(&store, "thread-1", &items)
        .await
        .unwrap();

    let messages = store.get_messages("thread-1").await.unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(
        messages
            .iter()
            .map(|message| message.timestamp)
            .collect::<Vec<_>>(),
        first_timestamps,
        "rebuild timestamps derive from stable session metadata"
    );
    assert_eq!(messages[0].role, "user");
    assert_eq!(messages[0].content.as_deref(), Some("hello"));
    assert_eq!(
        messages[0].compressed_content.as_deref(),
        Some("hello compressed")
    );
    let user_media: Vec<MediaAsset> =
        serde_json::from_str(messages[0].media_json.as_deref().unwrap()).unwrap();
    assert_eq!(user_media.len(), 1);
    assert_eq!(user_media[0].mime_type, "image/png");

    assert_eq!(messages[1].role, "assistant");
    assert_eq!(messages[1].content.as_deref(), Some("world"));
    assert_eq!(messages[1].compressed_content, None);
    assert_eq!(messages[1].reasoning.as_deref(), Some("careful reasoning"));
    assert_eq!(messages[1].tool_calls.as_ref().unwrap()[0]["id"], "call-1");
    assert!(messages[1].tool_calls.as_ref().unwrap()[0]["signature"].is_null());
    assert!(messages[1].reasoning_details.is_none());

    assert_eq!(messages[2].role, "tool");
    assert_eq!(messages[2].tool_call_id.as_deref(), Some("call-1"));
    assert_eq!(messages[2].tool_name.as_deref(), Some("exec_command"));
    assert_eq!(messages[2].content.as_deref(), Some("tool output"));
    assert_eq!(messages[2].compressed_content, None);
    let tool_media: Vec<MediaAsset> =
        serde_json::from_str(messages[2].media_json.as_deref().unwrap()).unwrap();
    assert_eq!(tool_media.len(), 1);

    let session = store.get_session("thread-1").await.unwrap().unwrap();
    assert_eq!(session.source, "rollout");
    assert_eq!(session.message_count, 3);
    assert_eq!(session.tool_call_count, 1);
}

#[tokio::test]
async fn rebuild_only_replaces_the_target_session_projection() {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db"))
        .await
        .unwrap();
    store.ensure_session("thread-1", "existing").await.unwrap();
    store
        .set_session_title("thread-1", "Keep this title")
        .await
        .unwrap();
    store.ensure_session("thread-2", "test").await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("other session survives"),
            ..NewMessage::empty("thread-2", "user")
        })
        .await
        .unwrap();

    session::store::rebuild_messages_from_rollout(
        &store,
        "thread-1",
        &[response_item(Message::user("replacement"))],
    )
    .await
    .unwrap();

    assert_eq!(
        store.get_messages("thread-2").await.unwrap()[0]
            .content
            .as_deref(),
        Some("other session survives")
    );
    assert_eq!(
        store
            .get_session("thread-2")
            .await
            .unwrap()
            .unwrap()
            .message_count,
        1
    );
    let target = store.get_session("thread-1").await.unwrap().unwrap();
    assert_eq!(target.title.as_deref(), Some("Keep this title"));
    assert_eq!(target.source, "existing");
    assert_eq!(target.message_count, 1);
}

#[tokio::test]
async fn failed_rebuild_rolls_back_target_delete_and_partial_inserts() {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db"))
        .await
        .unwrap();
    store.ensure_session("thread-1", "test").await.unwrap();
    store
        .append_message(NewMessage {
            content: Some("original projection"),
            ..NewMessage::empty("thread-1", "user")
        })
        .await
        .unwrap();
    let raw =
        agent_db::SqlitePool::connect(&format!("sqlite:{}?mode=rwc", store.db_path().display()))
            .await
            .unwrap();
    agent_db::sqlx::raw_sql(
        "CREATE TRIGGER reject_projection_insert
         BEFORE INSERT ON messages
         WHEN NEW.content = 'reject-me'
         BEGIN
             SELECT RAISE(ABORT, 'projection insert rejected');
         END;",
    )
    .execute(&raw)
    .await
    .unwrap();
    raw.close().await;

    let result = session::store::rebuild_messages_from_rollout(
        &store,
        "thread-1",
        &[
            response_item(Message::assistant("partial replacement")),
            response_item(Message::tool("reject-me")),
        ],
    )
    .await;
    assert!(result.is_err());

    let messages = store.get_messages("thread-1").await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content.as_deref(), Some("original projection"));
    let session = store.get_session("thread-1").await.unwrap().unwrap();
    assert_eq!(session.message_count, 1);
    assert_eq!(session.tool_call_count, 0);

    let missing_session_result = session::store::rebuild_messages_from_rollout(
        &store,
        "thread-new",
        &[response_item(Message::user("reject-me"))],
    )
    .await;
    assert!(missing_session_result.is_err());
    assert!(store.get_session("thread-new").await.unwrap().is_none());
}

#[tokio::test]
async fn duplicate_tool_call_ids_bind_results_in_rollout_order() {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db"))
        .await
        .unwrap();
    let call = |name: &str| ToolCall {
        id: "duplicate-id".into(),
        name: name.into(),
        arguments: serde_json::json!({"name": name}),
        signature: None,
    };
    let mut items = Vec::new();
    items.extend(response_items(
        &Message::assistant_with_tools("first", vec![call("tool-a")]),
        None,
    ));
    items.extend(response_items(
        &Message::tool_with_id("duplicate-id", "result-a"),
        Some("tool-a"),
    ));
    items.extend(response_items(
        &Message::assistant_with_tools("second", vec![call("tool-b")]),
        None,
    ));
    items.extend(response_items(
        &Message::tool_with_id("duplicate-id", "result-b"),
        Some("tool-b"),
    ));
    items.extend(response_items(
        &Message::tool_with_id("missing-id", "unmatched"),
        None,
    ));

    session::store::rebuild_messages_from_rollout(&store, "thread-tools", &items)
        .await
        .unwrap();

    let messages = store.get_messages("thread-tools").await.unwrap();
    assert_eq!(messages[1].tool_name.as_deref(), Some("tool-a"));
    assert_eq!(messages[3].tool_name.as_deref(), Some("tool-b"));
    assert_eq!(messages[4].tool_name, None);
}

#[tokio::test]
async fn parts_only_media_is_projected_and_redundant_explicit_media_is_deduplicated() {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db"))
        .await
        .unwrap();
    let parts_only = Message {
        role: Role::User,
        content: MessageContent::Parts(vec![
            ContentPart::text("inspect media"),
            ContentPart::image_url("data:image/png;base64,aW1hZ2U="),
            ContentPart::audio_url("https://example.test/audio.mp3", "audio/mpeg"),
            ContentPart::video_url("data:video/webm;base64,dmlkZW8=", "video/ignored"),
        ]),
        compressed_content: None,
        tool_calls: None,
        tool_call_id: None,
        media: Vec::new(),
        reasoning: None,
        thought_signature: None,
    };
    let duplicate_image = "data:image/png;base64,ZHVwbGljYXRl".to_string();
    let redundant = Message::user_with_images("dedupe", &[duplicate_image]);

    session::store::rebuild_messages_from_rollout(
        &store,
        "thread-media",
        &[
            response_item(parts_only),
            response_item(Message::assistant("boundary")),
            response_item(redundant),
        ],
    )
    .await
    .unwrap();

    let messages = store.get_messages("thread-media").await.unwrap();
    let media: Vec<MediaAsset> =
        serde_json::from_str(messages[0].media_json.as_deref().unwrap()).unwrap();
    assert_eq!(media.len(), 2);
    assert!(matches!(
        &media[0],
        MediaAsset {
            kind: MediaKind::Image,
            mime_type,
            reference: MediaRef::DataUrl(url),
            ..
        } if mime_type == "image/png" && url.starts_with("data:image/png;")
    ));
    assert!(matches!(
        &media[1],
        MediaAsset {
            kind: MediaKind::Audio,
            mime_type,
            reference: MediaRef::RemoteUri(url),
            ..
        } if mime_type == "audio/*" && url == "https://example.test/audio.mp3"
    ));
    let redundant_media: Vec<MediaAsset> =
        serde_json::from_str(messages[2].media_json.as_deref().unwrap()).unwrap();
    assert_eq!(redundant_media.len(), 1);

    let invalid = Message {
        role: Role::User,
        content: MessageContent::Parts(vec![ContentPart::audio_url(
            "relative/audio.mp3",
            "audio/mpeg",
        )]),
        compressed_content: None,
        tool_calls: None,
        tool_call_id: None,
        media: Vec::new(),
        reasoning: None,
        thought_signature: None,
    };
    assert!(session::store::rebuild_messages_from_rollout(
        &store,
        "thread-invalid-media",
        &[response_item(invalid)],
    )
    .await
    .is_err());
    assert!(store
        .get_session("thread-invalid-media")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn invalid_adjacent_roles_are_rejected_before_projection_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db"))
        .await
        .unwrap();
    store
        .ensure_session("thread-existing", "test")
        .await
        .unwrap();
    store
        .append_message(NewMessage {
            content: Some("original"),
            ..NewMessage::empty("thread-existing", "user")
        })
        .await
        .unwrap();

    let existing_result = session::store::rebuild_messages_from_rollout(
        &store,
        "thread-existing",
        &[
            response_item(Message::user("first")),
            response_item(Message::system("filtered")),
            response_item(Message::user("second")),
        ],
    )
    .await;
    assert!(existing_result.is_err());
    let messages = store.get_messages("thread-existing").await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content.as_deref(), Some("original"));
    assert_eq!(
        store
            .get_session("thread-existing")
            .await
            .unwrap()
            .unwrap()
            .message_count,
        1
    );

    session::store::rebuild_messages_from_rollout(
        &store,
        "thread-new-invalid",
        &[
            response_item(Message::assistant("first")),
            response_item(Message::assistant("second")),
        ],
    )
    .await
    .unwrap();
    let messages = store.get_messages("thread-new-invalid").await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content.as_deref(), Some("first\nsecond"));
}
