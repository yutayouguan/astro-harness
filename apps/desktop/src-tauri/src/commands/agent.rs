//! Agent 配置与管理 Tauri 命令：配置读取、Agent 创建/切换、图标、每日记忆、任务 worktree。

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use super::common::{bootstrap_workspace, memory_dir, memory_root, workspace_dir};
use crate::infra::grpc::default_grpc_address;

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct AppConfigDto {
    pub grpc_address: String,
    pub memory_dir: String,
    pub workspace_dir: String,
    pub active_agent_id: String,
    pub agents: Vec<AgentInfoDto>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentInfoDto {
    pub id: String,
    pub name: String,
    pub path: String,
    pub is_default: bool,
    pub is_active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vibe: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskWorktreeDto {
    pub path: String,
    pub repo_root: String,
    pub branch: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// 将内部 AgentInfo 转为前端 DTO。
fn agent_info_dto(a: home::AgentInfo) -> AgentInfoDto {
    AgentInfoDto {
        id: a.id,
        name: a.name,
        path: a.path,
        is_default: a.is_default,
        is_active: a.is_active,
        emoji: a.emoji,
        avatar: a.avatar,
        vibe: a.vibe,
    }
}

/// 与前端 `AGENTS_CHANGED_EVENT` 对齐。
const EVENT_AGENTS_CHANGED: &str = "agents-changed";

#[derive(Debug, Clone, Serialize)]
struct AgentsChangedPayload {
    active_agent_id: String,
}

fn emit_agents_changed(app: &AppHandle, active_agent_id: &str) {
    let _ = app.emit(
        EVENT_AGENTS_CHANGED,
        AgentsChangedPayload {
            active_agent_id: active_agent_id.to_string(),
        },
    );
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// 读取当前应用 / Agent 运行时配置快照。
#[tauri::command]
pub async fn get_config() -> Result<AppConfigDto, String> {
    bootstrap_workspace()?;
    let root = memory_root();
    let agents = home::list_agents(&root)
        .into_iter()
        .map(agent_info_dto)
        .collect();
    Ok(AppConfigDto {
        grpc_address: default_grpc_address(),
        memory_dir: memory_dir(),
        workspace_dir: workspace_dir(),
        active_agent_id: home::active_agent_id(&root),
        agents,
    })
}

/// 列出本机全部 Agent 及其工作区路径。
#[tauri::command]
pub async fn list_agents() -> Result<Vec<AgentInfoDto>, String> {
    bootstrap_workspace()?;
    Ok(home::list_agents(&memory_root())
        .into_iter()
        .map(agent_info_dto)
        .collect())
}

/// 创建新 Agent 记忆空间（可选激活）。
#[tauri::command]
pub async fn create_agent(app: AppHandle, name: String) -> Result<AgentInfoDto, String> {
    let _ = (app, name);
    Err("Astro 已切换为单专家模式，不能创建额外专家".into())
}

/// 切换当前活跃 Agent。
#[tauri::command]
pub async fn set_active_agent(app: AppHandle, agent_id: String) -> Result<AppConfigDto, String> {
    bootstrap_workspace()?;
    home::set_active_agent(&memory_root(), &agent_id).map_err(|e| e.to_string())?;
    let cfg = get_config().await?;
    emit_agents_changed(&app, &cfg.active_agent_id);
    Ok(cfg)
}

/// 创建引导页：暂存 Agent 图标（Lucide SVG / 上传图片），待 create_agent 时写入工作区。
#[tauri::command]
pub async fn set_pending_agent_icon(
    kind: String,
    data_base64: String,
    file_name: String,
) -> Result<(), String> {
    let kind = home::config::agent_icons::AgentIconKind::parse(&kind)
        .ok_or_else(|| format!("未知图标类型: {kind}"))?;
    let bytes = {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD
            .decode(data_base64.trim())
            .map_err(|e| format!("图标 base64 无效: {e}"))?
    };
    home::config::agent_icons::set_pending_agent_icon(&memory_root(), kind, &bytes, &file_name)
        .map_err(|e| e.to_string())
}

/// 清除创建引导页暂存的 Agent 图标。
#[tauri::command]
pub async fn clear_pending_agent_icon(kind: Option<String>) -> Result<(), String> {
    let kind = match kind.as_deref() {
        None => None,
        Some(s) => Some(
            home::config::agent_icons::AgentIconKind::parse(s)
                .ok_or_else(|| format!("未知图标类型: {s}"))?,
        ),
    };
    home::config::agent_icons::clear_pending_agent_icon(&memory_root(), kind)
        .map_err(|e| e.to_string())
}

/// 列出每日记忆日期。
#[tauri::command]
pub async fn list_daily_memory(agent_id: Option<String>) -> Result<Vec<String>, String> {
    bootstrap_workspace()?;
    let root = memory_root();
    let _ = agent_id;
    let id = home::DEFAULT_AGENT_ID.to_string();
    let ws = home::agent_workspace_dir(&root, &id);
    Ok(home::list_daily_memory_dates(&ws))
}

/// 读取指定日期的每日记忆 Markdown。
#[tauri::command]
pub async fn read_daily_memory(
    date: Option<String>,
    agent_id: Option<String>,
) -> Result<String, String> {
    bootstrap_workspace()?;
    let root = memory_root();
    let _ = agent_id;
    let id = home::DEFAULT_AGENT_ID.to_string();
    let ws = home::agent_workspace_dir(&root, &id);
    let date = date.unwrap_or_else(home::today_date_string);
    let path = home::ensure_daily_memory(&ws, &date).map_err(|e| e.to_string())?;
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
}

/// 写入指定日期的每日记忆。
#[tauri::command]
pub async fn write_daily_memory(
    content: String,
    date: Option<String>,
    agent_id: Option<String>,
) -> Result<(), String> {
    bootstrap_workspace()?;
    let root = memory_root();
    let _ = agent_id;
    let id = home::DEFAULT_AGENT_ID.to_string();
    let ws = home::agent_workspace_dir(&root, &id);
    let date = date.unwrap_or_else(home::today_date_string);
    let path = home::ensure_daily_memory(&ws, &date).map_err(|e| e.to_string())?;
    std::fs::write(&path, content).map_err(|e| e.to_string())
}

/// 独立任务：若能解析到 git root 则创建隔离 worktree；否则返回 `None`（降级共工作区）。
#[tauri::command]
pub fn prepare_task_worktree(task_id: String) -> Result<Option<TaskWorktreeDto>, String> {
    let Some(root) = agent::git_worktree::resolve_project_root(None) else {
        return Ok(None);
    };
    let Some(repo) = agent::git_worktree::find_git_root(&root) else {
        return Ok(None);
    };
    match agent::git_worktree::create_task_worktree(&repo, &task_id) {
        Ok(handle) => Ok(Some(TaskWorktreeDto {
            path: handle.path().to_string_lossy().into_owned(),
            repo_root: handle.source_root.to_string_lossy().into_owned(),
            branch: handle.branch.clone().unwrap_or(handle.head_sha.clone()),
        })),
        Err(e) => {
            tracing::warn!(error = %e, "prepare_task_worktree failed; continuing without");
            Ok(None)
        }
    }
}

/// 清理任务 worktree；脏树按 clean_only 保留。
#[tauri::command]
pub fn cleanup_task_worktree(
    path: String,
    repo_root: String,
    branch: String,
) -> Result<(), String> {
    let path = std::path::PathBuf::from(path.trim());
    let repo = std::path::PathBuf::from(repo_root.trim());
    let branch = branch.trim().to_string();
    if path.as_os_str().is_empty() || repo.as_os_str().is_empty() || branch.is_empty() {
        return Ok(());
    }
    // 泄漏 handle 字段到 cleanup API（不 drop 原 handle）
    agent::git_worktree::cleanup_task_worktree(&repo, &path, &branch, true)
        .map_err(|error| error.to_string())
}
