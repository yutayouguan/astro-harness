use super::*;
use std::fs;

fn fixture(root: &Path, id: u64) -> PathBuf {
    let key = format!("{id:064x}");
    let relative = PathBuf::from(format!("models/cache/astro-cache-v1-models-{key}.json"));
    let path = root.join(&relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::json!({"version":1,"domain":"models","key":key,"written_at":0,"payload":"PRIVATE_TEST_PAYLOAD"}).to_string()).unwrap();
    relative
}

#[test]
fn prepare_is_readonly_confirm_is_required_and_token_is_one_use() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let relative = fixture(root, 1);
    let before = fs::read(root.join(&relative)).unwrap();
    let service = CleanupService::default();
    let plan = service.prepare(root).unwrap();
    assert_eq!(plan.items.len(), 1);
    assert_eq!(plan.total_bytes, before.len() as u64);
    assert!(!serde_json::to_string(&plan)
        .unwrap()
        .contains("PRIVATE_TEST_PAYLOAD"));
    assert!(!root.join("security").exists());
    assert!(!root.join("backups").exists());
    assert_eq!(
        service.execute(root, &plan.token, false).unwrap_err(),
        CleanupError::ConfirmationRequired
    );
    assert!(!root.join("security").exists());
    let result = service.execute(root, &plan.token, true).unwrap();
    assert_eq!(result.moved_files, 1);
    assert_eq!(result.unverified_files, 0);
    assert!(result.manifest_complete);
    assert!(!root.join(&relative).exists());
    assert_eq!(
        fs::read(Path::new(&result.recovery_path).join("00.data")).unwrap(),
        before
    );
    let manifest =
        fs::read_to_string(Path::new(&result.recovery_path).join("manifest.json")).unwrap();
    assert!(!manifest.contains("PRIVATE_TEST_PAYLOAD"));
    assert!(manifest.contains("complete"));
    assert_eq!(
        recovery_path(root, &result.batch_id).unwrap(),
        PathBuf::from(&result.recovery_path)
    );
    assert!(recovery_path(root, "../../sessions").is_err());
    assert_eq!(
        service.execute(root, &plan.token, true).unwrap_err(),
        CleanupError::UnknownPlan
    );
    assert!(service.prepare(root).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&result.recovery_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

#[test]
fn changing_any_selected_file_aborts_before_the_first_move() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let first = fixture(root, 1);
    let second = fixture(root, 2);
    let service = CleanupService::default();
    let plan = service.prepare(root).unwrap();
    fs::write(root.join(&second), "changed").unwrap();
    assert_eq!(
        service.execute(root, &plan.token, true).unwrap_err(),
        CleanupError::Changed
    );
    assert!(root.join(first).exists());
    assert_eq!(fs::read_to_string(root.join(second)).unwrap(), "changed");
    assert!(!root.join("backups").exists());
}

#[test]
fn config_root_expiry_and_discard_invalidate_confirmation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let file = fixture(root, 1);
    let service = CleanupService::default();
    let plan = service.prepare(root).unwrap();
    fs::write(
        root.join("config.toml"),
        "[cache.models]\nttl_seconds=100\n",
    )
    .unwrap();
    assert_eq!(
        service.execute(root, &plan.token, true).unwrap_err(),
        CleanupError::Changed
    );
    let plan = service.prepare(root).unwrap();
    let other = tempfile::tempdir().unwrap();
    assert_eq!(
        service
            .execute(other.path(), &plan.token, true)
            .unwrap_err(),
        CleanupError::Changed
    );
    let plan = service.prepare(root).unwrap();
    service
        .plans
        .lock()
        .unwrap()
        .get_mut(&plan.token)
        .unwrap()
        .prepared = Instant::now() - LIFETIME - Duration::from_secs(1);
    assert_eq!(
        service.execute(root, &plan.token, true).unwrap_err(),
        CleanupError::Expired
    );
    let plan = service.prepare(root).unwrap();
    service.discard(&plan.token);
    assert_eq!(
        service.execute(root, &plan.token, true).unwrap_err(),
        CleanupError::UnknownPlan
    );
    assert!(root.join(file).exists());
    assert!(!root.join("backups").exists());
    let plan = service.prepare(root).unwrap();
    service
        .plans
        .lock()
        .unwrap()
        .get_mut(&plan.token)
        .unwrap()
        .public
        .expires_at_ms = 0;
    assert_eq!(
        service.execute(root, &plan.token, true).unwrap_err(),
        CleanupError::Expired
    );
}

