//! 聊天分支创建、旧元数据推断与递归谱系图查询。

use std::collections::{HashMap, HashSet, VecDeque};

use agent_db::sqlx::{self, Row};
use anyhow::{anyhow, Result};

use super::{
    is_unique_constraint, now_epoch_secs, truncate_chars, BranchKind, ForkBoundary, ForkedSession,
    SessionLineageGraph, SessionLineageNode, SessionStore, SessionTurnNode, StoredMessage,
    StoredSession,
};

#[derive(Debug)]
struct TurnSpan {
    index: i64,
    user_message_id: i64,
    content: Option<String>,
    completed: bool,
    start: usize,
    end: usize,
}

impl SessionStore {
    /// 从父会话的一个已完成 user turn 分叉（`last_turn_id` 语义）。
    ///
    /// `parent_user_message_id` 必须属于 `source_id`，且该 turn 在下一条 user 消息前
    /// 至少有一条 assistant 消息。复制范围从会话开头到该 turn 末尾，消息使用新行 id。
    pub async fn fork_session_at_user_message(
        &self,
        source_id: &str,
        new_id: &str,
        parent_user_message_id: i64,
    ) -> Result<ForkedSession> {
        self.fork_session_at_boundary(
            source_id,
            new_id,
            parent_user_message_id,
            ForkBoundary::ThroughTurn,
            BranchKind::Fork,
        )
        .await
    }

    /// 从父会话的一个已完成 user turn 创建临时 Side 分支。
    ///
    /// Side 仍复制完整前缀供模型使用，但会写入 `branch_kind = 'side'`，
    /// 供列表过滤、UI 隐藏继承回合和安全丢弃使用。
    pub async fn fork_side_session_at_user_message(
        &self,
        source_id: &str,
        new_id: &str,
        parent_user_message_id: i64,
    ) -> Result<ForkedSession> {
        self.fork_session_at_boundary(
            source_id,
            new_id,
            parent_user_message_id,
            ForkBoundary::ThroughTurn,
            BranchKind::Side,
        )
        .await
    }

    /// 分叉到某个 user turn 之前（`before_turn_id` 语义）。
    ///
    /// 锚点 turn 本身不进入新分支，因此未完成的 turn 也可作为锚点——这正是
    /// 「改写上一条消息重开一条路径」需要的边界。
    pub async fn fork_session_before_user_message(
        &self,
        source_id: &str,
        new_id: &str,
        parent_user_message_id: i64,
    ) -> Result<ForkedSession> {
        self.fork_session_at_boundary(
            source_id,
            new_id,
            parent_user_message_id,
            ForkBoundary::BeforeTurn,
            BranchKind::Fork,
        )
        .await
    }

    /// 在指定 user turn 之前创建临时 Side 分支。
    pub async fn fork_side_session_before_user_message(
        &self,
        source_id: &str,
        new_id: &str,
        parent_user_message_id: i64,
    ) -> Result<ForkedSession> {
        self.fork_session_at_boundary(
            source_id,
            new_id,
            parent_user_message_id,
            ForkBoundary::BeforeTurn,
            BranchKind::Side,
        )
        .await
    }

