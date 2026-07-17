//! Reflection：从执行轨迹 + 技能索引构造提示词，并解析模型输出为候选。
//!
//! v1 输入以 DecisionLog（工具失败 / 用户纠错等）+ 已启用 Skills 索引为主；
//! 会话逐字 transcript 富化留待后续。

use chrono::Utc;
use uuid::Uuid;

use memory::DecisionEntry;

use crate::candidate::{CandidateKind, SkillCandidate};

/// reflection 输入。
#[derive(Debug, Clone, Default)]
pub struct ReflectionInput {
    /// 近期决策（升序或降序均可，原样喂给模型）。
    pub decisions: Vec<DecisionEntry>,
    /// 已启用技能 `(name, description)`。
    pub enabled_skills: Vec<(String, String)>,
}

/// 供 reflection 模型的 system 指令：只输出 JSON。
pub const REFLECTION_SYSTEM_PROMPT: &str = r#"你是 Agent 技能进化助手。基于近期执行轨迹（工具失败、用户纠错等）与已有技能，提出可复用的技能改进。只输出 JSON（不要 markdown 围栏）：
{"candidates":[{"kind":"new_skill|patch","skill_id":"kebab-case-name","description":"新建时的一句话描述","content":"新建时的 SKILL.md 正文","old_string":"patch 时被替换的原文(须在该技能 SKILL.md 中唯一出现)","new_string":"patch 时的新文本","rationale":"为什么这样改"}]}
规则：
- 只在确有可复用工作流/修复时提出；没有则返回 {"candidates":[]}。
- new_skill 用 content（可省 frontmatter，会自动补）；patch 用 old_string/new_string。
- skill_id 仅小写字母数字与连字符；不要臆造未发生的失败。
- 最多 3 条，宁缺毋滥。"#;

/// 构造 user 提示词（轨迹 + 技能索引）。
pub fn build_reflection_user_prompt(input: &ReflectionInput) -> String {
    let mut s = String::new();
    s.push_str("## 已启用技能\n");
    if input.enabled_skills.is_empty() {
        s.push_str("（无）\n");
    } else {
        for (name, desc) in &input.enabled_skills {
            let d = desc.replace('\n', " ");
            s.push_str(&format!("- {name}: {d}\n"));
        }
    }
    s.push_str("\n## 近期决策日志\n");
    if input.decisions.is_empty() {
        s.push_str("（无）\n");
    } else {
        for d in &input.decisions {
            let tool = d.tool_name.as_deref().unwrap_or("-");
            s.push_str(&format!(
                "- [{:?}] tool={tool} :: {}\n",
                d.kind, d.summary
            ));
        }
    }
    s.push_str("\n请据此提出技能候选（JSON）。");
    s
}

/// 从模型输出解析候选（容忍 ```json 围栏）。为每条补 id/时间戳。
pub fn parse_candidates(raw: &str) -> anyhow::Result<Vec<SkillCandidate>> {
    let trimmed = raw.trim();
    let start = trimmed
        .find('{')
        .ok_or_else(|| anyhow::anyhow!("reflection 输出不含 JSON 对象"))?;
    let end = trimmed
        .rfind('}')
        .ok_or_else(|| anyhow::anyhow!("reflection 输出缺少 JSON 结尾"))?;
    let json_str = &trimmed[start..=end];

    #[derive(serde::Deserialize)]
    struct RawCandidate {
        kind: String,
        skill_id: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        content: Option<String>,
        #[serde(default)]
        old_string: Option<String>,
        #[serde(default)]
        new_string: Option<String>,
        #[serde(default)]
        rationale: String,
    }
    #[derive(serde::Deserialize)]
    struct RawOut {
        #[serde(default)]
        candidates: Vec<RawCandidate>,
    }

    let parsed: RawOut = serde_json::from_str(json_str)?;
    let now = Utc::now().to_rfc3339();
    let mut out = Vec::new();
    for c in parsed.candidates {
        let kind = match c.kind.trim().to_ascii_lowercase().as_str() {
            "new_skill" | "new" | "create" => CandidateKind::NewSkill,
            "patch" | "update" => CandidateKind::Patch,
            other => {
                tracing::warn!(kind = other, "跳过未知候选 kind");
                continue;
            }
        };
        let skill_id = c.skill_id.trim().to_string();
        if !valid_skill_id(&skill_id) {
            tracing::warn!(skill_id, "跳过非法 skill_id 候选");
            continue;
        }
        // 基本完整性校验
        match kind {
            CandidateKind::NewSkill => {
                if c.content.as_deref().map(str::trim).unwrap_or("").is_empty() {
                    continue;
                }
            }
            CandidateKind::Patch => {
                let old_ok = c.old_string.as_deref().map(str::is_empty) == Some(false);
                let new_ok = c.new_string.is_some();
                if !old_ok || !new_ok {
                    continue;
                }
            }
        }
        out.push(SkillCandidate {
            id: Uuid::new_v4().to_string(),
            kind,
            skill_id,
            description: c.description,
            content: c.content,
            old_string: c.old_string,
            new_string: c.new_string,
            rationale: c.rationale,
            sources: Vec::new(),
            created_at: now.clone(),
        });
    }
    Ok(out)
}

/// skill_id 合法性：仅小写字母数字、`-`、`_`、`.`，不含路径分隔与 `..`。
pub fn valid_skill_id(id: &str) -> bool {
    !id.is_empty()
        && !id.contains("..")
        && !id.contains('/')
        && !id.contains('\\')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use memory::DecisionKind;

    #[test]
    fn parses_candidates_and_filters_invalid() {
        let raw = r##"```json
{"candidates":[
 {"kind":"new_skill","skill_id":"pdf-merge","content":"# PDF Merge\n步骤...","description":"合并 PDF","rationale":"复用"},
 {"kind":"patch","skill_id":"pdf-merge","old_string":"步骤...","new_string":"步骤(改)","rationale":"修正"},
 {"kind":"new_skill","skill_id":"bad id!!","content":"x"},
 {"kind":"patch","skill_id":"pdf-merge","new_string":"only new"},
 {"kind":"weird","skill_id":"pdf-merge"}
]}
```"##;
        let cands = parse_candidates(raw).unwrap();
        assert_eq!(cands.len(), 2);
        assert_eq!(cands[0].kind, CandidateKind::NewSkill);
        assert_eq!(cands[0].skill_id, "pdf-merge");
        assert_eq!(cands[1].kind, CandidateKind::Patch);
    }

    #[test]
    fn empty_candidates_ok() {
        let cands = parse_candidates(r#"{"candidates":[]}"#).unwrap();
        assert!(cands.is_empty());
    }

    #[test]
    fn user_prompt_includes_sources() {
        let input = ReflectionInput {
            decisions: vec![
                DecisionEntry::new(DecisionKind::ToolFailure, "web_search timeout")
                    .with_tool("web_search"),
            ],
            enabled_skills: vec![("aihot".into(), "AI 资讯".into())],
        };
        let p = build_reflection_user_prompt(&input);
        assert!(p.contains("aihot"));
        assert!(p.contains("web_search timeout"));
        assert!(p.contains("ToolFailure"));
    }

    #[test]
    fn valid_skill_id_rules() {
        assert!(valid_skill_id("pdf-merge_1.0"));
        assert!(!valid_skill_id("../x"));
        assert!(!valid_skill_id("a/b"));
        assert!(!valid_skill_id("bad id"));
    }
}
