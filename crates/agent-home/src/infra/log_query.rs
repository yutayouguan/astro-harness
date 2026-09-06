//! 扫描 logs 目录下 agent/errors 日志尾部并按 session/turn 过滤。

use chrono::DateTime;
use serde::{Deserialize, Serialize};
use std::{cmp::Reverse, collections::HashMap, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogSource {
    Agent,
    Errors,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLogQuery {
    /// 测试可覆写；生产传 `logs_dir()`。
    #[serde(default)]
    pub logs_dir: PathBuf,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub min_level: Option<String>,
    /// Inclusive UTC epoch-millisecond lower bound.
    #[serde(default)]
    pub since_ms: Option<i64>,
    /// Inclusive UTC epoch-millisecond upper bound.
    #[serde(default)]
    pub until_ms: Option<i64>,
    pub lines: usize,
    pub source: LogSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLogLine {
    pub raw: String,
    pub source: String,
    pub timestamp: Option<String>,
    pub level: Option<String>,
    pub message: String,
}

const MAX_LINES: usize = 500;

/// 扫描 `agent.log.YYYY-MM-DD` / `errors.log.YYYY-MM-DD` 滚动日志尾部，
/// 按 session/turn/level 过滤后返回匹配行。同时兼容未滚动的
/// `agent.log` / `errors.log`。
///
/// - 同一来源先读最新日期文件，每个文件内按**从新到旧**（文件末尾优先）遍历。
/// - 所有来源按时间戳统一倒序合并。
/// - `LogSource::Both` 会去除 agent/errors 同时写入的同一条事件。
pub fn query_agent_logs(q: AgentLogQuery) -> anyhow::Result<Vec<AgentLogLine>> {
    anyhow::ensure!(
        !matches!((q.since_ms, q.until_ms), (Some(since), Some(until)) if since > until),
        "log time range start must not be later than end"
    );
    let limit = q.lines.clamp(1, MAX_LINES);
    let mut collected = Vec::new();
    let files: &[(&str, &str)] = match q.source {
        LogSource::Agent => &[("agent", "agent.log")],
        LogSource::Errors => &[("errors", "errors.log")],
        LogSource::Both => &[("agent", "agent.log"), ("errors", "errors.log")],
    };
    for (src, name) in files {
        let mut source_matches = 0usize;
        'source_files: for path in source_log_files(&q.logs_dir, name)? {
            let raw = std::fs::read_to_string(&path)?;
            for line in raw.lines().rev() {
                let parsed = parse_log_line(line);
                if !line_matches(line, parsed.timestamp_ms, &q) {
                    continue;
                }
                collected.push(CollectedLogLine {
                    timestamp_ms: parsed.timestamp_ms,
                    fingerprint: parsed.fingerprint,
                    line: AgentLogLine {
                        raw: line.to_string(),
                        source: (*src).into(),
                        timestamp: parsed.timestamp,
                        level: parsed.level,
                        message: parsed.message,
                    },
                });
                source_matches += 1;
                if source_matches >= limit {
                    break 'source_files;
                }
            }
        }
    }

    collected.sort_by_key(|row| Reverse(row.timestamp_ms.unwrap_or(i64::MIN)));
    if q.source == LogSource::Both {
        collected = deduplicate_cross_source(collected);
    }
    Ok(collected
        .into_iter()
        .take(limit)
        .map(|row| row.line)
        .collect())
}

#[derive(Debug)]
struct CollectedLogLine {
    line: AgentLogLine,
    timestamp_ms: Option<i64>,
    fingerprint: String,
}

#[derive(Debug)]
struct ParsedLogLine {
    timestamp: Option<String>,
    timestamp_ms: Option<i64>,
    level: Option<String>,
    message: String,
    fingerprint: String,
}

fn parse_log_line(raw: &str) -> ParsedLogLine {
    let trimmed = raw.trim_start();
    let (timestamp_raw, after_timestamp) = split_first_token(trimmed);
    let (level_raw, message_raw) = split_first_token(after_timestamp.trim_start());
    let message_raw = message_raw.trim_start();
    let timestamp_ms = DateTime::parse_from_rfc3339(timestamp_raw)
        .ok()
        .map(|value| value.timestamp_millis());
    let timestamp = timestamp_ms.map(|_| timestamp_raw.to_string());
    let level = matches!(
        level_raw.to_ascii_uppercase().as_str(),
        "TRACE" | "DEBUG" | "INFO" | "WARN" | "WARNING" | "ERROR" | "CRITICAL"
    )
    .then(|| level_raw.to_ascii_uppercase());
    let message = if timestamp.is_some() && level.is_some() {
        message_raw.to_string()
    } else {
        raw.to_string()
    };
    let fingerprint = format!("{}\n{}", level.as_deref().unwrap_or_default(), message);
    ParsedLogLine {
        timestamp,
        timestamp_ms,
        level,
        message,
        fingerprint,
    }
}

fn split_first_token(input: &str) -> (&str, &str) {
    input
        .find(char::is_whitespace)
        .map_or((input, ""), |index| (&input[..index], &input[index..]))
}

fn deduplicate_cross_source(rows: Vec<CollectedLogLine>) -> Vec<CollectedLogLine> {
    const DUPLICATE_WINDOW_MS: i64 = 100;
    let mut recent: HashMap<String, (i64, String)> = HashMap::new();
    let mut deduplicated = Vec::with_capacity(rows.len());
    for row in rows {
        let duplicate = row.timestamp_ms.is_some_and(|timestamp_ms| {
            recent
                .get(&row.fingerprint)
                .is_some_and(|(seen_at, source)| {
                    source != &row.line.source
                        && (seen_at - timestamp_ms).abs() <= DUPLICATE_WINDOW_MS
                })
        });
        if duplicate {
            continue;
        }
        if let Some(timestamp_ms) = row.timestamp_ms {
            recent.insert(
                row.fingerprint.clone(),
                (timestamp_ms, row.line.source.clone()),
            );
        }
        deduplicated.push(row);
    }
    deduplicated
}

fn source_log_files(logs_dir: &std::path::Path, base_name: &str) -> anyhow::Result<Vec<PathBuf>> {
    let exact = logs_dir.join(base_name);
    let mut dated = Vec::new();

    let entries = match std::fs::read_dir(logs_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };

    let dated_prefix = format!("{base_name}.");
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let Some(date) = file_name.strip_prefix(&dated_prefix) else {
            continue;
        };
        if is_iso_date(date) {
            dated.push(entry.path());
        }
    }

    // ISO 日期可直接按文件名逆序得到从新到旧。若未来切换为不滚动文件，
    // 则优先读取固定文件名，再补充历史日志。
    dated.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
    if exact.is_file() {
        dated.insert(0, exact);
    }
    Ok(dated)
}

fn is_iso_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit())
}

