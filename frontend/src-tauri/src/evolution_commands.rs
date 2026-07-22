//! 离线进化设置 Tauri 命令：`evolution.*`（enabled + reflection/judge 路由 + gates）。
//!
//! 与 `auxiliary_commands` 同构：`provider=auto` 跟随会话主模型，显式值保存
//! **UI Provider ID**。本模块只承载配置读写（无网络/凭据副作用）；
//! 实际运行 / 搜索 / 审批见 `evolution_run_commands`。

use serde::Serialize;

use crate::providers_commands::{self, ProviderConfigDto};

/// 单个进化路由在设置面的展示态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionRouteDto {
    pub id: String,
    pub provider: String,
    pub model: String,
    pub display_label: String,
    pub unavailable: bool,
}

/// 进化门禁展示态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionGatesDto {
    pub run_tests: bool,
    pub max_skill_bytes: u64,
    pub require_pr: bool,
    pub min_judge_score: f32,
}

/// 遗传搜索参数展示态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionSearchDto {
    pub generations: u32,
    pub variants: u32,
    pub crossover: bool,
    pub population_size: u32,
    pub max_eval_examples: usize,
    pub max_llm_calls: u32,
    /// [P0] 批准后冷却期（秒）。0 = 禁用。
    pub post_approval_cooldown_secs: u64,
}

/// 自动触发参数展示态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionAutoDto {
    pub enabled: bool,
    pub cooldown_secs: u64,
    pub min_new_decisions: u32,
    pub max_runs_per_day: u32,
    /// [P2] 触发定向进化所需的最少失败信号数。
    pub min_skill_failure_signals: u32,
    /// [P2] 失败信号统计窗口（天）。
    pub signal_window_days: u32,
}

/// 策展参数展示态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionCuratorDto {
    pub enabled: bool,
    pub interval_days: u32,
    pub max_enqueue: u32,
    pub llm_diagnose: bool,
    pub max_llm_calls: u32,
}

/// 进化设置全量 DTO。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionSettingsDto {
    pub enabled: bool,
    pub routes: Vec<EvolutionRouteDto>,
    pub gates: EvolutionGatesDto,
    pub search: EvolutionSearchDto,
    pub auto: EvolutionAutoDto,
    pub curator: EvolutionCuratorDto,
    pub active_provider_id: Option<String>,
    pub active_model: String,
}

fn parse_route_kind(kind: &str) -> Result<memory::EvolutionRouteKind, String> {
    match kind {
        "reflection" => Ok(memory::EvolutionRouteKind::Reflection),
        "judge" => Ok(memory::EvolutionRouteKind::Judge),
        _ => Err("unknown evolution route".to_string()),
    }
}

/// 只接受 `(auto, auto)` 或两个非空显式值，拒绝半自动组合。
fn parse_route(provider: String, model: String) -> Result<memory::AuxiliaryRoute, String> {
    let provider = provider.trim().to_string();
    let model = model.trim().to_string();
    let both_auto = provider.eq_ignore_ascii_case("auto") && model.eq_ignore_ascii_case("auto");
    let both_explicit = !provider.is_empty()
        && !model.is_empty()
        && !provider.eq_ignore_ascii_case("auto")
        && !model.eq_ignore_ascii_case("auto");
    if !both_auto && !both_explicit {
        return Err("provider and model must both be auto or explicit".into());
    }
    Ok(memory::AuxiliaryRoute { provider, model })
}

fn is_auto(route: &memory::AuxiliaryRoute) -> bool {
    route.provider.trim().eq_ignore_ascii_case("auto")
}

fn build_route_dto(
    kind: memory::EvolutionRouteKind,
    route: &memory::AuxiliaryRoute,
    providers: &[ProviderConfigDto],
) -> EvolutionRouteDto {
    if is_auto(route) {
        return EvolutionRouteDto {
            id: kind.config_key().to_string(),
            provider: "auto".into(),
            model: "auto".into(),
            display_label: "自动跟随主模型".into(),
            unavailable: false,
        };
    }
    let found = providers.iter().find(|p| p.id == route.provider);
    let (display_label, unavailable) = match found {
        Some(p) => (
            format!("{} · {}", p.display_name, route.model),
            !p.enabled || !p.has_api_key,
        ),
        None => (format!("{}（供应商不存在）", route.provider), true),
    };
    EvolutionRouteDto {
        id: kind.config_key().to_string(),
        provider: route.provider.clone(),
        model: route.model.clone(),
        display_label,
        unavailable,
    }
}

