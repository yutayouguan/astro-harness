//! Explicit, Desktop-owned Git worktree creation.
//!
//! Agent threads inherit their parent's checkout. This crate is reserved for explicit desktop
//! tasks that request an isolated checkout.

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const DISABLED_HOOKS_PATH: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };
const SAFE_BARE_REPOSITORY_CONFIG: &str = "safe.bareRepository=explicit";
const MANIFEST_FILE: &str = "worktree.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeSettings {
    pub root: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateWorktree {
    pub source_cwd: PathBuf,
    pub base: Option<String>,
    pub branch: Option<String>,
    pub owner_session_id: Option<String>,
}

#[derive(Debug)]
pub struct ManagedWorktree {
    pub id: String,
    pub root: PathBuf,
    pub cwd: PathBuf,
    pub source_root: PathBuf,
    pub source_cwd: PathBuf,
    pub head_sha: String,
    pub branch: Option<String>,
    pub owner_session_id: Option<String>,
    clean_only: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedWorktreeInfo {
    pub id: String,
    pub root: PathBuf,
    pub cwd: PathBuf,
    pub source_root: PathBuf,
    pub source_cwd: PathBuf,
    pub head_sha: String,
    pub branch: Option<String>,
    pub owner_session_id: Option<String>,
    pub dirty: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct WorktreeManifest {
    id: String,
    root: PathBuf,
    cwd: PathBuf,
    source_root: PathBuf,
    source_cwd: PathBuf,
    head_sha: String,
    branch: Option<String>,
    #[serde(default)]
    owner_session_id: Option<String>,
}

impl ManagedWorktree {
    pub fn path(&self) -> &Path {
        &self.root
    }

    pub fn cleanup(self) {
        cleanup_worktree(&self.source_root, &self.root, self.clean_only);
        if !self.root.exists() {
            if let Some(bucket) = self.root.parent() {
                let _ = fs::remove_file(bucket.join(MANIFEST_FILE));
            }
            remove_empty_bucket(&self.root);
        }
    }
}

#[derive(Clone, Debug)]
pub struct WorktreeManager {
    settings: WorktreeSettings,
}

impl WorktreeManager {
    pub fn new(settings: WorktreeSettings) -> Self {
        Self { settings }
    }

    pub fn settings(&self) -> &WorktreeSettings {
        &self.settings
    }

    pub fn create(&self, request: &CreateWorktree) -> anyhow::Result<ManagedWorktree> {
        if !self.settings.root.is_absolute() {
            bail!("managed worktree root must be an absolute path");
        }

        let source_cwd = fs::canonicalize(&request.source_cwd)
            .with_context(|| format!("cannot resolve {}", request.source_cwd.display()))?;
        let source_root = repository_root(&source_cwd)?;
        let relative_cwd = source_cwd
            .strip_prefix(&source_root)
            .context("working directory is outside the repository root")?;
        let repository_name = source_root
            .file_name()
            .context("repository root has no directory name")?;
        let revision = format!("{}^{{commit}}", request.base.as_deref().unwrap_or("HEAD"));
        let head_sha = git_stdout(
            &source_root,
            false,
            [
                "rev-parse",
                "--verify",
                "--end-of-options",
                revision.as_str(),
            ],
        )?;
        let (id, root) = allocate_worktree_root(&self.settings.root, repository_name)?;

        let branch = request
            .branch
            .as_deref()
            .map(str::trim)
            .filter(|branch| !branch.is_empty())
            .map(str::to_string);

        let mut add_args = vec![OsStr::new("worktree"), OsStr::new("add")];
        if let Some(branch) = branch.as_deref() {
            add_args.extend([OsStr::new("-b"), OsStr::new(branch)]);
        } else {
            add_args.push(OsStr::new("--detach"));
        }
        add_args.extend([
            OsStr::new("--no-checkout"),
            root.as_os_str(),
            OsStr::new(&head_sha),
        ]);

        if let Err(error) = git_output(&source_root, false, add_args) {
            remove_empty_bucket(&root);
            return Err(error).context("cannot create managed worktree");
        }

        let populate = git_output(
            &root,
            true,
            [
                OsStr::new("--work-tree=."),
                OsStr::new("reset"),
                OsStr::new("--hard"),
                OsStr::new("--no-recurse-submodules"),
                OsStr::new(&head_sha),
            ],
        );
        if let Err(error) = populate {
            remove_worktree(&source_root, &root);
            return Err(error).context("cannot populate managed worktree");
        }

        let cwd = root.join(relative_cwd);
        if !safe_worktree_cwd(&root, &cwd) {
            remove_worktree(&source_root, &root);
            bail!(
                "requested base does not contain a safe working directory {}",
                relative_cwd.display()
            );
        }

        copy_worktreeinclude(&source_root, &root)?;
        let managed = ManagedWorktree {
            id,
            root,
            cwd,
            source_root,
            source_cwd,
            head_sha,
            branch,
            owner_session_id: request
                .owner_session_id
                .as_deref()
                .map(str::trim)
                .filter(|owner| !owner.is_empty())
                .map(str::to_string),
            clean_only: true,
        };
        if let Err(error) = write_manifest(&managed) {
            remove_worktree(&managed.source_root, &managed.root);
            if let Some(bucket) = managed.root.parent() {
                let _ = fs::remove_file(bucket.join(MANIFEST_FILE));
            }
            remove_empty_bucket(&managed.root);
            return Err(error);
        }
        Ok(managed)
    }

    pub fn cleanup(&self, id: &str, clean_only: bool) -> anyhow::Result<bool> {
        anyhow::ensure!(valid_worktree_id(id), "invalid managed worktree id");
        let managed_root = fs::canonicalize(&self.settings.root).with_context(|| {
            format!(
                "cannot resolve managed worktree root {}",
                self.settings.root.display()
            )
        })?;
        let bucket = fs::canonicalize(managed_root.join(id))
            .context("cannot resolve managed worktree allocation")?;
        let manifest_path = bucket.join(MANIFEST_FILE);
        let manifest: WorktreeManifest = serde_json::from_slice(
            &fs::read(&manifest_path)
                .with_context(|| format!("cannot read {}", manifest_path.display()))?,
        )?;
        anyhow::ensure!(manifest.id == id, "managed worktree manifest id mismatch");
        let checkout =
            fs::canonicalize(&manifest.root).context("cannot resolve managed worktree checkout")?;
        anyhow::ensure!(
            checkout.parent() == Some(bucket.as_path()),
            "managed worktree escaped its bucket"
        );
        let source_root = repository_root(&manifest.source_root)?;
        anyhow::ensure!(
            source_root == manifest.source_root,
            "managed worktree source root mismatch"
        );
        anyhow::ensure!(
            registered_worktree_roots(&source_root)?.contains(&checkout),
            "managed worktree is not registered in its source repository"
        );
        if clean_only && is_worktree_dirty(&checkout) {
            return Ok(false);
        }
        let mut remove_args = vec![OsStr::new("worktree"), OsStr::new("remove")];
        if !clean_only {
            remove_args.push(OsStr::new("--force"));
        }
        remove_args.push(checkout.as_os_str());
        git_output(&source_root, false, remove_args)?;
        let _ = fs::remove_file(manifest_path);
        remove_empty_bucket(&checkout);
        Ok(true)
    }

    /// List only valid, currently registered Astro-managed worktrees for one repository.
    pub fn list(&self, source_cwd: &Path) -> anyhow::Result<Vec<ManagedWorktreeInfo>> {
        if !self.settings.root.exists() {
            return Ok(Vec::new());
        }
        let managed_root = fs::canonicalize(&self.settings.root).with_context(|| {
            format!(
                "cannot resolve managed worktree root {}",
                self.settings.root.display()
            )
        })?;
        let source_cwd = fs::canonicalize(source_cwd)
            .with_context(|| format!("cannot resolve {}", source_cwd.display()))?;
        let source_root = repository_root(&source_cwd)?;
        let registered = registered_worktree_roots(&source_root)?;
        let mut worktrees = Vec::new();

        for entry in fs::read_dir(&managed_root)? {
            let entry = entry?;
            let id = entry.file_name().to_string_lossy().into_owned();
            if !valid_worktree_id(&id) || !entry.file_type()?.is_dir() {
                continue;
            }
            let bucket = fs::canonicalize(entry.path())?;
            let manifest_path = bucket.join(MANIFEST_FILE);
            if !fs::symlink_metadata(&manifest_path).is_ok_and(|meta| meta.file_type().is_file()) {
                continue;
            }
            let manifest: WorktreeManifest = match fs::read(&manifest_path)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            {
                Some(manifest) => manifest,
                None => continue,
            };
            if manifest.id != id {
                continue;
            }
            let Ok(checkout) = fs::canonicalize(&manifest.root) else {
                continue;
            };
            if checkout.parent() != Some(bucket.as_path()) || !registered.contains(&checkout) {
                continue;
            }
            let Ok(manifest_source_root) = fs::canonicalize(&manifest.source_root) else {
                continue;
            };
            let Ok(manifest_source_cwd) = fs::canonicalize(&manifest.source_cwd) else {
                continue;
            };
            if manifest_source_root != source_root
                || !manifest_source_cwd.starts_with(&source_root)
                || !safe_worktree_cwd(&checkout, &manifest.cwd)
            {
                continue;
            }
            let Ok(head_sha) = git_stdout(&checkout, false, ["rev-parse", "HEAD"]) else {
                continue;
            };
            let branch =
                git_stdout(&checkout, false, ["symbolic-ref", "-q", "--short", "HEAD"]).ok();
            worktrees.push(ManagedWorktreeInfo {
                id,
                root: checkout.clone(),
                cwd: manifest.cwd,
                source_root: manifest_source_root,
                source_cwd: manifest_source_cwd,
                head_sha,
                branch,
                owner_session_id: manifest.owner_session_id,
                dirty: is_worktree_dirty(&checkout),
            });
        }
        worktrees.sort_by(|left, right| left.root.cmp(&right.root));
        Ok(worktrees)
    }
}

pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    git_stdout(start, false, ["rev-parse", "--show-toplevel"])
        .ok()
        .map(PathBuf::from)
}

pub fn resolve_project_root(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit.filter(|path| !path.as_os_str().is_empty()) {
        return find_git_root(path).or_else(|| path.is_dir().then(|| path.to_path_buf()));
    }
    if let Ok(value) = std::env::var("ASTRO_PROJECT_ROOT") {
        let path = PathBuf::from(value.trim());
        if !path.as_os_str().is_empty() {
            return find_git_root(&path).or_else(|| path.is_dir().then_some(path));
        }
    }
    std::env::current_dir()
        .ok()
        .and_then(|path| find_git_root(&path))
}

fn git_output(
    cwd: &Path,
    working_tree: bool,
    args: impl IntoIterator<Item = impl AsRef<OsStr>>,
) -> anyhow::Result<Output> {
    let mut command = base_git_command(cwd);
    if working_tree {
        for filter in configured_filters(cwd)? {
            for key in ["process", "clean", "smudge"] {
                command.arg("-c").arg(format!("filter.{filter}.{key}="));
            }
            command
                .arg("-c")
                .arg(format!("filter.{filter}.required=false"));
        }
    }
    let output = command.args(args).output().context("failed to start git")?;
    if !output.status.success() {
        bail!(
            "git command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output)
}

fn git_stdout(
    cwd: &Path,
    working_tree: bool,
    args: impl IntoIterator<Item = impl AsRef<OsStr>>,
) -> anyhow::Result<String> {
    let output = git_output(cwd, working_tree, args)?;
    Ok(String::from_utf8(output.stdout)
        .context("git output is not valid UTF-8")?
        .trim()
        .to_string())
}

fn base_git_command(cwd: &Path) -> Command {
    let mut command = Command::new("git");
    for name in [
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CEILING_DIRECTORIES",
        "GIT_CONFIG",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
        "GIT_OBJECT_DIRECTORY",
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_IMPLICIT_WORK_TREE",
        "GIT_GRAFT_FILE",
        "GIT_INDEX_FILE",
        "GIT_NO_REPLACE_OBJECTS",
        "GIT_REPLACE_REF_BASE",
        "GIT_PREFIX",
        "GIT_SHALLOW_FILE",
        "GIT_COMMON_DIR",
    ] {
        command.env_remove(name);
    }
    command
        .current_dir(cwd)
        .arg("-c")
        .arg(SAFE_BARE_REPOSITORY_CONFIG)
        .arg("-c")
        .arg(format!("core.hooksPath={DISABLED_HOOKS_PATH}"))
        .arg("-c")
        .arg("core.fsmonitor=")
        .arg("-c")
        .arg("attr.tree=")
        .arg("-c")
        .arg("core.attributesFile=")
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
    command
}

fn configured_filters(cwd: &Path) -> anyhow::Result<BTreeSet<String>> {
    let output = base_git_command(cwd)
        .args([
            "config",
            "--null",
            "--name-only",
            "--get-regexp",
            "^filter\\.",
        ])
        .output()
        .context("failed to inspect Git filter configuration")?;
    if !output.status.success() {
        if output.status.code() == Some(1) {
            return Ok(BTreeSet::new());
        }
        bail!(
            "git command failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let names = String::from_utf8(output.stdout).context("Git filter names are not UTF-8")?;
    Ok(names
        .split('\0')
        .filter_map(|name| {
            name.strip_prefix("filter.")
                .and_then(|name| name.rsplit_once('.'))
                .filter(|(driver, key)| {
                    !driver.is_empty()
                        && matches!(*key, "clean" | "smudge" | "process" | "required")
                })
                .map(|(driver, _)| driver.to_string())
        })
        .collect())
}

fn repository_root(cwd: &Path) -> anyhow::Result<PathBuf> {
    let root = PathBuf::from(git_stdout(cwd, false, ["rev-parse", "--show-toplevel"])?);
    fs::canonicalize(&root)
        .with_context(|| format!("cannot resolve repository root {}", root.display()))
}

fn allocate_worktree_root(
    root: &Path,
    repository_name: &OsStr,
) -> anyhow::Result<(String, PathBuf)> {
    fs::create_dir_all(root)
        .with_context(|| format!("cannot create worktree root {}", root.display()))?;
    for _ in 0..=u16::MAX {
        let id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
        let bucket = root.join(&id);
        match fs::create_dir(&bucket) {
            Ok(()) => return Ok((id, bucket.join(repository_name))),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    bail!("all managed worktree identifiers are in use")
}

fn valid_worktree_id(id: &str) -> bool {
    id.len() == 12 && id.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn write_manifest(worktree: &ManagedWorktree) -> anyhow::Result<()> {
    let manifest = WorktreeManifest {
        id: worktree.id.clone(),
        root: worktree.root.clone(),
        cwd: worktree.cwd.clone(),
        source_root: worktree.source_root.clone(),
        source_cwd: worktree.source_cwd.clone(),
        head_sha: worktree.head_sha.clone(),
        branch: worktree.branch.clone(),
        owner_session_id: worktree.owner_session_id.clone(),
    };
    let path = worktree
        .root
        .parent()
        .context("managed worktree has no allocation bucket")?
        .join(MANIFEST_FILE);
    fs::write(path, serde_json::to_vec_pretty(&manifest)?)?;
    Ok(())
}

fn safe_worktree_cwd(root: &Path, cwd: &Path) -> bool {
    let Ok(root) = fs::canonicalize(root) else {
        return false;
    };
    fs::canonicalize(cwd).is_ok_and(|resolved| resolved.is_dir() && resolved.starts_with(root))
}

fn cleanup_worktree(source_root: &Path, root: &Path, clean_only: bool) {
    if !root.exists() || (clean_only && is_worktree_dirty(root)) {
        if root.exists() && clean_only {
            tracing::warn!(path = %root.display(), "managed worktree is dirty; keeping it for recovery");
        }
        return;
    }
    let mut args = vec![OsStr::new("worktree"), OsStr::new("remove")];
    if !clean_only {
        args.push(OsStr::new("--force"));
    }
    args.push(root.as_os_str());
    let _ = git_output(source_root, false, args);
    remove_empty_bucket(root);
}

fn remove_worktree(source_root: &Path, root: &Path) {
    cleanup_worktree(source_root, root, false);
}

fn remove_empty_bucket(checkout: &Path) {
    if let Some(bucket) = checkout.parent() {
        let _ = fs::remove_dir(bucket);
    }
}

fn is_worktree_dirty(path: &Path) -> bool {
    git_stdout(path, false, ["status", "--porcelain", "--ignored=matching"])
        .map(|output| !output.is_empty())
        .unwrap_or(true)
}

fn registered_worktree_roots(source_root: &Path) -> anyhow::Result<BTreeSet<PathBuf>> {
    let output = git_output(
        source_root,
        false,
        ["worktree", "list", "--porcelain", "-z"],
    )?;
    let mut roots = BTreeSet::new();
    for field in output.stdout.split(|byte| *byte == 0) {
        let Some(path) = field.strip_prefix(b"worktree ") else {
            continue;
        };
        let path = std::str::from_utf8(path).context("worktree path is not valid UTF-8")?;
        if let Ok(path) = fs::canonicalize(path) {
            roots.insert(path);
        }
    }
    Ok(roots)
}

fn copy_worktreeinclude(repo: &Path, worktree: &Path) -> anyhow::Result<()> {
    let include = repo.join(".worktreeinclude");
    let Ok(contents) = fs::read_to_string(include) else {
        return Ok(());
    };
    for relative in contents.lines().map(str::trim) {
        if relative.is_empty()
            || relative.starts_with('#')
            || Path::new(relative).is_absolute()
            || Path::new(relative)
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            continue;
        }
        let source = repo.join(relative);
        let destination = worktree.join(relative);
        let Ok(metadata) = fs::symlink_metadata(&source) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        copy_included_path(&source, &destination)?;
    }
    Ok(())
}

fn copy_included_path(source: &Path, destination: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_dir() {
        fs::create_dir_all(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_included_path(&entry.path(), &destination.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if metadata.is_file() {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, destination).with_context(|| {
            format!(
                "cannot copy worktree include {} to {}",
                source.display(),
                destination.display()
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_git_repo(dir: &Path) {
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "test"],
        ] {
            assert!(Command::new("git")
                .args(args)
                .current_dir(dir)
                .status()
                .unwrap()
                .success());
        }
        fs::write(dir.join("README.md"), "hello").unwrap();
        assert!(Command::new("git")
            .args(["add", "."])
            .current_dir(dir)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success());
    }

    #[test]
    fn creates_detached_worktree_and_preserves_nested_cwd() {
        let repo = tempfile::tempdir().unwrap();
        init_git_repo(repo.path());
        let nested = repo.path().join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("tracked.txt"), "tracked").unwrap();
        assert!(Command::new("git")
            .args(["add", "."])
            .current_dir(repo.path())
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["commit", "-m", "nested"])
            .current_dir(repo.path())
            .status()
            .unwrap()
            .success());
        let storage = tempfile::tempdir().unwrap();
        let managed = WorktreeManager::new(WorktreeSettings {
            root: storage.path().to_path_buf(),
        })
        .create(&CreateWorktree {
            source_cwd: nested,
            base: None,
            branch: None,
            owner_session_id: None,
        })
        .unwrap();

        assert!(managed.cwd.ends_with("nested"));
        assert!(managed.cwd.join("tracked.txt").is_file());
        assert!(git_stdout(&managed.root, false, ["symbolic-ref", "-q", "HEAD"]).is_err());
        let root = managed.root.clone();
        managed.cleanup();
        assert!(!root.exists());
    }

    #[test]
    fn explicit_base_selects_the_requested_commit() {
        let repo = tempfile::tempdir().unwrap();
        init_git_repo(repo.path());
        let first = git_stdout(repo.path(), false, ["rev-parse", "HEAD"]).unwrap();
        fs::write(repo.path().join("README.md"), "new").unwrap();
        assert!(Command::new("git")
            .args(["commit", "-am", "second"])
            .current_dir(repo.path())
            .status()
            .unwrap()
            .success());
        let storage = tempfile::tempdir().unwrap();
        let managed = WorktreeManager::new(WorktreeSettings {
            root: storage.path().to_path_buf(),
        })
        .create(&CreateWorktree {
            source_cwd: repo.path().to_path_buf(),
            base: Some(first.clone()),
            branch: None,
            owner_session_id: None,
        })
        .unwrap();

        assert_eq!(managed.head_sha, first);
        assert_eq!(
            fs::read_to_string(managed.root.join("README.md")).unwrap(),
            "hello"
        );
        managed.cleanup();
    }

    #[test]
    fn list_returns_only_registered_manifests_with_owner() {
        let repo = tempfile::tempdir().unwrap();
        init_git_repo(repo.path());
        let storage = tempfile::tempdir().unwrap();
        let manager = WorktreeManager::new(WorktreeSettings {
            root: storage.path().to_path_buf(),
        });
        let managed = manager
            .create(&CreateWorktree {
                source_cwd: repo.path().to_path_buf(),
                base: None,
                branch: None,
                owner_session_id: Some("session-1".into()),
            })
            .unwrap();

        let listed = manager.list(repo.path()).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, managed.id);
        assert_eq!(listed[0].owner_session_id.as_deref(), Some("session-1"));
        assert!(!listed[0].dirty);
        assert!(manager.cleanup(&managed.id, true).unwrap());
    }

    #[test]
    fn git_commands_remove_inherited_repository_selectors() {
        let dir = tempfile::tempdir().unwrap();
        let command = base_git_command(dir.path());
        let overrides = command
            .get_envs()
            .map(|(key, value)| (key.to_string_lossy().into_owned(), value.is_none()))
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(overrides.get("GIT_DIR"), Some(&true));
        assert_eq!(overrides.get("GIT_WORK_TREE"), Some(&true));
        assert_eq!(overrides.get("GIT_INDEX_FILE"), Some(&true));
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args.iter().any(|arg| arg == SAFE_BARE_REPOSITORY_CONFIG));
    }

    #[test]
    fn cleanup_rejects_invalid_opaque_ids() {
        let repo = tempfile::tempdir().unwrap();
        init_git_repo(repo.path());
        let storage = tempfile::tempdir().unwrap();
        let manager = WorktreeManager::new(WorktreeSettings {
            root: storage.path().to_path_buf(),
        });

        let error = manager.cleanup("../outside", false).unwrap_err();

        assert!(error.to_string().contains("invalid managed worktree id"));
    }

    #[test]
    fn cleanup_accepts_an_astro_managed_checkout() {
        let repo = tempfile::tempdir().unwrap();
        init_git_repo(repo.path());
        let storage = tempfile::tempdir().unwrap();
        let manager = WorktreeManager::new(WorktreeSettings {
            root: storage.path().to_path_buf(),
        });
        let managed = manager
            .create(&CreateWorktree {
                source_cwd: repo.path().to_path_buf(),
                base: None,
                branch: None,
                owner_session_id: None,
            })
            .unwrap();
        let checkout = managed.root.clone();

        assert!(manager.cleanup(&managed.id, true).unwrap());

        assert!(!checkout.exists());
    }

    #[test]
    fn dirty_worktree_is_kept_for_recovery() {
        let repo = tempfile::tempdir().unwrap();
        init_git_repo(repo.path());
        fs::write(
            repo.path().join(".worktreeinclude"),
            ".env\n.cache\n../escape\n",
        )
        .unwrap();
        fs::write(repo.path().join(".env"), "TOKEN=test").unwrap();
        fs::create_dir(repo.path().join(".cache")).unwrap();
        fs::write(repo.path().join(".cache/data"), "cached").unwrap();
        let storage = tempfile::tempdir().unwrap();
        let manager = WorktreeManager::new(WorktreeSettings {
            root: storage.path().to_path_buf(),
        });
        let managed = manager
            .create(&CreateWorktree {
                source_cwd: repo.path().to_path_buf(),
                base: None,
                branch: None,
                owner_session_id: None,
            })
            .unwrap();
        assert_eq!(
            fs::read_to_string(managed.root.join(".env")).unwrap(),
            "TOKEN=test"
        );
        assert_eq!(
            fs::read_to_string(managed.root.join(".cache/data")).unwrap(),
            "cached"
        );
        assert!(!managed.root.join("escape").exists());
        fs::write(managed.root.join("new.txt"), "x").unwrap();
        let root = managed.root.clone();
        assert!(!manager.cleanup(&managed.id, true).unwrap());
        assert!(root.exists());
    }

    #[test]
    fn ignored_files_are_kept_for_recovery() {
        let repo = tempfile::tempdir().unwrap();
        init_git_repo(repo.path());
        fs::write(repo.path().join(".gitignore"), ".secret\n").unwrap();
        assert!(Command::new("git")
            .args(["add", ".gitignore"])
            .current_dir(repo.path())
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["commit", "-m", "ignore secret"])
            .current_dir(repo.path())
            .status()
            .unwrap()
            .success());
        let storage = tempfile::tempdir().unwrap();
        let manager = WorktreeManager::new(WorktreeSettings {
            root: storage.path().to_path_buf(),
        });
        let managed = manager
            .create(&CreateWorktree {
                source_cwd: repo.path().to_path_buf(),
                base: None,
                branch: None,
                owner_session_id: None,
            })
            .unwrap();
        fs::write(managed.root.join(".secret"), "keep").unwrap();

        assert!(!manager.cleanup(&managed.id, true).unwrap());
        assert!(managed.root.exists());
    }

    #[test]
    fn explicit_branch_mode_creates_the_requested_branch() {
        let repo = tempfile::tempdir().unwrap();
        init_git_repo(repo.path());
        let storage = tempfile::tempdir().unwrap();
        let manager = WorktreeManager::new(WorktreeSettings {
            root: storage.path().to_path_buf(),
        });
        let managed = manager
            .create(&CreateWorktree {
                source_cwd: repo.path().to_path_buf(),
                base: None,
                branch: Some("codex/explicit-worktree".into()),
                owner_session_id: None,
            })
            .unwrap();

        assert_eq!(managed.branch.as_deref(), Some("codex/explicit-worktree"));
        assert_eq!(
            git_stdout(&managed.root, false, ["branch", "--show-current"]).unwrap(),
            "codex/explicit-worktree"
        );
        assert!(manager.cleanup(&managed.id, true).unwrap());
    }

    #[test]
    fn explicit_non_git_directory_remains_a_project_root() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_project_root(Some(dir.path())),
            Some(dir.path().into())
        );
        assert!(find_git_root(dir.path()).is_none());
    }
}