    async fn fork_session_at_boundary(
        &self,
        source_id: &str,
        new_id: &str,
        parent_user_message_id: i64,
        boundary: ForkBoundary,
        kind: BranchKind,
    ) -> Result<ForkedSession> {
        anyhow::ensure!(
            source_id != new_id,
            "fork_session: source and target session ids must differ"
        );
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query("SELECT source, model, title FROM sessions WHERE id = ?1")
            .bind(source_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| anyhow!("fork_session: source session not found: {source_id:?}"))?;
        let source: String = row.get(0);
        let model: Option<String> = row.get(1);
        let title: Option<String> = row.get(2);

        let target_exists: bool =
            sqlx::query("SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)")
                .bind(new_id)
                .fetch_one(&mut *tx)
                .await?
                .get(0);
        anyhow::ensure!(
            !target_exists,
            "fork_session: target session already exists"
        );

        let anchor_row = sqlx::query(
            "SELECT timestamp, id, role FROM messages
             WHERE id = ?1 AND session_id = ?2",
        )
        .bind(parent_user_message_id)
        .bind(source_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| anyhow!("fork_session: anchor message not found in source session"))?;
        let anchor_timestamp: f64 = anchor_row.get(0);
        let anchor_id: i64 = anchor_row.get(1);
        let role: String = anchor_row.get(2);
        anyhow::ensure!(
            role == "user",
            "fork_session: anchor message must have role=user"
        );

        let next_user = sqlx::query(
            "SELECT timestamp, id FROM messages
             WHERE session_id = ?1 AND role = 'user'
               AND (timestamp > ?2 OR (timestamp = ?2 AND id > ?3))
             ORDER BY timestamp ASC, id ASC LIMIT 1",
        )
        .bind(source_id)
        .bind(anchor_timestamp)
        .bind(anchor_id)
        .fetch_optional(&mut *tx)
        .await?
        .map(|r| (r.get::<f64, _>(0), r.get::<i64, _>(1)));

        let copy_boundary = match boundary {
            ForkBoundary::ThroughTurn => next_user,
            ForkBoundary::BeforeTurn => Some((anchor_timestamp, anchor_id)),
        };

        if boundary == ForkBoundary::ThroughTurn {
            let completed = match next_user {
                Some((next_timestamp, next_id)) => sqlx::query(
                    "SELECT EXISTS(
                            SELECT 1 FROM messages
                            WHERE session_id = ?1 AND role = 'assistant'
                              AND (timestamp > ?2 OR (timestamp = ?2 AND id > ?3))
                              AND (timestamp < ?4 OR (timestamp = ?4 AND id < ?5))
                         )",
                )
                .bind(source_id)
                .bind(anchor_timestamp)
                .bind(anchor_id)
                .bind(next_timestamp)
                .bind(next_id)
                .fetch_one(&mut *tx)
                .await?
                .get::<bool, _>(0),
                None => sqlx::query(
                    "SELECT EXISTS(
                            SELECT 1 FROM messages
                            WHERE session_id = ?1 AND role = 'assistant'
                              AND (timestamp > ?2 OR (timestamp = ?2 AND id > ?3))
                         )",
                )
                .bind(source_id)
                .bind(anchor_timestamp)
                .bind(anchor_id)
                .fetch_one(&mut *tx)
                .await?
                .get::<bool, _>(0),
            };
            anyhow::ensure!(completed, "fork_session: user turn is not completed");
        }

        let anchor = match boundary {
            ForkBoundary::ThroughTurn => {
                let turn_index: i64 = sqlx::query(
                    "SELECT COUNT(*) FROM messages
                     WHERE session_id = ?1 AND role = 'user'
                       AND (timestamp < ?2 OR (timestamp = ?2 AND id <= ?3))",
                )
                .bind(source_id)
                .bind(anchor_timestamp)
                .bind(anchor_id)
                .fetch_one(&mut *tx)
                .await?
                .get(0);
                Some((parent_user_message_id, turn_index))
            }
            ForkBoundary::BeforeTurn => sqlx::query(
                "SELECT id, (
                        SELECT COUNT(*) FROM messages inner_m
                        WHERE inner_m.session_id = outer_m.session_id AND inner_m.role = 'user'
                          AND (inner_m.timestamp < outer_m.timestamp
                               OR (inner_m.timestamp = outer_m.timestamp
                                   AND inner_m.id <= outer_m.id))
                     ) FROM messages outer_m
                     WHERE outer_m.session_id = ?1 AND outer_m.role = 'user'
                       AND (outer_m.timestamp < ?2 OR (outer_m.timestamp = ?2 AND outer_m.id < ?3))
                     ORDER BY outer_m.timestamp DESC, outer_m.id DESC LIMIT 1",
            )
            .bind(source_id)
            .bind(anchor_timestamp)
            .bind(anchor_id)
            .fetch_optional(&mut *tx)
            .await?
            .map(|r| (r.get::<i64, _>(0), r.get::<i64, _>(1))),
        };

        let inherited_turn_count =
            completed_turn_count_before_boundary(&mut tx, source_id, copy_boundary).await?;
        let created_at = now_epoch_secs()?;
        sqlx::query(
            "INSERT INTO sessions (
                id, source, model, parent_session_id, started_at,
                branch_parent_message_id, branch_parent_turn_index,
                branch_inherited_turn_count, branch_created_at, branch_kind
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?5, ?9)",
        )
        .bind(new_id)
        .bind(&source)
        .bind(&model)
        .bind(source_id)
        .bind(created_at)
        .bind(anchor.map(|(message_id, _)| message_id))
        .bind(anchor.map(|(_, turn_index)| turn_index))
        .bind(inherited_turn_count)
        .bind(kind.as_str())
        .execute(&mut *tx)
        .await?;

        let copied_message_count =
            copy_prefix_through_turn(&mut tx, source_id, new_id, copy_boundary).await?;
        let counts_row = sqlx::query(
            "SELECT SUM(CASE WHEN role = 'tool' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN role = 'user' THEN 1 ELSE 0 END)
             FROM messages WHERE session_id = ?1",
        )
        .bind(new_id)
        .fetch_one(&mut *tx)
        .await?;
        let tool_call_count: i64 = counts_row.get::<Option<i64>, _>(0).unwrap_or(0);
        let copied_user_turns: i64 = counts_row.get::<Option<i64>, _>(1).unwrap_or(0);

        sqlx::query("UPDATE sessions SET message_count = ?1, tool_call_count = ?2 WHERE id = ?3")
            .bind(copied_message_count)
            .bind(tool_call_count)
            .bind(new_id)
            .execute(&mut *tx)
            .await?;
        set_branch_title(&mut tx, new_id, title).await?;
        tx.commit().await?;

        Ok(ForkedSession {
            session_id: new_id.to_string(),
            parent_session_id: source_id.to_string(),
            parent_message_id: anchor.map(|(message_id, _)| message_id),
            parent_turn_index: anchor.map(|(_, turn_index)| turn_index),
            inherited_turn_count,
            copied_message_count,
            copied_user_turns,
            created_at,
        })
    }

    /// 删除上次进程遗留的 Side 会话，保留已从它派生的持久分支。
    pub async fn delete_stale_side_sessions(&self) -> Result<usize> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query(
            "UPDATE sessions SET parent_session_id = NULL
             WHERE parent_session_id IN (
                 SELECT id FROM sessions WHERE branch_kind = 'side'
             )",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "DELETE FROM messages WHERE session_id IN (
                 SELECT id FROM sessions WHERE branch_kind = 'side'
             )",
        )
        .execute(&mut *tx)
        .await?;
        let deleted = sqlx::query("DELETE FROM sessions WHERE branch_kind = 'side'")
            .execute(&mut *tx)
            .await?
            .rows_affected();
        tx.commit().await?;
        Ok(usize::try_from(deleted).unwrap_or(usize::MAX))
    }

    /// 查询任意会话所在的完整聊天分支谱系：向上找到可达根，再递归收集全部后代。
    ///
    /// `branch_kind = 'agent'` 的会话（子 Agent 派生）连同其子树被剔除——它们共用
    /// `parent_session_id` 但不属于聊天分支；仅当请求会话本身在这条链上时才保留。
    /// v21 及更早创建的分支会通过父/子消息前缀推断锚点；损坏的父引用与循环不会死循环，
    /// 而是分别写入 `orphaned_parent_ids` / `cycle_detected`。
    pub async fn session_lineage_graph(&self, session_id: &str) -> Result<SessionLineageGraph> {
        let sessions = self.load_all_sessions().await?;
        anyhow::ensure!(
            sessions.contains_key(session_id),
            "session_lineage_graph: session not found"
        );

        let mut ancestry: Vec<String> = Vec::new();
        let mut ancestry_positions: HashMap<String, usize> = HashMap::new();
        let mut current = session_id.to_string();
        let mut cycle_detected = false;
        let mut orphaned_parent_ids = Vec::new();
        let root_session_id = loop {
            if let Some(position) = ancestry_positions.get(&current).copied() {
                cycle_detected = true;
                let mut cycle = ancestry[position..].to_vec();
                cycle.sort();
                break cycle[0].clone();
            }
            ancestry_positions.insert(current.clone(), ancestry.len());
            ancestry.push(current.clone());
            let parent = sessions[&current].parent_session_id.clone();
            match parent {
                None => break current,
                Some(parent_id) if sessions.contains_key(&parent_id) => current = parent_id,
                Some(parent_id) => {
                    orphaned_parent_ids.push(parent_id);
                    break current;
                }
            }
        };

        let on_requested_path = ancestry.iter().cloned().collect::<HashSet<_>>();
        let mut children: HashMap<String, Vec<String>> = HashMap::new();
        for session in sessions.values() {
            let derived_agent = session.branch_kind.as_deref() == Some(BranchKind::Agent.as_str())
                && !on_requested_path.contains(&session.id);
            if derived_agent {
                continue;
            }
            if let Some(parent_id) = &session.parent_session_id {
                if sessions.contains_key(parent_id) {
                    children
                        .entry(parent_id.clone())
                        .or_default()
                        .push(session.id.clone());
                }
            }
        }
        for ids in children.values_mut() {
            ids.sort();
        }

        let mut queue = VecDeque::from([root_session_id.clone()]);
        let mut included = Vec::new();
        let mut seen = HashSet::new();
        while let Some(id) = queue.pop_front() {
            if !seen.insert(id.clone()) {
                cycle_detected = true;
                continue;
            }
            included.push(id.clone());
            if let Some(child_ids) = children.get(&id) {
                queue.extend(child_ids.iter().cloned());
            }
        }

        let mut resolved = HashMap::new();
        for id in &included {
            let session = &sessions[id];
            resolved.insert(
                id.clone(),
                self.resolve_branch_metadata(session, &sessions).await?,
            );
        }

        let mut nodes = Vec::with_capacity(included.len());
        for id in included {
            let session = &sessions[&id];
            let mut turns = turn_spans(&self.get_messages(&id).await?)
                .into_iter()
                .map(|turn| SessionTurnNode {
                    turn_index: turn.index,
                    user_message_id: turn.user_message_id,
                    content: turn.content,
                    completed: turn.completed,
                    child_session_ids: Vec::new(),
                })
                .collect::<Vec<_>>();
            if let Some(child_ids) = children.get(&id) {
                for child_id in child_ids {
                    let metadata = &resolved[child_id];
                    if let Some(parent_message_id) = metadata.parent_message_id {
                        if let Some(turn) = turns
                            .iter_mut()
                            .find(|turn| turn.user_message_id == parent_message_id)
                        {
                            turn.child_session_ids.push(child_id.clone());
                        }
                    }
                }
            }
            let metadata = &resolved[&id];
            let orphaned = session
                .parent_session_id
                .as_ref()
                .is_some_and(|parent| !sessions.contains_key(parent));
            if orphaned {
                if let Some(parent) = &session.parent_session_id {
                    orphaned_parent_ids.push(parent.clone());
                }
            }
            nodes.push(SessionLineageNode {
                session_id: id,
                parent_session_id: session.parent_session_id.clone(),
                parent_message_id: metadata.parent_message_id,
                parent_turn_index: metadata.parent_turn_index,
                inherited_turn_count: metadata.inherited_turn_count,
                branch_created_at: session.branch_created_at,
                orphaned,
                turns,
            });
        }
        orphaned_parent_ids.sort();
        orphaned_parent_ids.dedup();

        Ok(SessionLineageGraph {
            requested_session_id: session_id.to_string(),
            root_session_id,
            nodes,
            cycle_detected,
            orphaned_parent_ids,
        })
    }

    pub(crate) async fn write_fork_metadata_from_messages(
        &self,
        source_id: &str,
        target_id: &str,
        copied_source_messages: &[StoredMessage],
        kind: BranchKind,
    ) -> Result<()> {
        let turns = turn_spans(copied_source_messages);
        let completed = turns
            .iter()
            .filter(|turn| turn.completed)
            .collect::<Vec<_>>();
        let anchor = completed.last().copied();
        self.write_branch_metadata(
            source_id,
            target_id,
            kind,
            &ResolvedBranchMetadata {
                parent_message_id: anchor.map(|turn| turn.user_message_id),
                parent_turn_index: anchor.map(|turn| turn.index),
                inherited_turn_count: i64::try_from(completed.len()).unwrap_or(i64::MAX),
            },
        )
        .await
    }

    pub(crate) async fn infer_and_write_branch_metadata(
        &self,
        source_id: &str,
        target_id: &str,
        kind: BranchKind,
    ) -> Result<()> {
        let inferred = infer_branch_metadata(
            &self.get_messages(source_id).await?,
            &self.get_messages(target_id).await?,
        );
        self.write_branch_metadata(source_id, target_id, kind, &inferred)
            .await
    }

    async fn write_branch_metadata(
        &self,
        source_id: &str,
        target_id: &str,
        kind: BranchKind,
        metadata: &ResolvedBranchMetadata,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE sessions SET
                branch_parent_message_id = ?1,
                branch_parent_turn_index = ?2,
                branch_inherited_turn_count = ?3,
                branch_created_at = COALESCE(branch_created_at, ?4),
                branch_kind = ?5
             WHERE id = ?6 AND parent_session_id = ?7",
        )
        .bind(metadata.parent_message_id)
        .bind(metadata.parent_turn_index)
        .bind(metadata.inherited_turn_count)
        .bind(now_epoch_secs()?)
        .bind(kind.as_str())
        .bind(target_id)
        .bind(source_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn load_all_sessions(&self) -> Result<HashMap<String, StoredSession>> {
        let ids: Vec<(String,)> = sqlx::query_as("SELECT id FROM sessions ORDER BY started_at, id")
            .fetch_all(&self.pool)
            .await?;
        let mut sessions = HashMap::with_capacity(ids.len());
        for (id,) in ids {
            if let Some(session) = self.get_session(&id).await? {
                sessions.insert(id, session);
            }
        }
        Ok(sessions)
    }

    async fn resolve_branch_metadata(
        &self,
        session: &StoredSession,
        sessions: &HashMap<String, StoredSession>,
    ) -> Result<ResolvedBranchMetadata> {
        if session.parent_session_id.is_none() {
            return Ok(ResolvedBranchMetadata::default());
        }
        if session.branch_kind.is_some()
            && session.branch_inherited_turn_count.is_some()
            && session.branch_created_at.is_some()
        {
            return Ok(ResolvedBranchMetadata {
                parent_message_id: session.branch_parent_message_id,
                parent_turn_index: session.branch_parent_turn_index,
                inherited_turn_count: session.branch_inherited_turn_count.unwrap_or(0),
            });
        }
        let parent_id = session.parent_session_id.as_deref().unwrap_or_default();
        let parent_state = if sessions.contains_key(parent_id) {
            "present"
        } else {
            "missing"
        };
        anyhow::bail!(
            "session {:?} has parent {:?} ({parent_state}) but no canonical branch metadata",
            session.id,
            parent_id
        )
    }
}

