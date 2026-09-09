//! Approval hashes are security state, never executable configuration or cache.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, io::Write, path::Path};

#[derive(Default, Deserialize, Serialize)]
struct TrustFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    hashes: HashMap<String, String>,
}

pub(crate) fn canonical_key(key: &str) -> String {
    let parts: Vec<_> = key.rsplitn(4, ':').collect();
    if parts.len() != 4 || !Path::new(parts[3]).is_absolute() {
        return key.into();
    }
    let source = Path::new(parts[3]);
    let canonical = source.canonicalize().ok().or_else(|| {
        Some(
            source
                .parent()?
                .canonicalize()
                .ok()?
                .join(source.file_name()?),
        )
    });
    canonical
        .map(|path| format!("{}:{}:{}:{}", path.display(), parts[2], parts[1], parts[0]))
        .unwrap_or_else(|| key.into())
}

pub fn load(base: &Path) -> Result<HashMap<String, String>> {
    let path = home::hook_trust_path(base);
    if !path.try_exists()? {
        return Ok(HashMap::new());
    }
    ensure!(
        !path.symlink_metadata()?.file_type().is_symlink(),
        "hook trust file must not be a symlink"
    );
    ensure!(
        path.metadata()?.len() <= 4 * 1024 * 1024,
        "hook trust file exceeds limit"
    );
    let file: TrustFile = serde_json::from_slice(&fs::read(path)?)
        .map_err(|_| anyhow::anyhow!("invalid hook trust state"))?;
    ensure!(file.version == 1, "unsupported hook trust state version");
    let mut hashes = HashMap::new();
    for (key, value) in file.hashes {
        validate_hash(&value)?;
        if let Some(old) = hashes.insert(canonical_key(&key), value.clone()) {
            ensure!(old == value, "conflicting hook trust state");
        }
    }
    Ok(hashes)
}

/// Only explicit approval/migration flows may write trust records. A caller
/// must still validate the source's project trust and the current content hash.
pub fn record(base: &Path, key: &str, hash: &str) -> Result<()> {
    validate_hash(hash)?;
    let path = home::hook_trust_path(base);
    let _guard = home::config_file::lock_config_file(&path)?;
    let mut hashes = load(base)?;
    hashes.insert(canonical_key(key), hash.into());
    let bytes = serde_json::to_vec(&TrustFile { version: 1, hashes })?;
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}

pub(crate) fn validate_hash(hash: &str) -> Result<()> {
    ensure!(
        hash.starts_with("sha256:")
            && hash.len() == 71
            && hash[7..].chars().all(|c| c.is_ascii_hexdigit()),
        "invalid hook trust digest"
    );
    Ok(())
}
