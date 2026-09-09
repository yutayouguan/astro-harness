//! Cross-process serialization for writers of the shared TOML configuration.
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

pub struct ConfigWriteGuard(File);

impl Drop for ConfigWriteGuard {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

pub fn lock_config_file(path: &Path) -> anyhow::Result<ConfigWriteGuard> {
    let base = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("config has no parent"))?;
    let locks = base.join("security/locks");
    fs::create_dir_all(&locks)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(locks.join("config.lock"))?;
    file.lock()?;
    Ok(ConfigWriteGuard(file))
}

/// Call while holding `lock_config_file`, including the preceding read/modify phase.
pub fn write_config_file(path: &Path, text: &str) -> anyhow::Result<()> {
    if let Ok(metadata) = path.symlink_metadata() {
        anyhow::ensure!(
            !metadata.file_type().is_symlink(),
            "refusing to replace symlink configuration: {}",
            path.display()
        );
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("config has no parent"))?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(text.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}
