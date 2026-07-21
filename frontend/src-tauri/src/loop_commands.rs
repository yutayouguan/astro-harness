use serde::{Deserialize, Serialize};
use workflow::engine::WorkflowRunResult;
use workflow::model::{NewWorkflow, NodeType, Position, Workflow, WorkflowEdge, WorkflowNode};
use workflow::run_db::{WorkflowRunDb, WorkflowRunRow, WorkflowStepLogRow};
use workflow::store::WorkflowStore;

// ── DTO ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopDto {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub ai_callable: bool,
    pub nodes: Vec<LoopNodeDto>,
    pub edges: Vec<LoopEdgeDto>,
    pub variables: std::collections::HashMap<String, serde_json::Value>,
    pub created_at: String,
    pub updated_at: String,
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
        ai_callable: wf.ai_callable,
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
    }
}

fn from_dto(dto: LoopDto) -> Workflow {
    Workflow {
        id: dto.id,
        name: dto.name,
        description: dto.description,
        enabled: dto.enabled,
        ai_callable: dto.ai_callable,
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
pub async fn set_loop_ai_callable(id: String, callable: bool) -> Result<bool, String> {
    let store = WorkflowStore::open_default().map_err(|e| e.to_string())?;
    store
        .set_ai_callable(&id, callable)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn run_loop(id: String) -> Result<WorkflowRunResult, String> {
    let store = WorkflowStore::open_default().map_err(|e| e.to_string())?;
    let wf = store
        .get(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("workflow {} 不存在", id))?;

    // WorkflowRunDb 含 rusqlite Connection（非 Send），需在 spawn_blocking + current_thread runtime 中执行
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(async {
            let run_db = WorkflowRunDb::open_default().map_err(|e| e.to_string())?;
            workflow::engine::execute_workflow(&wf, serde_json::json!({}), "manual", &run_db)
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
    let db = WorkflowRunDb::open_default().map_err(|e| e.to_string())?;
    db.list_runs(workflow_id.as_deref(), limit.unwrap_or(100))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_loop_run(run_id: String) -> Result<Option<WorkflowRunRow>, String> {
    let db = WorkflowRunDb::open_default().map_err(|e| e.to_string())?;
    db.get_run(&run_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_loop_run(run_id: String) -> Result<bool, String> {
    let db = WorkflowRunDb::open_default().map_err(|e| e.to_string())?;
    db.delete_run(&run_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_loop_step_logs(run_id: String) -> Result<Vec<WorkflowStepLogRow>, String> {
    let db = WorkflowRunDb::open_default().map_err(|e| e.to_string())?;
    db.list_step_logs(&run_id).map_err(|e| e.to_string())
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
pub async fn import_loop(json: String) -> Result<LoopDto, String> {
    let dto: LoopDto = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    let store = WorkflowStore::open_default().map_err(|e| e.to_string())?;
    let mut wf = from_dto(dto);
    wf.id = uuid::Uuid::new_v4().to_string();
    let saved = store.save_workflow(wf).map_err(|e| e.to_string())?;
    Ok(to_dto(saved))
}
