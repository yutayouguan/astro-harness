//! Astro 离线进化引擎（Rust 内置极简版）。
//!
//! 单轮反思式：读执行轨迹（DecisionLog）+ 已启用 Skills 索引 → reflection 模型
//! 产出技能候选（新建 / patch）→ gates 过滤 → 存「待审提案」；用户在 UI 审批后
//! 才写入 agent skills 目录（require_pr 语义 = 人工审批）。
//!
//! LLM 调用不在本 crate：调用方（Tauri）注入已解析的 reflection/judge 目标并
//! 用 [`REFLECTION_SYSTEM_PROMPT`] + [`build_reflection_user_prompt`] 得到文本，
//! 再把模型输出交给 [`parse_candidates`]。
//!
//! 非目标：完整 GEPA/Pareto 遗传搜索、自动应用、git/PR 自动化。

pub mod candidate;
pub mod gates;
pub mod judge;
pub mod proposal;
pub mod reflect;

pub use candidate::{CandidateKind, SkillCandidate};
pub use gates::{check_candidate, GateOutcome};
pub use judge::{build_judge_user_prompt, parse_judge_output, JudgeVerdict, JUDGE_SYSTEM_PROMPT};
pub use proposal::{
    approve_proposal, approve_proposal_checked, list_proposals, proposals_dir, reject_proposal,
    save_proposals,
};
pub use reflect::{
    build_reflection_user_prompt, parse_candidates, ReflectionInput, REFLECTION_SYSTEM_PROMPT,
};
