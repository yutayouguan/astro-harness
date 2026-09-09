//! All mutation is relative to pinned directory handles, never ambient source paths.
use super::{CleanupError as Error, Result};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

pub struct Root {
    pub path: PathBuf,
    pub dir: Dir,
}
impl Root {
    pub fn open(base: &Path) -> Result<Self> {
        let path = std::path::absolute(base).map_err(|_| Error::Unavailable)?;
        let parent = path
            .parent()
            .ok_or(Error::UnsafePath)?
            .canonicalize()
            .map_err(|_| Error::Unavailable)?;
        let name = path.file_name().ok_or(Error::UnsafePath)?;
        let parent_dir = Dir::open_ambient_dir(&parent, cap_std::ambient_authority())
            .map_err(|_| Error::UnsafePath)?;
        let dir = parent_dir
            .open_dir_nofollow(name)
            .map_err(|_| Error::UnsafePath)?;
        let path = parent.join(name);
        if crate::user_home_dir()
            .and_then(|home| home.canonicalize().ok())
            .as_ref()
            == Some(&path)
            || dir.symlink_metadata(".git").is_ok()
        {
            return Err(Error::UnsafePath);
        }
        Ok(Self { path, dir })
    }
}

pub fn same_dir(a: &Dir, b: &Dir) -> Result<bool> {
    let handle = |dir: &Dir| same_file::Handle::from_file(dir.try_clone()?.into_std_file());
    Ok(handle(a).map_err(|_| Error::Unavailable)? == handle(b).map_err(|_| Error::Unavailable)?)
}

pub fn walk(root: &Dir, relative: &Path, create: bool) -> Result<Dir> {
    let mut dir = root.try_clone().map_err(|_| Error::Unavailable)?;
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(Error::UnsafePath);
        };
        match dir.open_dir_nofollow(name) {
            Ok(next) => dir = next,
            Err(error) if create && error.kind() == std::io::ErrorKind::NotFound => {
                dir.create_dir(name).map_err(|_| Error::Unavailable)?;
                dir = dir.open_dir_nofollow(name).map_err(|_| Error::UnsafePath)?;
                private_directory(&dir)?;
            }
            Err(_) => return Err(Error::UnsafePath),
        }
    }
    Ok(dir)
}

/// A missing control path is different from a symlink or unreadable path.
pub fn metadata(root: &Dir, relative: &Path) -> Result<Option<cap_std::fs::Metadata>> {
    let mut dir = root.try_clone().map_err(|_| Error::Unavailable)?;
    let components: Vec<_> = relative.components().collect();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return Err(Error::UnsafePath);
        };
        if index + 1 == components.len() {
            return match dir.symlink_metadata(name) {
                Ok(meta) => Ok(Some(meta)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(_) => Err(Error::UnsafePath),
            };
        }
        dir = match dir.open_dir_nofollow(name) {
            Ok(next) => next,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(Error::UnsafePath),
        };
    }
    Err(Error::UnsafePath)
}

pub fn private_directory(dir: &Dir) -> Result<()> {
    #[cfg(unix)]
    {
        use cap_std::fs::PermissionsExt;
        dir.set_permissions(".", cap_std::fs::Permissions::from_mode(0o700))
            .map_err(|_| Error::Unavailable)?;
    }
    let _ = dir;
    Ok(())
}

pub struct Snapshot {
    id: same_file::Handle,
    pub bytes: u64,
    modified: SystemTime,
    pub digest: String,
}
impl Snapshot {
    pub fn matches(&self, other: &Self) -> bool {
        self.id == other.id
            && self.bytes == other.bytes
            && self.modified == other.modified
            && self.digest == other.digest
    }
}

pub fn open_file(dir: &Dir, name: &std::ffi::OsStr) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = dir
        .open_with(name, &options)
        .map_err(|_| Error::Changed)?
        .into_std();
    if !file.metadata().map_err(|_| Error::Unavailable)?.is_file() {
        return Err(Error::UnsafePath);
    }
    Ok(file)
}

pub fn read_file(
    dir: &Dir,
    name: &std::ffi::OsStr,
    cap: u64,
    budget: &mut u64,
) -> Result<(Snapshot, Vec<u8>, std::fs::Metadata)> {
    let file = open_file(dir, name)?;
    let meta = file.metadata().map_err(|_| Error::Unavailable)?;
    let charge = meta.len().checked_add(1).ok_or(Error::Budget)?;
    if meta.len() > cap || charge > *budget {
        return Err(Error::Budget);
    }
    *budget -= charge;
    let mut bytes = Vec::new();
    file.try_clone()
        .map_err(|_| Error::Unavailable)?
        .take(charge)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Unavailable)?;
    let after = file.metadata().map_err(|_| Error::Unavailable)?;
    let modified = meta.modified().map_err(|_| Error::Unavailable)?;
    if bytes.len() as u64 != meta.len()
        || after.len() != meta.len()
        || after.modified().ok() != Some(modified)
    {
        return Err(Error::Changed);
    }
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let id = same_file::Handle::from_file(file).map_err(|_| Error::Unavailable)?;
    Ok((
        Snapshot {
            id,
            bytes: meta.len(),
            modified,
            digest,
        },
        bytes,
        meta,
    ))
}

pub struct Lock(File);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
pub fn try_lock(dir: &Dir, name: &str) -> Result<Lock> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = dir
        .open_with(name, &options)
        .map_err(|_| Error::Busy)?
        .into_std();
    if !file.metadata().map_err(|_| Error::Busy)?.is_file() {
        return Err(Error::UnsafePath);
    }
    file.try_lock().map_err(|_| Error::Busy)?;
    Ok(Lock(file))
}

pub fn sync_directory(dir: &Dir) -> Result<()> {
    #[cfg(unix)]
    {
        dir.try_clone()
            .map_err(|_| Error::Unavailable)?
            .into_std_file()
            .sync_all()
            .map_err(|_| Error::Unavailable)?;
    }
    let _ = dir;
    Ok(())
}

pub fn write_manifest(dir: &Dir, bytes: &[u8]) -> Result<()> {
    let temporary = format!("manifest-{}.tmp", uuid::Uuid::new_v4());
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .follow(FollowSymlinks::No);
    let mut file = dir
        .open_with(&temporary, &options)
        .map_err(|_| Error::Unavailable)?;
    file.write_all(bytes).map_err(|_| Error::Unavailable)?;
    file.sync_all().map_err(|_| Error::Unavailable)?;
    drop(file);
    dir.rename(&temporary, dir, "manifest.json")
        .map_err(|_| Error::Unavailable)?;
    sync_directory(dir)
}
