//! 离线进化「运行 / 提案审批」Tauri 命令。
//!
//! `run_evolution`：读 DecisionLog + 已启用技能 → reflection 模型产候选 → gates
//! 过滤 → 存待审提案（`require_pr` 默认，永不自动应用）。审批走
//! `list/approve/reject_evolution_proposal`。judge 路由已可解析，v1 暂不参与打分。

use futures::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use std::path::{Path, PathBuf};
use std::time::Duration;

use std::collections::{HashMap, HashSet};

use evolution::{
    aggregate_critiques, apply_patch_unique, approve_proposal, approve_proposal_checked,
    build_auto_status, build_crossover_prompt, build_curator_status, build_eval_judge_prompt,
    build_judge_user_prompt, build_mutation_prompt, build_reflection_user_prompt,
    candidate_new_markdown, check_candidate, default_holdout_percent, detect_opportunities,
    effective_candidate_size, enqueue_curator_suggestions, examples_for_skill, list_examples,
    list_proposals, load_auto_state, load_curator_last, mark_auto_run, pareto_front,
    parse_candidates, parse_eval_judgement, parse_judge_output, parse_variants, reject_proposal,
    run_curator_and_save, sandbox_test_candidate, save_auto_state, save_proposals,
    select_front_capped, select_population, skill_last_approved_at, split_eval_examples,
    top_failing_skill, weighted_eval_score, AutoGate, AutoStatus, CandidateKind, CurateReport,
    CuratorStatus, EvalExample, EvalJudgement, FitnessResult, FitnessSideInfo, ReflectionInput,
    ScoredVariant, SearchBudget, SearchRunMeta, SkillCandidate, Verdict, CROSSOVER_SYSTEM_PROMPT,
    EVAL_JUDGE_SYSTEM_PROMPT, JUDGE_SYSTEM_PROMPT, MUTATION_SYSTEM_PROMPT,
    REFLECTION_SYSTEM_PROMPT,
};
use home::default_memory_dir;
use memory::DecisionKind;
use providers::types::message::Message as ProviderMessage;
use providers::types::stream::StreamChunk;
use providers::ProviderConfig;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use super::providers::{self as providers_commands, resolve_api_key, ProviderConfig as UiProvider};
use crate::meta::auxiliary_resolver::{
    resolve_evolution_targets, AuxiliaryTargets, ResolvedTarget,
};

/// 单条提案的展示态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionProposalDto {
    pub id: String,
    pub kind: String,
    pub skill_id: String,
    pub description: Option<String>,
    pub content: Option<String>,
    pub old_string: Option<String>,
    pub new_string: Option<String>,
    pub rationale: String,
    pub judge_score: Option<f32>,
    pub judge_reason: Option<String>,
    pub created_at: String,
}

impl From<SkillCandidate> for EvolutionProposalDto {
    fn from(c: SkillCandidate) -> Self {
        let kind = match c.kind {
            CandidateKind::NewSkill => "new_skill",
            CandidateKind::Patch => "patch",
            CandidateKind::Disable => "disable",
            CandidateKind::Merge => "merge",
        }
        .to_string();
        Self {
            id: c.id,
            kind,
            skill_id: c.skill_id,
            description: c.description,
            content: c.content,
            old_string: c.old_string,
            new_string: c.new_string,
            rationale: c.rationale,
            judge_score: c.judge_score,
            judge_reason: c.judge_reason,
            created_at: c.created_at,
        }
    }
}

/// 运行结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionRunReport {
    pub ok: bool,
    pub generated: usize,
    pub gated_out: usize,
    pub judged_out: usize,
    pub proposals: Vec<EvolutionProposalDto>,
    pub error: Option<String>,
}

/// GEPA-lite 遗传搜索运行结果。
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionSearchReport {
    pub ok: bool,
    pub generations: u32,
    pub variants_evaluated: usize,
    pub pareto_kept: usize,
    pub proposals: Vec<EvolutionProposalDto>,
    pub budget_used: u32,
    pub holdout_enabled: bool,
    pub sandbox_used: bool,
    pub sandbox_skills: usize,
    pub focus_skill: Option<String>,
    pub termination: String,
    pub error: Option<String>,
}

fn active_ui_provider() -> Result<UiProvider, String> {
    let state = providers_commands::get_providers_state()?;
    let id = state
        .active_provider_id
        .or_else(|| state.providers.first().map(|p| p.id.clone()))
        .ok_or_else(|| "请先在「模型提供商」中配置并启用至少一个提供商".to_string())?;
    providers_commands::find_provider(&id)
}

fn active_primary_target() -> Result<types::ChatTarget, String> {
    let ui = active_ui_provider()?;
    if ui.model.trim().is_empty() {
        return Err("激活提供商未配置模型".into());
    }
    let (_has, _src, _env, key) = resolve_api_key(&ui);
    Ok(types::ChatTarget {
        provider_id: ui.id,
        backend_id: ui.kind.backend_id().to_string(),
        model: ui.model,
        api_key: key.unwrap_or_default(),
        base_url: ui.endpoint,
    })
}

async fn complete_chat(
    target: &ResolvedTarget,
    system: &str,
    user: &str,
) -> Result<String, String> {
    let base_url = target.provider.endpoint.trim();
    let config = ProviderConfig {
        api_key: target.api_key.clone(),
        base_url: if base_url.is_empty() {
            None
        } else {
            Some(base_url.trim_end_matches('/').to_string())
        },
        model: target.model.clone(),
        temperature: 0.3,
        max_tokens: 4096,
        thinking_enabled: false,
        reasoning_effort: "high".to_string(),
        additional_params: serde_json::Value::Null,
        previous_interaction_id: None,
    };
    let messages = vec![
        ProviderMessage::system(system),
        ProviderMessage::user_text(user),
    ];
    let mut stream =
        providers::dispatch::chat_stream(&target.backend_id, messages, vec![], &config)
            .await
            .map_err(|e| format!("进化调用模型失败: {e}"))?;
    let mut out = String::new();
    while let Some(item) = stream.next().await {
        let chunk = item.map_err(|e| format!("进化流式读取失败: {e}"))?;
        if let StreamChunk::Text(token) = chunk {
            out.push_str(&token);
        }
    }
    if out.trim().is_empty() {
        return Err("模型未返回任何内容".into());
    }
    Ok(out)
}

fn target_chain(targets: &AuxiliaryTargets) -> Vec<&ResolvedTarget> {
    std::iter::once(&targets.preferred)
        .chain(targets.fallback.as_ref())
        .collect()
}

async fn reflect_over_targets(
    targets: &AuxiliaryTargets,
    system: &str,
    user: &str,
) -> Result<String, String> {
    let mut last_err = "reflection 未产出".to_string();
    for target in target_chain(targets) {
        match complete_chat(target, system, user).await {
            Ok(raw) => return Ok(raw),
            Err(e) => {
                tracing::warn!(backend = %target.backend_id, model = %target.model, error = %e, "reflection 目标失败，尝试下一个");
                last_err = e;
            }
        }
    }
    Err(last_err)
}

/// 为近期决策关联的会话构建精简 transcript（最多 3 个会话，各 ~1500 字符）。
fn build_transcripts(base: &Path, decisions: &[memory::DecisionEntry]) -> Vec<(String, String)> {
    let mut session_ids: Vec<String> = Vec::new();
    for d in decisions {
        if let Some(sid) = d.session_id.as_deref().filter(|s| !s.is_empty()) {
            if !session_ids.iter().any(|s| s == sid) {
                session_ids.push(sid.to_string());
            }
        }
        if session_ids.len() >= 3 {
            break;
        }
    }
    if session_ids.is_empty() {
        return Vec::new();
    }
    let store = match session::SessionStore::open_sessions_dir(&base.join("sessions")) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for sid in session_ids {
        let Ok(msgs) = store.get_messages(&sid) else {
            continue;
        };
        let start = msgs.len().saturating_sub(10);
        let mut text = String::new();
        for m in &msgs[start..] {
            let content = m.content.as_deref().unwrap_or("").trim();
            if content.is_empty() {
                continue;
            }
            let clipped: String = content.chars().take(300).collect();
            text.push_str(&format!("{}: {}\n", m.role, clipped));
            if text.len() > 1500 {
                text.push_str("…(截断)\n");
                break;
            }
        }
        if !text.trim().is_empty() {
            out.push((sid, text));
        }
    }
    out
}

fn load_skill_text(skill_id: &str) -> Option<String> {
    skills::load_skill_by_name(skill_id).ok().map(|s| s.content)
}

fn skill_text_for_candidate(c: &SkillCandidate) -> Option<String> {
    if c.kind == CandidateKind::Patch {
        load_skill_text(&c.skill_id)
    } else {
        None
    }
}

fn truncate_task(text: &str, max_chars: usize) -> String {
    let t = text.trim();
    if t.chars().count() <= max_chars {
        t.to_string()
    } else {
        format!("{}…", t.chars().take(max_chars).collect::<String>())
    }
}

fn session_user_task(store: &session::SessionStore, session_id: &str) -> Option<String> {
    let msgs = store.get_messages(session_id).ok()?;
    for m in msgs {
        if m.role != "user" {
            continue;
        }
        let content = m.content.as_deref()?.trim();
        if !content.is_empty() {
            return Some(truncate_task(content, 500));
        }
    }
    store
        .get_session(session_id)
        .ok()
        .flatten()
        .and_then(|s| s.title)
        .map(|t| truncate_task(&t, 500))
        .filter(|t| !t.is_empty())
}

