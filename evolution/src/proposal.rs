//! 提案队列：进化候选落盘为待审提案；审批后写入 Agent skills 目录。
//!
//! 路径：`{base}/learning/evolution/proposals/{id}.json`。
//! apply 语义对齐 `skills` 工具：新建写 SKILL.md、patch 唯一替换；仅限 agent
//! skills 目录（`skills::install::agent_skills_dir`）。

use std::fs;
use std::path::{Path, PathBuf};

use crate::candidate::{CandidateKind, SkillCandidate};
use crate::reflect::valid_skill_id;

/// `{base}/learning/evolution/proposals`
pub fn proposals_dir(base: &Path) -> PathBuf {
    base.join("learning").join("evolution").join("proposals")
}

fn proposal_path(base: &Path, id: &str) -> PathBuf {
    proposals_dir(base).join(format!("{id}.json"))
}

/// 批量保存候选为提案（各一个 JSON 文件）。
pub fn save_proposals(base: &Path, candidates: &[SkillCandidate]) -> anyhow::Result<()> {
    let dir = proposals_dir(base);
    fs::create_dir_all(&dir)?;
    for c in candidates {
        let path = proposal_path(base, &c.id);
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(c)?)?;
        fs::rename(&tmp, &path)?;
    }
    Ok(())
}

/// 列出全部待审提案（按创建时间升序）。
pub fn list_proposals(base: &Path) -> Vec<SkillCandidate> {
    let dir = proposals_dir(base);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<SkillCandidate> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
        .filter_map(|e| fs::read_to_string(e.path()).ok())
        .filter_map(|raw| serde_json::from_str::<SkillCandidate>(&raw).ok())
        .collect();
    out.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    out
}

fn get_proposal(base: &Path, id: &str) -> anyhow::Result<SkillCandidate> {
    let raw = fs::read_to_string(proposal_path(base, id))
        .map_err(|_| anyhow::anyhow!("提案不存在: {id}"))?;
    Ok(serde_json::from_str(&raw)?)
}

/// 拒绝并删除提案。
pub fn reject_proposal(base: &Path, id: &str) -> anyhow::Result<()> {
    let path = proposal_path(base, id);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

/// 渲染新建技能的 SKILL.md 文本（无 frontmatter 时自动补 name/description）。
pub fn candidate_new_markdown(cand: &SkillCandidate) -> anyhow::Result<String> {
    let body = cand
        .content
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("新建候选缺少 content"))?;
    if body.starts_with("---") {
        Ok(body.to_string())
    } else {
        let desc = cand
            .description
            .as_deref()
            .unwrap_or("")
            .trim()
            .replace('"', "'");
        Ok(format!(
            "---\nname: {}\ndescription: \"{desc}\"\n---\n\n{body}\n",
            cand.skill_id
        ))
    }
}

/// 唯一字符串替换：`old` 须在 `text` 中恰好出现一次。
pub fn apply_patch_unique(text: &str, old: &str, new: &str) -> anyhow::Result<String> {
    match text.matches(old).count() {
        0 => anyhow::bail!("patch old_string 未命中（技能可能已改）"),
        1 => Ok(text.replacen(old, new, 1)),
        n => anyhow::bail!("patch old_string 命中 {n} 处，不唯一"),
    }
}

/// 应用回滚令牌：写入失败或测试不过时恢复原状。
enum Rollback {
    /// 目录原本不存在：整树删除。
    RemoveDir(PathBuf),
    /// 目录已存在：把 SKILL.md 还原为原内容（None = 原本无此文件）。
    RestoreFile(PathBuf, Option<String>),
}

impl Rollback {
    fn run(self) {
        match self {
            Rollback::RemoveDir(dir) => {
                let _ = fs::remove_dir_all(&dir);
            }
            Rollback::RestoreFile(path, old) => match old {
                Some(text) => {
                    let _ = fs::write(&path, text.as_bytes());
                }
                None => {
                    let _ = fs::remove_file(&path);
                }
            },
        }
    }
}

/// 将候选写入 Agent skills，返回（摘要, 技能目录, 回滚令牌）。
fn apply_candidate(cand: &SkillCandidate) -> anyhow::Result<(String, PathBuf, Rollback)> {
    if !valid_skill_id(&cand.skill_id) {
        anyhow::bail!("非法 skill_id: {}", cand.skill_id);
    }
    let skills_dir = skills::install::agent_skills_dir(None)?;
    let dest = skills_dir.join(&cand.skill_id);
    let agent_root = skills_dir
        .canonicalize()
        .unwrap_or_else(|_| skills_dir.clone());
    let skill_md = dest.join("SKILL.md");
    let dir_existed = dest.exists();

    match cand.kind {
        CandidateKind::NewSkill => {
            let md = candidate_new_markdown(cand)?;
            let prev = fs::read_to_string(&skill_md).ok();
            fs::create_dir_all(&dest)?;
            fs::write(&skill_md, md.as_bytes())?;
            let _ = skills::set_enabled(&cand.skill_id, true);
            let rollback = if dir_existed {
                Rollback::RestoreFile(skill_md, prev)
            } else {
                Rollback::RemoveDir(dest.clone())
            };
            Ok((format!("已新建技能 `{}`", cand.skill_id), dest, rollback))
        }
        CandidateKind::Patch => {
            if !skill_md.is_file() {
                anyhow::bail!("patch 目标不存在于 Agent skills: {}", cand.skill_id);
            }
            let canon_parent = skill_md
                .parent()
                .and_then(|p| p.canonicalize().ok())
                .unwrap_or_else(|| dest.clone());
            if !canon_parent.starts_with(&agent_root) {
                anyhow::bail!("路径越界：拒绝修改 Agent skills 之外的文件");
            }
            let old = cand
                .old_string
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("patch 缺少 old_string"))?;
            let new = cand
                .new_string
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("patch 缺少 new_string"))?;
            let text = fs::read_to_string(&skill_md)?;
            let updated = apply_patch_unique(&text, old, new)?;
            let rollback = Rollback::RestoreFile(skill_md.clone(), Some(text));
            fs::write(&skill_md, updated.as_bytes())?;
            Ok((format!("已 patch 技能 `{}`", cand.skill_id), dest, rollback))
        }
    }
}