#[derive(Debug, Default)]
struct ResolvedBranchMetadata {
    parent_message_id: Option<i64>,
    parent_turn_index: Option<i64>,
    inherited_turn_count: i64,
}

async fn copy_prefix_through_turn(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    source_id: &str,
    new_id: &str,
    next_user: Option<(f64, i64)>,
) -> Result<i64> {
    let copied = match next_user {
        Some((timestamp, id)) => {
            sqlx::query(
                "INSERT INTO messages (
                    session_id, role, content, compressed_content, tool_call_id, tool_calls, tool_name,
                    timestamp, token_count, finish_reason, reasoning, reasoning_details, media_json
                 )
                 SELECT ?1, role, content, compressed_content, tool_call_id, tool_calls, tool_name,
                        timestamp, token_count, finish_reason, reasoning, reasoning_details, media_json
                 FROM messages WHERE session_id = ?2
                 AND (timestamp < ?3 OR (timestamp = ?3 AND id < ?4))
                 ORDER BY timestamp ASC, id ASC",
            )
            .bind(new_id)
            .bind(source_id)
            .bind(timestamp)
            .bind(id)
            .execute(&mut **tx)
            .await?
            .rows_affected()
        }
        None => {
            sqlx::query(
                "INSERT INTO messages (
                    session_id, role, content, compressed_content, tool_call_id, tool_calls, tool_name,
                    timestamp, token_count, finish_reason, reasoning, reasoning_details, media_json
                 )
                 SELECT ?1, role, content, compressed_content, tool_call_id, tool_calls, tool_name,
                        timestamp, token_count, finish_reason, reasoning, reasoning_details, media_json
                 FROM messages WHERE session_id = ?2
                 ORDER BY timestamp ASC, id ASC",
            )
            .bind(new_id)
            .bind(source_id)
            .execute(&mut **tx)
            .await?
            .rows_affected()
        }
    };
    Ok(i64::try_from(copied).unwrap_or(i64::MAX))
}