/// 运行一次离线进化（生成待审提案）。`mode` 写入历史（`reflect` / `auto`）。
///
/// `focus_skill`：[P2] signal 驱动定向进化时传入目标 skill_id，否则 None。
async fn run_evolution_core(
    app: &AppHandle,
    mode: &str,
    focus_skill: Option<String>,
) -> Result<EvolutionRunReport, String> {
    let base = default_memory_dir();
    let cfg = memory::load_evolution_config(&base);
    if !cfg.enabled {
        return Err("请先在「离线进化」中开启进化".into());
    }

    let primary = active_primary_target()?;
    let targets = resolve_evolution_targets(memory::EvolutionRouteKind::Reflection, &primary)?;
    if targets.preferred.provider.kind.requires_api_key()
        && targets.preferred.api_key.trim().is_empty()
        && targets
            .fallback
            .as_ref()
            .is_none_or(|fb| fb.provider.kind.requires_api_key() && fb.api_key.trim().is_empty())
    {
        return Err("未配置 API Key，无法运行进化".into());
    }

    let decisions = memory::list_recent_decisions(&base, 20).unwrap_or_default();
    let enabled_skills = skills::list_enabled_for_prompt();
    let transcripts = build_transcripts(&base, &decisions);
    let input = ReflectionInput {
        decisions,
        enabled_skills,
        transcripts,
        focus_skill,
    };
    let user = build_reflection_user_prompt(&input);

    let raw = reflect_over_targets(&targets, REFLECTION_SYSTEM_PROMPT, &user).await?;
    let candidates = parse_candidates(&raw).map_err(|e| e.to_string())?;
    let generated = candidates.len();

    let mut passed: Vec<SkillCandidate> = Vec::new();
    let mut gated_out = 0usize;
    for c in candidates {
        let outcome = check_candidate(&c, &cfg.gates, skill_text_for_candidate(&c).as_deref());
        if outcome.passed {
            passed.push(c);
        } else {
            gated_out += 1;
            tracing::info!(skill = %c.skill_id, reasons = ?outcome.reasons, "候选被门禁拦截");
        }
    }

    // judge 评审（min_judge_score <= 0 时关闭）；judge / 评分失败对该候选 fail-closed。
    let mut judged_out = 0usize;
    if cfg.gates.min_judge_score > 0.0 && !passed.is_empty() {
        if let Ok(judge_targets) =
            resolve_evolution_targets(memory::EvolutionRouteKind::Judge, &primary)
        {
            let enabled_now = skills::list_enabled_for_prompt();
            let evalset = list_examples(&base);
            let mut reflect_budget = SearchBudget::new(0);
            let mut split_stats = SearchSplitStats {
                holdout_enabled: false,
                optimize_examples: 0,
                holdout_examples: 0,
            };
            let mut kept: Vec<SkillCandidate> = Vec::new();
            for mut c in passed.into_iter() {
                if let Some(fr) = fitness_score(FitnessScoreArgs {
                    targets: &judge_targets,
                    cand: &c,
                    enabled_skills: &enabled_now,
                    evalset: &evalset,
                    max_eval_examples: cfg.search.max_eval_examples,
                    budget: &mut reflect_budget,
                    split_stats: Some(&mut split_stats),
                    eval_sampling: "fixed",
                    generation: 0,
                })
                .await
                {
                    c.judge_score = Some(fr.score);
                    c.judge_reason = Some(fr.reason);
                    if fr.score >= cfg.gates.min_judge_score {
                        kept.push(c);
                    } else {
                        judged_out += 1;
                        tracing::info!(skill = %c.skill_id, score = fr.score, "候选被适应度评分拒绝");
                    }
                } else {
                    judged_out += 1;
                    tracing::info!(skill = %c.skill_id, "候选评分失败或未评分，丢弃");
                }
            }
            passed = kept;
        }
    }

    save_proposals(&base, &passed).map_err(|e| e.to_string())?;

    let proposals: Vec<EvolutionProposalDto> =
        passed.into_iter().map(EvolutionProposalDto::from).collect();

    evolution::record_run(
        &base,
        mode,
        generated,
        gated_out,
        judged_out,
        proposals.len(),
    );
    let _ = app.emit(
        "evolution-updated",
        serde_json::json!({
            "generated": generated,
            "proposals": proposals.len(),
            "mode": mode,
        }),
    );

    Ok(EvolutionRunReport {
        ok: true,
        generated,
        gated_out,
        judged_out,
        proposals,
        error: None,
    })
}

/// 运行一次离线进化（生成待审提案）。
#[tauri::command]
pub async fn run_evolution(app: AppHandle) -> Result<EvolutionRunReport, String> {
    run_evolution_core(&app, "reflect", None).await
}

fn auto_inflight() -> &'static AtomicBool {
    static FLAG: OnceLock<AtomicBool> = OnceLock::new();
    FLAG.get_or_init(|| AtomicBool::new(false))
}

fn curator_inflight() -> &'static AtomicBool {
    static FLAG: OnceLock<AtomicBool> = OnceLock::new();
    FLAG.get_or_init(|| AtomicBool::new(false))
}

fn search_cancel_flag() -> &'static AtomicBool {
    static FLAG: OnceLock<AtomicBool> = OnceLock::new();
    FLAG.get_or_init(|| AtomicBool::new(false))
}

/// 请求取消进行中的遗传搜索（best-effort，下轮循环生效）。
#[tauri::command]
pub fn cancel_evolution_search() {
    search_cancel_flag().store(true, Ordering::SeqCst);
}

fn search_cancelled() -> bool {
    search_cancel_flag().load(Ordering::SeqCst)
}

struct SearchSplitStats {
    holdout_enabled: bool,
    optimize_examples: usize,
    holdout_examples: usize,
}

/// 自动触发结果（含跳过原因；不抛错以免打断 Chat Done）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionAutoRunDto {
    pub ran: bool,
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub skip_message: Option<String>,
    pub report: Option<EvolutionRunReport>,
    pub error: Option<String>,
}

/// 读取自动触发状态快照（配置 + 护栏水位）。
#[tauri::command]
pub async fn evolution_auto_status() -> Result<AutoStatus, String> {
    let base = default_memory_dir();
    let cfg = memory::load_evolution_config(&base);
    let state = load_auto_state(&base);
    let decisions = memory::list_recent_decisions(&base, 50).unwrap_or_default();
    Ok(build_auto_status(
        cfg.enabled,
        &cfg.auto,
        &state,
        &decisions,
    ))
}

/// Chat Done / 手动探测：护栏通过则跑一次单轮 reflect，产物只入待审。
///
/// 进程内互斥：已有自动运行在途时直接跳过。失败也会记水位/冷却，避免热重试烧钱。
#[tauri::command]
pub async fn maybe_run_evolution_auto(app: AppHandle) -> Result<EvolutionAutoRunDto, String> {
    let base = default_memory_dir();
    let cfg = memory::load_evolution_config(&base);
    let mut state = load_auto_state(&base);
    let decisions = memory::list_recent_decisions(&base, 50).unwrap_or_default();
    let gate = evolution::evaluate_auto_gate(
        cfg.enabled,
        &cfg.auto,
        &state,
        &decisions,
        chrono::Utc::now(),
    );
    match gate {
        AutoGate::Skip(reason) => {
            return Ok(EvolutionAutoRunDto {
                ran: false,
                skipped: true,
                skip_reason: Some(reason.as_str().to_string()),
                skip_message: Some(reason.message()),
                report: None,
                error: None,
            });
        }
        AutoGate::Allow => {}
    }

    if auto_inflight()
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(EvolutionAutoRunDto {
            ran: false,
            skipped: true,
            skip_reason: Some("inflight".into()),
            skip_message: Some("已有自动进化在运行".into()),
            report: None,
            error: None,
        });
    }

    let latest_id = decisions.last().map(|d| d.id.clone());

    // [P2] 信号驱动定向：若某 skill 失败信号超阈值，将其作为 reflect 焦点
    let known_skills: Vec<String> = skills::list_enabled_for_prompt()
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let signal_focus = if cfg.auto.min_skill_failure_signals > 0 {
        // 取最近 signal_window_days 天的条目（决策日志最近 N 条近似）
        top_failing_skill(
            &decisions,
            &known_skills,
            cfg.auto.min_skill_failure_signals,
        )
        .map(|s| {
            tracing::info!(
                skill = %s.skill_id,
                signals = s.failure_signals,
                "[P2] auto-trigger: 定向进化高失败率技能"
            );
            s.skill_id
        })
    } else {
        None
    };

    let result = run_evolution_core(&app, "auto", signal_focus).await;

    // 无论成败都记一次，挡住热重试；水位推进到当前最新决策。
    mark_auto_run(&mut state, chrono::Utc::now(), latest_id);
    let _ = save_auto_state(&base, &state);
    auto_inflight().store(false, Ordering::SeqCst);

    match result {
        Ok(report) => {
            tracing::info!(
                proposals = report.proposals.len(),
                generated = report.generated,
                "auto evolution produced proposals"
            );
            Ok(EvolutionAutoRunDto {
                ran: true,
                skipped: false,
                skip_reason: None,
                skip_message: None,
                report: Some(report),
                error: None,
            })
        }
        Err(e) => {
            tracing::warn!(error = %e, "auto evolution failed (cooldown recorded)");
            Ok(EvolutionAutoRunDto {
                ran: false,
                skipped: false,
                skip_reason: None,
                skip_message: None,
                report: None,
                error: Some(e),
            })
        }
    }
}

/// fire-and-forget：Chat Done 后尝试自动进化（默认关，护栏内自守）。
pub fn spawn_maybe_auto_evolution(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        match maybe_run_evolution_auto(app).await {
            Ok(dto) if dto.ran => {
                tracing::info!("auto evolution ran");
            }
            Ok(dto) if dto.skipped => {
                tracing::debug!(
                    reason = dto.skip_reason.as_deref().unwrap_or("-"),
                    "auto evolution skipped"
                );
            }
            Ok(dto) => {
                if let Some(err) = dto.error {
                    tracing::warn!(error = %err, "auto evolution error");
                }
            }
            Err(e) => tracing::warn!(error = %e, "auto evolution command failed"),
        }
    });
}

