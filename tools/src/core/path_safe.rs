//! Workspace 相对路径安全解析：防止 `..`、symlink 与目录逃逸攻击。
//!
//! 所有文件类工具（`file_ops`、`terminal` 等）在拼接用户给定相对路径前，
//! 须经 [`resolve_safe`] 校验，确保解析结果始终落在 Agent 工作区之内。

use std::path::{Component, Path, PathBuf};

/// 将相对路径解析为 workspace 下的绝对路径。
///
/// # 行为
/// - 空路径或 `"."` 返回 workspace 根目录（canonical）。
/// - **始终拒绝**路径中的 `..` 组件（含 `subdir/..`）。
/// - 逐段检查已存在路径：symlink 的目标须在 workspace 内；中间目录 symlink
///   会跟随到目标后继续拼接，避免经链接写出界。
/// - 新建文件（末段尚不存在）同样校验已存在前缀不越界。
///
/// # 约束
/// - 输入路径会先 `trim` 并去掉前导 `/`。
/// - 越界或含 `..` 时返回中文错误信息。
pub fn resolve_safe(workspace: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    let rel = rel.trim().trim_start_matches('/');
    let base = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());

    if rel.is_empty() || rel == "." {
        return Ok(base);
    }

    // 先规范化相对段：拒绝 `..` / 盘符 / 根，折叠 `.`
    let mut logical = PathBuf::new();
    for c in Path::new(rel).components() {
        match c {
            Component::Normal(s) => logical.push(s),
            Component::CurDir => {}
            Component::ParentDir => anyhow::bail!("路径越界：不允许使用 .."),
            _ => anyhow::bail!("非法路径组件"),
        }
    }
    if logical.as_os_str().is_empty() {
        return Ok(base);
    }

    let components: Vec<_> = logical.components().collect();
    let mut cur = base.clone();

    for (i, c) in components.iter().enumerate() {
        cur.push(c.as_os_str());
        let is_last = i + 1 == components.len();

        match std::fs::symlink_metadata(&cur) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let target = std::fs::canonicalize(&cur).map_err(|e| {
                    anyhow::anyhow!("无法解析符号链接 {}: {e}", cur.display())
                })?;
                if !target.starts_with(&base) {
                    anyhow::bail!("路径越界：不允许访问 workspace 之外的文件");
                }
                if !is_last {
                    // 中间目录链接：后续组件接到真实目标上
                    cur = target;
                }
                // 末段链接：返回工作区内的链接路径本身（delete 可删链；
                // read/write 跟随目标，目标已校验在 workspace 内）
            }
            Ok(_) => {
                let canon = std::fs::canonicalize(&cur)?;
                if !canon.starts_with(&base) {
                    anyhow::bail!("路径越界：不允许访问 workspace 之外的文件");
                }
                if !is_last {
                    cur = canon;
                }
            }
            Err(_) => {
                // 当前段尚不存在：父路径已在 workspace 内，追加剩余段供新建
                for c2 in &components[i + 1..] {
                    cur.push(c2.as_os_str());
                }
                return Ok(cur);
            }
        }
    }

    Ok(cur)
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
    fn rejects_subdir_dotdot() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("subdir")).unwrap();
        let err = resolve_safe(dir.path(), "subdir/..").unwrap_err();
        assert!(err.to_string().contains("..") || err.to_string().contains("越界"));
    }

    #[test]
    fn empty_and_dot_are_workspace_root() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        assert_eq!(resolve_safe(dir.path(), "").unwrap(), base);
        assert_eq!(resolve_safe(dir.path(), ".").unwrap(), base);
        assert_eq!(resolve_safe(dir.path(), "  ").unwrap(), base);
    }

    #[test]
    fn allows_nested_relative() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("a/b")).unwrap();
        let p = resolve_safe(dir.path(), "a/b/c.txt").unwrap();
        assert!(p.starts_with(dir.path().canonicalize().unwrap()));
        assert!(p.ends_with("c.txt"));
    }

    #[test]
    fn rejects_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let link = dir.path().join("escape");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path(), &link).unwrap();
            let err = resolve_safe(dir.path(), "escape/secret.txt").unwrap_err();
            assert!(err.to_string().contains("越界"));
        }
        #[cfg(not(unix))]
        {
            let _ = (link, outside);
        }
    }

    #[test]
    fn allows_symlink_inside_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let target_dir = dir.path().join("real");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("f.txt"), "ok").unwrap();
        let link = dir.path().join("alias");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&target_dir, &link).unwrap();
            let p = resolve_safe(dir.path(), "alias/f.txt").unwrap();
            assert!(p.starts_with(dir.path().canonicalize().unwrap()));
            assert!(fs::read_to_string(&p).unwrap().contains("ok") || p.exists());
        }
        #[cfg(not(unix))]
        {
            let _ = (link, target_dir);
        }
    }
}
