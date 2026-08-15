//! 辅助模型运行时目标解析：把持久化路由（`auto` / 显式 UI Provider ID）展开为
//! preferred + 可选 fallback 的具体调用凭据，供 `start_chat` 透传给 backend。
//!
//! `auto`：preferred 就是当前会话主模型（`primary`），fallback 为 `None`。
//! 显式路由的 `provider` 字段保存 **UI Provider ID**（`providers.json` 条目 `id`，
//! 与 `backend_id`/registry kind 不同）；查不到、已禁用或无可用凭据时静默退回
//! primary（不阻塞主聊天）。有效且与 primary 目标不完全相同时，追加
//! `fallback = Some(primary)`（含同 Provider 不同 model）。

use memory::{AuxiliaryConfig, AuxiliaryKind, AuxiliaryRoute};

use crate::commands::providers::{self, resolve_api_key, ProviderConfig as UiProvider};

/// 单个已解析目标：命中的 UI Provider 条目 + 实际调用用的后端凭据。
#[derive(Debug, Clone)]
pub struct ResolvedTarget {
    pub provider: UiProvider,
    pub backend_id: String,
    pub model: String,
    pub api_key: String,
}

impl ResolvedTarget {
    fn to_chat_target(&self) -> types::ChatTarget {
        types::ChatTarget {
            provider_id: self.provider.id.clone(),
            backend_id: self.backend_id.clone(),
            model: self.model.clone(),
            api_key: self.api_key.clone(),
            base_url: self.provider.endpoint.clone(),
        }
    }
}

/// 某辅助任务的解析结果：`preferred` 必有；`fallback` 在显式路由与 primary
/// 不完全相同时出现（值恒为 primary，供调用失败时兜底重试）。
#[derive(Debug, Clone)]
pub struct AuxiliaryTargets {
    pub preferred: ResolvedTarget,
    pub fallback: Option<ResolvedTarget>,
}

impl AuxiliaryTargets {
    /// 展开为跨进程 `ChatTarget` 链（最多 2 项：preferred + fallback）。
    pub fn to_chat_targets(&self) -> Vec<types::ChatTarget> {
        let mut out = vec![self.preferred.to_chat_target()];
        if let Some(fb) = &self.fallback {
            out.push(fb.to_chat_target());
        }
        out
    }
}

fn is_auto_route(route: &AuxiliaryRoute) -> bool {
    let provider = route.provider.trim();
    provider.is_empty() || provider.eq_ignore_ascii_case("auto")
}

fn resolved_from_ui_provider(
    provider: UiProvider,
    model: String,
    api_key: String,
) -> ResolvedTarget {
    let backend_id = provider.kind.backend_id().to_string();
    ResolvedTarget {
        provider,
        backend_id,
        model,
        api_key,
    }
}

/// 核心解析逻辑：Provider 查找与 API key 解析通过闭包注入，供单测脱离全局
/// Provider 状态（`providers::STATE`）独立验证。
fn resolve_with<F, K>(
    kind: AuxiliaryKind,
    aux: &AuxiliaryConfig,
    primary: &types::ChatTarget,
    mut find_provider: F,
    mut resolve_key: K,
) -> Result<AuxiliaryTargets, String>
where
    F: FnMut(&str) -> Option<UiProvider>,
    K: FnMut(&UiProvider) -> (bool, String),
{
    let primary_provider = find_provider(&primary.provider_id)
        .ok_or_else(|| format!("primary provider not found: {}", primary.provider_id))?;
    let primary_resolved = resolved_from_ui_provider(
        primary_provider,
        primary.model.clone(),
        primary.api_key.clone(),
    );

    let route = aux.route(kind);
    if is_auto_route(route) {
        return Ok(AuxiliaryTargets {
            preferred: primary_resolved,
            fallback: None,
        });
    }

    let explicit = find_provider(route.provider.trim()).filter(|p| p.enabled);
    let Some(p) = explicit else {
        // 显式供应商已被删除或禁用：静默退回主模型，不阻塞该辅助任务。
        return Ok(AuxiliaryTargets {
            preferred: primary_resolved,
            fallback: None,
        });
    };

    let (has_key, api_key) = resolve_key(&p);
    let allow_empty_key = p.kind.backend_id() == "ollama";
    if !has_key && !allow_empty_key {
        return Ok(AuxiliaryTargets {
            preferred: primary_resolved,
            fallback: None,
        });
    }

    let model = {
        let m = route.model.trim();
        if m.is_empty() || m.eq_ignore_ascii_case("auto") {
            p.model.clone()
        } else {
            m.to_string()
        }
    };
    let explicit_id = p.id.clone();
    let explicit_resolved = resolved_from_ui_provider(p, model, api_key);

    // 同 Provider 但不同 model 时也挂 primary 作一次重试；完全相同则无需 fallback。
    let same_target = explicit_id == primary_resolved.provider.id
        && explicit_resolved.model == primary_resolved.model;
    let fallback = if same_target {
        None
    } else {
        Some(primary_resolved)
    };

    Ok(AuxiliaryTargets {
        preferred: explicit_resolved,
        fallback,
    })
}

