//! 聊天分支创建、旧元数据推断与递归谱系图查询。

use std::collections::{HashMap, HashSet, VecDeque};

use anyhow::{anyhow, Result};
use rusqlite::{params, OptionalExtension, Transaction};

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
    /// 从父会话的一个已完成 user turn 分叉（Codex `last_turn_id` 语义）。
    ///
    /// `parent_user_message_id` 必须属于 `source_id`，且该 turn 在下一条 user 消息前
    /// 至少有一条 assistant 消息。复制范围从会话开头到该 turn 末尾，消息使用新行 id。
    pub fn fork_session_at_user_message(
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
        )
    }

    /// 分叉到某个 user turn 之前（Codex `before_turn_id` 语义）。
    ///
    /// 锚点 turn 本身不进入新分支，因此未完成的 turn 也可作为锚点——这正是
    /// 「改写上一条消息重开一条路径」需要的边界。
    pub fn fork_session_before_user_message(
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
        )
    }

    fn fork_session_at_boundary(
        &self,
        source_id: &str,
        new_id: &str,
        parent_user_message_id: i64,
        boundary: ForkBoundary,
    ) -> Result<ForkedSession> {
        anyhow::ensure!(
            source_id != new_id,
            "fork_session: source and target session ids must differ"
        );
        let tx = self.conn.unchecked_transaction()?;
        let (source, model, title) = tx
            .query_row(
                "SELECT source, model, title FROM sessions WHERE id = ?1",
                params![source_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| anyhow!("fork_session: source session not found: {source_id:?}"))?;
        let target_exists = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)",
            params![new_id],
            |row| row.get::<_, bool>(0),
        )?;
        anyhow::ensure!(
            !target_exists,
            "fork_session: target session already exists"
        );

        let (anchor_timestamp, anchor_id, role) = tx
            .query_row(
                "SELECT timestamp, id, role FROM messages
                 WHERE id = ?1 AND session_id = ?2",
                params![parent_user_message_id, source_id],
                |row| {
                    Ok((
                        row.get::<_, f64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| anyhow!("fork_session: anchor message not found in source session"))?;
        anyhow::ensure!(
            role == "user",
            "fork_session: anchor message must have role=user"
        );

        let next_user = tx
            .query_row(
                "SELECT timestamp, id FROM messages
                 WHERE session_id = ?1 AND role = 'user'
                   AND (timestamp > ?2 OR (timestamp = ?2 AND id > ?3))
                 ORDER BY timestamp ASC, id ASC LIMIT 1",
                params![source_id, anchor_timestamp, anchor_id],
                |row| Ok((row.get::<_, f64>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()?;
        // 复制上界：through 停在下一条 user 之前，before 停在锚点自身之前。
        let copy_boundary = match boundary {
            ForkBoundary::ThroughTurn => next_user,
            ForkBoundary::BeforeTurn => Some((anchor_timestamp, anchor_id)),
        };

        if boundary == ForkBoundary::ThroughTurn {
            let completed = match next_user {
                Some((next_timestamp, next_id)) => tx.query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM messages
                        WHERE session_id = ?1 AND role = 'assistant'
                          AND (timestamp > ?2 OR (timestamp = ?2 AND id > ?3))
                          AND (timestamp < ?4 OR (timestamp = ?4 AND id < ?5))
                     )",
                    params![
                        source_id,
                        anchor_timestamp,
                        anchor_id,
                        next_timestamp,
                        next_id
                    ],
                    |row| row.get::<_, bool>(0),
                )?,
                None => tx.query_row(
                    "SELECT EXISTS(
                        SELECT 1 FROM messages
                        WHERE session_id = ?1 AND role = 'assistant'
                          AND (timestamp > ?2 OR (timestamp = ?2 AND id > ?3))
                     )",
                    params![source_id, anchor_timestamp, anchor_id],
                    |row| row.get::<_, bool>(0),
                )?,
            };
            anyhow::ensure!(completed, "fork_session: user turn is not completed");
        }

        // 挂靠点取复制前缀里最后一个完整 turn：through 就是锚点本身，
        // before 则回退到上一轮，首轮之前分叉时为空。
        let anchor = match boundary {
            ForkBoundary::ThroughTurn => {
                let turn_index = tx.query_row(
                    "SELECT COUNT(*) FROM messages
                     WHERE session_id = ?1 AND role = 'user'
                       AND (timestamp < ?2 OR (timestamp = ?2 AND id <= ?3))",
                    params![source_id, anchor_timestamp, anchor_id],
                    |row| row.get::<_, i64>(0),
                )?;
                Some((parent_user_message_id, turn_index))
            }
            ForkBoundary::BeforeTurn => tx
                .query_row(
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
                    params![source_id, anchor_timestamp, anchor_id],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()?,
        };

        let inherited_turn_count =
            completed_turn_count_before_boundary(&tx, source_id, copy_boundary)?;
        let created_at = now_epoch_secs()?;
        tx.execute(
            "INSERT INTO sessions (
                id, source, model, parent_session_id, started_at,
                branch_parent_message_id, branch_parent_turn_index,
                branch_inherited_turn_count, branch_created_at, branch_kind
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?5, ?9)",
            params![
                new_id,
                source,
                model,
                source_id,
                created_at,
                anchor.map(|(message_id, _)| message_id),
                anchor.map(|(_, turn_index)| turn_index),
                inherited_turn_count,
                BranchKind::Fork.as_str()
            ],
        )?;

        let copied_message_count = copy_prefix_through_turn(&tx, source_id, new_id, copy_boundary)?;
        let (tool_call_count, copied_user_turns) = tx.query_row(
            "SELECT SUM(CASE WHEN role = 'tool' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN role = 'user' THEN 1 ELSE 0 END)
             FROM messages WHERE session_id = ?1",
            params![new_id],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                    row.get::<_, Option<i64>>(1)?.unwrap_or(0),
                ))
            },
        )?;
        tx.execute(
            "UPDATE sessions SET message_count = ?1, tool_call_count = ?2 WHERE id = ?3",
            params![copied_message_count, tool_call_count, new_id],
        )?;
        set_branch_title(&tx, new_id, title)?;
        tx.commit()?;

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

    /// 查询任意会话所在的完整聊天分支谱系：向上找到可达根，再递归收集全部后代。
    ///
    /// `branch_kind = 'agent'` 的会话（子 Agent 派生）连同其子树被剔除——它们共用
    /// `parent_session_id` 但不属于聊天分支；仅当请求会话本身在这条链上时才保留。
    /// v21 及更早创建的分支会通过父/子消息前缀推断锚点；损坏的父引用与循环不会死循环，
    /// 而是分别写入 `orphaned_parent_ids` / `cycle_detected`。
    pub fn session_lineage_graph(&self, session_id: &str) -> Result<SessionLineageGraph> {
        let sessions = self.load_all_sessions()?;
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
                self.resolve_branch_metadata(session, &sessions)?,
            );
        }

        let mut nodes = Vec::with_capacity(included.len());
        for id in included {
            let session = &sessions[&id];
            let mut turns = turn_spans(&self.get_messages(&id)?)
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
                legacy_metadata: metadata.legacy,
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

    pub(crate) fn write_legacy_fork_metadata(
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
                legacy: false,
            },
        )
    }

    pub(crate) fn infer_and_write_fork_metadata(
        &self,
        source_id: &str,
        target_id: &str,
        kind: BranchKind,
    ) -> Result<()> {
        let inferred = infer_legacy_metadata(
            &self.get_messages(source_id)?,
            &self.get_messages(target_id)?,
        );
        self.write_branch_metadata(source_id, target_id, kind, &inferred)
    }

    fn write_branch_metadata(
        &self,
        source_id: &str,
        target_id: &str,
        kind: BranchKind,
        metadata: &ResolvedBranchMetadata,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET
                branch_parent_message_id = ?1,
                branch_parent_turn_index = ?2,
                branch_inherited_turn_count = ?3,
                branch_created_at = COALESCE(branch_created_at, ?4),
                branch_kind = ?5
             WHERE id = ?6 AND parent_session_id = ?7",
            params![
                metadata.parent_message_id,
                metadata.parent_turn_index,
                metadata.inherited_turn_count,
                now_epoch_secs()?,
                kind.as_str(),
                target_id,
                source_id
            ],
        )?;
        Ok(())
    }

    fn load_all_sessions(&self) -> Result<HashMap<String, StoredSession>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM sessions ORDER BY started_at, id")?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        let mut sessions = HashMap::with_capacity(ids.len());
        for id in ids {
            if let Some(session) = self.get_session(&id)? {
                sessions.insert(id, session);
            }
        }
        Ok(sessions)
    }

    fn resolve_branch_metadata(
        &self,
        session: &StoredSession,
        sessions: &HashMap<String, StoredSession>,
    ) -> Result<ResolvedBranchMetadata> {
        if session.parent_session_id.is_none() {
            return Ok(ResolvedBranchMetadata::default());
        }
        if session.branch_parent_message_id.is_some()
            || session.branch_parent_turn_index.is_some()
            || session.branch_inherited_turn_count.is_some()
        {
            return Ok(ResolvedBranchMetadata {
                parent_message_id: session.branch_parent_message_id,
                parent_turn_index: session.branch_parent_turn_index,
                inherited_turn_count: session.branch_inherited_turn_count.unwrap_or(0),
                legacy: false,
            });
        }
        let Some(parent_id) = session
            .parent_session_id
            .as_ref()
            .filter(|id| sessions.contains_key(*id))
        else {
            return Ok(ResolvedBranchMetadata {
                legacy: true,
                ..ResolvedBranchMetadata::default()
            });
        };
        let parent_messages = self.get_messages(parent_id)?;
        let child_messages = self.get_messages(&session.id)?;
        Ok(infer_legacy_metadata(&parent_messages, &child_messages))
    }
}