async fn completed_turn_count_before_boundary(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    source_id: &str,
    boundary: Option<(f64, i64)>,
) -> Result<i64> {
    let roles: Vec<(String,)> = match boundary {
        Some((timestamp, id)) => {
            sqlx::query_as(
                "SELECT role FROM messages
                 WHERE session_id = ?1
                   AND (timestamp < ?2 OR (timestamp = ?2 AND id < ?3))
                 ORDER BY timestamp ASC, id ASC",
            )
            .bind(source_id)
            .bind(timestamp)
            .bind(id)
            .fetch_all(&mut **tx)
            .await?
        }
        None => {
            sqlx::query_as(
                "SELECT role FROM messages WHERE session_id = ?1
                 ORDER BY timestamp ASC, id ASC",
            )
            .bind(source_id)
            .fetch_all(&mut **tx)
            .await?
        }
    };
    let mut count = 0i64;
    let mut in_turn = false;
    let mut current_completed = false;
    for (role,) in roles {
        if role == "user" {
            if in_turn && current_completed {
                count += 1;
            }
            in_turn = true;
            current_completed = false;
        } else if role == "assistant" && in_turn {
            current_completed = true;
        }
    }
    if in_turn && current_completed {
        count += 1;
    }
    Ok(count)
}

async fn set_branch_title(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    new_id: &str,
    title: Option<String>,
) -> Result<()> {
    let Some(title) = title.filter(|title| !title.trim().is_empty()) else {
        return Ok(());
    };
    let branched = format!("{title} · branch");
    let result = sqlx::query("UPDATE sessions SET title = ?1 WHERE id = ?2")
        .bind(&branched)
        .bind(new_id)
        .execute(&mut **tx)
        .await;
    match result {
        Ok(_) => {}
        Err(err) if is_unique_constraint(&err) => {
            let suffix: String = new_id.chars().take(8).collect();
            let unique = format!("{} · {}", truncate_chars(&branched, 60), suffix);
            sqlx::query("UPDATE sessions SET title = ?1 WHERE id = ?2")
                .bind(&unique)
                .bind(new_id)
                .execute(&mut **tx)
                .await?;
        }
        Err(err) => return Err(err.into()),
    }
    Ok(())
}

