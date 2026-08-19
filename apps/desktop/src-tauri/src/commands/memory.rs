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

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

use super::chat::chat_control;
use crate::infra::thread_events::{
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

/// 危险命令审批设置（`approvals:` 段）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalSettingsDto {
    pub mode: String,
    pub command_allowlist: Vec<String>,
}

impl From<memory::ApprovalsConfig> for ApprovalSettingsDto {
    fn from(c: memory::ApprovalsConfig) -> Self {
        Self {
            mode: c.mode,
            command_allowlist: c.command_allowlist,
        }
    }
}

/// 权限与沙箱审计的安全展示 DTO；不暴露命令正文、路径或 capability target。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityAuditEventDto {
    pub source: String,
    pub id: String,
    pub event: String,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub tool_name: String,
    pub profile_id: String,
    pub result: Option<String>,
    pub duration_ms: Option<u64>,
    pub created_at: String,
    pub reviewer: Option<String>,
    pub scope: Option<String>,
    pub capabilities: Vec<SecurityAuditCapabilityDto>,
    pub backend: Option<String>,
    pub sandboxed: Option<bool>,
    pub mode: Option<String>,
    pub network_access: Option<bool>,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityAuditCapabilityDto {
    pub kind: String,
    pub target_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityAuditPageDto {
    pub items: Vec<SecurityAuditEventDto>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityAuditExportResultDto {
    pub event_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityAuditRetentionSourceDto {
    pub source: &'static str,
    pub max_file_bytes: u64,
    pub archive_count: usize,
    pub retained_file_count: usize,
    pub max_total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityAuditRetentionDto {
    pub sources: [SecurityAuditRetentionSourceDto; 2],
    pub max_total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecurityAuditClearResultDto {
    pub files_removed: usize,
    pub bytes_removed: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SecurityAuditExportFile {
    schema_version: u8,
    exported_at: String,
    scope: &'static str,
    source: String,
    event_count: usize,
    omitted_fields: [&'static str; 4],
    events: Vec<SecurityAuditEventDto>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SecurityAuditCursor {
    created_at: String,
    id: String,
}

fn permission_audit_dto(event: memory::PermissionAuditEvent) -> SecurityAuditEventDto {
    SecurityAuditEventDto {
        source: "permission".into(),
        id: event.id,
        event: serde_json::to_value(event.event)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "permission.unknown".into()),
        session_id: Some(event.session_id),
        turn_id: event.turn_id,
        tool_name: event.tool_name,
        profile_id: event.profile_id,
        result: event.result,
        duration_ms: event.duration_ms,
        created_at: event.created_at,
        reviewer: event
            .reviewer
            .map(|value| format!("{value:?}").to_ascii_lowercase()),
        scope: Some(format!("{:?}", event.scope).to_ascii_lowercase()),
        capabilities: event
            .capabilities
            .into_iter()
            .map(|capability| SecurityAuditCapabilityDto {
                kind: capability.kind,
                target_count: capability.targets.len(),
            })
            .collect(),
        backend: None,
        sandboxed: None,
        mode: None,
        network_access: None,
        target: None,
    }
}

fn sandbox_audit_dto(event: sandbox::SandboxAuditEvent) -> SecurityAuditEventDto {
    SecurityAuditEventDto {
        source: "sandbox".into(),
        id: event.id,
        event: serde_json::to_value(event.event)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "sandbox.unknown".into()),
        session_id: event.session_id,
        turn_id: event.turn_id,
        tool_name: event.tool_name,
        profile_id: event.profile_id,
        result: Some(event.result),
        duration_ms: event.duration_ms,
        created_at: event.created_at,
        reviewer: None,
        scope: None,
        capabilities: Vec::new(),
        backend: Some(event.backend),
        sandboxed: Some(event.sandboxed),
        mode: event.mode,
        network_access: Some(event.network_access),
        target: Some(event.target),
    }
}

fn list_security_audits_from(
    root: &std::path::Path,
    limit: usize,
) -> anyhow::Result<Vec<SecurityAuditEventDto>> {
    let limit = limit.clamp(1, 500);
    let mut events = memory::list_recent_permission_audits(root, limit)?
        .into_iter()
        .map(permission_audit_dto)
        .chain(
            sandbox::list_recent_sandbox_audits(root, limit)?
                .into_iter()
                .map(sandbox_audit_dto),
        )
        .collect::<Vec<_>>();
    events.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    events.truncate(limit);
    Ok(events)
}

fn collect_security_audits_for_export(
    root: &std::path::Path,
    source: &str,
    limit: usize,
) -> anyhow::Result<Vec<SecurityAuditEventDto>> {
    if !(1..=5_000).contains(&limit) {
        anyhow::bail!("security audit export limit must be between 1 and 5000");
    }
    let mut events = match source {
        "all" => memory::list_recent_permission_audits(root, limit)?
            .into_iter()
            .map(permission_audit_dto)
            .chain(
                sandbox::list_recent_sandbox_audits(root, limit)?
                    .into_iter()
                    .map(sandbox_audit_dto),
            )
            .collect::<Vec<_>>(),
        "permission" => memory::list_recent_permission_audits(root, limit)?
            .into_iter()
            .map(permission_audit_dto)
            .collect(),
        "sandbox" => sandbox::list_recent_sandbox_audits(root, limit)?
            .into_iter()
            .map(sandbox_audit_dto)
            .collect(),
        _ => anyhow::bail!("unsupported security audit source: {source}"),
    };
    events.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.id.cmp(&left.id))
    });
    events.truncate(limit);
    Ok(events)
}

fn security_audit_retention() -> SecurityAuditRetentionDto {
    let permission_file_count = memory::PERMISSION_AUDIT_ARCHIVE_COUNT.saturating_add(1);
    let permission_max_total = memory::MAX_PERMISSION_AUDIT_FILE_BYTES
        .saturating_mul(u64::try_from(permission_file_count).unwrap_or(u64::MAX));
    let sandbox_file_count = sandbox::SANDBOX_AUDIT_ARCHIVE_COUNT.saturating_add(1);
    let sandbox_max_total = sandbox::MAX_SANDBOX_AUDIT_FILE_BYTES
        .saturating_mul(u64::try_from(sandbox_file_count).unwrap_or(u64::MAX));
    SecurityAuditRetentionDto {
        sources: [
            SecurityAuditRetentionSourceDto {
                source: "permission",
                max_file_bytes: memory::MAX_PERMISSION_AUDIT_FILE_BYTES,
                archive_count: memory::PERMISSION_AUDIT_ARCHIVE_COUNT,
                retained_file_count: permission_file_count,
                max_total_bytes: permission_max_total,
            },
            SecurityAuditRetentionSourceDto {
                source: "sandbox",
                max_file_bytes: sandbox::MAX_SANDBOX_AUDIT_FILE_BYTES,
                archive_count: sandbox::SANDBOX_AUDIT_ARCHIVE_COUNT,
                retained_file_count: sandbox_file_count,
                max_total_bytes: sandbox_max_total,
            },
        ],
        max_total_bytes: permission_max_total.saturating_add(sandbox_max_total),
    }
}

fn clear_security_audits_from(
    root: &std::path::Path,
) -> anyhow::Result<SecurityAuditClearResultDto> {
    let (permission_files, permission_bytes) = memory::clear_permission_audits(root)?;
    let (sandbox_files, sandbox_bytes) = sandbox::clear_sandbox_audits(root)?;
    Ok(SecurityAuditClearResultDto {
        files_removed: permission_files.saturating_add(sandbox_files),
        bytes_removed: permission_bytes.saturating_add(sandbox_bytes),
    })
}

fn export_security_audits_to(
    root: &std::path::Path,
    path: &std::path::Path,
    source: &str,
    limit: usize,
) -> anyhow::Result<SecurityAuditExportResultDto> {
    if !path.is_absolute() {
        anyhow::bail!("security audit export path must be absolute");
    }
    if !path.parent().is_some_and(|parent| parent.is_dir()) {
        anyhow::bail!("security audit export directory does not exist");
    }
    let events = collect_security_audits_for_export(root, source, limit)?;
    let event_count = events.len();
    let export = SecurityAuditExportFile {
        schema_version: 1,
        exported_at: chrono::Utc::now().to_rfc3339(),
        scope: "loaded_filtered_events",
        source: source.to_string(),
        event_count,
        omitted_fields: ["command_text", "arguments", "paths", "capability_targets"],
        events,
    };
    let mut bytes = serde_json::to_vec_pretty(&export)?;
    bytes.push(b'\n');
    std::fs::write(path, bytes)?;
    Ok(SecurityAuditExportResultDto { event_count })
}

fn list_security_audit_page_from(
    root: &std::path::Path,
    limit: usize,
    cursor: Option<&str>,
) -> anyhow::Result<SecurityAuditPageDto> {
    let limit = limit.clamp(1, 100);
    let cursor = cursor
        .map(serde_json::from_str::<SecurityAuditCursor>)
        .transpose()
        .map_err(|error| anyhow::anyhow!("invalid security audit cursor: {error}"))?;
    let before = cursor
        .as_ref()
        .map(|cursor| (cursor.created_at.as_str(), cursor.id.as_str()));
    let source_limit = limit.saturating_add(1);
    let mut events = memory::list_permission_audits_before(root, before, source_limit)?
        .into_iter()
        .map(permission_audit_dto)
        .chain(
            sandbox::list_sandbox_audits_before(root, before, source_limit)?
                .into_iter()
                .map(sandbox_audit_dto),
        )
        .collect::<Vec<_>>();
    events.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.id.cmp(&left.id))
    });
    let has_more = events.len() > limit;
    events.truncate(limit);
    let next_cursor = if has_more {
        events
            .last()
            .map(|event| {
                serde_json::to_string(&SecurityAuditCursor {
                    created_at: event.created_at.clone(),
                    id: event.id.clone(),
                })
            })
            .transpose()?
    } else {
        None
    };
    Ok(SecurityAuditPageDto {
        items: events,
        next_cursor,
    })
}

