//! 首次启动时写入随应用编译发布的内置 Skills。

use std::fs;
use std::path::{Path, PathBuf};

/// 一次播种运行的结果汇总。
#[derive(Debug, Default, Clone)]
pub struct SeedReport {
    /// 新安装成功的名称。
    pub installed: Vec<String>,
    /// 安装失败的名称。
    pub failed: Vec<String>,
    /// 已存在而跳过的名称。
    pub skipped: Vec<String>,
    /// 已从用户技能目录移除的退役内置 Skill。
    pub removed: Vec<String>,
}

/// 仓库内置 Skill（编译期嵌入 `skills/bundled/`）。
const BUNDLED_STORYBOARD_VIDEO_MD: &str = include_str!("../bundled/storyboard-video/SKILL.md");
const BUNDLED_AIHOT_MD: &str = include_str!("../bundled/aihot/SKILL.md");
const BUNDLED_CREATIVE_MEDIA_MD: &str = include_str!("../bundled/creative-media/SKILL.md");

/// 内置 Skill 清单：`(目录名, SKILL.md 正文)`。
pub const BUNDLED_SKILLS: &[(&str, &str)] = &[
    ("aihot", BUNDLED_AIHOT_MD),
    ("creative-media", BUNDLED_CREATIVE_MEDIA_MD),
    ("storyboard-video", BUNDLED_STORYBOARD_VIDEO_MD),
];

/// 由 SkillHub 承接后续更新的内置 Skill 基线。
#[derive(Debug, Clone, Copy)]
pub struct BundledSkillHubSource {
    pub folder: &'static str,
    pub install_ref: &'static str,
    pub version: &'static str,
    pub updated_at: i64,
}

pub const BUNDLED_SKILLHUB_SOURCES: &[BundledSkillHubSource] = &[BundledSkillHubSource {
    folder: "aihot",
    install_ref: "skillhub:kkkkhazix/aihot",
    version: "0.1.1",
    updated_at: 1_788_148_472_699,
}];

/// 已从产品中退役、需清理旧播种副本的内置 Skill。
const RETIRED_BUNDLED_SKILLS: &[&str] = &["create-agent"];

/// 从 SKILL.md frontmatter 解析 `astro_bundled_rev`（缺省 0）。
fn bundled_rev_in(body: &str) -> u32 {
    for line in body.lines().take(40) {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("astro_bundled_rev:") {
            if let Ok(n) = rest.trim().parse::<u32>() {
                return n;
            }
        }
    }
    0
}

fn is_skillhub_managed(name: &str) -> bool {
    BUNDLED_SKILLHUB_SOURCES
        .iter()
        .any(|source| source.folder == name)
}

fn remove_retired_bundled_skills(base: &Path, report: &mut SeedReport) {
    for name in RETIRED_BUNDLED_SKILLS {
        let dest = base.join("skills").join(name);
        let skill_md = dest.join("SKILL.md");
        let is_seeded_copy = fs::read_to_string(&skill_md)
            .map(|body| bundled_rev_in(&body) > 0)
            .unwrap_or(false);
        if !is_seeded_copy {
            continue;
        }
        match fs::remove_dir_all(&dest) {
            Ok(()) => report.removed.push((*name).to_string()),
            Err(_) => report.failed.push((*name).to_string()),
        }
    }
}

/// 将内置 Skill 写入 `base/skills/<name>/`。
///
/// - 不存在 → 安装
/// - 由 SkillHub 管理且已存在 → 跳过，避免覆盖远程更新
/// - 其他内置 Skill 已是最新 bundled rev → 跳过
/// - 其他内置 Skill 版本落后或无 rev → 覆盖更新 SKILL.md
pub fn seed_bundled_into(base: &Path) -> SeedReport {
    let _ = fs::create_dir_all(base.join("skills"));
    let mut report = SeedReport::default();
    remove_retired_bundled_skills(base, &mut report);
    for &(name, body) in BUNDLED_SKILLS {
        let dest = base.join("skills").join(name);
        let skill_md = dest.join("SKILL.md");
        let want = bundled_rev_in(body);
        if skill_md.is_file() {
            if is_skillhub_managed(name) {
                report.skipped.push(name.to_string());
                continue;
            }
            let have = fs::read_to_string(&skill_md)
                .map(|s| bundled_rev_in(&s))
                .unwrap_or(0);
            if have >= want && want > 0 {
                report.skipped.push(name.to_string());
                continue;
            }
        }
        match fs::create_dir_all(&dest).and_then(|_| fs::write(&skill_md, body)) {
            Ok(()) => report.installed.push(name.to_string()),
            Err(_) => report.failed.push(name.to_string()),
        }
    }
    report
}

