//! 扫描 logs 目录下 agent/errors 日志尾部并按 session/turn 过滤。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    pub lines: usize,
    pub source: LogSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentLogLine {
    pub raw: String,
    pub source: String,
}

const MAX_LINES: usize = 500;

/// 扫描 `agent.log` / `errors.log` 尾部，按 session/turn/level 过滤后返回匹配行。
///
/// - 每个文件内按**从新到旧**（文件末尾优先）遍历；返回顺序与遍历一致。
/// - `LogSource::Both` 先读 `agent.log` 再读 `errors.log`，在共享 `lines` 上限内顺序拼接；
///   **不会**按时间戳跨文件合并排序。
pub fn query_agent_logs(q: AgentLogQuery) -> anyhow::Result<Vec<AgentLogLine>> {
    let limit = q.lines.clamp(1, MAX_LINES);
    let mut out = Vec::new();
    let files: &[(&str, &str)] = match q.source {
        LogSource::Agent => &[("agent", "agent.log")],
        LogSource::Errors => &[("errors", "errors.log")],
        LogSource::Both => &[("agent", "agent.log"), ("errors", "errors.log")],
    };
    for (src, name) in files {
        let path = q.logs_dir.join(name);
        if !path.exists() {
            continue;
        }
        let raw = std::fs::read_to_string(&path)?;
        for line in raw.lines().rev() {
            if !line_matches(line, &q) {
                continue;
            }
            out.push(AgentLogLine {
                raw: line.to_string(),
                source: (*src).into(),
            });
            if out.len() >= limit {
                return Ok(out);
            }
        }
    }
    Ok(out)
}

fn line_matches(line: &str, q: &AgentLogQuery) -> bool {
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
            lines: 1,
            source: LogSource::Both,
        })
        .unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].source, "agent");
        assert!(lines[0].raw.contains("agent-line"));
    }
}