#[test]
fn missing_roots_and_migration_markers_never_prepare_mutations() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let service = CleanupService::default();
    assert!(service.prepare(&root.join("missing")).is_err());
    assert!(!root.join("missing").exists());
    let file = fixture(root, 1);
    fs::create_dir(root.join("backups")).unwrap();
    fs::write(crate::extension_migration_marker(root), "{}").unwrap();
    assert_eq!(service.prepare(root).unwrap_err(), CleanupError::NotReady);
    assert!(root.join(file).exists());
    assert!(!root.join(RECOVERY_ROOT).exists());
}

#[test]
fn repository_roots_are_not_cleanup_homes() {
    let temp = tempfile::tempdir().unwrap();
    fixture(temp.path(), 1);
    fs::create_dir(temp.path().join(".git")).unwrap();
    assert_eq!(
        CleanupService::default().prepare(temp.path()).unwrap_err(),
        CleanupError::UnsafePath
    );
    assert!(!temp.path().join("backups").exists());
}

#[test]
fn cache_writer_lock_prevents_cleanup_without_losing_data() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let file = fixture(root, 1);
    let service = CleanupService::default();
    let plan = service.prepare(root).unwrap();
    let lock = fs::File::create(
        root.join("models/cache")
            .join(crate::cache::CACHE_LOCK_FILENAME),
    )
    .unwrap();
    lock.lock().unwrap();
    assert_eq!(
        service.execute(root, &plan.token, true).unwrap_err(),
        CleanupError::Busy
    );
    assert!(root.join(file).exists());
    assert!(!root.join("backups").exists());
}

#[test]
fn batch_limits_and_protected_files_are_explicit() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    for n in 0..MAX_BATCH + 5 {
        fixture(root, n as u64);
    }
    fs::write(root.join("models/cache/unrelated.json"), "user owned").unwrap();
    fs::create_dir(root.join("sessions")).unwrap();
    fs::write(root.join("sessions/state.db"), "not a real db").unwrap();
    let service = CleanupService::default();
    let plan = service.prepare(root).unwrap();
    assert_eq!(plan.items.len(), MAX_BATCH);
    assert!(plan.omitted_files);
    let result = service.execute(root, &plan.token, true).unwrap();
    assert_eq!(result.moved_files, MAX_BATCH);
    assert_eq!(
        fs::read_to_string(root.join("models/cache/unrelated.json")).unwrap(),
        "user owned"
    );
    assert_eq!(
        fs::read_to_string(root.join("sessions/state.db")).unwrap(),
        "not a real db"
    );
    assert_eq!(service.prepare(root).unwrap().items.len(), 5);
}

#[test]
fn old_runtime_logs_move_but_security_audits_do_not() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("logs")).unwrap();
    let log = root.join("logs/agent.log.2020-01-01");
    fs::write(&log, "old runtime log").unwrap();
    fs::File::options()
        .write(true)
        .open(&log)
        .unwrap()
        .set_times(
            fs::FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(31 * 86400)),
        )
        .unwrap();
    fs::create_dir_all(root.join("security/audit")).unwrap();
    fs::write(root.join("security/audit/events.jsonl"), "keep audit").unwrap();
    let service = CleanupService::default();
    let plan = service.prepare(root).unwrap();
    assert_eq!(plan.items.len(), 1);
    assert_eq!(plan.items[0].policy, "logs_30_days");
    let result = service.execute(root, &plan.token, true).unwrap();
    assert_eq!(result.moved_files, 1);
    assert_eq!(
        fs::read_to_string(root.join("security/audit/events.jsonl")).unwrap(),
        "keep audit"
    );
}

