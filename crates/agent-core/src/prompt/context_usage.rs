//! 上下文占用分层估算（ceil(chars/4)），与账单 Usage 无关。
//! 分段含可选 `items` 明细（单工具 / 单 skill 等）。

use serde::{Deserialize, Serialize};
use types::message::{Message, Role};

pub const DEFAULT_CONTEXT_WINDOW: u32 = 128_000;

const SUBAGENT_TOOLS: &[&str] = &[
    "spawn_agent",
    "list_agents",
    "send_message",
    "followup_task",
    "wait_agent",
    "interrupt_agent",
];

/// 写入上下文的 Agent 编排工具定义（与「子 Agent 返回」区分）。
const AGENT_DEF_TOOLS: &[&str] = &[
    "spawn_agent",
    "list_agents",
    "send_message",
    "followup_task",
    "wait_agent",
    "interrupt_agent",
    "persona_create",
];

fn is_agent_def_tool_name(name: &str) -> bool {
    AGENT_DEF_TOOLS.contains(&name)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextUsageSegmentMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextUsageItem {
    pub id: String,
    pub label: String,
    pub tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextUsageSegment {
    pub id: String,
    pub tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ContextUsageSegmentMeta>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ContextUsageItem>,
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

/// 记忆子块：(id, 展示名, 字符数)
pub type NamedChars = (String, String, usize);

pub struct ContextUsageInput<'a> {
    pub system_chars: usize,
    pub developer_chars: usize,
    pub user_context_chars: usize,
    pub memory_chars: usize,
    pub skills_chars: usize,
    pub recall_chars: usize,
    pub mcp_instruction_chars: usize,
    /// SOUL / guidance 等系统提示子项
    pub system_items: &'a [NamedChars],
    /// 交互模式等 developer message 子项
    pub developer_items: &'a [NamedChars],
    /// AGENTS / Hook / timestamp 等 contextual user message 子项
    pub user_context_items: &'a [NamedChars],
    /// MEMORY / USER / daily 等子项
    pub memory_items: &'a [NamedChars],
    /// 技能索引条目：(skill_id, 展示名, 该行字符数)
    pub skill_items: &'a [NamedChars],
    pub mcp_instruction_items: &'a [NamedChars],
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

/// `system_prompt_layer_breakdown` 的返回值。
#[derive(Debug, Clone, Default)]
pub struct LayerBreakdown {
    pub system_chars: usize,
    pub developer_chars: usize,
    pub user_context_chars: usize,
    pub memory_chars: usize,
    pub skills_chars: usize,
    pub recall_chars: usize,
    pub mcp_instruction_chars: usize,
    pub system_items: Vec<NamedChars>,
    pub developer_items: Vec<NamedChars>,
    pub user_context_items: Vec<NamedChars>,
    pub memory_items: Vec<NamedChars>,
    pub skill_items: Vec<NamedChars>,
    pub mcp_instruction_items: Vec<NamedChars>,
}

pub fn is_subagent_tool_name(name: &str) -> bool {
    SUBAGENT_TOOLS.contains(&name)
}

fn items_from_named(named: &[NamedChars]) -> Vec<ContextUsageItem> {
    let mut items: Vec<ContextUsageItem> = named
        .iter()
        .filter_map(|(id, label, chars)| {
            let tokens = estimate_tokens(*chars);
            if tokens == 0 {
                return None;
            }
            Some(ContextUsageItem {
                id: id.clone(),
                label: label.clone(),
                tokens,
            })
        })
        .collect();
    items.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.label.cmp(&b.label)));
    items
}

fn push_seg(
    out: &mut Vec<ContextUsageSegment>,
    id: &str,
    chars: usize,
    count: Option<u32>,
    items: Vec<ContextUsageItem>,
) {
    let tokens = estimate_tokens(chars);
    if tokens == 0 && items.is_empty() {
        return;
    }
    out.push(ContextUsageSegment {
        id: id.to_string(),
        tokens,
        meta: count.map(|c| ContextUsageSegmentMeta { count: Some(c) }),
        items,
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
    let mut mcp_chars = input.mcp_instruction_chars;
    let mut agent_def_chars = 0usize;
    let mut tool_items: Vec<ContextUsageItem> = Vec::new();
    let mut mcp_items: Vec<ContextUsageItem> = items_from_named(input.mcp_instruction_items);
    let mut agent_def_items: Vec<ContextUsageItem> = Vec::new();
    for t in input.tools {
        let s = t.to_string();
        let n = tool_schema_name(t).unwrap_or("unknown");
        let tokens = estimate_tokens(s.len());
        if tokens == 0 {
            continue;
        }
        let item = ContextUsageItem {
            id: n.to_string(),
            label: n.to_string(),
            tokens,
        };
        if n.starts_with("mcp__") || matches!(n, mcp::MCP_RESOURCES_TOOL | mcp::MCP_PROMPTS_TOOL) {
            mcp_chars += s.len();
            mcp_items.push(item);
        } else if is_agent_def_tool_name(n) {
            agent_def_chars += s.len();
            agent_def_items.push(item);
        } else {
            tools_chars += s.len();
            tool_items.push(item);
        }
    }
    tool_items.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.label.cmp(&b.label)));
    mcp_items.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.label.cmp(&b.label)));
    agent_def_items.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.label.cmp(&b.label)));

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
    let mut role_chars: std::collections::HashMap<&'static str, (usize, u32)> =
        std::collections::HashMap::new();
    let mut subagent_by_name: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    let bump_role = |map: &mut std::collections::HashMap<&'static str, (usize, u32)>,
                     role: &'static str,
                     chars: usize| {
        let e = map.entry(role).or_insert((0, 0));
        e.0 += chars;
        e.1 += 1;
    };

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
                    *subagent_by_name.entry(name.to_string()).or_default() += text.len();
                } else {
                    conversation_chars += text.len();
                    msg_n += 1;
                    bump_role(&mut role_chars, "tool", text.len());
                }
            }
            Role::System => {
                conversation_chars += text.len();
                msg_n += 1;
                bump_role(&mut role_chars, "system", text.len());
            }
            Role::User => {
                conversation_chars += text.len();
                msg_n += 1;
                bump_role(&mut role_chars, "user", text.len());
            }
            Role::Assistant => {
                let mut chars = text.len();
                if let Some(calls) = &m.tool_calls {
                    for c in calls {
                        if !is_subagent_tool_name(&c.name) {
                            chars += c.name.len() + c.arguments.to_string().len();
                        }
                    }
                }
                conversation_chars += chars;
                msg_n += 1;
                bump_role(&mut role_chars, "assistant", chars);
            }
        }
    }

    let subagent_items: Vec<ContextUsageItem> = {
        let mut items: Vec<_> = subagent_by_name
            .into_iter()
            .filter_map(|(name, chars)| {
                let tokens = estimate_tokens(chars);
                (tokens > 0).then_some(ContextUsageItem {
                    id: name.clone(),
                    label: name,
                    tokens,
                })
            })
            .collect();
        items.sort_by_key(|b| std::cmp::Reverse(b.tokens));
        items
    };

    let conversation_items: Vec<ContextUsageItem> = {
        let mut items: Vec<_> = role_chars
            .into_iter()
            .filter_map(|(role, (chars, count))| {
                let tokens = estimate_tokens(chars);
                if tokens == 0 {
                    return None;
                }
                let (id, base) = match role {
                    "user" => ("user", "用户消息"),
                    "assistant" => ("assistant", "助手消息"),
                    "tool" => ("tool", "工具结果"),
                    "system" => ("system", "会话内 system"),
                    other => (other, other),
                };
                Some(ContextUsageItem {
                    id: id.to_string(),
                    label: format!("{base} ×{count}"),
                    tokens,
                })
            })
            .collect();
        items.sort_by_key(|b| std::cmp::Reverse(b.tokens));
        items
    };

    let system_items = items_from_named(input.system_items);
    let developer_items = items_from_named(input.developer_items);
    let user_context_items = items_from_named(input.user_context_items);
    let memory_items = items_from_named(input.memory_items);
    let skill_items = items_from_named(input.skill_items);

    let mut segments = Vec::new();
    push_seg(
        &mut segments,
        "system",
        input.system_chars,
        (!system_items.is_empty()).then_some(system_items.len() as u32),
        system_items,
    );
    push_seg(
        &mut segments,
        "developer",
        input.developer_chars,
        (!developer_items.is_empty()).then_some(developer_items.len() as u32),
        developer_items,
    );
    push_seg(
        &mut segments,
        "user_context",
        input.user_context_chars,
        (!user_context_items.is_empty()).then_some(user_context_items.len() as u32),
        user_context_items,
    );
    push_seg(
        &mut segments,
        "tools",
        tools_chars,
        (!tool_items.is_empty()).then_some(tool_items.len() as u32),
        tool_items,
    );
    push_seg(
        &mut segments,
        "agents",
        agent_def_chars,
        (!agent_def_items.is_empty()).then_some(agent_def_items.len() as u32),
        agent_def_items,
    );
    push_seg(
        &mut segments,
        "mcp",
        mcp_chars,
        (!mcp_items.is_empty()).then_some(mcp_items.len() as u32),
        mcp_items,
    );
    push_seg(
        &mut segments,
        "memory",
        input.memory_chars,
        (!memory_items.is_empty()).then_some(memory_items.len() as u32),
        memory_items,
    );
    push_seg(
        &mut segments,
        "skills",
        input.skills_chars,
        (!skill_items.is_empty()).then_some(skill_items.len() as u32),
        skill_items,
    );
    push_seg(&mut segments, "recall", input.recall_chars, None, vec![]);
    push_seg(
        &mut segments,
        "subagent",
        subagent_chars,
        (subagent_n > 0).then_some(subagent_n),
        subagent_items,
    );
    push_seg(
        &mut segments,
        "conversation",
        conversation_chars,
        (msg_n > 0).then_some(msg_n),
        conversation_items,
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

    #[test]
    fn subagent_context_usage_uses_only_six_v2_tool_names() {
        assert_eq!(
            SUBAGENT_TOOLS,
            [
                "spawn_agent",
                "list_agents",
                "send_message",
                "followup_task",
                "wait_agent",
                "interrupt_agent",
            ]
        );
        for legacy in [
            "read_agent",
            "close_agent",
            "send_message_to_agent",
            "wait_agents",
        ] {
            assert!(!AGENT_DEF_TOOLS.contains(&legacy));
        }
    }
    use types::message::{Message, ToolCall};

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
            {"type":"function","function":{"name":"exec_command","parameters":{}}},
            {"type":"function","function":{"name":"mcp__fs__read","parameters":{"a":1}}}
        ])
        .as_array()
        .unwrap()
        .clone();
        let snap = build_snapshot(ContextUsageInput {
            system_chars: 40,
            developer_chars: 0,
            user_context_chars: 0,
            memory_chars: 0,
            skills_chars: 0,
            recall_chars: 0,
            mcp_instruction_chars: 0,
            system_items: &[],
            developer_items: &[],
            user_context_items: &[],
            memory_items: &[],
            skill_items: &[],
            mcp_instruction_items: &[],
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
        assert_eq!(tools_seg.items.len(), 1);
        assert_eq!(tools_seg.items[0].id, "exec_command");
        assert_eq!(mcp_seg.items[0].id, "mcp__fs__read");
    }

    #[test]
    fn mcp_instructions_share_the_mcp_segment() {
        let instructions = vec![(
            "instructions:docs".into(),
            "Docs instructions".into(),
            40usize,
        )];
        let snap = build_snapshot(ContextUsageInput {
            system_chars: 0,
            developer_chars: 0,
            user_context_chars: 0,
            memory_chars: 0,
            skills_chars: 0,
            recall_chars: 0,
            mcp_instruction_chars: 40,
            system_items: &[],
            developer_items: &[],
            user_context_items: &[],
            memory_items: &[],
            skill_items: &[],
            mcp_instruction_items: &instructions,
            tools: &[],
            messages: &[],
            context_window: 128_000,
            updated_at_ms: 1,
            recommend_compact: false,
            recommend_compact_ratio: 0.85,
        });
        let mcp = snap.segment("mcp").unwrap();
        assert_eq!(mcp.tokens, 10);
        assert_eq!(mcp.items[0].id, "instructions:docs");
    }

    #[test]
    fn mcp_broker_schemas_share_the_mcp_segment() {
        let tools: Vec<serde_json::Value> = serde_json::json!([
            {"type":"function","function":{"name":"mcp_resources","parameters":{}}},
            {"type":"function","function":{"name":"mcp_prompts","parameters":{}}}
        ])
        .as_array()
        .unwrap()
        .clone();
        let snap = build_snapshot(ContextUsageInput {
            system_chars: 0,
            developer_chars: 0,
            user_context_chars: 0,
            memory_chars: 0,
            skills_chars: 0,
            recall_chars: 0,
            mcp_instruction_chars: 0,
            system_items: &[],
            developer_items: &[],
            user_context_items: &[],
            memory_items: &[],
            skill_items: &[],
            mcp_instruction_items: &[],
            tools: &tools,
            messages: &[],
            context_window: 128_000,
            updated_at_ms: 1,
            recommend_compact: false,
            recommend_compact_ratio: 0.85,
        });
        let mcp = snap.segment("mcp").unwrap();
        assert_eq!(mcp.meta.as_ref().and_then(|meta| meta.count), Some(2));
        assert!(mcp.items.iter().any(|item| item.id == "mcp_resources"));
        assert!(mcp.items.iter().any(|item| item.id == "mcp_prompts"));
    }

    #[test]
    fn agent_def_tools_go_to_agents_segment() {
        let tools: Vec<serde_json::Value> = serde_json::json!([
            {"type":"function","function":{"name":"exec_command","parameters":{}}},
            {"type":"function","function":{"name":"spawn_agent","parameters":{"task_name":"x","message":"work"}}},
            {"type":"function","function":{"name":"wait_agent","parameters":{}}}
        ])
        .as_array()
        .unwrap()
        .clone();
        let snap = build_snapshot(ContextUsageInput {
            system_chars: 0,
            developer_chars: 0,
            user_context_chars: 0,
            memory_chars: 0,
            skills_chars: 0,
            recall_chars: 0,
            mcp_instruction_chars: 0,
            system_items: &[],
            developer_items: &[],
            user_context_items: &[],
            memory_items: &[],
            skill_items: &[],
            mcp_instruction_items: &[],
            tools: &tools,
            messages: &[],
            context_window: 128_000,
            updated_at_ms: 1,
            recommend_compact: false,
            recommend_compact_ratio: 0.85,
        });
        assert_eq!(snap.segment("tools").unwrap().items.len(), 1);
        let agents = snap.segment("agents").unwrap();
        assert_eq!(agents.items.len(), 2);
        assert!(agents.items.iter().any(|i| i.id == "spawn_agent"));
        assert!(agents.items.iter().any(|i| i.id == "wait_agent"));
    }

    #[test]
    fn skill_and_memory_items() {
        let memory = vec![
            ("memory".into(), "MEMORY.md".into(), 40usize),
            ("user".into(), "USER.md".into(), 8usize),
        ];
        let skills = vec![
            ("demo".into(), "demo".into(), 20usize),
            ("big".into(), "big".into(), 80usize),
        ];
        let snap = build_snapshot(ContextUsageInput {
            system_chars: 0,
            developer_chars: 0,
            user_context_chars: 0,
            memory_chars: 48,
            skills_chars: 100,
            recall_chars: 0,
            mcp_instruction_chars: 0,
            system_items: &[],
            developer_items: &[],
            user_context_items: &[],
            memory_items: &memory,
            skill_items: &skills,
            mcp_instruction_items: &[],
            tools: &[],
            messages: &[],
            context_window: 128_000,
            updated_at_ms: 1,
            recommend_compact: false,
            recommend_compact_ratio: 0.85,
        });
        let mem = snap.segment("memory").unwrap();
        assert_eq!(mem.items.len(), 2);
        assert_eq!(mem.items[0].id, "memory");
        let sk = snap.segment("skills").unwrap();
        assert_eq!(sk.items[0].id, "big");
        assert_eq!(sk.items.len(), 2);
    }

    #[test]
    fn role_bearing_prompt_context_has_distinct_segments() {
        let developer = vec![("mode".into(), "交互模式引导".into(), 20usize)];
        let user_context = vec![("agents".into(), "AGENTS.md".into(), 40usize)];
        let snap = build_snapshot(ContextUsageInput {
            system_chars: 80,
            developer_chars: 20,
            user_context_chars: 40,
            memory_chars: 0,
            skills_chars: 0,
            recall_chars: 0,
            mcp_instruction_chars: 0,
            system_items: &[],
            developer_items: &developer,
            user_context_items: &user_context,
            memory_items: &[],
            skill_items: &[],
            mcp_instruction_items: &[],
            tools: &[],
            messages: &[],
            context_window: 128_000,
            updated_at_ms: 1,
            recommend_compact: false,
            recommend_compact_ratio: 0.85,
        });

        assert_eq!(snap.segment("system").unwrap().tokens, 20);
        assert_eq!(snap.segment("developer").unwrap().tokens, 5);
        assert_eq!(snap.segment("user_context").unwrap().tokens, 10);
        assert_eq!(snap.total_tokens, 35);
    }

    #[test]
    fn agent_thread_tool_result_counts_as_subagent() {
        let assistant = Message::assistant_with_tools(
            "",
            vec![ToolCall {
                id: "c1".into(),
                name: "spawn_agent".into(),
                arguments: serde_json::json!({}),
                signature: None,
            }],
        );
        let tool = Message::tool_with_id("c1", &"x".repeat(40));
        let snap = build_snapshot(ContextUsageInput {
            system_chars: 0,
            developer_chars: 0,
            user_context_chars: 0,
            memory_chars: 0,
            skills_chars: 0,
            recall_chars: 0,
            mcp_instruction_chars: 0,
            system_items: &[],
            developer_items: &[],
            user_context_items: &[],
            memory_items: &[],
            skill_items: &[],
            mcp_instruction_items: &[],
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
        assert_eq!(snap.segment("subagent").unwrap().items[0].id, "spawn_agent");
    }
}
