//! Git worktree 隔离：委派子任务 / 会话级项目检出。
//!
//! 记忆工作区（`~/.astro/workspace-*`）与代码工作树分离；本模块只管仓库检出。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 一次任务工作树句柄；清理时若干净则删除 worktree + 分支。
#[derive(Debug)]
pub struct WorktreeHandle {
    pub path: PathBuf,
    pub branch: String,
    pub repo_root: PathBuf,
    clean_only: bool,
}

impl WorktreeHandle {
    /// 清理工作树。`clean_only=true`（默认）时仅在工作树干净时删除；脏则保留并打日志。
    pub fn cleanup(self) {
        cleanup_worktree(&self.repo_root, &self.path, &self.branch, self.clean_only);
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// `git rev-parse --show-toplevel`；失败或不在仓内返回 `None`。
pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(start)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() {
        return None;
    }
    Some(PathBuf::from(s))
}

/// 解析代码仓根：显式路径 → `ASTRO_PROJECT_ROOT` → `current_dir` 的 git root。
pub fn resolve_project_root(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        if p.as_os_str().is_empty() {
            // fall through
        } else if let Some(root) = find_git_root(p) {
            return Some(root);
        } else if p.is_dir() {
            // 显式非 git 目录：不建 worktree，但仍可作为 project_root 使用
            return Some(p.to_path_buf());
        }
    }
    if let Ok(env) = std::env::var("ASTRO_PROJECT_ROOT") {
        let p = PathBuf::from(env.trim());
        if !p.as_os_str().is_empty() {
            if let Some(root) = find_git_root(&p) {
                return Some(root);
            }
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    let cwd = std::env::current_dir().ok()?;
    find_git_root(&cwd)
}

/// 在 `{repo}/.worktrees/astro-{short}/` 创建独立检出，分支 `astro/task-{short}`。
pub fn create_task_worktree(repo: &Path, task_id: &str) -> anyhow::Result<WorktreeHandle> {
    let short = short_id(task_id);
    let worktrees_dir = repo.join(".worktrees");
    fs::create_dir_all(&worktrees_dir)?;
    ensure_worktrees_gitignore(repo)?;

    let mut path = worktrees_dir.join(format!("astro-{short}"));
    let mut branch = format!("astro/task-{short}");
    if path.exists() {
        let suffix = uuid_short();
        path = worktrees_dir.join(format!("astro-{short}-{suffix}"));
        branch = format!("astro/task-{short}-{suffix}");
    }

    add_worktree(repo, &path, &branch)
}

fn add_worktree(repo: &Path, path: &Path, branch: &str) -> anyhow::Result<WorktreeHandle> {
    // 基于当前 HEAD 新建分支并检出到 worktree
    let out = Command::new("git")
        .args([
            "worktree",
            "add",
            "-b",
            branch,
            &path.to_string_lossy(),
            "HEAD",
        ])
        .current_dir(repo)
        .output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        // 分支已存在时尝试不带 -b
        if err.contains("already exists") {
            let out2 = Command::new("git")
                .args(["worktree", "add", &path.to_string_lossy(), branch])
                .current_dir(repo)
                .output()?;
            if !out2.status.success() {
                anyhow::bail!(
                    "git worktree add failed: {}",
                    String::from_utf8_lossy(&out2.stderr)
                );
            }
        } else {
            anyhow::bail!("git worktree add failed: {err}");
        }
    }

    copy_worktreeinclude(repo, path);

    Ok(WorktreeHandle {
        path: path.to_path_buf(),
        branch: branch.to_string(),
        repo_root: repo.to_path_buf(),
        clean_only: true,
    })
}

fn copy_worktreeinclude(repo: &Path, worktree: &Path) {
    let include = repo.join(".worktreeinclude");
    let Ok(text) = fs::read_to_string(&include) else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let src = repo.join(line);
        let dst = worktree.join(line);
        if !src.exists() {
            continue;
        }
        if let Some(parent) = dst.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if src.is_dir() {
            let _ = copy_dir_recursive(&src, &dst);
        } else {
            let _ = fs::copy(&src, &dst);
        }
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

fn ensure_worktrees_gitignore(repo: &Path) -> anyhow::Result<()> {
    let gi = repo.join(".gitignore");
    let needle = ".worktrees/";
    if gi.is_file() {
        let text = fs::read_to_string(&gi)?;
        if text.lines().any(|l| {
            let t = l.trim();
            t == needle || t == ".worktrees" || t == "**/.worktrees/"
        }) {
            return Ok(());
        }
        let mut text = text;
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(needle);
        text.push('\n');
        fs::write(&gi, text)?;
    } else {
        fs::write(&gi, format!("{needle}\n"))?;
    }
    Ok(())
}

/// 清理任务工作树。`clean_only=true`（默认）时脏树保留。
pub fn cleanup_task_worktree(repo: &Path, path: &Path, branch: &str, clean_only: bool) {
    cleanup_worktree(repo, path, branch, clean_only);
}

fn cleanup_worktree(repo: &Path, path: &Path, branch: &str, clean_only: bool) {
    if !path.exists() {
        return;
    }
    let dirty = is_worktree_dirty(path);
    if dirty && clean_only {
        tracing::warn!(
            path = %path.display(),
            branch = %branch,
            "delegate worktree dirty; keeping for manual recovery"
        );
        return;
    }

    let _ = Command::new("git")
        .args(["worktree", "remove", "--force", &path.to_string_lossy()])
        .current_dir(repo)
        .output();
    let _ = Command::new("git")
        .args(["branch", "-D", branch])
        .current_dir(repo)
        .output();
}

fn is_worktree_dirty(path: &Path) -> bool {
    let Ok(out) = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(path)
        .output()
    else {
        return true;
    };
    if !out.status.success() {
        return true;
    }
    !String::from_utf8_lossy(&out.stdout).trim().is_empty()
}

fn short_id(task_id: &str) -> String {
    let t = task_id.trim();
    let take = t.chars().take(8).collect::<String>();
    if take.is_empty() {
        uuid_short()
    } else {
        take.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect()
    }
}

fn uuid_short() -> String {
    uuid::Uuid::new_v4().to_string().chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn init_git_repo(dir: &Path) {
        assert!(Command::new("git")
            .args(["init"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("git")
            .args(["config", "user.name", "test"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success());
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
    fn find_git_root_none_outside_repo() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_git_root(dir.path()).is_none());
    }

    #[test]
    fn create_and_cleanup_clean_worktree() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        let handle = create_task_worktree(dir.path(), "abc12345-xxxx").unwrap();
        assert!(handle.path().exists());
        assert!(handle.path().join("README.md").exists());
        let gi = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(gi.contains(".worktrees/"));
        let path = handle.path().to_path_buf();
        handle.cleanup();
        assert!(!path.exists());
    }

    #[test]
    fn dirty_worktree_kept() {
        let dir = tempfile::tempdir().unwrap();
        init_git_repo(dir.path());
        let handle = create_task_worktree(dir.path(), "dirty001").unwrap();
        fs::write(handle.path().join("new.txt"), "x").unwrap();
        let path = handle.path().to_path_buf();
        handle.cleanup();
        assert!(path.exists(), "dirty worktree should be kept");
    }

    #[test]
    fn resolve_skips_non_git_without_env() {
        let dir = tempfile::tempdir().unwrap();
        // No ASTRO_PROJECT_ROOT; cwd may be git — we only assert explicit non-git
        let root = resolve_project_root(Some(dir.path()));
        // explicit non-git dir still returned as project_root for cwd purposes
        assert_eq!(root.as_deref(), Some(dir.path()));
        assert!(find_git_root(dir.path()).is_none());
    }
}