/// fire-and-forget：启动或 Chat Done 后尝试到期策展（仅报告，不入队、不调 LLM）。
pub fn spawn_maybe_curator(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        match maybe_run_skill_curator().await {
            Ok(dto) if dto.ran => {
                let n = dto
                    .report
                    .as_ref()
                    .map(|r| r.suggestions.len())
                    .unwrap_or(0);
                tracing::info!(suggestions = n, "auto curator report refreshed");
                let _ = app.emit(
                    "curator-updated",
                    serde_json::json!({
                        "suggestionCount": n,
                        "auto": true,
                    }),
                );
            }
            Ok(dto) if dto.skipped => {
                tracing::debug!(
                    reason = dto.skip_reason.as_deref().unwrap_or("-"),
                    "auto curator skipped"
                );
            }
            Ok(dto) => {
                if let Some(err) = dto.error {
                    tracing::warn!(error = %err, "auto curator error");
                }
            }
            Err(e) => tracing::warn!(error = %e, "auto curator command failed"),
        }
    });
}

/// 用 judge 给单个候选打分；失败返回 None（fail-closed）。
async fn judge_candidate(
    targets: &AuxiliaryTargets,
    cand: &SkillCandidate,
    enabled_skills: &[(String, String)],
) -> Option<(f32, String)> {
    let user = build_judge_user_prompt(cand, enabled_skills);
    let raw = reflect_over_targets(targets, JUDGE_SYSTEM_PROMPT, &user)
        .await
        .ok()?;
    let v = parse_judge_output(&raw).ok()?;
    Some((v.score, v.reason))
}

async fn score_grounded_examples(
    targets: &AuxiliaryTargets,
    cand: &SkillCandidate,
    examples: &[&EvalExample],
    max_eval_examples: usize,
    budget: &mut SearchBudget,
    eval_sampling: &str,
    generation: u32,
) -> (Vec<(Verdict, f32)>, Vec<EvalJudgement>) {
    let selected: Vec<&EvalExample> = if eval_sampling == "shuffle" && max_eval_examples > 0 {
        evolution::sample_eval_examples(examples, max_eval_examples, generation as u64)
    } else if max_eval_examples > 0 && examples.len() > max_eval_examples {
        let mut sorted: Vec<&EvalExample> = examples.to_vec();
        sorted.sort_by_key(|e| if e.verdict == Verdict::Fail { 0u8 } else { 1u8 });
        sorted.truncate(max_eval_examples);
        sorted
    } else {
        examples.to_vec()
    };

    let mut verdict_scores: Vec<(Verdict, f32)> = Vec::new();
    let mut judgements: Vec<EvalJudgement> = Vec::new();
    for ex in &selected {
        if !budget.try_reserve_one() {
            break;
        }
        let user = build_eval_judge_prompt(cand, ex);
        if let Ok(raw) = reflect_over_targets(targets, EVAL_JUDGE_SYSTEM_PROMPT, &user).await {
            if let Ok(j) = parse_eval_judgement(&raw) {
                verdict_scores.push((ex.verdict, j.score));
                judgements.push(j);
            }
        }
    }
    (verdict_scores, judgements)
}

/// [`fitness_score`] 入参打包。
struct FitnessScoreArgs<'a> {
    targets: &'a AuxiliaryTargets,
    cand: &'a SkillCandidate,
    enabled_skills: &'a [(String, String)],
    evalset: &'a [EvalExample],
    max_eval_examples: usize,
    budget: &'a mut SearchBudget,
    split_stats: Option<&'a mut SearchSplitStats>,
    eval_sampling: &'a str,
    generation: u32,
}

/// 客观适应度：匹配评测集时用 optimize 分区 grounded 评分；否则泛化 judge。
///
/// 返回结构化 `FitnessResult`（对标 GEPA 的 `(score, side_info)`）。
/// `None` 表示预算耗尽或 judge 失败（fail-closed）。
async fn fitness_score(a: FitnessScoreArgs<'_>) -> Option<FitnessResult> {
    let all_matched = examples_for_skill(a.evalset, &a.cand.skill_id);
    if all_matched.is_empty() {
        if !a.budget.try_reserve_one() {
            return None;
        }
        let (s, r) = judge_candidate(a.targets, a.cand, a.enabled_skills).await?;
        return Some(FitnessResult {
            score: s,
            reason: r,
            judgements: Vec::new(),
            side_info: FitnessSideInfo {
                eval_mode: "generic_judge",
                examples_scored: 1,
                fail_examples: 0,
                holdout_enabled: false,
            },
        });
    }

    let split = split_eval_examples(&all_matched, default_holdout_percent());
    if let Some(stats) = a.split_stats {
        stats.holdout_enabled |= split.holdout_enabled;
        stats.optimize_examples = stats.optimize_examples.max(split.optimize.len());
        stats.holdout_examples = stats.holdout_examples.max(split.holdout.len());
    }

    let (verdict_scores, judgements) = score_grounded_examples(
        a.targets,
        a.cand,
        &split.optimize,
        a.max_eval_examples,
        a.budget,
        a.eval_sampling,
        a.generation,
    )
    .await;

    match weighted_eval_score(&verdict_scores) {
        Some(avg) => {
            let n = verdict_scores.len();
            let fail_n = verdict_scores
                .iter()
                .filter(|(v, _)| *v == Verdict::Fail)
                .count();
            let reason = if split.holdout_enabled {
                if fail_n > 0 {
                    format!("optimize 评分（{n} 例，{fail_n} Fail 双权重；holdout 待最终验证）")
                } else {
                    format!("optimize 评分（{n} 例；holdout 待最终验证）")
                }
            } else if fail_n > 0 {
                format!("grounded 评分（{n} 例，其中 {fail_n} 例 Fail 双权重）")
            } else {
                format!("grounded 评分（{n} 例）")
            };
            Some(FitnessResult {
                score: avg,
                reason,
                judgements,
                side_info: FitnessSideInfo {
                    eval_mode: "grounded",
                    examples_scored: n,
                    fail_examples: fail_n,
                    holdout_enabled: split.holdout_enabled,
                },
            })
        }
        None => None,
    }
}

async fn holdout_fitness_score(
    targets: &AuxiliaryTargets,
    cand: &SkillCandidate,
    holdout: &[&EvalExample],
    max_eval_examples: usize,
    budget: &mut SearchBudget,
) -> Option<f32> {
    if holdout.is_empty() {
        return None;
    }
    let (verdict_scores, _) = score_grounded_examples(
        targets,
        cand,
        holdout,
        max_eval_examples,
        budget,
        "fixed",
        0,
    )
    .await;
    weighted_eval_score(&verdict_scores)
}

