use std::collections::{HashMap, HashSet};

use agent::exec::dispatch::{DefaultDesktopAgentThreadControl, DesktopAgentThreadControl};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;

use super::common::open_sessions;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchGraphNodeDto {
    id: String,
    kind: String,
    session_id: String,
    parent_id: Option<String>,
    edge_kind: Option<String>,
    title: String,
    preview: String,
    status: String,
    created_at: Option<String>,
    source_message_id: Option<i64>,
    turn_index: Option<i64>,
    model: Option<String>,
    agent_path: Option<String>,
    is_current: bool,
    can_fork: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchGraphDto {
    root_session_id: String,
    current_session_id: String,
    nodes: Vec<BranchGraphNodeDto>,
    branch_count: usize,
    turn_count: usize,
    agent_count: usize,
}

#[derive(Debug, Clone)]
struct TurnAnchor {
    id: String,
    timestamp: f64,
}

fn iso_time(timestamp: f64) -> Option<String> {
    if !timestamp.is_finite() || timestamp < 0.0 {
        return None;
    }
    let seconds = timestamp.floor() as i64;
    let nanos = ((timestamp.fract() * 1_000_000_000.0).round() as u32).min(999_999_999);
    DateTime::<Utc>::from_timestamp(seconds, nanos)
        .map(|time| time.to_rfc3339_opts(SecondsFormat::Secs, true))
}

fn truncate(value: &str, max: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max {
        return compact;
    }
    format!(
        "{}…",
        compact
            .chars()
            .take(max.saturating_sub(1))
            .collect::<String>()
    )
}

fn agent_status(status: &subagents::AgentStatusV2) -> &'static str {
    match status {
        subagents::AgentStatusV2::PendingInit => "pending_init",
        subagents::AgentStatusV2::Running => "running",
        subagents::AgentStatusV2::Interrupted => "interrupted",
        subagents::AgentStatusV2::Completed { .. } => "completed",
        subagents::AgentStatusV2::Errored { .. } => "errored",
        subagents::AgentStatusV2::Shutdown => "shutdown",
    }
}

