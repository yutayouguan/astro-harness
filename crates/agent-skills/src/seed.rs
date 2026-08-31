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
}

/// 仓库内置 Skill（编译期嵌入 `skills/bundled/`）。
const BUNDLED_STORYBOARD_VIDEO_MD: &str = include_str!("../bundled/storyboard-video/SKILL.md");
const BUNDLED_CREATE_AGENT_MD: &str = include_str!("../bundled/create-agent/SKILL.md");
const BUNDLED_AIHOT_MD: &str = include_str!("../bundled/aihot/SKILL.md");
const BUNDLED_CREATIVE_MEDIA_MD: &str = include_str!("../bundled/creative-media/SKILL.md");

/// 内置 Skill 清单：`(目录名, SKILL.md 正文)`。
pub const BUNDLED_SKILLS: &[(&str, &str)] = &[
    ("aihot", BUNDLED_AIHOT_MD),
    ("create-agent", BUNDLED_CREATE_AGENT_MD),
    ("creative-media", BUNDLED_CREATIVE_MEDIA_MD),
    ("storyboard-video", BUNDLED_STORYBOARD_VIDEO_MD),
];

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

/// 将内置 Skill 写入 `base/skills/<name>/`。
///
/// - 不存在 → 安装  
/// - 已存在且 `astro_bundled_rev` ≥ 内置版本 → 跳过  
/// - 已存在但版本落后或无 rev → 覆盖更新 SKILL.md（便于补装新版分镜 Skill）
pub fn seed_bundled_into(base: &Path) -> SeedReport {
    let _ = fs::create_dir_all(base.join("skills"));
    let mut report = SeedReport::default();
    for &(name, body) in BUNDLED_SKILLS {
        let dest = base.join("skills").join(name);
        let skill_md = dest.join("SKILL.md");
        let want = bundled_rev_in(body);
        if skill_md.is_file() {
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
    fn seed_bundled_installs_create_agent_and_storyboard() {
        let dir = tempdir().unwrap();
        let r1 = seed_bundled_into(dir.path());
        assert!(r1.installed.contains(&"aihot".to_string()));
        assert!(r1.installed.contains(&"create-agent".to_string()));
        assert!(r1.installed.contains(&"creative-media".to_string()));
        assert!(r1.installed.contains(&"storyboard-video".to_string()));
        assert!(dir.path().join("skills/aihot/SKILL.md").is_file());
        assert!(dir.path().join("skills/create-agent/SKILL.md").is_file());
        assert!(dir.path().join("skills/creative-media/SKILL.md").is_file());
        assert!(dir
            .path()
            .join("skills/storyboard-video/SKILL.md")
            .is_file());
        let body = fs::read_to_string(dir.path().join("skills/create-agent/SKILL.md")).unwrap();
        assert!(body.contains("create-agent"));
        assert!(body.contains("astro_bundled_rev:"));
        assert!(body.contains("spawn_agent"));
        assert!(body.contains("persona_create"));
        let aihot = fs::read_to_string(dir.path().join("skills/aihot/SKILL.md")).unwrap();
        assert!(aihot.contains("aihot.virxact.com"));
        let creative =
            fs::read_to_string(dir.path().join("skills/creative-media/SKILL.md")).unwrap();
        assert!(creative.contains("music_gen"));
        assert!(creative.contains("ask_user"));
        let r2 = seed_bundled_into(dir.path());
        assert!(r2.installed.is_empty());
        assert!(r2.skipped.contains(&"aihot".to_string()));
        assert!(r2.skipped.contains(&"create-agent".to_string()));
        assert!(r2.skipped.contains(&"creative-media".to_string()));
        assert!(r2.skipped.contains(&"storyboard-video".to_string()));
    }

    #[test]
    fn seed_bundled_upgrades_stale_create_agent_without_rev() {
        let dir = tempdir().unwrap();
        let dest = dir.path().join("skills/create-agent");
        fs::create_dir_all(&dest).unwrap();
        fs::write(
            dest.join("SKILL.md"),
            "---\nname: create-agent\n---\nold create-agent body\n",
        )
        .unwrap();
        let r = seed_bundled_into(dir.path());
        assert!(r.installed.contains(&"create-agent".to_string()));
        let body = fs::read_to_string(dest.join("SKILL.md")).unwrap();
        assert!(bundled_rev_in(&body) >= 1);
        assert!(body.contains("spawn_agent"));
        assert!(body.contains("persona_create"));
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
        assert!(r.installed.contains(&"create-agent".to_string()));
        let body = fs::read_to_string(dest.join("SKILL.md")).unwrap();
        assert!(bundled_rev_in(&body) >= 2);
        assert!(body.contains("先出图再出视频"));
    }
}
