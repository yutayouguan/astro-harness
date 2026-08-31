//! 技能安装来源持久化（`skill-origins.json`）。

use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::agent_id::normalize as normalize_agent_id;
use crate::digest::skill_content_digest;
use crate::install::{is_safe_skill_folder, is_skillhub_http_ref};
use crate::models::{SkillOriginRecord, SkillOriginsFile, StoreSkill, StoreSkillDetail};
use crate::seed::BUNDLED_SKILLHUB_SOURCES;
use crate::store::fetch_detail_strict;

const ORIGINS_FILE: &str = "skill-origins.json";
const ORIGINS_VERSION: u32 = 3;

/// 解析本机 Astro 数据根目录。
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

fn valid_scope(scope: Option<&str>) -> Option<&str> {
    match scope.map(str::trim) {
        Some("global") => Some("global"),
        Some("project") => Some("project"),
        _ => None,
    }
}

fn origin_key(agent_id: Option<&str>, scope: &str, folder: &str) -> (String, String, String) {
    (
        normalize_agent_id(agent_id),
        scope.to_string(),
        folder.to_string(),
    )
}

/// `skill-origins.json` 路径。
pub fn origins_path() -> PathBuf {
    memory_dir().join(ORIGINS_FILE)
}

/// 读取来源清单；缺失或空文件时返回默认空清单。
pub fn load_origins() -> Result<SkillOriginsFile> {
    let path = origins_path();
    if !path.exists() {
        return Ok(SkillOriginsFile {
            version: ORIGINS_VERSION,
            records: Vec::new(),
        });
    }
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(SkillOriginsFile {
            version: ORIGINS_VERSION,
            records: Vec::new(),
        });
    }
    let mut file: SkillOriginsFile =
        serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    let original_len = file.records.len();
    file.records.retain(|record| {
        record.store.eq_ignore_ascii_case("skillhub")
            && is_skillhub_http_ref(&record.install_ref)
            && is_safe_skill_folder(&record.folder)
            && valid_scope(record.scope.as_deref()).is_some()
    });
    let needs_migration = file.version != ORIGINS_VERSION || file.records.len() != original_len;
    file.version = ORIGINS_VERSION;
    if needs_migration {
        save_origins(&file)?;
    }
    Ok(file)
}

/// 写入来源清单。
pub fn save_origins(file: &SkillOriginsFile) -> Result<()> {
    let path = origins_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(file)?;
    fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}

/// 为可由 SkillHub 更新的内置 Skill 建立一次性基线。
///
/// 用户已安装或更新过的来源记录优先，不会被内置基线覆盖。
pub fn ensure_bundled_skillhub_origins(agent_id: Option<&str>) -> Result<()> {
    let agent_id = normalize_agent_id(agent_id);
    let skills_dir = memory_dir().join("skills");
    let mut file = load_origins()?;
    let mut changed = false;

    for source in BUNDLED_SKILLHUB_SOURCES {
        let skill_dir = skills_dir.join(source.folder);
        if !skill_dir.join("SKILL.md").is_file() {
            continue;
        }
        let key = origin_key(Some(&agent_id), "global", source.folder);
        if file.records.iter().any(|record| {
            valid_scope(record.scope.as_deref())
                .map(|scope| origin_key(record.agent_id.as_deref(), scope, &record.folder))
                .as_ref()
                == Some(&key)
        }) {
            continue;
        }
        file.records.push(SkillOriginRecord {
            folder: source.folder.to_string(),
            skill_id: Some(source.install_ref.to_string()),
            name: source.folder.to_string(),
            store: "skillhub".to_string(),
            install_ref: source.install_ref.to_string(),
            agent_id: Some(agent_id.clone()),
            scope: Some("global".to_string()),
            installed_at: chrono::Utc::now().timestamp(),
            last_updated_at: None,
            remote_version: Some(source.version.to_string()),
            remote_updated_at: Some(source.updated_at),
            content_digest: skill_content_digest(&skill_dir).ok(),
        });
        changed = true;
    }

    if changed {
        save_origins(&file)?;
    }
    Ok(())
}