fn turn_spans(messages: &[StoredMessage]) -> Vec<TurnSpan> {
    let user_positions = messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message.role == "user")
        .map(|(position, _)| position)
        .collect::<Vec<_>>();
    user_positions
        .iter()
        .enumerate()
        .map(|(turn, start)| {
            let end = user_positions
                .get(turn + 1)
                .copied()
                .unwrap_or(messages.len());
            TurnSpan {
                index: i64::try_from(turn + 1).unwrap_or(i64::MAX),
                user_message_id: messages[*start].id,
                content: messages[*start].content.clone(),
                completed: messages[*start + 1..end]
                    .iter()
                    .any(|message| message.role == "assistant"),
                start: *start,
                end,
            }
        })
        .collect()
}

fn infer_branch_metadata(
    parent_messages: &[StoredMessage],
    child_messages: &[StoredMessage],
) -> ResolvedBranchMetadata {
    let parent_turns = turn_spans(parent_messages);
    let child_turns = turn_spans(child_messages);
    let Some(first_child) = child_turns.first() else {
        return ResolvedBranchMetadata::default();
    };

    let mut best: Option<(usize, usize)> = None;
    for (parent_start, parent_turn) in parent_turns.iter().enumerate() {
        if !same_turn(parent_messages, parent_turn, child_messages, first_child) {
            continue;
        }
        let matched = parent_turns[parent_start..]
            .iter()
            .zip(&child_turns)
            .take_while(|(parent, child)| same_turn(parent_messages, parent, child_messages, child))
            .count();
        if best.is_none_or(|(_, best_count)| matched > best_count) {
            best = Some((parent_start, matched));
        }
    }
    let Some((parent_start, matched)) = best.filter(|(_, matched)| *matched > 0) else {
        return ResolvedBranchMetadata::default();
    };
    let inherited = child_turns[..matched]
        .iter()
        .filter(|turn| turn.completed)
        .count();
    let anchor_offset = child_turns[..matched]
        .iter()
        .rposition(|turn| turn.completed);
    let anchor = anchor_offset.map(|offset| &parent_turns[parent_start + offset]);
    ResolvedBranchMetadata {
        parent_message_id: anchor.map(|turn| turn.user_message_id),
        parent_turn_index: anchor.map(|turn| turn.index),
        inherited_turn_count: i64::try_from(inherited).unwrap_or(i64::MAX),
    }
}

fn same_turn(
    left_messages: &[StoredMessage],
    left: &TurnSpan,
    right_messages: &[StoredMessage],
    right: &TurnSpan,
) -> bool {
    let left_rows = &left_messages[left.start..left.end];
    let right_rows = &right_messages[right.start..right.end];
    left_rows.len() == right_rows.len()
        && left_rows.iter().zip(right_rows).all(|(left, right)| {
            left.role == right.role
                && left.content == right.content
                && left.tool_call_id == right.tool_call_id
                && left.tool_name == right.tool_name
                && left.timestamp == right.timestamp
        })
}