#[derive(Debug, Default)]
struct ResolvedBranchMetadata {
    parent_message_id: Option<i64>,
    parent_turn_index: Option<i64>,
    inherited_turn_count: i64,
    legacy: bool,
}

fn copy_prefix_through_turn(
    tx: &Transaction<'_>,
    source_id: &str,
    new_id: &str,
    next_user: Option<(f64, i64)>,
) -> Result<i64> {
    let base = "INSERT INTO messages (
            session_id, role, content, compressed_content, tool_call_id, tool_calls, tool_name,
            timestamp, token_count, finish_reason, reasoning, reasoning_content, reasoning_details,
            codex_reasoning_items, codex_message_items, media_json
         )
         SELECT ?1, role, content, compressed_content, tool_call_id, tool_calls, tool_name,
                timestamp, token_count, finish_reason, reasoning, reasoning_content,
                reasoning_details, codex_reasoning_items, codex_message_items, media_json
         FROM messages WHERE session_id = ?2";
    let copied = match next_user {
        Some((timestamp, id)) => tx.execute(
            &format!(
                "{base}
                 AND (timestamp < ?3 OR (timestamp = ?3 AND id < ?4))
                 ORDER BY timestamp ASC, id ASC"
            ),
            params![new_id, source_id, timestamp, id],
        )?,
        None => tx.execute(
            &format!("{base} ORDER BY timestamp ASC, id ASC"),
            params![new_id, source_id],
        )?,
    };
    Ok(i64::try_from(copied).unwrap_or(i64::MAX))
}

