//! 回合后记忆自我改进：digest 构建与建议应用。
//!
//! LLM 调用由 Agent / Tauri 层负责；本模块提供可测的解析与落盘逻辑。
//! 触发方式：主 turn 成功后可由调用方异步执行（见 `docs/memory.md`）。

use serde::{Deserialize, Serialize};

use crate::session::manager::{MemoryManager, MemoryTarget};

/// 一条记忆变更建议（与 `memory` 工具字段对齐）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewSuggestion {
    pub action: String,
    pub target: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub old_text: Option<String>,
}

/// Review 模型输出的可应用结构。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ReviewOutput {
    #[serde(default)]
    pub suggestions: Vec<ReviewSuggestion>,
    #[serde(default)]
    pub daily_note: Option<String>,
}

/// 供 LLM 使用的 system 指引（调用方拼进 prompt）。
pub const REVIEW_SYSTEM_PROMPT: &str = r#"你是记忆精炼助手。根据对话 digest，只输出 JSON（不要 markdown 围栏）：
{"suggestions":[{"action":"add|replace|remove","target":"memory|user","content":"...","old_text":"..."}],"daily_note":"可选今日日记一行"}
规则：只保存持久偏好/约定/环境事实；跳过临时任务细节；条目尽量短。"#;

/// 从近期对话消息构建 digest。
///
/// `messages` 为 `(role, content)`，按时间升序；取末尾 `recent_n` 轮完整，更早部分做成短摘要。
pub fn build_review_digest(messages: &[(String, String)], recent_n: usize) -> String {
    if messages.is_empty() {
        return String::new();
    }
    let recent_n = recent_n.max(1);
    let split = messages.len().saturating_sub(recent_n);
    let mut out = String::new();
    if split > 0 {
        out.push_str("## Earlier (compact)\n");
        for (role, content) in &messages[..split] {
            let snippet: String = content.chars().take(120).collect();
            out.push_str(&format!("- {role}: {snippet}\n"));
        }
        out.push('\n');
    }
    out.push_str("## Recent\n");
    for (role, content) in &messages[split..] {
        out.push_str(&format!("### {role}\n{content}\n\n"));
    }
    out
}

/// 解析 LLM 文本为 [`ReviewOutput`]（容忍外层 ```json 围栏）。
pub fn parse_review_llm_output(raw: &str) -> anyhow::Result<ReviewOutput> {
    let trimmed = raw.trim();
    let json_str = if let Some(start) = trimmed.find('{') {
        let end = trimmed
            .rfind('}')
            .ok_or_else(|| anyhow::anyhow!("review 输出缺少 JSON 对象结尾"))?;
        &trimmed[start..=end]
    } else {
        anyhow::bail!("review 输出不含 JSON 对象");
    };
    Ok(serde_json::from_str(json_str)?)
}

fn parse_target(raw: &str) -> anyhow::Result<MemoryTarget> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "user" => Ok(MemoryTarget::User),
        "memory" => Ok(MemoryTarget::Memory),
        other => anyhow::bail!("未知 review target: {other}"),
    }
}

/// 将建议应用到 [`MemoryManager`]（走 `handle_memory_op_with_source(..., "review")`，尊重 write_approval）。
pub fn apply_review_suggestions(
    mgr: &mut MemoryManager,
    output: &ReviewOutput,
) -> anyhow::Result<Vec<String>> {
    let mut messages = Vec::new();
    for s in &output.suggestions {
        let target = parse_target(&s.target)?;
        let msg = mgr.handle_memory_op_with_source(
            s.action.trim(),
            target,
            s.content.as_deref(),
            s.old_text.as_deref(),
            "review",
        )?;
        messages.push(msg);
    }
    if let Some(note) = output.daily_note.as_deref() {
        let note = note.trim();
        if !note.is_empty() {
            messages.push(mgr.append_daily(note, None)?);
        }
    }
    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::manager::MemoryManager;
    use std::fs;

    #[test]
    fn digest_keeps_recent_verbatim() {
        let msgs: Vec<(String, String)> = (0..8)
            .map(|i| ("user".into(), format!("msg-{i}-long-enough")))
            .collect();
        let d = build_review_digest(&msgs, 3);
        assert!(d.contains("## Earlier"));
        assert!(d.contains("## Recent"));
        assert!(d.contains("### user\nmsg-7-long-enough"));
        assert!(d.contains("- user: msg-0"));
    }

    #[test]
    fn parse_and_apply_add_respects_write_approval() {
        let dir = tempfile::tempdir().unwrap();
        crate::workspace::ensure_workspace(dir.path()).unwrap();
        crate::config::set_write_approval(dir.path(), true).unwrap();
        let mut mgr = MemoryManager::for_agent(dir.path().to_path_buf(), "workspace").unwrap();
        assert!(mgr.config.write_approval);

        let raw = r#"{"suggestions":[{"action":"add","target":"memory","content":"prefers rust"}],"daily_note":"reviewed today"}"#;
        let out = parse_review_llm_output(raw).unwrap();
        let msgs = apply_review_suggestions(&mut mgr, &out).unwrap();
        assert!(msgs.iter().any(|m| m.contains("待审批") || m.contains("pending") || m.contains("入队")));
        // live untouched
        assert!(!mgr.memory.live_render().contains("prefers rust"));
        // diary not gated
        let daily = mgr.daily_content_today();
        assert!(daily.contains("reviewed today"));
    }

    #[test]
    fn apply_without_approval_writes_live() {
        let dir = tempfile::tempdir().unwrap();
        crate::workspace::ensure_workspace(dir.path()).unwrap();
        let mut mgr = MemoryManager::for_agent(dir.path().to_path_buf(), "workspace").unwrap();
        let out = ReviewOutput {
            suggestions: vec![ReviewSuggestion {
                action: "add".into(),
                target: "user".into(),
                content: Some("likes tea".into()),
                old_text: None,
            }],
            daily_note: None,
        };
        apply_review_suggestions(&mut mgr, &out).unwrap();
        assert!(mgr.user.live_render().contains("likes tea"));
        let _ = fs::metadata(dir.path().join("workspace").join("USER.md")).unwrap();
    }
}