fn line_matches(line: &str, timestamp_ms: Option<i64>, q: &AgentLogQuery) -> bool {
    if q.since_ms.is_some() || q.until_ms.is_some() {
        let Some(timestamp_ms) = timestamp_ms else {
            return false;
        };
        if q.since_ms.is_some_and(|since| timestamp_ms < since)
            || q.until_ms.is_some_and(|until| timestamp_ms > until)
        {
            return false;
        }
    }
    if let Some(ref sid) = q.session_id {
        if !sid.is_empty() && !line.contains(sid.as_str()) {
            return false;
        }
    }
    if let Some(ref tid) = q.turn_id {
        if !tid.is_empty() && !line.contains(tid.as_str()) {
            return false;
        }
    }
    if let Some(ref lvl) = q.min_level {
        if !level_ok(line, lvl) {
            return false;
        }
    }
    true
}

fn level_ok(line: &str, min: &str) -> bool {
    let order = |s: &str| match s.to_ascii_uppercase().as_str() {
        "DEBUG" => 0,
        "INFO" => 1,
        "WARN" | "WARNING" => 2,
        "ERROR" => 3,
        "CRITICAL" => 4,
        _ => -1,
    };
    let min_o = order(min);
    if min_o < 0 {
        return true;
    }
    for cand in ["CRITICAL", "ERROR", "WARNING", "WARN", "INFO", "DEBUG"] {
        if line.contains(cand) {
            let o = order(cand);
            if o < 0 {
                return true;
            }
            return o >= min_o;
        }
    }
    true // 无法解析则保留
}

