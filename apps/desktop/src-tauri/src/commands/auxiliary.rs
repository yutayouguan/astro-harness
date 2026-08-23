//! 辅助模型设置 Tauri 命令：`auxiliary.*` 五类任务路由的读取/写入/重置。
//!
//! `provider` 为 `"auto"` 时跟随会话主模型；显式值保存 **UI Provider ID**
//! （`providers.json` 条目 `id`），由 `auxiliary_resolver` 在运行时解析为具体
//! 后端凭据。本文件只负责设置面的读写与展示，不做任何网络/凭据解析副作用。

use serde::Serialize;

use super::providers::{self as providers_commands, ProviderConfigDto};

/// 单个辅助任务在设置面的展示态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuxiliaryTaskDto {
    pub id: String,
    pub provider: String,
    pub model: String,
    pub display_label: String,
    pub unavailable: bool,
}

/// 全部五类辅助任务 + 当前激活主模型（供「auto」展示用参照）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuxiliarySettingsDto {
    pub tasks: Vec<AuxiliaryTaskDto>,
    pub active_provider_id: Option<String>,
    pub active_model: String,
}

/// 将 `task` 字符串穷举解析为 [`memory::AuxiliaryKind`]；未知值报错。
fn parse_task(task: &str) -> Result<memory::AuxiliaryKind, String> {
    match task {
        "title_generation" => Ok(memory::AuxiliaryKind::TitleGeneration),
        "compaction" => Ok(memory::AuxiliaryKind::Compaction),
        "smart_approval" => Ok(memory::AuxiliaryKind::SmartApproval),
        "dreaming" => Ok(memory::AuxiliaryKind::Dreaming),
        "background_review" => Ok(memory::AuxiliaryKind::BackgroundReview),
        _ => Err("unknown auxiliary task".to_string()),
    }
}

/// 校验并构造路由：只接受 `(auto, auto)` 或两个非空显式值，拒绝半自动组合
/// （避免 provider 显式但 model 仍 auto 等来源不一致的状态）。
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

