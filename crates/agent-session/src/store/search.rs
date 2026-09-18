//! 消息检索、聊天历史与 FTS。

use super::{truncate_chars, RecentSession, SearchHit, SessionStore};
use agent_db::sqlx::{self, AssertSqlSafe, Row};
use anyhow::Result;
use std::collections::HashSet;

type RecentSessionRow = (
    String,
    Option<String>,
    f64,
    Option<String>,
    Option<f64>,
    Option<String>,
    Option<f64>,
    Option<f64>,
    Option<String>,
    String,
);

/// 摘要前后各取的字符数。
const SNIPPET_BEFORE_CHARS: usize = 24;
const SNIPPET_AFTER_CHARS: usize = 60;

/// FTS 命中收集参数（一次查询的过滤与输出缓冲）。
struct FtsCollect<'a> {
    fts_query: &'a str,
    raw_query: &'a str,
    source_filter: Option<&'a str>,
    role_filter: Option<&'a str>,
    limit: i64,
    hits: &'a mut Vec<SearchHit>,
    seen: &'a mut HashSet<i64>,
}

/// 取 `byte_end` 之前最多 `max_chars` 个字符的起点。
fn context_start(text: &str, byte_end: usize, max_chars: usize) -> usize {
    let mut start = byte_end;
    let mut chars = text[..byte_end].char_indices().rev();
    for _ in 0..max_chars {
        match chars.next() {
            Some((index, _)) => start = index,
            None => return 0,
        }
    }
    start
}

/// 取 `byte_start` 之后最多 `max_chars` 个字符的终点。
fn context_end(text: &str, byte_start: usize, max_chars: usize) -> usize {
    let mut end = byte_start;
    let mut chars = text[byte_start..].char_indices();
    for _ in 0..max_chars {
        match chars.next() {
            Some((index, c)) => end = byte_start + index + c.len_utf8(),
            None => return text.len(),
        }
    }
    end
}

/// 在原文里生成命中摘要（带前后省略号）。
///
/// 展示文本是原文，不走切分——索引列里的空格不会泄漏到界面；`snippet()` 在
/// contentless FTS 表上返回 NULL，这里改为直接切原文。
fn build_snippet(text: &str, query: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let Some((match_start, match_end)) = types::search_text::find_match_range(trimmed, query)
    else {
        return truncate_chars(trimmed, SNIPPET_BEFORE_CHARS + SNIPPET_AFTER_CHARS);
    };
    let from = context_start(trimmed, match_start, SNIPPET_BEFORE_CHARS);
    let to = context_end(trimmed, match_end, SNIPPET_AFTER_CHARS);
    let mut snippet = String::with_capacity(to - from + 2);
    if from > 0 {
        snippet.push('…');
    }
    snippet.push_str(&trimmed[from..to]);
    if to < trimmed.len() {
        snippet.push('…');
    }
    snippet
}

fn add_placement_conditions(
    conditions: &mut Vec<&'static str>,
    placement: super::SessionPlacementFilter,
) {
    match placement {
        super::SessionPlacementFilter::All => {}
        super::SessionPlacementFilter::Pinned => conditions.push("s.pinned_at IS NOT NULL"),
        super::SessionPlacementFilter::Project => {
            conditions.push("s.pinned_at IS NULL");
            conditions.push("s.source != 'cron'");
            conditions.push("s.project_id IS NOT NULL");
        }
        super::SessionPlacementFilter::Automation => {
            conditions.push("s.pinned_at IS NULL");
            conditions.push("s.source = 'cron'");
        }
        super::SessionPlacementFilter::Recent => {
            conditions.push("s.pinned_at IS NULL");
            conditions.push("s.source != 'cron'");
            conditions.push("s.project_id IS NULL");
        }
    }
}

impl SessionStore {
    /// 跨会话 item FTS：索引侧与查询侧使用同一套按字切分。
    pub async fn search_messages(
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
        let Some(fts_query) = types::search_text::match_query(q) else {
            return Ok(Vec::new());
        };
        let mut hits = Vec::new();
        let mut seen = HashSet::new();

        self.collect_fts_hits(FtsCollect {
            fts_query: &fts_query,
            raw_query: q,
            source_filter,
            role_filter,
            limit,
            hits: &mut hits,
            seen: &mut seen,
        })
        .await?;

        for hit in &mut hits {
            hit.context = self
                .neighbor_context(hit.session_id.as_str(), hit.id)
                .await?;
        }
        Ok(hits)
    }

