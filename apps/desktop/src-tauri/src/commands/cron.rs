//! 定时任务 Tauri 命令：CRUD、自然语言提取、立即执行、运行记录查询。

use serde::{Deserialize, Serialize};

use super::common::bootstrap_workspace;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct CronJobDto {
    pub id: String,
    pub schedule: String,
    pub task: String,
    pub title: String,
    pub agent_id: String,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub last_run_at: Option<String>,
    pub next_run_at: Option<String>,
    pub show_in_chat: bool,
}

#[derive(Debug, Deserialize)]
pub struct AddCronJobArgs {
    pub schedule: String,
    pub task: String,
    pub title: String,
    #[serde(default = "default_agent")]
    pub agent_id: String,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub show_in_chat: bool,
}

#[derive(Debug, Deserialize)]
pub struct ExtractCronJobArgs {
    /// 自然语言描述，如「每天早上九点提醒我喝水」
    pub text: String,
    /// 可选：指定提供商配置 id；缺省用当前激活提供商
    pub provider_id: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExtractCronJobDto {
    pub schedule: String,
    pub task: String,
    pub title: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCronJobArgs {
    pub id: String,
    pub schedule: String,
    pub task: String,
    pub title: String,
    #[serde(default = "default_agent")]
    pub agent_id: String,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub show_in_chat: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CronRunDto {
    pub id: String,
    pub job_id: String,
    pub title: String,
    pub agent_id: String,
    pub schedule: String,
    pub task: String,
    pub fired_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub summary: String,
    pub output: String,
    pub error: Option<String>,
    pub session_id: Option<String>,
    pub trigger: String,
}

#[derive(Debug, Deserialize)]
pub struct ListCronRunsArgs {
    pub job_id: Option<String>,
    pub agent_id: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    #[serde(default = "default_run_limit")]
    pub limit: u32,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// 定时任务运行列表默认条数上限。
fn default_run_limit() -> u32 {
    100
}

/// 解析默认 / 活跃 Agent id。
fn default_agent() -> String {
    home::DEFAULT_AGENT_ID.into()
}

/// CronJob → 前端任务 DTO。
fn job_to_dto(j: cron::CronJob) -> CronJobDto {
    CronJobDto {
        id: j.id,
        schedule: j.schedule,
        task: j.task,
        title: j.title,
        agent_id: j.agent_id,
        provider_id: j.provider_id,
        model: j.model,
        enabled: j.enabled,
        created_at: j.created_at,
        last_run_at: j.last_run_at,
        next_run_at: j.next_run_at,
        show_in_chat: j.show_in_chat,
    }
}

/// CronRun → 前端运行记录 DTO。
fn run_to_dto(r: cron::run_db::CronRunRow) -> CronRunDto {
    CronRunDto {
        id: r.id,
        job_id: r.job_id,
        title: r.title,
        agent_id: r.agent_id,
        schedule: r.schedule,
        task: r.task,
        fired_at: r.fired_at,
        finished_at: r.finished_at,
        status: r.status,
        summary: r.summary,
        output: r.output,
        error: r.error,
        session_id: r.session_id,
        trigger: r.trigger,
    }
}

/// 为定时任务解析 Provider/模型/密钥。
fn resolve_creds_for_job(
    job: &cron::CronJob,
) -> Result<agent::exec::cron::CronExecCredentials, String> {
    use super::providers::{find_provider, find_provider_by_backend, resolve_chat_targets};

    let provider_cfg = if let Some(id) = job.provider_id.as_deref().filter(|s| !s.is_empty()) {
        find_provider(id).or_else(|_| find_provider_by_backend(id))?
    } else {
        let state = super::providers::get_providers_state()?;
        let id = state
            .active_provider_id
            .or_else(|| state.providers.first().map(|p| p.id.clone()))
            .ok_or_else(|| "请先在「模型提供商」中配置至少一个提供商".to_string())?;
        find_provider(&id)?
    };

    let model = job
        .model
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| provider_cfg.model.clone());

    let targets = resolve_chat_targets(
        Some(provider_cfg.id.as_str()),
        provider_cfg.kind.backend_id(),
        &model,
    )?;
    let primary = targets
        .first()
        .ok_or_else(|| "未能解析聊天目标链，请检查模型提供商配置".to_string())?;

    Ok(agent::exec::cron::CronExecCredentials {
        provider: primary.backend_id.clone(),
        model: primary.model.clone(),
        api_key: primary.api_key.clone(),
        base_url: primary.base_url.clone(),
        targets,
    })
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// 列出定时任务。
#[tauri::command]
pub async fn list_cron_jobs() -> Result<Vec<CronJobDto>, String> {
    bootstrap_workspace()?;
    let store = cron::CronStore::open_default().map_err(|e| e.to_string())?;
    let jobs = store.list().map_err(|e| e.to_string())?;
    Ok(jobs.into_iter().map(job_to_dto).collect())
}

/// 用 Extractor 从自然语言提炼 schedule/task/title（不落库，仅填表）。
#[tauri::command]
pub async fn extract_cron_job(args: ExtractCronJobArgs) -> Result<ExtractCronJobDto, String> {
    bootstrap_workspace()?;
    let text = args.text.trim();
    if text.is_empty() {
        return Err("请输入自然语言描述".into());
    }

    use super::providers::{find_provider, resolve_api_key};

    let provider_cfg = if let Some(id) = args.provider_id.as_deref().filter(|s| !s.is_empty()) {
        find_provider(id)?
    } else {
        let state = super::providers::get_providers_state()?;
        let id = state
            .active_provider_id
            .or_else(|| state.providers.first().map(|p| p.id.clone()))
            .ok_or_else(|| "请先在「模型提供商」中配置至少一个提供商".to_string())?;
        find_provider(&id)?
    };

    let (has, _src, _env, key) = resolve_api_key(&provider_cfg);
    if provider_cfg.kind.requires_api_key() && !has {
        return Err(format!(
            "未配置 API Key。请在「模型提供商」中为 {} 保存密钥。",
            provider_cfg.display_name
        ));
    }
    let api_key = key.unwrap_or_default();
    let model = args
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(provider_cfg.model.as_str())
        .to_string();
    if model.trim().is_empty() {
        return Err("当前提供商未设置默认模型".into());
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
        additional_params: serde_json::Value::Null,
        previous_interaction_id: None,
        api_mode: String::new(),
    };
    let extractor = providers::build_extractor::<cron::CronJobExtract>(
        provider_cfg.kind.backend_id(),
        &model,
        config,
    )
    .preamble(cron::cron_extract_preamble())
    .build();

    let raw = extractor
        .extract(text)
        .await
        .map_err(|e| format!("抽取失败: {e}"))?;
    let normalized = cron::normalize_cron_extract(raw).map_err(|e| e.to_string())?;

    Ok(ExtractCronJobDto {
        schedule: normalized.schedule,
        task: normalized.task,
        title: normalized.title.unwrap_or_default(),
    })
}

/// 新增定时任务。
#[tauri::command]
pub async fn add_cron_job(args: AddCronJobArgs) -> Result<CronJobDto, String> {
    bootstrap_workspace()?;
    let store = cron::CronStore::open_default().map_err(|e| e.to_string())?;
    let j = store
        .add_job(cron::NewCronJob {
            schedule: args.schedule,
            task: args.task,
            title: args.title,
            agent_id: args.agent_id,
            provider_id: args.provider_id,
            model: args.model,
            show_in_chat: args.show_in_chat,
        })
        .map_err(|e| e.to_string())?;
    Ok(job_to_dto(j))
}

/// 更新定时任务。
#[tauri::command]
pub async fn update_cron_job(args: UpdateCronJobArgs) -> Result<CronJobDto, String> {
    bootstrap_workspace()?;
    let store = cron::CronStore::open_default().map_err(|e| e.to_string())?;
    let j = store
        .update_job(
            &args.id,
            cron::NewCronJob {
                schedule: args.schedule,
                task: args.task,
                title: args.title,
                agent_id: args.agent_id,
                provider_id: args.provider_id,
                model: args.model,
                show_in_chat: args.show_in_chat,
            },
        )
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "未找到定时任务".to_string())?;
    Ok(job_to_dto(j))
}

/// 删除定时任务。
#[tauri::command]
pub async fn remove_cron_job(id: String) -> Result<bool, String> {
    bootstrap_workspace()?;
    let store = cron::CronStore::open_default().map_err(|e| e.to_string())?;
    store.remove(&id).map_err(|e| e.to_string())
}

/// Tauri 命令：set_cron_job_enabled。
#[tauri::command]
pub async fn set_cron_job_enabled(id: String, enabled: bool) -> Result<bool, String> {
    bootstrap_workspace()?;
    let store = cron::CronStore::open_default().map_err(|e| e.to_string())?;
    store.set_enabled(&id, enabled).map_err(|e| e.to_string())
}

/// 立即执行一次定时任务（后台跑完；立即返回 running 记录以便 UI 边跑边看）。
#[tauri::command]
pub async fn run_cron_job_now(id: String) -> Result<CronRunDto, String> {
    bootstrap_workspace()?;
    let store = cron::CronStore::open_default().map_err(|e| e.to_string())?;
    let job = store
        .list()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|j| j.id == id || j.id.starts_with(&id))
        .ok_or_else(|| "未找到定时任务".to_string())?;
    let creds = resolve_creds_for_job(&job)?;
    let label = {
        let t = job.title.trim();
        if !t.is_empty() {
            t.to_string()
        } else {
            let task = job.task.trim();
            let mut it = task.chars();
            let head: String = it.by_ref().take(48).collect();
            if it.next().is_some() {
                format!("{head}…")
            } else {
                head
            }
        }
    };
    let row = match agent::exec::cron::spawn_job(&job, creds, "manual").await {
        Ok(row) => row,
        Err(err) => {
            types::notify_kind(types::ImportantKind::CronFailure, format!("{label}\n{err}"));
            return Err(err.to_string());
        }
    };
    let _ = store.touch_last_run(&job.id, Some(row.fired_at.clone()));

    // 后台结束后再通知；轮询 get_cron_run 拿终态
    let run_id = row.id.clone();
    let label_bg = label.clone();
    let job_id = job.id.clone();
    tauri::async_runtime::spawn(async move {
        // 轮询直到非 running（最长约 10 分钟 + 余量）
        for _ in 0..650 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let Ok(db) = cron::CronRunDb::open_default().await else {
                continue;
            };
            let Ok(Some(finished)) = db.get(&run_id).await else {
                continue;
            };
            if finished.status == "running" {
                continue;
            }
            if let Ok(store) = cron::CronStore::open_default() {
                let _ = store.touch_last_run(&job_id, Some(finished.fired_at.clone()));
            }
            if finished.status == "success" {
                let body = if finished.summary.trim().is_empty() {
                    label_bg
                } else {
                    format!(
                        "{label_bg}\n{}",
                        types::truncate_notify(&finished.summary, 120)
                    )
                };
                types::notify_kind(types::ImportantKind::CronSuccess, body);
            } else {
                let detail = finished
                    .error
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| types::truncate_notify(s, 120))
                    .or_else(|| {
                        let s = finished.summary.trim();
                        if s.is_empty() {
                            None
                        } else {
                            Some(types::truncate_notify(s, 120))
                        }
                    })
                    .unwrap_or_else(|| finished.status.clone());
                types::notify_kind(
                    types::ImportantKind::CronFailure,
                    format!("{label_bg}\n{detail}"),
                );
            }
            break;
        }
    });

