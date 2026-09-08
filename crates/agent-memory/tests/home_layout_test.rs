use std::fs;

#[tokio::test(flavor = "current_thread")]
async fn fresh_bootstrap_and_restart_preserve_domain_data() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    memory::ensure_workspace(root).unwrap();
    let store = session::SessionStore::open(&home::session_db_path(root))
        .await
        .unwrap();
    store
        .create_session("layout-restart", "test", None, None, None)
        .await
        .unwrap();
    let soul = home::agent_workspace_dir(root, home::DEFAULT_AGENT_ID).join("SOUL.md");
    fs::write(&soul, "personalized, must not be replaced").unwrap();
    memory::set_write_approval(root, true).unwrap();
    drop(store);

    memory::ensure_workspace(root).unwrap();
    let reopened = session::SessionStore::open(&home::session_db_path(root))
        .await
        .unwrap();
    assert!(reopened
        .get_session("layout-restart")
        .await
        .unwrap()
        .is_some());
    assert!(memory::load_memory_config(root).write_approval);
    assert_eq!(
        fs::read_to_string(soul).unwrap(),
        "personalized, must not be replaced"
    );
    assert!(home::skills_enabled_path(root).is_file());
    assert!(home::global_tools_path(root).is_file());
    assert!(home::cron_dir(root).join("jobs.json").is_file());
    for old in [
        "data",
        "cache",
        "cron",
        "workflows",
        "uploads",
        "learning",
        "audit",
        "config.yaml",
        "providers.json",
        "models.json",
        "tools-enabled.json",
        "skills-enabled.json",
    ] {
        assert!(!root.join(old).exists(), "bootstrap recreated {old}");
    }
}

#[test]
fn bootstrap_refuses_old_automation_or_an_incomplete_migration() {
    for old in ["cron", "workflows", "data", "cache"] {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join(old)).unwrap();
        assert!(memory::ensure_workspace(temp.path()).is_err());
        assert!(!home::session_db_path(temp.path()).exists());
    }
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("backups")).unwrap();
    fs::write(temp.path().join("backups/layout-in-progress.json"), "{}").unwrap();
    assert!(memory::ensure_workspace(temp.path()).is_err());
    assert!(!home::session_db_path(temp.path()).exists());
}