/// 单个路由 → 展示 DTO：`auto` 直显跟随主模型；显式路由按 `providers` 列表
/// 查 enabled/hasApiKey 判定 `unavailable`（模型缓存缺失不计入，避免离线误报）。
fn build_task_dto(
    kind: memory::AuxiliaryKind,
    route: &memory::AuxiliaryRoute,
    providers: &[ProviderConfigDto],
) -> AuxiliaryTaskDto {
    if is_auto(route) {
        return AuxiliaryTaskDto {
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
    AuxiliaryTaskDto {
        id: kind.config_key().to_string(),
        provider: route.provider.clone(),
        model: route.model.clone(),
        display_label,
        unavailable,
    }
}

fn build_settings_dto() -> Result<AuxiliarySettingsDto, String> {
    let base = home::default_memory_dir();
    let aux = memory::load_auxiliary_config(&base);
    let state = providers_commands::get_providers_state()?;

    let tasks = memory::AuxiliaryKind::ALL
        .into_iter()
        .map(|kind| build_task_dto(kind, aux.route(kind), &state.providers))
        .collect();

    let active_model = state
        .active_provider_id
        .as_deref()
        .and_then(|id| state.providers.iter().find(|p| p.id == id))
        .map(|p| p.model.clone())
        .unwrap_or_default();

    Ok(AuxiliarySettingsDto {
        tasks,
        active_provider_id: state.active_provider_id,
        active_model,
    })
}

/// Tauri 命令：读取五类辅助任务当前路由与展示态。
#[tauri::command]
pub async fn get_auxiliary_settings() -> Result<AuxiliarySettingsDto, String> {
    build_settings_dto()
}

/// Tauri 命令：设置单个辅助任务的路由（`auto`/`auto` 或两个显式值）。
#[tauri::command]
pub async fn set_auxiliary_route(
    task: String,
    provider: String,
    model: String,
) -> Result<AuxiliarySettingsDto, String> {
    let kind = parse_task(&task)?;
    let route = parse_route(provider, model)?;
    let base = home::default_memory_dir();
    memory::set_auxiliary_route(&base, kind, route).map_err(|e| e.to_string())?;
    build_settings_dto()
}

/// Tauri 命令：将单个辅助任务重置为 `auto`/`auto`。
#[tauri::command]
pub async fn reset_auxiliary_route(task: String) -> Result<AuxiliarySettingsDto, String> {
    let kind = parse_task(&task)?;
    let base = home::default_memory_dir();
    memory::set_auxiliary_route(&base, kind, memory::AuxiliaryRoute::default())
        .map_err(|e| e.to_string())?;
    build_settings_dto()
}

/// Tauri 命令：将全部五类辅助任务重置为 `auto`/`auto`。
#[tauri::command]
pub async fn reset_all_auxiliary_routes() -> Result<AuxiliarySettingsDto, String> {
    let base = home::default_memory_dir();
    memory::reset_all_auxiliary_routes(&base).map_err(|e| e.to_string())?;
    build_settings_dto()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dto(id: &str, display_name: &str, enabled: bool, has_api_key: bool) -> ProviderConfigDto {
        ProviderConfigDto {
            id: id.into(),
            kind: "openai".into(),
            display_name: display_name.into(),
            endpoint: "https://api.openai.com/v1".into(),
            model: "gpt-5.6".into(),
            enabled,
            has_api_key,
            key_source: if has_api_key {
                "keyring".into()
            } else {
                "none".into()
            },
            env_key_name: None,
            backend_id: "openai".into(),
            official_key_url: None,
            fallback: Vec::new(),
            image_model: String::new(),
            video_model: String::new(),
            tts_model: String::new(),
            vision_model: String::new(),
            music_model: String::new(),
            asr_model: String::new(),
            embedding_model: String::new(),
            supports_image: false,
            supports_video: false,
            supports_tts: false,
            supports_music: false,
            supports_asr: false,
            supports_embedding: false,
            api_mode: "chat_completions".into(),
            supports_responses_api: false,
            config_source: "builtin".into(),
        }
    }

    #[test]
    fn parse_task_covers_all_five_and_rejects_unknown() {
        for id in [
            "title_generation",
            "compaction",
            "smart_approval",
            "dreaming",
            "background_review",
        ] {
            assert!(parse_task(id).is_ok(), "{id} should parse");
        }
        assert!(parse_task("unknown").is_err());
        assert_eq!(parse_task("nope").unwrap_err(), "unknown auxiliary task");
    }

    #[test]
    fn parse_route_accepts_both_auto() {
        let route = parse_route("auto".into(), "auto".into()).unwrap();
        assert_eq!(route.provider, "auto");
        assert_eq!(route.model, "auto");
        // 大小写不敏感
        let route2 = parse_route("Auto".into(), "AUTO".into()).unwrap();
        assert_eq!(route2.provider, "Auto");
    }

    #[test]
    fn parse_route_accepts_both_explicit() {
        let route = parse_route("prov-1".into(), "gpt-mini".into()).unwrap();
        assert_eq!(route.provider, "prov-1");
        assert_eq!(route.model, "gpt-mini");
    }

    #[test]
    fn parse_route_rejects_half_auto_combinations() {
        assert!(parse_route("prov-1".into(), "auto".into()).is_err());
        assert!(parse_route("auto".into(), "gpt-mini".into()).is_err());
        assert!(parse_route("prov-1".into(), "".into()).is_err());
        assert!(parse_route("".into(), "gpt-mini".into()).is_err());
        assert!(parse_route("".into(), "".into()).is_err());
    }

    #[test]
    fn build_task_dto_auto_route_is_always_available() {
        let route = memory::AuxiliaryRoute::default();
        let task = build_task_dto(memory::AuxiliaryKind::Dreaming, &route, &[]);
        assert_eq!(task.id, "dreaming");
        assert_eq!(task.provider, "auto");
        assert!(!task.unavailable);
    }

    #[test]
    fn build_task_dto_explicit_route_marks_unavailable_when_disabled_or_no_key() {
        let providers = vec![
            dto("prov-ok", "OK Provider", true, true),
            dto("prov-disabled", "Disabled Provider", false, true),
            dto("prov-nokey", "No Key Provider", true, false),
        ];

        let ok_route = memory::AuxiliaryRoute {
            provider: "prov-ok".into(),
            model: "gpt-mini".into(),
        };
        let ok_task = build_task_dto(memory::AuxiliaryKind::Compaction, &ok_route, &providers);
        assert!(!ok_task.unavailable);
        assert_eq!(ok_task.display_label, "OK Provider · gpt-mini");

        let disabled_route = memory::AuxiliaryRoute {
            provider: "prov-disabled".into(),
            model: "gpt-mini".into(),
        };
        let disabled_task = build_task_dto(
            memory::AuxiliaryKind::Compaction,
            &disabled_route,
            &providers,
        );
        assert!(disabled_task.unavailable);

        let nokey_route = memory::AuxiliaryRoute {
            provider: "prov-nokey".into(),
            model: "gpt-mini".into(),
        };
        let nokey_task =
            build_task_dto(memory::AuxiliaryKind::Compaction, &nokey_route, &providers);
        assert!(nokey_task.unavailable);
    }

    #[test]
    fn build_task_dto_missing_provider_is_unavailable() {
        let route = memory::AuxiliaryRoute {
            provider: "prov-gone".into(),
            model: "m".into(),
        };
        let task = build_task_dto(memory::AuxiliaryKind::SmartApproval, &route, &[]);
        assert!(task.unavailable);
        assert_eq!(task.provider, "prov-gone");
    }
}
