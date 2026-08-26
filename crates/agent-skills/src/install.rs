//! 从商店引用安装 Skill：SkillHub 走 HTTP API；skills.sh / GitHub 走 `npx skills add`。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

use crate::agent_id::normalize as normalize_agent_id;
use crate::digest::skill_content_digest;
use crate::models::SkillOriginRecord;
use crate::origins::{
    fill_origin_remote_baseline, find_origin, infer_folder, infer_store, upsert_origin,
};

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

/// 去掉 GitHub URL 前缀，得到 `owner/repo`。
fn strip_github_prefix(url: &str) -> Option<String> {
    url.strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .map(|s| s.trim_end_matches('/').trim_end_matches(".git").to_string())
}

/// URL 末段路径。
fn last_path_segment(url: &str) -> Option<String> {
    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// 是否走 SkillHub HTTP 直装（避免损坏的 `--registry skillhub.cn` CLI）。
pub(crate) fn is_skillhub_http_ref(install_ref: &str) -> bool {
    let r = install_ref.trim();
    r.starts_with("skillhub:")
        || r.contains("api.skillhub.cn/")
        || (r.contains("skillhub.cn/") && !r.contains("skills.sh/"))
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

/// `npx skills add` / clawhub 非交互参数。
fn skills_add_args(package: &str, skill: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "--yes".to_string(),
        "skills".to_string(),
        "add".to_string(),
        package.to_string(),
    ];
    if let Some(name) = skill.filter(|s| !s.is_empty()) {
        args.push("--skill".to_string());
        args.push(name.to_string());
    }
    args.push("-y".to_string());
    args
}

/// 解析 `skills.sh/{source}/{skillId}`（source 可含 `/`）。
fn parse_skills_sh_path(path: &str) -> Result<(String, Option<String>)> {
    let path = path.trim().trim_end_matches('/');
    if path.is_empty() {
        bail!("skills.sh 路径为空");
    }
    // owner/repo → 整包；owner/repo/skill → 指定 skill
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    match parts.as_slice() {
        [] => bail!("skills.sh 路径为空"),
        [a] => Ok(((*a).to_string(), None)),
        [a, b] => Ok((format!("{a}/{b}"), None)),
        [..] => {
            let skill = parts.last().unwrap().to_string();
            let package = parts[..parts.len() - 1].join("/");
            Ok((package, Some(skill)))
        }
    }
}

/// 将商店 `install_ref` 解析为 `npx` 参数。SkillHub HTTP 引用应走 `is_skillhub_http_ref`。
pub(crate) fn build_install_args(install_ref: &str, _install_dir: &Path) -> Result<Vec<String>> {
    let install_ref = install_ref.trim();
    if install_ref.is_empty() {
        return Err(anyhow!("安装引用为空"));
    }
    if is_skillhub_http_ref(install_ref) {
        return Err(anyhow!(
            "SkillHub 引用应使用 HTTP 直装，而非 skillhub CLI: {install_ref}"
        ));
    }

    if let Some(rest) = install_ref.strip_prefix("skillsdotsh:") {
        let (package, skill) = parse_skills_sh_path(rest)?;
        return Ok(skills_add_args(&package, skill.as_deref()));
    }

    if let Some(slug) = install_ref.strip_prefix("clawhub:") {
        return Ok(vec![
            "--yes".to_string(),
            "clawhub@latest".to_string(),
            "install".to_string(),
            slug.to_string(),
        ]);
    }

    if install_ref.starts_with("http://") || install_ref.starts_with("https://") {
        if let Some(repo) = strip_github_prefix(install_ref) {
            return Ok(skills_add_args(&repo, None));
        }
        if install_ref.contains("clawhub") {
            let slug = last_path_segment(install_ref)
                .ok_or_else(|| anyhow!("无法从 ClawHub 链接解析 slug: {install_ref}"))?;
            return Ok(vec![
                "--yes".to_string(),
                "clawhub@latest".to_string(),
                "install".to_string(),
                slug,
            ]);
        }
        if install_ref.contains("skills.sh/") {
            let path = install_ref
                .split("skills.sh/")
                .nth(1)
                .ok_or_else(|| anyhow!("无法解析 skills.sh 链接: {install_ref}"))?;
            let (package, skill) = parse_skills_sh_path(path)?;
            return Ok(skills_add_args(&package, skill.as_deref()));
        }
        return Err(anyhow!(
            "暂不支持该链接自动安装: {install_ref}。请使用 owner/repo 或 GitHub 仓库地址。"
        ));
    }

    if install_ref.contains('/') {
        return Ok(skills_add_args(install_ref, None));
    }

    Err(anyhow!(
        "暂不支持自动安装: {install_ref}。请使用 npx skills add <owner/repo>、skillhub:owner/slug 或 GitHub 链接。"
    ))
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

/// 调用 `npx` 执行 skills / clawhub 安装命令。
fn run_npx(args: &[String], cwd: &Path) -> Result<String> {
    let output = Command::new("npx")
        .args(args)
        .current_dir(cwd)
        .output()
        .context("执行 npx 失败（请确认已安装 Node.js）")?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(if stdout.trim().is_empty() {
            format!("安装完成 → {}（npx {}）", cwd.display(), args.join(" "))
        } else {
            format!("{}\n→ {}", stdout.trim(), cwd.display())
        })
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let detail = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else {
            stdout.trim().to_string()
        };
        Err(anyhow!("安装失败: {detail}"))
    }
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src).with_context(|| format!("read {}", src.display()))? {
        let entry = entry?;
        let source = entry.path();
        let target = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&source, &target)?;
        } else {
            fs::copy(&source, &target)
                .with_context(|| format!("copy {} -> {}", source.display(), target.display()))?;
        }
    }
    Ok(())
}