fn completed_turn_count_before_boundary(
    tx: &Transaction<'_>,
    source_id: &str,
    boundary: Option<(f64, i64)>,
) -> Result<i64> {
    let roles = match boundary {
        Some((timestamp, id)) => {
            let mut stmt = tx.prepare(
                "SELECT role FROM messages
                 WHERE session_id = ?1
                   AND (timestamp < ?2 OR (timestamp = ?2 AND id < ?3))
                 ORDER BY timestamp ASC, id ASC",
            )?;
            let rows = stmt
                .query_map(params![source_id, timestamp, id], |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        }
        None => {
            let mut stmt = tx.prepare(
                "SELECT role FROM messages WHERE session_id = ?1
                 ORDER BY timestamp ASC, id ASC",
            )?;
            let rows = stmt
                .query_map(params![source_id], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        }
    };
    let mut count = 0i64;
    let mut in_turn = false;
    let mut current_completed = false;
    for role in roles {
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

fn set_branch_title(tx: &Transaction<'_>, new_id: &str, title: Option<String>) -> Result<()> {
    let Some(title) = title.filter(|title| !title.trim().is_empty()) else {
        return Ok(());
    };
    let branched = format!("{title} · branch");
    if let Err(error) = tx.execute(
        "UPDATE sessions SET title = ?1 WHERE id = ?2",
        params![branched, new_id],
    ) {
        if !is_unique_constraint(&error) {
            return Err(error.into());
        }
        let suffix: String = new_id.chars().take(8).collect();
        let unique = format!("{} · {}", truncate_chars(&branched, 60), suffix);
        tx.execute(
            "UPDATE sessions SET title = ?1 WHERE id = ?2",
            params![unique, new_id],
        )?;
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

fn infer_legacy_metadata(
    parent_messages: &[StoredMessage],
    child_messages: &[StoredMessage],
) -> ResolvedBranchMetadata {
    let parent_turns = turn_spans(parent_messages);
    let child_turns = turn_spans(child_messages);
    let Some(first_child) = child_turns.first() else {
        return ResolvedBranchMetadata {
            legacy: true,
            ..ResolvedBranchMetadata::default()
        };
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
        return ResolvedBranchMetadata {
            legacy: true,
            ..ResolvedBranchMetadata::default()
        };
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
        legacy: true,
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