    /// 取指定会话最近 `limit` 条消息，按时间正序返回（`is_anchor` 均为 false）。
    pub async fn recent_messages(
        &self,
        session_id: &str,
        limit: usize,
    ) -> Result<Vec<crate::context_recall::ScrolledResponseItem>> {
        let rows = sqlx::query(
            "SELECT id, item_json
             FROM response_items
             WHERE session_id = ?1
             ORDER BY id DESC
             LIMIT ?2",
        )
        .bind(session_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut result: Vec<crate::context_recall::ScrolledResponseItem> = rows
            .iter()
            .map(|row| -> Result<_> {
                let id: i64 = row.get::<i64, _>(0);
                Ok(crate::context_recall::ScrolledResponseItem {
                    id,
                    item: serde_json::from_str(&row.get::<String, _>(1))?,
                    is_anchor: false,
                })
            })
            .collect::<Result<_>>()?;
        result.reverse();
        Ok(result)
    }

    /// 返回全局最近一条消息所属的 `session_id`；无消息时返回 `None`。
    pub async fn latest_session_id(&self) -> Result<Option<String>> {
        let row = sqlx::query("SELECT session_id FROM response_items ORDER BY id DESC LIMIT 1")
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get::<String, _>(0)))
    }

    /// 在指定会话内按 FTS 召回消息 id，按相关度排序。
    pub async fn recall_message_ids(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<i64>> {
        let query = query.trim();
        if query.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let Some(fts_query) = types::search_text::match_query(query) else {
            return Ok(Vec::new());
        };
        let mut ids = Vec::new();
        let mut seen = HashSet::new();
        self.collect_session_fts_ids(session_id, &fts_query, limit, &mut ids, &mut seen)
            .await?;
        Ok(ids)
    }

    /// 以 `around_message_id` 为中心，取前后各 `window_size` 条消息（含中心），按 id 升序。
    pub async fn scroll_context_window(
        &self,
        session_id: &str,
        around_message_id: i64,
        window_size: i64,
    ) -> Result<Vec<crate::context_recall::ScrolledResponseItem>> {
        let rows = sqlx::query(
            "SELECT id, item_json
             FROM response_items
             WHERE session_id = ?1
               AND id BETWEEN (?2 - ?3) AND (?2 + ?3)
             ORDER BY id ASC",
        )
        .bind(session_id)
        .bind(around_message_id)
        .bind(window_size)
        .fetch_all(&self.pool)
        .await?;

        let result: Vec<crate::context_recall::ScrolledResponseItem> = rows
            .iter()
            .map(|row| -> Result<_> {
                let id: i64 = row.get::<i64, _>(0);
                Ok(crate::context_recall::ScrolledResponseItem {
                    id,
                    item: serde_json::from_str(&row.get::<String, _>(1))?,
                    is_anchor: id == around_message_id,
                })
            })
            .collect::<Result<_>>()?;
        Ok(result)
    }

    /// 按 `started_at` 降序列出会话；preview 取首条 user content（截断 120 字）。
    /// `project_root` 过滤：`Some(path)` = 仅该项目，`None` = 全部。
    pub async fn list_sessions(
        &self,
        filter: super::SessionListFilter,
        limit: usize,
    ) -> Result<Vec<RecentSession>> {
        self.list_sessions_filtered_by_placement(
            filter,
            super::SessionPlacementFilter::All,
            limit,
            None,
        )
        .await
    }

    pub async fn list_sessions_filtered(
        &self,
        filter: super::SessionListFilter,
        limit: usize,
        project_root: Option<&str>,
    ) -> Result<Vec<RecentSession>> {
        self.list_sessions_filtered_by_placement(
            filter,
            super::SessionPlacementFilter::All,
            limit,
            project_root,
        )
        .await
    }

    pub async fn list_sessions_filtered_by_placement(
        &self,
        filter: super::SessionListFilter,
        placement: super::SessionPlacementFilter,
        limit: usize,
        project_root: Option<&str>,
    ) -> Result<Vec<RecentSession>> {
        if let Some(root) = project_root.filter(|r| !r.is_empty() && *r != "default") {
            if let Some(proj) = self.find_project_by_root(root).await? {
                return self
                    .list_sessions_by_project_placement(filter, placement, limit, &proj.id)
                    .await;
            }
        }
        self.list_sessions_inner(filter, placement, limit, project_root)
            .await
    }

    /// 按 `project_id` 过滤会话列表。
    pub async fn list_sessions_by_project(
        &self,
        filter: super::SessionListFilter,
        limit: usize,
        project_id: &str,
    ) -> Result<Vec<RecentSession>> {
        self.list_sessions_by_project_placement(
            filter,
            super::SessionPlacementFilter::All,
            limit,
            project_id,
        )
        .await
    }

    pub async fn list_sessions_by_project_placement(
        &self,
        filter: super::SessionListFilter,
        placement: super::SessionPlacementFilter,
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
        // Side 会话是进程内临时旁路，不进入普通会话列表。
        conditions.push("COALESCE(s.branch_kind, '') != 'side'");
        add_placement_conditions(&mut conditions, placement);
        conditions.push("s.project_id = ?2");
        let where_clause = format!("WHERE {}", conditions.join(" AND "));
        let sql = format!(
            "SELECT s.id, s.title, s.started_at,
                    (SELECT m.search_text FROM response_items m
                     WHERE m.session_id = s.id
                       AND m.role = 'user'
                       AND TRIM(m.search_text) != ''
                     ORDER BY m.timestamp ASC, m.id ASC
                     LIMIT 1) AS preview,
                    s.ended_at, s.end_reason, s.archived_at, s.pinned_at, s.project_id,
                    s.source
             FROM sessions s
             {where_clause}
             ORDER BY (s.pinned_at IS NULL) ASC, s.pinned_at DESC, s.started_at DESC
             LIMIT ?1"
        );
        let rows = sqlx::query(AssertSqlSafe(sql))
            .bind(limit as i64)
            .bind(project_id)
            .fetch_all(&self.pool)
            .await?;
        let tuples: Vec<_> = rows
            .iter()
            .map(|row| {
                (
                    row.get::<String, _>(0),
                    row.get::<Option<String>, _>(1),
                    row.get::<f64, _>(2),
                    row.get::<Option<String>, _>(3),
                    row.get::<Option<f64>, _>(4),
                    row.get::<Option<String>, _>(5),
                    row.get::<Option<f64>, _>(6),
                    row.get::<Option<f64>, _>(7),
                    row.get::<Option<String>, _>(8),
                    row.get::<String, _>(9),
                )
            })
            .collect();
        Ok(Self::to_recent_sessions(tuples))
    }

    async fn list_sessions_inner(
        &self,
        filter: super::SessionListFilter,
        placement: super::SessionPlacementFilter,
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
        add_placement_conditions(&mut conditions, placement);
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
                    (SELECT m.search_text FROM response_items m
                     WHERE m.session_id = s.id
                       AND m.role = 'user'
                       AND TRIM(m.search_text) != ''
                     ORDER BY m.timestamp ASC, m.id ASC
                     LIMIT 1) AS preview,
                    s.ended_at, s.end_reason, s.archived_at, s.pinned_at, s.project_id,
                    s.source
             FROM sessions s
             {where_clause}
             ORDER BY (s.pinned_at IS NULL) ASC, s.pinned_at DESC, s.started_at DESC
             LIMIT ?1"
        );
        let rows = if let Some(root) = project_root.filter(|r| !r.is_empty() && *r != "default") {
            sqlx::query(AssertSqlSafe(sql.clone()))
                .bind(limit as i64)
                .bind(root)
                .fetch_all(&self.pool)
                .await?
        } else {
            sqlx::query(AssertSqlSafe(sql))
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
        };
        let tuples: Vec<_> = rows
            .iter()
            .map(|row| {
                (
                    row.get::<String, _>(0),
                    row.get::<Option<String>, _>(1),
                    row.get::<f64, _>(2),
                    row.get::<Option<String>, _>(3),
                    row.get::<Option<f64>, _>(4),
                    row.get::<Option<String>, _>(5),
                    row.get::<Option<f64>, _>(6),
                    row.get::<Option<f64>, _>(7),
                    row.get::<Option<String>, _>(8),
                    row.get::<String, _>(9),
                )
            })
            .collect();
        Ok(Self::to_recent_sessions(tuples))
    }

    fn to_recent_sessions(rows: Vec<RecentSessionRow>) -> Vec<RecentSession> {
        rows.into_iter()
            .map(
                |(
                    id,
                    title,
                    started_at,
                    preview,
                    ended_at,
                    end_reason,
                    archived_at,
                    pinned_at,
                    project_id,
                    source,
                )| {
                    RecentSession {
                        id,
                        source,
                        project_id,
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
    pub async fn list_recent_sessions(&self, limit: usize) -> Result<Vec<RecentSession>> {
        self.list_sessions(super::SessionListFilter::Active, limit)
            .await
    }

    async fn collect_session_fts_ids(
        &self,
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
        let rows = sqlx::query(
            "SELECT m.id
             FROM response_items_fts AS f
             JOIN response_items AS m ON m.id = f.rowid
             WHERE m.session_id = ?1 AND response_items_fts MATCH ?2
             ORDER BY rank
             LIMIT ?3",
        )
        .bind(session_id)
        .bind(fts_query)
        .bind(remaining as i64)
        .fetch_all(&self.pool)
        .await?;
        for row in &rows {
            let id: i64 = row.get::<i64, _>(0);
            if seen.insert(id) {
                ids.push(id);
                if ids.len() >= limit {
                    break;
                }
            }
        }
        Ok(())
    }

    async fn collect_fts_hits(&self, q: FtsCollect<'_>) -> Result<()> {
        let remaining = q.limit - q.hits.len() as i64;
        if remaining <= 0 {
            return Ok(());
        }

        let rows = sqlx::query(
            "SELECT m.id, m.session_id, m.role, m.search_text, m.tool_name
             FROM response_items_fts AS f
             JOIN response_items AS m ON m.id = f.rowid
             JOIN sessions AS s ON s.id = m.session_id
             WHERE response_items_fts MATCH ?1
               AND (?2 IS NULL OR s.source = ?2)
               AND (?3 IS NULL OR m.role = ?3)
             ORDER BY m.timestamp DESC, m.id DESC
             LIMIT ?4",
        )
        .bind(q.fts_query)
        .bind(q.source_filter)
        .bind(q.role_filter)
        .bind(remaining)
        .fetch_all(&self.pool)
        .await?;

        for row in &rows {
            let id: i64 = row.get::<i64, _>(0);
            let session_id: String = row.get::<String, _>(1);
            let role: String = row.get::<String, _>(2);
            let text: String = row.get::<String, _>(3);
            let tool_name: Option<String> = row.get::<Option<String>, _>(4);
            if !q.seen.insert(id) {
                continue;
            }
            q.hits.push(SearchHit {
                id,
                session_id,
                role,
                snippet: build_snippet(&text, q.raw_query),
                context: String::new(),
                tool_name,
            });
            if q.hits.len() as i64 >= q.limit {
                break;
            }
        }
        Ok(())
    }

    async fn neighbor_context(&self, session_id: &str, message_id: i64) -> Result<String> {
        let mut parts = Vec::new();
        let prev = sqlx::query(
            "SELECT COALESCE(role, ''), search_text FROM response_items
             WHERE session_id = ?1 AND id < ?2
             ORDER BY id DESC LIMIT 1",
        )
        .bind(session_id)
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = prev {
            let role: String = row.get::<String, _>(0);
            let content: String = row.get::<String, _>(1);
            parts.push(format!("[prev:{role}] {}", truncate_chars(&content, 80)));
        }
        let next = sqlx::query(
            "SELECT COALESCE(role, ''), search_text FROM response_items
             WHERE session_id = ?1 AND id > ?2
             ORDER BY id ASC LIMIT 1",
        )
        .bind(session_id)
        .bind(message_id)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(row) = next {
            let role: String = row.get::<String, _>(0);
            let content: String = row.get::<String, _>(1);
            parts.push(format!("[next:{role}] {}", truncate_chars(&content, 80)));
        }
        Ok(parts.join(" | "))
    }
}
