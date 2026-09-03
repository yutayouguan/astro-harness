//! Explicit, Desktop-owned Git worktree creation.
//!
//! Agent threads inherit their parent's checkout. This crate is reserved for explicit desktop
//! tasks that request an isolated checkout.

use anyhow::{bail, Context};
use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const DISABLED_HOOKS_PATH: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };
const SAFE_BARE_REPOSITORY_CONFIG: &str = "safe.bareRepository=explicit";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeSettings {
    pub root: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateWorktree {
    pub source_cwd: PathBuf,
    pub base: Option<String>,
}

#[derive(Debug)]
pub struct ManagedWorktree {
    pub root: PathBuf,
    pub cwd: PathBuf,
    pub source_root: PathBuf,
    pub source_cwd: PathBuf,
    pub head_sha: String,
    pub branch: Option<String>,
    clean_only: bool,
}

impl ManagedWorktree {
    pub fn path(&self) -> &Path {
        &self.root
    }

    pub fn cleanup(self) {
        cleanup_worktree(&self.source_root, &self.root, self.clean_only);
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
        let root = allocate_worktree_root(&self.settings.root, repository_name)?;

        if let Err(error) = git_output(
            &source_root,
            false,
            [
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new("--detach"),
                OsStr::new("--no-checkout"),
                root.as_os_str(),
                OsStr::new(&head_sha),
            ],
        ) {
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

        Ok(ManagedWorktree {
            root,
            cwd,
            source_root,
            source_cwd,
            head_sha,
            branch: None,
            clean_only: true,
        })
    }
}

/// Compatibility entry point for callers that previously used the Core helper.
pub fn create_task_worktree(repo: &Path, _task_id: &str) -> anyhow::Result<ManagedWorktree> {
    let root = repository_root(repo)?;
    let managed_root = root.join(".worktrees");
    ensure_worktrees_gitignore(&root)?;
    let managed =
        WorktreeManager::new(WorktreeSettings { root: managed_root }).create(&CreateWorktree {
            source_cwd: repo.to_path_buf(),
            base: None,
        })?;
    copy_worktreeinclude(&root, &managed.root)?;
    Ok(managed)
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

pub fn cleanup_task_worktree(
    repo: &Path,
    path: &Path,
    _branch: &str,
    clean_only: bool,
) -> anyhow::Result<()> {
    let source_root = repository_root(repo)?;
    let managed_root = source_root.join(".worktrees");
    let managed_root = fs::canonicalize(&managed_root).with_context(|| {
        format!(
            "cannot resolve managed worktree root {}",
            managed_root.display()
        )
    })?;
    let checkout = fs::canonicalize(path)
        .with_context(|| format!("cannot resolve managed worktree {}", path.display()))?;
    let relative = checkout
        .strip_prefix(&managed_root)
        .context("worktree is outside Astro's managed worktree root")?;
    let components = relative.components().collect::<Vec<_>>();
    let expected_name = source_root
        .file_name()
        .context("repository root has no directory name")?;
    anyhow::ensure!(
        components.len() == 2
            && components[0].as_os_str().to_string_lossy().len() == 4
            && components[1].as_os_str() == expected_name,
        "worktree does not use Astro's managed allocation layout"
    );
    cleanup_worktree(&source_root, &checkout, clean_only);
    Ok(())
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

fn allocate_worktree_root(root: &Path, repository_name: &OsStr) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(root)
        .with_context(|| format!("cannot create worktree root {}", root.display()))?;
    for _ in 0..=u16::MAX {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let bucket = root.join(&id[..4]);
        match fs::create_dir(&bucket) {
            Ok(()) => return Ok(bucket.join(repository_name)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    bail!("all managed worktree identifiers are in use")
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
    let _ = git_output(
        source_root,
        false,
        [
            OsStr::new("worktree"),
            OsStr::new("remove"),
            OsStr::new("--force"),
            root.as_os_str(),
        ],
    );
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
    git_stdout(path, false, ["status", "--porcelain"])
        .map(|output| !output.is_empty())
        .unwrap_or(true)
}

fn ensure_worktrees_gitignore(repo: &Path) -> anyhow::Result<()> {
    let path = repo.join(".gitignore");
    let needle = ".worktrees/";
    let mut contents = fs::read_to_string(&path).unwrap_or_default();
    if contents.lines().any(|line| line.trim() == needle) {
        return Ok(());
    }
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str(needle);
    contents.push('\n');
    fs::write(path, contents).context("cannot update .gitignore")
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
    fn cleanup_rejects_paths_outside_the_managed_layout() {
        let repo = tempfile::tempdir().unwrap();
        init_git_repo(repo.path());
        fs::create_dir(repo.path().join(".worktrees")).unwrap();
        let unrelated = tempfile::tempdir().unwrap();

        let error =
            cleanup_task_worktree(repo.path(), unrelated.path(), "ignored", false).unwrap_err();

        assert!(error
            .to_string()
            .contains("outside Astro's managed worktree root"));
        assert!(unrelated.path().exists());
    }

    #[test]
    fn cleanup_accepts_an_astro_managed_checkout() {
        let repo = tempfile::tempdir().unwrap();
        init_git_repo(repo.path());
        let managed = create_task_worktree(repo.path(), "cleanup-1").unwrap();
        let checkout = managed.root.clone();

        cleanup_task_worktree(repo.path(), &checkout, "ignored", true).unwrap();

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
        let managed = create_task_worktree(repo.path(), "dirty-1").unwrap();
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
        managed.cleanup();
        assert!(root.exists());
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
