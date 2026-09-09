//! Bounded, owned cache files. Cache contents never grant tool authority.
use anyhow::{ensure, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy)]
pub enum Domain {
    Models,
    Mcp,
}
impl Domain {
    fn key(self) -> &'static str {
        match self {
            Self::Models => "models",
            Self::Mcp => "mcp",
        }
    }
}
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    pub enabled: bool,
    pub directory: Option<PathBuf>,
    pub ttl_seconds: u64,
    pub max_size_mb: u64,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            enabled: true,
            directory: None,
            ttl_seconds: 600,
            max_size_mb: 256,
        }
    }
}
pub fn policy(base: &Path, domain: Domain) -> Result<Policy> {
    let doc = crate::settings::read_document(&crate::config_path(base))?;
    let policy: Policy = crate::settings::get(&doc, &["cache", domain.key()])?.unwrap_or_default();
    ensure!(
        (1..=2_592_000).contains(&policy.ttl_seconds),
        "cache TTL must be 1..2592000 seconds"
    );
    ensure!(
        (1..=10240).contains(&policy.max_size_mb),
        "cache size must be 1..10240 MiB"
    );
    Ok(policy)
}
fn canonical_future(path: &Path) -> Result<PathBuf> {
    if path.try_exists()? {
        return Ok(path.canonicalize()?);
    }
    let parent = path.parent().context("invalid cache path")?;
    let name = path.file_name().context("invalid cache path component")?;
    Ok(canonical_future(parent)?.join(name))
}
pub fn directory(base: &Path, domain: Domain, policy: &Policy) -> Result<PathBuf> {
    let default = match domain {
        Domain::Models => crate::models_cache_dir(base),
        Domain::Mcp => crate::mcp_cache_dir(base),
    };
    let requested = policy
        .directory
        .as_ref()
        .map(|p| {
            if p.is_absolute() {
                p.clone()
            } else {
                base.join(p)
            }
        })
        .unwrap_or(default);
    let dir = canonical_future(&requested)?;
    let root = canonical_future(base)?;
    if policy.directory.as_ref().is_some_and(|p| p.is_relative()) {
        ensure!(
            dir.starts_with(&root),
            "relative cache directory escapes Astro home"
        );
    }
    ensure!(
        dir != root && dir.parent().is_some(),
        "cache directory must not be a storage root"
    );
    if let Some(user) = crate::user_home_dir() {
        ensure!(
            dir != canonical_future(&user)?,
            "cache directory must not be the user home"
        );
    }
    for durable in [
        "sessions",
        "agents",
        "security",
        "browser",
        "workspace",
        "artifacts",
        "usage",
        "backups",
    ] {
        ensure!(
            !dir.starts_with(root.join(durable)),
            "cache directory overlaps durable data"
        );
    }
    Ok(dir)
}
pub fn fingerprint(value: &impl Serialize) -> Result<String> {
    fn sorted(value: serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => serde_json::Value::Object(
                map.into_iter()
                    .map(|(k, v)| (k, sorted(v)))
                    .collect::<std::collections::BTreeMap<_, _>>()
                    .into_iter()
                    .collect(),
            ),
            serde_json::Value::Array(values) => {
                serde_json::Value::Array(values.into_iter().map(sorted).collect())
            }
            value => value,
        }
    }
    let bytes = serde_json::to_vec(&sorted(serde_json::to_value(value)?))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
#[derive(Serialize, Deserialize)]
struct Entry {
    version: u32,
    domain: String,
    key: String,
    written_at: u64,
    payload: serde_json::Value,
}
fn now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}
fn filename(domain: Domain, key: &str) -> String {
    format!("astro-cache-v1-{}-{key}.json", domain.key())
}
fn read_entry(path: &Path, max_bytes: u64) -> Result<Option<Entry>> {
    let meta = match path.symlink_metadata() {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink(),
        "invalid cache file"
    );
    ensure!(meta.len() <= max_bytes, "cache file exceeds budget");
    let bytes = fs::read(path)?;
    ensure!(bytes.len() as u64 <= max_bytes, "cache file exceeds budget");
    Ok(serde_json::from_slice(&bytes).ok())
}
pub fn read<T: DeserializeOwned>(
    base: &Path,
    domain: Domain,
    identity: &impl Serialize,
) -> Result<Option<T>> {
    let policy = policy(base, domain)?;
    if !policy.enabled {
        return Ok(None);
    }
    let key = fingerprint(identity)?;
    let path = directory(base, domain, &policy)?.join(filename(domain, &key));
    let Some(entry) = read_entry(&path, policy.max_size_mb * 1024 * 1024)? else {
        return Ok(None);
    };
    let now = now()?;
    if entry.version != 1
        || entry.domain != domain.key()
        || entry.key != key
        || entry.written_at > now
        || now - entry.written_at > policy.ttl_seconds
    {
        return Ok(None);
    }
    Ok(serde_json::from_value(entry.payload).ok())
}