fn resolve_for_config(
    kind: AuxiliaryKind,
    aux: &AuxiliaryConfig,
    primary: &types::ChatTarget,
) -> Result<AuxiliaryTargets, String> {
    resolve_with(
        kind,
        aux,
        primary,
        |id| providers::find_provider(id).ok(),
        |p| {
            let (has, _source, _env, key) = resolve_api_key(p);
            (has, key.unwrap_or_default())
        },
    )
}

/// 将某辅助任务路由解析为 preferred/fallback（`kind` + 当前会话 `primary`）。
///
/// 从磁盘读取 `auxiliary.*` 配置；批量解析五类任务时优先使用
/// [`build_auxiliary_model_targets`]，避免重复读盘。
pub fn resolve_auxiliary_targets(
    kind: AuxiliaryKind,
    primary: &types::ChatTarget,
) -> Result<AuxiliaryTargets, String> {
    let aux = memory::load_auxiliary_config(&home::default_memory_dir());
    resolve_for_config(kind, &aux, primary)
}

/// 将单条路由（`auto` / 显式 UI Provider ID）解析为 preferred/fallback。
///
/// 与辅助任务同规则：`auto`/查不到/禁用/无凭据 → 退回 `primary`；显式且有效时
/// preferred=显式、fallback=primary（同 provider 不同 model 也挂 fallback）。
/// 供离线进化 `reflection` / `judge` 路由复用。
fn resolve_route_for(
    route: &AuxiliaryRoute,
    primary: &types::ChatTarget,
) -> Result<AuxiliaryTargets, String> {
    let primary_provider = providers::find_provider(&primary.provider_id)
        .ok()
        .ok_or_else(|| format!("primary provider not found: {}", primary.provider_id))?;
    let primary_resolved = resolved_from_ui_provider(
        primary_provider,
        primary.model.clone(),
        primary.api_key.clone(),
    );

    if is_auto_route(route) {
        return Ok(AuxiliaryTargets {
            preferred: primary_resolved,
            fallback: None,
        });
    }

    let explicit = providers::find_provider(route.provider.trim())
        .ok()
        .filter(|p| p.enabled);
    let Some(p) = explicit else {
        return Ok(AuxiliaryTargets {
            preferred: primary_resolved,
            fallback: None,
        });
    };

    let (has_key, _src, _env, key) = resolve_api_key(&p);
    let api_key = key.unwrap_or_default();
    let allow_empty_key = p.kind.backend_id() == "ollama";
    if !has_key && !allow_empty_key {
        return Ok(AuxiliaryTargets {
            preferred: primary_resolved,
            fallback: None,
        });
    }

    let model = {
        let m = route.model.trim();
        if m.is_empty() || m.eq_ignore_ascii_case("auto") {
            p.model.clone()
        } else {
            m.to_string()
        }
    };
    let explicit_id = p.id.clone();
    let explicit_resolved = resolved_from_ui_provider(p, model, api_key);
    let same_target = explicit_id == primary_resolved.provider.id
        && explicit_resolved.model == primary_resolved.model;
    let fallback = if same_target {
        None
    } else {
        Some(primary_resolved)
    };
    Ok(AuxiliaryTargets {
        preferred: explicit_resolved,
        fallback,
    })
}

/// 将离线进化路由（`reflection` / `judge`）解析为 preferred/fallback。
pub fn resolve_evolution_targets(
    kind: memory::EvolutionRouteKind,
    primary: &types::ChatTarget,
) -> Result<AuxiliaryTargets, String> {
    let cfg = memory::load_evolution_config(&home::default_memory_dir());
    let route = cfg.route(kind).clone();
    resolve_route_for(&route, primary)
}