/// 返回当前会话所在完整聊天谱系，并把子 Agent spawn 图作为独立边类型附着。
#[tauri::command]
pub async fn get_chat_branch_graph(session_id: String) -> Result<BranchGraphDto, String> {
    let current_session_id = session_id.trim().to_string();
    if current_session_id.is_empty() {
        return Err("session_id cannot be empty".into());
    }

    let (lineage, mut nodes, anchors, branch_heads) = {
        let store = open_sessions()?;
        let lineage = store
            .session_lineage_graph(&current_session_id)
            .map_err(|error| error.to_string())?;
        let mut nodes = Vec::new();
        let mut anchors: HashMap<String, Vec<TurnAnchor>> = HashMap::new();
        let mut branch_heads = HashMap::new();

        for session_node in &lineage.nodes {
            let metadata = store
                .get_session(&session_node.session_id)
                .map_err(|error| error.to_string())?;
            let model = metadata.as_ref().and_then(|session| session.model.clone());
            let title = metadata
                .as_ref()
                .and_then(|session| session.title.clone())
                .unwrap_or_else(|| "Branch".into());
            let messages = store
                .get_messages(&session_node.session_id)
                .map_err(|error| error.to_string())?;
            let message_by_id = messages
                .iter()
                .map(|message| (message.id, message))
                .collect::<HashMap<_, _>>();
            let visible_turns = session_node
                .turns
                .iter()
                .skip(session_node.inherited_turn_count.max(0) as usize)
                .collect::<Vec<_>>();
            let fork_parent = session_node.parent_session_id.as_ref().and_then(|parent| {
                session_node
                    .parent_message_id
                    .map(|message| format!("turn:{parent}:{message}"))
            });
            let mut previous = fork_parent.clone();
            let mut session_anchors = Vec::new();

            for (visible_index, turn) in visible_turns.iter().enumerate() {
                let id = format!("turn:{}:{}", session_node.session_id, turn.user_message_id);
                let edge_kind = if visible_index == 0 && session_node.parent_session_id.is_some() {
                    Some("fork".into())
                } else if previous.is_some() {
                    Some("continuation".into())
                } else {
                    None
                };
                let timestamp = message_by_id
                    .get(&turn.user_message_id)
                    .map(|message| message.timestamp)
                    .unwrap_or_default();
                let assistant = messages
                    .iter()
                    .skip_while(|message| message.id != turn.user_message_id)
                    .skip(1)
                    .take_while(|message| message.role != "user")
                    .filter(|message| message.role == "assistant")
                    .filter_map(|message| message.content.as_deref())
                    .find(|content| !content.trim().is_empty())
                    .unwrap_or_default();
                let user = turn.content.as_deref().unwrap_or_default();
                nodes.push(BranchGraphNodeDto {
                    id: id.clone(),
                    kind: "turn".into(),
                    session_id: session_node.session_id.clone(),
                    parent_id: previous.clone(),
                    edge_kind,
                    title: if user.trim().is_empty() {
                        format!("Turn {}", turn.turn_index)
                    } else {
                        truncate(user, 42)
                    },
                    preview: truncate(assistant, 96),
                    status: if turn.completed {
                        "completed".into()
                    } else {
                        "in_progress".into()
                    },
                    created_at: iso_time(timestamp),
                    source_message_id: Some(turn.user_message_id),
                    turn_index: Some(turn.turn_index),
                    model: model.clone(),
                    agent_path: None,
                    is_current: session_node.session_id == current_session_id,
                    can_fork: turn.completed,
                });
                previous = Some(id.clone());
                session_anchors.push(TurnAnchor { id, timestamp });
            }

            if visible_turns.is_empty() || previous == fork_parent {
                let id = format!("head:{}", session_node.session_id);
                nodes.push(BranchGraphNodeDto {
                    id: id.clone(),
                    kind: "branchHead".into(),
                    session_id: session_node.session_id.clone(),
                    parent_id: fork_parent,
                    edge_kind: session_node
                        .parent_session_id
                        .as_ref()
                        .map(|_| "fork".into()),
                    title,
                    preview: String::new(),
                    status: if session_node.legacy_metadata {
                        "legacy".into()
                    } else {
                        "idle".into()
                    },
                    created_at: session_node.branch_created_at.and_then(iso_time),
                    source_message_id: None,
                    turn_index: None,
                    model,
                    agent_path: None,
                    is_current: session_node.session_id == current_session_id,
                    can_fork: false,
                });
                branch_heads.insert(session_node.session_id.clone(), id);
            } else if let Some(last) = previous {
                branch_heads.insert(session_node.session_id.clone(), last);
            }
            anchors.insert(session_node.session_id.clone(), session_anchors);
        }

        (lineage, nodes, anchors, branch_heads)
    };

    let control = DefaultDesktopAgentThreadControl::new(home::default_memory_dir());
    let mut agent_count = 0usize;
    for session in &lineage.nodes {
        let Ok(snapshot) = control.snapshot(&session.session_id).await else {
            continue;
        };
        let threads = snapshot
            .threads
            .into_iter()
            .filter(|thread| {
                thread.canonical_path.as_str() != "/root"
                    && !matches!(thread.status, subagents::AgentStatusV2::Shutdown)
            })
            .collect::<Vec<_>>();
        let thread_ids = threads
            .iter()
            .map(|thread| thread.thread_id.clone())
            .collect::<HashSet<_>>();
        for thread in threads {
            let id = format!("agent:{}", thread.thread_id);
            let parent_agent = thread
                .parent_thread_id
                .as_ref()
                .filter(|parent| thread_ids.contains(*parent))
                .map(|parent| format!("agent:{parent}"));
            let created_epoch = DateTime::parse_from_rfc3339(&thread.created_at)
                .ok()
                .map(|time| time.timestamp_millis() as f64 / 1000.0);
            let turn_parent = created_epoch.and_then(|created| {
                anchors
                    .get(&session.session_id)
                    .and_then(|session_anchors| {
                        session_anchors
                            .iter()
                            .rev()
                            .find(|anchor| anchor.timestamp <= created)
                            .map(|anchor| anchor.id.clone())
                    })
            });
            let parent_id = parent_agent
                .or(turn_parent)
                .or_else(|| branch_heads.get(&session.session_id).cloned());
            nodes.push(BranchGraphNodeDto {
                id,
                kind: "agent".into(),
                session_id: thread.session_id,
                parent_id,
                edge_kind: Some("spawn".into()),
                title: thread.task_name,
                preview: thread.agent_type,
                status: agent_status(&thread.status).into(),
                created_at: Some(thread.created_at),
                source_message_id: None,
                turn_index: None,
                model: None,
                agent_path: Some(thread.canonical_path.as_str().to_string()),
                is_current: false,
                can_fork: false,
            });
            agent_count += 1;
        }
    }

    let turn_count = nodes.iter().filter(|node| node.kind == "turn").count();
    Ok(BranchGraphDto {
        root_session_id: lineage.root_session_id,
        current_session_id,
        branch_count: lineage.nodes.len().saturating_sub(1),
        turn_count,
        agent_count,
        nodes,
    })
}
