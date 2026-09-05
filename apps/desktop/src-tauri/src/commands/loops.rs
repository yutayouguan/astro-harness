use serde::{Deserialize, Serialize};
use workflow::engine::WorkflowRunResult;
use workflow::model::{
    NewWorkflow, NodeType, Position, Workflow, WorkflowAgentTool, WorkflowAgentToolPatch,
    WorkflowEdge, WorkflowNode,
};
use workflow::run_db::{WorkflowRunDb, WorkflowRunRow, WorkflowStepLogRow};
use workflow::store::WorkflowStore;

fn workflow_provider_configs(
) -> Result<std::collections::HashMap<String, workflow::engine::RuntimeProviderConfig>, String> {
    use super::providers::{find_provider, get_providers_state, resolve_api_key};

    let state = get_providers_state()?;
    let mut runtime = std::collections::HashMap::new();
    for provider in state
        .providers
        .into_iter()
        .filter(|provider| provider.enabled)
    {
        let stored = find_provider(&provider.id)?;
        let (has_key, _, _, api_key) = resolve_api_key(&stored);
        if stored.kind.requires_api_key() && !has_key {
            continue;
        }
        let resolved = workflow::engine::RuntimeProviderConfig {
            backend_id: provider.backend_id.clone(),
            config: providers::ProviderConfig {
                api_key: api_key.unwrap_or_default(),
                base_url: (!stored.endpoint.trim().is_empty()).then_some(stored.endpoint.clone()),
                model: stored.model.clone(),
                api_mode: String::new(),
                ..providers::ProviderConfig::default()
            },
            image_model: provider.image_model.clone(),
            video_model: provider.video_model.clone(),
            tts_model: provider.tts_model.clone(),
            music_model: provider.music_model.clone(),
        };
        runtime.insert(provider.backend_id, resolved.clone());
        runtime.insert(provider.id, resolved);
    }
    Ok(runtime)
}

// ── DTO ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopDto {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    #[serde(default)]
    pub agent_tool: WorkflowAgentTool,
    pub nodes: Vec<LoopNodeDto>,
    pub edges: Vec<LoopEdgeDto>,
    pub variables: std::collections::HashMap<String, serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopNodeDto {
    pub id: String,
    pub node_type: NodeType,
    pub label: String,
    pub position: Position,
    pub config: serde_json::Value,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopEdgeDto {
    pub id: String,
    pub source: String,
    pub source_handle: Option<String>,
    pub target: String,
    pub target_handle: Option<String>,
}

fn to_dto(wf: Workflow) -> LoopDto {
    LoopDto {
        id: wf.id,
        name: wf.name,
        description: wf.description,
        enabled: wf.enabled,
        agent_tool: wf.agent_tool,
        nodes: wf
            .nodes
            .into_iter()
            .map(|n| LoopNodeDto {
                id: n.id,
                node_type: n.node_type,
                label: n.label,
                position: n.position,
                config: n.config,
                disabled: n.disabled,
            })
            .collect(),
        edges: wf
            .edges
            .into_iter()
            .map(|e| LoopEdgeDto {
                id: e.id,
                source: e.source,
                source_handle: e.source_handle,
                target: e.target,
                target_handle: e.target_handle,
            })
            .collect(),
        variables: wf.variables,
        created_at: wf.created_at,
        updated_at: wf.updated_at,
        icon: wf.icon,
    }
}

fn from_dto(dto: LoopDto) -> Workflow {
    Workflow {
        id: dto.id,
        name: dto.name,
        description: dto.description,
        enabled: dto.enabled,
        agent_tool: dto.agent_tool,
        nodes: dto
            .nodes
            .into_iter()
            .map(|n| WorkflowNode {
                id: n.id,
                node_type: n.node_type,
                label: n.label,
                position: n.position,
                config: n.config,
                disabled: n.disabled,
            })
            .collect(),
        edges: dto
            .edges
            .into_iter()
            .map(|e| WorkflowEdge {
                id: e.id,
                source: e.source,
                source_handle: e.source_handle,
                target: e.target,
                target_handle: e.target_handle,
            })
            .collect(),
        variables: dto.variables,
        created_at: dto.created_at,
        updated_at: dto.updated_at,
        icon: dto.icon,
    }
}

// ── Commands ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_loops() -> Result<Vec<LoopDto>, String> {
    let store = WorkflowStore::open_default().map_err(|e| e.to_string())?;
    let list = store.list().map_err(|e| e.to_string())?;
    Ok(list.into_iter().map(to_dto).collect())
}