fn to_common_task(kind: AuxiliaryKind) -> types::AuxiliaryTask {
    match kind {
        AuxiliaryKind::TitleGeneration => types::AuxiliaryTask::TitleGeneration,
        AuxiliaryKind::Compaction => types::AuxiliaryTask::Compaction,
        AuxiliaryKind::SmartApproval => types::AuxiliaryTask::SmartApproval,
        AuxiliaryKind::Dreaming => types::AuxiliaryTask::Dreaming,
        AuxiliaryKind::BackgroundReview => types::AuxiliaryTask::BackgroundReview,
        AuxiliaryKind::WorkflowAiPolish => types::AuxiliaryTask::WorkflowAiPolish,
    }
}

/// 解析会话侧辅助任务用的 primary：优先匹配会话账单里的 backend/endpoint/model，
/// 否则回退到 UI 当前激活提供商。
pub fn primary_chat_target_for_session(session_id: &str) -> Result<types::ChatTarget, String> {
    let root = home::default_memory_dir();
    memory::ensure_workspace(&root).map_err(|e| e.to_string())?;
    let store = session::SessionStore::open_sessions_dir(&root.join("sessions"))
        .map_err(|e| e.to_string())?;
    let billing = store
        .get_session_billing(session_id)
        .map_err(|e| e.to_string())?;
    let session_model = billing
        .as_ref()
        .and_then(|b| b.model.as_ref())
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty());
    let billing_provider = billing
        .as_ref()
        .and_then(|b| b.billing_provider.as_ref())
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty());
    let billing_base_url = billing
        .as_ref()
        .and_then(|b| b.billing_base_url.as_ref())
        .map(|m| m.trim().trim_end_matches('/').to_string())
        .filter(|m| !m.is_empty());

    let state = providers::get_providers_state()?;
    let matched_id = billing_provider.as_ref().and_then(|bp| {
        let candidates: Vec<&providers::ProviderConfigDto> = state
            .providers
            .iter()
            .filter(|p| p.enabled && p.backend_id == *bp)
            .collect();
        if let Some(url) = billing_base_url.as_ref() {
            if let Some(p) = candidates
                .iter()
                .find(|p| p.endpoint.trim().trim_end_matches('/') == url.as_str())
            {
                return Some(p.id.clone());
            }
        }
        candidates.first().map(|p| p.id.clone())
    });

    let provider_id = matched_id
        .or_else(|| state.active_provider_id.clone())
        .or_else(|| {
            state
                .providers
                .iter()
                .find(|p| p.enabled)
                .map(|p| p.id.clone())
        })
        .ok_or_else(|| "请先在「模型提供商」中配置并启用至少一个提供商".to_string())?;

    let ui = providers::find_provider(&provider_id)?;
    let model = session_model.unwrap_or_else(|| ui.model.clone());
    if model.trim().is_empty() {
        return Err("会话/提供商未配置模型".into());
    }
    let (_has, _src, _env, key) = resolve_api_key(&ui);
    Ok(types::ChatTarget {
        provider_id: ui.id,
        backend_id: ui.kind.backend_id().to_string(),
        model,
        api_key: key.unwrap_or_default(),
        base_url: ui.endpoint,
    })
}

