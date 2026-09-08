//! Prompt 契约。
//!
//! 模型请求包含三个独立层：
//! - 稳定的基础指令；
//! - 带角色的动态上下文；
//! - 原生工具 schema（由请求管线持有，不在此处渲染）。

use agent_protocol::ResponseItem;
use serde::{Deserialize, Serialize};

use crate::prompt::context::{DynamicContext, StaticContext};
use crate::prompt::context_source::{ContextBudget, ContextSource, RenderedSource};
use crate::prompt::prompt_builder::PromptBuilder;

const LAYER_SEP: &str = "\n\n---\n\n";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromptSourceUsage {
    pub id: String,
    pub chars: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptContractUsage {
    pub base: Vec<PromptSourceUsage>,
    pub developer: Vec<PromptSourceUsage>,
    pub user: Vec<PromptSourceUsage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptContextRole {
    Developer,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptContextSection {
    pub id: String,
    pub role: PromptContextRole,
    pub content: String,
}

/// 面向 Provider 的 prompt 数据，不含原生工具 schema。
#[derive(Debug, Clone, Default)]
pub struct PromptContract {
    /// 通过 Provider 专用的 instructions/system 字段发送的稳定指令。
    pub base_instructions: String,
    /// 以显式 developer/user 消息保持的动态上下文。
    pub context: Vec<ResponseItem>,
    /// 稳定的 source 标识，用于渲染模型可见的 world-state diff。
    pub context_sections: Vec<PromptContextSection>,
    /// 共享预算分配后各 source 的精确字符用量。
    pub usage: PromptContractUsage,
}

impl PromptContract {
    pub fn from_base_instructions(base_instructions: impl Into<String>) -> Self {
        let base_instructions = base_instructions.into();
        let mut usage = PromptContractUsage::default();
        if !base_instructions.trim().is_empty() {
            usage.base.push(PromptSourceUsage {
                id: "base".into(),
                chars: base_instructions.chars().count(),
            });
        }
        Self {
            base_instructions,
            context: Vec::new(),
            context_sections: Vec::new(),
            usage,
        }
    }

    /// 兼容性渲染，供诊断和仍暴露扁平 prompt 的调用方使用。
    /// 采样路径必须使用 [`Self::context`] 以保留角色边界。
    pub fn flattened(&self) -> String {
        let mut layers = Vec::new();
        if !self.base_instructions.trim().is_empty() {
            layers.push(self.base_instructions.trim().to_string());
        }
        for message in &self.context {
            let (role, content) = match message {
                ResponseItem::Message { role, .. } if role == "developer" || role == "user" => {
                    (role.as_str(), message.text())
                }
                _ => continue,
            };
            if !content.trim().is_empty() {
                layers.push(format!("[{role}]\n{}", content.trim()));
            }
        }
        layers.join(LAYER_SEP)
    }
}

/// 分配到显式 Provider 角色的动态运行时片段。
pub struct RuntimePromptLayers<'a> {
    /// 稳定的工具使用行为说明，随基础指令一起发送。
    pub base_guidance: &'a str,
    /// 每轮变化的 developer 策略，如当前交互模式。
    pub developer_guidance: &'a str,
    /// 上下文化的当前时间；遵循 contextual-user-message 模型。
    pub timestamp: &'a str,
    /// 服务端提供的 MCP 使用说明。
    pub mcp_instructions: &'a str,
    /// Thread-local working state, never a source of authority.
    pub thread_checkpoint: &'a str,
}

#[derive(Default)]
struct RoleBuffer {
    sources: Vec<AllocatedSource>,
}

struct AllocatedSource {
    id: String,
    content: String,
}

struct RenderedRole {
    body: String,
    usage: Vec<PromptSourceUsage>,
    sources: Vec<AllocatedSource>,
}

impl RoleBuffer {
    fn allocate(&mut self, budget: &mut ContextBudget, source: &dyn ContextSource) {
        let separator_chars = LAYER_SEP.chars().count();
        if !self.sources.is_empty() && budget.remaining() < separator_chars + 1 {
            return;
        }

        let has_previous = !self.sources.is_empty();
        if has_previous {
            let _ = budget.take_chars(LAYER_SEP);
        }
        let chunk = source.contribute(budget);
        if chunk.is_empty() {
            if has_previous {
                budget.refund(separator_chars);
            }
            return;
        }

        self.sources.push(AllocatedSource {
            id: source.id().to_string(),
            content: chunk,
        });
    }

    /// 在分配之后渲染，使预算优先级和指令顺序保持独立。
    fn render(mut self, source_order: &[&str]) -> RenderedRole {
        self.sources.sort_by_key(|source| {
            source_order
                .iter()
                .position(|id| *id == source.id.as_str())
                .unwrap_or(source_order.len())
        });

        let separator_chars = LAYER_SEP.chars().count();
        let mut body = String::new();
        let mut usage = Vec::with_capacity(self.sources.len());
        let mut sources = Vec::with_capacity(self.sources.len());
        for source in self.sources {
            let has_previous = !body.is_empty();
            if has_previous {
                body.push_str(LAYER_SEP);
            }
            body.push_str(&source.content);
            usage.push(PromptSourceUsage {
                id: source.id.clone(),
                chars: source.content.chars().count() + usize::from(has_previous) * separator_chars,
            });
            sources.push(source);
        }
        RenderedRole {
            body,
            usage,
            sources,
        }
    }
}

fn titled(title: &str, body: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        String::new()
    } else {
        format!("{title}\n{body}")
    }
}

/// 组装规范的三层 prompt 契约，并附精确的预算后诊断信息。
///
/// 分配优先级与 Provider 消息顺序独立。这使得高优先级的项目和 hook 上下文
/// 可在可选的 Skills/MCP 指引之前预留预算，而最终请求仍按一条 developer 消息
/// 在一条上下文 user 消息之前的顺序发出。
pub(crate) fn assemble_prompt_contract_with_usage(
    budget: &mut ContextBudget,
    static_ctx: &StaticContext,
    inject: Option<&str>,
    skill_index: &[(&str, &str)],
    dynamic_ctx: &DynamicContext,
    runtime: RuntimePromptLayers<'_>,
) -> PromptContract {
    let soul = RenderedSource::new("soul", titled("# 身份 / SOUL", &static_ctx.soul));
    let identity = RenderedSource::new("identity", titled("# IDENTITY", &static_ctx.identity));
    let tool_guidance = RenderedSource::new("tool_guidance", runtime.base_guidance);
    let mode = RenderedSource::new("mode", runtime.developer_guidance);
    let agents = RenderedSource::new("agents", titled("# AGENTS.md", &static_ctx.agent_md));
    let tools = RenderedSource::new("tools", titled("# TOOLS.md", &static_ctx.tools_md));
    let hook = RenderedSource::new(
        "hook",
        inject
            .map(|text| titled("# Hook context", text))
            .unwrap_or_default(),
    );
    let user_profile = RenderedSource::new(
        "user_profile",
        titled("# 用户画像", &static_ctx.user_profile),
    );
    let memory = RenderedSource::new(
        "memory",
        titled("# 长期记忆（MEMORY.md）", &static_ctx.memory),
    );
    let daily = RenderedSource::new("daily", titled("# 今日记忆（流水截断）", &static_ctx.daily));
    let skills = RenderedSource::new(
        "skills",
        PromptBuilder::new().with_skills_index(skill_index).build(),
    );
    let mcp = RenderedSource::new("mcp", runtime.mcp_instructions);
    let timestamp = RenderedSource::new("timestamp", runtime.timestamp);
    let dynamic = RenderedSource::new("dynamic", dynamic_ctx.render());
    let checkpoint = RenderedSource::new("thread_checkpoint", runtime.thread_checkpoint);

    let mut base = RoleBuffer::default();
    let mut developer = RoleBuffer::default();
    let mut user = RoleBuffer::default();

    // 信任/优先级顺序。输出的角色顺序在下方单独组装。
    base.allocate(budget, &soul);
    base.allocate(budget, &identity);
    base.allocate(budget, &tool_guidance);
    developer.allocate(budget, &mode);
    user.allocate(budget, &agents);
    user.allocate(budget, &checkpoint);
    user.allocate(budget, &hook);
    user.allocate(budget, &tools);
    user.allocate(budget, &user_profile);
    user.allocate(budget, &memory);
    user.allocate(budget, &daily);
    developer.allocate(budget, &skills);
    developer.allocate(budget, &mcp);
    user.allocate(budget, &timestamp);
    user.allocate(budget, &dynamic);

    let base = base.render(&["soul", "identity", "tool_guidance"]);
    // 在活跃协作模式之前渲染能力说明，以便模式可以覆盖通用使用指引
    // 而不丢失其先前的预算预留。
    let developer = developer.render(&["skills", "mcp", "mode"]);
    let user = user.render(&[
        "agents",
        "thread_checkpoint",
        "tools",
        "hook",
        "user_profile",
        "memory",
        "daily",
        "timestamp",
        "dynamic",
    ]);

    let mut context = Vec::with_capacity(2);
    if !developer.body.is_empty() {
        context.push(ResponseItem::developer_text(developer.body.clone()));
    }
    if !user.body.is_empty() {
        context.push(ResponseItem::user_text(user.body.clone()));
    }
    let context_sections = developer
        .sources
        .iter()
        .map(|source| PromptContextSection {
            id: source.id.clone(),
            role: PromptContextRole::Developer,
            content: source.content.clone(),
        })
        .chain(user.sources.iter().map(|source| PromptContextSection {
            id: source.id.clone(),
            role: PromptContextRole::User,
            content: source.content.clone(),
        }))
        .collect();

    PromptContract {
        base_instructions: base.body,
        context,
        context_sections,
        usage: PromptContractUsage {
            base: base.usage,
            developer: developer.usage,
            user: user.usage,
        },
    }
}

/// 组装规范的三层 prompt 契约。
///
/// 原生工具 schema 被有意省略：调用方将其独立传递给补全请求。
pub fn assemble_prompt_contract(
    budget: &mut ContextBudget,
    static_ctx: &StaticContext,
    inject: Option<&str>,
    skill_index: &[(&str, &str)],
    dynamic_ctx: &DynamicContext,
    runtime: RuntimePromptLayers<'_>,
) -> PromptContract {
    assemble_prompt_contract_with_usage(
        budget,
        static_ctx,
        inject,
        skill_index,
        dynamic_ctx,
        runtime,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::prompt_builder::TOOL_GUIDANCE;
    fn runtime<'a>(mode: &'a str, mcp: &'a str) -> RuntimePromptLayers<'a> {
        RuntimePromptLayers {
            base_guidance: TOOL_GUIDANCE,
            developer_guidance: mode,
            timestamp: "CURRENT_TIME",
            mcp_instructions: mcp,
            thread_checkpoint: "THREAD_CHECKPOINT",
        }
    }

    #[test]
    fn preserves_three_layer_boundaries() {
        let static_ctx = StaticContext {
            soul: "STABLE_SOUL".into(),
            identity: "STABLE_IDENTITY".into(),
            agent_md: "PROJECT_RULES".into(),
            tools_md: "LOCAL_TOOL_RULES".into(),
            memory: "PROJECT_MEMORY".into(),
            user_profile: "USER_PROFILE".into(),
            daily: "DAILY_CONTEXT".into(),
        };
        let dynamic = DynamicContext::from_recalled(1, "RECALLED_CONTEXT");
        let mut budget = ContextBudget::new(20_000);
        let contract = assemble_prompt_contract(
            &mut budget,
            &static_ctx,
            Some("HOOK_CONTEXT"),
            &[("example-skill", "Example skill")],
            &dynamic,
            runtime("DEVELOPER_POLICY", "MCP_INSTRUCTIONS"),
        );

        assert!(contract.base_instructions.contains("STABLE_SOUL"));
        assert!(contract.base_instructions.contains("STABLE_IDENTITY"));
        assert!(contract.base_instructions.contains(TOOL_GUIDANCE));
        for dynamic_text in [
            "PROJECT_RULES",
            "LOCAL_TOOL_RULES",
            "PROJECT_MEMORY",
            "USER_PROFILE",
            "DAILY_CONTEXT",
            "HOOK_CONTEXT",
            "RECALLED_CONTEXT",
            "DEVELOPER_POLICY",
            "example-skill",
            "MCP_INSTRUCTIONS",
            "THREAD_CHECKPOINT",
        ] {
            assert!(!contract.base_instructions.contains(dynamic_text));
        }
        assert_eq!(contract.context.len(), 2);
        assert_eq!(contract.context[0].role(), Some("developer"));
        assert!(contract.context[0]
            .text_content()
            .contains("DEVELOPER_POLICY"));
        assert!(contract.context[0].text_content().contains("example-skill"));
        assert!(contract.context[0]
            .text_content()
            .contains("MCP_INSTRUCTIONS"));
        assert!(!contract.context[0].text_content().contains(TOOL_GUIDANCE));
        let developer = contract.context[0].text_content();
        assert!(
            developer.find("example-skill").unwrap() < developer.find("MCP_INSTRUCTIONS").unwrap()
        );
        assert!(
            developer.find("MCP_INSTRUCTIONS").unwrap()
                < developer.find("DEVELOPER_POLICY").unwrap()
        );
        assert_eq!(contract.context[1].role(), Some("user"));
        assert!(contract.context[1].text_content().contains("PROJECT_RULES"));
        assert!(contract.context[1]
            .text_content()
            .contains("PROJECT_MEMORY"));
        assert!(contract.context[1].text_content().contains("HOOK_CONTEXT"));
        assert!(contract.context[1].text_content().contains("CURRENT_TIME"));
        assert!(contract.context[1]
            .text_content()
            .contains("RECALLED_CONTEXT"));
        assert_eq!(
            contract
                .context_sections
                .iter()
                .map(|section| (section.role, section.id.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (PromptContextRole::Developer, "skills"),
                (PromptContextRole::Developer, "mcp"),
                (PromptContextRole::Developer, "mode"),
                (PromptContextRole::User, "agents"),
                (PromptContextRole::User, "thread_checkpoint"),
                (PromptContextRole::User, "tools"),
                (PromptContextRole::User, "hook"),
                (PromptContextRole::User, "user_profile"),
                (PromptContextRole::User, "memory"),
                (PromptContextRole::User, "daily"),
                (PromptContextRole::User, "timestamp"),
                (PromptContextRole::User, "dynamic"),
            ]
        );
    }

    #[test]
    fn optional_capabilities_cannot_displace_project_or_hook_context() {
        let static_ctx = StaticContext {
            agent_md: "PROJECT_RULES".into(),
            tools_md: "LOCAL_TOOL_RULES".into(),
            ..Default::default()
        };
        let dynamic = DynamicContext::from_recalled(1, "RECALL_SHOULD_BE_DROPPED");
        let mode = "MODE_POLICY";
        let hook = "HOOK_CONTEXT";
        let required_chars = TOOL_GUIDANCE.chars().count()
            + "THREAD_CHECKPOINT".chars().count()
            + LAYER_SEP.chars().count()
            + mode.chars().count()
            + titled("# AGENTS.md", &static_ctx.agent_md).chars().count()
            + titled("# Hook context", hook).chars().count()
            + titled("# TOOLS.md", &static_ctx.tools_md).chars().count()
            + LAYER_SEP.chars().count() * 2;
        let huge_skill = "S".repeat(10_000);
        let huge_mcp = "M".repeat(10_000);
        let mut budget = ContextBudget::new(required_chars + 8);
        let contract = assemble_prompt_contract_with_usage(
            &mut budget,
            &static_ctx,
            Some(hook),
            &[("huge-skill", huge_skill.as_str())],
            &dynamic,
            runtime(mode, &huge_mcp),
        );

        let developer = contract.context[0].text_content();
        let user = contract.context[1].text_content();
        assert!(developer.contains(mode));
        assert!(user.contains("PROJECT_RULES"));
        assert!(user.contains(hook));
        assert!(user.contains("LOCAL_TOOL_RULES"));
        assert!(!developer.contains("huge-skill"));
        assert!(!user.contains("RECALL_SHOULD_BE_DROPPED"));
    }

    #[test]
    fn usage_matches_budgeted_contract_bodies() {
        let static_ctx = StaticContext {
            soul: "SOUL".into(),
            identity: "IDENTITY".into(),
            agent_md: "AGENTS".into(),
            tools_md: "TOOLS".into(),
            memory: "MEMORY".into(),
            user_profile: "USER".into(),
            daily: "DAILY".into(),
        };
        let mut budget = ContextBudget::new(20_000);
        let contract = assemble_prompt_contract_with_usage(
            &mut budget,
            &static_ctx,
            Some("HOOK"),
            &[("skill", "description")],
            &DynamicContext::from_recalled(1, "RECALL"),
            runtime("MODE", "MCP"),
        );

        let base_chars: usize = contract.usage.base.iter().map(|item| item.chars).sum();
        let developer_chars: usize = contract.usage.developer.iter().map(|item| item.chars).sum();
        let user_chars: usize = contract.usage.user.iter().map(|item| item.chars).sum();
        assert_eq!(base_chars, contract.base_instructions.chars().count());
        assert_eq!(
            developer_chars,
            contract.context[0].text_content().chars().count()
        );
        assert_eq!(
            user_chars,
            contract.context[1].text_content().chars().count()
        );
    }
}
