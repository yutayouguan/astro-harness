//! Agent 调用链 Tracing：按 `session_id` 聚合，并尽量用会话消息补全 I/O。
//!
//! 一次对话会话 = 一条 Trace。优先从 `state.db` 的 chat history 展开
//! user → tool/skill/mcp → llm 调用链（含 input/output），再合并 `usage.db` 的 token/费用。

use chrono::{SecondsFormat, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use types::truncate_chars;

use crate::db::{period_window, UsageDb, UsagePeriod};
use home::{data_dir, default_memory_dir};
use session::SessionStore;

/// 列表默认条数
pub const TRACE_LIST_LIMIT: usize = 50;
/// 单条 Trace 事件上限
pub const TRACE_EVENTS_LIMIT: usize = 500;
/// 单字段 I/O 截断（字符）
const IO_TRUNCATE_CHARS: usize = 12_000;

/// Tracing 洞察完整结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceInsights {
    pub kpis: TraceKpis,
    pub traces: Vec<TraceSummary>,
}

/// Tracing KPI
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TraceKpis {
    pub traces: i64,
    pub events: i64,
    pub llm: i64,
    pub tools: i64,
    pub skills: i64,
}

/// 单条会话 Trace 摘要（含事件链）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSummary {
    pub session_id: String,
    pub agent_id: String,
    /// 首条用户消息预览（便于列表识别）
    #[serde(default)]
    pub title: String,
    pub started_at: String,
    pub ended_at: String,
    pub event_count: i64,
    pub tokens: i64,
    pub cost_usd: f64,
    pub kinds: Vec<String>,
    pub events: Vec<TraceEvent>,
}

/// Trace 上的单个事件（LangSmith 风格 span）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEvent {
    pub id: String,
    pub ts: String,
    pub kind: String,
    pub name: String,
    pub agent_id: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub cost_usd: f64,
    /// 事件墙钟耗时；历史消息可由相邻 user/tool → assistant 或 tool call → result 推导。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    /// 父 span id（工具挂在同轮 assistant 下）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// ok / error / running / …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
}

/// Tracing 查询参数
#[derive(Debug, Clone)]
pub struct TraceInsightsQuery {
    pub period: UsagePeriod,
    pub as_of: Option<String>,
    pub agent_id: Option<String>,
}

/// 查询 Tracing 洞察（会话列表 + 含 I/O 的调用链）
pub async fn query_trace_insights(q: TraceInsightsQuery) -> anyhow::Result<TraceInsights> {
    let (start, end) = period_window(q.period, q.as_of.as_deref())?;
    let agent = q.agent_id.filter(|s| !s.is_empty());
    let db = UsageDb::open_default().await?;
    let summaries = db
        .list_trace_sessions(&start, &end, agent.as_deref(), TRACE_LIST_LIMIT)
        .await?;

    let sessions_dir = data_dir(&default_memory_dir());
    let store = SessionStore::open_sessions_dir(&sessions_dir).await.ok();

    let mut traces = Vec::with_capacity(summaries.len());
    let mut kpi = TraceKpis {
        traces: summaries.len() as i64,
        ..Default::default()
    };

    for s in summaries {
        let usage_rows = db
            .list_trace_events(&s.session_id, TRACE_EVENTS_LIMIT)
            .await?;
        let (mut events, preview_title) = if let Some(store) = store.as_ref() {
            match spans_from_chat_history(store, &s.session_id, &s.agent_id).await {
                Ok(built) if !built.0.is_empty() => built,
                _ => (
                    usage_rows_to_events(&usage_rows, &s.agent_id),
                    String::new(),
                ),
            }
        } else {
            (
                usage_rows_to_events(&usage_rows, &s.agent_id),
                String::new(),
            )
        };
        // 与会话列表保持同一标题真源：优先 sessions.title；
        // 旧会话尚未生成标题时，才回退首条用户消息预览。
        let title = resolved_trace_title(store.as_ref(), &s.session_id, preview_title).await;

        merge_usage_into_spans(&mut events, &usage_rows);
        propagate_turn_ids(&mut events);

        for e in &events {
            kpi.events += 1;
            match e.kind.as_str() {
                "llm" => kpi.llm += 1,
                "tool" | "mcp" | "cron" => kpi.tools += 1,
                "skill" => kpi.skills += 1,
                _ => {}
            }
        }
        let kinds = unique_kinds(&events);
        let event_count = events.len() as i64;
        traces.push(TraceSummary {
            session_id: s.session_id,
            agent_id: s.agent_id,
            title,
            started_at: s.started_at,
            ended_at: s.ended_at,
            event_count,
            tokens: s.tokens,
            cost_usd: s.cost_usd,
            kinds,
            events,
        });
    }

    Ok(TraceInsights { kpis: kpi, traces })
}

