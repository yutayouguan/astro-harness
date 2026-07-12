//! 首次启动时播种一组默认公开 Skill。

use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// 默认 Skill 目录项：名称 + 安装源。
pub struct DefaultSkillSeed {
    /// Skill 目录名 / 标识。
    pub name: &'static str,
    /// `skills add` 用的源 URL。
    pub source: &'static str,
}

/// 内置公开 Skill 清单（find-skills、skill-creator 等）。
pub const DEFAULT_PUBLIC_SKILLS: &[DefaultSkillSeed] = &[
    DefaultSkillSeed {
        name: "find-skills",
        source: "https://github.com/vercel-labs/skills",
    },
    DefaultSkillSeed {
        name: "skill-creator",
        source: "https://github.com/anthropics/skills",
    },
    DefaultSkillSeed {
        name: "brainstorming",
        source: "https://github.com/obra/superpowers",
    },
    DefaultSkillSeed {
        name: "agent-browser",
        source: "https://github.com/vercel-labs/agent-browser",
    },
];

/// 判断 `base/skills/<name>/SKILL.md` 是否已存在。
pub fn is_public_skill_installed(base: &Path, name: &str) -> bool {
    base.join("skills").join(name).join("SKILL.md").is_file()
}

/// 构造 `npx skills add <source> --skill <name> -y` 参数列表。
pub fn build_seed_npx_args(source: &str, skill_name: &str) -> Vec<String> {
    vec![
        "--yes".into(),
        "skills".into(),
        "add".into(),
        source.into(),
        "--skill".into(),
        skill_name.into(),
        "-y".into(),
    ]
}

/// 将 CLI 产物迁到 `base/skills/<name>/`。已在目标则 Ok。
pub fn relocate_skill_to_public(base: &Path, name: &str) -> Result<()> {
    let dest = base.join("skills").join(name);
    if dest.join("SKILL.md").is_file() {
        return Ok(());
    }

    let candidates = [
        base.join(".agents/skills").join(name),
        base.join(".cursor/skills").join(name),
    ];

    let src = candidates
        .into_iter()
        .find(|p| p.join("SKILL.md").is_file())
        .ok_or_else(|| anyhow!("skill `{name}` not found under .agents/.cursor after install"))?;

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    if dest.exists() {
        fs::remove_dir_all(&dest)?;
    }

    match fs::rename(&src, &dest) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_dir_recursive(&src, &dest)?;
            fs::remove_dir_all(&src).ok();
            if dest.join("SKILL.md").is_file() {
                Ok(())
            } else {
                Err(anyhow!("relocate copy failed for `{name}`"))
            }
        }
    }
}

/// 递归复制目录（`rename` 跨盘失败时的回退）。
fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src).with_context(|| format!("read {}", src.display()))? {
        let entry = entry?;
        let to = dest.join(entry.file_name());
        let ft = entry.file_type()?;
        if ft.is_dir() {
            copy_dir_recursive(&entry.path(), &to)?;
        } else {
            fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// 一次播种运行的结果汇总。
#[derive(Debug, Default, Clone)]
pub struct SeedReport {
    /// 新安装成功的名称。
    pub installed: Vec<String>,
    /// 安装失败的名称。
    pub failed: Vec<String>,
    /// 已存在而跳过的名称。
    pub skipped: Vec<String>,
}

/// 可注入的安装回调：`(source, skill_name) -> Result`。
type InstallFn<'a> = dyn Fn(&str, &str) -> Result<()> + 'a;

/// 使用自定义 `install` 回调播种 `catalog`（便于单测）。
pub fn seed_with_installer(
    base: &Path,
    catalog: &[DefaultSkillSeed],
    install: &InstallFn<'_>,
) -> SeedReport {
    let _ = fs::create_dir_all(base.join("skills"));
    let mut report = SeedReport::default();

    for skill in catalog {
        if is_public_skill_installed(base, skill.name) {
            report.skipped.push(skill.name.to_string());
            continue;
        }
        match install(skill.source, skill.name) {
            Ok(()) => match relocate_skill_to_public(base, skill.name) {
                Ok(()) if is_public_skill_installed(base, skill.name) => {
                    report.installed.push(skill.name.to_string());
                }
                Ok(()) | Err(_) => report.failed.push(skill.name.to_string()),
            },
            Err(_) => report.failed.push(skill.name.to_string()),
        }
    }
    report
}

/// 用 `npx skills add` 安装单个默认公共 skill。
fn run_npx_seed(base: &Path, source: &str, name: &str) -> Result<()> {
    let args = build_seed_npx_args(source, name);
    let output = Command::new("npx")
        .args(&args)
        .current_dir(base)
        .output()
        .context("执行 npx 失败（请确认已安装 Node.js）")?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow!("npx skills add failed for {name}: {stderr}"))
    }
}

