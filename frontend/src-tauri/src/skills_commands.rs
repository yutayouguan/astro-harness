//! Tauri IPC 薄封装 → `skills` 领域层

use serde::Serialize;
use chrono;
use skills::models::SkillOriginRecord;
use skills::origins::load_origins;
use skills::{
    check_updates_for_agent, fetch_detail, install_from_ref, link_skill_to_agent,
    list_installed_for_agent, list_skill_backups as skills_list_skill_backups, list_skill_files_ex,
    load_skill_by_name, open_skill_file_externally as open_skill_file_fs,
    open_skill_folder as open_skill_folder_fs, preview_skill_update as skills_preview_skill_update,
    read_skill_file_ex, reveal_skill_backup as skills_reveal_skill_backup,
    reveal_skill_file as reveal_skill_file_fs, search, set_enabled_for_agent,
    update_all_with_origin, update_installed_skill_ex, update_outdated_skills, InstallOriginHint,
    InstalledSkill, SkillBackupEntry, SkillBundle, SkillStoreFilter, SkillUpdateCheckResult,
    SkillUpdateItemResult, SkillUpdatePreview, SkillSnapshot, StoreSkill, StoreSkillDetail,
    UpdateSkillOpts,
};

#[derive(Serialize)]
pub struct SkillMetadataDto {
    pub name: String,
    pub description: String,
    pub version: String,
}

#[derive(Serialize)]
pub struct SkillContentDto {
    pub metadata: SkillMetadataDto,
    pub content: String,
}

fn normalize_agent_id(agent_id: Option<String>) -> Option<String> {
    agent_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s == "default" {
                "default".to_string()
            } else {
                s
            }
        })
}

fn origin_agent_id(record: &SkillOriginRecord) -> String {
    match record
        .agent_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some("default") | None => "default".to_string(),
        Some(id) => id.to_string(),
    }
}

/// Tauri 命令：list_skill_origins。
#[tauri::command]
pub async fn list_skill_origins(
    agent_id: Option<String>,
) -> Result<Vec<SkillOriginRecord>, String> {
    let file = load_origins().map_err(|e| e.to_string())?;
    let filter_agent = normalize_agent_id(agent_id);
    let records = match filter_agent.as_deref() {
        Some(target) => file
            .records
            .into_iter()
            .filter(|r| origin_agent_id(r) == target)
            .collect(),
        None => file.records,
    };
    Ok(records)
}

/// Tauri 命令：preview_skill_update。
#[tauri::command]
pub fn preview_skill_update(
    folder: String,
    agent_id: Option<String>,
) -> Result<SkillUpdatePreview, String> {
    let agent = normalize_agent_id(agent_id);
    skills_preview_skill_update(agent.as_deref(), &folder).map_err(|e| e.to_string())
}

/// Tauri 命令：update_installed_skill。
#[tauri::command]
pub async fn update_installed_skill(
    folder: String,
    agent_id: Option<String>,
    force: Option<bool>,
    backup_if_dirty: Option<bool>,
) -> Result<String, String> {
    let agent = normalize_agent_id(agent_id);
    update_installed_skill_ex(
        agent.as_deref(),
        &folder,
        UpdateSkillOpts {
            force: force.unwrap_or(true),
            backup_if_dirty: backup_if_dirty.unwrap_or(true),
            max_retries: 1,
        },
    )
    .await
    .map_err(|e| e.to_string())
}

