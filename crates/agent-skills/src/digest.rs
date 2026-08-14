//! 技能目录内容 digest：用于检测本地改动。

use std::{
    fs,
    io::Read,
    path::{Component, Path},
};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

/// 对技能根目录下常规文件计算稳定 digest。
///
/// - 跳过以 `.` 开头的文件/目录
/// - 相对路径统一用 `/`
/// - 按 path 字典序排序
/// - 对每个文件先算 content sha256，再拼 manifest 行 `path\0content_hash\n`，最后对 manifest 做 sha256 hex
pub fn skill_content_digest(skill_root: &Path) -> Result<String> {
    let mut entries = Vec::new();
    collect_files(skill_root, skill_root, &mut entries)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut manifest = Vec::new();
    for (path, content_hash) in &entries {
        manifest.extend_from_slice(path.as_bytes());
        manifest.push(0);
        manifest.extend_from_slice(content_hash.as_bytes());
        manifest.push(b'\n');
    }

    Ok(hex_sha256(&manifest))
}

fn collect_files(skill_root: &Path, dir: &Path, out: &mut Vec<(String, String)>) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .with_context(|| format!("read_dir {}", dir.display()))?
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }

        let path = entry.path();
        if path.is_dir() {
            collect_files(skill_root, &path, out)?;
            continue;
        }
        if !path.is_file() {
            continue;
        }

        let rel = path
            .strip_prefix(skill_root)
            .with_context(|| format!("strip_prefix {}", path.display()))?;
        let relative_path = rel
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => Some(s.to_string_lossy()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/");
        if relative_path.is_empty() {
            continue;
        }

        let content_hash = file_content_sha256(&path)?;
        out.push((relative_path, content_hash));
    }

    Ok(())
}

fn file_content_sha256(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file
            .read(&mut buf)
            .with_context(|| format!("read {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex_sha256(&hasher.finalize()))
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const EMPTY_DIR_DIGEST: &str =
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn empty_directory_has_deterministic_digest() {
        let dir = tempdir().unwrap();
        let digest = skill_content_digest(dir.path()).unwrap();
        assert_eq!(digest, EMPTY_DIR_DIGEST);
        assert_eq!(skill_content_digest(dir.path()).unwrap(), digest);
    }

    #[test]
    fn two_files_produce_stable_digest() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("b.txt"), "beta").unwrap();
        fs::write(dir.path().join("a.txt"), "alpha").unwrap();

        let first = skill_content_digest(dir.path()).unwrap();
        let second = skill_content_digest(dir.path()).unwrap();
        assert_eq!(first, second);
        assert_ne!(first, EMPTY_DIR_DIGEST);
    }

    #[test]
    fn changing_one_file_changes_digest() {
        let dir = tempdir().unwrap();
        let file_a = dir.path().join("a.txt");
        let file_b = dir.path().join("b.txt");
        fs::write(&file_a, "alpha").unwrap();
        fs::write(&file_b, "beta").unwrap();

        let before = skill_content_digest(dir.path()).unwrap();
        fs::write(&file_a, "alpha-updated").unwrap();
        let after = skill_content_digest(dir.path()).unwrap();

        assert_ne!(before, after);
    }

    #[test]
    fn skips_dotfiles_and_dot_dirs() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("visible.txt"), "ok").unwrap();
        fs::write(dir.path().join(".hidden"), "secret").unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".git/config"), "ignored").unwrap();

        let only_visible = tempdir().unwrap();
        fs::write(only_visible.path().join("visible.txt"), "ok").unwrap();

        assert_eq!(
            skill_content_digest(dir.path()).unwrap(),
            skill_content_digest(only_visible.path()).unwrap()
        );
    }

    #[test]
    fn nested_paths_use_forward_slashes() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("refs")).unwrap();
        fs::write(dir.path().join("refs/doc.md"), "# doc").unwrap();

        let digest = skill_content_digest(dir.path()).unwrap();
        assert_ne!(digest, EMPTY_DIR_DIGEST);
        assert_eq!(skill_content_digest(dir.path()).unwrap(), digest);
    }

    #[test]
    fn path_order_is_stable() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("z.txt"), "z").unwrap();
        fs::write(dir.path().join("a.txt"), "a").unwrap();

        let digest = skill_content_digest(dir.path()).unwrap();

        let reversed = tempdir().unwrap();
        fs::write(reversed.path().join("a.txt"), "a").unwrap();
        fs::write(reversed.path().join("z.txt"), "z").unwrap();

        assert_eq!(digest, skill_content_digest(reversed.path()).unwrap());
    }
}
