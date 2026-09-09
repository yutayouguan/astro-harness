//! Bounded-size write probe for an explicitly selected onboarding directory.
use serde::Serialize;
use std::{fs, io::Write, path::Path};

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceStatus {
    Ready,
    Missing,
    NotDirectory,
    NotWritable,
    Unavailable,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceCheck {
    pub path: String,
    pub status: WorkspaceStatus,
}

pub(super) fn inspect_workspace(path: &Path) -> WorkspaceCheck {
    let status = if !path.is_absolute() {
        WorkspaceStatus::Unavailable
    } else {
        match fs::metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => WorkspaceStatus::Missing,
            Err(_) => WorkspaceStatus::Unavailable,
            Ok(metadata) if !metadata.is_dir() => WorkspaceStatus::NotDirectory,
            Ok(metadata) if metadata.permissions().readonly() => WorkspaceStatus::NotWritable,
            Ok(_) if fs::read_dir(path).is_err() => WorkspaceStatus::Unavailable,
            Ok(_) => probe_write(path),
        }
    };
    WorkspaceCheck {
        path: path.to_string_lossy().into_owned(),
        status,
    }
}

fn probe_write(directory: &Path) -> WorkspaceStatus {
    let probe = directory.join(format!(".astro-write-check-{}", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let Ok(mut file) = options.open(&probe) else {
        return WorkspaceStatus::NotWritable;
    };
    let written = file
        .write_all(b"astro workspace check")
        .and_then(|()| file.sync_all());
    drop(file);
    // Only remove the file this invocation successfully created.
    if fs::remove_file(&probe).is_err() {
        return WorkspaceStatus::Unavailable;
    }
    if written.is_ok() {
        WorkspaceStatus::Ready
    } else {
        WorkspaceStatus::NotWritable
    }
}

#[tauri::command]
pub async fn check_onboarding_workspace(path: String) -> Result<WorkspaceCheck, String> {
    tauri::async_runtime::spawn_blocking(move || inspect_workspace(Path::new(&path)))
        .await
        .map_err(|_| "Workspace check unavailable".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn writable_probe_leaves_existing_files_untouched() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("keep.txt"), b"unchanged").unwrap();
        assert_eq!(
            inspect_workspace(root.path()).status,
            WorkspaceStatus::Ready
        );
        assert_eq!(
            fs::read(root.path().join("keep.txt")).unwrap(),
            b"unchanged"
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
    #[test]
    fn missing_and_file_paths_are_not_created_or_replaced() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("missing");
        assert_eq!(inspect_workspace(&missing).status, WorkspaceStatus::Missing);
        assert!(!missing.exists());
        let file = root.path().join("file");
        fs::write(&file, b"keep").unwrap();
        assert_eq!(
            inspect_workspace(&file).status,
            WorkspaceStatus::NotDirectory
        );
        assert_eq!(
            inspect_workspace(Path::new("relative")).status,
            WorkspaceStatus::Unavailable
        );
    }
    #[cfg(unix)]
    #[test]
    fn readonly_directory_is_rejected() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o500)).unwrap();
        let status = inspect_workspace(root.path()).status;
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(status, WorkspaceStatus::NotWritable);
    }
}
