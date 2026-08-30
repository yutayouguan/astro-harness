//! 从 SkillHub API 安装 Skill。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

use crate::agent_id::normalize as normalize_agent_id;
use crate::digest::skill_content_digest;
use crate::models::SkillOriginRecord;
use crate::origins::{fill_origin_remote_baseline, find_origin, infer_folder, upsert_origin};

/// SkillHub 公开文件 API（无需 CLI / login）。
const SKILLHUB_API: &str = "https://api.skillhub.cn";

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

/// Agent 工作区根目录。
fn agent_workspace(agent_id: &str) -> PathBuf {
    home::agent_workspace_dir(&memory_dir(), agent_id)
}

/// 当前 Agent 工作区 skills 目录（在线安装 / 更新目标）
pub fn agent_skills_dir(agent_id: Option<&str>) -> Result<PathBuf> {
    let id = normalize_agent_id(agent_id);
    let dir = agent_workspace(&id).join("skills");
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    Ok(dir)
}

/// 解析商店安装目标。`global` 写入当前用户 `~/.astro/skills`，
/// `project` 写入可信项目的 `<project>/.astro/skills`。
pub fn scoped_skills_dir(scope: &str, project_root: Option<&Path>) -> Result<PathBuf> {
    let dir = match scope {
        "global" => memory_dir().join("skills"),
        "project" => project_root
            .ok_or_else(|| anyhow!("project scope requires a project root"))?
            .join(".astro")
            .join("skills"),
        "builtin" => bail!("builtin Skills are read-only"),
        other => bail!("unsupported Skill install scope: {other}"),
    };
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    Ok(dir)
}

/// URL 末段路径。
fn last_path_segment(url: &str) -> Option<String> {
    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// 是否为受支持的 SkillHub 安装引用。
pub(crate) fn is_skillhub_http_ref(install_ref: &str) -> bool {
    let r = install_ref.trim();
    r.starts_with("skillhub:")
        || r.starts_with("https://api.skillhub.cn/")
        || r.starts_with("https://skillhub.cn/")
        || r.starts_with("https://www.skillhub.cn/")
}

/// 从 `skillhub:` 或 SkillHub URL 解析 slug。
fn skillhub_slug(install_ref: &str) -> Result<String> {
    let r = install_ref.trim();
    if let Some(rest) = r.strip_prefix("skillhub:") {
        let slug = rest.rsplit('/').next().unwrap_or(rest).trim();
        if slug.is_empty() {
            bail!("SkillHub slug 为空: {install_ref}");
        }
        return Ok(slug.to_string());
    }
    last_path_segment(r).ok_or_else(|| anyhow!("无法从 SkillHub 引用解析 slug: {install_ref}"))
}

/// 将文件树写入 `{dest}/{slug}/...`。
pub(crate) fn write_skill_files(
    dest_root: &Path,
    slug: &str,
    files: &[(String, Vec<u8>)],
) -> Result<PathBuf> {
    if slug.trim().is_empty() {
        bail!("skill slug 为空");
    }
    if files.is_empty() {
        bail!("SkillHub 包文件列表为空: {slug}");
    }
    let dest = dest_root.join(slug);
    if dest.exists() {
        fs::remove_dir_all(&dest).with_context(|| format!("remove {}", dest.display()))?;
    }
    for (rel, bytes) in files {
        let rel = rel.trim_start_matches('/');
        if rel.is_empty() || rel.contains("..") {
            bail!("非法文件路径: {rel}");
        }
        let path = dest.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))?;
    }
    if !dest.join("SKILL.md").is_file() {
        bail!("安装后未找到 SKILL.md: {}", dest.display());
    }
    Ok(dest)
}

#[derive(Debug, Deserialize)]
struct SkillHubFilesResponse {
    files: Vec<SkillHubFileMeta>,
}

#[derive(Debug, Deserialize)]
struct SkillHubFileMeta {
    path: String,
}

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("build HTTP client")
}

/// 从 api.skillhub.cn 拉取文件并写入本地 skills 目录。
async fn install_skillhub_http(slug: &str, skills_dir: &Path) -> Result<String> {
    let client = http_client()?;
    let list_url = format!("{SKILLHUB_API}/api/v1/skills/{slug}/files");
    let resp = client
        .get(&list_url)
        .header("Accept", "application/json")
        .send()
        .await
        .with_context(|| format!("GET {list_url}"))?;
    if !resp.status().is_success() {
        bail!(
            "SkillHub 文件列表失败 HTTP {}（slug={slug}）",
            resp.status()
        );
    }
    let listing: SkillHubFilesResponse = resp.json().await.context("parse SkillHub files JSON")?;
    if listing.files.is_empty() {
        bail!("SkillHub 未返回任何文件: {slug}");
    }

    let mut files = Vec::with_capacity(listing.files.len());
    for meta in &listing.files {
        let file_url = format!(
            "{SKILLHUB_API}/api/v1/skills/{slug}/file?path={}",
            urlencoding::encode(&meta.path)
        );
        let file_resp = client
            .get(&file_url)
            .send()
            .await
            .with_context(|| format!("GET {file_url}"))?;
        if !file_resp.status().is_success() {
            bail!(
                "下载 SkillHub 文件失败 HTTP {}：{}",
                file_resp.status(),
                meta.path
            );
        }
        let bytes = file_resp.bytes().await?.to_vec();
        files.push((meta.path.clone(), bytes));
    }

    let dest = write_skill_files(skills_dir, slug, &files)?;
    Ok(format!(
        "已从 SkillHub 安装 {slug} → {}（{} 个文件）",
        dest.display(),
        files.len()
    ))
}

