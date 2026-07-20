//! 技能更新备份目录的列举与在文件管理器中打开。

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::installed::reveal_path_in_file_manager;

/// 单次技能目录备份条目（`~/.astro/skill-backups/{agent}/{folder}/{timestamp}/`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillBackupEntry {
    pub agent_id: String,
    pub folder: String,
    /// 备份目录名（Unix 时间戳字符串）。
    pub timestamp: String,
    /// 备份目录绝对路径。
    pub path: String,
    /// 目录 mtime（秒，Unix epoch）。
    pub created_at: Option<i64>,
}

fn memory_dir() -> PathBuf {
    std::env::var("ASTRO_MEMORY_DIR")
        .map(PathBuf::from)
        .or_else(|_| {
            std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .map(|h| PathBuf::from(h).join(".astro"))
        })
        .unwrap_or_else(|_| PathBuf::from(".astro"))
}

fn backups_root() -> PathBuf {
    memory_dir().join("skill-backups")
}

/// 规范化 Agent id：空/`default` → `workspace`（与 `update` / `origins` 一致）。
fn normalize_agent_id(agent_id: Option<&str>) -> String {
    match agent_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some("default") | None => "workspace".to_string(),
        Some(id) => id.to_string(),
    }
}

fn dir_mtime_secs(path: &Path) -> Option<i64> {
    fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

/// 列举备份；`agent_id` 为 `None` 时返回全部 Agent，否则按 normalize 后的 id 过滤。
/// 结果按时间戳新→旧排序。
pub fn list_skill_backups(agent_id: Option<&str>) -> Result<Vec<SkillBackupEntry>> {
    let root = backups_root();
    if !root.is_dir() {
        return Ok(Vec::new());
    }

    let filter_agent = agent_id.map(|id| normalize_agent_id(Some(id)));

    let mut entries = Vec::new();

    for agent_entry in
        fs::read_dir(&root).with_context(|| format!("read_dir {}", root.display()))?
    {
        let agent_entry = agent_entry?;
        let agent_path = agent_entry.path();
        if !agent_path.is_dir() {
            continue;
        }
        let agent_id_str = agent_entry.file_name().to_string_lossy().to_string();
        if filter_agent
            .as_ref()
            .is_some_and(|target| *target != agent_id_str)
        {
            continue;
        }

        for folder_entry in fs::read_dir(&agent_path)
            .with_context(|| format!("read_dir {}", agent_path.display()))?
        {
            let folder_entry = folder_entry?;
            let folder_path = folder_entry.path();
            if !folder_path.is_dir() {
                continue;
            }
            let folder = folder_entry.file_name().to_string_lossy().to_string();

            for ts_entry in fs::read_dir(&folder_path)
                .with_context(|| format!("read_dir {}", folder_path.display()))?
            {
                let ts_entry = ts_entry?;
                let backup_path = ts_entry.path();
                if !backup_path.is_dir() {
                    continue;
                }
                let timestamp = ts_entry.file_name().to_string_lossy().to_string();
                entries.push(SkillBackupEntry {
                    agent_id: agent_id_str.clone(),
                    folder: folder.clone(),
                    timestamp,
                    path: backup_path.to_string_lossy().to_string(),
                    created_at: dir_mtime_secs(&backup_path),
                });
            }
        }
    }

    entries.sort_by(|a, b| {
        let ta = a.timestamp.parse::<i64>().unwrap_or(0);
        let tb = b.timestamp.parse::<i64>().unwrap_or(0);
        tb.cmp(&ta)
            .then_with(|| b.created_at.unwrap_or(0).cmp(&a.created_at.unwrap_or(0)))
    });

    Ok(entries)
}

fn ensure_backup_path(path: &Path) -> Result<PathBuf> {
    let canon = path
        .canonicalize()
        .with_context(|| format!("路径不存在: {}", path.display()))?;
    let root = backups_root();
    let root_canon = if root.is_dir() {
        root.canonicalize().unwrap_or(root)
    } else {
        bail!("备份根目录不存在");
    };
    if !canon.starts_with(&root_canon) {
        bail!("路径不在备份目录内");
    }
    if !canon.is_dir() {
        bail!("不是有效的备份目录");
    }
    Ok(canon)
}

/// 在系统文件管理器中显示指定备份目录。
pub fn reveal_skill_backup(path: &str) -> Result<()> {
    let canon = ensure_backup_path(Path::new(path))?;
    reveal_path_in_file_manager(&canon)
}

#[cfg(test)]
mod tests {
    use super::*;
        use tempfile::tempdir;

    static ENV_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn mkdir_backup(base: &Path, agent: &str, folder: &str, timestamp: &str) {
        let p = base
            .join("skill-backups")
            .join(agent)
            .join(folder)
            .join(timestamp);
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("SKILL.md"), "# backup").unwrap();
    }

    #[test]
    fn list_skill_backups_filters_and_sorts() {
        let _guard = ENV_TEST_LOCK.blocking_lock();
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        mkdir_backup(dir.path(), "workspace", "skill-a", "1000");
        mkdir_backup(dir.path(), "workspace", "skill-a", "2000");
        mkdir_backup(dir.path(), "workspace", "skill-b", "1500");
        mkdir_backup(dir.path(), "other-agent", "skill-a", "3000");

        let all = list_skill_backups(None).unwrap();
        assert_eq!(all.len(), 4);

        let ws = list_skill_backups(Some("workspace")).unwrap();
        assert_eq!(ws.len(), 3);
        assert_eq!(ws[0].timestamp, "2000");
        assert_eq!(ws[0].folder, "skill-a");
        assert_eq!(ws[1].timestamp, "1500");
        assert_eq!(ws[2].timestamp, "1000");

        let default_norm = list_skill_backups(Some("default")).unwrap();
        assert_eq!(default_norm.len(), 3);

        let other = list_skill_backups(Some("other-agent")).unwrap();
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].timestamp, "3000");
    }

    #[test]
    fn list_skill_backups_empty_when_missing_root() {
        let _guard = ENV_TEST_LOCK.blocking_lock();
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        let list = list_skill_backups(Some("workspace")).unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn reveal_skill_backup_rejects_outside_root() {
        let _guard = ENV_TEST_LOCK.blocking_lock();
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        mkdir_backup(dir.path(), "workspace", "demo", "123");
        let outside = dir.path().join("outside");
        fs::create_dir_all(&outside).unwrap();

        let err = reveal_skill_backup(outside.to_str().unwrap()).unwrap_err();
        assert!(err.to_string().contains("备份"));
    }
}