/// 读取最近安全审计；默认 100 条，最多 500 条。
#[tauri::command]
pub async fn list_security_audits(
    limit: Option<usize>,
) -> Result<Vec<SecurityAuditEventDto>, String> {
    list_security_audits_from(&home::default_memory_dir(), limit.unwrap_or(100))
        .map_err(|error| error.to_string())
}

/// 分页读取安全审计；游标稳定指向上一页最后一条记录。
#[tauri::command]
pub async fn list_security_audit_page(
    limit: Option<usize>,
    cursor: Option<String>,
) -> Result<SecurityAuditPageDto, String> {
    list_security_audit_page_from(
        &home::default_memory_dir(),
        limit.unwrap_or(50),
        cursor.as_deref(),
    )
    .map_err(|error| error.to_string())
}

/// 将当前筛选下已加载的安全审计导出为隐私裁剪后的 JSON。
#[tauri::command]
pub async fn export_security_audits(
    app: AppHandle,
    source: String,
    limit: usize,
) -> Result<Option<SecurityAuditExportResultDto>, String> {
    let file_name = format!(
        "astro-security-audit-{}.json",
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    );
    let Some(file_path) = app
        .dialog()
        .file()
        .add_filter("JSON", &["json"])
        .set_file_name(file_name)
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = file_path.into_path().map_err(|error| error.to_string())?;
    export_security_audits_to(&home::default_memory_dir(), &path, &source, limit)
        .map(Some)
        .map_err(|error| error.to_string())
}

