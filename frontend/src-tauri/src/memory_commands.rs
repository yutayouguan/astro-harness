//! 记忆相关 Tauri 命令。
//!
//! ## 架构说明（Frozen Snapshot / `refresh_memory`）
//!
//! - **Tauri 进程不持有长驻 [`agent::runtime::AgentLoop`]。** 聊天经 gRPC 交给 `backend`，
//!   由后端按 `session_id` 缓存 `AgentLoop`；同会话 system prompt 使用冻结 snapshot。
//! - Tauri 侧记忆读写多为请求作用域的 [`memory::MemoryManager`]（`new` / `for_agent`）。
//! - 本命令：对**当前活跃 Agent** 打开 `MemoryManager`，调用
//!   [`MemoryManager::refresh_memory_snapshot`] 从磁盘重载并返回渲染文本。
//! - 若传入 `session_id`，还会经 gRPC `ChatControl.REFRESH_MEMORY` 刷新 **backend 活会话**
//!   的 [`AgentLoop::refresh_memory`]，使后续轮次 system prompt 立刻用到新 snapshot。

use serde::Serialize;
use tauri::AppHandle;

use crate::commands::chat_control;
use crate::session_events::{
    emit_session_event, now_ts_ms, MemoryUpdatedDto, PendingChangedDto, SessionEventDto,
};

/// `refresh_memory` 返回：活跃 Agent 重载后的 MEMORY / USER snapshot 渲染。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshMemoryDto {
    pub agent_id: String,
    pub memory_content: String,
    pub user_content: String,
    /// 是否已成功通知 backend 刷新活会话（无活会话时为 false）。
    pub session_refreshed: bool,
}

/// Pending 记忆写入（供设置页列表）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingMemoryWriteDto {
    pub id: String,
    pub agent_id: String,
    pub target: String,
    pub action: String,
    pub content: Option<String>,
    pub old_text: Option<String>,
    pub source: String,
    pub created_at: String,
}

impl From<memory::PendingMemoryWrite> for PendingMemoryWriteDto {
    fn from(p: memory::PendingMemoryWrite) -> Self {
        Self {
            id: p.id,
            agent_id: p.agent_id,
            target: match p.target {
                memory::MemoryTarget::Memory => "memory".into(),
                memory::MemoryTarget::User => "user".into(),
            },
            action: p.action,
            content: p.content,
            old_text: p.old_text,
            source: p.source,
            created_at: p.created_at,
        }
    }
}

/// 从磁盘重载 MEMORY / USER；可选刷新指定 backend 会话的 frozen snapshot。
#[tauri::command]
pub async fn refresh_memory(
    agent_id: Option<String>,
    session_id: Option<String>,
) -> Result<RefreshMemoryDto, String> {
    let root = home::default_memory_dir();
    memory::ensure_workspace(&root).map_err(|e| e.to_string())?;
    let id = agent_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| home::active_agent_id(&root));
    let mut mgr = memory::MemoryManager::for_agent(root, &id).map_err(|e| e.to_string())?;
    mgr.refresh_memory_snapshot().map_err(|e| e.to_string())?;
    let (memory_content, user_content) = mgr.prompt_content();

    let mut session_refreshed = false;
    if let Some(sid) = session_id.filter(|s| !s.trim().is_empty()) {
        match chat_control(sid, "refresh_memory".into()).await {
            Ok(()) => session_refreshed = true,
            Err(e) => {
                tracing::debug!(error = %e, "活会话 refresh_memory 跳过（可能无内存会话）");
            }
        }
    }

    Ok(RefreshMemoryDto {
        agent_id: mgr.agent_id,
        memory_content,
        user_content,
        session_refreshed,
    })
}

/// 列出 `write_approval` pending 队列。
#[tauri::command]
pub async fn list_pending_memory_writes() -> Result<Vec<PendingMemoryWriteDto>, String> {
    let root = home::default_memory_dir();
    memory::list_pending(&root)
        .map(|items| items.into_iter().map(PendingMemoryWriteDto::from).collect())
        .map_err(|e| e.to_string())
}

fn pending_target_str(target: memory::MemoryTarget) -> &'static str {
    match target {
        memory::MemoryTarget::Memory => "memory",
        memory::MemoryTarget::User => "user",
    }
}

/// 批准并应用一条 pending（写 live）；成功后 emit `session_event`。
#[tauri::command]
pub async fn approve_pending_memory_write(app: AppHandle, id: String) -> Result<String, String> {
    let root = home::default_memory_dir();
    let item = memory::list_pending(&root)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("pending 写入不存在: {id}"))?;
    let agent_id = item.agent_id.clone();
    let target = pending_target_str(item.target).to_string();

    let msg = memory::approve_pending_memory(&root, &id).map_err(|e| e.to_string())?;
    let pending_count = memory::list_pending(&root)
        .map(|v| v.len() as u32)
        .unwrap_or(0);

    emit_session_event(
        &app,
        SessionEventDto {
            session_id: None,
            agent_id,
            ts_ms: now_ts_ms(),
            memory_updated: Some(MemoryUpdatedDto {
                source: "approve".into(),
                target,
                summary: msg.clone(),
                live_written: true,
            }),
            pending_changed: Some(PendingChangedDto {
                pending_count,
                reason: "approved".into(),
            }),
            session_metadata_changed: None,
        },
    );
    Ok(msg)
}