/// 按 `(agent_id, scope, folder)` 插入或更新。
pub fn upsert_origin(mut record: SkillOriginRecord) -> Result<()> {
    if !record.store.eq_ignore_ascii_case("skillhub")
        || !is_skillhub_http_ref(&record.install_ref)
        || !is_safe_skill_folder(&record.folder)
    {
        bail!("仅支持记录 SkillHub 安装来源");
    }
    let scope = valid_scope(record.scope.as_deref())
        .ok_or_else(|| anyhow::anyhow!("SkillHub 安装来源缺少有效 scope"))?
        .to_string();
    record.store = "skillhub".to_string();
    record.agent_id = Some(normalize_agent_id(record.agent_id.as_deref()));
    record.scope = Some(scope.clone());
    let key = origin_key(record.agent_id.as_deref(), &scope, &record.folder);
    let mut file = load_origins()?;
    if let Some(existing) = file.records.iter_mut().find(|r| {
        valid_scope(r.scope.as_deref())
            .map(|record_scope| origin_key(r.agent_id.as_deref(), record_scope, &r.folder))
            .as_ref()
            == Some(&key)
    }) {
        *existing = record;
    } else {
        file.records.push(record);
    }
    save_origins(&file)
}

/// 按 Agent、作用域与文件夹名查找来源记录。
pub fn find_origin(
    agent_id: Option<&str>,
    scope: &str,
    folder: &str,
) -> Result<Option<SkillOriginRecord>> {
    let scope =
        valid_scope(Some(scope)).ok_or_else(|| anyhow::anyhow!("无效 Skill scope: {scope}"))?;
    let key = origin_key(agent_id, scope, folder);
    let file = load_origins()?;
    Ok(file.records.into_iter().find(|r| {
        valid_scope(r.scope.as_deref())
            .map(|record_scope| origin_key(r.agent_id.as_deref(), record_scope, &r.folder))
            .as_ref()
            == Some(&key)
    }))
}

fn derive_store_skill_id(origin: &SkillOriginRecord) -> String {
    let r = origin.install_ref.trim();
    if let Some(rest) = r.strip_prefix("skillhub:") {
        return format!("skillhub:{rest}");
    }
    format!("skillhub:{}", origin.folder)
}

fn derive_source(install_ref: &str, store: &str) -> String {
    let r = install_ref.trim();
    if let Some(rest) = r.strip_prefix("skillhub:") {
        if let Some((owner, _)) = rest.split_once('/') {
            return owner.to_string();
        }
    }
    store.to_string()
}

/// 从 origin 记录构造商店查询用的 `StoreSkill`（供 `fetch_detail`）。
pub fn origin_to_store_skill(origin: &SkillOriginRecord) -> StoreSkill {
    let id = origin
        .skill_id
        .clone()
        .unwrap_or_else(|| derive_store_skill_id(origin));
    StoreSkill {
        id,
        name: origin.name.clone(),
        description: String::new(),
        source: derive_source(&origin.install_ref, &origin.store),
        store: "skillhub".to_string(),
        installs: None,
        install_ref: origin.install_ref.clone(),
        homepage: None,
        icon_url: None,
        category: None,
        requires_api_key: None,
    }
}

/// 用远端详情填充 origin 的 baseline `remote_*`，保留其余字段（含 `installed_at`）。
pub fn origin_with_remote_baseline(
    origin: &SkillOriginRecord,
    detail: &StoreSkillDetail,
) -> SkillOriginRecord {
    SkillOriginRecord {
        remote_version: detail.version.clone(),
        remote_updated_at: detail.updated_at,
        ..origin.clone()
    }
}

