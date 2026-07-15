use memory::session_store::NewMessage;
use memory::MemoryManager;
use tempfile::TempDir;

#[test]
fn memory_manager_record_message_uses_session_store() {
    let dir = TempDir::new().unwrap();
    let mgr = MemoryManager::new(dir.path().to_path_buf()).unwrap();
    mgr.ensure_session("s1", "test").unwrap();
    let id = mgr
        .record_message_ex(
            "s1",
            NewMessage {
                content: Some("hi"),
                ..NewMessage::empty("s1", "user")
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