async fn resolved_trace_title(
    store: Option<&SessionStore>,
    session_id: &str,
    preview_title: String,
) -> String {
    if let Some(store) = store {
        if let Ok(Some(session)) = store.get_session(session_id).await {
            if let Some(title) = session.title.filter(|t| !t.trim().is_empty()) {
                return title;
            }
        }
    }
    preview_title
}

fn usage_rows_to_events(
    rows: &[crate::db::TraceEventRow],
    fallback_agent: &str,
) -> Vec<TraceEvent> {
    rows.iter()
        .map(|e| TraceEvent {
            id: e.id.clone(),
            ts: e.ts.clone(),
            kind: e.kind.clone(),
            name: e.name.clone(),
            agent_id: if e.agent_id.is_empty() {
                fallback_agent.to_string()
            } else {
                e.agent_id.clone()
            },
            input_tokens: e.input_tokens,
            output_tokens: e.output_tokens,
            total_tokens: e.total_tokens,
            cost_usd: e.cost_usd,
            duration_ms: None,
            parent_id: None,
            status: None,
            input: None,
            output: None,
            turn_id: e.turn_id.clone(),
        })
        .collect()
}

/// 从原始会话消息构建 LangSmith 风格 span 链；返回 (events, title)。
///
/// 不使用 `build_chat_history`，因为它会合并连续 assistant 气泡，导致工具循环中的
/// 多次 LLM 调用被压成一条，输入、输出和耗时也无法逐次对应。
async fn spans_from_chat_history(
    store: &SessionStore,
    session_id: &str,
    agent_id: &str,
) -> anyhow::Result<(Vec<TraceEvent>, String)> {
    let messages = store.get_messages(session_id).await?;
    if messages.is_empty() {
        return Ok((Vec::new(), String::new()));
    }

    let mut events = Vec::new();
    let mut title = String::new();
    let mut pending_tools: std::collections::HashMap<String, (usize, f64)> =
        std::collections::HashMap::new();
    let mut last_llm_input: Option<String> = None;
    let mut last_input_at: Option<f64> = None;

    for msg in messages {
        let ts = epoch_to_rfc3339(msg.timestamp);
        match msg.role.as_str() {
            "user" => {
                let content = msg.content.unwrap_or_default();
                if title.is_empty() && !content.trim().is_empty() {
                    title = truncate_chars(&content, 80);
                }
                last_llm_input = nonempty_truncated(&content);
                last_input_at = Some(msg.timestamp);
                events.push(TraceEvent {
                    id: format!("user-{}", msg.id),
                    ts,
                    kind: "user".into(),
                    name: "user".into(),
                    agent_id: agent_id.to_string(),
                    input_tokens: 0,
                    output_tokens: 0,
                    total_tokens: 0,
                    cost_usd: 0.0,
                    duration_ms: None,
                    parent_id: None,
                    status: Some("ok".into()),
                    input: None,
                    output: nonempty_truncated(&content),
                    turn_id: None,
                });
            }
            "assistant" => {
                let parent = format!("llm-{}", msg.id);
                let output = msg.content.as_deref().and_then(nonempty_truncated);
                let has_tool_calls = matches!(msg.tool_calls.as_ref(), Some(serde_json::Value::Array(a)) if !a.is_empty());
                let reasoning = msg
                    .reasoning
                    .as_deref()
                    .or(msg.reasoning_content.as_deref())
                    .and_then(nonempty_truncated);

                if has_tool_calls || output.is_some() || reasoning.is_some() {
                    events.push(TraceEvent {
                        id: parent.clone(),
                        ts: ts.clone(),
                        kind: "llm".into(),
                        name: "assistant".into(),
                        agent_id: agent_id.to_string(),
                        input_tokens: 0,
                        output_tokens: 0,
                        total_tokens: 0,
                        cost_usd: 0.0,
                        duration_ms: last_input_at
                            .and_then(|start| elapsed_ms(start, msg.timestamp)),
                        parent_id: None,
                        status: Some("ok".into()),
                        input: last_llm_input.clone(),
                        output,
                        turn_id: None,
                    });
                }

                if let Some(serde_json::Value::Array(calls)) = msg.tool_calls.as_ref() {
                    for call in calls {
                        let Some((id, tool_name, input)) = parse_tool_call(call) else {
                            continue;
                        };
                        let (kind, name) = classify_activity(&tool_name, input.as_deref());
                        let idx = events.len();
                        pending_tools.insert(id.clone(), (idx, msg.timestamp));
                        events.push(TraceEvent {
                            id: format!("act-{id}"),
                            ts: ts.clone(),
                            kind: kind.into(),
                            name,
                            agent_id: agent_id.to_string(),
                            input_tokens: 0,
                            output_tokens: 0,
                            total_tokens: 0,
                            cost_usd: 0.0,
                            duration_ms: None,
                            parent_id: Some(parent.clone()),
                            status: Some("running".into()),
                            input: input.as_deref().and_then(nonempty_truncated),
                            output: None,
                            turn_id: None,
                        });
                    }
                }
                last_llm_input = None;
                last_input_at = None;
            }
            "tool" => {
                let output = msg.content.as_deref().and_then(nonempty_truncated);
                let is_error = msg
                    .content
                    .as_deref()
                    .map(|s| s.starts_with("工具错误") || s.starts_with("Tool error"))
                    .unwrap_or(false);
                let status = if is_error { "error" } else { "done" };
                let call_id = msg.tool_call_id.as_deref().unwrap_or("");
                if let Some((idx, started_at)) = pending_tools.remove(call_id) {
                    if let Some(event) = events.get_mut(idx) {
                        event.output = output.clone();
                        event.status = Some(status.into());
                        event.duration_ms = elapsed_ms(started_at, msg.timestamp);
                        if event.name == "tool" {
                            if let Some(name) = msg.tool_name.as_deref() {
                                event.name = name.to_string();
                            }
                        }
                    }
                } else {
                    let tool_name = msg.tool_name.as_deref().unwrap_or("tool");
                    let (kind, name) = classify_activity(tool_name, None);
                    events.push(TraceEvent {
                        id: format!("tool-{}", msg.id),
                        ts,
                        kind: kind.into(),
                        name,
                        agent_id: agent_id.to_string(),
                        input_tokens: 0,
                        output_tokens: 0,
                        total_tokens: 0,
                        cost_usd: 0.0,
                        duration_ms: None,
                        parent_id: None,
                        status: Some(status.into()),
                        input: None,
                        output,
                        turn_id: None,
                    });
                }
                last_llm_input = msg.content.as_deref().and_then(nonempty_truncated);
                last_input_at = Some(msg.timestamp);
            }
            _ => {}
        }
        if events.len() >= TRACE_EVENTS_LIMIT {
            break;
        }
    }

    Ok((events, title))
}

