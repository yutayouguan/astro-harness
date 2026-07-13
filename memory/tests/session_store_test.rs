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