/// 返回权限与沙箱审计的轮转保留上限。
#[tauri::command]
pub async fn get_security_audit_retention() -> Result<SecurityAuditRetentionDto, String> {
    Ok(security_audit_retention())
}

/// 清除权限与沙箱审计的当前文件及轮转归档。
#[tauri::command]
pub async fn clear_security_audits() -> Result<SecurityAuditClearResultDto, String> {
    clear_security_audits_from(&home::default_memory_dir()).map_err(|error| error.to_string())
}

/// 读取危险命令审批设置。
#[tauri::command]
pub async fn get_approval_settings() -> Result<ApprovalSettingsDto, String> {
    let root = home::default_memory_dir();
    Ok(memory::load_approvals_config(&root).into())
}

/// 设置审批模式（`smart` | `manual` | `off`）。
#[tauri::command]
pub async fn set_approval_mode(mode: String) -> Result<ApprovalSettingsDto, String> {
    let normalized = match mode.trim().to_ascii_lowercase().as_str() {
        m @ ("smart" | "manual" | "off") => m.to_string(),
        other => return Err(format!("无效的审批模式: {other}（应为 smart|manual|off）")),
    };
    let root = home::default_memory_dir();
    Ok(memory::set_approval_mode(&root, &normalized)
        .map_err(|e| e.to_string())?
        .into())
}

