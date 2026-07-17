//! DecisionLog：工具失败、用户纠错、关键选择的结构化日志。
//!
//! Append-only JSONL：`{base}/learning/decisions.jsonl`。
//! 默认 LearningMode ≈ Propose（不自动改记忆）；本模块只记一笔。

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 决策类别。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind {
    ToolFailure,
    MemoryRejected,
    UserCorrection,
    KeyChoice,
    Other,
}

/// 一条决策记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionEntry {
    pub id: String,
    pub kind: DecisionKind,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    pub created_at: String,
}

impl DecisionEntry {
    pub fn new(kind: DecisionKind, summary: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            kind,
            summary: summary.into(),
            session_id: None,
            tool_name: None,
            created_at: Utc::now().to_rfc3339(),
        }
    }

    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn with_tool(mut self, tool_name: impl Into<String>) -> Self {
        self.tool_name = Some(tool_name.into());
        self
    }
}

fn learning_dir(base: &Path) -> PathBuf {
    base.join("learning")
}

/// `{base}/learning/decisions.jsonl`
pub fn decisions_path(base: &Path) -> PathBuf {
    learning_dir(base).join("decisions.jsonl")
}

/// 追加一条决策；失败返回 Err。
pub fn append_decision(base: &Path, entry: &DecisionEntry) -> anyhow::Result<()> {
    let dir = learning_dir(base);
    fs::create_dir_all(&dir)?;
    let path = decisions_path(base);
    let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
    serde_json::to_writer(&mut f, entry)?;
    f.write_all(b"\n")?;
    Ok(())
}

/// 尽力写入（失败只 warn）。
pub fn try_append_decision(base: &Path, entry: DecisionEntry) {
    if let Err(e) = append_decision(base, &entry) {
        tracing::warn!(error = %e, "decision_log append failed");
    }
}

/// 读取最近 `n` 条（文件尾部优先）。
pub fn list_recent(base: &Path, n: usize) -> anyhow::Result<Vec<DecisionEntry>> {
    let path = decisions_path(base);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let f = fs::File::open(&path)?;
    let mut all = Vec::new();
    for line in BufReader::new(f).lines() {
        let line = line?;
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        match serde_json::from_str::<DecisionEntry>(t) {
            Ok(e) => all.push(e),
            Err(err) => tracing::warn!(error = %err, "skip bad decision_log line"),
        }
    }
    if all.len() <= n {
        Ok(all)
    } else {
        Ok(all[all.len() - n..].to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn append_and_list_recent() {
        let dir = TempDir::new().unwrap();
        append_decision(
            dir.path(),
            &DecisionEntry::new(DecisionKind::ToolFailure, "web_search timeout")
                .with_tool("web_search")
                .with_session("s1"),
        )
        .unwrap();
        append_decision(
            dir.path(),
            &DecisionEntry::new(DecisionKind::MemoryRejected, "user denied memory add"),
        )
        .unwrap();
        let recent = list_recent(dir.path(), 10).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].kind, DecisionKind::ToolFailure);
        assert_eq!(recent[1].kind, DecisionKind::MemoryRejected);
        let last1 = list_recent(dir.path(), 1).unwrap();
        assert_eq!(last1.len(), 1);
        assert_eq!(last1[0].kind, DecisionKind::MemoryRejected);
    }
}
