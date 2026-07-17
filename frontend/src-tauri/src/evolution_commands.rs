//! 离线进化设置 Tauri 命令：`evolution.*`（enabled + reflection/judge 路由 + gates）。
//!
//! 与 `auxiliary_commands` 同构：`provider=auto` 跟随会话主模型，显式值保存
//! **UI Provider ID**。进化引擎（GEPA/DSPy 流水线）为 Phase 2，未实现；这里只
//! 承载配置读写，无网络/凭据副作用。

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
}

/// 进化设置全量 DTO。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionSettingsDto {
    pub enabled: bool,
    pub routes: Vec<EvolutionRouteDto>,
    pub gates: EvolutionGatesDto,
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
) -> Result<EvolutionSettingsDto, String> {
    let base = home::default_memory_dir();
    let gates = memory::EvolutionGates {
        run_tests,
        max_skill_bytes: max_skill_bytes as usize,
        require_pr,
    };
    memory::set_evolution_gates(&base, &gates).map_err(|e| e.to_string())?;
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
