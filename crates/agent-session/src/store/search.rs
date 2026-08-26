//! 消息检索、聊天历史与 FTS。

use super::{
    activities_from_tool_calls, attach_tool_output, coalesce_consecutive_assistants,
    escape_fts5_query, truncate_chars, ChatHistoryMessage, RecentSession, SearchHit, SessionStore,
};
use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use std::collections::HashSet;

/// FTS 命中收集参数（一次查询的表、过滤与输出缓冲）。
struct FtsCollect<'a> {
    fts_table: &'a str,
    fts_query: &'a str,
    source_filter: Option<&'a str>,
    role_filter: Option<&'a str>,
    limit: i64,
    hits: &'a mut Vec<SearchHit>,
    seen: &'a mut HashSet<i64>,
}

impl SessionStore {
    /// 跨会话消息 FTS：优先 `messages_fts`，再合并 `messages_fts_trigram`（CJK / 子串）。
    pub fn search_messages(
        &self,
        query: &str,
        source_filter: Option<&str>,
        role_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<SearchHit>> {
        let q = query.trim();
        if q.is_empty() || limit <= 0 {
            return Ok(Vec::new());
        }
        let fts_query = escape_fts5_query(q);
        let mut hits = Vec::new();
        let mut seen = HashSet::new();

        self.collect_fts_hits(FtsCollect {
            fts_table: "messages_fts",
            fts_query: &fts_query,
            source_filter,
            role_filter,
            limit,
            hits: &mut hits,
            seen: &mut seen,
        })?;
        if (hits.len() as i64) < limit {
            self.collect_fts_hits(FtsCollect {
                fts_table: "messages_fts_trigram",
                fts_query: &fts_query,
                source_filter,
                role_filter,
                limit,
                hits: &mut hits,
                seen: &mut seen,
            })?;
        }

        for hit in &mut hits {
            hit.context = self.neighbor_context(hit.session_id.as_str(), hit.id)?;
        }
        Ok(hits)
    }

    /// 按时间扫描并折叠为 UI 气泡：user / assistant（含 activities）；tool 不单独成泡。
    pub fn build_chat_history(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<ChatHistoryMessage>> {
        let messages = self.get_messages(session_id)?;
        let mut out: Vec<ChatHistoryMessage> = Vec::new();

        for m in messages {
            match m.role.as_str() {
                "user" => {
                    out.push(ChatHistoryMessage {
                        id: m.id.to_string(),
                        role: "user".into(),
                        content: m.content.unwrap_or_default(),
                        reasoning: None,
                        activities: Vec::new(),
                        segments: None,
                        ui_surfaces: None,
                    });
                }
                "assistant" => {
                    let activities = activities_from_tool_calls(m.tool_calls.as_ref());
                    let (segments, ui_surfaces) = match &m.reasoning_details {
                        Some(Value::Object(map)) => (
                            map.get("astro_timeline_v1").cloned(),
                            map.get("astro_surfaces_v1").cloned(),
                        ),
                        _ => (None, None),
                    };
                    out.push(ChatHistoryMessage {
                        id: m.id.to_string(),
                        role: "assistant".into(),
                        content: m.content.unwrap_or_default(),
                        reasoning: m.reasoning.or(m.reasoning_content),
                        activities,
                        segments,
                        ui_surfaces,
                    });
                }
                "tool" => {
                    let call_id = m.tool_call_id.as_deref();
                    let output = m.content.clone();
                    let media = m
                        .media_json
                        .as_deref()
                        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                        .filter(|v| matches!(v, Value::Array(a) if !a.is_empty()));
                    if let Some(assistant) =
                        out.iter_mut().rev().find(|msg| msg.role == "assistant")
                    {
                        attach_tool_output(
                            assistant,
                            call_id,
                            output,
                            m.tool_name.as_deref(),
                            media,
                        );
                    }
                }
                _ => {}
            }
        }

        // 同轮工具循环会落多条 assistant；UI 期望合并为一条气泡。
        out = coalesce_consecutive_assistants(out);

        if out.len() > limit {
            let skip = out.len() - limit;
            out = out.into_iter().skip(skip).collect();
        }
        Ok(out)
    }

    /// 取指定会话最近 `limit` 条消息，按时间正序返回（`is_anchor` 均为 false）。
    pub fn recent_messages(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<crate::message_db::ScrolledMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, role, COALESCE(content, '')
             FROM messages
             WHERE session_id = ?1
             ORDER BY id DESC
             LIMIT ?2",
        )?;
        let mut rows: Vec<crate::message_db::ScrolledMessage> = stmt
            .query_map(params![session_id, limit as i64], |row| {
                let id: i64 = row.get(0)?;
                Ok(crate::message_db::ScrolledMessage {
                    id,
                    role: row.get(1)?,
                    content: row.get(2)?,
                    is_anchor: false,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.reverse();
        Ok(rows)
    }

    /// 返回全局最近一条消息所属的 `session_id`；无消息时返回 `None`。
    pub fn latest_session_id(&self) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT session_id FROM messages ORDER BY id DESC LIMIT 1")?;
        let mut rows = stmt.query([])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// 在指定会话内按 FTS 召回消息 id（优先 unicode61，再补 trigram），按相关度排序。
    pub fn recall_message_ids(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<i64>> {
        let query = query.trim();
        if query.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let fts_query = escape_fts5_query(query);
        let mut ids = Vec::new();
        let mut seen = HashSet::new();
        self.collect_session_fts_ids(
            "messages_fts",
            session_id,
            &fts_query,
            limit,
            &mut ids,
            &mut seen,
        )?;
        if ids.len() < limit {
            self.collect_session_fts_ids(
                "messages_fts_trigram",
                session_id,
                &fts_query,
                limit,
                &mut ids,
                &mut seen,
            )?;
        }
        Ok(ids)
    }

    /// 以 `around_message_id` 为中心，取前后各 `window_size` 条消息（含中心），按 id 升序。
    pub fn scroll_context_window(
        &self,
        session_id: &str,
        around_message_id: i64,
        window_size: i64,
    ) -> Result<Vec<crate::message_db::ScrolledMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, role, COALESCE(content, '')
             FROM messages
             WHERE session_id = ?1
               AND id BETWEEN (?2 - ?3) AND (?2 + ?3)
             ORDER BY id ASC",
        )?;
        let rows = stmt
            .query_map(params![session_id, around_message_id, window_size], |row| {
                let id: i64 = row.get(0)?;
                Ok(crate::message_db::ScrolledMessage {
                    id,
                    role: row.get(1)?,
                    content: row.get(2)?,
                    is_anchor: id == around_message_id,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// 按 `started_at` 降序列出会话；preview 取首条 user content（截断 120 字）。
    /// `project_root` 过滤：`Some(path)` = 仅该项目，`None` = 全部。
    pub fn list_sessions(
        &self,
        filter: super::SessionListFilter,
        limit: usize,
    ) -> Result<Vec<RecentSession>> {
        self.list_sessions_filtered(filter, limit, None)
    }

    pub fn list_sessions_filtered(
        &self,
        filter: super::SessionListFilter,
        limit: usize,
        project_root: Option<&str>,
    ) -> Result<Vec<RecentSession>> {
        // 兼容：如果传了非 default 的 project_root，先尝试按 project_id 查找
        if let Some(root) = project_root.filter(|r| !r.is_empty() && *r != "default") {
            if let Some(proj) = self.find_project_by_root(root)? {
                return self.list_sessions_by_project(filter, limit, &proj.id);
            }
        }
        self.list_sessions_inner(filter, limit, project_root)
    }

    /// 按 `project_id` 过滤会话列表。
    pub fn list_sessions_by_project(
        &self,
        filter: super::SessionListFilter,
        limit: usize,
        project_id: &str,
    ) -> Result<Vec<RecentSession>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut conditions = Vec::new();
        match filter {
            super::SessionListFilter::Active => conditions.push("s.archived_at IS NULL"),
            super::SessionListFilter::Archived => conditions.push("s.archived_at IS NOT NULL"),
        }
        // Codex-style Side 会话是进程内临时旁路，不进入普通会话列表。
        conditions.push("COALESCE(s.branch_kind, '') != 'side'");
        conditions.push("s.project_id = ?2");
        let where_clause = format!("WHERE {}", conditions.join(" AND "));
        let sql = format!(
            "SELECT s.id, s.title, s.started_at,
                    (SELECT m.content FROM messages m
                     WHERE m.session_id = s.id
                       AND m.role = 'user'
                       AND m.content IS NOT NULL
                       AND TRIM(m.content) != ''
                     ORDER BY m.timestamp ASC, m.id ASC
                     LIMIT 1) AS preview,
                    s.ended_at, s.end_reason, s.archived_at, s.pinned_at
             FROM sessions s
             {where_clause}
             ORDER BY (s.pinned_at IS NULL) ASC, s.pinned_at DESC, s.started_at DESC
             LIMIT ?1"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt
            .query_map(params![limit as i64, project_id], Self::map_recent_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self::to_recent_sessions(rows))
    }

    fn list_sessions_inner(
        &self,
        filter: super::SessionListFilter,
        limit: usize,
        project_root: Option<&str>,
    ) -> Result<Vec<RecentSession>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let mut conditions = Vec::new();
        match filter {
            super::SessionListFilter::Active => conditions.push("s.archived_at IS NULL"),
            super::SessionListFilter::Archived => conditions.push("s.archived_at IS NOT NULL"),
        }
        conditions.push("COALESCE(s.branch_kind, '') != 'side'");
        if let Some(root) = project_root {
            if root.is_empty() || root == "default" {
                conditions.push(
                    "(s.project_id IS NULL AND (s.project_root IS NULL OR s.project_root = ''))",
                );
            } else {
                conditions.push("s.project_root = ?2");
            }
        }
        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };
        let sql = format!(
            "SELECT s.id, s.title, s.started_at,
                    (SELECT m.content FROM messages m
                     WHERE m.session_id = s.id
                       AND m.role = 'user'
                       AND m.content IS NOT NULL
                       AND TRIM(m.content) != ''
                     ORDER BY m.timestamp ASC, m.id ASC
                     LIMIT 1) AS preview,
                    s.ended_at, s.end_reason, s.archived_at, s.pinned_at
             FROM sessions s
             {where_clause}
             ORDER BY (s.pinned_at IS NULL) ASC, s.pinned_at DESC, s.started_at DESC
             LIMIT ?1"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = if let Some(root) = project_root.filter(|r| !r.is_empty() && *r != "default") {
            stmt.query_map(params![limit as i64, root], Self::map_recent_row)?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map(params![limit as i64], Self::map_recent_row)?
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(Self::to_recent_sessions(rows))
    }

    fn map_recent_row(
        row: &rusqlite::Row<'_>,
    ) -> rusqlite::Result<(
        String,
        Option<String>,
        f64,
        Option<String>,
        Option<f64>,
        Option<String>,
        Option<f64>,
        Option<f64>,
    )> {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
            row.get(7)?,
        ))
    }

    fn to_recent_sessions(
        rows: Vec<(
            String,
            Option<String>,
            f64,
            Option<String>,
            Option<f64>,
            Option<String>,
            Option<f64>,
            Option<f64>,
        )>,
    ) -> Vec<RecentSession> {
        rows.into_iter()
            .map(
                |(id, title, started_at, preview, ended_at, end_reason, archived_at, pinned_at)| {
                    RecentSession {
                        id,
                        title,
                        started_at,
                        preview: preview.map(|p| truncate_chars(&p, 120)),
                        ended_at,
                        end_reason,
                        archived_at,
                        pinned_at,
                    }
                },
            )
            .collect()
    }

    /// 兼容旧调用：仅列出未归档会话。
    pub fn list_recent_sessions(&self, limit: usize) -> Result<Vec<RecentSession>> {
        self.list_sessions(super::SessionListFilter::Active, limit)
    }

    fn collect_session_fts_ids(
        &self,
        fts_table: &str,
        session_id: &str,
        fts_query: &str,
        limit: usize,
        ids: &mut Vec<i64>,
        seen: &mut HashSet<i64>,
    ) -> Result<()> {
        let remaining = limit.saturating_sub(ids.len());
        if remaining == 0 {
            return Ok(());
        }
        let sql = format!(
            "SELECT m.id
             FROM {fts} AS f
             JOIN messages AS m ON m.id = f.rowid
             WHERE m.session_id = ?1 AND {fts} MATCH ?2
             ORDER BY rank
             LIMIT ?3",
            fts = fts_table
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![session_id, fts_query, remaining as i64], |row| {
            row.get::<_, i64>(0)
        })?;
        for row in rows {
            let id = row?;
            if seen.insert(id) {
                ids.push(id);
                if ids.len() >= limit {
                    break;
                }
            }
        }
        Ok(())
    }

    fn collect_fts_hits(&self, q: FtsCollect<'_>) -> Result<()> {
        let remaining = q.limit - q.hits.len() as i64;
        if remaining <= 0 {
            return Ok(());
        }

        // fts_table 仅内部常量 "messages_fts" | "messages_fts_trigram"。
        let sql = format!(
            "SELECT m.id, m.session_id, m.role,
                    COALESCE(snippet({fts}, 0, '', '', '…', 32), m.content, ''),
                    m.tool_name
             FROM {fts} AS f
             JOIN messages AS m ON m.id = f.rowid
             JOIN sessions AS s ON s.id = m.session_id
             WHERE {fts} MATCH ?1
               AND (?2 IS NULL OR s.source = ?2)
               AND (?3 IS NULL OR m.role = ?3)
             ORDER BY m.timestamp DESC, m.id DESC
             LIMIT ?4",
            fts = q.fts_table
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![q.fts_query, q.source_filter, q.role_filter, remaining],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )?;

        for row in rows {
            let (id, session_id, role, snippet, tool_name) = row?;
            if !q.seen.insert(id) {
                continue;
            }
            q.hits.push(SearchHit {
                id,
                session_id,
                role,
                snippet,
                context: String::new(),
                tool_name,
            });
            if q.hits.len() as i64 >= q.limit {
                break;
            }
        }
        Ok(())
    }

    fn neighbor_context(&self, session_id: &str, message_id: i64) -> Result<String> {
        let mut parts = Vec::new();
        let prev: Option<(String, Option<String>)> = self
            .conn
            .query_row(
                "SELECT role, content FROM messages
                 WHERE session_id = ?1 AND id < ?2
                 ORDER BY id DESC LIMIT 1",
                params![session_id, message_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((role, content)) = prev {
            parts.push(format!(
                "[prev:{role}] {}",
                truncate_chars(content.as_deref().unwrap_or(""), 80)
            ));
        }
        let next: Option<(String, Option<String>)> = self
            .conn
            .query_row(
                "SELECT role, content FROM messages
                 WHERE session_id = ?1 AND id > ?2
                 ORDER BY id ASC LIMIT 1",
                params![session_id, message_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((role, content)) = next {
            parts.push(format!(
                "[next:{role}] {}",
                truncate_chars(content.as_deref().unwrap_or(""), 80)
            ));
        }
        Ok(parts.join(" | "))
    }
}