/// 为全部五类辅助任务解析目标并转换为 proto 透传结构，供 `start_chat` 下发。
///
/// 单个任务解析失败（如 primary 的 UI Provider 已被删除）时跳过该任务，不阻塞
/// 主聊天；跳过的任务在 backend 侧 `AgentLoop::auxiliary_targets` 中回退主模型。
pub fn build_auxiliary_model_targets(
    primary: &types::ChatTarget,
) -> Vec<proto::AuxiliaryModelTarget> {
    let aux = memory::load_auxiliary_config(&home::default_memory_dir());
    let mut out = Vec::new();
    for kind in AuxiliaryKind::ALL {
        let Ok(resolved) = resolve_for_config(kind, &aux, primary) else {
            continue;
        };
        let task = to_common_task(kind).as_str().to_string();
        for (order, target) in resolved.to_chat_targets().into_iter().enumerate() {
            out.push(proto::AuxiliaryModelTarget {
                task: task.clone(),
                provider_id: target.provider_id,
                backend_id: target.backend_id,
                model: target.model,
                api_key: target.api_key,
                base_url: target.base_url,
                order: order as u32,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui_provider(
        id: &str,
        kind: providers::ProviderKind,
        model: &str,
        enabled: bool,
    ) -> UiProvider {
        UiProvider {
            id: id.into(),
            kind,
            display_name: format!("{id}-display"),
            endpoint: format!("https://{id}.example"),
            model: model.into(),
            enabled,
            fallback: Vec::new(),
            image_model: String::new(),
            video_model: String::new(),
            tts_model: String::new(),
            vision_model: String::new(),
            music_model: String::new(),
            api_mode: String::new(),
        }
    }

    fn primary_target() -> types::ChatTarget {
        types::ChatTarget {
            provider_id: "prov-primary".into(),
            backend_id: "openai".into(),
            model: "gpt-5.6".into(),
            api_key: "primary-key".into(),
            base_url: "https://prov-primary.example".into(),
        }
    }

    fn lookup(catalog: Vec<UiProvider>) -> impl FnMut(&str) -> Option<UiProvider> {
        move |id: &str| catalog.iter().find(|p| p.id == id).cloned()
    }

    fn always_has_key(key: &str) -> impl FnMut(&UiProvider) -> (bool, String) {
        let key = key.to_string();
        move |_p: &UiProvider| (true, key.clone())
    }

    #[test]
    fn auto_route_resolves_to_primary_with_no_fallback() {
        let aux = AuxiliaryConfig::default();
        let primary_ui = ui_provider(
            "prov-primary",
            providers::ProviderKind::Openai,
            "gpt-5.6",
            true,
        );
        let primary = primary_target();
        let result = resolve_with(
            AuxiliaryKind::Dreaming,
            &aux,
            &primary,
            lookup(vec![primary_ui]),
            always_has_key("primary-key"),
        )
        .unwrap();

        assert_eq!(result.preferred.provider.id, "prov-primary");
        assert_eq!(result.preferred.model, "gpt-5.6");
        assert!(result.fallback.is_none());
    }

    #[test]
    fn explicit_route_to_different_provider_adds_primary_as_fallback() {
        let aux = AuxiliaryConfig {
            compaction: AuxiliaryRoute {
                provider: "prov-cheap".into(),
                model: "gpt-mini".into(),
            },
            ..Default::default()
        };
        let primary_ui = ui_provider(
            "prov-primary",
            providers::ProviderKind::Openai,
            "gpt-5.6",
            true,
        );
        let cheap_ui = ui_provider(
            "prov-cheap",
            providers::ProviderKind::Deepseek,
            "deepseek-chat",
            true,
        );
        let primary = primary_target();
        let result = resolve_with(
            AuxiliaryKind::Compaction,
            &aux,
            &primary,
            lookup(vec![primary_ui, cheap_ui]),
            always_has_key("cheap-key"),
        )
        .unwrap();

        assert_eq!(result.preferred.provider.id, "prov-cheap");
        assert_eq!(result.preferred.model, "gpt-mini");
        assert_eq!(result.preferred.backend_id, "deepseek");
        let fallback = result.fallback.expect("expected primary fallback");
        assert_eq!(fallback.provider.id, "prov-primary");
    }

    #[test]
    fn explicit_route_to_same_provider_different_model_falls_back() {
        let aux = AuxiliaryConfig {
            smart_approval: AuxiliaryRoute {
                provider: "prov-primary".into(),
                model: "gpt-mini".into(),
            },
            ..Default::default()
        };
        let primary_ui = ui_provider(
            "prov-primary",
            providers::ProviderKind::Openai,
            "gpt-5.6",
            true,
        );
        let primary = primary_target();
        let result = resolve_with(
            AuxiliaryKind::SmartApproval,
            &aux,
            &primary,
            lookup(vec![primary_ui]),
            always_has_key("primary-key"),
        )
        .unwrap();

        assert_eq!(result.preferred.provider.id, "prov-primary");
        assert_eq!(result.preferred.model, "gpt-mini");
        let fallback = result
            .fallback
            .expect("same provider different model needs fallback");
        assert_eq!(fallback.model, "gpt-5.6");
    }

    #[test]
    fn explicit_route_identical_to_primary_has_no_fallback() {
        let aux = AuxiliaryConfig {
            smart_approval: AuxiliaryRoute {
                provider: "prov-primary".into(),
                model: "gpt-5.6".into(),
            },
            ..Default::default()
        };
        let primary_ui = ui_provider(
            "prov-primary",
            providers::ProviderKind::Openai,
            "gpt-5.6",
            true,
        );
        let primary = primary_target();
        let result = resolve_with(
            AuxiliaryKind::SmartApproval,
            &aux,
            &primary,
            lookup(vec![primary_ui]),
            always_has_key("primary-key"),
        )
        .unwrap();

        assert_eq!(result.preferred.model, "gpt-5.6");
        assert!(result.fallback.is_none());
    }

    #[test]
    fn explicit_route_falls_back_to_primary_when_provider_missing() {
        let aux = AuxiliaryConfig {
            dreaming: AuxiliaryRoute {
                provider: "prov-deleted".into(),
                model: "m".into(),
            },
            ..Default::default()
        };
        let primary_ui = ui_provider(
            "prov-primary",
            providers::ProviderKind::Openai,
            "gpt-5.6",
            true,
        );
        let primary = primary_target();
        let result = resolve_with(
            AuxiliaryKind::Dreaming,
            &aux,
            &primary,
            lookup(vec![primary_ui]),
            always_has_key("k"),
        )
        .unwrap();

        assert_eq!(result.preferred.provider.id, "prov-primary");
        assert!(result.fallback.is_none());
    }

    #[test]
    fn explicit_route_falls_back_to_primary_when_provider_disabled() {
        let aux = AuxiliaryConfig {
            title_generation: AuxiliaryRoute {
                provider: "prov-disabled".into(),
                model: "m".into(),
            },
            ..Default::default()
        };
        let primary_ui = ui_provider(
            "prov-primary",
            providers::ProviderKind::Openai,
            "gpt-5.6",
            true,
        );
        let disabled_ui = ui_provider(
            "prov-disabled",
            providers::ProviderKind::Anthropic,
            "claude",
            false,
        );
        let primary = primary_target();
        let result = resolve_with(
            AuxiliaryKind::TitleGeneration,
            &aux,
            &primary,
            lookup(vec![primary_ui, disabled_ui]),
            always_has_key("k"),
        )
        .unwrap();

        assert_eq!(result.preferred.provider.id, "prov-primary");
        assert!(result.fallback.is_none());
    }

    #[test]
    fn explicit_route_falls_back_to_primary_when_no_api_key() {
        let aux = AuxiliaryConfig {
            background_review: AuxiliaryRoute {
                provider: "prov-nokey".into(),
                model: "m".into(),
            },
            ..Default::default()
        };
        let primary_ui = ui_provider(
            "prov-primary",
            providers::ProviderKind::Openai,
            "gpt-5.6",
            true,
        );
        let nokey_ui = ui_provider(
            "prov-nokey",
            providers::ProviderKind::Anthropic,
            "claude",
            true,
        );
        let primary = primary_target();
        let result = resolve_with(
            AuxiliaryKind::BackgroundReview,
            &aux,
            &primary,
            lookup(vec![primary_ui, nokey_ui]),
            |_p: &UiProvider| (false, String::new()),
        )
        .unwrap();

        assert_eq!(result.preferred.provider.id, "prov-primary");
        assert!(result.fallback.is_none());
    }

    #[test]
    fn explicit_route_allows_empty_key_for_ollama() {
        let aux = AuxiliaryConfig {
            compaction: AuxiliaryRoute {
                provider: "prov-ollama".into(),
                model: "llama".into(),
            },
            ..Default::default()
        };
        let primary_ui = ui_provider(
            "prov-primary",
            providers::ProviderKind::Openai,
            "gpt-5.6",
            true,
        );
        let ollama_ui = ui_provider(
            "prov-ollama",
            providers::ProviderKind::Ollama,
            "llama",
            true,
        );
        let primary = primary_target();
        let result = resolve_with(
            AuxiliaryKind::Compaction,
            &aux,
            &primary,
            lookup(vec![primary_ui, ollama_ui]),
            |_p: &UiProvider| (false, String::new()),
        )
        .unwrap();

        assert_eq!(result.preferred.provider.id, "prov-ollama");
        assert_eq!(result.preferred.backend_id, "ollama");
    }

    #[test]
    fn to_chat_targets_includes_fallback_when_present() {
        let aux = AuxiliaryConfig {
            dreaming: AuxiliaryRoute {
                provider: "prov-cheap".into(),
                model: "m".into(),
            },
            ..Default::default()
        };
        let primary_ui = ui_provider(
            "prov-primary",
            providers::ProviderKind::Openai,
            "gpt-5.6",
            true,
        );
        let cheap_ui = ui_provider(
            "prov-cheap",
            providers::ProviderKind::Deepseek,
            "deepseek-chat",
            true,
        );
        let primary = primary_target();
        let result = resolve_with(
            AuxiliaryKind::Dreaming,
            &aux,
            &primary,
            lookup(vec![primary_ui, cheap_ui]),
            always_has_key("k"),
        )
        .unwrap();
        let chain = result.to_chat_targets();
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].provider_id, "prov-cheap");
        assert_eq!(chain[1].provider_id, "prov-primary");
    }
}
