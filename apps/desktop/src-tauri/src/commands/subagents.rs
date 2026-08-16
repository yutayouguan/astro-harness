use serde::{Deserialize, Serialize};
use tools::AgentThreadDispatch;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListThreadsArgs {
    pub parent_session_id: Option<String>,
    #[serde(default)]
    pub include_closed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadIdArgs {
    pub thread_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageArgs {
    pub thread_id: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadDetail {
    pub thread: subagents::AgentThread,
    pub messages: Vec<subagents::AgentThreadMessage>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDefinitionDto {
    pub name: String,
    pub description: String,
    pub model: Option<String>,
    pub model_reasoning_effort: Option<String>,
}

#[tauri::command]
pub async fn list_subagent_threads(
    args: ListThreadsArgs,
) -> Result<Vec<subagents::AgentThread>, String> {
    let dispatch = agent::exec::dispatch::DefaultAgentThreadDispatch;
    dispatch
        .list_agents(subagents::ListAgentThreadsRequest {
            parent_session_id: args.parent_session_id,
            include_closed: args.include_closed,
        })
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn read_subagent_thread(args: ThreadIdArgs) -> Result<ThreadDetail, String> {
    let dispatch = agent::exec::dispatch::DefaultAgentThreadDispatch;
    let (thread, messages) = dispatch
        .read_agent(args.thread_id.trim())
        .await
        .map_err(|error| error.to_string())?;
    Ok(ThreadDetail { thread, messages })
}

#[tauri::command]
pub async fn send_subagent_message(
    args: SendMessageArgs,
) -> Result<subagents::AgentThread, String> {
    let dispatch = agent::exec::dispatch::DefaultAgentThreadDispatch;
    dispatch
        .send_message(subagents::SendAgentMessageRequest {
            thread_id: args.thread_id,
            message: args.message,
        })
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn interrupt_subagent_thread(
    args: ThreadIdArgs,
) -> Result<subagents::AgentThread, String> {
    let dispatch = agent::exec::dispatch::DefaultAgentThreadDispatch;
    dispatch
        .interrupt_agent(subagents::InterruptAgentRequest {
            thread_id: args.thread_id,
        })
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn close_subagent_thread(args: ThreadIdArgs) -> Result<subagents::AgentThread, String> {
    let dispatch = agent::exec::dispatch::DefaultAgentThreadDispatch;
    dispatch
        .close_agent(subagents::CloseAgentRequest {
            thread_id: args.thread_id,
        })
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_subagent_definitions() -> Result<Vec<AgentDefinitionDto>, String> {
    let memory_dir = home::default_memory_dir();
    let project_root = worktree::resolve_project_root(None);
    let catalog = subagents::load_agent_catalog(&memory_dir, project_root.as_deref());
    Ok(catalog
        .agents
        .into_values()
        .map(|definition| AgentDefinitionDto {
            name: definition.name,
            description: definition.description,
            model: definition.model,
            model_reasoning_effort: definition.model_reasoning_effort,
        })
        .collect())
}