/// GEPA-lite 遗传搜索：种子 → 每目标多代变异 + judge 打分 + Pareto 选择 → 待审提案。
///
/// `skill_id` 非空时定向进化：reflection 聚焦该技能，并只保留该 skill 的种子。
#[tauri::command]
pub async fn run_evolution_search(
    app: AppHandle,
    skill_id: Option<String>,
) -> Result<EvolutionSearchReport, String> {
    search_cancel_flag().store(false, Ordering::SeqCst);
    let base = default_memory_dir();
    let cfg = memory::load_evolution_config(&base);
    if !cfg.enabled {
        return Err("请先在「离线进化」中开启进化".into());
    }

    let focus_skill = skill_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // [P0] Post-Approval Cooldown Gate：避免对刚批准的技能重复进化
    if let Some(ref focus) = focus_skill {
        let cooldown = cfg.search.post_approval_cooldown_secs;
        if cooldown > 0 {
            if let Some(last_ts) = skill_last_approved_at(&base, focus) {
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&last_ts) {
                    let elapsed =
                        chrono::Utc::now().signed_duration_since(dt.with_timezone(&chrono::Utc));
                    if elapsed.num_seconds() < cooldown as i64 {
                        let remain_h = (cooldown as i64 - elapsed.num_seconds()).max(0) / 3600 + 1;
                        tracing::info!(
                            skill = %focus,
                            remain_hours = remain_h,
                            "[P0] 冷却期内，跳过进化"
                        );
                        return Ok(EvolutionSearchReport {
                            termination: format!("cooldown:{remain_h}h"),
                            ..Default::default()
                        });
                    }
                }
            }
        }
    }

    let primary = active_primary_target()?;
    let refl_targets = resolve_evolution_targets(memory::EvolutionRouteKind::Reflection, &primary)?;
    if refl_targets.preferred.provider.kind.requires_api_key()
        && refl_targets.preferred.api_key.trim().is_empty()
        && refl_targets
            .fallback
            .as_ref()
            .is_none_or(|fb| fb.provider.kind.requires_api_key() && fb.api_key.trim().is_empty())
    {
        return Err("未配置 API Key，无法运行进化".into());
    }
    let judge_targets = resolve_evolution_targets(memory::EvolutionRouteKind::Judge, &primary)?;

    // 种子：reflection 产候选
    let decisions = memory::list_recent_decisions(&base, 50).unwrap_or_default();
    let mut enabled_skills = skills::list_enabled_for_prompt();
    if let Some(ref focus) = focus_skill {
        // 定向：把目标技能提到前面，便于模型聚焦
        enabled_skills.sort_by_key(|(name, _)| if name == focus { 0 } else { 1 });
        if !enabled_skills.iter().any(|(n, _)| n == focus) {
            return Err(format!("定向技能 `{focus}` 未启用或不存在"));
        }
    }
    // [P3] 预加载 curator 健康报告（可选，用于 opportunity hints）
    let curator_report = load_curator_last(&base);
    // [P3] 准备 known_skill_ids（用于信号提取）
    let known_skill_ids: Vec<String> = enabled_skills.iter().map(|(n, _)| n.clone()).collect();
    let transcripts = build_transcripts(&base, &decisions);
    let seed_user = build_reflection_user_prompt(&ReflectionInput {
        decisions: decisions.clone(), // [P3] 保留 decisions 所有权用于后续 detect_opportunities
        enabled_skills: enabled_skills.clone(),
        transcripts,
        focus_skill: focus_skill.clone(),
    });
    let seed_raw =
        reflect_over_targets(&refl_targets, REFLECTION_SYSTEM_PROMPT, &seed_user).await?;
    let seeds_all = parse_candidates(&seed_raw).map_err(|e| e.to_string())?;

    // 按 (skill_id, kind) 去重，最多 3 个目标；定向时只保留 focus
    let mut seen: HashSet<String> = HashSet::new();
    let mut seeds: Vec<SkillCandidate> = Vec::new();
    for c in seeds_all {
        if let Some(ref focus) = focus_skill {
            if &c.skill_id != focus {
                continue;
            }
        }
        let key = format!("{}::{:?}", c.skill_id, c.kind);
        if seen.insert(key) {
            seeds.push(c);
        }
        if seeds.len() >= 3 {
            break;
        }
    }

    // 定向且模型未产出：注入一条 patch 占位种子（空 patch 会被后续门禁/变异消化）
    // 更稳妥：若无种子，用当前技能内容作为 NewSkill 基线不可行；直接报错让用户重试。
    if seeds.is_empty() && focus_skill.is_some() {
        return Err("定向技能未产出候选；可先积累 DecisionLog 失败信号后再试".into());
    }

    let evalset = list_examples(&base);
    let generations = cfg.search.generations.max(1);
    let variants = cfg.search.variants.max(1);
    let max_eval_examples = cfg.search.max_eval_examples;
    let eval_sampling = cfg.search.eval_sampling.clone();
    let mut budget = SearchBudget::new(cfg.search.max_llm_calls);
    budget.try_reserve_one(); // seed reflection 调用已消费
    let mut variants_evaluated = 0usize;
    let mut pareto_kept = 0usize;
    let mut search_gated_out = 0usize;
    let mut search_judged_out = 0usize;
    let mut final_props: Vec<SkillCandidate> = Vec::new();
    let mut split_stats = SearchSplitStats {
        holdout_enabled: false,
        optimize_examples: 0,
        holdout_examples: 0,
    };
    let mut holdout_ids_by_skill: HashMap<String, Vec<String>> = HashMap::new();
    let mut termination = "completed".to_string();
    let mut sandbox_skill_ids: HashSet<String> = HashSet::new();

    let pop_size = cfg.search.population_size.max(1) as usize;
    let no_seeds = seeds.is_empty();
    let mutation_prompt = cfg
        .search
        .mutation_system_prompt
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(MUTATION_SYSTEM_PROMPT);
    let crossover_prompt = cfg
        .search
        .crossover_system_prompt
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(CROSSOVER_SYSTEM_PROMPT);

    let seed_count = seeds.len();
    'seed: for (seed_idx, seed) in seeds.into_iter().enumerate() {
        if search_cancelled() {
            termination = "cancelled".into();
            break;
        }
        let mut population: Vec<ScoredVariant> = vec![ScoredVariant::new(seed.clone(), 0.0)];
        let mut critiques: Vec<String> = Vec::new();

        let loaded_skill = skills::load_skill_by_name(&seed.skill_id).ok();
        let current_skill_text: Option<String> = loaded_skill.as_ref().map(|s| s.content.clone());
        let test_scripts_dir: Option<PathBuf> = loaded_skill.as_ref().and_then(|s| {
            let skill_dir = std::path::Path::new(&s.path).parent()?;
            let scripts = skill_dir.join("scripts");
            (scripts.join("test.sh").is_file() || scripts.join("test.py").is_file())
                .then_some(scripts)
        });
        let run_sandbox = cfg.gates.run_tests && test_scripts_dir.is_some();
        if run_sandbox {
            sandbox_skill_ids.insert(seed.skill_id.clone());
        }

        let mut strengths: Vec<String> = Vec::new();

        for gen in 0..generations {
            if search_cancelled() {
                termination = "cancelled".into();
                break 'seed;
            }
            if !budget.try_reserve_one() {
                tracing::info!(
                    skill = %seed.skill_id,
                    used = budget.used(),
                    "LLM 预算耗尽，停止搜索"
                );
                termination = "budget_exhausted".into();
                break 'seed;
            }

            let parent = &population[gen as usize % population.len()].candidate;
            // [P3] 收集运行时机会 hints，注入 mutation prompt
            let opp_hints = detect_opportunities(
                &seed.skill_id,
                &base,
                &decisions,
                curator_report.as_ref().map(|r| r.rows.as_slice()),
                &known_skill_ids,
            );
            let muser = build_mutation_prompt(parent, variants, &critiques, &strengths, &opp_hints);
            let raw = match reflect_over_targets(&refl_targets, mutation_prompt, &muser).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(skill = %seed.skill_id, error = %e, "变异调用失败，停止该目标");
                    break;
                }
            };
            let mut cands = parse_variants(&raw, parent).unwrap_or_default();
            // 保留当前种群作为基线
            cands.extend(population.iter().map(|v| v.candidate.clone()));
            if cands.is_empty() {
                break;
            }

            let mut scored: Vec<ScoredVariant> = Vec::new();
            let mut gen_judgements: Vec<EvalJudgement> = Vec::new();
            for mut c in cands {
                let eff_size = match effective_candidate_size(&c, current_skill_text.as_deref()) {
                    Ok(s) => s,
                    Err(e) => {
                        search_gated_out += 1;
                        tracing::info!(skill = %c.skill_id, error = %e, "候选 dry-run 失败，跳过");
                        continue;
                    }
                };

                if let Some(fr) = fitness_score(FitnessScoreArgs {
                    targets: &judge_targets,
                    cand: &c,
                    enabled_skills: &enabled_skills,
                    evalset: &evalset,
                    max_eval_examples,
                    budget: &mut budget,
                    split_stats: Some(&mut split_stats),
                    eval_sampling: &eval_sampling,
                    generation: gen,
                })
                .await
                {
                    if split_stats.holdout_enabled {
                        let split = split_eval_examples(
                            &examples_for_skill(&evalset, &c.skill_id),
                            default_holdout_percent(),
                        );
                        if split.holdout_enabled {
                            holdout_ids_by_skill
                                .entry(c.skill_id.clone())
                                .or_insert_with(|| {
                                    split.holdout.iter().map(|e| e.id.clone()).collect()
                                });
                        }
                    }
                    c.judge_score = Some(fr.score);
                    c.judge_reason = Some(fr.reason);
                    gen_judgements.extend(fr.judgements);
                    variants_evaluated += 1;
                    let test_pass = if run_sandbox {
                        sandbox_test_candidate(
                            &c,
                            current_skill_text.as_deref(),
                            test_scripts_dir.as_deref(),
                            Duration::from_secs(60),
                            &cfg.gates.sandbox_mode,
                            &cfg.gates.sandbox_docker_image,
                        )
                        .fitness()
                    } else {
                        None
                    };
                    scored.push(
                        ScoredVariant::with_size(c, fr.score, eff_size).with_test_pass(test_pass),
                    );
                } else {
                    tracing::debug!(skill = %c.skill_id, "预算不足或评分失败，跳过该候选");
                }
            }

            // 交叉：从种群中选两个内容不同的高分个体
            if cfg.search.crossover && scored.len() >= 2 {
                let top = select_front_capped(pareto_front(&scored), 2);
                if top.len() == 2 && budget.try_reserve_one() {
                    let cx = build_crossover_prompt(&top[0].candidate, &top[1].candidate);
                    if let Ok(raw) =
                        reflect_over_targets(&refl_targets, crossover_prompt, &cx).await
                    {
                        for mut child in parse_variants(&raw, &seed).unwrap_or_default() {
                            let eff_size = match effective_candidate_size(
                                &child,
                                current_skill_text.as_deref(),
                            ) {
                                Ok(s) => s,
                                Err(_) => {
                                    search_gated_out += 1;
                                    continue;
                                }
                            };
                            if let Some(fr) = fitness_score(FitnessScoreArgs {
                                targets: &judge_targets,
                                cand: &child,
                                enabled_skills: &enabled_skills,
                                evalset: &evalset,
                                max_eval_examples,
                                budget: &mut budget,
                                split_stats: Some(&mut split_stats),
                                eval_sampling: &eval_sampling,
                                generation: gen,
                            })
                            .await
                            {
                                child.judge_score = Some(fr.score);
                                child.judge_reason = Some(format!("[交叉] {}", fr.reason));
                                gen_judgements.extend(fr.judgements);
                                variants_evaluated += 1;
                                let test_pass = if run_sandbox {
                                    sandbox_test_candidate(
                                        &child,
                                        current_skill_text.as_deref(),
                                        test_scripts_dir.as_deref(),
                                        Duration::from_secs(60),
                                        &cfg.gates.sandbox_mode,
                                        &cfg.gates.sandbox_docker_image,
                                    )
                                    .fitness()
                                } else {
                                    None
                                };
                                scored.push(
                                    ScoredVariant::with_size(child, fr.score, eff_size)
                                        .with_test_pass(test_pass),
                                );
                            }
                        }
                    }
                }
            }

            // 选择种群：Pareto front 优先 + 被支配补齐 + 去重
            population = select_population(scored, pop_size);
            if population.is_empty() {
                tracing::warn!(skill = %seed.skill_id, "种群为空（所有候选被门禁或预算拦截），停止该目标");
                break;
            }
            // 结构化 critique：unmet 优先，去重，按频次排序
            critiques = aggregate_critiques(&gen_judgements, 8);
            // 保留已满足要点作为 strengths
            strengths = gen_judgements
                .iter()
                .flat_map(|j| j.satisfied.iter().cloned())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .take(8)
                .collect();

            let _ = app.emit(
                "evolution-search-progress",
                serde_json::json!({
                    "seedSkill": seed.skill_id,
                    "seedIndex": seed_idx,
                    "seedTotal": seed_count,
                    "generation": gen,
                    "generationTotal": generations,
                    "populationScores": population.iter().map(|v| v.score).collect::<Vec<_>>(),
                    "populationBest": population.iter().map(|v| v.score).fold(0.0f32, f32::max),
                    "populationSize": population.len(),
                    "variantsEvaluated": variants_evaluated,
                    "budgetUsed": budget.used(),
                    "budgetLimit": cfg.search.max_llm_calls,
                    "gatedOut": search_gated_out,
                    "judgedOut": search_judged_out,
                    "critiques": critiques.iter().take(3).cloned().collect::<Vec<_>>(),
                    // [P3] 运行时机会 hints（当代，最多 4 条）
                    "hints": opp_hints.iter().map(|h| serde_json::json!({"tag": h.tag, "focus": h.focus})).collect::<Vec<_>>(),
                }),
            );
        }

        // 从最终种群中取前 2 个，holdout 复验后过门禁
        for mut v in select_front_capped(population, 2) {
            pareto_kept += 1;
            if let Some(ids) = holdout_ids_by_skill.get(&v.candidate.skill_id) {
                let holdout_refs: Vec<&EvalExample> =
                    evalset.iter().filter(|e| ids.contains(&e.id)).collect();
                if let Some(hs) = holdout_fitness_score(
                    &judge_targets,
                    &v.candidate,
                    &holdout_refs,
                    max_eval_examples,
                    &mut budget,
                )
                .await
                {
                    v.candidate.judge_score = Some(hs);
                    v.candidate.judge_reason =
                        Some(format!("holdout 验证 {hs:.2}（optimize {:.2}）", v.score));
                }
            }
            if !check_candidate(&v.candidate, &cfg.gates, current_skill_text.as_deref()).passed {
                search_gated_out += 1;
                continue;
            }
            if cfg.gates.min_judge_score > 0.0 {
                let score = v.candidate.judge_score.unwrap_or(0.0);
                if score < cfg.gates.min_judge_score {
                    search_judged_out += 1;
                    tracing::info!(
                        skill = %v.candidate.skill_id,
                        score,
                        threshold = cfg.gates.min_judge_score,
                        "搜索候选被 min_judge_score 拒绝"
                    );
                    continue;
                }
            }
            final_props.push(v.candidate);
        }
    }

    if no_seeds && termination == "completed" {
        termination = "no_candidates".into();
    }

    save_proposals(&base, &final_props).map_err(|e| e.to_string())?;
    let proposals: Vec<EvolutionProposalDto> = final_props
        .into_iter()
        .map(EvolutionProposalDto::from)
        .collect();

    tracing::info!(
        variants_evaluated,
        pareto_kept,
        gated_out = search_gated_out,
        judged_out = search_judged_out,
        llm_calls = budget.used(),
        proposals = proposals.len(),
        "GEPA-lite 搜索完成"
    );
    evolution::record_run_meta(
        &base,
        "search",
        variants_evaluated,
        search_gated_out,
        search_judged_out,
        proposals.len(),
        Some(SearchRunMeta {
            generations,
            variants,
            population_size: cfg.search.population_size,
            crossover: cfg.search.crossover,
            budget_limit: cfg.search.max_llm_calls,
            budget_used: budget.used(),
            optimize_examples: split_stats.optimize_examples,
            holdout_examples: split_stats.holdout_examples,
            holdout_enabled: split_stats.holdout_enabled,
            sandbox_used: !sandbox_skill_ids.is_empty(),
            sandbox_skills: sandbox_skill_ids.len(),
            focus_skill: focus_skill.clone().unwrap_or_default(),
            reflection_model: format!(
                "{} / {}",
                refl_targets.preferred.provider.id, refl_targets.preferred.model
            ),
            judge_model: format!(
                "{} / {}",
                judge_targets.preferred.provider.id, judge_targets.preferred.model
            ),
            termination: termination.clone(),
        }),
    );
    let _ = app.emit(
        "evolution-updated",
        serde_json::json!({
            "search": true,
            "proposals": proposals.len(),
        }),
    );

    Ok(EvolutionSearchReport {
        ok: true,
        generations,
        variants_evaluated,
        pareto_kept,
        proposals,
        budget_used: budget.used(),
        holdout_enabled: split_stats.holdout_enabled,
        sandbox_used: !sandbox_skill_ids.is_empty(),
        sandbox_skills: sandbox_skill_ids.len(),
        focus_skill: focus_skill.clone(),
        termination,
        error: None,
    })
}

