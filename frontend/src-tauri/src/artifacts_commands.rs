//! Tauri IPC 薄封装 → artifacts 索引（文件空间）

use memory::{
    active_agent_id, default_memory_dir, open_default, ArtifactRow, ArtifactSource, MemoryManager,
    ReconcileReport,
};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Serialize)]
pub struct ArtifactDto {
    pub id: String,
    pub path: String,
    pub name: String,
    pub category: String,
    pub mime: Option<String>,
    pub size: i64,
    pub source: String,
    pub session_id: Option<String>,
    pub message_id: Option<String>,
    pub agent_id: String,
    pub created_at: String,
    pub missing: bool,
}

#[derive(Debug, Serialize)]
pub struct ArtifactSessionGroupDto {
    pub session_id: Option<String>,
    pub session_title: String,
    pub files: Vec<ArtifactDto>,
}

#[derive(Debug, Serialize)]
pub struct ListArtifactsResult {
    pub groups: Vec<ArtifactSessionGroupDto>,
    pub counts: HashMap<String, i64>,
    pub total: i64,
}

#[derive(Debug, Serialize)]
pub struct ReconcileResultDto {
    pub added: u32,
    pub marked_missing: u32,
}

fn map_row(r: ArtifactRow) -> ArtifactDto {
    ArtifactDto {
        id: r.id,
        path: r.path,
        name: r.name,
        category: r.category,
        mime: r.mime,
        size: r.size,
        source: r.source,
        session_id: r.session_id,
        message_id: r.message_id,
        agent_id: r.agent_id,
        created_at: r.created_at,
        missing: r.missing,
    }
}

/// 列出产物数据库条目。
#[tauri::command]
pub async fn list_artifacts(
    category: Option<String>,
    query: Option<String>,
    recent_only: Option<bool>,
    limit: Option<i32>,
    include_missing: Option<bool>,
    agent_id: Option<String>,
) -> Result<ListArtifactsResult, String> {
    let mem = default_memory_dir();
    let db = open_default(&mem).map_err(|e| e.to_string())?;
    let limit = limit.unwrap_or(200).clamp(1, 1000) as usize;
    let agent_filter = agent_id.as_deref();
    let rows = db
        .list(
            category.as_deref(),
            query.as_deref(),
            recent_only.unwrap_or(false),
            limit,
            include_missing.unwrap_or(false),
            agent_filter,
        )
        .map_err(|e| e.to_string())?;

    let counts_vec = db
        .category_counts(false, agent_filter)
        .map_err(|e| e.to_string())?;
    let mut counts: HashMap<String, i64> = counts_vec.into_iter().collect();
    let total: i64 = counts.values().sum();
    counts.insert("all".into(), total);

    let mgr = MemoryManager::new(mem.clone()).map_err(|e| e.to_string())?;
    let recent_sessions = mgr
        .list_recent_sessions(200)
        .map_err(|e| e.to_string())?;
    let title_map: HashMap<String, String> = recent_sessions
        .into_iter()
        .map(|s| {
            let title = s
                .title
                .filter(|t| !t.trim().is_empty())
                .or(s.preview)
                .unwrap_or_default();
            (s.id, title)
        })
        .collect();

    let mut group_map: HashMap<Option<String>, Vec<ArtifactDto>> = HashMap::new();
    for r in rows {
        group_map
            .entry(r.session_id.clone())
            .or_default()
            .push(map_row(r));
    }

    let mut groups: Vec<ArtifactSessionGroupDto> = group_map
        .into_iter()
        .map(|(session_id, files)| {
            let session_title = match &session_id {
                Some(id) => title_map
                    .get(id)
                    .cloned()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| "未命名会话".into()),
                None => "未关联会话".into(),
            };
            ArtifactSessionGroupDto {
                session_id,
                session_title,
                files,
            }
        })
        .collect();
    groups.sort_by(|a, b| {
        let at = a.files.first().map(|f| f.created_at.as_str()).unwrap_or("");
        let bt = b.files.first().map(|f| f.created_at.as_str()).unwrap_or("");
        bt.cmp(at)
    });

    Ok(ListArtifactsResult {
        groups,
        counts,
        total,
    })
}

/// 对账产物库与磁盘文件。
#[tauri::command]
pub async fn reconcile_artifacts() -> Result<ReconcileResultDto, String> {
    let mem = default_memory_dir();
    let db = open_default(&mem).map_err(|e| e.to_string())?;
    let ReconcileReport {
        added,
        marked_missing,
    } = db.reconcile(&mem).map_err(|e| e.to_string())?;
    Ok(ReconcileResultDto {
        added,
        marked_missing,
    })
}

/// Tauri 命令：register_artifact。
#[tauri::command]
pub async fn register_artifact(
    path: String,
    source: String,
    session_id: Option<String>,
    message_id: Option<String>,
    agent_id: Option<String>,
) -> Result<ArtifactDto, String> {
    let src = match source.as_str() {
        "user_upload" => ArtifactSource::UserUpload,
        "agent_write" => ArtifactSource::AgentWrite,
        _ => ArtifactSource::Reconcile,
    };
    let mem = default_memory_dir();
    let db = open_default(&mem).map_err(|e| e.to_string())?;
    let resolved_agent = agent_id
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| active_agent_id(&mem));
    let row = db
        .register(
            &path,
            src,
            session_id.as_deref(),
            message_id.as_deref(),
            Some(&resolved_agent),
        )
        .map_err(|e| e.to_string())?;
    Ok(map_row(row))
}

/// Tauri 命令：save_chat_upload。
#[tauri::command]
pub async fn save_chat_upload(
    session_id: String,
    file_name: String,
    data_base64: String,
    message_id: Option<String>,
) -> Result<ArtifactDto, String> {
    use base64::Engine;

    let mem = default_memory_dir();
    let safe = file_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    if safe.is_empty() || safe == "." || safe == ".." {
        return Err("无效的文件名".into());
    }

    let dir = mem.join("uploads").join(&session_id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(&safe);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64.trim())
        .map_err(|e| e.to_string())?;
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;

    let db = open_default(&mem).map_err(|e| e.to_string())?;
    let agent = active_agent_id(&mem);
    let row = db
        .register(
            path.to_str().ok_or("bad path")?,
            ArtifactSource::UserUpload,
            Some(&session_id),
            message_id.as_deref(),
            Some(&agent),
        )
        .map_err(|e| e.to_string())?;
    Ok(map_row(row))
}

/// Tauri 命令：remove_artifacts_by_paths。
#[tauri::command]
pub async fn remove_artifacts_by_paths(paths: Vec<String>) -> Result<u32, String> {
    let mem = default_memory_dir();
    let db = open_default(&mem).map_err(|e| e.to_string())?;
    let n = db.remove_by_paths(&paths).map_err(|e| e.to_string())?;
    Ok(n as u32)
}
