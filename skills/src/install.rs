//! 从商店引用安装 Skill（`npx skills add`），并落到当前 Agent 工作区。

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{anyhow, Context, Result};

/// SkillHub 默认 registry URL。
const SKILLHUB_REGISTRY: &str = "https://skillhub.cn";

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

/// 规范化 Agent id：空/`default` → `workspace`。
fn normalize_agent_id(agent_id: Option<&str>) -> String {
    match agent_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some("default") | None => "workspace".to_string(),
        Some(id) => id.to_string(),
    }
}

/// Agent 工作区根目录。
fn agent_workspace(agent_id: &str) -> PathBuf {
    let base = memory_dir();
    if agent_id == "workspace" {
        base.join("workspace")
    } else {
        base.join(format!("workspace-{agent_id}"))
    }
}

/// 当前 Agent 工作区 skills 目录（在线安装目标）
fn agent_skills_dir(agent_id: Option<&str>) -> Result<PathBuf> {
    let id = normalize_agent_id(agent_id);
    let dir = agent_workspace(&id).join("skills");
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    Ok(dir)
}

/// 去掉 GitHub URL 前缀，得到 `owner/repo`。
fn strip_github_prefix(url: &str) -> Option<String> {
    url.strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .map(|s| s.trim_end_matches('/').trim_end_matches(".git").to_string())
}

/// URL 末段路径（用作默认 skill 名）。
fn last_path_segment(url: &str) -> Option<String> {
    url.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// 将商店 `install_ref` 解析为 `npx` 参数（便于测试与执行）
pub(crate) fn build_install_args(
    install_ref: &str,
    install_dir: &std::path::Path,
) -> Result<Vec<String>> {
    let install_ref = install_ref.trim();
    if install_ref.is_empty() {
        return Err(anyhow!("安装引用为空"));
    }
    let dir = install_dir.to_string_lossy().into_owned();

    if let Some(rest) = install_ref.strip_prefix("skillhub:") {
        let slug = rest.rsplit('/').next().unwrap_or(rest);
        return Ok(vec![
            "@astron-team/skillhub@latest".into(),
            "install".into(),
            slug.into(),
            "--registry".into(),
            SKILLHUB_REGISTRY.into(),
            "--dir".into(),
            dir,
            "--force".into(),
        ]);
    }

    if let Some(slug) = install_ref.strip_prefix("clawhub:") {
        return Ok(vec!["clawhub@latest".into(), "install".into(), slug.into()]);
    }

    if install_ref.starts_with("http://") || install_ref.starts_with("https://") {
        if let Some(repo) = strip_github_prefix(install_ref) {
            return Ok(vec!["skills".into(), "add".into(), repo]);
        }
        if install_ref.contains("skillhub.cn") {
            let slug = last_path_segment(install_ref)
                .ok_or_else(|| anyhow!("无法从 SkillHub 链接解析 slug: {install_ref}"))?;
            return Ok(vec![
                "@astron-team/skillhub@latest".into(),
                "install".into(),
                slug,
                "--registry".into(),
                SKILLHUB_REGISTRY.into(),
                "--dir".into(),
                dir,
                "--force".into(),
            ]);
        }
        if install_ref.contains("clawhub") {
            let slug = last_path_segment(install_ref)
                .ok_or_else(|| anyhow!("无法从 ClawHub 链接解析 slug: {install_ref}"))?;
            return Ok(vec!["clawhub@latest".into(), "install".into(), slug]);
        }
        if install_ref.contains("skills.sh/") {
            let path = install_ref
                .split("skills.sh/")
                .nth(1)
                .ok_or_else(|| anyhow!("无法解析 skills.sh 链接: {install_ref}"))?;
            return Ok(vec![
                "skills".into(),
                "add".into(),
                path.trim_end_matches('/').into(),
            ]);
        }
        return Err(anyhow!(
            "暂不支持该链接自动安装: {install_ref}。请使用 owner/repo 或 GitHub 仓库地址。"
        ));
    }

    if install_ref.contains('/') {
        return Ok(vec!["skills".into(), "add".into(), install_ref.into()]);
    }

    Err(anyhow!(
        "暂不支持自动安装: {install_ref}。请使用 npx skills add <owner/repo>、skillhub:owner/slug 或 GitHub 链接。"
    ))
}

/// 调用 `npx` 执行 skills 安装命令。
fn run_npx(args: &[String], cwd: &std::path::Path) -> Result<String> {
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
        Err(anyhow!("安装失败: {stderr}"))
    }
}

/// 通过 `npx` 安装技能到指定 Agent 工作区的 `skills/`（需本机 Node.js）
pub fn install_from_ref(install_ref: &str, agent_id: Option<&str>) -> Result<String> {
    let skills_dir = agent_skills_dir(agent_id)?;
    let workspace = skills_dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| skills_dir.clone());
    let args = build_install_args(install_ref, &skills_dir)?;
    // SkillHub 用 --dir 直装；其它 CLI 在 Agent 工作区 cwd 下执行，落盘到本地 skills
    let cwd = if args.iter().any(|a| a == "--dir") {
        &workspace
    } else {
        &workspace
    };
    run_npx(&args, cwd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn resolves_skillhub_prefix() {
        let dir = Path::new("/tmp/agent-skills");
        let args = build_install_args("skillhub:acme/pdf-parser", dir).unwrap();
        assert_eq!(args[0], "@astron-team/skillhub@latest");
        assert_eq!(args[1], "install");
        assert_eq!(args[2], "pdf-parser");
        assert!(args.contains(&"--registry".to_string()));
        assert!(args.contains(&"--dir".to_string()));
        assert!(args.contains(&"/tmp/agent-skills".to_string()));
    }

    #[test]
    fn resolves_owner_repo() {
        let args =
            build_install_args("vercel-labs/agent-skills", Path::new("/tmp/x")).unwrap();
        assert_eq!(args, vec!["skills", "add", "vercel-labs/agent-skills"]);
    }

    #[test]
    fn resolves_github_url() {
        let args = build_install_args(
            "https://github.com/vercel-labs/agent-skills.git",
            Path::new("/tmp/x"),
        )
        .unwrap();
        assert_eq!(args[2], "vercel-labs/agent-skills");
    }

    #[test]
    fn resolves_clawhub_prefix() {
        let args =
            build_install_args("clawhub:my-namespace--my-skill", Path::new("/tmp/x")).unwrap();
        assert_eq!(
            args,
            vec!["clawhub@latest", "install", "my-namespace--my-skill"]
        );
    }

    #[test]
    fn resolves_skills_sh_url() {
        let args =
            build_install_args("https://skills.sh/owner/repo", Path::new("/tmp/x")).unwrap();
        assert_eq!(args, vec!["skills", "add", "owner/repo"]);
    }
}