/// 列出全部待审进化提案。
#[tauri::command]
pub async fn list_evolution_proposals() -> Result<Vec<EvolutionProposalDto>, String> {
    let base = default_memory_dir();
    Ok(list_proposals(&base)
        .into_iter()
        .map(EvolutionProposalDto::from)
        .collect())
}

/// 在技能目录查找并运行 `scripts/test.{sh,py}`；无脚本则视为通过。
///
/// 仅在用户点「批准」后执行（内容已经人工审阅），60s 超时。
fn run_skill_tests(skill_dir: &Path) -> Result<(), String> {
    let scripts = skill_dir.join("scripts");
    let (program, script) = if scripts.join("test.sh").is_file() {
        ("sh", scripts.join("test.sh"))
    } else if scripts.join("test.py").is_file() {
        ("python3", scripts.join("test.py"))
    } else {
        return Ok(()); // 无测试脚本：跳过
    };

    let mut cmd = std::process::Command::new(program);
    cmd.arg(&script)
        .current_dir(skill_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("启动测试失败: {e}"))?;

    // 简单超时：轮询 60s
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return Ok(());
                }
                let out = child.wait_with_output().ok();
                let tail = out
                    .map(|o| {
                        String::from_utf8_lossy(&o.stderr)
                            .chars()
                            .take(500)
                            .collect::<String>()
                    })
                    .unwrap_or_default();
                return Err(format!("测试退出码非零: {tail}"));
            }
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(60) {
                    let _ = child.kill();
                    return Err("测试超时（>60s）".into());
                }
                std::thread::sleep(Duration::from_millis(150));
            }
            Err(e) => return Err(format!("测试执行出错: {e}")),
        }
    }
}

fn proposal_meta(base: &Path, id: &str) -> (String, String, Option<f32>) {
    list_proposals(base)
        .into_iter()
        .find(|p| p.id == id)
        .map(|p| {
            let kind = match p.kind {
                CandidateKind::NewSkill => "new_skill",
                CandidateKind::Patch => "patch",
                CandidateKind::Disable => "disable",
                CandidateKind::Merge => "merge",
            }
            .to_string();
            (p.skill_id, kind, p.judge_score)
        })
        .unwrap_or_default()
}

/// 批准一条提案：写入 Agent skills 目录；`run_tests` 开启时批准后跑测试，失败回滚。
#[tauri::command]
pub async fn approve_evolution_proposal(id: String) -> Result<String, String> {
    let base = default_memory_dir();
    let (skill_id, kind, score) = proposal_meta(&base, &id);

    // [P1] 批准前保存当前版本快照，便于一键回滚
    if let Ok(loaded) = skills::load_skill_by_name(&skill_id) {
        if let Some(parent) = std::path::Path::new(&loaded.path).parent() {
            let _ = skills::save_snapshot(parent); // 失败不阻塞审批
        }
    }

    let skip_tests = matches!(kind.as_str(), "disable" | "merge");
    let run_tests = !skip_tests && memory::load_evolution_config(&base).gates.run_tests;
    let res = if run_tests {
        approve_proposal_checked(&base, &id, run_skill_tests).map_err(|e| e.to_string())
    } else {
        approve_proposal(&base, &id).map_err(|e| e.to_string())
    };
    if res.is_ok() {
        evolution::record_outcome(&base, &id, &skill_id, &kind, score, "approved");
    }
    res
}

/// 拒绝并删除一条提案。
#[tauri::command]
pub async fn reject_evolution_proposal(id: String) -> Result<(), String> {
    let base = default_memory_dir();
    let (skill_id, kind, score) = proposal_meta(&base, &id);
    reject_proposal(&base, &id).map_err(|e| e.to_string())?;
    evolution::record_outcome(&base, &id, &skill_id, &kind, score, "rejected");
    Ok(())
}

/// 聚合统计展示态（camelCase）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistorySummaryDto {
    pub total_runs: usize,
    pub runs_by_mode: std::collections::BTreeMap<String, usize>,
    pub total_generated: usize,
    pub total_proposals: usize,
    pub approved: usize,
    pub rejected: usize,
    pub branched: usize,
    pub adoption_rate: f32,
    pub avg_adopted_score: f32,
    pub score_trend: Vec<f32>,
}

/// 进化历史（聚合 + 近期事件）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionHistoryDto {
    pub summary: HistorySummaryDto,
    pub recent: Vec<serde_json::Value>,
}

