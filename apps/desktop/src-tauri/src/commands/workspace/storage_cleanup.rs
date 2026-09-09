//! UI-only, explicitly confirmed recovery moves. No arbitrary source paths.
use std::sync::LazyLock;

static CLEANUP: LazyLock<home::storage_cleanup::CleanupService> = LazyLock::new(Default::default);

#[tauri::command]
pub async fn prepare_storage_cleanup() -> Result<home::storage_cleanup::CleanupPlan, String> {
    let root = home::default_memory_dir();
    tokio::task::spawn_blocking(move || CLEANUP.prepare(&root))
        .await
        .map_err(|_| "cleanup_unavailable".to_string())?
        .map_err(|error| error.code().into())
}

#[tauri::command]
pub async fn execute_storage_cleanup(
    token: String,
    confirmed: bool,
) -> Result<home::storage_cleanup::CleanupResult, String> {
    let root = home::default_memory_dir();
    tokio::task::spawn_blocking(move || CLEANUP.execute(&root, &token, confirmed))
        .await
        .map_err(|_| "cleanup_unavailable".to_string())?
        .map_err(|error| error.code().into())
}

#[tauri::command]
pub fn discard_storage_cleanup(token: String) {
    CLEANUP.discard(&token);
}

#[tauri::command]
pub async fn reveal_storage_recovery(batch_id: String) -> Result<(), String> {
    let root = home::default_memory_dir();
    let path =
        tokio::task::spawn_blocking(move || home::storage_cleanup::recovery_path(&root, &batch_id))
            .await
            .map_err(|_| "cleanup_unavailable".to_string())?
            .map_err(|error| error.code().to_string())?;
    super::files::reveal_in_folder(path.to_string_lossy().into_owned()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(flavor = "current_thread")]
    async fn native_cleanup_requires_confirmation_and_only_moves_the_test_fixture() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let _env = home::test_env::AstroMemoryDirGuard::set(root);
        let key = home::cache::fingerprint(&"native-storage-test").unwrap();
        let directory = home::models_cache_dir(root);
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join(format!("astro-cache-v1-models-{key}.json"));
        std::fs::write(&file, serde_json::json!({"version":1,"domain":"models","key":key,"written_at":0,"payload":"test data"}).to_string()).unwrap();
        let plan = prepare_storage_cleanup().await.unwrap();
        assert_eq!(
            std::path::PathBuf::from(&plan.root_path),
            root.canonicalize().unwrap()
        );
        assert_eq!(plan.items.len(), 1);
        assert!(!root.join("backups").exists());
        assert_eq!(
            execute_storage_cleanup(plan.token.clone(), false)
                .await
                .unwrap_err(),
            "cleanup_confirmation_required"
        );
        let result = execute_storage_cleanup(plan.token.clone(), true)
            .await
            .unwrap();
        assert_eq!(result.moved_files, 1);
        assert!(!file.exists());
        assert!(std::path::Path::new(&result.recovery_path)
            .join("manifest.json")
            .is_file());
        assert!(execute_storage_cleanup(plan.token, true).await.is_err());
    }
}
