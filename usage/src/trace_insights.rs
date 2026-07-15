//! Agent 调用链 Tracing：按 `session_id` 聚合，并尽量用会话消息补全 I/O。
//!
//! 一次对话会话 = 一条 Trace。优先从 `state.db` 的 chat history 展开
//! user → tool/skill/mcp → llm 调用链（含 input/output），再合并 `usage.db` 的 token/费用。

use chrono::{SecondsFormat, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use session::SessionStore;
use crate::db::{period_window, UsageDb, UsagePeriod};
use home::default_memory_dir;

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
pub fn query_trace_insights(q: TraceInsightsQuery) -> anyhow::Result<TraceInsights> {
    let (start, end) = period_window(q.period, q.as_of.as_deref())?;
    let agent = q.agent_id.filter(|s| !s.is_empty());
    let db = UsageDb::open_default()?;
    let summaries = db.list_trace_sessions(&start, &end, agent.as_deref(), TRACE_LIST_LIMIT)?;

    let sessions_dir = default_memory_dir().join("sessions");
    let store = SessionStore::open_sessions_dir(&sessions_dir).ok();

    let mut traces = Vec::with_capacity(summaries.len());
    let mut kpi = TraceKpis::default();
    kpi.traces = summaries.len() as i64;

    for s in summaries {
        let usage_rows = db.list_trace_events(&s.session_id, TRACE_EVENTS_LIMIT)?;
        let (mut events, title) = if let Some(store) = store.as_ref() {
            match spans_from_chat_history(store, &s.session_id, &s.agent_id) {
                Ok(built) if !built.0.is_empty() => built,
                _ => (usage_rows_to_events(&usage_rows, &s.agent_id), String::new()),
            }
        } else {
            (usage_rows_to_events(&usage_rows, &s.agent_id), String::new())
        };

        merge_usage_into_spans(&mut events, &usage_rows);

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

    Ok(TraceInsights {
        kpis: kpi,
        traces,
    })
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
            parent_id: None,
            status: None,
            input: None,
            output: None,
            turn_id: e.turn_id.clone(),
        })
        .collect()
}

/// 从会话 chat history 构建 LangSmith 风格 span 链；返回 (events, title)。
fn spans_from_chat_history(
    store: &SessionStore,
    session_id: &str,
    agent_id: &str,
) -> anyhow::Result<(Vec<TraceEvent>, String)> {
    let history = store.build_chat_history(session_id, TRACE_EVENTS_LIMIT)?;
    if history.is_empty() {
        return Ok((Vec::new(), String::new()));
    }

    let messages = store.get_messages(session_id)?;
    let mut ts_by_id = std::collections::HashMap::new();
    for m in &messages {
        ts_by_id.insert(m.id.to_string(), epoch_to_rfc3339(m.timestamp));
    }

    let mut events = Vec::new();
    let mut title = String::new();

    for msg in &history {
        let ts = ts_by_id
            .get(&msg.id)
            .cloned()
            .unwrap_or_else(|| Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true));

        match msg.role.as_str() {
            "user" => {
                if title.is_empty() && !msg.content.trim().is_empty() {
                    title = truncate_chars(&msg.content, 80);
                }
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
                    parent_id: None,
                    status: Some("ok".into()),
                    input: None,
                    output: nonempty_truncated(&msg.content),
                    turn_id: None,
                });
            }
            "assistant" => {
                let parent = format!("llm-{}", msg.id);
                for act in &msg.activities {
                    let (kind, name) = classify_activity(&act.title, act.input.as_deref());
                    events.push(TraceEvent {
                        id: format!("act-{}", act.id),
                        ts: ts.clone(),
                        kind: kind.into(),
                        name,
                        agent_id: agent_id.to_string(),
                        input_tokens: 0,
                        output_tokens: 0,
                        total_tokens: 0,
                        cost_usd: 0.0,
                        parent_id: Some(parent.clone()),
                        status: act.status.clone(),
                        input: act.input.as_ref().and_then(|s| nonempty_truncated(s)),
                        output: act.output.as_ref().and_then(|s| nonempty_truncated(s)),
                        turn_id: None,
                    });
                }

                let mut input = None;
                if let Some(r) = msg.reasoning.as_ref().filter(|s| !s.trim().is_empty()) {
                    input = nonempty_truncated(r);
                }
                let output = nonempty_truncated(&msg.content);
                // 有工具或有正文/推理时才记 llm span
                if !msg.activities.is_empty() || output.is_some() || input.is_some() {
                    events.push(TraceEvent {
                        id: parent,
                        ts,
                        kind: "llm".into(),
                        name: "assistant".into(),
                        agent_id: agent_id.to_string(),
                        input_tokens: 0,
                        output_tokens: 0,
                        total_tokens: 0,
                        cost_usd: 0.0,
                        parent_id: None,
                        status: Some("ok".into()),
                        input,
                        output,
                        turn_id: None,
                    });
                }
            }
            _ => {}
        }
    }

    Ok((events, title))
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

