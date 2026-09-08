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
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
    let result = (|| -> anyhow::Result<()> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
