//! [P1] Skill 快照环：审批前自动保存历史版本，支持一键回滚。
//!
//! 快照存于 `{skill_dir}/.snapshots/`，文件名为 RFC3339 时间戳（冒号替换为连字符）。
//! 上限 5 个，超出时删除最旧的。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const SNAPSHOTS_DIR: &str = ".snapshots";
const MAX_SNAPSHOTS: usize = 5;

/// 单个快照的元数据（不含完整内容，减少 IPC 负担）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSnapshot {
    /// RFC3339 时间戳（以连字符替换冒号，用作文件名）。
    pub timestamp: String,
    /// 前 120 字符的预览（用于 UI 区分）。
    pub preview: String,
    /// 内容字节数。
    pub bytes: usize,
}

/// 快照目录路径：`{skill_dir}/.snapshots/`。
pub fn snapshots_dir(skill_dir: &Path) -> PathBuf {
    skill_dir.join(SNAPSHOTS_DIR)
}

/// 在 `SKILL.md` 被覆写之前调用，将当前内容保存到快照目录。
///
/// - 若 SKILL.md 不存在则静默跳过（不算错误）。
/// - 写入成功后自动修剪到 MAX_SNAPSHOTS。
pub fn save_snapshot(skill_dir: &Path) -> anyhow::Result<()> {
    let skill_file = skill_dir.join("SKILL.md");
    if !skill_file.is_file() {
        return Ok(()); // 没有当前版本，无需保存
    }
    let content = fs::read_to_string(&skill_file)?;
    let snap_dir = snapshots_dir(skill_dir);
    fs::create_dir_all(&snap_dir)?;

    let ts = chrono::Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        .replace(':', "-");
    let snap_path = snap_dir.join(format!("{ts}.md"));
    // 避免同秒内覆盖（极罕见场景）
    if snap_path.exists() {
        return Ok(());
    }
    fs::write(&snap_path, content)?;
    prune_snapshots(&snap_dir)?;
    Ok(())
}

/// 返回按时间戳倒序排列的快照列表（最新在前）。
pub fn list_snapshots(skill_dir: &Path) -> Vec<SkillSnapshot> {
    let snap_dir = snapshots_dir(skill_dir);
    let Ok(entries) = fs::read_dir(&snap_dir) else {
        return Vec::new();
    };
    let mut snaps: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_str()?.to_string();
            if path.extension().and_then(|x| x.to_str()) == Some("md") {
                let ts = name.trim_end_matches(".md").to_string();
                Some((ts, path))
            } else {
                None
            }
        })
        .collect();
    snaps.sort_by(|a, b| b.0.cmp(&a.0)); // 倒序
    snaps
        .into_iter()
        .filter_map(|(ts, path)| {
            let content = fs::read_to_string(&path).ok()?;
            let preview: String = content.chars().take(120).collect();
            Some(SkillSnapshot {
                timestamp: ts,
                preview,
                bytes: content.len(),
            })
        })
        .collect()
}

/// 恢复最新快照（覆盖 SKILL.md）。
///
/// - 返回 `Ok(true)` 表示恢复成功；`Ok(false)` 表示无快照可恢复。
pub fn restore_latest(skill_dir: &Path) -> anyhow::Result<bool> {
    let snap_dir = snapshots_dir(skill_dir);
    let Ok(entries) = fs::read_dir(&snap_dir) else {
        return Ok(false);
    };
    let mut snaps: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_str()?.to_string();
            if path.extension().and_then(|x| x.to_str()) == Some("md") {
                let ts = name.trim_end_matches(".md").to_string();
                Some((ts, path))
            } else {
                None
            }
        })
        .collect();
    if snaps.is_empty() {
        return Ok(false);
    }
    snaps.sort_by(|a, b| b.0.cmp(&a.0));
    let (_, latest_path) = &snaps[0];
    let content = fs::read_to_string(latest_path)?;
    let skill_file = skill_dir.join("SKILL.md");
    fs::write(&skill_file, content)?;
    Ok(true)
}

/// 修剪快照目录，保留最新 MAX_SNAPSHOTS 个，删除多余的旧快照。
fn prune_snapshots(snap_dir: &Path) -> anyhow::Result<()> {
    let Ok(entries) = fs::read_dir(snap_dir) else {
        return Ok(());
    };
    let mut snaps: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_str()?.to_string();
            if path.extension().and_then(|x| x.to_str()) == Some("md") {
                let ts = name.trim_end_matches(".md").to_string();
                Some((ts, path))
            } else {
                None
            }
        })
        .collect();
    if snaps.len() <= MAX_SNAPSHOTS {
        return Ok(());
    }
    snaps.sort_by(|a, b| b.0.cmp(&a.0)); // 最新在前
    for (_, path) in snaps.iter().skip(MAX_SNAPSHOTS) {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_skill(dir: &Path, content: &str) {
        fs::write(dir.join("SKILL.md"), content).unwrap();
    }

    #[test]
    fn save_and_list_snapshot() {
        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path();
        write_skill(skill_dir, "# v1\n内容一");
        save_snapshot(skill_dir).unwrap();
        let snaps = list_snapshots(skill_dir);
        assert_eq!(snaps.len(), 1);
        assert!(snaps[0].preview.contains("v1"));
    }

    #[test]
    fn restore_latest_overwrites_skill_md() {
        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path();
        write_skill(skill_dir, "# 旧版本");
        save_snapshot(skill_dir).unwrap();
        write_skill(skill_dir, "# 新版本");
        let restored = restore_latest(skill_dir).unwrap();
        assert!(restored);
        let content = fs::read_to_string(skill_dir.join("SKILL.md")).unwrap();
        assert!(content.contains("旧版本"));
    }

    #[test]
    fn prunes_to_max_snapshots() {
        let dir = TempDir::new().unwrap();
        let skill_dir = dir.path();
        let snap_dir = snapshots_dir(skill_dir);
        fs::create_dir_all(&snap_dir).unwrap();
        // 手动写入 7 个假快照
        for i in 0..7 {
            let ts = format!("2026-07-{:02}T00-00-00Z", i + 1);
            fs::write(snap_dir.join(format!("{ts}.md")), format!("v{i}")).unwrap();
        }
        prune_snapshots(&snap_dir).unwrap();
        let count = fs::read_dir(&snap_dir).unwrap().count();
        assert_eq!(count, MAX_SNAPSHOTS);
    }

    #[test]
    fn restore_latest_no_snapshots_returns_false() {
        let dir = TempDir::new().unwrap();
        let restored = restore_latest(dir.path()).unwrap();
        assert!(!restored);
    }
}