/// 补装缺失的默认公共 skills → `ASTRO_MEMORY_DIR` 或 `~/.astro/skills`
pub fn seed_default_public_skills() -> SeedReport {
    let base = memory_dir();
    let _ = fs::create_dir_all(&base);
    seed_with_installer(&base, DEFAULT_PUBLIC_SKILLS, &|source, name| {
        run_npx_seed(&base, source, name)
    })
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;
    use tempfile::tempdir;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn default_catalog_has_four_skills() {
        assert_eq!(DEFAULT_PUBLIC_SKILLS.len(), 4);
        let names: Vec<_> = DEFAULT_PUBLIC_SKILLS.iter().map(|s| s.name).collect();
        assert!(names.contains(&"find-skills"));
        assert!(names.contains(&"skill-creator"));
        assert!(names.contains(&"brainstorming"));
        assert!(names.contains(&"agent-browser"));
    }

    #[test]
    fn is_installed_when_skill_md_exists() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        let skill = dir.path().join("skills/find-skills");
        fs::create_dir_all(&skill).unwrap();
        fs::write(skill.join("SKILL.md"), "---\nname: find-skills\n---\n").unwrap();
        assert!(is_public_skill_installed(dir.path(), "find-skills"));
        assert!(!is_public_skill_installed(dir.path(), "brainstorming"));
    }

    #[test]
    fn relocate_moves_agents_skill_into_public_skills() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        let src = dir.path().join(".agents/skills/brainstorming");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("SKILL.md"), "---\nname: brainstorming\n---\n# B\n").unwrap();

        relocate_skill_to_public(dir.path(), "brainstorming").unwrap();

        assert!(is_public_skill_installed(dir.path(), "brainstorming"));
        assert!(!src.join("SKILL.md").is_file());
    }

    #[test]
    fn build_seed_npx_args_includes_skill_and_yes() {
        let args = build_seed_npx_args(
            "https://github.com/obra/superpowers",
            "brainstorming",
        );
        assert_eq!(
            args,
            vec![
                "--yes".to_string(),
                "skills".to_string(),
                "add".to_string(),
                "https://github.com/obra/superpowers".to_string(),
                "--skill".to_string(),
                "brainstorming".to_string(),
                "-y".to_string(),
            ]
        );
    }

    #[test]
    fn relocate_noop_if_already_in_public() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        let dest = dir.path().join("skills/find-skills");
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("SKILL.md"), "---\nname: find-skills\n---\n").unwrap();

        relocate_skill_to_public(dir.path(), "find-skills").unwrap();
        assert!(is_public_skill_installed(dir.path(), "find-skills"));
    }

    #[test]
    fn seed_skips_installed_and_installs_missing() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        let existing = dir.path().join("skills/find-skills");
        fs::create_dir_all(&existing).unwrap();
        fs::write(existing.join("SKILL.md"), "---\nname: find-skills\n---\n").unwrap();

        let report = seed_with_installer(dir.path(), &DEFAULT_PUBLIC_SKILLS[..2], &|_source, name| {
            // 模拟 npx：写入 .agents/skills/<name>
            let src = dir.path().join(".agents/skills").join(name);
            fs::create_dir_all(&src).unwrap();
            fs::write(src.join("SKILL.md"), format!("---\nname: {name}\n---\n")).unwrap();
            Ok(())
        });

        assert_eq!(report.skipped, vec!["find-skills".to_string()]);
        assert_eq!(report.installed, vec!["skill-creator".to_string()]);
        assert!(report.failed.is_empty());
        assert!(is_public_skill_installed(dir.path(), "skill-creator"));
        std::env::remove_var("ASTRO_MEMORY_DIR");
    }

    #[test]
    fn seed_records_failure_and_continues() {
        let _g = ENV_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        let catalog = &DEFAULT_PUBLIC_SKILLS[..2];
        let report = seed_with_installer(dir.path(), catalog, &|_s, name| {
            if name == "find-skills" {
                Err(anyhow::anyhow!("npx boom"))
            } else {
                let src = dir.path().join(".agents/skills").join(name);
                fs::create_dir_all(&src).unwrap();
                fs::write(src.join("SKILL.md"), format!("---\nname: {name}\n---\n")).unwrap();
                Ok(())
            }
        });
        assert_eq!(report.failed, vec!["find-skills".to_string()]);
        assert_eq!(report.installed, vec!["skill-creator".to_string()]);
    }
}