/// 安装时附带的来源提示（展示名 / 本地文件夹名）。
#[derive(Debug, Clone, Default)]
pub struct InstallOriginHint {
    pub name: Option<String>,
    pub folder: Option<String>,
}

/// 安装成功后写入 `skill-origins.json`；无法推断 folder 时仅告警，不使安装失败。
pub async fn record_after_install(
    install_ref: &str,
    agent_id: Option<&str>,
    hint: &InstallOriginHint,
) -> Result<()> {
    let skills_dir = agent_skills_dir(agent_id)?;
    record_after_install_in_dir(install_ref, agent_id, hint, &skills_dir, None).await
}

async fn record_after_install_in_dir(
    install_ref: &str,
    agent_id: Option<&str>,
    hint: &InstallOriginHint,
    skills_dir: &Path,
    scope: Option<&str>,
) -> Result<()> {
    let folder = hint
        .folder
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| infer_folder(install_ref));

    let Some(folder) = folder else {
        tracing::warn!(
            install_ref = %install_ref,
            "无法推断 skill folder，跳过 origin 记录"
        );
        return Ok(());
    };

    let name = hint
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| folder.clone());

    let normalized_agent = normalize_agent_id(agent_id);
    let existing = find_origin(Some(&normalized_agent), &folder)?;
    let now = chrono::Utc::now().timestamp();
    let installed_at = existing.as_ref().map(|r| r.installed_at).unwrap_or(now);
    let is_update = existing.is_some();

    let content_digest = match skill_content_digest(&skills_dir.join(&folder)) {
        Ok(digest) => Some(digest),
        Err(e) => {
            tracing::debug!(
                folder = %folder,
                error = %e,
                "compute content_digest after install failed; continuing"
            );
            None
        }
    };

    upsert_origin(SkillOriginRecord {
        folder: folder.clone(),
        skill_id: None,
        name,
        store: "skillhub".to_string(),
        install_ref: install_ref.to_string(),
        agent_id: Some(normalized_agent.clone()),
        scope: scope.map(str::to_string),
        installed_at,
        last_updated_at: if is_update { Some(now) } else { None },
        remote_version: None,
        remote_updated_at: None,
        content_digest,
    })?;

    fill_origin_remote_baseline(Some(&normalized_agent), &folder).await
}

/// 安装 SkillHub 技能到指定 Agent 工作区的 `skills/`。
pub async fn install_from_ref(
    install_ref: &str,
    agent_id: Option<&str>,
    hint: Option<InstallOriginHint>,
) -> Result<String> {
    let skills_dir = agent_skills_dir(agent_id)?;
    install_from_ref_into(install_ref, agent_id, hint, skills_dir, None).await
}

/// 按个人或项目作用域安装在线 Skill。
pub async fn install_from_ref_scoped(
    install_ref: &str,
    agent_id: Option<&str>,
    hint: Option<InstallOriginHint>,
    scope: &str,
    project_root: Option<&Path>,
) -> Result<String> {
    let skills_dir = scoped_skills_dir(scope, project_root)?;
    install_from_ref_into(install_ref, agent_id, hint, skills_dir, Some(scope)).await
}

async fn install_from_ref_into(
    install_ref: &str,
    agent_id: Option<&str>,
    hint: Option<InstallOriginHint>,
    skills_dir: PathBuf,
    scope: Option<&str>,
) -> Result<String> {
    let hint = hint.unwrap_or_default();
    if !is_skillhub_http_ref(install_ref) {
        bail!("仅支持 SkillHub 安装引用: {install_ref}");
    }
    let slug = skillhub_slug(install_ref)?;
    let result = install_skillhub_http(&slug, &skills_dir).await?;

    record_after_install_in_dir(install_ref, agent_id, &hint, &skills_dir, scope).await?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_skillhub_refs() {
        assert!(is_skillhub_http_ref(
            "skillhub:user_ec205dbb/web-tools-guide"
        ));
        assert!(is_skillhub_http_ref(
            "https://api.skillhub.cn/user_ec205dbb/web-tools-guide"
        ));
        assert!(!is_skillhub_http_ref("legacy:owner--skill"));
        assert!(!is_skillhub_http_ref("https://legacy.example/owner/skill"));
    }

    #[test]
    fn writes_skillhub_file_tree_under_slug_dir() {
        let dir = tempfile::tempdir().unwrap();
        let files = vec![
            ("SKILL.md".into(), b"# hello\n".to_vec()),
            ("refs/a.md".into(), b"a\n".to_vec()),
        ];
        write_skill_files(dir.path(), "web-tools-guide", &files).unwrap();
        assert!(dir.path().join("web-tools-guide/SKILL.md").is_file());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("web-tools-guide/refs/a.md")).unwrap(),
            "a\n"
        );
    }

    #[test]
    fn resolves_project_install_target_and_rejects_builtin() {
        let project = tempfile::tempdir().unwrap();
        let target = scoped_skills_dir("project", Some(project.path())).unwrap();

        assert_eq!(target, project.path().join(".astro/skills"));
        assert!(target.is_dir());
        assert!(scoped_skills_dir("builtin", Some(project.path())).is_err());
        assert!(scoped_skills_dir("project", None).is_err());
    }

    #[tokio::test]
    #[ignore = "requires public SkillHub network"]
    async fn skillhub_http_install_from_public_api() {
        let dir = tempfile::tempdir().unwrap();
        let msg = install_skillhub_http("web-tools-guide", dir.path())
            .await
            .expect("SkillHub HTTP install");
        assert!(msg.contains("web-tools-guide"));
        assert!(dir.path().join("web-tools-guide/SKILL.md").is_file());
    }
}
