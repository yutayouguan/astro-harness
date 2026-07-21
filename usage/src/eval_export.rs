//! 将 session trace 导出为 eval 数据集（JSONL）。
//!
//! 复用 [`crate::db::UsageDb`] 事件与可选的 [`session::SessionStore`] 消息，
//! 不必新造监控栈。每行一条会话记录，便于离线评测。

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;

use common::truncate_chars;
use serde::{Deserialize, Serialize};
use session::SessionStore;

use crate::db::UsageDb;
use crate::trace_insights::TRACE_EVENTS_LIMIT;
use home::default_memory_dir;

/// 单行 eval 记录（一个 session）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalSessionRecord {
    pub session_id: String,
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub title: String,
    pub tokens: i64,
    pub cost_usd: f64,
    pub event_count: usize,
    pub events: Vec<EvalEvent>,
    /// 可选：会话消息角色摘要（role + 截断 content）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<EvalMessagePreview>,
}

/// eval 事件（精简）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalEvent {
    pub kind: String,
    pub name: String,
    pub total_tokens: i64,
    pub cost_usd: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

/// 消息预览。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalMessagePreview {
    pub role: String,
    pub content: String,
}

const MSG_TRUNCATE: usize = 2_000;

/// 从默认 `usage.db` + sessions 导出单会话 JSONL（通常一行）。
pub fn export_session_eval_jsonl(session_id: &str, path: &Path) -> anyhow::Result<usize> {
    let db = UsageDb::open_default()?;
    export_session_eval_jsonl_with_db(&db, session_id, path)
}

/// 可注入 [`UsageDb`]（测试用 tempfile）。
pub fn export_session_eval_jsonl_with_db(
    db: &UsageDb,
    session_id: &str,
    path: &Path,
) -> anyhow::Result<usize> {
    let sid = session_id.trim();
    if sid.is_empty() {
        anyhow::bail!("session_id 为空");
    }

    let rows = db.list_trace_events(sid, TRACE_EVENTS_LIMIT)?;
    let agent_id = rows.first().map(|r| r.agent_id.clone()).unwrap_or_default();
    let tokens: i64 = rows.iter().map(|r| r.total_tokens).sum();
    let cost_usd: f64 = rows.iter().map(|r| r.cost_usd).sum();

    let (title, messages) = load_session_previews(sid);
    let events: Vec<EvalEvent> = rows
        .into_iter()
        .map(|r| EvalEvent {
            kind: r.kind,
            name: r.name,
            total_tokens: r.total_tokens,
            cost_usd: r.cost_usd,
            input: None,
            output: None,
        })
        .collect();

    let record = EvalSessionRecord {
        session_id: sid.to_string(),
        agent_id,
        title,
        tokens,
        cost_usd,
        event_count: events.len(),
        events,
        messages,
    };

    write_eval_record_jsonl(&record, path)?;
    Ok(1)
}

fn load_session_previews(session_id: &str) -> (String, Vec<EvalMessagePreview>) {
    let sessions_dir = default_memory_dir().join("sessions");
    let Ok(store) = SessionStore::open_sessions_dir(&sessions_dir) else {
        return (String::new(), Vec::new());
    };
    let Ok(msgs) = store.get_messages(session_id) else {
        return (String::new(), Vec::new());
    };
    let title = msgs
        .iter()
        .find(|m| m.role == "user")
        .and_then(|m| m.content.as_deref())
        .map(|c| truncate_chars(c, 120))
        .unwrap_or_default();
    let previews: Vec<EvalMessagePreview> = msgs
        .iter()
        .take(50)
        .map(|m| EvalMessagePreview {
            role: m.role.clone(),
            content: truncate_chars(m.content.as_deref().unwrap_or(""), MSG_TRUNCATE),
        })
        .collect();
    (title, previews)
}

/// 写单条 eval 记录为 JSONL（单测 / 无 DB 路径）。
pub fn write_eval_record_jsonl(record: &EvalSessionRecord, path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(path)?;
    let mut w = BufWriter::new(file);
    serde_json::to_writer(&mut w, record)?;
    w.write_all(b"\n")?;
    w.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::NewUsageEvent;
    use tempfile::TempDir;

    #[test]
    fn export_jsonl_roundtrip() {
        let dir = TempDir::new().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        let db_path = dir.path().join("usage.db");
        let db = UsageDb::new(db_path).unwrap();
        db.insert(NewUsageEvent {
            ts: "2026-07-17T00:00:00Z".into(),
            kind: "llm".into(),
            name: "gpt-test".into(),
            agent_id: "default".into(),
            session_id: Some("sess-eval-1".into()),
            turn_id: Some("t1".into()),
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 15,
            cost_usd: 0.001,
            cost_status: None,
            cost_source: None,
            pricing_version: None,
            billing_provider: None,
            billing_base_url: None,
            billing_mode: None,
            meta_json: None,
        })
        .unwrap();

        let out = dir.path().join("eval.jsonl");
        let n = export_session_eval_jsonl_with_db(&db, "sess-eval-1", &out).unwrap();
        assert_eq!(n, 1);
        let text = fs::read_to_string(&out).unwrap();
        let rec: EvalSessionRecord = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(rec.session_id, "sess-eval-1");
        assert_eq!(rec.event_count, 1);
        assert_eq!(rec.tokens, 15);
        assert_eq!(rec.events[0].name, "gpt-test");
    }
}
