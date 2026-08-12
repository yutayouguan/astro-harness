//! 沙箱内目录列举：限制在 `sandbox_root` 之下，跳过点文件与 `target`。

use std::path::{Path, PathBuf};

use proto::FileEntry;

/// 在沙箱根下列出目录条目（可递归 `max_depth`）。
///
/// # 参数
/// - `sandbox_root`：允许访问的根目录
/// - `request_path`：相对或绝对路径；空则列出根
/// - `max_depth`：递归深度（至少 1）
///
/// # 错误
/// 路径越出沙箱、无法规范化，或读目录失败。
pub fn list_directory(
    sandbox_root: &Path,
    request_path: &str,
    max_depth: i32,
) -> anyhow::Result<Vec<FileEntry>> {
    let root = sandbox_root
        .canonicalize()
        .unwrap_or_else(|_| sandbox_root.to_path_buf());
    let target = if request_path.is_empty() {
        root.clone()
    } else {
        let candidate = PathBuf::from(request_path);
        let absolute = if candidate.is_absolute() {
            candidate
        } else {
            root.join(candidate)
        };
        absolute
            .canonicalize()
            .map_err(|e| anyhow::anyhow!("路径无效: {e}"))?
    };

    if !target.starts_with(&root) {
        anyhow::bail!("禁止访问沙箱外路径");
    }

    collect_entries(&target, max_depth.max(1) as usize)
}

/// 收集一层目录项；`max_depth > 1` 时递归子目录。
fn collect_entries(dir: &Path, max_depth: usize) -> anyhow::Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    let read_dir = std::fs::read_dir(dir)?;
    for entry in read_dir.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "target" {
            continue;
        }
        let metadata = entry.metadata()?;
        let is_dir = metadata.is_dir();
        entries.push(FileEntry {
            path: path.display().to_string(),
            is_dir,
            size: if is_dir { 0 } else { metadata.len() as i64 },
        });
        if is_dir && max_depth > 1 {
            entries.extend(collect_entries(&path, max_depth - 1)?);
        }
    }
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.path.cmp(&b.path),
    });
    Ok(entries)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_list_directory_sandbox() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("MEMORY.md"), "- test").unwrap();
        std::fs::create_dir(dir.path().join("sessions")).unwrap();

        let entries = list_directory(dir.path(), "", 2).unwrap();
        assert!(entries.iter().any(|e| e.path.contains("MEMORY.md")));
    }

    #[test]
    fn test_list_directory_blocks_escape() {
        let dir = TempDir::new().unwrap();
        let err = list_directory(dir.path(), "/etc", 1).unwrap_err();
        assert!(err.to_string().contains("沙箱"));
    }
}
