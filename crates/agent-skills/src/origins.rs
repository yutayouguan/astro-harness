//! 技能安装来源持久化（`skill-origins.json`）。

use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::agent_id::normalize as normalize_agent_id;
use crate::models::{SkillOriginRecord, SkillOriginsFile, StoreSkill, StoreSkillDetail};
use crate::store::fetch_detail;

const ORIGINS_FILE: &str = "skill-origins.json";
const ORIGINS_VERSION: u32 = 2;

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

fn origin_key(agent_id: Option<&str>, folder: &str) -> (String, String) {
    (normalize_agent_id(agent_id), folder.to_string())
}

fn is_skillhub_install_ref(install_ref: &str) -> bool {
    let value = install_ref.trim();
    value.starts_with("skillhub:")
        || value.starts_with("https://api.skillhub.cn/")
        || value.starts_with("https://skillhub.cn/")
        || value.starts_with("https://www.skillhub.cn/")
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
            && is_skillhub_install_ref(&record.install_ref)
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

/// 按 `(agent_id, folder)` 插入或更新；更新时写入新记录的 `last_updated_at`。
pub fn upsert_origin(mut record: SkillOriginRecord) -> Result<()> {
    if !record.store.eq_ignore_ascii_case("skillhub")
        || !is_skillhub_install_ref(&record.install_ref)
    {
        bail!("仅支持记录 SkillHub 安装来源");
    }
    record.store = "skillhub".to_string();
    record.agent_id = Some(normalize_agent_id(record.agent_id.as_deref()));
    let key = origin_key(record.agent_id.as_deref(), &record.folder);
    let mut file = load_origins()?;
    if let Some(existing) = file
        .records
        .iter_mut()
        .find(|r| origin_key(r.agent_id.as_deref(), &r.folder) == key)
    {
        *existing = record;
    } else {
        file.records.push(record);
    }
    save_origins(&file)
}

/// 按 Agent 与文件夹名查找来源记录。
pub fn find_origin(agent_id: Option<&str>, folder: &str) -> Result<Option<SkillOriginRecord>> {
    let key = origin_key(agent_id, folder);
    let file = load_origins()?;
    Ok(file
        .records
        .into_iter()
        .find(|r| origin_key(r.agent_id.as_deref(), &r.folder) == key))
}

/// 从 `install_ref` 推断本地技能文件夹名。
pub fn infer_folder(install_ref: &str) -> Option<String> {
    let r = install_ref.trim();
    if let Some(rest) = r.strip_prefix("skillhub:") {
        let slug = rest.rsplit('/').next().unwrap_or(rest).trim();
        return (!slug.is_empty()).then(|| slug.to_string());
    }
    if r.contains("api.skillhub.cn/") || r.contains("skillhub.cn/") {
        let slug = r.trim_end_matches('/').rsplit('/').next()?.trim();
        return (!slug.is_empty()).then(|| slug.to_string());
    }
    None
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
pub async fn fill_origin_remote_baseline(agent_id: Option<&str>, folder: &str) -> Result<()> {
    let Some(origin) = find_origin(agent_id, folder)? else {
        return Ok(());
    };
    let store_skill = origin_to_store_skill(&origin);
    match fetch_detail(&store_skill).await {
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
    use crate::ENV_TEST_LOCK;
    use tempfile::tempdir;

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
            scope: None,
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
            scope: None,
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
    fn infer_folder_from_skillhub_ref() {
        assert_eq!(
            infer_folder("skillhub:owner/ppt-generator-skill").as_deref(),
            Some("ppt-generator-skill")
        );
        assert_eq!(infer_folder("legacy:owner--weather"), None);
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
            scope: None,
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
            scope: None,
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
                    scope: None,
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
                    scope: None,
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
            scope: None,
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
            scope: None,
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
            scope: None,
            installed_at: 1,
            last_updated_at: None,
            remote_version: None,
            remote_updated_at: None,
            content_digest: None,
        })
        .unwrap();

        fill_origin_remote_baseline(Some("workspace"), "demo-skill")
            .await
            .unwrap();

        let o = find_origin(Some("workspace"), "demo-skill")
            .unwrap()
            .unwrap();
        assert!(o.remote_version.is_none());
        assert!(o.remote_updated_at.is_none());
    }

    #[tokio::test]
    async fn record_after_install_upserts() {
        use crate::install::{record_after_install, InstallOriginHint};

        let _guard = ENV_TEST_LOCK.lock().await;
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        record_after_install(
            "skillhub:owner/demo-skill",
            Some("workspace"),
            &InstallOriginHint {
                name: Some("Demo".into()),
                folder: Some("demo-skill".into()),
            },
        )
        .await
        .unwrap();
        let o = find_origin(Some("workspace"), "demo-skill")
            .unwrap()
            .unwrap();
        assert_eq!(o.install_ref, "skillhub:owner/demo-skill");
        assert_eq!(o.store, "skillhub");
        assert_eq!(o.name, "Demo");
    }

    #[tokio::test]
    async fn record_after_install_persists_content_digest() {
        use crate::digest::skill_content_digest;
        use crate::install::{agent_skills_dir, record_after_install, InstallOriginHint};
        use std::io::Write;

        let _guard = ENV_TEST_LOCK.lock().await;
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        let skills_dir = agent_skills_dir(Some("workspace")).unwrap();
        let skill_dir = skills_dir.join("demo-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        let mut f = std::fs::File::create(skill_dir.join("SKILL.md")).unwrap();
        writeln!(f, "# Demo").unwrap();

        let expected = skill_content_digest(&skill_dir).unwrap();

        record_after_install(
            "skillhub:owner/demo-skill",
            Some("workspace"),
            &InstallOriginHint {
                name: Some("Demo".into()),
                folder: Some("demo-skill".into()),
            },
        )
        .await
        .unwrap();

        let o = find_origin(Some("workspace"), "demo-skill")
            .unwrap()
            .unwrap();
        assert_eq!(o.content_digest.as_deref(), Some(expected.as_str()));
    }
}