/// 补装随应用发布的内置 Skills → `ASTRO_MEMORY_DIR` 或 `~/.astro/skills`。
pub fn seed_bundled_skills() -> SeedReport {
    let base = memory_dir();
    let _ = fs::create_dir_all(&base);
    seed_bundled_into(&base)
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
    use tempfile::tempdir;

    #[test]
    fn seed_bundled_installs_supported_skills() {
        let dir = tempdir().unwrap();
        let r1 = seed_bundled_into(dir.path());
        assert!(r1.installed.contains(&"aihot".to_string()));
        assert!(r1.installed.contains(&"creative-media".to_string()));
        assert!(r1.installed.contains(&"storyboard-video".to_string()));
        assert!(dir.path().join("skills/aihot/SKILL.md").is_file());
        assert!(dir.path().join("skills/creative-media/SKILL.md").is_file());
        assert!(dir
            .path()
            .join("skills/storyboard-video/SKILL.md")
            .is_file());
        let aihot = fs::read_to_string(dir.path().join("skills/aihot/SKILL.md")).unwrap();
        assert!(aihot.contains("aihot.virxact.com"));
        let creative =
            fs::read_to_string(dir.path().join("skills/creative-media/SKILL.md")).unwrap();
        assert!(creative.contains("music_gen"));
        assert!(creative.contains("ask_user"));
        assert!(creative.contains("astro_tools: [music_gen, image_gen, speech_gen, video_gen]"));
        let storyboard =
            fs::read_to_string(dir.path().join("skills/storyboard-video/SKILL.md")).unwrap();
        assert!(storyboard.contains("astro_tools: [image_gen, video_gen]"));
        let r2 = seed_bundled_into(dir.path());
        assert!(r2.installed.is_empty());
        assert!(r2.skipped.contains(&"aihot".to_string()));
        assert!(r2.skipped.contains(&"creative-media".to_string()));
        assert!(r2.skipped.contains(&"storyboard-video".to_string()));
    }

    #[test]
    fn seed_bundled_removes_retired_create_agent_snapshot() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("skills/create-agent");
        fs::create_dir_all(&dest).unwrap();
        fs::write(
            dest.join("SKILL.md"),
            "---\nname: create-agent\nastro_bundled_rev: 5\n---\nold bundled body\n",
        )
        .unwrap();
        let r = seed_bundled_into(dir.path());
        assert!(r.removed.contains(&"create-agent".to_string()));
        assert!(!dest.exists());
    }

    #[test]
    fn seed_bundled_preserves_user_owned_create_agent() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("skills/create-agent");
        fs::create_dir_all(&dest).unwrap();
        fs::write(
            dest.join("SKILL.md"),
            "---\nname: create-agent\n---\ncustom\n",
        )
        .unwrap();

        let r = seed_bundled_into(dir.path());

        assert!(!r.removed.contains(&"create-agent".to_string()));
        assert!(dest.join("SKILL.md").is_file());
    }

    #[test]
    fn seed_bundled_preserves_skillhub_managed_aihot() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("skills/aihot");
        fs::create_dir_all(&dest).unwrap();
        let remote_body = "---\nname: aihot\nversion: 9.9.9\n---\nremote update\n";
        fs::write(dest.join("SKILL.md"), remote_body).unwrap();

        let report = seed_bundled_into(dir.path());

        assert!(report.skipped.contains(&"aihot".to_string()));
        assert_eq!(
            fs::read_to_string(dest.join("SKILL.md")).unwrap(),
            remote_body
        );
    }

    #[test]
    fn seed_bundled_storyboard_video_once() {
        let dir = tempdir().unwrap();
        let r1 = seed_bundled_into(dir.path());
        assert!(r1.installed.contains(&"storyboard-video".to_string()));
        assert!(dir
            .path()
            .join("skills/storyboard-video/SKILL.md")
            .is_file());
        let body = fs::read_to_string(dir.path().join("skills/storyboard-video/SKILL.md")).unwrap();
        assert!(body.contains("storyboard-video"));
        assert!(body.contains("astro_bundled_rev:"));
        let r2 = seed_bundled_into(dir.path());
        assert!(!r2.installed.contains(&"storyboard-video".to_string()));
        assert!(r2.skipped.contains(&"storyboard-video".to_string()));
    }

    #[test]
    fn seed_bundled_upgrades_stale_rev() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("skills/storyboard-video");
        fs::create_dir_all(&dest).unwrap();
        fs::write(
            dest.join("SKILL.md"),
            "---\nname: storyboard-video\nastro_bundled_rev: 1\n---\nold\n",
        )
        .unwrap();
        let r = seed_bundled_into(dir.path());
        assert!(r.installed.contains(&"storyboard-video".to_string()));
        let body = fs::read_to_string(dest.join("SKILL.md")).unwrap();
        assert!(bundled_rev_in(&body) >= 2);
        assert!(body.contains("先出图再出视频"));
    }
}
