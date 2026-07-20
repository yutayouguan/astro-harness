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
//! 非目标：完整外部 DSPy/GEPA 遗传引擎内置、自动应用、git/PR 全自动
//! （本 crate 已含 GEPA-lite：变异 / 交叉 / Pareto；「批准到分支」仅为可选辅助）。

pub mod auto;
pub mod candidate;
pub mod curator;
pub mod evalset;
pub mod gates;
pub mod history;
pub mod judge;
pub mod proposal;
pub mod reflect;
pub mod search;

pub use auto::{
    auto_state_path, build_auto_status, count_new_decisions, evaluate_auto_gate, load_auto_state,
    mark_auto_run, save_auto_state, AutoGate, AutoState, AutoStatus, SkipReason,
};
pub use candidate::{CandidateKind, SkillCandidate};
pub use curator::{
    apply_diagnoses, build_curator_status, build_diagnose_prompt, curator_last_path,
    enqueue_curator_suggestions, evaluate_curator_due, find_overlap_clusters, load_curator_last,
    parse_diagnose_output, parse_generated_at, run_curator, run_curator_and_save,
    run_curator_with_skills, suggestions_to_candidates, CurateReport, CurateSkillRow,
    CurateSuggestion, CuratorDue, CuratorDueReason, CuratorStatus, CURATOR_DIAGNOSE_SYSTEM_PROMPT,
};
pub use evalset::{
    aggregate_critiques, append_example, build_eval_judge_prompt, default_holdout_percent,
    evalset_path, examples_for_skill, list_examples, parse_eval_judgement, parse_eval_score,
    remove_example, split_eval_examples, weighted_eval_score, EvalExample, EvalJudgement,
    EvalSplit, Verdict, EVAL_JUDGE_SYSTEM_PROMPT,
};
pub use gates::{
    check_candidate, run_skill_tests_in_dir, sandbox_test_candidate, GateOutcome, TestOutcome,
};
pub use history::{
    history_path, list_all as list_history, record_outcome, record_run, record_run_meta,
    summarize as summarize_history, HistoryEvent, HistorySummary, SearchRunMeta,
};
pub use judge::{build_judge_user_prompt, parse_judge_output, JudgeVerdict, JUDGE_SYSTEM_PROMPT};
pub use proposal::{
    apply_patch_unique, approve_proposal, approve_proposal_checked, candidate_new_markdown,
    list_proposals, proposals_dir, reject_proposal, save_proposals,
};
pub use reflect::{
    build_reflection_user_prompt, parse_candidates, ReflectionInput, REFLECTION_SYSTEM_PROMPT,
};
pub use search::{
    build_crossover_prompt, build_mutation_prompt, candidate_fingerprint,
    effective_candidate_size, pareto_front, parse_variants, select_front_capped,
    select_population, ScoredVariant, SearchBudget, CROSSOVER_SYSTEM_PROMPT,
    MUTATION_SYSTEM_PROMPT,
};