/// 安装成功后 best-effort 拉取远端元数据并写回 origin baseline；fetch 失败不报错。
pub async fn fill_origin_remote_baseline(
    agent_id: Option<&str>,
    scope: &str,
    folder: &str,
) -> Result<()> {
    let Some(origin) = find_origin(agent_id, scope, folder)? else {
        return Ok(());
    };
    let store_skill = origin_to_store_skill(&origin);
    match fetch_detail_strict(&store_skill).await {
        Ok(detail) => {
            if let Err(e) = upsert_origin(origin_with_remote_baseline(&origin, &detail)) {
                tracing::debug!(
                    folder = %folder,
                    error = %e,
                    "upsert remote baseline after install failed; continuing"
                );
            }
        }
        Err(e) => {
            tracing::debug!(
                folder = %folder,
                error = %e,
                "fetch remote baseline after install failed; continuing"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SkillOriginRecord;
    use crate::seed::seed_bundled_into;
    use crate::ENV_TEST_LOCK;
    use tempfile::tempdir;

    #[test]
    fn ensure_bundled_skillhub_origins_adds_aihot_once() {
        let _guard = ENV_TEST_LOCK.blocking_lock();
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        seed_bundled_into(dir.path());

        ensure_bundled_skillhub_origins(Some("workspace")).unwrap();
        ensure_bundled_skillhub_origins(Some("workspace")).unwrap();

        let file = load_origins().unwrap();
        assert_eq!(file.records.len(), 1);
        let origin = &file.records[0];
        assert_eq!(origin.folder, "aihot");
        assert_eq!(origin.skill_id.as_deref(), Some("skillhub:kkkkhazix/aihot"));
        assert_eq!(origin.install_ref, "skillhub:kkkkhazix/aihot");
        assert_eq!(origin.agent_id.as_deref(), Some("default"));
        assert_eq!(origin.scope.as_deref(), Some("global"));
        assert_eq!(origin.remote_version.as_deref(), Some("0.1.1"));
        assert_eq!(origin.remote_updated_at, Some(1_788_148_472_699));
        assert!(origin.content_digest.is_some());
    }

    #[test]
    fn ensure_bundled_skillhub_origins_preserves_existing_origin() {
        let _guard = ENV_TEST_LOCK.blocking_lock();
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        seed_bundled_into(dir.path());
        upsert_origin(SkillOriginRecord {
            folder: "aihot".into(),
            skill_id: Some("skillhub:custom/aihot".into()),
            name: "Custom aihot".into(),
            store: "skillhub".into(),
            install_ref: "skillhub:custom/aihot".into(),
            agent_id: Some("workspace".into()),
            scope: Some("global".into()),
            installed_at: 7,
            last_updated_at: Some(8),
            remote_version: Some("9.9.9".into()),
            remote_updated_at: Some(10),
            content_digest: Some("custom-digest".into()),
        })
        .unwrap();

        ensure_bundled_skillhub_origins(Some("workspace")).unwrap();

        let origin = find_origin(Some("workspace"), "global", "aihot")
            .unwrap()
            .unwrap();
        assert_eq!(origin.install_ref, "skillhub:custom/aihot");
        assert_eq!(origin.remote_version.as_deref(), Some("9.9.9"));
        assert_eq!(origin.content_digest.as_deref(), Some("custom-digest"));
        assert_eq!(load_origins().unwrap().records.len(), 1);
    }

    #[test]
    fn upsert_same_folder_updates_not_duplicates() {
        let _guard = ENV_TEST_LOCK.blocking_lock();
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        upsert_origin(SkillOriginRecord {
            folder: "ppt-generator-skill".into(),
            skill_id: None,
            name: "a".into(),
            store: "skillhub".into(),
            install_ref: "skillhub:x/ppt-generator-skill".into(),
            agent_id: Some("workspace".into()),
            scope: Some("global".into()),
            installed_at: 1,
            last_updated_at: None,
            remote_version: None,
            remote_updated_at: None,
            content_digest: None,
        })
        .unwrap();
        upsert_origin(SkillOriginRecord {
            folder: "ppt-generator-skill".into(),
            skill_id: None,
            name: "a".into(),
            store: "skillhub".into(),
            install_ref: "skillhub:x/ppt-generator-skill".into(),
            agent_id: Some("workspace".into()),
            scope: Some("global".into()),
            installed_at: 1,
            last_updated_at: Some(2),
            remote_version: None,
            remote_updated_at: None,
            content_digest: None,
        })
        .unwrap();
        let file = load_origins().unwrap();
        assert_eq!(file.records.len(), 1);
        assert_eq!(file.records[0].last_updated_at, Some(2));
    }

    #[test]
    fn same_folder_in_global_and_project_keeps_distinct_origins() {
        let _guard = ENV_TEST_LOCK.blocking_lock();
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        for scope in ["global", "project"] {
            upsert_origin(SkillOriginRecord {
                folder: "demo".into(),
                skill_id: None,
                name: "demo".into(),
                store: "skillhub".into(),
                install_ref: "skillhub:owner/demo".into(),
                agent_id: Some("default".into()),
                scope: Some(scope.into()),
                installed_at: 1,
                last_updated_at: None,
                remote_version: None,
                remote_updated_at: None,
                content_digest: None,
            })
            .unwrap();
        }

        let file = load_origins().unwrap();
        assert_eq!(file.records.len(), 2);
        assert!(find_origin(Some("default"), "global", "demo")
            .unwrap()
            .is_some());
        assert!(find_origin(Some("default"), "project", "demo")
            .unwrap()
            .is_some());
    }

    #[test]
    fn missing_file_returns_empty_origins() {
        let _guard = ENV_TEST_LOCK.blocking_lock();
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        let file = load_origins().unwrap();
        assert_eq!(file.version, ORIGINS_VERSION);
        assert!(file.records.is_empty());
    }

    #[test]
    fn upsert_default_agent_matches_legacy_workspace_alias() {
        let _guard = ENV_TEST_LOCK.blocking_lock();
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        upsert_origin(SkillOriginRecord {
            folder: "demo".into(),
            skill_id: None,
            name: "demo".into(),
            store: "skillhub".into(),
            install_ref: "skillhub:x/demo".into(),
            agent_id: None,
            scope: Some("global".into()),
            installed_at: 1,
            last_updated_at: None,
            remote_version: None,
            remote_updated_at: None,
            content_digest: None,
        })
        .unwrap();
        upsert_origin(SkillOriginRecord {
            folder: "demo".into(),
            skill_id: None,
            name: "demo".into(),
            store: "skillhub".into(),
            install_ref: "skillhub:x/demo".into(),
            agent_id: Some("workspace".into()),
            scope: Some("global".into()),
            installed_at: 1,
            last_updated_at: Some(9),
            remote_version: None,
            remote_updated_at: None,
            content_digest: None,
        })
        .unwrap();

        let file = load_origins().unwrap();
        assert_eq!(file.records.len(), 1);
        assert_eq!(file.records[0].agent_id.as_deref(), Some("default"));
        assert_eq!(file.records[0].last_updated_at, Some(9));
    }

    #[test]
    fn load_origins_removes_non_skillhub_history() {
        let _guard = ENV_TEST_LOCK.blocking_lock();
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        save_origins(&SkillOriginsFile {
            version: 1,
            records: vec![
                SkillOriginRecord {
                    folder: "keep".into(),
                    skill_id: None,
                    name: "keep".into(),
                    store: "skillhub".into(),
                    install_ref: "skillhub:owner/keep".into(),
                    agent_id: None,
                    scope: Some("global".into()),
                    installed_at: 1,
                    last_updated_at: None,
                    remote_version: None,
                    remote_updated_at: None,
                    content_digest: None,
                },
                SkillOriginRecord {
                    folder: "remove".into(),
                    skill_id: None,
                    name: "remove".into(),
                    store: "legacy-market".into(),
                    install_ref: "legacy:owner/remove".into(),
                    agent_id: None,
                    scope: Some("global".into()),
                    installed_at: 1,
                    last_updated_at: None,
                    remote_version: None,
                    remote_updated_at: None,
                    content_digest: None,
                },
                SkillOriginRecord {
                    folder: "remove-missing-scope".into(),
                    skill_id: None,
                    name: "remove-missing-scope".into(),
                    store: "skillhub".into(),
                    install_ref: "skillhub:owner/remove-missing-scope".into(),
                    agent_id: None,
                    scope: None,
                    installed_at: 1,
                    last_updated_at: None,
                    remote_version: None,
                    remote_updated_at: None,
                    content_digest: None,
                },
                SkillOriginRecord {
                    folder: "..".into(),
                    skill_id: None,
                    name: "remove-unsafe-folder".into(),
                    store: "skillhub".into(),
                    install_ref: "skillhub:owner/remove-unsafe-folder".into(),
                    agent_id: None,
                    scope: Some("global".into()),
                    installed_at: 1,
                    last_updated_at: None,
                    remote_version: None,
                    remote_updated_at: None,
                    content_digest: None,
                },
            ],
        })
        .unwrap();

        let file = load_origins().unwrap();
        assert_eq!(file.records.len(), 1);
        assert_eq!(file.records[0].folder, "keep");

        let persisted: SkillOriginsFile =
            serde_json::from_str(&fs::read_to_string(origins_path()).unwrap()).unwrap();
        assert_eq!(persisted.version, ORIGINS_VERSION);
        assert_eq!(persisted.records.len(), 1);
    }

    fn sample_detail(version: Option<&str>, updated_at: Option<i64>) -> StoreSkillDetail {
        StoreSkillDetail {
            name: "Demo".into(),
            slug: "demo-skill".into(),
            description: String::new(),
            overview: String::new(),
            source: "owner".into(),
            store: "skillhub".into(),
            installs: None,
            downloads: None,
            stars: None,
            install_ref: "skillhub:owner/demo-skill".into(),
            homepage: None,
            detail_url: "https://skillhub.cn/skills/demo-skill".into(),
            icon_url: None,
            category: None,
            sub_categories: vec![],
            version: version.map(str::to_string),
            updated_at,
            owner_name: None,
            verified: None,
        }
    }

    #[test]
    fn origin_with_remote_baseline_preserves_installed_at() {
        let origin = SkillOriginRecord {
            folder: "demo-skill".into(),
            skill_id: None,
            name: "Demo".into(),
            store: "skillhub".into(),
            install_ref: "skillhub:owner/demo-skill".into(),
            agent_id: Some("workspace".into()),
            scope: Some("global".into()),
            installed_at: 42,
            last_updated_at: None,
            remote_version: None,
            remote_updated_at: None,
            content_digest: None,
        };
        let detail = sample_detail(Some("1.2.3"), Some(999));
        let updated = origin_with_remote_baseline(&origin, &detail);
        assert_eq!(updated.installed_at, 42);
        assert_eq!(updated.remote_version.as_deref(), Some("1.2.3"));
        assert_eq!(updated.remote_updated_at, Some(999));
    }

    #[test]
    fn origin_with_remote_baseline_empty_detail_leaves_remote_none() {
        let origin = SkillOriginRecord {
            folder: "demo-skill".into(),
            skill_id: None,
            name: "Demo".into(),
            store: "skillhub".into(),
            install_ref: "skillhub:owner/demo-skill".into(),
            agent_id: Some("workspace".into()),
            scope: Some("global".into()),
            installed_at: 1,
            last_updated_at: None,
            remote_version: None,
            remote_updated_at: None,
            content_digest: None,
        };
        let detail = sample_detail(None, None);
        let updated = origin_with_remote_baseline(&origin, &detail);
        assert!(updated.remote_version.is_none());
        assert!(updated.remote_updated_at.is_none());
    }

    #[tokio::test]
    async fn fill_origin_remote_baseline_fetch_failure_leaves_remote_none() {
        let _guard = ENV_TEST_LOCK.lock().await;
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        upsert_origin(SkillOriginRecord {
            folder: "demo-skill".into(),
            skill_id: None,
            name: "Demo".into(),
            store: "skillhub".into(),
            install_ref: "skillhub:nonexistent-slug-xyz".into(),
            agent_id: Some("workspace".into()),
            scope: Some("global".into()),
            installed_at: 1,
            last_updated_at: None,
            remote_version: None,
            remote_updated_at: None,
            content_digest: None,
        })
        .unwrap();

        fill_origin_remote_baseline(Some("workspace"), "global", "demo-skill")
            .await
            .unwrap();

        let o = find_origin(Some("workspace"), "global", "demo-skill")
            .unwrap()
            .unwrap();
        assert!(o.remote_version.is_none());
        assert!(o.remote_updated_at.is_none());
    }

    #[tokio::test]
    async fn record_after_install_upserts() {
        use crate::install::{record_after_install_in_dir, scoped_skills_dir, InstallOriginHint};

        let _guard = ENV_TEST_LOCK.lock().await;
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        let skills_dir = scoped_skills_dir("global", None).unwrap();
        record_after_install_in_dir(
            "skillhub:owner/demo-skill",
            Some("workspace"),
            &InstallOriginHint {
                name: Some("Demo".into()),
            },
            &skills_dir,
            "global",
        )
        .await
        .unwrap();
        let o = find_origin(Some("workspace"), "global", "demo-skill")
            .unwrap()
            .unwrap();
        assert_eq!(o.install_ref, "skillhub:owner/demo-skill");
        assert_eq!(o.store, "skillhub");
        assert_eq!(o.name, "Demo");
    }

    #[tokio::test]
    async fn record_after_install_persists_content_digest() {
        use crate::digest::skill_content_digest;
        use crate::install::{record_after_install_in_dir, scoped_skills_dir, InstallOriginHint};
        use std::io::Write;

        let _guard = ENV_TEST_LOCK.lock().await;
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        let skills_dir = scoped_skills_dir("global", None).unwrap();
        let skill_dir = skills_dir.join("demo-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let mut f = std::fs::File::create(skill_dir.join("SKILL.md")).unwrap();
        writeln!(f, "# Demo").unwrap();

        let expected = skill_content_digest(&skill_dir).unwrap();

        record_after_install_in_dir(
            "skillhub:owner/demo-skill",
            Some("workspace"),
            &InstallOriginHint {
                name: Some("Demo".into()),
            },
            &skills_dir,
            "global",
        )
        .await
        .unwrap();

        let o = find_origin(Some("workspace"), "global", "demo-skill")
            .unwrap()
            .unwrap();
        assert_eq!(o.content_digest.as_deref(), Some(expected.as_str()));
    }
}
