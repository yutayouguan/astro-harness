//! 入梦提炼 Tauri 命令：用当前激活的模型提供商调用 LLM，凝练日记 → MEMORY.md。

use futures::StreamExt;
use serde::Serialize;

use tauri::{AppHandle, Emitter};

use memory::dreaming::{
    build_dream_extract_inputs, finalize_dream_job_from_update, load_dreaming_state,
    mark_agent_dream_error, prepare_all_dream_jobs, save_dreaming_state, set_dreaming_enabled,
    DreamAgentReport, DreamJob, DreamMemoryUpdate, DreamRunReport, DreamingState,
};
use memory::{
    default_memory_dir, list_agents, load_auxiliary_config, resolve_auxiliary, AuxiliaryKind,
};
use providers::client::ProviderClient;
use providers::registry::ProviderRegistry;
use providers::trait_::{ChatMessage, ProviderConfig};

use crate::providers_commands::{self, resolve_api_key, ProviderConfig as UiProvider};

/// Dreaming 总状态（供偏好 / 状态页）。
#[derive(Debug, Clone, Serialize)]
pub struct DreamingStatusDto {
    pub enabled: bool,
    pub running: bool,
    pub last_run_at: Option<String>,
    pub last_error: Option<String>,
    pub total_points: u64,
    pub total_summaries: u64,
    pub pending_diaries: usize,
    pub agents: Vec<DreamAgentStatusDto>,
}

/// 单个 Agent 的 Dreaming 统计。
#[derive(Debug, Clone, Serialize)]
pub struct DreamAgentStatusDto {
    pub agent_id: String,
    pub agent_name: String,
    pub points: u64,
    pub new_memories: u64,
    pub pending_diaries: usize,
    pub last_run_at: Option<String>,
    pub last_error: Option<String>,
}

/// 取 UI 当前激活（或列表首个）供应商配置。
fn active_ui_provider() -> Result<UiProvider, String> {
    let state = providers_commands::get_providers_state()?;
    let id = state
        .active_provider_id
        .or_else(|| state.providers.first().map(|p| p.id.clone()))
        .ok_or_else(|| "请先在「模型提供商」中配置并启用至少一个提供商".to_string())?;
    providers_commands::find_provider(&id)
}

/// 用指定后端做一次非流式 chat 补全（Dreaming 提炼用）。
async fn complete_chat(
    backend_id: &str,
    model: &str,
    api_key: &str,
    base_url: &str,
    system: &str,
    user: &str,
) -> Result<String, String> {
    let registry = ProviderRegistry::default();
    let provider = registry.get(backend_id).ok_or_else(|| {
        format!("不支持的提供商后端: {backend_id}（请换用 OpenAI / DeepSeek / Google / Claude 等）")
    })?;
    let config = ProviderConfig {
        api_key: api_key.to_string(),
        base_url: if base_url.trim().is_empty() {
            None
        } else {
            Some(base_url.trim_end_matches('/').to_string())
        },
        model: model.to_string(),
        temperature: 0.3,
        max_tokens: 4096,
        thinking_enabled: false,
        reasoning_effort: "high".to_string(),
        additional_params: serde_json::Value::Null,
    };
    let messages = vec![
        ChatMessage::text("system", system),
        ChatMessage::text("user", user),
    ];
    let mut stream = provider
        .chat_stream(messages, vec![], &config)
        .await
        .map_err(|e| format!("入梦调用模型失败: {e}"))?;
    let mut out = String::new();
    while let Some(item) = stream.next().await {
        let chunk = item.map_err(|e| format!("入梦流式读取失败: {e}"))?;
        if let Some(token) = chunk.token {
            out.push_str(&token);
        }
    }
    if out.trim().is_empty() {
        return Err("模型未返回任何内容".into());
    }
    Ok(out)
}