    Ok(run_to_dto(row))
}

/// 按 id 取单条定时任务运行记录（供执行记录抽屉轮询）。
#[tauri::command]
pub async fn get_cron_run(id: String) -> Result<Option<CronRunDto>, String> {
    bootstrap_workspace()?;
    let _ = agent::exec::cron::reconcile_orphaned_runs();
    let db = cron::CronRunDb::open_default()
        .await
        .map_err(|e| e.to_string())?;
    let row = db.get(&id).await.map_err(|e| e.to_string())?;
    Ok(row.map(run_to_dto))
}

/// 按会话 id 取对应的定时任务运行记录（供聊天结果卡片使用）。
#[tauri::command]
pub async fn get_cron_run_by_session(session_id: String) -> Result<Option<CronRunDto>, String> {
    bootstrap_workspace()?;
    let _ = agent::exec::cron::reconcile_orphaned_runs();
    let db = cron::CronRunDb::open_default()
        .await
        .map_err(|e| e.to_string())?;
    let row = db
        .get_by_session_id(&session_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(row.map(run_to_dto))
}

/// 删除单条定时任务运行记录。
#[tauri::command]
pub async fn delete_cron_run(id: String) -> Result<bool, String> {
    bootstrap_workspace()?;
    let db = cron::CronRunDb::open_default()
        .await
        .map_err(|e| e.to_string())?;
    db.delete(&id).await.map_err(|e| e.to_string())
}

/// 列出定时任务运行记录。
#[tauri::command]
pub async fn list_cron_runs(args: ListCronRunsArgs) -> Result<Vec<CronRunDto>, String> {
    bootstrap_workspace()?;
    let _ = agent::exec::cron::reconcile_orphaned_runs();
    let db = cron::CronRunDb::open_default()
        .await
        .map_err(|e| e.to_string())?;
    let rows = db
        .list_filtered(cron::run_db::CronRunFilters {
            job_id: args.job_id,
            agent_id: args.agent_id,
            date_from: args.date_from,
            date_to: args.date_to,
            limit: args.limit,
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(run_to_dto).collect())
}

/// Tauri 命令：list_cron_job_runs。
#[tauri::command]
pub async fn list_cron_job_runs(id: String) -> Result<Vec<CronRunDto>, String> {
    list_cron_runs(ListCronRunsArgs {
        job_id: Some(id),
        agent_id: None,
        date_from: None,
        date_to: None,
        limit: default_run_limit(),
    })
    .await
}