#[cfg(unix)]
#[test]
fn symlinked_parent_leaf_and_recovery_directory_cannot_escape() {
    for attack in ["parent", "leaf", "recovery", "lock"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let file = fixture(root, 1);
        let service = CleanupService::default();
        let plan = service.prepare(root).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let secret = outside.path().join("secret");
        fs::write(&secret, "outside value").unwrap();
        match attack {
            "parent" => {
                fs::rename(root.join("models/cache"), root.join("old-cache")).unwrap();
                std::os::unix::fs::symlink(outside.path(), root.join("models/cache")).unwrap();
            }
            "leaf" => {
                fs::remove_file(root.join(&file)).unwrap();
                std::os::unix::fs::symlink(&secret, root.join(&file)).unwrap();
            }
            "lock" => {
                std::os::unix::fs::symlink(outside.path(), root.join("security")).unwrap();
            }
            _ => {
                fs::create_dir(root.join("backups")).unwrap();
                std::os::unix::fs::symlink(outside.path(), root.join(RECOVERY_ROOT)).unwrap();
            }
        }
        assert!(
            service.execute(root, &plan.token, true).is_err(),
            "accepted {attack}"
        );
        assert_eq!(fs::read_to_string(&secret).unwrap(), "outside value");
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1);
    }
}

#[test]
fn replaced_file_during_move_is_kept_for_recovery_not_deleted() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let relative = fixture(root, 1);
    let service = CleanupService::default();
    let public = service.prepare(root).unwrap();
    let plan = service.plans.lock().unwrap().remove(&public.token).unwrap();
    let (doc, _) = configuration(&plan.root).unwrap();
    let result = move_to_recovery_with(&plan, &doc, |_| {
        fs::write(root.join(&relative), "concurrent replacement kept").unwrap();
    })
    .unwrap();
    assert_eq!(result.unverified_files, 1);
    assert_eq!(result.moved_files, 0);
    assert_eq!(result.outcomes[0].status, "changed_in_recovery");
    assert_eq!(
        fs::read_to_string(Path::new(&result.recovery_path).join("00.data")).unwrap(),
        "concurrent replacement kept"
    );
}

#[cfg(unix)]
#[test]
fn a_parent_swapped_after_validation_does_not_redirect_the_move() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let relative = fixture(root, 1);
    let service = CleanupService::default();
    let public = service.prepare(root).unwrap();
    let plan = service.plans.lock().unwrap().remove(&public.token).unwrap();
    let (doc, _) = configuration(&plan.root).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let external_file = outside.path().join(relative.file_name().unwrap());
    fs::write(&external_file, "untouched").unwrap();
    let result = move_to_recovery_with(&plan, &doc, |_| {
        fs::rename(root.join("models/cache"), root.join("old-cache")).unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("models/cache")).unwrap();
    })
    .unwrap();
    assert_eq!(result.moved_files, 1);
    assert_eq!(fs::read_to_string(external_file).unwrap(), "untouched");
}

#[cfg(unix)]
#[test]
fn fifo_replacement_cannot_block_the_cleanup_worker() {
    use std::os::unix::ffi::OsStrExt;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let file = fixture(root, 1);
    let service = CleanupService::default();
    let plan = service.prepare(root).unwrap();
    let path = root.join(&file);
    fs::remove_file(&path).unwrap();
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: this is a NUL-terminated path inside the test's own temporary root.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    let root = root.to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        tx.send(service.execute(&root, &plan.token, true).is_err())
            .unwrap();
    });
    assert!(rx
        .recv_timeout(Duration::from_secs(2))
        .expect("worker blocked on a FIFO"));
    worker.join().unwrap();
    assert!(!temp.path().join("backups").exists());
}

#[test]
fn external_config_change_stops_the_remaining_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fixture(root, 1);
    let second = fixture(root, 2);
    let service = CleanupService::default();
    let public = service.prepare(root).unwrap();
    let plan = service.plans.lock().unwrap().remove(&public.token).unwrap();
    let (doc, _) = configuration(&plan.root).unwrap();
    let result = move_to_recovery_with(&plan, &doc, |index| {
        if index == 0 {
            fs::write(root.join("config.toml"), "[changed]\nvalue=true\n").unwrap();
        }
    })
    .unwrap();
    assert_eq!(result.moved_files, 1);
    assert_eq!(result.outcomes[1].status, "changed_not_moved");
    assert!(root.join(second).exists());
}
