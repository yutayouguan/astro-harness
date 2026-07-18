//! 技能候选类型：新建或对已有 SKILL.md 的唯一 patch。

use serde::{Deserialize, Serialize};

/// 候选类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateKind {
    /// 新建 Skill。
    NewSkill,
    /// 对已有 Skill 的唯一字符串替换。
    Patch,
}

/// 一条进化候选（也是持久化的提案单元）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillCandidate {
    /// 提案 id（uuid）。
    pub id: String,
    pub kind: CandidateKind,
    /// 目标 skill 名称 / id。
    pub skill_id: String,
    /// 新建时的描述（写入 frontmatter）。
    #[serde(default)]
    pub description: Option<String>,
    /// 新建时的 SKILL.md 正文（可含 frontmatter）。
    #[serde(default)]
    pub content: Option<String>,
    /// patch：原文（须唯一匹配）。
    #[serde(default)]
    pub old_string: Option<String>,
    /// patch：替换文本。
    #[serde(default)]
    pub new_string: Option<String>,
    /// 提出该候选的理由（来自 reflection 模型）。
    #[serde(default)]
    pub rationale: String,
    /// 关联的决策来源（DecisionLog summary/id 等）。
    #[serde(default)]
    pub sources: Vec<String>,
    /// judge 模型打分（0–1）；未评分为 None。
    #[serde(default)]
    pub judge_score: Option<f32>,
    /// judge 评语。
    #[serde(default)]
    pub judge_reason: Option<String>,
    /// 创建时间 RFC3339。
    pub created_at: String,
}

impl SkillCandidate {
    /// 估算写入体积（字节）：新建取 content，patch 取 new_string。
    pub fn payload_len(&self) -> usize {
        match self.kind {
            CandidateKind::NewSkill => self.content.as_deref().map(str::len).unwrap_or(0),
            CandidateKind::Patch => self.new_string.as_deref().map(str::len).unwrap_or(0),
        }
    }
}