/// 读取进化可观测数据。
#[tauri::command]
pub async fn evolution_history() -> Result<EvolutionHistoryDto, String> {
    let base = default_memory_dir();
    let s = evolution::summarize_history(&base);
    let summary = HistorySummaryDto {
        total_runs: s.total_runs,
        runs_by_mode: s.runs_by_mode,
        total_generated: s.total_generated,
        total_proposals: s.total_proposals,
        approved: s.approved,
        rejected: s.rejected,
        branched: s.branched,
        adoption_rate: s.adoption_rate,
        avg_adopted_score: s.avg_adopted_score,
        score_trend: s.score_trend,
    };
    let mut all = evolution::list_history(&base);
    if all.len() > 20 {
        all = all.split_off(all.len() - 20);
    }
    all.reverse(); // 最新在前
    let recent = all
        .into_iter()
        .filter_map(|e| serde_json::to_value(e).ok())
        .collect();
    Ok(EvolutionHistoryDto { summary, recent })
}

// ===== Phase 3: DSPy 外部引擎桥接 =====

/// 解析 evolution-dspy 项目路径：config 显式 → resource_dir → 仓库/当前目录。
fn resolve_dspy_project<R: tauri::Runtime>(app: &AppHandle<R>, cfg_path: &str) -> Option<PathBuf> {
    let c = cfg_path.trim();
    if !c.is_empty() {
        let p = PathBuf::from(c);
        if p.is_dir() {
            return Some(p);
        }
    }
    if let Ok(res) = app.path().resource_dir() {
        let p = res.join("evolution-dspy");
        if p.is_dir() {
            return Some(p);
        }
    }
    let cwd = std::env::current_dir().ok()?.join("evolution-dspy");
    if cwd.is_dir() {
        Some(cwd)
    } else {
        None
    }
}

/// 解析 Python 可执行：config 显式 → ~/.astro venv → 系统 python3。
fn resolve_dspy_python(cfg_bin: &str) -> String {
    let c = cfg_bin.trim();
    if !c.is_empty() {
        return c.to_string();
    }
    let venv = default_memory_dir()
        .join("evolution-dspy")
        .join(".venv")
        .join("bin")
        .join("python");
    if venv.is_file() {
        return venv.to_string_lossy().to_string();
    }
    "python3".to_string()
}

/// DSPy 对接状态。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DspyStatusDto {
    pub enabled: bool,
    pub python_bin: String,
    pub python_ok: bool,
    pub project_path: Option<String>,
    pub dspy_installed: bool,
    pub timeout_secs: u64,
}

