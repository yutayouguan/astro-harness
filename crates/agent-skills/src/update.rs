//! 从已记录的安装来源重新安装 / 更新本机 Skill。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::agent_id::normalize as normalize_agent_id;
use crate::check::{check_updates_for_agent, filter_outdated_folders};
use crate::install::{install_from_ref_scoped_as, scoped_skills_path, InstallOriginHint};
use crate::models::{SkillOriginRecord, SkillUpdateItemResult, UpdateSkillOpts};
use crate::origins::{find_origin, load_origins};
use crate::preview::preview_skill_update;

/// 解析本机 Astro 数据根目录（`ASTRO_MEMORY_DIR` / `~/.astro`）。
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

fn origin_hint(record: &SkillOriginRecord) -> InstallOriginHint {
    InstallOriginHint {
        name: Some(record.name.clone()),
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).with_context(|| format!("create {}", dst.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("read_dir {}", src.display()))? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path).with_context(|| {
                format!("copy {} -> {}", src_path.display(), dst_path.display())
            })?;
        }
    }
    Ok(())
}

/// 将技能目录拷贝到 `~/.astro/skill-backups/{agent}/{folder}/{timestamp}/`。
pub fn backup_skill_dir(
    agent_id: Option<&str>,
    scope: &str,
    project_root: Option<&Path>,
    folder: &str,
) -> Result<PathBuf> {
    let agent = normalize_agent_id(agent_id);
    let skills_dir = scoped_skills_path(scope, project_root)?;
    let src = skills_dir.join(folder);
    if !src.is_dir() {
        bail!("本地未找到技能目录: {folder}");
    }

    let timestamp = chrono::Utc::now().timestamp();
    let backup_root = memory_dir()
        .join("skill-backups")
        .join(&agent)
        .join(folder)
        .join(timestamp.to_string());
    copy_dir_recursive(&src, &backup_root)?;
    Ok(backup_root)
}

