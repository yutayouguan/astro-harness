//! Astro Agent 桌面二进制入口。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // QA bundles can be reopened by LaunchServices without the original process environment.
    // Bind isolation in the dedicated debug binary, before logging or credential hydration.
    #[cfg(debug_assertions)]
    if let Some(root) = option_env!("ASTRO_NATIVE_ACCEPTANCE_ROOT") {
        let Some(path) = native_acceptance_root(std::path::Path::new(root)) else {
            eprintln!("Refusing to start a QA binary without its isolated data root");
            std::process::exit(2);
        };
        std::env::set_var("ASTRO_MEMORY_DIR", path);
        std::env::set_var("ASTRO_ENV_HYDRATE", "disabled");
    }
    astro_agent_lib::run();
}

#[cfg(debug_assertions)]
fn native_acceptance_root(path: &std::path::Path) -> Option<std::path::PathBuf> {
    if !path.is_absolute() || !path.symlink_metadata().ok()?.is_dir() {
        return None;
    }
    let path = path.canonicalize().ok()?;
    if path.file_name()? != "home"
        || !path
            .parent()?
            .file_name()?
            .to_string_lossy()
            .starts_with("astro-config-native-")
    {
        return None;
    }
    let marker = path.join(".native-acceptance-root");
    let metadata = marker.symlink_metadata().ok()?;
    if !metadata.is_file() || metadata.len() != b"astro-config-native-v1\n".len() as u64 {
        return None;
    }
    (std::fs::read(marker).ok()? == b"astro-config-native-v1\n").then_some(path)
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;

    #[test]
    fn native_acceptance_requires_the_private_marker() {
        let dir = tempfile::Builder::new()
            .prefix("astro-config-native-")
            .tempdir()
            .unwrap();
        let root = dir.path().join("home");
        std::fs::create_dir(&root).unwrap();
        assert!(native_acceptance_root(&root).is_none());
        std::fs::write(root.join(".native-acceptance-root"), "wrong marker\n").unwrap();
        assert!(native_acceptance_root(&root).is_none());
        std::fs::write(
            root.join(".native-acceptance-root"),
            "astro-config-native-v1\n",
        )
        .unwrap();
        assert_eq!(
            native_acceptance_root(&root),
            Some(root.canonicalize().unwrap())
        );
        assert!(native_acceptance_root(dir.path()).is_none());
        assert!(native_acceptance_root(std::path::Path::new("home")).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn native_acceptance_rejects_symlink_roots_and_markers() {
        let dir = tempfile::Builder::new()
            .prefix("astro-config-native-")
            .tempdir()
            .unwrap();
        let actual = dir.path().join("actual");
        std::fs::create_dir(&actual).unwrap();
        let root = dir.path().join("home");
        std::os::unix::fs::symlink(&actual, &root).unwrap();
        assert!(native_acceptance_root(&root).is_none());
        std::fs::remove_file(&root).unwrap();
        std::fs::create_dir(&root).unwrap();
        let marker = dir.path().join("marker");
        std::fs::write(&marker, "astro-config-native-v1\n").unwrap();
        std::os::unix::fs::symlink(&marker, root.join(".native-acceptance-root")).unwrap();
        assert!(native_acceptance_root(&root).is_none());
    }
}
