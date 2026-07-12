//! Workspace 相对路径安全解析：防止 `..` 与目录逃逸攻击。
//!
//! 所有文件类工具（`file_ops`、`terminal` 等）在拼接用户给定相对路径前，
//! 须经 [`resolve_safe`] 校验，确保解析结果始终落在 Agent 工作区之内。

use std::path::{Component, Path, PathBuf};

/// 将相对路径解析为 workspace 下的绝对路径。
///
/// # 行为
/// - 空路径或 `"."` 返回 workspace 根目录。
/// - 父目录已存在时，通过 `canonicalize` 校验不越界。
/// - 父目录不存在时，逐组件检查，拒绝 `..` 与非法组件。
///
/// # 约束
/// - 输入路径会先 `trim` 并去掉前导 `/`。
/// - 越界或含 `..` 时返回中文错误信息。
pub fn resolve_safe(workspace: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    let rel = rel.trim().trim_start_matches('/');
    if rel.is_empty() || rel == "." {
        return Ok(workspace.to_path_buf());
    }
    let candidate = workspace.join(rel);
    let canonical_base = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    let parent = candidate.parent().unwrap_or(workspace);
    if parent.exists() {
        let canon_parent = parent.canonicalize()?;
        if !canon_parent.starts_with(&canonical_base) {
            anyhow::bail!("路径越界：不允许访问 workspace 之外的文件");
        }
    } else {
        for c in Path::new(rel).components() {
            if matches!(c, Component::ParentDir) {
                anyhow::bail!("路径越界：不允许使用 ..");
            }
        }
        let mut check = PathBuf::new();
        for c in Path::new(rel).components() {
            match c {
                Component::Normal(s) => check.push(s),
                Component::CurDir => {}
                _ => anyhow::bail!("非法路径组件"),
            }
        }
    }
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn rejects_parent_dir_escape() {
        let dir = tempfile::tempdir().unwrap();
        let err = resolve_safe(dir.path(), "../etc/passwd").unwrap_err();
        assert!(err.to_string().contains("越界") || err.to_string().contains(".."));
    }

    #[test]
    fn allows_nested_relative() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("a/b")).unwrap();
        let p = resolve_safe(dir.path(), "a/b/c.txt").unwrap();
        assert!(p.starts_with(dir.path()));
    }
}
