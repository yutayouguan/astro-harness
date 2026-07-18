//! 离线进化「运行 / 提案审批」Tauri 命令。
//!
//! `run_evolution`：读 DecisionLog + 已启用技能 → reflection 模型产候选 → gates
//! 过滤 → 存待审提案（`require_pr` 默认，永不自动应用）。审批走
//! `list/approve/reject_evolution_proposal`。judge 路由已可解析，v1 暂不参与打分。

use futures::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use evolution::{
    approve_proposal, build_judge_user_prompt, build_reflection_user_prompt, check_candidate,
    list_proposals, parse_candidates, parse_judge_output, reject_proposal, save_proposals,
    CandidateKind, ReflectionInput, SkillCandidate, JUDGE_SYSTEM_PROMPT, REFLECTION_SYSTEM_PROMPT,
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
    let input = ReflectionInput {
        decisions,
        enabled_skills,
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
            let mut kept: Vec<SkillCandidate> = Vec::new();
            for mut c in passed.into_iter() {
                let user = build_judge_user_prompt(&c, &enabled_now);
                match reflect_over_targets(&judge_targets, JUDGE_SYSTEM_PROMPT, &user).await {
                    Ok(raw) => match parse_judge_output(&raw) {
                        Ok(v) => {
                            c.judge_score = Some(v.score);
                            c.judge_reason = Some(v.reason);
                            if v.keep && v.score >= cfg.gates.min_judge_score {
                                kept.push(c);
                            } else {
                                judged_out += 1;
                                tracing::info!(skill = %c.skill_id, score = v.score, "候选被 judge 拒绝");
                            }
                        }
                        Err(e) => {
                            tracing::warn!(skill = %c.skill_id, error = %e, "judge 解析失败，保留候选");
                            kept.push(c);
                        }
                    },
                    Err(e) => {
                        tracing::warn!(skill = %c.skill_id, error = %e, "judge 调用失败，保留候选");
                        kept.push(c);
                    }
                }
            }
            passed = kept;
        }
    }

    save_proposals(&base, &passed).map_err(|e| e.to_string())?;

    let proposals: Vec<EvolutionProposalDto> =
        passed.into_iter().map(EvolutionProposalDto::from).collect();

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

/// 列出全部待审进化提案。
#[tauri::command]
pub async fn list_evolution_proposals() -> Result<Vec<EvolutionProposalDto>, String> {
    let base = default_memory_dir();
    Ok(list_proposals(&base)
        .into_iter()
        .map(EvolutionProposalDto::from)
        .collect())
}

/// 批准一条提案：写入 Agent skills 目录。
#[tauri::command]
pub async fn approve_evolution_proposal(id: String) -> Result<String, String> {
    let base = default_memory_dir();
    approve_proposal(&base, &id).map_err(|e| e.to_string())
}

/// 拒绝并删除一条提案。
#[tauri::command]
pub async fn reject_evolution_proposal(id: String) -> Result<(), String> {
    let base = default_memory_dir();
    reject_proposal(&base, &id).map_err(|e| e.to_string())
}
