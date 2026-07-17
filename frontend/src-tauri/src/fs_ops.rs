//! 工作区 / 文件空间的安全读写与路径解析（供命令层调用）。

use std::path::{Path, PathBuf};

/// 若 `dest_dir/preferred_name` 已存在，生成 `stem (n).ext`；目录无扩展名时 `name (n)`。
pub fn unique_dest_name(dest_dir: &Path, preferred_name: &str) -> PathBuf {
    let candidate = dest_dir.join(preferred_name);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(preferred_name);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(preferred_name);
    let ext = path.extension().and_then(|e| e.to_str());
    for n in 1..10_000 {
        let name = match ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        };
        let p = dest_dir.join(&name);
        if !p.exists() {
            return p;
        }
    }
    dest_dir.join(format!("{preferred_name}.{}", uuid_fallback()))
}

/// 无法生成唯一文件名时的毫秒时间戳兜底后缀。
fn uuid_fallback() -> String {
    // 极不可能走到；避免无限循环
    format!(
        "{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    )
}

/// 词法判断 `path` 是否位于 `root` 之下。
///
/// 拒绝任何 `..` 组件，避免 `{root}/../outside` 骗过朴素的 `starts_with`。
pub fn is_lexically_under(root: &Path, path: &Path) -> bool {
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return false;
    }
    path.starts_with(root)
}

/// `child` 是否等于 `parent` 或位于其目录树下。
pub fn is_same_or_subdir(parent: &Path, child: &Path) -> bool {
    let Ok(parent_canon) = parent.canonicalize() else {
        return false;
    };
    let Ok(child_canon) = child.canonicalize() else {
        // 目标尚不存在时用组件前缀判断。
        // 同时比对原始 parent 与 canonicalize 结果，避免 macOS 上
        // `/var/...` 与 `/private/var/...` 前缀不一致导致漏判。
        return child.starts_with(parent) || child.starts_with(&parent_canon);
    };
    child_canon == parent_canon || child_canon.starts_with(&parent_canon)
}

/// `copy_path_recursive`。
pub fn copy_path_recursive(src: &Path, dest: &Path) -> Result<(), String> {
    if src.is_dir() {
        std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
        for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name();
            copy_path_recursive(&entry.path(), &dest.join(name))?;
        }
        Ok(())
    } else {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::copy(src, dest).map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// `move_path`。
pub fn move_path(src: &Path, dest: &Path) -> Result<(), String> {
    if is_same_or_subdir(src, dest) {
        return Err("不能移动到自身或其子目录".into());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    match std::fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_path_recursive(src, dest)?;
            if src.is_dir() {
                std::fs::remove_dir_all(src).map_err(|e| e.to_string())?;
            } else {
                std::fs::remove_file(src).map_err(|e| e.to_string())?;
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn unique_dest_name_no_conflict() {
        let dir = tempfile_dir();
        let p = unique_dest_name(&dir, "a.md");
        assert_eq!(p.file_name().unwrap(), "a.md");
    }

    #[test]
    fn unique_dest_name_adds_suffix() {
        let dir = tempfile_dir();
        fs::write(dir.join("a.md"), b"x").unwrap();
        let p = unique_dest_name(&dir, "a.md");
        assert_eq!(p.file_name().unwrap(), "a (1).md");
        fs::write(&p, b"y").unwrap();
        let p2 = unique_dest_name(&dir, "a.md");
        assert_eq!(p2.file_name().unwrap(), "a (2).md");
    }

    #[test]
    fn unique_dest_name_dir_no_ext() {
        let dir = tempfile_dir();
        fs::create_dir(dir.join("folder")).unwrap();
        let p = unique_dest_name(&dir, "folder");
        assert_eq!(p.file_name().unwrap(), "folder (1)");
    }

    #[test]
    fn rejects_move_into_self() {
        let dir = tempfile_dir();
        let sub = dir.join("sub");
        fs::create_dir(&sub).unwrap();
        let nested = sub.join("x");
        fs::create_dir(&nested).unwrap();
        let err = move_path(&sub, &nested.join("sub")).unwrap_err();
        assert!(err.contains("自身"));
    }

    #[test]
    fn lexically_under_accepts_child() {
        let root = PathBuf::from("/Users/me/.astro");
        assert!(is_lexically_under(
            &root,
            &PathBuf::from("/Users/me/.astro/ws/a.md")
        ));
    }

    #[test]
    fn lexically_under_rejects_parent_dir_escape() {
        let root = PathBuf::from("/Users/me/.astro");
        assert!(!is_lexically_under(
            &root,
            &PathBuf::from("/Users/me/.astro/../outside.txt")
        ));
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "astro-fs-ops-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