/// 批准提案：写入 Agent skills 目录，成功后删除提案文件，返回摘要。
pub fn approve_proposal(base: &Path, id: &str) -> anyhow::Result<String> {
    approve_proposal_checked(base, id, |_dir| Ok(()))
}

/// 批准提案并在写入后运行校验闭包（如 run_tests）；校验失败自动回滚并报错。
///
/// `check` 收到已写入的技能目录路径；返回 `Err` 时回滚本次写入且不删除提案。
pub fn approve_proposal_checked<F>(base: &Path, id: &str, check: F) -> anyhow::Result<String>
where
    F: FnOnce(&Path) -> Result<(), String>,
{
    let cand = get_proposal(base, id)?;
    let (summary, skill_dir, rollback) = apply_candidate(&cand)?;

    if let Err(reason) = check(&skill_dir) {
        rollback.run();
        if cand.kind == CandidateKind::NewSkill {
            let _ = skills::set_enabled(&cand.skill_id, false);
        }
        anyhow::bail!("测试未通过，已回滚: {reason}");
    }

    reject_proposal(base, id)?; // 删除已应用提案
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::candidate::CandidateKind;
    use tempfile::TempDir;

    fn new_cand(id: &str, skill: &str) -> SkillCandidate {
        SkillCandidate {
            id: id.into(),
            kind: CandidateKind::NewSkill,
            skill_id: skill.into(),
            description: Some("demo".into()),
            content: Some("# Demo\n步骤一".into()),
            old_string: None,
            new_string: None,
            rationale: "复用".into(),
            sources: vec![],
            judge_score: None,
            judge_reason: None,
            created_at: "2026-07-18T00:00:00Z".into(),
        }
    }

    #[test]
    fn save_list_reject_roundtrip() {
        let dir = TempDir::new().unwrap();
        save_proposals(dir.path(), &[new_cand("a", "demo-a"), new_cand("b", "demo-b")]).unwrap();
        let listed = list_proposals(dir.path());
        assert_eq!(listed.len(), 2);
        reject_proposal(dir.path(), "a").unwrap();
        assert_eq!(list_proposals(dir.path()).len(), 1);
    }

    #[test]
    fn approve_new_skill_writes_and_removes_proposal() {
        let dir = TempDir::new().unwrap();
        let _env = home::test_env::AstroMemoryDirGuard::set(dir.path());
        save_proposals(dir.path(), &[new_cand("x", "demo-approve")]).unwrap();
        let msg = approve_proposal(dir.path(), "x").unwrap();
        assert!(msg.contains("已新建"));
        let skills_dir = skills::install::agent_skills_dir(None).unwrap();
        assert!(skills_dir.join("demo-approve/SKILL.md").is_file());
        assert!(list_proposals(dir.path()).is_empty());
    }

    #[test]
    fn approve_checked_rolls_back_new_on_failure() {
        let dir = TempDir::new().unwrap();
        let _env = home::test_env::AstroMemoryDirGuard::set(dir.path());
        save_proposals(dir.path(), &[new_cand("f", "demo-fail")]).unwrap();
        let err = approve_proposal_checked(dir.path(), "f", |_dir| Err("boom".into()));
        assert!(err.is_err());
        let skills_dir = skills::install::agent_skills_dir(None).unwrap();
        // 新建应被整树回滚
        assert!(!skills_dir.join("demo-fail").exists());
        // 提案保留（未删除）
        assert_eq!(list_proposals(dir.path()).len(), 1);
    }

    #[test]
    fn approve_checked_keeps_on_success() {
        let dir = TempDir::new().unwrap();
        let _env = home::test_env::AstroMemoryDirGuard::set(dir.path());
        save_proposals(dir.path(), &[new_cand("s", "demo-ok")]).unwrap();
        let msg = approve_proposal_checked(dir.path(), "s", |dir| {
            assert!(dir.join("SKILL.md").is_file());
            Ok(())
        })
        .unwrap();
        assert!(msg.contains("已新建"));
        assert!(list_proposals(dir.path()).is_empty());
    }

    #[test]
    fn approve_patch_unique_replace() {
        let dir = TempDir::new().unwrap();
        let _env = home::test_env::AstroMemoryDirGuard::set(dir.path());
        // 先建技能
        save_proposals(dir.path(), &[new_cand("n", "demo-patch")]).unwrap();
        approve_proposal(dir.path(), "n").unwrap();
        // patch 它
        let patch = SkillCandidate {
            id: "p".into(),
            kind: CandidateKind::Patch,
            skill_id: "demo-patch".into(),
            description: None,
            content: None,
            old_string: Some("步骤一".into()),
            new_string: Some("步骤一（改）".into()),
            rationale: "fix".into(),
            sources: vec![],
            judge_score: None,
            judge_reason: None,
            created_at: "2026-07-18T00:01:00Z".into(),
        };
        save_proposals(dir.path(), &[patch]).unwrap();
        let msg = approve_proposal(dir.path(), "p").unwrap();
        assert!(msg.contains("已 patch"));
        let skills_dir = skills::install::agent_skills_dir(None).unwrap();
        let body = fs::read_to_string(skills_dir.join("demo-patch/SKILL.md")).unwrap();
        assert!(body.contains("步骤一（改）"));
    }
}