/// 按 folder 查找来源并重新安装，支持备份、强制覆盖与失败重试。
pub async fn update_installed_skill_ex(
    agent_id: Option<&str>,
    scope: &str,
    project_root: Option<&Path>,
    folder: &str,
    opts: UpdateSkillOpts,
) -> Result<String> {
    let record = find_origin(agent_id, scope, folder)?;
    let Some(record) = record else {
        bail!("无法追溯安装源: {folder}");
    };

    let preview = preview_skill_update(agent_id, scope, project_root, folder)?;
    if preview.has_local_changes && !opts.force {
        bail!("本地有改动，请确认后 force 更新");
    }

    if preview.has_local_changes && opts.backup_if_dirty {
        backup_skill_dir(agent_id, scope, project_root, folder)?;
    }

    let hint = origin_hint(&record);
    let install_ref = record.install_ref.clone();
    let mut retries_left = opts.max_retries;
    loop {
        match install_from_ref_scoped_as(
            &install_ref,
            agent_id,
            Some(hint.clone()),
            scope,
            project_root,
            folder,
        )
        .await
        {
            Ok(message) => return Ok(message),
            Err(_) if retries_left > 0 => {
                retries_left -= 1;
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(e) => return Err(e),
        }
    }
}

/// 本地 Agent skills 目录下是否仍存在该 folder（与安装扫描 / 更新目标一致）。
fn installed_skill_folder_exists(
    scope: &str,
    project_root: Option<&Path>,
    folder: &str,
) -> Result<bool> {
    let skills_dir = scoped_skills_path(scope, project_root)?;
    Ok(skills_dir.join(folder).is_dir())
}

async fn update_folders_serial(
    agent_id: Option<&str>,
    scope: &str,
    project_root: Option<&Path>,
    folders: Vec<String>,
    skip_missing_local: bool,
) -> Result<Vec<SkillUpdateItemResult>> {
    let mut results = Vec::with_capacity(folders.len());
    for folder in folders {
        let item = if skip_missing_local {
            match installed_skill_folder_exists(scope, project_root, &folder) {
                Ok(false) => SkillUpdateItemResult {
                    folder,
                    ok: false,
                    message: "本地未找到技能目录，已跳过".to_string(),
                },
                Ok(true) => match update_installed_skill_ex(
                    agent_id,
                    scope,
                    project_root,
                    &folder,
                    UpdateSkillOpts {
                        backup_if_dirty: true,
                        force: true,
                        max_retries: 1,
                    },
                )
                .await
                {
                    Ok(message) => SkillUpdateItemResult {
                        folder,
                        ok: true,
                        message,
                    },
                    Err(e) => SkillUpdateItemResult {
                        folder,
                        ok: false,
                        message: e.to_string(),
                    },
                },
                Err(e) => SkillUpdateItemResult {
                    folder,
                    ok: false,
                    message: e.to_string(),
                },
            }
        } else {
            match update_installed_skill_ex(
                agent_id,
                scope,
                project_root,
                &folder,
                UpdateSkillOpts {
                    backup_if_dirty: true,
                    force: true,
                    max_retries: 1,
                },
            )
            .await
            {
                Ok(message) => SkillUpdateItemResult {
                    folder,
                    ok: true,
                    message,
                },
                Err(e) => SkillUpdateItemResult {
                    folder,
                    ok: false,
                    message: e.to_string(),
                },
            }
        };
        results.push(item);
    }
    Ok(results)
}

/// 串行更新当前 Agent 下所有有来源记录的技能；单条失败写入结果 Vec，不中断。
pub async fn update_all_with_origin(
    agent_id: Option<&str>,
    scope: &str,
    project_root: Option<&Path>,
) -> Result<Vec<SkillUpdateItemResult>> {
    let target = normalize_agent_id(agent_id);
    let file = load_origins()?;
    let folders: Vec<String> = file
        .records
        .iter()
        .filter(|r| {
            normalize_agent_id(r.agent_id.as_deref()) == target && r.scope.as_deref() == Some(scope)
        })
        .map(|r| r.folder.clone())
        .collect();

    update_folders_serial(agent_id, scope, project_root, folders, true).await
}

/// 先检查更新状态，仅对 Outdated 技能串行重装。
pub async fn update_outdated_skills(
    agent_id: Option<&str>,
    scope: &str,
    project_root: Option<&Path>,
) -> Result<Vec<SkillUpdateItemResult>> {
    let check_results = check_updates_for_agent(agent_id, scope, project_root).await?;
    let folders = filter_outdated_folders(&check_results);
    update_folders_serial(agent_id, scope, project_root, folders, false).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::digest::skill_content_digest;
    use crate::install::scoped_skills_dir;
    use crate::models::SkillOriginRecord;
    use crate::origins::upsert_origin;
    use crate::ENV_TEST_LOCK;
    use std::io::Write;
    use tempfile::tempdir;

    fn write_skill_md(dir: &Path, body: &str) {
        fs::create_dir_all(dir).unwrap();
        let mut f = fs::File::create(dir.join("SKILL.md")).unwrap();
        writeln!(f, "{body}").unwrap();
    }

    fn upsert_test_origin(folder: &str, content_digest: Option<String>) {
        upsert_origin(SkillOriginRecord {
            folder: folder.into(),
            skill_id: None,
            name: "demo".into(),
            store: "skillhub".into(),
            install_ref: "skillhub:owner/demo".into(),
            agent_id: Some("workspace".into()),
            scope: Some("global".into()),
            installed_at: 1,
            last_updated_at: None,
            remote_version: None,
            remote_updated_at: None,
            content_digest,
        })
        .unwrap();
    }

    #[test]
    fn backup_skill_dir_creates_copy() {
        let _guard = ENV_TEST_LOCK.blocking_lock();
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        let skills_dir = scoped_skills_dir("global", None).unwrap();
        let skill_dir = skills_dir.join("demo-skill");
        write_skill_md(&skill_dir, "# Demo");
        fs::write(skill_dir.join("extra.txt"), "payload").unwrap();

        let backup_path =
            backup_skill_dir(Some("workspace"), "global", None, "demo-skill").unwrap();
        assert!(backup_path.is_dir());
        assert!(backup_path.join("SKILL.md").is_file());
        assert!(backup_path.join("extra.txt").is_file());
        assert_eq!(
            fs::read_to_string(backup_path.join("extra.txt")).unwrap(),
            "payload"
        );
        assert!(backup_path
            .components()
            .any(|c| c.as_os_str() == "skill-backups"));
    }

    #[tokio::test]
    async fn update_ex_dirty_without_force_errors() {
        let _guard = ENV_TEST_LOCK.lock().await;
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        let skills_dir = scoped_skills_dir("global", None).unwrap();
        let skill_dir = skills_dir.join("demo-skill");
        write_skill_md(&skill_dir, "# Changed locally");
        let current = skill_content_digest(&skill_dir).unwrap();

        upsert_test_origin("demo-skill", Some("stale-baseline-digest".into()));
        assert_ne!(current, "stale-baseline-digest");

        let err = update_installed_skill_ex(
            Some("workspace"),
            "global",
            None,
            "demo-skill",
            UpdateSkillOpts {
                backup_if_dirty: true,
                force: false,
                max_retries: 0,
            },
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("本地有改动"));
    }

    #[tokio::test]
    async fn update_all_skips_missing_local_folder() {
        let _guard = ENV_TEST_LOCK.lock().await;
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        upsert_origin(SkillOriginRecord {
            folder: "ghost-skill".into(),
            skill_id: None,
            name: "ghost".into(),
            store: "skillhub".into(),
            install_ref: "skillhub:owner/ghost".into(),
            agent_id: Some("workspace".into()),
            scope: Some("global".into()),
            installed_at: 1,
            last_updated_at: None,
            remote_version: None,
            remote_updated_at: None,
            content_digest: None,
        })
        .unwrap();

        let results = update_all_with_origin(Some("workspace"), "global", None)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert!(!results[0].ok);
        assert_eq!(results[0].folder, "ghost-skill");
        assert!(results[0].message.contains("本地未找到技能目录"));
    }

    #[tokio::test]
    async fn update_without_origin_errors() {
        let _guard = ENV_TEST_LOCK.lock().await;
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        let err = update_installed_skill_ex(
            Some("workspace"),
            "global",
            None,
            "missing",
            UpdateSkillOpts {
                backup_if_dirty: true,
                force: true,
                max_retries: 1,
            },
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("无法追溯"));
    }

    #[tokio::test]
    async fn update_all_empty_ok() {
        let _guard = ENV_TEST_LOCK.lock().await;
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        let r = update_all_with_origin(Some("workspace"), "global", None)
            .await
            .unwrap();
        assert!(r.is_empty());
    }

    #[tokio::test]
    async fn update_outdated_skills_empty_when_none_outdated() {
        let _guard = ENV_TEST_LOCK.lock().await;
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        let r = update_outdated_skills(Some("workspace"), "global", None)
            .await
            .unwrap();
        assert!(r.is_empty());
    }
}