#[tauri::command]
pub async fn get_loop(id: String) -> Result<Option<LoopDto>, String> {
    let store = WorkflowStore::open_default().map_err(|e| e.to_string())?;
    let wf = store.get(&id).map_err(|e| e.to_string())?;
    Ok(wf.map(to_dto))
}

#[tauri::command]
pub async fn create_loop(name: String, description: String) -> Result<LoopDto, String> {
    let store = WorkflowStore::open_default().map_err(|e| e.to_string())?;
    let wf = store
        .create(NewWorkflow { name, description })
        .map_err(|e| e.to_string())?;
    Ok(to_dto(wf))
}

#[tauri::command]
pub async fn save_loop(data: LoopDto) -> Result<LoopDto, String> {
    let store = WorkflowStore::open_default().map_err(|e| e.to_string())?;
    let wf = store
        .save_workflow(from_dto(data))
        .map_err(|e| e.to_string())?;
    Ok(to_dto(wf))
}

#[tauri::command]
pub async fn delete_loop(id: String) -> Result<bool, String> {
    let store = WorkflowStore::open_default().map_err(|e| e.to_string())?;
    store.delete(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_loop_enabled(id: String, enabled: bool) -> Result<bool, String> {
    let store = WorkflowStore::open_default().map_err(|e| e.to_string())?;
    store.set_enabled(&id, enabled).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_loop_agent_tool(
    id: String,
    patch: WorkflowAgentToolPatch,
) -> Result<bool, String> {
    let store = WorkflowStore::open_default().map_err(|e| e.to_string())?;
    store
        .update_agent_tool(&id, patch)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn run_loop(id: String) -> Result<WorkflowRunResult, String> {
    let store = WorkflowStore::open_default().map_err(|e| e.to_string())?;
    let wf = store
        .get(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("workflow {} 不存在", id))?;
    let provider_configs = workflow_provider_configs()?;

    // WorkflowRunDb 在 spawn_blocking + current_thread runtime 中执行
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(async {
            let run_db = WorkflowRunDb::open_default()
                .await
                .map_err(|e| e.to_string())?;
            workflow::engine::execute_workflow_with_provider_configs(
                &wf,
                serde_json::json!({}),
                "manual",
                &run_db,
                provider_configs,
            )
            .await
            .map_err(|e| e.to_string())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_loop_runs(
    workflow_id: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<WorkflowRunRow>, String> {
    let db = WorkflowRunDb::open_default()
        .await
        .map_err(|e| e.to_string())?;
    db.list_runs(workflow_id.as_deref(), limit.unwrap_or(100))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_loop_run(run_id: String) -> Result<Option<WorkflowRunRow>, String> {
    let db = WorkflowRunDb::open_default()
        .await
        .map_err(|e| e.to_string())?;
    db.get_run(&run_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_loop_run(run_id: String) -> Result<bool, String> {
    let db = WorkflowRunDb::open_default()
        .await
        .map_err(|e| e.to_string())?;
    db.delete_run(&run_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_loop_step_logs(run_id: String) -> Result<Vec<WorkflowStepLogRow>, String> {
    let db = WorkflowRunDb::open_default()
        .await
        .map_err(|e| e.to_string())?;
    db.list_step_logs(&run_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn export_loop(id: String) -> Result<String, String> {
    let store = WorkflowStore::open_default().map_err(|e| e.to_string())?;
    let wf = store
        .get(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("workflow {} 不存在", id))?;
    serde_json::to_string_pretty(&to_dto(wf)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn export_loop_svg(path: String, content: String) -> Result<String, String> {
    let p = std::path::Path::new(&path);
    let final_path = if p.is_absolute() {
        p.to_path_buf()
    } else {
        // 非绝对路径 → 保存到桌面
        let desktop = home::user_home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
            .join("Desktop");
        std::fs::create_dir_all(&desktop).ok();
        desktop.join(&path)
    };
    if let Some(parent) = final_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&final_path, &content).map_err(|e| e.to_string())?;
    Ok(final_path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn open_loop_export(path: String) -> Result<(), String> {
    let desktop = home::user_home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("Desktop");
    let canonical_desktop = desktop.canonicalize().map_err(|e| e.to_string())?;
    let canonical_path = std::path::PathBuf::from(path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !canonical_path.starts_with(&canonical_desktop)
        || canonical_path.extension().and_then(|ext| ext.to_str()) != Some("svg")
    {
        return Err("只能打开桌面目录中的 SVG 导出文件".into());
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&canonical_path)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(&canonical_path)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&canonical_path)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        let _ = canonical_path;
        Err("当前平台不支持用系统应用打开文件".into())
    }
}

#[tauri::command]
pub async fn import_loop(json: String) -> Result<LoopDto, String> {
    let dto: LoopDto = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    let store = WorkflowStore::open_default().map_err(|e| e.to_string())?;
    let mut wf = from_dto(dto);
    wf.id = uuid::Uuid::new_v4().to_string();
    let saved = store.save_workflow(wf).map_err(|e| e.to_string())?;
    Ok(to_dto(saved))
}

// ── AI 工作流生成 ────────────────────────────────────────────────

/// AI 生成的工作流结构
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AiGeneratedWorkflow {
    pub explanation: String,
    pub nodes: Vec<AiGenNode>,
    pub edges: Vec<AiGenEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AiGenNode {
    pub id: String,
    pub node_type: String,
    pub label: String,
    pub config: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AiGenEdge {
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub source_handle: Option<String>,
}

const AI_WORKFLOW_SYSTEM_PROMPT: &str = r#"你是一个工作流设计专家。用户会描述需求，你需要生成一个工作流的节点和连线。

可用的节点类型（node_type）：
【触发器】manual_trigger（手动触发）、scheduled_trigger（定时触发，config: {schedule, timezone}）、webhook_trigger（Webhook 触发，config: {path, method}）
【AI】ai_agent_task（AI 智能体任务，config: {prompt_template, provider_id?, model?, reasoning_level?}）、parameter_extraction（参数提取，config: {prompt_template, output_schema}）、question_classification（问题分类，config: {classes: [{id, label, description}]}）
【多媒体】image_generation（生成图片，config: {prompt_template, size?, style?}）、video_generation（生成视频，config: {prompt_template, duration_seconds?}）、music_generation（生成音乐，config: {prompt_template, duration_seconds?, instrumental?}）、text_to_speech（文字转语音，config: {text_template, voice?, speed?}）、subtitle_generation（字幕生成，config: {audio_source}）
【流程控制】conditional（条件判断，config: {conditions: [{id, label, expression}]}）、multi_branch（多路分支）、filter（过滤，config: {condition}）、merge（合并）、loop（循环，config: {max_iterations, break_condition?}）、human_approval（人工审批，config: {prompt_template}）
【数据处理】set_fields（设置字段，config: {assignments: [{field, value}]}）、format_text（格式化文本，config: {template}）、json（JSON 处理，config: {mode, expression}）、code（代码，config: {language, source}）、sort（排序）、slice（截取）、aggregate（聚合）
【动作】http_request（HTTP 请求，config: {method, url_template, headers?, body_template?}）、run_loop（运行子 Loop）、delay_wait（延时等待，config: {seconds}）、output（输出，config: {fields?}）

规则：
1. 每个工作流必须以一个触发器节点开始
2. 通常以 output 节点结束
3. 节点 id 用简短的英文标识如 "trigger1", "ai1", "filter1"
4. config 中的模板字段支持 {{node_id.field}} 引用上游输出
5. 生成 explanation 简要说明设计思路
6. edges 的 source 和 target 必须是已有节点的 id"#;

#[tauri::command]
pub async fn ai_generate_workflow(
    prompt: String,
    current_nodes: Option<Vec<AiGenNode>>,
    provider_id: Option<String>,
    model: Option<String>,
) -> Result<AiGeneratedWorkflow, String> {
    use super::providers::{find_provider, resolve_api_key};
    if prompt.trim().is_empty() {
        return Err("请描述你想要创建的工作流".into());
    }

    let provider_cfg = if let Some(id) = provider_id.as_deref().filter(|s| !s.is_empty()) {
        find_provider(id)?
    } else {
        let state = super::providers::get_providers_state()?;
        let id = state
            .active_provider_id
            .or_else(|| state.providers.first().map(|p| p.id.clone()))
            .ok_or_else(|| "请先在「模型服务」中配置至少一个供应商".to_string())?;
        find_provider(&id)?
    };

    let (has, _, _, key) = resolve_api_key(&provider_cfg);
    if provider_cfg.kind.requires_api_key() && !has {
        return Err(format!(
            "未配置 API Key。请在「模型服务」中为 {} 保存密钥。",
            provider_cfg.display_name
        ));
    }
    let api_key = key.unwrap_or_default();
    let model_name = model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(provider_cfg.model.as_str())
        .to_string();
    if model_name.trim().is_empty() {
        return Err("当前供应商未设置默认模型".into());
    }

    let base_url = if provider_cfg.endpoint.trim().is_empty() {
        None
    } else {
        Some(provider_cfg.endpoint.trim_end_matches('/').to_string())
    };

    let config = providers::ProviderConfig {
        api_key,
        base_url,
        model: String::new(),
        temperature: 0.2,
        max_tokens: 4096,
        thinking_enabled: false,
        reasoning_effort: "high".into(),
        additional_params: serde_json::json!(serde_json::Value::Null),
        previous_interaction_id: None,
        api_mode: String::new(),
    };

    // 构建用户消息：包含当前画布状态
    let mut user_msg = prompt.clone();
    if let Some(nodes) = &current_nodes {
        if !nodes.is_empty() {
            let existing = serde_json::to_string(nodes).unwrap_or_default();
            user_msg = format!(
                "{}\n\n当前画布上已有的节点：\n{}\n\n请在此基础上修改或扩展。",
                prompt, existing
            );
        }
    }

    let extractor = providers::build_extractor::<AiGeneratedWorkflow>(
        provider_cfg.kind.backend_id(),
        &model_name,
        config,
    )
    .preamble(AI_WORKFLOW_SYSTEM_PROMPT)
    .build();

    let result = extractor
        .extract(&user_msg)
        .await
        .map_err(|e| format!("AI 生成失败: {e}"))?;

    Ok(result)
}

// ── AI 辅助润色 ─────────────────────────────────────────────────────

/// 工作流配置面板 AI 辅助：润色 / 生成文本。
///
/// `task` 描述字段用途（如 "视频生成提示词"），`text` 为当前文本（可空）。
/// 空文本时生成，有文本时润色。
///
/// 模型优先级：per-workflow 参数 > 辅助模型全局配置（模型服务 → 辅助模型 → 工作流 AI 辅助）> 活跃供应商。
#[tauri::command]
pub async fn loop_ai_polish(
    text: String,
    task: String,
    provider_id: Option<String>,
    model: Option<String>,
) -> Result<String, String> {
    use super::providers::{find_provider, resolve_api_key};

    // 优先级：per-workflow 参数 > 辅助模型全局配置 > 活跃供应商
    let (resolved_pid, resolved_mdl) = {
        let pid = provider_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let mdl = model.as_deref().map(str::trim).filter(|s| !s.is_empty());
        if pid.is_some() {
            (pid.map(str::to_string), mdl.map(str::to_string))
        } else {
            let aux = memory::load_auxiliary_config(&home::default_memory_dir());
            let route = &aux.workflow_ai_polish;
            let rp = if route.provider != "auto" && !route.provider.is_empty() {
                Some(route.provider.clone())
            } else {
                None
            };
            let rm = if route.model != "auto" && !route.model.is_empty() {
                Some(route.model.clone())
            } else {
                None
            };
            (rp, rm)
        }
    };

    let provider_cfg = if let Some(ref id) = resolved_pid {
        find_provider(id)?
    } else {
        let state = super::providers::get_providers_state()?;
        let id = state
            .active_provider_id
            .or_else(|| state.providers.first().map(|p| p.id.clone()))
            .ok_or_else(|| "请先在「模型服务」中配置至少一个供应商".to_string())?;
        find_provider(&id)?
    };

    let (has, _, _, key) = resolve_api_key(&provider_cfg);
    if provider_cfg.kind.requires_api_key() && !has {
        return Err(format!(
            "未配置 API Key。请在「模型服务」中为 {} 保存密钥。",
            provider_cfg.display_name
        ));
    }
    let api_key = key.unwrap_or_default();
    let model_name = resolved_mdl.unwrap_or_else(|| provider_cfg.model.clone());

    let base_url = if provider_cfg.endpoint.trim().is_empty() {
        None
    } else {
        Some(provider_cfg.endpoint.trim_end_matches('/').to_string())
    };

    let config = providers::ProviderConfig {
        api_key,
        base_url,
        model: model_name.clone(),
        temperature: 0.7,
        max_tokens: 2048,
        thinking_enabled: false,
        reasoning_effort: String::new(),
        additional_params: serde_json::json!(null),
        previous_interaction_id: None,
        api_mode: String::new(),
    };

    let system = if text.trim().is_empty() {
        format!(
            "你是一位专业的 AI 工作流配置助手。用户正在配置「{}」字段。\n\
             请直接生成一段高质量的内容，不要解释。\n\
             要求：专业、具体、直接可用。只输出内容本身。",
            task
        )
    } else {
        format!(
            "你是一位专业的 AI 工作流配置助手。用户正在配置「{}」字段。\n\
             请润色和改进用户提供的文本，使其更专业、更具体。\n\
             保持原意，提升质量。只输出改进后的文本，不要解释。",
            task
        )
    };
    let user_msg = if text.trim().is_empty() {
        format!("请为「{}」生成一段优质内容。", task)
    } else {
        text
    };

    workflow::nodes::ai::one_shot_llm(provider_cfg.kind.backend_id(), &config, &system, &user_msg)
        .await
        .map_err(|e| format!("AI 润色失败: {e}"))
}