fn elapsed_ms(start: f64, end: f64) -> Option<i64> {
    let ms = ((end - start) * 1000.0).round() as i64;
    (ms >= 0).then_some(ms)
}

fn parse_tool_call(call: &serde_json::Value) -> Option<(String, String, Option<String>)> {
    let id = call.get("id")?.as_str()?.trim().to_string();
    if id.is_empty() {
        return None;
    }
    let name = call
        .get("name")
        .and_then(|v| v.as_str())
        .or_else(|| {
            call.get("function")
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("tool")
        .to_string();
    let input = call
        .get("arguments")
        .cloned()
        .or_else(|| {
            call.get("function")
                .and_then(|f| f.get("arguments"))
                .cloned()
        })
        .map(|v| match v {
            serde_json::Value::String(s) => s,
            other => other.to_string(),
        });
    Some((id, name, input))
}

fn classify_activity(title: &str, input: Option<&str>) -> (&'static str, String) {
    if title.starts_with("mcp__") {
        return ("mcp", title.to_string());
    }
    if title == "skills" {
        if let Some(skill_id) = extract_skill_id(input) {
            return ("skill", skill_id);
        }
        return ("skill", "skills".into());
    }
    ("tool", title.to_string())
}

fn extract_skill_id(input: Option<&str>) -> Option<String> {
    let raw = input?.trim();
    if raw.is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    v.get("skill_id")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 将 llm 上的 turn_id 回填到同回合前面的 user / tool / skill / mcp。
/// chat history 展开的 span 默认没有 turn_id，只有 usage 合并后的 llm 带 id。
fn propagate_turn_ids(events: &mut [TraceEvent]) {
    for i in 0..events.len() {
        if events[i].kind != "llm" {
            continue;
        }
        let Some(tid) = events[i]
            .turn_id
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
        else {
            continue;
        };
        let mut j = i;
        while j > 0 {
            j -= 1;
            if events[j].kind == "llm" {
                break;
            }
            if events[j]
                .turn_id
                .as_ref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(true)
            {
                events[j].turn_id = Some(tid.clone());
            }
            if events[j].kind == "user" {
                break;
            }
        }
    }
}

/// 将 usage 中的 llm token/费用按顺序合并到 chat history 的 llm span；
/// 若 history 无 llm 名，用 usage 的模型名覆盖。
fn merge_usage_into_spans(events: &mut [TraceEvent], usage_rows: &[crate::db::TraceEventRow]) {
    let llm_usage: Vec<_> = usage_rows.iter().filter(|e| e.kind == "llm").collect();
    let mut i = 0usize;
    for ev in events.iter_mut() {
        if ev.kind != "llm" {
            continue;
        }
        let Some(u) = llm_usage.get(i) else {
            break;
        };
        ev.input_tokens = u.input_tokens;
        ev.output_tokens = u.output_tokens;
        ev.total_tokens = u.total_tokens;
        ev.cost_usd = u.cost_usd;
        if ev.name == "assistant" && !u.name.is_empty() {
            ev.name = u.name.clone();
        }
        if !u.agent_id.is_empty() {
            ev.agent_id = u.agent_id.clone();
        }
        ev.turn_id = u.turn_id.clone();
        i += 1;
    }
}

fn epoch_to_rfc3339(epoch_secs: f64) -> String {
    let secs = epoch_secs.floor() as i64;
    let nsecs = ((epoch_secs - secs as f64) * 1_000_000_000.0).round() as u32;
    match Utc.timestamp_opt(secs, nsecs) {
        chrono::LocalResult::Single(dt) => dt.to_rfc3339_opts(SecondsFormat::Secs, true),
        _ => Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
    }
}

fn nonempty_truncated(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(truncate_chars(t, IO_TRUNCATE_CHARS))
    }
}

fn unique_kinds(events: &[TraceEvent]) -> Vec<String> {
    let mut out = Vec::new();
    for e in events {
        if !out.iter().any(|k| k == &e.kind) {
            out.push(e.kind.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{NewUsageEvent, UsageDb};
    use serde_json::json;
    use session::{NewMessage, SessionStore};
    use tempfile::TempDir;

    use home::test_env::AstroMemoryDirGuard;

    struct Evt<'a> {
        ts: &'a str,
        kind: &'a str,
        name: &'a str,
        agent_id: &'a str,
        session_id: &'a str,
        input_tokens: i64,
        output_tokens: i64,
        total_tokens: i64,
        cost_usd: f64,
    }

    fn evt(e: Evt<'_>) -> NewUsageEvent {
        NewUsageEvent {
            ts: e.ts.into(),
            kind: e.kind.into(),
            name: e.name.into(),
            agent_id: e.agent_id.into(),
            session_id: Some(e.session_id.into()),
            turn_id: None,
            input_tokens: e.input_tokens,
            output_tokens: e.output_tokens,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: e.total_tokens,
            cost_usd: e.cost_usd,
            cost_status: None,
            cost_source: None,
            pricing_version: None,
            billing_provider: None,
            billing_base_url: None,
            billing_mode: None,
            meta_json: None,
        }
    }

    #[tokio::test]
    async fn trace_title_prefers_stored_session_title() {
        let dir = TempDir::new().unwrap();
        let store = SessionStore::open(&dir.path().join("state.db"))
            .await
            .unwrap();
        store.ensure_session("s-title", "test").await.unwrap();
        store
            .set_session_title("s-title", "云南采菌子女孩")
            .await
            .unwrap();

        assert_eq!(
            resolved_trace_title(Some(&store), "s-title", "首条用户消息".into()).await,
            "云南采菌子女孩"
        );
        assert_eq!(
            resolved_trace_title(Some(&store), "missing", "首条用户消息".into()).await,
            "首条用户消息"
        );
    }

    #[tokio::test]
    async fn propagate_turn_ids_fills_user_and_tool_before_llm() {
        let mut events = vec![
            TraceEvent {
                id: "u1".into(),
                ts: "2026-07-13T10:00:00Z".into(),
                kind: "user".into(),
                name: "user".into(),
                agent_id: "a".into(),
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
                cost_usd: 0.0,
                duration_ms: None,
                parent_id: None,
                status: None,
                input: None,
                output: Some("查天气".into()),
                turn_id: None,
            },
            TraceEvent {
                id: "t1".into(),
                ts: "2026-07-13T10:00:01Z".into(),
                kind: "tool".into(),
                name: "web_search".into(),
                agent_id: "a".into(),
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
                cost_usd: 0.0,
                duration_ms: None,
                parent_id: None,
                status: None,
                input: None,
                output: None,
                turn_id: None,
            },
            TraceEvent {
                id: "l1".into(),
                ts: "2026-07-13T10:00:02Z".into(),
                kind: "llm".into(),
                name: "gpt".into(),
                agent_id: "a".into(),
                input_tokens: 1,
                output_tokens: 1,
                total_tokens: 2,
                cost_usd: 0.0,
                duration_ms: None,
                parent_id: None,
                status: None,
                input: None,
                output: None,
                turn_id: Some("turn-abc".into()),
            },
        ];
        propagate_turn_ids(&mut events);
        assert_eq!(events[0].turn_id.as_deref(), Some("turn-abc"));
        assert_eq!(events[1].turn_id.as_deref(), Some("turn-abc"));
        assert_eq!(events[2].turn_id.as_deref(), Some("turn-abc"));
    }

    #[tokio::test]
    async fn list_trace_events_includes_turn_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.db");
        let db = UsageDb::new(path).await.unwrap();
        db.insert(NewUsageEvent {
            ts: "2026-07-14T12:00:00Z".into(),
            kind: "llm".into(),
            name: "m".into(),
            agent_id: "a".into(),
            session_id: Some("sess-1".into()),
            turn_id: Some("turn-abc".into()),
            input_tokens: 1,
            output_tokens: 2,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 3,
            cost_usd: 0.01,
            cost_status: Some("estimated".into()),
            cost_source: None,
            pricing_version: None,
            billing_provider: None,
            billing_base_url: None,
            billing_mode: None,
            meta_json: None,
        })
        .await
        .unwrap();
        let rows = db.list_trace_events("sess-1", 50).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].turn_id.as_deref(), Some("turn-abc"));
    }

    #[tokio::test]
    async fn traces_group_by_session_and_order_events() {
        let dir = TempDir::new().unwrap();
        let _env = AstroMemoryDirGuard::set(dir.path());
        let db = UsageDb::new(dir.path().join("usage.db")).await.unwrap();
        db.insert(evt(Evt {
            ts: "2026-07-13T10:00:00Z",
            kind: "llm",
            name: "gpt-5.6",
            agent_id: "default",
            session_id: "s1",
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
            cost_usd: 0.01,
        }))
        .await
        .unwrap();
        db.insert(evt(Evt {
            ts: "2026-07-13T10:00:01Z",
            kind: "tool",
            name: "terminal",
            agent_id: "default",
            session_id: "s1",
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            cost_usd: 0.0,
        }))
        .await
        .unwrap();
        db.insert(evt(Evt {
            ts: "2026-07-13T11:00:00Z",
            kind: "llm",
            name: "gpt-5.6",
            agent_id: "other",
            session_id: "s2",
            input_tokens: 1,
            output_tokens: 1,
            total_tokens: 2,
            cost_usd: 0.0,
        }))
        .await
        .unwrap();

        let insights = query_trace_insights(TraceInsightsQuery {
            period: UsagePeriod::Month,
            as_of: Some("2026-07-13T12:00:00Z".into()),
            agent_id: None,
        })
        .await
        .unwrap();
        assert_eq!(insights.kpis.traces, 2);
        assert_eq!(insights.kpis.events, 3);
        assert_eq!(insights.kpis.llm, 2);
        assert_eq!(insights.kpis.tools, 1);
        let s1 = insights
            .traces
            .iter()
            .find(|t| t.session_id == "s1")
            .expect("s1");
        assert_eq!(s1.events.len(), 2);
        assert_eq!(s1.events[0].kind, "llm");
        assert_eq!(s1.events[1].kind, "tool");
    }

    #[tokio::test]
    async fn chat_history_builds_io_chain_and_merges_llm_usage() {
        let dir = TempDir::new().unwrap();
        let _env = AstroMemoryDirGuard::set(dir.path());

        let sessions = dir.path().join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let store = SessionStore::open(&sessions.join("state.db"))
            .await
            .unwrap();
        store.ensure_session("s-io", "test").await.unwrap();
        store
            .set_session_title("s-io", "云南采菌子女孩")
            .await
            .unwrap();
        store
            .append_message(NewMessage {
                content: Some("帮我查天气"),
                ..NewMessage::empty("s-io", "user")
            })
            .await
            .unwrap();
        store
            .append_message(NewMessage {
                content: Some("好的"),
                tool_calls: Some(json!([{
                    "id": "call_1",
                    "type": "function",
                    "function": {
                        "name": "web_search",
                        "arguments": "{\"query\":\"weather\"}"
                    }
                }])),
                ..NewMessage::empty("s-io", "assistant")
            })
            .await
            .unwrap();
        store
            .append_message(NewMessage {
                content: Some("晴天 25°C"),
                tool_call_id: Some("call_1"),
                tool_name: Some("web_search"),
                ..NewMessage::empty("s-io", "tool")
            })
            .await
            .unwrap();
        store
            .append_message(NewMessage {
                content: Some("今天晴，约 25°C。"),
                reasoning: Some("先搜索再回答"),
                ..NewMessage::empty("s-io", "assistant")
            })
            .await
            .unwrap();

        let db = UsageDb::new(dir.path().join("usage.db")).await.unwrap();
        db.insert(evt(Evt {
            ts: "2026-07-13T10:00:00Z",
            kind: "llm",
            name: "gpt-test",
            agent_id: "default",
            session_id: "s-io",
            input_tokens: 100,
            output_tokens: 40,
            total_tokens: 140,
            cost_usd: 0.02,
        }))
        .await
        .unwrap();
        db.insert(evt(Evt {
            ts: "2026-07-13T10:00:05Z",
            kind: "llm",
            name: "gpt-test",
            agent_id: "default",
            session_id: "s-io",
            input_tokens: 50,
            output_tokens: 20,
            total_tokens: 70,
            cost_usd: 0.01,
        }))
        .await
        .unwrap();

        let insights = query_trace_insights(TraceInsightsQuery {
            period: UsagePeriod::Month,
            as_of: Some("2026-07-13T12:00:00Z".into()),
            agent_id: None,
        })
        .await
        .unwrap();
        let tr = insights
            .traces
            .iter()
            .find(|t| t.session_id == "s-io")
            .expect("s-io");
        assert_eq!(tr.title, "云南采菌子女孩");
        let kinds: Vec<_> = tr.events.iter().map(|e| e.kind.as_str()).collect();
        assert!(kinds.contains(&"user"));
        assert!(kinds.contains(&"tool"));
        assert!(kinds.contains(&"llm"));

        let tool = tr.events.iter().find(|e| e.kind == "tool").unwrap();
        assert_eq!(tool.name, "web_search");
        assert!(tool.input.as_ref().unwrap().contains("weather"));
        assert_eq!(tool.output.as_deref(), Some("晴天 25°C"));
        assert!(tool.duration_ms.is_some());

        let llms: Vec<_> = tr.events.iter().filter(|e| e.kind == "llm").collect();
        assert_eq!(llms.len(), 2);
        assert_eq!(llms[0].name, "gpt-test");
        assert_eq!(llms[0].total_tokens, 140);
        assert_eq!(llms[0].input.as_deref(), Some("帮我查天气"));
        assert!(llms[0].duration_ms.is_some());
        assert_eq!(llms[1].output.as_deref(), Some("今天晴，约 25°C。"));
        assert_eq!(llms[1].input.as_deref(), Some("晴天 25°C"));
        assert!(llms[1].duration_ms.is_some());
    }
}