/// 新权限系统的当前组合与平台沙箱健康状态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionSettingsDto {
    pub preset: Option<String>,
    pub settings: memory::LoadedPermissionSettings,
    pub sandbox_health: sandbox::SandboxHealth,
}

fn permission_settings_dto() -> PermissionSettingsDto {
    let root = home::default_memory_dir();
    let settings = memory::load_permission_settings(&root);
    let preset = types::PermissionPreset::from_selection(&settings.selection).map(|value| {
        match value {
            types::PermissionPreset::AskForApproval => "ask_for_approval",
            types::PermissionPreset::ApproveForMe => "approve_for_me",
            types::PermissionPreset::ReadOnly => "read_only",
            types::PermissionPreset::FullAccess => "full_access",
        }
        .to_string()
    });
    PermissionSettingsDto {
        preset,
        settings,
        sandbox_health: sandbox::SandboxRunner.probe(),
    }
}

#[tauri::command]
pub async fn get_permission_settings() -> Result<PermissionSettingsDto, String> {
    Ok(permission_settings_dto())
}

#[tauri::command]
pub async fn set_permission_preset(
    preset: String,
    confirmed: bool,
) -> Result<PermissionSettingsDto, String> {
    let preset = match preset.trim().to_ascii_lowercase().as_str() {
        "ask_for_approval" => types::PermissionPreset::AskForApproval,
        "approve_for_me" => types::PermissionPreset::ApproveForMe,
        "read_only" => types::PermissionPreset::ReadOnly,
        "full_access" if confirmed => types::PermissionPreset::FullAccess,
        "full_access" => return Err("启用完全访问需要显式确认".to_string()),
        other => return Err(format!("未知权限组合: {other}")),
    };
    let root = home::default_memory_dir();
    memory::set_permission_preset(&root, preset).map_err(|error| error.to_string())?;
    Ok(permission_settings_dto())
}

/// 向命令白名单追加一条（精确或 glob）。
#[tauri::command]
pub async fn add_command_allowlist(entry: String) -> Result<ApprovalSettingsDto, String> {
    let entry = entry.trim();
    if entry.is_empty() {
        return Err("白名单条目不能为空".to_string());
    }
    let root = home::default_memory_dir();
    Ok(memory::add_command_to_allowlist(&root, entry)
        .map_err(|e| e.to_string())?
        .into())
}