/// 将 usage 中的 llm token/费用按顺序合并到 chat history 的 llm span；
/// 若 history 无 llm 名，用 usage 的模型名覆盖。
fn merge_usage_into_spans(
    events: &mut [TraceEvent],
    usage_rows: &[crate::db::TraceEventRow],
) {
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

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    format!("{truncated}…")
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
    use session::{NewMessage, SessionStore};
    use crate::db::{NewUsageEvent, UsageDb};
    use serde_json::json;
    use tempfile::TempDir;

    use home::test_env::AstroMemoryDirGuard;

    fn evt(
        ts: &str,
        kind: &str,
        name: &str,
        agent_id: &str,
        session_id: &str,
        input_tokens: i64,
        output_tokens: i64,
        total_tokens: i64,
        cost_usd: f64,
    ) -> NewUsageEvent {
        NewUsageEvent {
            ts: ts.into(),
            kind: kind.into(),
            name: name.into(),
            agent_id: agent_id.into(),
            session_id: Some(session_id.into()),
            turn_id: None,
            input_tokens,
            output_tokens,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            total_tokens,
            cost_usd,
            cost_status: None,
            cost_source: None,
            pricing_version: None,
            billing_provider: None,
            billing_base_url: None,
            billing_mode: None,
            meta_json: None,
        }
    }

    #[test]
    fn list_trace_events_includes_turn_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.db");
        let db = UsageDb::new(path).unwrap();
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
        .unwrap();
        let rows = db.list_trace_events("sess-1", 50).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].turn_id.as_deref(), Some("turn-abc"));
    }

    #[test]
    fn traces_group_by_session_and_order_events() {
        let dir = TempDir::new().unwrap();
        let _env = AstroMemoryDirGuard::set(dir.path());
        let db = UsageDb::new(dir.path().join("usage.db")).unwrap();
        db.insert(evt(
            "2026-07-13T10:00:00Z",
            "llm",
            "gpt-5.6",
            "workspace",
            "s1",
            10,
            5,
            15,
            0.01,
        ))
        .unwrap();
        db.insert(evt(
            "2026-07-13T10:00:01Z",
            "tool",
            "terminal",
            "workspace",
            "s1",
            0,
            0,
            0,
            0.0,
        ))
        .unwrap();
        db.insert(evt(
            "2026-07-13T11:00:00Z",
            "llm",
            "gpt-5.6",
            "other",
            "s2",
            1,
            1,
            2,
            0.0,
        ))
        .unwrap();

        let insights = query_trace_insights(TraceInsightsQuery {
            period: UsagePeriod::Month,
            as_of: Some("2026-07-13T12:00:00Z".into()),
            agent_id: None,
        })
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

    #[test]
    fn chat_history_builds_io_chain_and_merges_llm_usage() {
        let dir = TempDir::new().unwrap();
        let _env = AstroMemoryDirGuard::set(dir.path());

        let sessions = dir.path().join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let store = SessionStore::open(&sessions.join("state.db")).unwrap();
        store.ensure_session("s-io", "test").unwrap();
        store
            .append_message(NewMessage {
                content: Some("帮我查天气"),
                ..NewMessage::empty("s-io", "user")
            })
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
            .unwrap();
        store
            .append_message(NewMessage {
                content: Some("晴天 25°C"),
                tool_call_id: Some("call_1"),
                tool_name: Some("web_search"),
                ..NewMessage::empty("s-io", "tool")
            })
            .unwrap();
        store
            .append_message(NewMessage {
                content: Some("今天晴，约 25°C。"),
                reasoning: Some("先搜索再回答"),
                ..NewMessage::empty("s-io", "assistant")
            })
            .unwrap();

        let db = UsageDb::new(dir.path().join("usage.db")).unwrap();
        db.insert(evt(
            "2026-07-13T10:00:00Z",
            "llm",
            "gpt-test",
            "workspace",
            "s-io",
            100,
            40,
            140,
            0.02,
        ))
        .unwrap();
        // 第二轮 assistant 也对应一条 llm usage
        db.insert(evt(
            "2026-07-13T10:00:05Z",
            "llm",
            "gpt-test",
            "workspace",
            "s-io",
            50,
            20,
            70,
            0.01,
        ))
        .unwrap();

        let insights = query_trace_insights(TraceInsightsQuery {
            period: UsagePeriod::Month,
            as_of: Some("2026-07-13T12:00:00Z".into()),
            agent_id: None,
        })
        .unwrap();
        let tr = insights
            .traces
            .iter()
            .find(|t| t.session_id == "s-io")
            .expect("s-io");
        assert!(tr.title.contains("天气"));
        let kinds: Vec<_> = tr.events.iter().map(|e| e.kind.as_str()).collect();
        assert!(kinds.contains(&"user"));
        assert!(kinds.contains(&"tool"));
        assert!(kinds.contains(&"llm"));

        let tool = tr.events.iter().find(|e| e.kind == "tool").unwrap();
        assert_eq!(tool.name, "web_search");
        assert!(tool.input.as_ref().unwrap().contains("weather"));
        assert_eq!(tool.output.as_deref(), Some("晴天 25°C"));

        let llms: Vec<_> = tr.events.iter().filter(|e| e.kind == "llm").collect();
        assert_eq!(llms.len(), 2);
        assert_eq!(llms[0].name, "gpt-test");
        assert_eq!(llms[0].total_tokens, 140);
        assert_eq!(llms[1].output.as_deref(), Some("今天晴，约 25°C。"));
        assert_eq!(llms[1].input.as_deref(), Some("先搜索再回答"));
    }
}
