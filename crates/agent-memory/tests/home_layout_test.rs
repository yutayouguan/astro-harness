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
    assert!(home::config_path(root).is_file());
    assert!(!root.join("skills/enabled.json").exists());
    assert!(!root.join("tools/enabled.json").exists());
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

#[test]
fn layout_then_settings_migration_preserves_preferences_and_allows_restart() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("tools")).unwrap();
    fs::create_dir_all(root.join("models")).unwrap();
    fs::write(root.join("tools/enabled.json"), r#"{"exec_command":false}"#).unwrap();
    fs::write(root.join("models/providers.json"), r#"{"providers":[]}"#).unwrap();
    fs::write(
        home::config_path(root),
        "# preserve\n[memory]\nwrite_approval = true # memory preference\n",
    )
    .unwrap();
    assert!(memory::ensure_workspace(root).is_err());
    home::settings::migration::migrate(root, true).unwrap();
    memory::ensure_workspace(root).unwrap();
    memory::ensure_workspace(root).unwrap();
    let gates: std::collections::HashMap<String, bool> =
        home::settings::read(root, &["desktop", "tools"])
            .unwrap()
            .unwrap();
    assert_eq!(gates.get("exec_command"), Some(&false));
    assert!(memory::load_memory_config(root).write_approval);
    assert!(fs::read_to_string(home::config_path(root))
        .unwrap()
        .contains("# memory preference"));
}

#[test]
fn memory_and_desktop_writes_share_a_lock_and_preserve_unrelated_sections() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::write(
        home::config_path(root),
        "# preserve\n[desktop.tools]\nexec_command = false # keep gate\n[custom]\npublished = 2026-09-09T00:00:00Z # keep timestamp\n[memory]\ncustom_date = 2026-09-09 # keep nested date\n",
    )
    .unwrap();
    std::thread::scope(|scope| {
        scope.spawn(|| memory::set_write_approval(root, true).unwrap());
        scope.spawn(|| {
            home::settings::write(
                root,
                &["desktop", "skills"],
                &std::collections::HashMap::from([("example", false)]),
            )
            .unwrap()
        });
    });
    let text = fs::read_to_string(home::config_path(root)).unwrap();
    assert!(text.contains("# keep gate"));
    assert!(text.contains("published = 2026-09-09T00:00:00Z # keep timestamp"));
    assert!(text.contains("custom_date = 2026-09-09 # keep nested date"));
    assert!(memory::load_memory_config(root).write_approval);
    let gates: std::collections::HashMap<String, bool> =
        home::settings::read(root, &["desktop", "tools"])
            .unwrap()
            .unwrap();
    assert_eq!(gates.get("exec_command"), Some(&false));
    let skills: std::collections::HashMap<String, bool> =
        home::settings::read(root, &["desktop", "skills"])
            .unwrap()
            .unwrap();
    assert_eq!(skills.get("example"), Some(&false));
}
