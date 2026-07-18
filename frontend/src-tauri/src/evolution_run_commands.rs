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

use std::collections::HashSet;

use evolution::{
    apply_patch_unique, approve_proposal, approve_proposal_checked, build_eval_judge_prompt,
    build_judge_user_prompt, build_mutation_prompt, build_reflection_user_prompt,
    candidate_new_markdown, check_candidate, examples_for_skill, list_examples, list_proposals,
    parse_candidates, parse_eval_score, parse_judge_output, parse_variants, pareto_front,
    reject_proposal, save_proposals, select_front_capped, build_crossover_prompt, CandidateKind,
    EvalExample, ReflectionInput, ScoredVariant, SkillCandidate, CROSSOVER_SYSTEM_PROMPT,
    EVAL_JUDGE_SYSTEM_PROMPT, JUDGE_SYSTEM_PROMPT, MUTATION_SYSTEM_PROMPT, REFLECTION_SYSTEM_PROMPT,
};
use home::default_memory_dir;
use providers::registry::ProviderRegistry;
use providers::trait_::{ChatMessage, ProviderConfig};

use crate::auxiliary_resolver::{resolve_evolution_targets, AuxiliaryTargets, ResolvedTarget};
use crate::providers_commands::{self, resolve_api_key, ProviderConfig as UiProvider};

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
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionSearchReport {
    pub ok: bool,
    pub generations: u32,
    pub variants_evaluated: usize,
    pub pareto_kept: usize,
    pub proposals: Vec<EvolutionProposalDto>,
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

fn active_primary_target() -> Result<common::ChatTarget, String> {
    let ui = active_ui_provider()?;
    if ui.model.trim().is_empty() {
        return Err("激活提供商未配置模型".into());
    }
    let (_has, _src, _env, key) = resolve_api_key(&ui);
    Ok(common::ChatTarget {
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
    let registry = ProviderRegistry::default();
    let provider = registry
        .get(&target.backend_id)
        .ok_or_else(|| format!("不支持的提供商后端: {}", target.backend_id))?;
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
        ChatMessage::text("system", system),
        ChatMessage::text("user", user),
    ];
    let mut stream = provider
        .chat_stream(messages, vec![], &config)
        .await
        .map_err(|e| format!("进化调用模型失败: {e}"))?;
    let mut out = String::new();
    while let Some(item) = stream.next().await {
        let chunk = item.map_err(|e| format!("进化流式读取失败: {e}"))?;
        if let Some(token) = chunk.token {
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
fn build_transcripts(
    base: &Path,
    decisions: &[memory::DecisionEntry],
) -> Vec<(String, String)> {
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

/// 运行一次离线进化（生成待审提案）。
#[tauri::command]
pub async fn run_evolution(app: AppHandle) -> Result<EvolutionRunReport, String> {
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
            .map_or(true, |fb| fb.provider.kind.requires_api_key() && fb.api_key.trim().is_empty())
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
    };
    let user = build_reflection_user_prompt(&input);

    let raw = reflect_over_targets(&targets, REFLECTION_SYSTEM_PROMPT, &user).await?;
    let candidates = parse_candidates(&raw).map_err(|e| e.to_string())?;
    let generated = candidates.len();

    let mut passed: Vec<SkillCandidate> = Vec::new();
    let mut gated_out = 0usize;
    for c in candidates {
        let outcome = check_candidate(&c, &cfg.gates);
        if outcome.passed {
            passed.push(c);
        } else {
            gated_out += 1;
            tracing::info!(skill = %c.skill_id, reasons = ?outcome.reasons, "候选被门禁拦截");
        }
    }

    // judge 评审（min_judge_score <= 0 时关闭）；judge 调用失败对该候选 fail-open 保留。
    let mut judged_out = 0usize;
    if cfg.gates.min_judge_score > 0.0 && !passed.is_empty() {
        if let Ok(judge_targets) =
            resolve_evolution_targets(memory::EvolutionRouteKind::Judge, &primary)
        {
            let enabled_now = skills::list_enabled_for_prompt();
            let evalset = list_examples(&base);
            let mut kept: Vec<SkillCandidate> = Vec::new();
            for mut c in passed.into_iter() {
                let (score, reason) =
                    fitness_score(&judge_targets, &c, &enabled_now, &evalset).await;
                c.judge_score = Some(score);
                c.judge_reason = Some(reason);
                if score >= cfg.gates.min_judge_score {
                    kept.push(c);
                } else {
                    judged_out += 1;
                    tracing::info!(skill = %c.skill_id, score, "候选被适应度评分拒绝");
                }
            }
            passed = kept;
        }
    }

    save_proposals(&base, &passed).map_err(|e| e.to_string())?;

    let proposals: Vec<EvolutionProposalDto> =
        passed.into_iter().map(EvolutionProposalDto::from).collect();

    evolution::record_run(&base, "reflect", generated, gated_out, judged_out, proposals.len());
    let _ = app.emit(
        "evolution-updated",
        serde_json::json!({
            "generated": generated,
            "proposals": proposals.len(),
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

/// 用 judge 给单个候选打分：失败回退中性分 0.5。
async fn judge_candidate(
    targets: &AuxiliaryTargets,
    cand: &SkillCandidate,
    enabled_skills: &[(String, String)],
) -> (f32, String) {
    let user = build_judge_user_prompt(cand, enabled_skills);
    match reflect_over_targets(targets, JUDGE_SYSTEM_PROMPT, &user).await {
        Ok(raw) => match parse_judge_output(&raw) {
            Ok(v) => (v.score, v.reason),
            Err(_) => (0.5, "judge 解析失败（中性分）".into()),
        },
        Err(_) => (0.5, "judge 调用失败（中性分）".into()),
    }
}

/// 客观适应度：若候选技能有匹配评测例子，则对每个例子做 grounded 评分取均值；
/// 否则回退泛化 judge。返回 (score, reason)。
async fn fitness_score(
    targets: &AuxiliaryTargets,
    cand: &SkillCandidate,
    enabled_skills: &[(String, String)],
    evalset: &[EvalExample],
) -> (f32, String) {
    let matched = examples_for_skill(evalset, &cand.skill_id);
    if matched.is_empty() {
        return judge_candidate(targets, cand, enabled_skills).await;
    }
    let mut sum = 0.0f32;
    let mut n = 0u32;
    for ex in &matched {
        let user = build_eval_judge_prompt(cand, ex);
        if let Ok(raw) = reflect_over_targets(targets, EVAL_JUDGE_SYSTEM_PROMPT, &user).await {
            if let Ok(score) = parse_eval_score(&raw) {
                sum += score;
                n += 1;
            }
        }
    }
    if n == 0 {
        return judge_candidate(targets, cand, enabled_skills).await;
    }
    let avg = sum / n as f32;
    (avg, format!("基于 {n} 个评测例子的 grounded 评分"))
}

/// GEPA-lite 遗传搜索：种子 → 每目标多代变异 + judge 打分 + Pareto 选择 → 待审提案。
#[tauri::command]
pub async fn run_evolution_search(app: AppHandle) -> Result<EvolutionSearchReport, String> {
    let base = default_memory_dir();
    let cfg = memory::load_evolution_config(&base);
    if !cfg.enabled {
        return Err("请先在「离线进化」中开启进化".into());
    }

    let primary = active_primary_target()?;
    let refl_targets =
        resolve_evolution_targets(memory::EvolutionRouteKind::Reflection, &primary)?;
    if refl_targets.preferred.provider.kind.requires_api_key()
        && refl_targets.preferred.api_key.trim().is_empty()
        && refl_targets.fallback.as_ref().map_or(true, |fb| {
            fb.provider.kind.requires_api_key() && fb.api_key.trim().is_empty()
        })
    {
        return Err("未配置 API Key，无法运行进化".into());
    }
    let judge_targets =
        resolve_evolution_targets(memory::EvolutionRouteKind::Judge, &primary)?;

    // 种子：reflection 产候选
    let decisions = memory::list_recent_decisions(&base, 20).unwrap_or_default();
    let enabled_skills = skills::list_enabled_for_prompt();
    let transcripts = build_transcripts(&base, &decisions);
    let seed_user = build_reflection_user_prompt(&ReflectionInput {
        decisions,
        enabled_skills: enabled_skills.clone(),
        transcripts,
    });
    let seed_raw = reflect_over_targets(&refl_targets, REFLECTION_SYSTEM_PROMPT, &seed_user).await?;
    let seeds_all = parse_candidates(&seed_raw).map_err(|e| e.to_string())?;

    // 按 (skill_id, kind) 去重，最多 3 个目标
    let mut seen: HashSet<String> = HashSet::new();
    let mut seeds: Vec<SkillCandidate> = Vec::new();
    for c in seeds_all {
        let key = format!("{}::{:?}", c.skill_id, c.kind);
        if seen.insert(key) {
            seeds.push(c);
        }
        if seeds.len() >= 3 {
            break;
        }
    }

    let evalset = list_examples(&base);
    let generations = cfg.search.generations.max(1);
    let variants = cfg.search.variants.max(1);
    let mut variants_evaluated = 0usize;
    let mut pareto_kept = 0usize;
    let mut final_props: Vec<SkillCandidate> = Vec::new();

    for seed in seeds {
        let mut current = seed.clone();
        let mut critiques: Vec<String> = Vec::new();
        let mut last_front: Vec<ScoredVariant> = Vec::new();

        for _gen in 0..generations {
            let muser = build_mutation_prompt(&current, variants, &critiques);
            let raw = match reflect_over_targets(&refl_targets, MUTATION_SYSTEM_PROMPT, &muser).await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(skill = %current.skill_id, error = %e, "变异调用失败，停止该目标");
                    break;
                }
            };
            let mut cands = parse_variants(&raw, &current).unwrap_or_default();
            cands.push(current.clone()); // 保留上一代最优作为基线
            if cands.is_empty() {
                break;
            }

            let mut scored: Vec<ScoredVariant> = Vec::new();
            for mut c in cands {
                let (score, reason) =
                    fitness_score(&judge_targets, &c, &enabled_skills, &evalset).await;
                c.judge_score = Some(score);
                c.judge_reason = Some(reason);
                variants_evaluated += 1;
                scored.push(ScoredVariant::new(c, score));
            }

            // 交叉：对当前最高分的两个变体融合出一个子代，评分后并入选择。
            if cfg.search.crossover && scored.len() >= 2 {
                let top = select_front_capped(pareto_front(&scored), 2);
                if top.len() == 2 {
                    let cx = build_crossover_prompt(&top[0].candidate, &top[1].candidate);
                    if let Ok(raw) =
                        reflect_over_targets(&refl_targets, CROSSOVER_SYSTEM_PROMPT, &cx).await
                    {
                        for mut child in parse_variants(&raw, &current).unwrap_or_default() {
                            let (score, reason) =
                                fitness_score(&judge_targets, &child, &enabled_skills, &evalset)
                                    .await;
                            child.judge_score = Some(score);
                            child.judge_reason = Some(format!("[交叉] {reason}"));
                            variants_evaluated += 1;
                            scored.push(ScoredVariant::new(child, score));
                        }
                    }
                }
            }

            let front = pareto_front(&scored);
            critiques = front
                .iter()
                .filter_map(|v| v.candidate.judge_reason.clone())
                .collect();
            if let Some(best) = select_front_capped(front.clone(), 1).into_iter().next() {
                current = best.candidate.clone();
            }
            last_front = front;
        }

        // 每目标取 Pareto front 前 2 个，过静态门禁
        for v in select_front_capped(last_front, 2) {
            pareto_kept += 1;
            if check_candidate(&v.candidate, &cfg.gates).passed {
                final_props.push(v.candidate);
            }
        }
    }

    save_proposals(&base, &final_props).map_err(|e| e.to_string())?;
    let proposals: Vec<EvolutionProposalDto> =
        final_props.into_iter().map(EvolutionProposalDto::from).collect();

    evolution::record_run(&base, "search", variants_evaluated, 0, 0, proposals.len());
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
                    .map(|o| String::from_utf8_lossy(&o.stderr).chars().take(500).collect::<String>())
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
    let run_tests = memory::load_evolution_config(&base).gates.run_tests;
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
    let project = resolve_dspy_project(&app, &memory::load_evolution_config(&base).dspy.project_path)
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
        (String::new(), String::new(), "mock".to_string(), String::new())
    } else {
        let primary = active_primary_target()?;
        let targets = resolve_evolution_targets(memory::EvolutionRouteKind::Reflection, &primary)?;
        let t = targets.preferred;
        (t.model, t.provider.endpoint, t.backend_id, t.api_key)
    };

    // 导出输入到临时目录
    let run_id: String = uuid::Uuid::new_v4().to_string().chars().take(8).collect();
    let dir = base.join("learning").join("evolution").join(format!("dspy-run-{run_id}"));
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
        serde_json::to_string_pretty(&config_json).unwrap_or_default().as_bytes(),
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

    let raw = std::fs::read_to_string(&output)
        .map_err(|_| "DSPy 未产出 result.json".to_string())?;
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
    let gated = if check_candidate(&cand, &cfg.gates).passed {
        save_proposals(&base, std::slice::from_ref(&cand)).map_err(|e| e.to_string())?;
        proposals.push(EvolutionProposalDto::from(cand));
        0
    } else {
        1
    };
    evolution::record_run(&base, "dspy", 1, gated, 0, proposals.len());
    let _ = std::fs::remove_dir_all(&dir);

    let _ = app.emit("evolution-updated", serde_json::json!({ "dspy": true, "proposals": proposals.len() }));

    Ok(EvolutionRunReport {
        ok: true,
        generated: 1,
        gated_out: if proposals.is_empty() { 1 } else { 0 },
        judged_out: 0,
        proposals,
        error: None,
    })
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

    let skills_dir =
        skills::install::agent_skills_dir(None).map_err(|e| e.to_string())?;
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
        &["worktree", "add", "-b", &branch, &wt.to_string_lossy(), "HEAD"],
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
        }
        git(&["add", "-A"], &wt)?;
        let msg = format!("evolve: {} ({:?})", cand.skill_id, cand.kind);
        git(&["commit", "-m", &msg], &wt)?;
        Ok(())
    };

    if let Err(e) = apply_and_commit() {
        // 回滚 worktree + 分支（best-effort）
        let _ = git(&["worktree", "remove", "--force", &wt.to_string_lossy()], &repo);
        let _ = git(&["branch", "-D", &branch], &repo);
        return Err(e);
    }

    let kind = match cand.kind {
        CandidateKind::NewSkill => "new_skill",
        CandidateKind::Patch => "patch",
    };
    evolution::record_outcome(&base, &id, &cand.skill_id, kind, cand.judge_score, "branch");
    reject_proposal(&base, &id).map_err(|e| e.to_string())?;
    Ok(format!(
        "已在分支 `{branch}` 提交（worktree: {}）。可在该分支 review / 推送 / 开 PR。",
        home::display_user_path(&wt)
    ))
}