/// 优先走 Extractor（submit 结构化）；失败则回退自由文本 MEMORY.md。
async fn extract_or_fallback_dream(
    backend_id: &str,
    model: &str,
    api_key: &str,
    base_url: &str,
    job: &DreamJob,
) -> Result<DreamMemoryUpdate, String> {
    let base = if base_url.trim().is_empty() {
        None
    } else {
        Some(base_url.trim_end_matches('/').to_string())
    };
    let client = ProviderClient::from_config(backend_id, api_key.to_string(), base);
    let (preamble, text) =
        build_dream_extract_inputs(&job.agent_name, &job.memory_before, &job.diaries);

    let extractor = client
        .extractor::<DreamMemoryUpdate>(model)
        .preamble(preamble)
        .build();

    match extractor.extract(&text).await {
        Ok(update) if !update.memory_markdown.trim().is_empty() => Ok(update),
        extract_result => {
            let extract_err = match &extract_result {
                Ok(_) => "结构化抽取返回空 memory_markdown".to_string(),
                Err(e) => e.to_string(),
            };
            tracing::warn!(
                agent = %job.agent_id,
                error = %extract_err,
                "入梦 Extractor 失败，回退自由文本"
            );
            let output = complete_chat(
                backend_id,
                model,
                api_key,
                base_url,
                &job.system_prompt,
                &job.user_prompt,
            )
            .await?;
            Ok(DreamMemoryUpdate {
                memory_markdown: output,
                change_summary: Some(format!("fallback after extract error: {extract_err}")),
            })
        }
    }
}

/// 将持久化 [`DreamingState`] 转为前端状态 DTO（含各 Agent 待处理日记数）。
fn status_from_state(base: &std::path::Path, state: &DreamingState) -> DreamingStatusDto {
    let agents_info = list_agents(base);
    let jobs = prepare_all_dream_jobs(base, state);
    let pending_total: usize = jobs.iter().map(|j| j.diaries.len()).sum();
    let pending_by_agent: std::collections::HashMap<String, usize> = jobs
        .iter()
        .map(|j| (j.agent_id.clone(), j.diaries.len()))
        .collect();

    let agents = agents_info
        .into_iter()
        .map(|a| {
            let st = state.agents.get(&a.id);
            DreamAgentStatusDto {
                agent_id: a.id.clone(),
                agent_name: a.name,
                points: st.map(|s| s.points).unwrap_or(0),
                new_memories: st.map(|s| s.new_memories).unwrap_or(0),
                pending_diaries: *pending_by_agent.get(&a.id).unwrap_or(&0),
                last_run_at: st.and_then(|s| s.last_run_at.clone()),
                last_error: st.and_then(|s| s.last_error.clone()),
            }
        })
        .collect();

    DreamingStatusDto {
        enabled: state.enabled,
        running: state.running,
        last_run_at: state.last_run_at.clone(),
        last_error: state.last_error.clone(),
        total_points: state.total_points,
        total_summaries: state.total_summaries,
        pending_diaries: pending_total,
        agents,
    }
}

/// 将 Dreaming `running` 标志清为 false（崩溃恢复 / Guard 用）。
fn clear_dreaming_running(base: &std::path::Path) {
    let mut state = load_dreaming_state(base);
    if !state.running {
        return;
    }
    state.running = false;
    state.updated_at = Some(chrono::Utc::now().to_rfc3339());
    let _ = save_dreaming_state(base, &state);
}

/// 确保 `run_dreaming` 无论正常返回、Err 还是 panic，都会清掉 running。
struct DreamingRunningGuard {
    /// 记忆根目录。
    base: std::path::PathBuf,
}

impl Drop for DreamingRunningGuard {
    /// 退出作用域时调用 [`clear_dreaming_running`]。
    fn drop(&mut self) {
        clear_dreaming_running(&self.base);
    }
}

/// Tauri 命令：get_dreaming_status。
#[tauri::command]
pub async fn get_dreaming_status() -> Result<DreamingStatusDto, String> {
    let base = default_memory_dir();
    let state = load_dreaming_state(&base);
    Ok(status_from_state(&base, &state))
}

/// Tauri 命令：set_dreaming_enabled_cmd。
#[tauri::command]
pub async fn set_dreaming_enabled_cmd(enabled: bool) -> Result<DreamingStatusDto, String> {
    let base = default_memory_dir();
    let state = set_dreaming_enabled(&base, enabled).map_err(|e| e.to_string())?;
    Ok(status_from_state(&base, &state))
}

/// 取入梦用提供商：先按 `auxiliary.dreaming` 解析，再回退到 UI 激活提供商。
fn resolve_dreaming_provider() -> Result<(UiProvider, String, String), String> {
    let ui = active_ui_provider()?;
    let session_backend = ui.kind.backend_id().to_string();
    let session_model = ui.model.clone();
    let base = default_memory_dir();
    let aux = load_auxiliary_config(&base);
    let (prov, model) = resolve_auxiliary(
        AuxiliaryKind::Dreaming,
        &aux,
        &session_backend,
        &session_model,
    );

    let provider = if prov == session_backend {
        ui
    } else {
        providers_commands::find_provider_by_backend(&prov).unwrap_or(ui)
    };
    if model.trim().is_empty() {
        return Err(
            "入梦模型未配置（auxiliary.dreaming.model 与激活提供商均无模型）".into(),
        );
    }
    let backend_id = provider.kind.backend_id().to_string();
    Ok((provider, backend_id, model))
}