/// Tauri 命令：check_skill_updates。
#[tauri::command]
pub async fn check_skill_updates(
    agent_id: Option<String>,
) -> Result<Vec<SkillUpdateCheckResult>, String> {
    let agent = normalize_agent_id(agent_id);
    check_updates_for_agent(agent.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Tauri 命令：update_all_skills。
#[tauri::command]
pub async fn update_all_skills(
    agent_id: Option<String>,
    only_outdated: Option<bool>,
) -> Result<Vec<SkillUpdateItemResult>, String> {
    let agent = normalize_agent_id(agent_id);
    if only_outdated.unwrap_or(false) {
        update_outdated_skills(agent.as_deref())
            .await
            .map_err(|e| e.to_string())
    } else {
        update_all_with_origin(agent.as_deref())
            .await
            .map_err(|e| e.to_string())
    }
}

/// Tauri 命令：list_installed_skills。
#[tauri::command]
pub async fn list_installed_skills(
    agent_id: Option<String>,
    scope: Option<String>,
) -> Result<Vec<InstalledSkill>, String> {
    let id = normalize_agent_id(agent_id);
    let scope = scope.as_deref().map(str::trim).filter(|s| !s.is_empty());
    Ok(list_installed_for_agent(id.as_deref(), scope))
}

/// Tauri 命令：search_store_skills。
#[tauri::command]
pub async fn search_store_skills(
    query: String,
    store: String,
    limit: Option<usize>,
    page: Option<usize>,
) -> Result<Vec<StoreSkill>, String> {
    let filter =
        SkillStoreFilter::parse(&store).ok_or_else(|| format!("unknown store: {store}"))?;
    let limit = limit.unwrap_or(24);
    let page = page.unwrap_or(1);
    search(&query, filter, limit, page)
        .await
        .map_err(|e| e.to_string())
}

/// Tauri 命令：get_store_skill_detail。
#[tauri::command]
pub async fn get_store_skill_detail(skill: StoreSkill) -> Result<StoreSkillDetail, String> {
    fetch_detail(&skill).await.map_err(|e| e.to_string())
}

/// 启用/禁用 Skill。
#[tauri::command]
pub async fn set_skill_enabled(
    id: String,
    enabled: bool,
    agent_id: Option<String>,
) -> Result<(), String> {
    let agent = normalize_agent_id(agent_id);
    set_enabled_for_agent(agent.as_deref(), &id, enabled).map_err(|e| e.to_string())
}

/// Tauri 命令：link_machine_skill。
#[tauri::command]
pub async fn link_machine_skill(
    id: String,
    linked: bool,
    agent_id: Option<String>,
) -> Result<(), String> {
    let agent = normalize_agent_id(agent_id);
    link_skill_to_agent(agent.as_deref(), &id, linked).map_err(|e| e.to_string())
}

/// Tauri 命令：install_store_skill。
#[tauri::command]
pub async fn install_store_skill(
    install_ref: String,
    agent_id: Option<String>,
    name: Option<String>,
    store: Option<String>,
    folder: Option<String>,
) -> Result<String, String> {
    let agent = normalize_agent_id(agent_id);
    let hint = InstallOriginHint {
        name,
        store,
        folder,
    };
    install_from_ref(&install_ref, agent.as_deref(), Some(hint))
        .await
        .map_err(|e| e.to_string())
}

/// Tauri 命令：get_skill_content。
#[tauri::command]
pub async fn get_skill_content(name: String) -> Result<SkillContentDto, String> {
    let skill = load_skill_by_name(&name).map_err(|e| e.to_string())?;
    Ok(SkillContentDto {
        metadata: SkillMetadataDto {
            name: skill.metadata.name,
            description: skill.metadata.description,
            version: String::new(),
        },
        content: skill.content,
    })
}

/// 列出技能目录下的全部相关文件。
#[tauri::command]
pub async fn list_skill_bundle(name: String, id: Option<String>) -> Result<SkillBundle, String> {
    list_skill_files_ex(&name, id.as_deref()).map_err(|e| e.to_string())
}

/// 读取技能目录内某个相对路径的文本文件。
#[tauri::command]
pub async fn get_skill_file(
    name: String,
    relative_path: String,
    id: Option<String>,
) -> Result<String, String> {
    read_skill_file_ex(&name, &relative_path, id.as_deref()).map_err(|e| e.to_string())
}

/// 打开技能根目录。
#[tauri::command]
pub async fn open_skill_folder(name: String, id: Option<String>) -> Result<(), String> {
    open_skill_folder_fs(&name, id.as_deref()).map_err(|e| e.to_string())
}

/// 在文件管理器中显示技能内某个文件。
#[tauri::command]
pub async fn reveal_skill_file(
    name: String,
    relative_path: String,
    id: Option<String>,
) -> Result<(), String> {
    reveal_skill_file_fs(&name, &relative_path, id.as_deref()).map_err(|e| e.to_string())
}

/// 用系统默认应用打开技能内文件。
#[tauri::command]
pub async fn open_skill_file(
    name: String,
    relative_path: String,
    id: Option<String>,
) -> Result<(), String> {
    open_skill_file_fs(&name, &relative_path, id.as_deref()).map_err(|e| e.to_string())
}

/// 列举技能更新备份（可选按 Agent 过滤）。
#[tauri::command]
pub fn list_skill_backups(agent_id: Option<String>) -> Result<Vec<SkillBackupEntry>, String> {
    let agent = normalize_agent_id(agent_id);
    skills_list_skill_backups(agent.as_deref()).map_err(|e| e.to_string())
}

/// 在文件管理器中显示备份目录。
#[tauri::command]
pub fn reveal_skill_backup(path: String) -> Result<(), String> {
    skills_reveal_skill_backup(&path).map_err(|e| e.to_string())
}

// ─── [P0] 冷却查询 ────────────────────────────────────────────────────────────

/// 查询某 skill 距离批准冷却结束还剩多少秒。None = 无冷却（可立即进化）。
#[tauri::command]
pub async fn get_skill_cooldown_remaining(skill_id: String) -> Result<Option<u64>, String> {
    use evolution::skill_last_approved_at;
    use home::default_memory_dir;
    let base = default_memory_dir();
    let cfg = memory::load_evolution_config(&base);
    let cooldown = cfg.search.post_approval_cooldown_secs;
    if cooldown == 0 {
        return Ok(None);
    }
    let Some(last_ts) = skill_last_approved_at(&base, &skill_id) else {
        return Ok(None);
    };
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&last_ts) else {
        return Ok(None);
    };
    let elapsed =
        chrono::Utc::now().signed_duration_since(dt.with_timezone(&chrono::Utc));
    let remain = cooldown as i64 - elapsed.num_seconds();
    if remain > 0 {
        Ok(Some(remain as u64))
    } else {
        Ok(None)
    }
}

// ─── [P1] 快照管理 ───────────────────────────────────────────────────────────

/// 列出某 skill 的所有快照（按时间倒序，最新在前）。
#[tauri::command]
pub async fn list_skill_snapshots(skill_id: String) -> Result<Vec<SkillSnapshot>, String> {
    let Some(loaded) = load_skill_by_name(&skill_id).ok() else {
        return Ok(Vec::new());
    };
    let Some(skill_dir) = std::path::Path::new(&loaded.path).parent() else {
        return Ok(Vec::new());
    };
    Ok(skills::list_snapshots(skill_dir))
}

/// 恢复某 skill 的最新快照（覆盖当前 SKILL.md）。
#[tauri::command]
pub async fn restore_skill_snapshot(skill_id: String) -> Result<(), String> {
    let loaded = load_skill_by_name(&skill_id)
        .map_err(|e| format!("技能不存在：{e}"))?;
    let skill_dir = std::path::Path::new(&loaded.path)
        .parent()
        .ok_or("无法确定技能目录")?;
    let restored = skills::restore_skill_snapshot(skill_dir).map_err(|e| e.to_string())?;
    if restored {
        Ok(())
    } else {
        Err("该技能没有可用的快照".into())
    }
}

// ─── [P2] Signal 摘要 ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSignalDto {
    pub skill_id: String,
    pub failure_signals: usize,
}

/// 返回当前所有启用技能的失败信号摘要（只返回 ≥1 的）。
#[tauri::command]
pub async fn get_skill_signal_summary() -> Result<Vec<SkillSignalDto>, String> {
    use evolution::skill_failure_signals;
    use home::default_memory_dir;
    let base = default_memory_dir();
    let decisions = memory::list_recent_decisions(&base, 200).unwrap_or_default();
    let known: Vec<String> = skills::list_enabled_for_prompt()
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    let counts = skill_failure_signals(&decisions, &known);
    let mut result: Vec<SkillSignalDto> = counts
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .map(|(skill_id, failure_signals)| SkillSignalDto { skill_id, failure_signals })
        .collect();
    result.sort_by(|a, b| b.failure_signals.cmp(&a.failure_signals));
    Ok(result)
}
