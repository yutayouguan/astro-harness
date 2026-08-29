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
    is_ephemeral: bool,
    /// 该 turn 的完整用户输入，供「在此轮前分支并改写」回填输入框。
    #[serde(skip_serializing_if = "Option::is_none")]
    user_message: Option<String>,
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
    side_count: usize,
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
///
/// 子 Agent 会话与聊天分叉共用 `parent_session_id`，因此它们只以 `agent` 节点出现一次：
/// v22 起靠 `branch_kind` 区分，更早的数据靠 Agent 图里的 session id 兜底剔除。
#[tauri::command]
pub async fn get_chat_branch_graph(session_id: String) -> Result<BranchGraphDto, String> {
    let current_session_id = session_id.trim().to_string();
    if current_session_id.is_empty() {
        return Err("session_id cannot be empty".into());
    }

    let lineage = {
        let store = open_sessions().await?;
        store
            .session_lineage_graph(&current_session_id)
            .await
            .map_err(|error| error.to_string())?
    };

    // Agent 图按会话取快照，因此顺带记下宿主聊天会话；thread.session_id 是 Agent 自己的会话。
    let control = DefaultDesktopAgentThreadControl::new(home::default_memory_dir());
    let mut threads: Vec<(String, subagents::AgentThreadV2)> = Vec::new();
    let mut seen_threads = HashSet::new();
    for session in &lineage.nodes {
        let Ok(snapshot) = control.snapshot(&session.session_id).await else {
            continue;
        };
        for thread in snapshot.threads {
            if thread.canonical_path.as_str() == "/root"
                || matches!(thread.status, subagents::AgentStatusV2::Shutdown)
                || !seen_threads.insert(thread.thread_id.clone())
            {
                continue;
            }
            threads.push((session.session_id.clone(), thread));
        }
    }
    let agent_session_ids = threads
        .iter()
        .map(|(_, thread)| thread.session_id.clone())
        .filter(|id| *id != current_session_id)
        .collect::<HashSet<_>>();

    let (mut nodes, anchors, branch_heads, spawn_anchors, branch_count, side_count) = {
        let store = open_sessions().await?;
        let mut nodes = Vec::new();
        let mut anchors: HashMap<String, Vec<TurnAnchor>> = HashMap::new();
        let mut branch_heads = HashMap::new();
        let mut persistent_branches = 0usize;
        let mut side_count = 0usize;
        let chat_sessions = lineage
            .nodes
            .iter()
            .filter(|node| !agent_session_ids.contains(&node.session_id))
            .collect::<Vec<_>>();

        for session_node in &chat_sessions {
            let metadata = store
                .get_session(&session_node.session_id)
                .await
                .map_err(|error| error.to_string())?;
            let model = metadata.as_ref().and_then(|session| session.model.clone());
            let is_ephemeral = metadata
                .as_ref()
                .is_some_and(|session| session.branch_kind.as_deref() == Some("side"));
            if session_node.parent_session_id.is_some() {
                if is_ephemeral {
                    side_count += 1;
                } else {
                    persistent_branches += 1;
                }
            }
            let title = metadata
                .as_ref()
                .and_then(|session| session.title.clone())
                .unwrap_or_else(|| "Branch".into());
            let messages = store
                .get_messages(&session_node.session_id)
                .await
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
                    Some(if is_ephemeral { "side" } else { "fork" }.into())
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
                    is_ephemeral,
                    user_message: turn.content.clone(),
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
                    edge_kind: session_node.parent_session_id.as_ref().map(|_| {
                        if is_ephemeral {
                            "side".into()
                        } else {
                            "fork".into()
                        }
                    }),
                    title,
                    preview: String::new(),
                    status: if is_ephemeral {
                        "ephemeral".into()
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
                    is_ephemeral,
                    user_message: None,
                });
                branch_heads.insert(session_node.session_id.clone(), id);
            } else if let Some(last) = previous {
                branch_heads.insert(session_node.session_id.clone(), last);
            }
            anchors.insert(session_node.session_id.clone(), session_anchors);
        }

        // spawn 时复制的历史前缀就是 Agent 的发起点，读它的分叉锚点比按创建时间猜更准。
        let mut spawn_anchors = HashMap::new();
        for (_, thread) in &threads {
            let Ok(Some(session)) = store.get_session(&thread.session_id).await else {
                continue;
            };
            if let (Some(parent), Some(message_id)) =
                (session.parent_session_id, session.branch_parent_message_id)
            {
                spawn_anchors.insert(
                    thread.thread_id.clone(),
                    format!("turn:{parent}:{message_id}"),
                );
            }
        }

        (
            nodes,
            anchors,
            branch_heads,
            spawn_anchors,
            persistent_branches,
            side_count,
        )
    };

    let node_ids = nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<HashSet<_>>();
    let agent_count = threads.len();
    for (host_session, thread) in &threads {
        let parent_agent = thread
            .parent_thread_id
            .as_ref()
            .filter(|parent| seen_threads.contains(*parent))
            .map(|parent| format!("agent:{parent}"));
        let persisted_anchor = spawn_anchors
            .get(&thread.thread_id)
            .filter(|id| node_ids.contains(*id))
            .cloned();
        let created_epoch = DateTime::parse_from_rfc3339(&thread.created_at)
            .ok()
            .map(|time| time.timestamp_millis() as f64 / 1000.0);
        let turn_parent = created_epoch.and_then(|created| {
            anchors.get(host_session).and_then(|session_anchors| {
                session_anchors
                    .iter()
                    .rev()
                    .find(|anchor| anchor.timestamp <= created)
                    .map(|anchor| anchor.id.clone())
            })
        });
        let parent_id = parent_agent
            .or(persisted_anchor)
            .or(turn_parent)
            .or_else(|| branch_heads.get(host_session).cloned());
        nodes.push(BranchGraphNodeDto {
            id: format!("agent:{}", thread.thread_id),
            kind: "agent".into(),
            session_id: thread.session_id.clone(),
            parent_id,
            edge_kind: Some("spawn".into()),
            title: thread.task_name.clone(),
            preview: thread.agent_type.clone(),
            status: agent_status(&thread.status).into(),
            created_at: Some(thread.created_at.clone()),
            source_message_id: None,
            turn_index: None,
            model: None,
            agent_path: Some(thread.canonical_path.as_str().to_string()),
            is_current: false,
            can_fork: false,
            is_ephemeral: false,
            user_message: None,
        });
    }

    let turn_count = nodes.iter().filter(|node| node.kind == "turn").count();
    Ok(BranchGraphDto {
        root_session_id: lineage.root_session_id,
        current_session_id,
        branch_count,
        turn_count,
        agent_count,
        side_count,
        nodes,
    })
}