/// Tauri 命令：run_dreaming。
#[tauri::command]
pub async fn run_dreaming(app: AppHandle) -> Result<DreamRunReport, String> {
    let base = default_memory_dir();
    let mut state = load_dreaming_state(&base);
    if state.running {
        return Err("入梦正在进行中，请稍候".into());
    }

    let (ui, backend_id, model) = resolve_dreaming_provider()?;
    let (has, _src, _env, key) = resolve_api_key(&ui);
    if ui.kind.requires_api_key() && !has {
        return Err(format!(
            "未配置 API Key。请在「模型提供商」中为 {} 保存密钥。",
            ui.display_name
        ));
    }
    let api_key = key.unwrap_or_default();
    let base_url = ui.endpoint.clone();

    if !state.enabled {
        return Err("请先开启做梦功能".into());
    }

    state.running = true;
    state.last_error = None;
    state.updated_at = Some(chrono::Utc::now().to_rfc3339());
    save_dreaming_state(&base, &state).map_err(|e| e.to_string())?;
    let _running_guard = DreamingRunningGuard { base: base.clone() };

    let jobs = prepare_all_dream_jobs(&base, &state);
    if jobs.is_empty() {
        state.running = false;
        state.last_run_at = Some(chrono::Utc::now().to_rfc3339());
        save_dreaming_state(&base, &state).map_err(|e| e.to_string())?;
        return Ok(DreamRunReport {
            ok: true,
            agents_processed: 0,
            diaries_processed: 0,
            total_summaries: state.total_summaries,
            total_points: state.total_points,
            last_error: None,
            agents: vec![],
        });
    }

    tracing::info!(
        backend = %backend_id,
        model = %model,
        "入梦使用 auxiliary.dreaming 解析后的模型"
    );

    let mut reports: Vec<DreamAgentReport> = Vec::new();
    let mut diaries_processed = 0usize;
    let mut last_error: Option<String> = None;

    for job in &jobs {
        match extract_or_fallback_dream(&backend_id, &model, &api_key, &base_url, job).await {
            Ok(update) => match finalize_dream_job_from_update(&mut state, job, &update) {
                Ok(rep) => {
                    diaries_processed += rep.diaries;
                    reports.push(rep);
                }
                Err(e) => {
                    let msg = e.to_string();
                    mark_agent_dream_error(&mut state, &job.agent_id, &msg);
                    last_error = Some(msg.clone());
                    reports.push(DreamAgentReport {
                        agent_id: job.agent_id.clone(),
                        agent_name: job.agent_name.clone(),
                        diaries: job.diaries.len(),
                        new_memories: 0,
                        points: 0,
                        error: Some(msg),
                    });
                }
            },
            Err(e) => {
                mark_agent_dream_error(&mut state, &job.agent_id, &e);
                last_error = Some(e.clone());
                reports.push(DreamAgentReport {
                    agent_id: job.agent_id.clone(),
                    agent_name: job.agent_name.clone(),
                    diaries: job.diaries.len(),
                    new_memories: 0,
                    points: 0,
                    error: Some(e),
                });
            }
        }
        let _ = save_dreaming_state(&base, &state);
    }

    state.running = false;
    state.last_run_at = Some(chrono::Utc::now().to_rfc3339());
    state.last_error = last_error.clone();
    save_dreaming_state(&base, &state).map_err(|e| e.to_string())?;

    let new_memories: u64 = reports.iter().map(|r| r.new_memories).sum();
    if new_memories > 0 {
        let _ = app.emit(
            "memory-updated",
            serde_json::json!({
                "op": "dreaming",
                "new_memories": new_memories,
                "content": format!("记忆已更新（入梦·{new_memories}）"),
            }),
        );
    }

    Ok(DreamRunReport {
        ok: last_error.is_none(),
        agents_processed: reports.iter().filter(|r| r.error.is_none()).count(),
        diaries_processed,
        total_summaries: state.total_summaries,
        total_points: state.total_points,
        last_error,
        agents: reports,
    })
}
