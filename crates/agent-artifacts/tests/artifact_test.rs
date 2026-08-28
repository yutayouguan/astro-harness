use agent_db::{sqlx, AstroDb, DbSpec};
use artifacts::db::{category_from_name, ArtifactDb, ArtifactSource};
use tempfile::tempdir;

#[test]
fn category_from_extension() {
    assert_eq!(category_from_name("a.md"), "doc");
    assert_eq!(category_from_name("a.CSV"), "sheet");
    assert_eq!(category_from_name("x.png"), "image");
    assert_eq!(category_from_name("v.mp4"), "av");
    assert_eq!(category_from_name("m.rs"), "code");
    assert_eq!(category_from_name("s.pdf"), "pdf_ppt");
    assert_eq!(category_from_name("z.bin"), "other");
}

#[test]
fn junk_artifact_names() {
    use artifacts::db::is_junk_artifact_name;
    assert!(is_junk_artifact_name(".DS_Store"));
    assert!(is_junk_artifact_name(".ds_store"));
    assert!(is_junk_artifact_name("Thumbs.db"));
    assert!(is_junk_artifact_name("desktop.ini"));
    assert!(is_junk_artifact_name("._photo.png"));
    assert!(!is_junk_artifact_name("notes.md"));
    assert!(!is_junk_artifact_name(".gitignore"));
}

#[tokio::test]
async fn reconcile_skips_ds_store() {
    let dir = tempdir().unwrap();
    let memory_root = dir.path().to_path_buf();
    let workspace = memory_root.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("keep.md"), b"ok").unwrap();
    std::fs::write(workspace.join(".DS_Store"), b"junk").unwrap();
    std::fs::write(workspace.join("._keep.md"), b"appledouble").unwrap();

    let db_path = memory_root.join("sessions/artifacts.db");
    std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();

    {
        let spec = DbSpec::new("artifacts", "artifacts.db");
        let old_db = AstroDb::new(db_path.parent().unwrap());
        let pool = old_db.open_pool(&spec).await.unwrap();
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS artifacts (
                id TEXT PRIMARY KEY,
                path TEXT UNIQUE NOT NULL,
                name TEXT NOT NULL,
                category TEXT NOT NULL,
                mime TEXT,
                size INTEGER NOT NULL DEFAULT 0,
                source TEXT NOT NULL,
                session_id TEXT,
                message_id TEXT,
                agent_id TEXT NOT NULL DEFAULT 'workspace',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                missing INTEGER NOT NULL DEFAULT 0
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO artifacts (id, path, name, category, size, source, agent_id, missing)
             VALUES ('x', ?1, '.DS_Store', 'other', 1, 'reconcile', 'workspace', 0)",
        )
        .bind(workspace.join(".DS_Store").to_string_lossy().to_string())
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
    }

    let db = ArtifactDb::new(db_path).await.unwrap();
    let report = db.reconcile(&memory_root).await.unwrap();
    assert_eq!(report.added, 1);

    let all = db.list(None, None, false, 100, true, None).await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].name, "keep.md");
}

#[tokio::test]
async fn register_and_list_by_category() {
    let dir = tempdir().unwrap();
    let db = ArtifactDb::new(dir.path().join("artifacts.db")).await.unwrap();
    let path = dir.path().join("workspace").join("note.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"hi").unwrap();

    db.register(
        path.to_str().unwrap(),
        ArtifactSource::AgentWrite,
        Some("sess-1"),
        Some("msg-1"),
        None,
    )
    .await
    .unwrap();

    let listed = db
        .list(Some("doc"), None, false, 50, false, None)
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].session_id.as_deref(), Some("sess-1"));
    assert_eq!(listed[0].name, "note.md");
    assert_eq!(listed[0].category, "doc");
}

