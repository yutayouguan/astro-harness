//! Tauri IPC 薄封装 → `skills` 领域层

use skills::{
    fetch_detail, install_from_ref, link_skill_to_agent, list_installed_for_agent,
    list_skill_files_ex, load_skill_by_name, update_all_with_origin,
    update_installed_skill as skills_update_installed_skill, InstallOriginHint,
    open_skill_file_externally as open_skill_file_fs, open_skill_folder as open_skill_folder_fs,
    read_skill_file_ex, reveal_skill_file as reveal_skill_file_fs, search, set_enabled_for_agent,
    InstalledSkill, SkillBundle, SkillStoreFilter, SkillUpdateItemResult, StoreSkill,
    StoreSkillDetail,
};
use skills::models::SkillOriginRecord;
use skills::origins::load_origins;
use serde::Serialize;

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
                "workspace".to_string()
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
        Some("default") | None => "workspace".to_string(),
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

/// Tauri 命令：update_installed_skill。
#[tauri::command]
pub async fn update_installed_skill(
    folder: String,
    agent_id: Option<String>,
) -> Result<String, String> {
    let agent = normalize_agent_id(agent_id);
    skills_update_installed_skill(agent.as_deref(), &folder)
        .await
        .map_err(|e| e.to_string())
}

/// Tauri 命令：update_all_skills。
#[tauri::command]
pub async fn update_all_skills(
    agent_id: Option<String>,
) -> Result<Vec<SkillUpdateItemResult>, String> {
    let agent = normalize_agent_id(agent_id);
    update_all_with_origin(agent.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Tauri 命令：list_installed_skills。
#[tauri::command]
pub async fn list_installed_skills(
    agent_id: Option<String>,
    scope: Option<String>,
) -> Result<Vec<InstalledSkill>, String> {
    let id = normalize_agent_id(agent_id);
    let scope = scope
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
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
    let filter = SkillStoreFilter::parse(&store).ok_or_else(|| format!("unknown store: {store}"))?;
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
pub async fn list_skill_bundle(
    name: String,
    id: Option<String>,
) -> Result<SkillBundle, String> {
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
