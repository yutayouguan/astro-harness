//! 从 SkillHub API 安装 Skill。

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

use crate::agent_id::normalize as normalize_agent_id;
use crate::digest::skill_content_digest;
use crate::models::SkillOriginRecord;
use crate::origins::{fill_origin_remote_baseline, find_origin, upsert_origin};

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

/// Agent 运行时生成或演化 Skill 的工作区目录；不用于在线市场安装。
pub fn agent_workspace_skills_dir(agent_id: Option<&str>) -> Result<PathBuf> {
    let id = normalize_agent_id(agent_id);
    let dir = home::agent_workspace_dir(&memory_dir(), &id).join("skills");
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    Ok(dir)
}

/// 解析商店安装目标。`global` 写入当前用户 `~/.astro/skills`，
/// `project` 写入可信项目的 `<project>/.astro/skills`。
pub fn scoped_skills_path(scope: &str, project_root: Option<&Path>) -> Result<PathBuf> {
    let path = match scope {
        "global" => memory_dir().join("skills"),
        "project" => project_root
            .ok_or_else(|| anyhow!("project scope requires a project root"))?
            .join(".astro")
            .join("skills"),
        "builtin" => bail!("builtin Skills are read-only"),
        other => bail!("unsupported Skill install scope: {other}"),
    };
    Ok(path)
}

/// 解析并创建商店安装目标目录。
pub fn scoped_skills_dir(scope: &str, project_root: Option<&Path>) -> Result<PathBuf> {
    let dir = scoped_skills_path(scope, project_root)?;
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    Ok(dir)
}

fn validate_path_component(value: &str, label: &str) -> Result<()> {
    let mut components = Path::new(value).components();
    if value.is_empty()
        || value.contains(['/', '\\', '\0'])
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
    {
        bail!("非法 {label}: {value}");
    }
    Ok(())
}

pub(crate) fn is_safe_skill_folder(folder: &str) -> bool {
    validate_path_component(folder, "skill folder").is_ok()
}

/// 是否为受支持的 SkillHub 安装引用。
pub(crate) fn is_skillhub_http_ref(install_ref: &str) -> bool {
    skillhub_slug(install_ref).is_ok()
}

/// 从 `skillhub:` 或 SkillHub URL 解析 slug。
fn skillhub_slug(install_ref: &str) -> Result<String> {
    let r = install_ref.trim();
    if let Some(rest) = r.strip_prefix("skillhub:") {
        let segments = rest.split('/').collect::<Vec<_>>();
        if segments.is_empty() || segments.len() > 2 {
            bail!("无效 SkillHub 安装引用: {install_ref}");
        }
        for segment in &segments {
            validate_path_component(segment.trim(), "SkillHub 引用段")?;
        }
        let slug = segments.last().copied().unwrap_or_default().trim();
        validate_path_component(slug, "SkillHub slug")?;
        return Ok(slug.to_string());
    }
    let url =
        reqwest::Url::parse(r).with_context(|| format!("无法解析 SkillHub 引用: {install_ref}"))?;
    let allowed_host = matches!(
        url.host_str(),
        Some("api.skillhub.cn" | "skillhub.cn" | "www.skillhub.cn")
    );
    if url.scheme() != "https" || !allowed_host || url.port().is_some() {
        bail!("仅支持官方 SkillHub HTTPS 引用: {install_ref}");
    }
    let slug = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|segment| !segment.is_empty())
        .ok_or_else(|| anyhow!("无法从 SkillHub 引用解析 slug: {install_ref}"))?;
    validate_path_component(slug, "SkillHub slug")?;
    Ok(slug.to_string())
}

fn safe_relative_path(value: &str) -> Result<PathBuf> {
    if value.is_empty() || value.contains(['\\', '\0']) {
        bail!("非法文件路径: {value}");
    }
    let path = Path::new(value);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("非法文件路径: {value}");
    }
    Ok(path.to_path_buf())
}

fn unique_install_path(dest_root: &Path, slug: &str, kind: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    dest_root.join(format!(
        ".{slug}.astro-{kind}-{}-{nonce}",
        std::process::id()
    ))
}

fn remove_path(path: &Path) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e).with_context(|| format!("stat {}", path.display())),
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path).with_context(|| format!("remove {}", path.display()))
    } else {
        fs::remove_file(path).with_context(|| format!("remove {}", path.display()))
    }
}