fn build_settings_dto() -> Result<EvolutionSettingsDto, String> {
    let base = home::default_memory_dir();
    let cfg = memory::load_evolution_config(&base);
    let state = providers_commands::get_providers_state()?;

    let routes = memory::EvolutionRouteKind::ALL
        .into_iter()
        .map(|kind| build_route_dto(kind, cfg.route(kind), &state.providers))
        .collect();

    let active_model = state
        .active_provider_id
        .as_deref()
        .and_then(|id| state.providers.iter().find(|p| p.id == id))
        .map(|p| p.model.clone())
        .unwrap_or_default();

    Ok(EvolutionSettingsDto {
        enabled: cfg.enabled,
        routes,
        gates: EvolutionGatesDto {
            run_tests: cfg.gates.run_tests,
            max_skill_bytes: cfg.gates.max_skill_bytes as u64,
            require_pr: cfg.gates.require_pr,
            min_judge_score: cfg.gates.min_judge_score,
        },
        search: EvolutionSearchDto {
            generations: cfg.search.generations,
            variants: cfg.search.variants,
            crossover: cfg.search.crossover,
            population_size: cfg.search.population_size,
            max_eval_examples: cfg.search.max_eval_examples,
            max_llm_calls: cfg.search.max_llm_calls,
            post_approval_cooldown_secs: cfg.search.post_approval_cooldown_secs,
        },
        auto: EvolutionAutoDto {
            enabled: cfg.auto.enabled,
            cooldown_secs: cfg.auto.cooldown_secs,
            min_new_decisions: cfg.auto.min_new_decisions as u32,
            max_runs_per_day: cfg.auto.max_runs_per_day,
            min_skill_failure_signals: cfg.auto.min_skill_failure_signals as u32,
            signal_window_days: cfg.auto.signal_window_days,
        },
        curator: EvolutionCuratorDto {
            enabled: cfg.curator.enabled,
            interval_days: cfg.curator.interval_days,
            max_enqueue: cfg.curator.max_enqueue as u32,
            llm_diagnose: cfg.curator.llm_diagnose,
            max_llm_calls: cfg.curator.max_llm_calls,
        },
        active_provider_id: state.active_provider_id,
        active_model,
    })
}

/// 读取进化设置。
#[tauri::command]
pub async fn get_evolution_settings() -> Result<EvolutionSettingsDto, String> {
    build_settings_dto()
}

/// 设置 `evolution.enabled`。
#[tauri::command]
pub async fn set_evolution_enabled(enabled: bool) -> Result<EvolutionSettingsDto, String> {
    let base = home::default_memory_dir();
    memory::set_evolution_enabled(&base, enabled).map_err(|e| e.to_string())?;
    build_settings_dto()
}

/// 设置单条进化路由（`reflection` / `judge`）。
#[tauri::command]
pub async fn set_evolution_route(
    route: String,
    provider: String,
    model: String,
) -> Result<EvolutionSettingsDto, String> {
    let kind = parse_route_kind(&route)?;
    let parsed = parse_route(provider, model)?;
    let base = home::default_memory_dir();
    memory::set_evolution_route(&base, kind, parsed).map_err(|e| e.to_string())?;
    build_settings_dto()
}

/// 将单条进化路由重置为 `auto`/`auto`。
#[tauri::command]
pub async fn reset_evolution_route(route: String) -> Result<EvolutionSettingsDto, String> {
    let kind = parse_route_kind(&route)?;
    let base = home::default_memory_dir();
    memory::set_evolution_route(&base, kind, memory::AuxiliaryRoute::default())
        .map_err(|e| e.to_string())?;
    build_settings_dto()
}

/// 更新进化门禁。
#[tauri::command]
pub async fn set_evolution_gates(
    run_tests: bool,
    max_skill_bytes: u64,
    require_pr: bool,
    min_judge_score: f32,
) -> Result<EvolutionSettingsDto, String> {
    let base = home::default_memory_dir();
    let _ = require_pr; // 产品不变量：始终人审
    let current = memory::load_evolution_config(&base);
    let gates = memory::EvolutionGates {
        run_tests,
        max_skill_bytes: max_skill_bytes as usize,
        require_pr: true,
        min_judge_score,
        sandbox_mode: current.gates.sandbox_mode,
        sandbox_docker_image: current.gates.sandbox_docker_image,
    };
    memory::set_evolution_gates(&base, &gates).map_err(|e| e.to_string())?;
    build_settings_dto()
}