pub fn default_agent_log_query() -> AgentLogQuery {
    AgentLogQuery {
        logs_dir: crate::infra::logging::logs_dir(),
        session_id: None,
        turn_id: None,
        min_level: None,
        since_ms: None,
        until_ms: None,
        lines: 50,
        source: LogSource::Both,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_lines(path: &std::path::Path, lines: &[&str]) {
        let mut f = std::fs::File::create(path).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
    }

    #[test]
    fn filters_by_session_and_turn_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let agent = dir.path().join("agent.log");
        write_lines(
            &agent,
            &[
                "INFO keep session_id=s1 turn_id=t1 hello",
                "INFO skip session_id=s2 turn_id=t9 other",
                "WARN err session_id=s1 turn_id=t1 boom",
            ],
        );
        let lines = query_agent_logs(AgentLogQuery {
            logs_dir: dir.path().to_path_buf(),
            session_id: Some("s1".into()),
            turn_id: Some("t1".into()),
            min_level: None,
            since_ms: None,
            until_ms: None,
            lines: 50,
            source: LogSource::Agent,
        })
        .unwrap();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].raw.contains("boom")); // newest first
        assert_eq!(lines[0].source, "agent");
    }

    #[test]
    fn errors_source_only_reads_errors_file() {
        let dir = tempfile::tempdir().unwrap();
        write_lines(&dir.path().join("agent.log"), &["INFO session_id=s1 a"]);
        write_lines(
            &dir.path().join("errors.log"),
            &["WARN session_id=s1 turn_id=t1 e"],
        );
        let lines = query_agent_logs(AgentLogQuery {
            logs_dir: dir.path().to_path_buf(),
            session_id: Some("s1".into()),
            turn_id: None,
            min_level: None,
            since_ms: None,
            until_ms: None,
            lines: 50,
            source: LogSource::Errors,
        })
        .unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].source, "errors");
    }

    #[test]
    fn min_level_warn_filters_info_keeps_warn() {
        let dir = tempfile::tempdir().unwrap();
        write_lines(
            &dir.path().join("agent.log"),
            &[
                "INFO session_id=s1 turn_id=t1 info-only",
                "WARN session_id=s1 turn_id=t1 warn-line",
            ],
        );
        let lines = query_agent_logs(AgentLogQuery {
            logs_dir: dir.path().to_path_buf(),
            session_id: Some("s1".into()),
            turn_id: Some("t1".into()),
            min_level: Some("WARN".into()),
            since_ms: None,
            until_ms: None,
            lines: 50,
            source: LogSource::Agent,
        })
        .unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].raw.contains("warn-line"));
    }

    #[test]
    fn empty_agent_log_returns_empty_vec() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::File::create(dir.path().join("agent.log")).unwrap();
        let lines = query_agent_logs(AgentLogQuery {
            logs_dir: dir.path().to_path_buf(),
            session_id: None,
            turn_id: None,
            min_level: None,
            since_ms: None,
            until_ms: None,
            lines: 50,
            source: LogSource::Agent,
        })
        .unwrap();
        assert!(lines.is_empty());
    }

    #[test]
    fn lines_zero_clamps_to_one() {
        let dir = tempfile::tempdir().unwrap();
        write_lines(
            &dir.path().join("agent.log"),
            &[
                "INFO session_id=s1 turn_id=t1 first",
                "INFO session_id=s1 turn_id=t1 second",
            ],
        );
        let lines = query_agent_logs(AgentLogQuery {
            logs_dir: dir.path().to_path_buf(),
            session_id: Some("s1".into()),
            turn_id: Some("t1".into()),
            min_level: None,
            since_ms: None,
            until_ms: None,
            lines: 0,
            source: LogSource::Agent,
        })
        .unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].raw.contains("second"));
    }

    #[test]
    fn lines_over_max_clamps_to_500() {
        let dir = tempfile::tempdir().unwrap();
        let content: Vec<String> = (0..600)
            .map(|i| format!("INFO session_id=s1 turn_id=t1 line-{i}"))
            .collect();
        write_lines(
            &dir.path().join("agent.log"),
            &content.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        let lines = query_agent_logs(AgentLogQuery {
            logs_dir: dir.path().to_path_buf(),
            session_id: Some("s1".into()),
            turn_id: Some("t1".into()),
            min_level: None,
            since_ms: None,
            until_ms: None,
            lines: 9999,
            source: LogSource::Agent,
        })
        .unwrap();
        assert_eq!(lines.len(), 500);
        assert!(lines[0].raw.contains("line-599"));
        assert!(lines[499].raw.contains("line-100"));
    }

    #[test]
    fn both_source_agent_fills_limit_before_errors() {
        let dir = tempfile::tempdir().unwrap();
        write_lines(
            &dir.path().join("agent.log"),
            &["INFO session_id=s1 turn_id=t1 agent-line"],
        );
        write_lines(
            &dir.path().join("errors.log"),
            &["WARN session_id=s1 turn_id=t1 error-line"],
        );
        let lines = query_agent_logs(AgentLogQuery {
            logs_dir: dir.path().to_path_buf(),
            session_id: Some("s1".into()),
            turn_id: Some("t1".into()),
            min_level: None,
            since_ms: None,
            until_ms: None,
            lines: 1,
            source: LogSource::Both,
        })
        .unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].source, "agent");
        assert!(lines[0].raw.contains("agent-line"));
    }

    #[test]
    fn reads_daily_rolling_logs_newest_file_first() {
        let dir = tempfile::tempdir().unwrap();
        write_lines(
            &dir.path().join("agent.log.2026-09-04"),
            &["INFO previous-day"],
        );
        write_lines(
            &dir.path().join("agent.log.2026-09-05"),
            &["INFO current-day-first", "WARN current-day-latest"],
        );
        write_lines(
            &dir.path().join("agent.log.backup"),
            &["ERROR must-not-be-read"],
        );

        let lines = query_agent_logs(AgentLogQuery {
            logs_dir: dir.path().to_path_buf(),
            session_id: None,
            turn_id: None,
            min_level: None,
            since_ms: None,
            until_ms: None,
            lines: 50,
            source: LogSource::Agent,
        })
        .unwrap();

        assert_eq!(lines.len(), 3);
        assert!(lines[0].raw.contains("current-day-latest"));
        assert!(lines[1].raw.contains("current-day-first"));
        assert!(lines[2].raw.contains("previous-day"));
        assert!(lines
            .iter()
            .all(|line| !line.raw.contains("must-not-be-read")));
    }

    #[test]
    fn unrotated_log_precedes_daily_history() {
        let dir = tempfile::tempdir().unwrap();
        write_lines(&dir.path().join("agent.log.2026-09-05"), &["INFO dated"]);
        write_lines(&dir.path().join("agent.log"), &["INFO active"]);

        let lines = query_agent_logs(AgentLogQuery {
            logs_dir: dir.path().to_path_buf(),
            session_id: None,
            turn_id: None,
            min_level: None,
            since_ms: None,
            until_ms: None,
            lines: 50,
            source: LogSource::Agent,
        })
        .unwrap();

        assert_eq!(lines.len(), 2);
        assert!(lines[0].raw.contains("active"));
        assert!(lines[1].raw.contains("dated"));
    }

    #[test]
    fn filters_by_inclusive_time_range_and_returns_structured_fields() {
        let dir = tempfile::tempdir().unwrap();
        write_lines(
            &dir.path().join("agent.log"),
            &[
                "2026-09-05T10:00:00.000Z  INFO server: before",
                "2026-09-05T10:30:00.000Z  WARN sqlx::query: inside",
                "2026-09-05T11:00:00.000Z  ERROR server: after",
            ],
        );
        let since = DateTime::parse_from_rfc3339("2026-09-05T10:15:00Z")
            .unwrap()
            .timestamp_millis();
        let until = DateTime::parse_from_rfc3339("2026-09-05T10:45:00Z")
            .unwrap()
            .timestamp_millis();
        let lines = query_agent_logs(AgentLogQuery {
            logs_dir: dir.path().to_path_buf(),
            session_id: None,
            turn_id: None,
            min_level: None,
            since_ms: Some(since),
            until_ms: Some(until),
            lines: 50,
            source: LogSource::Agent,
        })
        .unwrap();

        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0].timestamp.as_deref(),
            Some("2026-09-05T10:30:00.000Z")
        );
        assert_eq!(lines[0].level.as_deref(), Some("WARN"));
        assert_eq!(lines[0].message, "sqlx::query: inside");
    }

    #[test]
    fn both_sources_merge_by_time_and_deduplicate_mirrored_events() {
        let dir = tempfile::tempdir().unwrap();
        write_lines(
            &dir.path().join("agent.log"),
            &[
                "2026-09-05T10:00:00.100Z  WARN sqlx::query: mirrored",
                "2026-09-05T10:01:00.000Z  INFO server: newest",
            ],
        );
        write_lines(
            &dir.path().join("errors.log"),
            &[
                "2026-09-05T10:00:00.110Z  WARN sqlx::query: mirrored",
                "2026-09-05T09:59:00.000Z  ERROR updater: unique",
            ],
        );
        let lines = query_agent_logs(AgentLogQuery {
            logs_dir: dir.path().to_path_buf(),
            session_id: None,
            turn_id: None,
            min_level: None,
            since_ms: None,
            until_ms: None,
            lines: 50,
            source: LogSource::Both,
        })
        .unwrap();

        assert_eq!(lines.len(), 3);
        assert!(lines[0].message.contains("newest"));
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.message.contains("mirrored"))
                .count(),
            1
        );
        assert!(lines[2].message.contains("unique"));
    }

    #[test]
    fn rejects_reversed_time_range() {
        let dir = tempfile::tempdir().unwrap();
        let error = query_agent_logs(AgentLogQuery {
            logs_dir: dir.path().to_path_buf(),
            session_id: None,
            turn_id: None,
            min_level: None,
            since_ms: Some(2),
            until_ms: Some(1),
            lines: 50,
            source: LogSource::Both,
        })
        .unwrap_err();
        assert!(error.to_string().contains("start must not be later"));
    }
}