/// 读取 DSPy 对接状态（检测 python 与 dspy 是否可用）。
#[tauri::command]
pub async fn evolution_dspy_status<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<DspyStatusDto, String> {
    let base = default_memory_dir();
    let cfg = memory::load_evolution_config(&base).dspy;
    let python_bin = resolve_dspy_python(&cfg.python_bin);
    let project = resolve_dspy_project(&app, &cfg.project_path);

    let python_ok = std::process::Command::new(&python_bin)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let dspy_installed = python_ok
        && std::process::Command::new(&python_bin)
            .args(["-c", "import dspy"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

    Ok(DspyStatusDto {
        enabled: cfg.enabled,
        python_bin,
        python_ok,
        project_path: project.map(|p| home::display_user_path(&p)),
        dspy_installed,
        timeout_secs: cfg.timeout_secs,
    })
}

/// 在 `~/.astro/evolution-dspy/.venv` 建 venv 并 `pip install -e <project>`。
#[tauri::command]
pub async fn setup_evolution_dspy<R: tauri::Runtime>(app: AppHandle<R>) -> Result<String, String> {
    let base = default_memory_dir();
    let project = resolve_dspy_project(
        &app,
        &memory::load_evolution_config(&base).dspy.project_path,
    )
    .ok_or_else(|| "找不到 evolution-dspy 项目目录".to_string())?;
    let venv = base.join("evolution-dspy").join(".venv");
    if let Some(parent) = venv.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // 建 venv
    let out = std::process::Command::new("python3")
        .args(["-m", "venv", &venv.to_string_lossy()])
        .output()
        .map_err(|e| format!("创建 venv 失败: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "创建 venv 失败: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    // pip install -e project
    let pip = venv.join("bin").join("pip");
    let out = std::process::Command::new(&pip)
        .args(["install", "-e", &project.to_string_lossy()])
        .output()
        .map_err(|e| format!("pip install 失败: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "pip install 失败: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(format!(
        "已安装到 {}（-e {}）",
        home::display_user_path(&venv),
        home::display_user_path(&project)
    ))
}

/// 用外部 DSPy 引擎优化某技能，产物入待审提案队列。
#[tauri::command]
pub async fn run_evolution_dspy<R: tauri::Runtime>(
    app: AppHandle<R>,
    skill_id: String,
    mock: Option<bool>,
) -> Result<EvolutionRunReport, String> {
    let mock = mock.unwrap_or(false);
    let base = default_memory_dir();
    let cfg = memory::load_evolution_config(&base);
    if !cfg.enabled {
        return Err("请先开启离线进化".into());
    }
    if !mock && !cfg.dspy.enabled {
        return Err("请先在配置中开启 evolution.dspy.enabled".into());
    }
    let python_bin = resolve_dspy_python(&cfg.dspy.python_bin);
    let project = resolve_dspy_project(&app, &cfg.dspy.project_path)
        .ok_or_else(|| "找不到 evolution-dspy 项目目录".to_string())?;

    // 目标技能内容
    let loaded = skills::load_skill_by_name(&skill_id).map_err(|e| e.to_string())?;

    // 解析 reflection 目标作为 LLM 端点；mock 自测跳过（不需要真实模型/凭据）
    let (model, base_url, backend_id, api_key) = if mock {
        (
            String::new(),
            String::new(),
            "mock".to_string(),
            String::new(),
        )
    } else {
        let primary = active_primary_target()?;
        let targets = resolve_evolution_targets(memory::EvolutionRouteKind::Reflection, &primary)?;
        let t = targets.preferred;
        (t.model, t.provider.endpoint, t.backend_id, t.api_key)
    };

    // 导出输入到临时目录
    let run_id: String = uuid::Uuid::new_v4().to_string().chars().take(8).collect();
    let dir = base
        .join("learning")
        .join("evolution")
        .join(format!("dspy-run-{run_id}"));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(dir.join("skill.md"), loaded.content.as_bytes()).map_err(|e| e.to_string())?;

    let evalset = list_examples(&base);
    let matched: Vec<&EvalExample> = examples_for_skill(&evalset, &skill_id);
    let mut jsonl = String::new();
    for ex in &matched {
        if let Ok(line) = serde_json::to_string(ex) {
            jsonl.push_str(&line);
            jsonl.push('\n');
        }
    }
    std::fs::write(dir.join("evalset.jsonl"), jsonl.as_bytes()).map_err(|e| e.to_string())?;

    let config_json = serde_json::json!({
        "skill_id": skill_id,
        "model": model,
        "base_url": base_url,
        "provider_backend": backend_id,
    });
    std::fs::write(
        dir.join("config.json"),
        serde_json::to_string_pretty(&config_json)
            .unwrap_or_default()
            .as_bytes(),
    )
    .map_err(|e| e.to_string())?;

    let output = dir.join("result.json");
    let mut cmd_args: Vec<String> = vec![
        "-m".into(),
        "evolution_dspy".into(),
        "optimize".into(),
        "--input".into(),
        dir.to_string_lossy().to_string(),
        "--output".into(),
        output.to_string_lossy().to_string(),
    ];
    if mock {
        cmd_args.push("--mock".into());
    }
    let mut child = std::process::Command::new(&python_bin)
        .args(&cmd_args)
        .current_dir(&project)
        .env("ASTRO_DSPY_API_KEY", &api_key)
        .env("PYTHONPATH", &project)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 DSPy 失败: {e}（python_bin={python_bin}）"))?;

    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(cfg.dspy.timeout_secs.max(30));
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return Err("DSPy 运行超时".into());
                }
                std::thread::sleep(Duration::from_millis(300));
            }
            Err(e) => return Err(format!("DSPy 执行出错: {e}")),
        }
    }

    let raw =
        std::fs::read_to_string(&output).map_err(|_| "DSPy 未产出 result.json".to_string())?;
    let val: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if let Some(err) = val.get("error").and_then(|v| v.as_str()) {
        return Err(format!("DSPy 失败: {err}"));
    }
    let content = val
        .get("content")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "DSPy 结果缺少 content".to_string())?;
    let score = val.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
    let rationale = val
        .get("rationale")
        .and_then(|v| v.as_str())
        .unwrap_or("DSPy 优化产物")
        .to_string();

    let cand = SkillCandidate {
        id: uuid::Uuid::new_v4().to_string(),
        kind: CandidateKind::NewSkill,
        skill_id: skill_id.clone(),
        description: None,
        content: Some(content),
        old_string: None,
        new_string: None,
        rationale,
        sources: vec![format!("dspy-run-{run_id}")],
        judge_score: Some(score),
        judge_reason: Some("DSPy+GEPA".into()),
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    let mut proposals = Vec::new();
    let gated = if check_candidate(
        &cand,
        &cfg.gates,
        skill_text_for_candidate(&cand).as_deref(),
    )
    .passed
    {
        save_proposals(&base, std::slice::from_ref(&cand)).map_err(|e| e.to_string())?;
        proposals.push(EvolutionProposalDto::from(cand));
        0
    } else {
        1
    };
    evolution::record_run(&base, "dspy", 1, gated, 0, proposals.len());
    let _ = std::fs::remove_dir_all(&dir);

    let _ = app.emit(
        "evolution-updated",
        serde_json::json!({ "dspy": true, "proposals": proposals.len() }),
    );

    Ok(EvolutionRunReport {
        ok: true,
        generated: 1,
        gated_out: if proposals.is_empty() { 1 } else { 0 },
        judged_out: 0,
        proposals,
        error: None,
    })
}

/// 可从失败会话导入的评测例候选。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalImportCandidateDto {
    pub session_id: String,
    pub task: String,
    pub expectations: Vec<String>,
    pub fail_count: usize,
}

fn collect_eval_import_candidates(
    base: &Path,
    limit: usize,
) -> Result<Vec<EvalImportCandidateDto>, String> {
    let decisions = memory::list_recent_decisions(base, 200)
        .map_err(|e| format!("读取 DecisionLog 失败: {e}"))?;
    let existing = list_examples(base);
    let imported: HashSet<String> = existing
        .iter()
        .filter_map(|e| e.source_session.clone())
        .collect();

    let store = session::SessionStore::open_sessions_dir(&base.join("sessions"))
        .map_err(|e| format!("打开会话库失败: {e}"))?;

    let mut by_session: HashMap<String, Vec<String>> = HashMap::new();
    for d in decisions {
        if !matches!(
            d.kind,
            DecisionKind::ToolFailure | DecisionKind::UserCorrection
        ) {
            continue;
        }
        let Some(sid) = d.session_id.as_deref().filter(|s| !s.is_empty()) else {
            continue;
        };
        if imported.contains(sid) {
            continue;
        }
        let summary = d.summary.trim();
        if summary.is_empty() {
            continue;
        }
        by_session
            .entry(sid.to_string())
            .or_default()
            .push(summary.to_string());
    }

    let mut out: Vec<EvalImportCandidateDto> = Vec::new();
    for (session_id, mut expectations) in by_session {
        expectations.sort();
        expectations.dedup();
        let Some(task) = session_user_task(&store, &session_id) else {
            continue;
        };
        let fail_count = expectations.len();
        out.push(EvalImportCandidateDto {
            session_id,
            task,
            expectations,
            fail_count,
        });
    }
    out.sort_by(|a, b| {
        b.fail_count
            .cmp(&a.fail_count)
            .then_with(|| b.session_id.cmp(&a.session_id))
    });
    if limit > 0 {
        out.truncate(limit);
    }
    Ok(out)
}

/// 列出可从失败会话导入的评测例（未导入且含 ToolFailure / UserCorrection）。
#[tauri::command]
pub async fn list_eval_import_candidates(
    limit: Option<u32>,
) -> Result<Vec<EvalImportCandidateDto>, String> {
    let base = default_memory_dir();
    collect_eval_import_candidates(&base, limit.unwrap_or(12) as usize)
}

/// 从指定会话导入一条失败评测例。
#[tauri::command]
pub async fn import_eval_from_session(
    session_id: String,
    skill_id: Option<String>,
) -> Result<Vec<EvalExampleDto>, String> {
    let session_id = session_id.trim().to_string();
    if session_id.is_empty() {
        return Err("session_id 不能为空".into());
    }
    let base = default_memory_dir();
    let existing = list_examples(&base);
    if existing
        .iter()
        .any(|e| e.source_session.as_deref() == Some(session_id.as_str()))
    {
        return Err("该会话已导入评测集".into());
    }

    let store = session::SessionStore::open_sessions_dir(&base.join("sessions"))
        .map_err(|e| format!("打开会话库失败: {e}"))?;
    let task = session_user_task(&store, &session_id)
        .ok_or_else(|| "无法从会话提取任务文本".to_string())?;

    let decisions = memory::list_recent_decisions(&base, 200)
        .map_err(|e| format!("读取 DecisionLog 失败: {e}"))?;
    let mut expectations: Vec<String> = decisions
        .iter()
        .filter(|d| d.session_id.as_deref() == Some(session_id.as_str()))
        .filter(|d| {
            matches!(
                d.kind,
                DecisionKind::ToolFailure | DecisionKind::UserCorrection
            )
        })
        .map(|d| d.summary.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    expectations.sort();
    expectations.dedup();
    if expectations.is_empty() {
        return Err("该会话无工具失败或用户纠错记录".into());
    }

    let mut ex = EvalExample::new(
        skill_id.filter(|s| !s.trim().is_empty()),
        task,
        expectations,
        Verdict::Fail,
    );
    ex.source_session = Some(session_id);
    evolution::append_example(&base, &ex).map_err(|e| e.to_string())?;
    list_eval_examples().await
}

/// 评测例子展示态。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalExampleDto {
    pub id: String,
    pub skill_id: Option<String>,
    pub task: String,
    pub expectations: Vec<String>,
    pub verdict: String,
    pub source_session: Option<String>,
    pub created_at: String,
}

impl From<EvalExample> for EvalExampleDto {
    fn from(e: EvalExample) -> Self {
        let verdict = match e.verdict {
            evolution::Verdict::Pass => "pass",
            evolution::Verdict::Fail => "fail",
        }
        .to_string();
        Self {
            id: e.id,
            skill_id: e.skill_id,
            task: e.task,
            expectations: e.expectations,
            verdict,
            source_session: e.source_session,
            created_at: e.created_at,
        }
    }
}

/// 列出评测集。
#[tauri::command]
pub async fn list_eval_examples() -> Result<Vec<EvalExampleDto>, String> {
    let base = default_memory_dir();
    Ok(list_examples(&base)
        .into_iter()
        .map(EvalExampleDto::from)
        .collect())
}

/// 新增一条评测例子。
#[tauri::command]
pub async fn add_eval_example(
    skill_id: Option<String>,
    task: String,
    expectations: Vec<String>,
    verdict: String,
    source_session: Option<String>,
) -> Result<Vec<EvalExampleDto>, String> {
    let task = task.trim().to_string();
    if task.is_empty() {
        return Err("task 不能为空".into());
    }
    let v = match verdict.trim().to_lowercase().as_str() {
        "pass" => evolution::Verdict::Pass,
        "fail" => evolution::Verdict::Fail,
        _ => return Err("verdict 只能是 pass 或 fail".into()),
    };
    let mut ex = EvalExample::new(
        skill_id.filter(|s| !s.trim().is_empty()),
        task,
        expectations
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        v,
    );
    ex.source_session = source_session.filter(|s| !s.trim().is_empty());
    let base = default_memory_dir();
    evolution::append_example(&base, &ex).map_err(|e| e.to_string())?;
    list_eval_examples().await
}

/// 删除一条评测例子。
#[tauri::command]
pub async fn remove_eval_example(id: String) -> Result<Vec<EvalExampleDto>, String> {
    let base = default_memory_dir();
    evolution::remove_example(&base, &id).map_err(|e| e.to_string())?;
    list_eval_examples().await
}

/// 策展报告 DTO（camelCase）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurateReportDto {
    pub generated_at: String,
    pub unused_skill_days: u32,
    pub enabled_count: usize,
    pub stale: Vec<String>,
    pub rows: Vec<CurateSkillRowDto>,
    pub overlap_clusters: Vec<Vec<String>>,
    pub suggestions: Vec<CurateSuggestionDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurateSkillRowDto {
    pub skill_id: String,
    pub description: String,
    pub last_loaded: Option<String>,
    pub stale: bool,
    pub health_score: Option<f32>,
    pub health_reasons: Vec<String>,
    pub bytes: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurateSuggestionDto {
    pub kind: String,
    pub skill_id: String,
    pub reason: String,
}

fn curate_report_dto(r: CurateReport) -> CurateReportDto {
    let suggestions = r
        .suggestions
        .into_iter()
        .map(|s| match s {
            evolution::CurateSuggestion::Disable { skill_id, reason } => CurateSuggestionDto {
                kind: "disable".into(),
                skill_id,
                reason,
            },
            evolution::CurateSuggestion::Merge {
                keep,
                absorb,
                reason,
            } => CurateSuggestionDto {
                kind: "merge".into(),
                skill_id: format!("{keep} ← {}", absorb.join(", ")),
                reason,
            },
            evolution::CurateSuggestion::Rewrite { skill_id, reason } => CurateSuggestionDto {
                kind: "rewrite".into(),
                skill_id,
                reason,
            },
        })
        .collect();
    CurateReportDto {
        generated_at: r.generated_at,
        unused_skill_days: r.unused_skill_days,
        enabled_count: r.enabled_count,
        stale: r.stale,
        rows: r
            .rows
            .into_iter()
            .map(|row| CurateSkillRowDto {
                skill_id: row.skill_id,
                description: row.description,
                last_loaded: row.last_loaded,
                stale: row.stale,
                health_score: row.health_score,
                health_reasons: row.health_reasons,
                bytes: row.bytes,
            })
            .collect(),
        overlap_clusters: r.overlap_clusters,
        suggestions,
    }
}

/// 策展运行结果（含可选入队数）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CuratorRunReportDto {
    pub report: CurateReportDto,
    pub enqueued: usize,
}

/// 运行技能策展（结构化报告；可选将 Disable/Merge 建议入待审）。
///
/// 若 `curator.llm_diagnose` 开启且有可用 judge 路由，对低健康分或重叠簇
/// 的建议调用 LLM 生成一句可操作诊断，替换 reason。失败静默回退启发式 reason。
#[tauri::command]
pub async fn run_skill_curator(
    app: AppHandle,
    enqueue: Option<bool>,
) -> Result<CuratorRunReportDto, String> {
    let base = default_memory_dir();
    let unused = memory::load_learning_config(&base).unused_skill_days;
    let cfg = memory::load_evolution_config(&base);
    let mut report = run_curator_and_save(&base, unused).map_err(|e| e.to_string())?;

    // LLM 辅助诊断
    if cfg.curator.llm_diagnose && !report.suggestions.is_empty() {
        if let Ok(primary) = active_primary_target() {
            if let Ok(judge_targets) =
                resolve_evolution_targets(memory::EvolutionRouteKind::Judge, &primary)
            {
                let max_calls = cfg.curator.max_llm_calls.max(1);
                let mut diagnoses: Vec<(usize, String)> = Vec::new();
                for (i, sug) in report.suggestions.iter().enumerate() {
                    if diagnoses.len() as u32 >= max_calls {
                        break;
                    }
                    let prompt = evolution::build_diagnose_prompt(sug, &report.rows);
                    if let Ok(raw) = reflect_over_targets(
                        &judge_targets,
                        evolution::CURATOR_DIAGNOSE_SYSTEM_PROMPT,
                        &prompt,
                    )
                    .await
                    {
                        if let Some(d) = evolution::parse_diagnose_output(&raw) {
                            diagnoses.push((i, d));
                        }
                    }
                }
                if !diagnoses.is_empty() {
                    evolution::apply_diagnoses(&mut report.suggestions, &diagnoses);
                    // 重新落盘（带增强 reason）
                    let path = evolution::curator_last_path(&base);
                    if let Ok(json) = serde_json::to_string_pretty(&report) {
                        let _ = std::fs::write(&path, json.as_bytes());
                    }
                }
            }
        }
    }

    let mut enqueued = 0usize;
    if enqueue.unwrap_or(false) {
        enqueued = enqueue_curator_suggestions(&base, &report, cfg.curator.max_enqueue)
            .map_err(|e| e.to_string())?;
    }
    evolution::record_run(
        &base,
        "curator",
        report.enabled_count,
        0,
        0,
        report.suggestions.len().max(enqueued),
    );
    let suggestion_count = report.suggestions.len();
    let dto = CuratorRunReportDto {
        report: curate_report_dto(report),
        enqueued,
    };
    let _ = app.emit(
        "curator-updated",
        serde_json::json!({
            "suggestionCount": suggestion_count,
            "auto": false,
        }),
    );
    Ok(dto)
}

/// 将上次策展报告中的 Disable/Merge 建议入待审队列。
#[tauri::command]
pub async fn enqueue_curator_proposals() -> Result<usize, String> {
    let base = default_memory_dir();
    let report =
        load_curator_last(&base).ok_or_else(|| "尚无策展报告，请先运行策展".to_string())?;
    let max = memory::load_evolution_config(&base).curator.max_enqueue;
    let n = enqueue_curator_suggestions(&base, &report, max).map_err(|e| e.to_string())?;
    Ok(n)
}

/// 读取上次策展报告（若有）。
#[tauri::command]
pub async fn get_curator_last() -> Result<Option<CurateReportDto>, String> {
    let base = default_memory_dir();
    Ok(load_curator_last(&base).map(curate_report_dto))
}

/// 策展调度状态（是否到期、距上次天数）。
#[tauri::command]
pub async fn curator_status() -> Result<CuratorStatus, String> {
    let base = default_memory_dir();
    let cfg = memory::load_evolution_config(&base);
    let last = load_curator_last(&base);
    Ok(build_curator_status(
        cfg.curator.enabled,
        cfg.curator.interval_days,
        last.as_ref(),
        chrono::Utc::now(),
    ))
}

/// 自动策展探测结果（到期才跑；永不入队、不调 LLM 诊断）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CuratorMaybeRunDto {
    pub ran: bool,
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub skip_message: Option<String>,
    pub report: Option<CurateReportDto>,
    pub error: Option<String>,
}