/// 设置遗传搜索参数。
#[tauri::command]
pub async fn set_evolution_search(
    generations: u32,
    variants: u32,
    crossover: bool,
    population_size: Option<u32>,
    max_eval_examples: Option<u32>,
    max_llm_calls: Option<u32>,
    post_approval_cooldown_secs: Option<u64>,
) -> Result<EvolutionSettingsDto, String> {
    let base = home::default_memory_dir();
    let current = memory::load_evolution_config(&base);
    let search = memory::EvolutionSearch {
        generations: generations.clamp(1, 6),
        variants: variants.clamp(1, 6),
        crossover,
        population_size: population_size
            .unwrap_or(current.search.population_size)
            .clamp(1, 8),
        max_eval_examples: max_eval_examples
            .map(|v| v as usize)
            .unwrap_or(current.search.max_eval_examples)
            .min(32),
        max_llm_calls: max_llm_calls
            .unwrap_or(current.search.max_llm_calls)
            .min(500),
        mutation_system_prompt: current.search.mutation_system_prompt,
        crossover_system_prompt: current.search.crossover_system_prompt,
        eval_sampling: current.search.eval_sampling,
        post_approval_cooldown_secs: post_approval_cooldown_secs
            .unwrap_or(current.search.post_approval_cooldown_secs)
            .min(604_800),
    };
    memory::set_evolution_search(&base, &search).map_err(|e| e.to_string())?;
    build_settings_dto()
}

/// 设置自动触发参数（默认关；冷却/日限额/最低新决策数）。
#[tauri::command]
pub async fn set_evolution_auto(
    enabled: bool,
    cooldown_secs: u64,
    min_new_decisions: u32,
    max_runs_per_day: u32,
    min_skill_failure_signals: Option<u32>,
    signal_window_days: Option<u32>,
) -> Result<EvolutionSettingsDto, String> {
    let base = home::default_memory_dir();
    let current = memory::load_evolution_config(&base);
    let auto = memory::EvolutionAuto {
        enabled,
        cooldown_secs: cooldown_secs.clamp(60, 86_400),
        min_new_decisions: (min_new_decisions.clamp(1, 50)) as usize,
        max_runs_per_day: max_runs_per_day.clamp(1, 24),
        min_skill_failure_signals: min_skill_failure_signals
            .unwrap_or(current.auto.min_skill_failure_signals as u32)
            .clamp(0, 20) as usize,
        signal_window_days: signal_window_days
            .unwrap_or(current.auto.signal_window_days)
            .clamp(1, 30),
    };
    memory::set_evolution_auto(&base, &auto).map_err(|e| e.to_string())?;
    build_settings_dto()
}

/// 设置策展参数。
#[tauri::command]
pub async fn set_evolution_curator(
    enabled: bool,
    interval_days: u32,
    max_enqueue: u32,
    llm_diagnose: bool,
    max_llm_calls: u32,
) -> Result<EvolutionSettingsDto, String> {
    let base = home::default_memory_dir();
    let curator = memory::EvolutionCurator {
        enabled,
        interval_days: interval_days.clamp(1, 90),
        max_enqueue: (max_enqueue.clamp(1, 20)) as usize,
        llm_diagnose,
        max_llm_calls: max_llm_calls.clamp(1, 20),
    };
    memory::set_evolution_curator(&base, &curator).map_err(|e| e.to_string())?;
    build_settings_dto()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_route_kind_covers_both_and_rejects_unknown() {
        assert!(parse_route_kind("reflection").is_ok());
        assert!(parse_route_kind("judge").is_ok());
        assert!(parse_route_kind("nope").is_err());
    }

    #[test]
    fn parse_route_rejects_half_auto() {
        assert!(parse_route("auto".into(), "auto".into()).is_ok());
        assert!(parse_route("p".into(), "m".into()).is_ok());
        assert!(parse_route("p".into(), "auto".into()).is_err());
        assert!(parse_route("".into(), "".into()).is_err());
    }

    #[test]
    fn build_route_dto_auto_is_available() {
        let route = memory::AuxiliaryRoute::default();
        let dto = build_route_dto(memory::EvolutionRouteKind::Reflection, &route, &[]);
        assert_eq!(dto.id, "reflection");
        assert_eq!(dto.provider, "auto");
        assert!(!dto.unavailable);
    }

    #[test]
    fn build_route_dto_missing_provider_unavailable() {
        let route = memory::AuxiliaryRoute {
            provider: "gone".into(),
            model: "m".into(),
        };
        let dto = build_route_dto(memory::EvolutionRouteKind::Judge, &route, &[]);
        assert!(dto.unavailable);
        assert_eq!(dto.id, "judge");
    }
}