/// 拒绝并丢弃一条 pending；成功后 emit `session_event`。
#[tauri::command]
pub async fn reject_pending_memory_write(app: AppHandle, id: String) -> Result<(), String> {
    let root = home::default_memory_dir();
    let item = memory::list_pending(&root)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("pending 写入不存在: {id}"))?;
    let agent_id = item.agent_id;

    memory::reject_pending_memory(&root, &id).map_err(|e| e.to_string())?;
    let pending_count = memory::list_pending(&root)
        .map(|v| v.len() as u32)
        .unwrap_or(0);

    emit_session_event(
        &app,
        SessionEventDto {
            session_id: None,
            agent_id,
            ts_ms: now_ts_ms(),
            memory_updated: None,
            pending_changed: Some(PendingChangedDto {
                pending_count,
                reason: "rejected".into(),
            }),
            session_metadata_changed: None,
        },
    );
    Ok(())
}

/// 记忆面板开关状态（从 `config.yaml` 读取）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySettingsDto {
    pub write_approval: bool,
    pub background_review_enabled: bool,
    pub auto_refresh_on_update: bool,
}

/// 读取记忆相关开关。
#[tauri::command]
pub async fn get_memory_settings() -> Result<MemorySettingsDto, String> {
    let root = home::default_memory_dir();
    let mem = memory::load_memory_config(&root);
    let aux = memory::load_auxiliary_config(&root);
    Ok(MemorySettingsDto {
        write_approval: mem.write_approval,
        background_review_enabled: aux.background_review_enabled,
        auto_refresh_on_update: mem.auto_refresh_on_update,
    })
}

/// 设置 `memory.write_approval`。
#[tauri::command]
pub async fn set_memory_write_approval(enabled: bool) -> Result<MemorySettingsDto, String> {
    let root = home::default_memory_dir();
    memory::set_write_approval(&root, enabled).map_err(|e| e.to_string())?;
    get_memory_settings().await
}

/// 设置 `memory.auto_refresh_on_update`。
#[tauri::command]
pub async fn set_memory_auto_refresh(enabled: bool) -> Result<MemorySettingsDto, String> {
    let root = home::default_memory_dir();
    memory::set_auto_refresh_on_update(&root, enabled).map_err(|e| e.to_string())?;
    get_memory_settings().await
}

/// 设置 `auxiliary.background_review_enabled`。
#[tauri::command]
pub async fn set_background_review_enabled(enabled: bool) -> Result<MemorySettingsDto, String> {
    let root = home::default_memory_dir();
    memory::set_background_review_enabled(&root, enabled).map_err(|e| e.to_string())?;
    get_memory_settings().await
}

/// 批准全部 pending；逐条 emit（末条角标为准）。
#[tauri::command]
pub async fn approve_all_pending_memory_writes(app: AppHandle) -> Result<String, String> {
    let root = home::default_memory_dir();
    let items = memory::list_pending(&root).map_err(|e| e.to_string())?;
    let mut ok = 0usize;
    let mut err = 0usize;
    let mut last_agent = home::active_agent_id(&root);
    for p in items {
        last_agent = p.agent_id.clone();
        match memory::approve_pending_memory(&root, &p.id) {
            Ok(msg) => {
                ok += 1;
                let pending_count = memory::list_pending(&root)
                    .map(|v| v.len() as u32)
                    .unwrap_or(0);
                emit_session_event(
                    &app,
                    SessionEventDto {
                        session_id: None,
                        agent_id: p.agent_id,
                        ts_ms: now_ts_ms(),
                        memory_updated: Some(MemoryUpdatedDto {
                            source: "approve".into(),
                            target: pending_target_str(p.target).into(),
                            summary: msg,
                            live_written: true,
                        }),
                        pending_changed: Some(PendingChangedDto {
                            pending_count,
                            reason: "approved".into(),
                        }),
                        session_metadata_changed: None,
                    },
                );
            }
            Err(_) => err += 1,
        }
    }
    let _ = last_agent;
    Ok(format!("approved={ok} failed={err}"))
}

/// 拒绝全部 pending。
#[tauri::command]
pub async fn reject_all_pending_memory_writes(app: AppHandle) -> Result<String, String> {
    let root = home::default_memory_dir();
    let items = memory::list_pending(&root).map_err(|e| e.to_string())?;
    let mut ok = 0usize;
    let mut err = 0usize;
    for p in items {
        match memory::reject_pending_memory(&root, &p.id) {
            Ok(()) => {
                ok += 1;
                let pending_count = memory::list_pending(&root)
                    .map(|v| v.len() as u32)
                    .unwrap_or(0);
                emit_session_event(
                    &app,
                    SessionEventDto {
                        session_id: None,
                        agent_id: p.agent_id,
                        ts_ms: now_ts_ms(),
                        memory_updated: None,
                        pending_changed: Some(PendingChangedDto {
                            pending_count,
                            reason: "rejected".into(),
                        }),
                        session_metadata_changed: None,
                    },
                );
            }
            Err(_) => err += 1,
        }
    }
    Ok(format!("rejected={ok} failed={err}"))
}