struct CacheLock(File);
impl Drop for CacheLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
pub fn write(
    base: &Path,
    domain: Domain,
    identity: &impl Serialize,
    payload: &impl Serialize,
) -> Result<bool> {
    let policy = policy(base, domain)?;
    if !policy.enabled {
        return Ok(false);
    }
    let key = fingerprint(identity)?;
    let entry = Entry {
        version: 1,
        domain: domain.key().into(),
        key: key.clone(),
        written_at: now()?,
        payload: serde_json::to_value(payload)?,
    };
    let bytes = serde_json::to_vec(&entry)?;
    let budget = policy.max_size_mb * 1024 * 1024;
    if bytes.len() as u64 > budget {
        return Ok(false);
    }
    let dir = directory(base, domain, &policy)?;
    fs::create_dir_all(&dir)?;
    let lock_path = dir.join(".astro-cache.lock");
    ensure!(
        !lock_path
            .symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink()),
        "cache lock is a symlink"
    );
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    lock.lock()?;
    let _guard = CacheLock(lock);
    let target = dir.join(filename(domain, &key));
    let mut owned = Vec::new();
    for (index, file) in fs::read_dir(&dir)?.enumerate() {
        ensure!(index < 4096, "cache directory contains too many files");
        let file = file?;
        let path = file.path();
        if !file
            .file_name()
            .to_string_lossy()
            .starts_with(&format!("astro-cache-v1-{}-", domain.key()))
        {
            continue;
        }
        if let Ok(Some(existing)) = read_entry(&path, budget) {
            if existing.version == 1
                && existing.domain == domain.key()
                && file.file_name().to_string_lossy() == filename(domain, &existing.key)
            {
                if path != target {
                    owned.push((existing.written_at, path, file.metadata()?.len()));
                }
            } else {
                ensure!(path != target, "cache target is not owned by Astro");
            }
        } else {
            ensure!(
                path != target,
                "refusing to overwrite an invalid cache target"
            );
        }
    }
    owned.sort_by_key(|entry| entry.0);
    let mut size = owned.iter().map(|entry| entry.2).sum::<u64>() + bytes.len() as u64;
    for (_, path, len) in owned {
        if size <= budget {
            break;
        }
        fs::remove_file(path)?;
        size = size.saturating_sub(len);
    }
    let mut file = tempfile::NamedTempFile::new_in(&dir)?;
    file.write_all(&bytes)?;
    file.as_file().sync_all()?;
    file.persist(target).map_err(|e| e.error)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ttl_identity_capacity_and_disabled_policy() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            crate::config_path(dir.path()),
            "[cache.mcp]\nttl_seconds=2\nmax_size_mb=1\n",
        )
        .unwrap();
        let payload = "x".repeat(700_000);
        write(dir.path(), Domain::Mcp, &"first", &payload).unwrap();
        let cache_dir = directory(
            dir.path(),
            Domain::Mcp,
            &policy(dir.path(), Domain::Mcp).unwrap(),
        )
        .unwrap();
        fs::write(cache_dir.join("unrelated.json"), "do not evict").unwrap();
        write(dir.path(), Domain::Mcp, &"second", &payload).unwrap();
        assert!(read::<String>(dir.path(), Domain::Mcp, &"first")
            .unwrap()
            .is_none());
        assert_eq!(
            fs::read_to_string(cache_dir.join("unrelated.json")).unwrap(),
            "do not evict"
        );
        let path = cache_dir.join(filename(Domain::Mcp, &fingerprint(&"second").unwrap()));
        let mut entry = read_entry(&path, 1024 * 1024).unwrap().unwrap();
        entry.written_at = 0;
        fs::write(&path, serde_json::to_vec(&entry).unwrap()).unwrap();
        assert!(read::<String>(dir.path(), Domain::Mcp, &"second")
            .unwrap()
            .is_none());
        fs::write(
            crate::config_path(dir.path()),
            "[cache.mcp]\nenabled=false\n",
        )
        .unwrap();
        assert!(!write(dir.path(), Domain::Mcp, &"disabled", &payload).unwrap());
    }
    #[test]
    fn cache_is_lazy_bounded_and_does_not_modify_configuration() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read::<Vec<String>>(dir.path(), Domain::Mcp, &"id")
            .unwrap()
            .is_none());
        assert!(!dir.path().join("mcp").exists());
        let data = vec!["tool".to_string()];
        write(dir.path(), Domain::Mcp, &"id", &data).unwrap();
        assert_eq!(
            read::<Vec<String>>(dir.path(), Domain::Mcp, &"id").unwrap(),
            Some(data)
        );
        assert!(!crate::config_path(dir.path()).exists());
        assert!(directory(
            dir.path(),
            Domain::Mcp,
            &Policy {
                directory: Some("sessions/cache".into()),
                ..Default::default()
            }
        )
        .is_err());
    }
}