/// `npx skills add` 会先落到 cwd 下的兼容目录；scoped install 再归一化到目标层。
fn relocate_cli_install(workspace: &Path, skills_dir: &Path, folder: &str) -> Result<PathBuf> {
    let destination = skills_dir.join(folder);
    if destination.join("SKILL.md").is_file() {
        return Ok(destination);
    }
    let source = [
        workspace.join(".agents/skills").join(folder),
        workspace.join(".cursor/skills").join(folder),
        workspace.join("skills").join(folder),
    ]
    .into_iter()
    .find(|candidate| candidate.join("SKILL.md").is_file())
    .ok_or_else(|| anyhow!("installed Skill `{folder}` was not found after CLI completed"))?;
    if destination.exists() {
        fs::remove_dir_all(&destination)?;
    }
    let resolved = fs::canonicalize(&source).unwrap_or(source);
    copy_dir_recursive(&resolved, &destination)?;
    Ok(destination)
}

/// 安装时附带的来源提示（商店名 / 展示名 / 本地文件夹名）。
#[derive(Debug, Clone, Default)]
pub struct InstallOriginHint {
    pub name: Option<String>,
    pub store: Option<String>,
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

    let store = hint
        .store
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| infer_store(install_ref));

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
        store,
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

/// 安装技能到指定 Agent 工作区的 `skills/`（SkillHub 无需 Node；其余需本机 Node.js）
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
    let workspace = skills_dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| skills_dir.clone());

    let hint = hint.unwrap_or_default();
    let folder = hint
        .folder
        .as_deref()
        .map(str::trim)
        .filter(|folder| !folder.is_empty())
        .map(str::to_string)
        .or_else(|| infer_folder(install_ref));
    let result = if is_skillhub_http_ref(install_ref) {
        let slug = skillhub_slug(install_ref)?;
        install_skillhub_http(&slug, &skills_dir).await?
    } else {
        let args = build_install_args(install_ref, &skills_dir)?;
        let cli_workspace = workspace.clone();
        let output = tokio::task::spawn_blocking(move || run_npx(&args, &cli_workspace))
            .await
            .context("npx 任务 join 失败")??;
        if scope.is_some() {
            let folder = folder
                .as_deref()
                .ok_or_else(|| anyhow!("cannot determine installed Skill folder"))?;
            let destination = relocate_cli_install(&workspace, &skills_dir, folder)?;
            format!("{output}\n→ {}", destination.display())
        } else {
            output
        }
    };

    record_after_install_in_dir(install_ref, agent_id, &hint, &skills_dir, scope).await?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn resolves_owner_repo() {
        let args = build_install_args("vercel-labs/agent-skills", Path::new("/tmp/x")).unwrap();
        assert_eq!(
            args,
            vec![
                "--yes".to_string(),
                "skills".to_string(),
                "add".to_string(),
                "vercel-labs/agent-skills".to_string(),
                "-y".to_string(),
            ]
        );
    }

    #[test]
    fn resolves_github_url() {
        let args = build_install_args(
            "https://github.com/vercel-labs/agent-skills.git",
            Path::new("/tmp/x"),
        )
        .unwrap();
        assert_eq!(args[3], "vercel-labs/agent-skills");
        assert!(args.contains(&"--yes".to_string()));
        assert!(args.contains(&"-y".to_string()));
    }

    #[test]
    fn resolves_clawhub_prefix() {
        let args =
            build_install_args("clawhub:my-namespace--my-skill", Path::new("/tmp/x")).unwrap();
        assert_eq!(
            args,
            vec![
                "--yes".to_string(),
                "clawhub@latest".to_string(),
                "install".to_string(),
                "my-namespace--my-skill".to_string(),
            ]
        );
    }

    #[test]
    fn resolves_skills_sh_url() {
        let args = build_install_args("https://skills.sh/owner/repo", Path::new("/tmp/x")).unwrap();
        assert_eq!(
            args,
            vec![
                "--yes".to_string(),
                "skills".to_string(),
                "add".to_string(),
                "owner/repo".to_string(),
                "-y".to_string(),
            ]
        );
    }

    #[test]
    fn resolves_skillsdotsh_ref_with_skill_flag() {
        let args = build_install_args(
            "skillsdotsh:vercel-labs/skills/find-skills",
            Path::new("/tmp/x"),
        )
        .unwrap();
        assert_eq!(
            args,
            vec![
                "--yes".to_string(),
                "skills".to_string(),
                "add".to_string(),
                "vercel-labs/skills".to_string(),
                "--skill".to_string(),
                "find-skills".to_string(),
                "-y".to_string(),
            ]
        );
    }

    #[test]
    fn resolves_skills_sh_homepage_with_nested_source() {
        let args = build_install_args(
            "https://skills.sh/vercel-labs/skills/find-skills",
            Path::new("/tmp/x"),
        )
        .unwrap();
        assert_eq!(
            args,
            vec![
                "--yes".to_string(),
                "skills".to_string(),
                "add".to_string(),
                "vercel-labs/skills".to_string(),
                "--skill".to_string(),
                "find-skills".to_string(),
                "-y".to_string(),
            ]
        );
    }

    #[test]
    fn skillhub_prefix_is_native_http_not_broken_cli_registry() {
        assert!(is_skillhub_http_ref(
            "skillhub:user_ec205dbb/web-tools-guide"
        ));
        assert!(is_skillhub_http_ref(
            "https://api.skillhub.cn/user_ec205dbb/web-tools-guide"
        ));
        assert!(build_install_args(
            "skillhub:user_ec205dbb/web-tools-guide",
            Path::new("/tmp/x")
        )
        .is_err());
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

    #[test]
    fn relocates_cli_output_into_scoped_skills_directory() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path();
        let source = workspace.join(".agents/skills/demo");
        let target_root = workspace.join(".astro/skills");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "# demo").unwrap();

        let target = relocate_cli_install(workspace, &target_root, "demo").unwrap();

        assert_eq!(target, target_root.join("demo"));
        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "# demo"
        );
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