/// 若 `curator.enabled` 且已过 `interval_days`（或从未跑过），生成启发式报告并落盘。
///
/// **不**入队、**不**调用 LLM（避免静默烧钱）；入队与诊断仍走手动 `run_skill_curator`。
#[tauri::command]
pub async fn maybe_run_skill_curator() -> Result<CuratorMaybeRunDto, String> {
    let base = default_memory_dir();
    let cfg = memory::load_evolution_config(&base);
    let last = load_curator_last(&base);
    let status = build_curator_status(
        cfg.curator.enabled,
        cfg.curator.interval_days,
        last.as_ref(),
        chrono::Utc::now(),
    );
    if !status.due {
        return Ok(CuratorMaybeRunDto {
            ran: false,
            skipped: true,
            skip_reason: status.skip_reason,
            skip_message: status.skip_message,
            report: last.map(curate_report_dto),
            error: None,
        });
    }

    if curator_inflight()
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(CuratorMaybeRunDto {
            ran: false,
            skipped: true,
            skip_reason: Some("inflight".into()),
            skip_message: Some("已有策展任务在运行".into()),
            report: last.map(curate_report_dto),
            error: None,
        });
    }

    let unused = memory::load_learning_config(&base).unused_skill_days;
    let result = run_curator_and_save(&base, unused);
    curator_inflight().store(false, Ordering::SeqCst);

    match result {
        Ok(report) => {
            evolution::record_run(
                &base,
                "curator",
                report.enabled_count,
                0,
                0,
                report.suggestions.len(),
            );
            tracing::info!(
                suggestions = report.suggestions.len(),
                "scheduled curator report saved (no enqueue)"
            );
            Ok(CuratorMaybeRunDto {
                ran: true,
                skipped: false,
                skip_reason: None,
                skip_message: None,
                report: Some(curate_report_dto(report)),
                error: None,
            })
        }
        Err(e) => {
            tracing::warn!(error = %e, "scheduled curator failed");
            Ok(CuratorMaybeRunDto {
                ran: false,
                skipped: false,
                skip_reason: None,
                skip_message: None,
                report: None,
                error: Some(e.to_string()),
            })
        }
    }
}

fn git(args: &[&str], cwd: &Path) -> Result<String, String> {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| format!("git 执行失败: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn git_root(start: &Path) -> Option<PathBuf> {
    let s = git(&["rev-parse", "--show-toplevel"], start).ok()?;
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

/// 批准提案到独立 worktree 分支：在新分支的独立检出里写入+commit，不动当前工作树。
///
/// 前提：Agent 技能目录位于某 git 仓库内；否则返回错误。成功后删除提案并返回分支与路径。
#[tauri::command]
pub async fn approve_evolution_proposal_to_branch(id: String) -> Result<String, String> {
    let base = default_memory_dir();
    let cand = list_proposals(&base)
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("提案不存在: {id}"))?;

    let skills_dir = skills::install::agent_skills_dir(None).map_err(|e| e.to_string())?;
    let repo = git_root(&skills_dir)
        .ok_or_else(|| "技能目录不在 git 仓库中，无法开分支（可用普通「批准写入」）".to_string())?;

    let canon_skills = skills_dir.canonicalize().unwrap_or(skills_dir.clone());
    let canon_repo = repo.canonicalize().unwrap_or(repo.clone());
    let rel = canon_skills
        .strip_prefix(&canon_repo)
        .map_err(|_| "技能目录不在仓库根之下".to_string())?
        .to_path_buf();

    let short: String = id.chars().take(6).collect();
    let branch = format!("astro/evolution/{}-{}", cand.skill_id, short);
    let wt_root = base.join("evolution-worktrees");
    std::fs::create_dir_all(&wt_root).map_err(|e| e.to_string())?;
    let wt = wt_root.join(format!("{}-{}", cand.skill_id, short));
    if wt.exists() {
        let _ = std::fs::remove_dir_all(&wt);
    }

    // 新分支 + 独立检出
    git(
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            &wt.to_string_lossy(),
            "HEAD",
        ],
        &repo,
    )?;

    let apply_and_commit = || -> Result<(), String> {
        let skill_dir = wt.join(&rel).join(&cand.skill_id);
        let skill_md = skill_dir.join("SKILL.md");
        match cand.kind {
            CandidateKind::NewSkill => {
                let md = candidate_new_markdown(&cand).map_err(|e| e.to_string())?;
                std::fs::create_dir_all(&skill_dir).map_err(|e| e.to_string())?;
                std::fs::write(&skill_md, md.as_bytes()).map_err(|e| e.to_string())?;
            }
            CandidateKind::Patch => {
                if !skill_md.is_file() {
                    return Err(format!("patch 目标在仓库中不存在: {}", cand.skill_id));
                }
                let old = cand.old_string.as_deref().unwrap_or("");
                let new = cand.new_string.as_deref().unwrap_or("");
                let text = std::fs::read_to_string(&skill_md).map_err(|e| e.to_string())?;
                let updated = apply_patch_unique(&text, old, new).map_err(|e| e.to_string())?;
                std::fs::write(&skill_md, updated.as_bytes()).map_err(|e| e.to_string())?;
            }
            CandidateKind::Disable | CandidateKind::Merge => {
                return Err("策展 Disable/Merge 请用「批准写入」，不支持批准到分支".into());
            }
        }
        git(&["add", "-A"], &wt)?;
        let msg = format!("evolve: {} ({:?})", cand.skill_id, cand.kind);
        git(&["commit", "-m", &msg], &wt)?;
        Ok(())
    };

    if let Err(e) = apply_and_commit() {
        // 回滚 worktree + 分支（best-effort）
        let _ = git(
            &["worktree", "remove", "--force", &wt.to_string_lossy()],
            &repo,
        );
        let _ = git(&["branch", "-D", &branch], &repo);
        return Err(e);
    }

    let kind = match cand.kind {
        CandidateKind::NewSkill => "new_skill",
        CandidateKind::Patch => "patch",
        CandidateKind::Disable => "disable",
        CandidateKind::Merge => "merge",
    };
    evolution::record_outcome(&base, &id, &cand.skill_id, kind, cand.judge_score, "branch");
    reject_proposal(&base, &id).map_err(|e| e.to_string())?;
    Ok(format!(
        "已在分支 `{branch}` 提交（worktree: {}）。可在该分支 review / 推送 / 开 PR。",
        home::display_user_path(&wt)
    ))
}
