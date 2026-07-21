//! 上下文占用分层估算（ceil(chars/4)），与账单 Usage 无关。

use common::message::{Message, Role};
use serde::{Deserialize, Serialize};

pub const DEFAULT_CONTEXT_WINDOW: u32 = 128_000;

const SUBAGENT_TOOLS: &[&str] = &[
    "delegate",
    "delegate",
    "orchestrate",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextUsageSegmentMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextUsageSegment {
    pub id: String,
    pub tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ContextUsageSegmentMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextUsageSnapshot {
    pub context_window: u32,
    pub total_tokens: u32,
    pub segments: Vec<ContextUsageSegment>,
    pub updated_at: i64,
    /// 建议用户执行会话级 `/compact`（占用临界或维护后仍高）。
    #[serde(default)]
    pub recommend_compact: bool,
}

impl ContextUsageSnapshot {
    pub fn segment(&self, id: &str) -> Option<&ContextUsageSegment> {
        self.segments.iter().find(|s| s.id == id)
    }
}

pub struct ContextUsageInput<'a> {
    pub system_chars: usize,
    pub memory_chars: usize,
    pub skills_chars: usize,
    pub recall_chars: usize,
    pub tools: &'a [serde_json::Value],
    pub messages: &'a [Message],
    pub context_window: u32,
    pub updated_at_ms: i64,
    pub recommend_compact: bool,
    /// 占用 ≥ 该比例时也建议 `/compact`；默认 0.85。
    pub recommend_compact_ratio: f32,
}

pub fn estimate_tokens(chars: usize) -> u32 {
    u32::try_from(chars.div_ceil(4)).unwrap_or(u32::MAX)
}

pub fn is_subagent_tool_name(name: &str) -> bool {
    SUBAGENT_TOOLS.contains(&name)
}

fn push_seg(out: &mut Vec<ContextUsageSegment>, id: &str, chars: usize, count: Option<u32>) {
    let tokens = estimate_tokens(chars);
    if tokens == 0 {
        return;
    }
    out.push(ContextUsageSegment {
        id: id.to_string(),
        tokens,
        meta: count.map(|c| ContextUsageSegmentMeta { count: Some(c) }),
    });
}

fn tool_schema_name(tool: &serde_json::Value) -> Option<&str> {
    tool.get("function")
        .and_then(|f| f.get("name"))
        .and_then(|n| n.as_str())
        .or_else(|| tool.get("name").and_then(|n| n.as_str()))
}

pub fn build_snapshot(input: ContextUsageInput<'_>) -> ContextUsageSnapshot {
    let mut tools_chars = 0usize;
    let mut mcp_chars = 0usize;
    let mut tools_n = 0u32;
    let mut mcp_n = 0u32;
    for t in input.tools {
        let s = t.to_string();
        let n = tool_schema_name(t).unwrap_or("");
        if n.starts_with("mcp__") {
            mcp_chars += s.len();
            mcp_n += 1;
        } else {
            tools_chars += s.len();
            tools_n += 1;
        }
    }

    let mut call_names: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for m in input.messages {
        if let Some(calls) = &m.tool_calls {
            for c in calls {
                call_names.insert(c.id.clone(), c.name.clone());
            }
        }
    }

    let mut conversation_chars = 0usize;
    let mut subagent_chars = 0usize;
    let mut subagent_n = 0u32;
    let mut msg_n = 0u32;
    for m in input.messages {
        let text = m.content_str();
        match m.role {
            Role::Tool => {
                let name = m
                    .tool_call_id
                    .as_ref()
                    .and_then(|id| call_names.get(id))
                    .map(String::as_str)
                    .unwrap_or("");
                if is_subagent_tool_name(name) {
                    subagent_chars += text.len();
                    subagent_n += 1;
                } else {
                    conversation_chars += text.len();
                    msg_n += 1;
                }
            }
            Role::System => {
                // system 已在分层字符中统计；会话里偶发 system 归入 conversation
                conversation_chars += text.len();
                msg_n += 1;
            }
            _ => {
                conversation_chars += text.len();
                if let Some(calls) = &m.tool_calls {
                    for c in calls {
                        if !is_subagent_tool_name(&c.name) {
                            conversation_chars += c.name.len() + c.arguments.to_string().len();
                        }
                    }
                }
                msg_n += 1;
            }
        }
    }

    let mut segments = Vec::new();
    push_seg(&mut segments, "system", input.system_chars, None);
    push_seg(
        &mut segments,
        "tools",
        tools_chars,
        (tools_n > 0).then_some(tools_n),
    );
    push_seg(
        &mut segments,
        "mcp",
        mcp_chars,
        (mcp_n > 0).then_some(mcp_n),
    );
    push_seg(&mut segments, "memory", input.memory_chars, None);
    push_seg(&mut segments, "skills", input.skills_chars, None);
    push_seg(&mut segments, "recall", input.recall_chars, None);
    push_seg(
        &mut segments,
        "subagent",
        subagent_chars,
        (subagent_n > 0).then_some(subagent_n),
    );
    push_seg(
        &mut segments,
        "conversation",
        conversation_chars,
        (msg_n > 0).then_some(msg_n),
    );

    let total_tokens = segments.iter().map(|s| s.tokens).sum();
    let window = if input.context_window == 0 {
        DEFAULT_CONTEXT_WINDOW
    } else {
        input.context_window
    };

    let recommend_ratio = if input.recommend_compact_ratio > 0.0 {
        input.recommend_compact_ratio as f64
    } else {
        0.85
    };
    ContextUsageSnapshot {
        context_window: window,
        total_tokens,
        segments,
        updated_at: input.updated_at_ms,
        recommend_compact: input.recommend_compact
            || (window > 0 && total_tokens as f64 / window as f64 >= recommend_ratio),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::message::{Message, ToolCall};

    #[test]
    fn estimate_tokens_ceil_div_4() {
        assert_eq!(estimate_tokens(0), 0);
        assert_eq!(estimate_tokens(1), 1);
        assert_eq!(estimate_tokens(4), 1);
        assert_eq!(estimate_tokens(5), 2);
    }

    #[test]
    fn mcp_prefix_goes_to_mcp_segment() {
        let tools: Vec<serde_json::Value> = serde_json::json!([
            {"type":"function","function":{"name":"file_ops","parameters":{}}},
            {"type":"function","function":{"name":"mcp__fs__read","parameters":{"a":1}}}
        ])
        .as_array()
        .unwrap()
        .clone();
        let snap = build_snapshot(ContextUsageInput {
            system_chars: 40,
            memory_chars: 0,
            skills_chars: 0,
            recall_chars: 0,
            tools: &tools,
            messages: &[],
            context_window: 128_000,
            updated_at_ms: 1,
            recommend_compact: false,
            recommend_compact_ratio: 0.85,
        });
        let tools_seg = snap.segment("tools").unwrap();
        let mcp_seg = snap.segment("mcp").unwrap();
        assert!(tools_seg.tokens > 0);
        assert!(mcp_seg.tokens > 0);
        assert_eq!(mcp_seg.meta.as_ref().and_then(|m| m.count), Some(1));
    }

    #[test]
    fn delegate_tool_result_counts_as_subagent() {
        let assistant = Message::assistant_with_tools(
            "",
            vec![ToolCall {
                id: "c1".into(),
                name: "delegate".into(),
                arguments: serde_json::json!({}),
                signature: None,
            }],
        );
        let tool = Message::tool_with_id("c1", &"x".repeat(40));
        let snap = build_snapshot(ContextUsageInput {
            system_chars: 0,
            memory_chars: 0,
            skills_chars: 0,
            recall_chars: 0,
            tools: &[],
            messages: &[assistant, tool],
            context_window: 128_000,
            updated_at_ms: 1,
            recommend_compact: false,
            recommend_compact_ratio: 0.85,
        });
        assert_eq!(snap.segment("subagent").map(|s| s.tokens), Some(10));
        assert_eq!(
            snap.segment("conversation").map(|s| s.tokens).unwrap_or(0),
            0
        );
    }
}