/// 从命令白名单移除一条。
#[tauri::command]
pub async fn remove_command_allowlist(entry: String) -> Result<ApprovalSettingsDto, String> {
    let root = home::default_memory_dir();
    Ok(memory::remove_command_from_allowlist(&root, &entry)
        .map_err(|e| e.to_string())?
        .into())
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

#[cfg(test)]
mod security_audit_tests {
    use super::*;

    #[test]
    fn merged_audits_are_newest_first_and_hide_capability_targets() {
        let dir = tempfile::tempdir().unwrap();
        let permission = memory::PermissionAuditEvent {
            id: "permission-1".into(),
            event: memory::PermissionAuditKind::Denied,
            request_id: "request-1".into(),
            session_id: "session-1".into(),
            turn_id: Some("turn-1".into()),
            tool_call_id: "call-1".into(),
            tool_name: "terminal".into(),
            profile_id: ":workspace".into(),
            snapshot_hash: "hash".into(),
            scope: types::GrantScope::Once,
            reviewer: Some(types::ApprovalsReviewer::User),
            result: Some("denied".into()),
            duration_ms: Some(4),
            capabilities: vec![memory::PermissionAuditCapability {
                kind: "file_write".into(),
                targets: vec!["/secret/project/file.txt".into()],
            }],
            created_at: "2026-08-17T00:00:01+00:00".into(),
        };
        memory::append_permission_audit(dir.path(), &permission).unwrap();
        let sandbox = sandbox::SandboxAuditEvent {
            id: "sandbox-1".into(),
            event: sandbox::SandboxAuditKind::Spawned,
            session_id: Some("session-1".into()),
            turn_id: Some("turn-1".into()),
            tool_name: "code_exec".into(),
            profile_id: ":workspace".into(),
            policy_hash: Some("0".repeat(64)),
            backend: "seatbelt".into(),
            sandboxed: true,
            mode: Some("workspacewrite".into()),
            network_access: false,
            writable_root_count: 1,
            target: "python3".into(),
            result: "spawned".into(),
            duration_ms: Some(2),
            created_at: "2026-08-17T00:00:02+00:00".into(),
        };
        sandbox::append_sandbox_audit(dir.path(), &sandbox).unwrap();

        let events = list_security_audits_from(dir.path(), 10).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].id, "sandbox-1");
        assert_eq!(events[1].capabilities[0].target_count, 1);
        let serialized = serde_json::to_string(&events).unwrap();
        assert!(!serialized.contains("secret/project"));
        assert!(!serialized.contains("targets"));

        let first_page = list_security_audit_page_from(dir.path(), 1, None).unwrap();
        assert_eq!(first_page.items.len(), 1);
        assert_eq!(first_page.items[0].id, "sandbox-1");
        let second_page =
            list_security_audit_page_from(dir.path(), 1, first_page.next_cursor.as_deref())
                .unwrap();
        assert_eq!(second_page.items.len(), 1);
        assert_eq!(second_page.items[0].id, "permission-1");
        assert!(second_page.next_cursor.is_none());

        let export_path = dir.path().join("security-audit.json");
        let exported = export_security_audits_to(dir.path(), &export_path, "all", 10).unwrap();
        assert_eq!(exported.event_count, 2);
        let raw = std::fs::read_to_string(export_path).unwrap();
        assert!(!raw.contains("secret/project"));
        assert!(!raw.contains("\"targets\":"));
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(value["schemaVersion"], 1);
        assert_eq!(value["scope"], "loaded_filtered_events");
        assert_eq!(value["eventCount"], 2);
        assert_eq!(value["events"][0]["id"], "sandbox-1");
        assert_eq!(value["omittedFields"][3], "capability_targets");
        let permission_only =
            collect_security_audits_for_export(dir.path(), "permission", 10).unwrap();
        assert_eq!(permission_only.len(), 1);
        assert_eq!(permission_only[0].source, "permission");
        assert!(collect_security_audits_for_export(dir.path(), "unknown", 10).is_err());
        assert!(export_security_audits_to(
            dir.path(),
            std::path::Path::new("relative.json"),
            "all",
            10,
        )
        .is_err());

        let retention = security_audit_retention();
        assert_eq!(retention.sources[0].source, "permission");
        assert_eq!(retention.sources[1].source, "sandbox");
        assert_eq!(
            retention.max_total_bytes,
            retention.sources[0]
                .max_total_bytes
                .saturating_add(retention.sources[1].max_total_bytes)
        );

        let cleared = clear_security_audits_from(dir.path()).unwrap();
        assert_eq!(cleared.files_removed, 2);
        assert!(cleared.bytes_removed > 0);
        assert!(list_security_audits_from(dir.path(), 10)
            .unwrap()
            .is_empty());
    }
}