/// 将文件树先写入同盘临时目录，校验后再替换 `{dest}/{slug}`；失败时保留旧版本。
pub(crate) fn write_skill_files(
    dest_root: &Path,
    slug: &str,
    files: &[(String, Vec<u8>)],
) -> Result<PathBuf> {
    validate_path_component(slug, "skill slug")?;
    if files.is_empty() {
        bail!("SkillHub 包文件列表为空: {slug}");
    }
    let validated = files
        .iter()
        .map(|(rel, bytes)| Ok((safe_relative_path(rel)?, bytes)))
        .collect::<Result<Vec<_>>>()?;
    if !validated
        .iter()
        .any(|(rel, _)| rel == Path::new("SKILL.md"))
    {
        bail!("SkillHub 包缺少 SKILL.md: {slug}");
    }

    fs::create_dir_all(dest_root).with_context(|| format!("create {}", dest_root.display()))?;
    let dest = dest_root.join(slug);
    let staging = unique_install_path(dest_root, slug, "staging");
    let displaced = unique_install_path(dest_root, slug, "previous");
    fs::create_dir(&staging).with_context(|| format!("create {}", staging.display()))?;

    let write_result = (|| -> Result<()> {
        for (rel, bytes) in &validated {
            let path = staging.join(rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create {}", parent.display()))?;
            }
            fs::write(&path, bytes).with_context(|| format!("write {}", path.display()))?;
        }
        if !staging.join("SKILL.md").is_file() {
            bail!("安装后未找到 SKILL.md: {}", staging.display());
        }
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = remove_path(&staging);
        return Err(error);
    }

    let had_dest = fs::symlink_metadata(&dest).is_ok();
    if had_dest {
        fs::rename(&dest, &displaced)
            .with_context(|| format!("move existing {}", dest.display()))?;
    }
    if let Err(error) = fs::rename(&staging, &dest) {
        let _ = remove_path(&staging);
        if had_dest {
            fs::rename(&displaced, &dest).with_context(|| {
                format!(
                    "activate {} failed ({error}); restoring previous version also failed",
                    dest.display()
                )
            })?;
        }
        return Err(error).with_context(|| format!("activate {}", dest.display()));
    }
    if had_dest {
        if let Err(error) = remove_path(&displaced) {
            tracing::warn!(
                path = %displaced.display(),
                error = %error,
                "installed Skill but could not remove displaced directory"
            );
        }
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

/// 从 api.skillhub.cn 拉取 `remote_slug` 并写入本地 `target_folder`。
///
/// 官方镜像的 SkillHub slug 可能带上游前缀，而本地安装目录仍保持原名。
async fn install_skillhub_http_as(
    remote_slug: &str,
    target_folder: &str,
    skills_dir: &Path,
) -> Result<String> {
    let client = http_client()?;
    let list_url = format!("{SKILLHUB_API}/api/v1/skills/{remote_slug}/files");
    let resp = client
        .get(&list_url)
        .header("Accept", "application/json")
        .send()
        .await
        .with_context(|| format!("GET {list_url}"))?;
    if !resp.status().is_success() {
        bail!(
            "SkillHub 文件列表失败 HTTP {}（slug={remote_slug}）",
            resp.status()
        );
    }
    let listing: SkillHubFilesResponse = resp.json().await.context("parse SkillHub files JSON")?;
    if listing.files.is_empty() {
        bail!("SkillHub 未返回任何文件: {remote_slug}");
    }

    let mut files = Vec::with_capacity(listing.files.len());
    for meta in &listing.files {
        let file_url = format!(
            "{SKILLHUB_API}/api/v1/skills/{remote_slug}/file?path={}",
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

    let dest = write_skill_files(skills_dir, target_folder, &files)?;
    Ok(format!(
        "已从 SkillHub 安装 {remote_slug} → {}（{} 个文件）",
        dest.display(),
        files.len()
    ))
}

#[cfg(test)]
async fn install_skillhub_http(slug: &str, skills_dir: &Path) -> Result<String> {
    install_skillhub_http_as(slug, slug, skills_dir).await
}

/// 安装时附带的来源提示（展示名 / 本地文件夹名）。
#[derive(Debug, Clone, Default)]
pub struct InstallOriginHint {
    pub name: Option<String>,
}

#[cfg(test)]
pub(crate) async fn record_after_install_in_dir(
    install_ref: &str,
    agent_id: Option<&str>,
    hint: &InstallOriginHint,
    skills_dir: &Path,
    scope: &str,
) -> Result<()> {
    let folder = skillhub_slug(install_ref)?;
    record_after_install_for_folder_in_dir(install_ref, agent_id, hint, skills_dir, scope, &folder)
        .await
}

async fn record_after_install_for_folder_in_dir(
    install_ref: &str,
    agent_id: Option<&str>,
    hint: &InstallOriginHint,
    skills_dir: &Path,
    scope: &str,
    folder: &str,
) -> Result<()> {
    validate_path_component(folder, "skill folder")?;

    let name = hint
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| folder.to_string());

    let normalized_agent = normalize_agent_id(agent_id);
    let existing = find_origin(Some(&normalized_agent), scope, folder)?;
    let now = chrono::Utc::now().timestamp();
    let installed_at = existing.as_ref().map(|r| r.installed_at).unwrap_or(now);
    let is_update = existing.is_some();

    let content_digest = match skill_content_digest(&skills_dir.join(folder)) {
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
        folder: folder.to_string(),
        skill_id: existing.as_ref().and_then(|record| record.skill_id.clone()),
        name,
        store: "skillhub".to_string(),
        install_ref: install_ref.to_string(),
        agent_id: Some(normalized_agent.clone()),
        scope: Some(scope.to_string()),
        installed_at,
        last_updated_at: if is_update { Some(now) } else { None },
        remote_version: existing
            .as_ref()
            .and_then(|record| record.remote_version.clone()),
        remote_updated_at: existing
            .as_ref()
            .and_then(|record| record.remote_updated_at),
        content_digest,
    })?;

    fill_origin_remote_baseline(Some(&normalized_agent), scope, folder).await
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
    install_from_ref_into(install_ref, agent_id, hint, skills_dir, scope, None).await
}

/// 从 SkillHub 更新已有本地目录；远程 slug 与本地 folder 可不同。
pub(crate) async fn install_from_ref_scoped_as(
    install_ref: &str,
    agent_id: Option<&str>,
    hint: Option<InstallOriginHint>,
    scope: &str,
    project_root: Option<&Path>,
    target_folder: &str,
) -> Result<String> {
    let skills_dir = scoped_skills_dir(scope, project_root)?;
    install_from_ref_into(
        install_ref,
        agent_id,
        hint,
        skills_dir,
        scope,
        Some(target_folder),
    )
    .await
}

async fn install_from_ref_into(
    install_ref: &str,
    agent_id: Option<&str>,
    hint: Option<InstallOriginHint>,
    skills_dir: PathBuf,
    scope: &str,
    target_folder: Option<&str>,
) -> Result<String> {
    let hint = hint.unwrap_or_default();
    if !is_skillhub_http_ref(install_ref) {
        bail!("仅支持 SkillHub 安装引用: {install_ref}");
    }
    let slug = skillhub_slug(install_ref)?;
    let target_folder = target_folder.unwrap_or(&slug);
    validate_path_component(target_folder, "skill folder")?;
    let result = install_skillhub_http_as(&slug, target_folder, &skills_dir).await?;

    record_after_install_for_folder_in_dir(
        install_ref,
        agent_id,
        &hint,
        &skills_dir,
        scope,
        target_folder,
    )
    .await?;

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
        assert!(!is_skillhub_http_ref(
            "https://skillhub.cn.evil.test/skills/demo"
        ));
        assert!(!is_skillhub_http_ref(
            "https://skillhub.cn:8443/skills/demo"
        ));
        assert!(!is_skillhub_http_ref("skillhub:.."));
        assert!(!is_skillhub_http_ref(
            "skillhub:https://github.com/owner/demo"
        ));
        assert!(!is_skillhub_http_ref("skillhub:owner//demo"));
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
    fn invalid_package_does_not_replace_existing_skill() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("demo");
        std::fs::create_dir_all(&existing).unwrap();
        std::fs::write(existing.join("SKILL.md"), "# existing\n").unwrap();

        let error = write_skill_files(dir.path(), "demo", &[("../escape".into(), b"bad".to_vec())])
            .unwrap_err();

        assert!(error.to_string().contains("非法文件路径"));
        assert_eq!(
            std::fs::read_to_string(existing.join("SKILL.md")).unwrap(),
            "# existing\n"
        );
        assert!(!dir.path().join("escape").exists());
    }

    #[test]
    fn valid_package_replaces_existing_skill_without_stale_files() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("demo");
        std::fs::create_dir_all(&existing).unwrap();
        std::fs::write(existing.join("SKILL.md"), "# old\n").unwrap();
        std::fs::write(existing.join("stale.txt"), "stale").unwrap();

        write_skill_files(
            dir.path(),
            "demo",
            &[("SKILL.md".into(), b"# new\n".to_vec())],
        )
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(existing.join("SKILL.md")).unwrap(),
            "# new\n"
        );
        assert!(!existing.join("stale.txt").exists());
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