#[tokio::test]
async fn reconcile_adds_and_marks_missing() {
    let dir = tempdir().unwrap();
    let memory_root = dir.path().to_path_buf();
    let uploads = memory_root.join("uploads").join("sess-a");
    let workspace = memory_root.join("workspace");
    std::fs::create_dir_all(&uploads).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();

    let keep = uploads.join("photo.png");
    std::fs::write(&keep, b"img").unwrap();
    std::fs::write(workspace.join("IDENTITY.md"), b"skip").unwrap();
    std::fs::write(workspace.join("out.rs"), b"fn main(){}").unwrap();

    let db = ArtifactDb::new(memory_root.join("sessions/artifacts.db"))
        .await
        .unwrap();
    let gone = workspace.join("gone.txt");
    std::fs::write(&gone, b"x").unwrap();
    db.register(
        gone.to_str().unwrap(),
        ArtifactSource::AgentWrite,
        Some("s"),
        None,
        None,
    )
    .await
    .unwrap();
    std::fs::remove_file(&gone).unwrap();

    let report = db.reconcile(&memory_root).await.unwrap();
    assert_eq!(report.added, 2);
    assert_eq!(report.marked_missing, 1);

    let all = db.list(None, None, false, 100, true, None).await.unwrap();
    assert!(all.iter().any(|a| a.name == "photo.png" && !a.missing));
    assert!(all.iter().any(|a| a.name == "out.rs" && !a.missing));
    assert!(!all.iter().any(|a| a.name == "IDENTITY.md"));
    assert!(all.iter().any(|a| a.name == "gone.txt" && a.missing));
}

#[tokio::test]
async fn reconcile_registers_uploaded_memory_template() {
    let dir = tempdir().unwrap();
    let memory_root = dir.path().to_path_buf();
    let uploads = memory_root.join("uploads").join("sess-a");
    std::fs::create_dir_all(&uploads).unwrap();

    let memory_md = uploads.join("MEMORY.md");
    std::fs::write(&memory_md, b"# session memory").unwrap();

    let db = ArtifactDb::new(memory_root.join("sessions/artifacts.db"))
        .await
        .unwrap();
    let report = db.reconcile(&memory_root).await.unwrap();
    assert_eq!(report.added, 1);

    let row = db
        .get_by_path(memory_md.to_str().unwrap())
        .await
        .unwrap();
    let row = row.expect("uploads/sess-a/MEMORY.md should be registered");
    assert_eq!(row.name, "MEMORY.md");
    assert_eq!(row.source, "reconcile");
    assert!(!row.missing);
}

#[tokio::test]
async fn list_filters_by_agent_id() {
    let dir = tempfile::tempdir().unwrap();
    let db = ArtifactDb::new(dir.path().join("artifacts.db")).await.unwrap();
    let p1 = dir.path().join("a.md");
    let p2 = dir.path().join("b.md");
    std::fs::write(&p1, b"1").unwrap();
    std::fs::write(&p2, b"2").unwrap();

    db.register(
        p1.to_str().unwrap(),
        ArtifactSource::AgentWrite,
        Some("s1"),
        None,
        Some("default"),
    )
    .await
    .unwrap();
    db.register(
        p2.to_str().unwrap(),
        ArtifactSource::AgentWrite,
        Some("s2"),
        None,
        Some("coder"),
    )
    .await
    .unwrap();

    let rows = db
        .list(None, None, false, 50, false, Some("coder"))
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].agent_id, "coder");
}

#[tokio::test]
async fn re_register_without_agent_preserves_coder_ownership() {
    let dir = tempfile::tempdir().unwrap();
    let db = ArtifactDb::new(dir.path().join("artifacts.db")).await.unwrap();
    let path = dir.path().join("tool.rs");
    std::fs::write(&path, b"fn main() {}").unwrap();
    let path_str = path.to_str().unwrap();

    db.register(
        path_str,
        ArtifactSource::AgentWrite,
        None,
        None,
        Some("coder"),
    )
    .await
    .unwrap();
    db.register(
        path_str,
        ArtifactSource::AgentWrite,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    let rows = db
        .list(None, None, false, 50, false, Some("coder"))
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].agent_id, "coder");
}

#[tokio::test]
async fn remove_by_paths_deletes_rows() {
    let dir = tempdir().unwrap();
    let db = ArtifactDb::new(dir.path().join("artifacts.db")).await.unwrap();
    let p1 = dir.path().join("a.md");
    let p2 = dir.path().join("b.md");
    std::fs::write(&p1, b"1").unwrap();
    std::fs::write(&p2, b"2").unwrap();
    db.register(
        p1.to_str().unwrap(),
        ArtifactSource::AgentWrite,
        None,
        None,
        None,
    )
    .await
    .unwrap();
    db.register(
        p2.to_str().unwrap(),
        ArtifactSource::AgentWrite,
        None,
        None,
        None,
    )
    .await
    .unwrap();

    let n = db
        .remove_by_paths(&[p1.to_string_lossy().to_string()])
        .await
        .unwrap();
    assert_eq!(n, 1);
    let all = db.list(None, None, false, 50, true, None).await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].name, "b.md");
}
