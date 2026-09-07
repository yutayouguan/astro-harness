//! Side-effect-free validation and translation of chat settings.

use std::collections::HashMap;
use std::path::PathBuf;

pub(crate) fn parse_auxiliary_targets(
    items: Vec<proto::AuxiliaryModelTarget>,
) -> HashMap<types::AuxiliaryTask, Vec<types::ModelTarget>> {
    let mut grouped: HashMap<types::AuxiliaryTask, Vec<(u32, types::ModelTarget)>> = HashMap::new();
    for item in items {
        let Some(task) = types::AuxiliaryTask::parse(item.task.trim()) else {
            continue;
        };
        if !providers::dispatch::supports_agent_responses(&item.backend_id) {
            continue;
        }
        grouped.entry(task).or_default().push((
            item.order,
            types::ModelTarget {
                provider_id: item.provider_id,
                backend_id: item.backend_id,
                model: item.model,
                api_key: item.api_key,
                base_url: item.base_url,
            },
        ));
    }
    grouped
        .into_iter()
        .map(|(task, mut ordered)| {
            ordered.sort_by_key(|(order, _)| *order);
            (
                task,
                ordered.into_iter().map(|(_, target)| target).collect(),
            )
        })
        .collect()
}

pub(crate) fn from_chat_request(
    req: &proto::ChatRequest,
) -> Result<agent_protocol::ThreadSettingsOverrides, String> {
    let interaction_mode =
        types::InteractionMode::parse(&req.interaction_mode).ok_or_else(|| {
            format!(
                "unsupported interaction_mode: {}",
                req.interaction_mode.trim().to_ascii_lowercase()
            )
        })?;
    if let Some(temperature) = req.temperature {
        if !temperature.is_finite() || !(0.0..=2.0).contains(&temperature) {
            return Err("temperature must be between 0 and 2".into());
        }
    }
    let mut additional_params = if req.additional_params_json.trim().is_empty() {
        None
    } else {
        let params: serde_json::Value = serde_json::from_str(&req.additional_params_json)
            .map_err(|error| format!("invalid additional_params_json: {error}"))?;
        if !params.is_object() {
            return Err("additional_params_json must be an object".into());
        }
        Some(params)
    };

    let provider = if req.provider.trim().is_empty() {
        "openai"
    } else {
        req.provider.trim()
    };
    if !providers::dispatch::supports_agent_responses(provider) {
        return Err(format!(
            "provider `{provider}` does not support the Responses API"
        ));
    }
    let persistent_instructions = req.persistent_instructions.trim();
    if req.reasoning_effort.trim() == "persistent" {
        if provider != "openai" {
            return Err(format!(
                "provider `{provider}` does not support persistent reasoning"
            ));
        }
        if persistent_instructions.is_empty() {
            return Err("persistent reasoning requires persistent_instructions".into());
        }
    }
    if provider == "openai" && !persistent_instructions.is_empty() {
        additional_params
            .get_or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .expect("validated additional params object")
            .insert(
                "astro_persistent_instructions".into(),
                persistent_instructions.into(),
            );
    }
    let model = if req.model.trim().is_empty() {
        providers::dispatch::default_model(provider).to_string()
    } else {
        req.model.trim().to_string()
    };
    let api_key = if req.api_key.trim().is_empty() {
        providers::read_env_api_key(provider).unwrap_or_default()
    } else {
        req.api_key.trim().to_string()
    };
    let model_tool_mode = if req.tool_mode.trim().is_empty() {
        None
    } else {
        Some(
            types::ToolMode::parse(&req.tool_mode)
                .ok_or_else(|| format!("unsupported tool_mode: {}", req.tool_mode))?,
        )
    };
    let model_profile = if req.model_profile_json.trim().is_empty() {
        types::ModelProfile::default()
    } else {
        serde_json::from_str(&req.model_profile_json)
            .map_err(|error| format!("invalid model_profile_json: {error}"))?
    };
    let mut targets = vec![types::ModelTarget {
        provider_id: String::new(),
        backend_id: provider.into(),
        model: model.clone(),
        api_key,
        base_url: req.base_url.trim().to_string(),
    }];
    targets.extend(
        req.chat_fallbacks
            .iter()
            .filter(|fallback| providers::dispatch::supports_agent_responses(&fallback.provider))
            .map(|fallback| types::ModelTarget {
                provider_id: fallback.provider_id.clone(),
                backend_id: fallback.provider.clone(),
                model: fallback.model.clone(),
                api_key: fallback.api_key.clone(),
                base_url: fallback.base_url.clone(),
            }),
    );

    Ok(agent_protocol::ThreadSettingsOverrides {
        model_targets: Some(targets),
        model_spec: Some(types::ModelSpec {
            provider_id: provider.into(),
            model_id: model,
            temperature: req.temperature,
            max_tokens: (req.max_output_tokens > 0).then_some(req.max_output_tokens),
            tool_mode: model_tool_mode,
            profile: model_profile,
        }),
        auxiliary_targets: Some(parse_auxiliary_targets(req.auxiliary_targets.clone())),
        image_gen_targets: Some(tools::image_gen_targets_from_parts(tools::ImageGenParts {
            provider: &req.image_gen_provider,
            model: &req.image_gen_model,
            api_key: &req.image_gen_api_key,
            base_url: &req.image_gen_base_url,
            fb_provider: &req.image_gen_fallback_provider,
            fb_model: &req.image_gen_fallback_model,
            fb_api_key: &req.image_gen_fallback_api_key,
            fb_base_url: &req.image_gen_fallback_base_url,
            video_model: &req.image_gen_video_model,
            music_model: &req.image_gen_music_model,
            tts_model: &req.image_gen_tts_model,
            fb_video_model: &req.image_gen_fallback_video_model,
            fb_music_model: &req.image_gen_fallback_music_model,
            fb_tts_model: &req.image_gen_fallback_tts_model,
            vision_model: &req.image_gen_vision_model,
            fb_vision_model: &req.image_gen_fallback_vision_model,
        })),
        context_window: (req.context_window > 0).then_some(req.context_window),
        interaction_mode: Some(interaction_mode),
        project_root: Some(
            (!req.project_root.trim().is_empty()).then(|| PathBuf::from(req.project_root.trim())),
        ),
        workspace_roots: Some(
            req.workspace_roots
                .iter()
                .map(|root| PathBuf::from(root.trim()))
                .filter(|root| !root.as_os_str().is_empty())
                .collect(),
        ),
        temperature: req.temperature,
        additional_params,
        thinking_enabled: Some(req.thinking_enabled),
        reasoning_effort: Some(if req.reasoning_effort.trim().is_empty() {
            "high".into()
        } else {
            req.reasoning_effort.trim().into()
        }),
        max_tokens: Some(if req.max_output_tokens > 0 {
            req.max_output_tokens
        } else {
            8192
        }),
    })
}
