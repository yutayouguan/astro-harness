//! Codex-style prompt contract.
//!
//! A model request has three independent layers:
//! - stable base instructions;
//! - role-bearing dynamic context;
//! - native tool schemas (owned by the request pipeline, never rendered here).

use providers::types::message::Message as ProviderMessage;

use crate::prompt::context::{DynamicContext, StaticContext};
use crate::prompt::context_source::{ContextBudget, ContextSource, RenderedSource};
use crate::prompt::prompt_builder::PromptBuilder;

const LAYER_SEP: &str = "\n\n---\n\n";

#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Provider-facing prompt data excluding native tool schemas.
#[derive(Debug, Clone, Default)]
pub struct PromptContract {
    /// Stable instructions sent through the provider's dedicated instructions/system field.
    pub base_instructions: String,
    /// Dynamic context kept as explicit developer/user messages.
    pub context: Vec<ProviderMessage>,
    /// Exact per-source character usage after the shared budget is applied.
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
            usage,
        }
    }

    /// Compatibility rendering for diagnostics and callers that still expose a flat prompt.
    /// The sampling path must use [`Self::context`] so role boundaries are preserved.
    pub fn flattened(&self) -> String {
        let mut layers = Vec::new();
        if !self.base_instructions.trim().is_empty() {
            layers.push(self.base_instructions.trim().to_string());
        }
        for message in &self.context {
            let (role, content) = match message {
                ProviderMessage::Developer { content } => ("developer", content.as_str()),
                ProviderMessage::User { .. } => ("user", message.text_content()),
                _ => continue,
            };
            if !content.trim().is_empty() {
                layers.push(format!("[{role}]\n{}", content.trim()));
            }
        }
        layers.join(LAYER_SEP)
    }
}

/// Dynamic runtime fragments assigned to explicit provider roles.
pub struct RuntimePromptLayers<'a> {
    /// Stable tool-use behavior, sent with the base instructions.
    pub base_guidance: &'a str,
    /// Turn-varying developer policy such as the current interaction mode.
    pub developer_guidance: &'a str,
    /// Contextual current time; follows Codex's contextual-user-message model.
    pub timestamp: &'a str,
    /// Server-provided MCP usage instructions.
    pub mcp_instructions: &'a str,
}

#[derive(Default)]
struct RoleBuffer {
    body: String,
    usage: Vec<PromptSourceUsage>,
}

impl RoleBuffer {
    fn push(&mut self, budget: &mut ContextBudget, source: &dyn ContextSource) {
        let separator_chars = LAYER_SEP.chars().count();
        if !self.body.is_empty() && budget.remaining() < separator_chars + 1 {
            return;
        }

        let has_previous = !self.body.is_empty();
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

        if has_previous {
            self.body.push_str(LAYER_SEP);
        }
        self.body.push_str(&chunk);
        self.usage.push(PromptSourceUsage {
            id: source.id().to_string(),
            chars: chunk.chars().count() + usize::from(has_previous) * separator_chars,
        });
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

/// Assemble the canonical three-layer prompt contract with exact post-budget diagnostics.
///
/// Allocation priority is independent from provider message order. This lets high-priority
/// project and hook context reserve budget before optional Skills/MCP guidance while the final
/// request still emits one developer message before one contextual user message.
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

    let mut base = RoleBuffer::default();
    let mut developer = RoleBuffer::default();
    let mut user = RoleBuffer::default();

    // Trust/priority order. Output role order is assembled separately below.
    base.push(budget, &soul);
    base.push(budget, &identity);
    base.push(budget, &tool_guidance);
    developer.push(budget, &mode);
    user.push(budget, &agents);
    user.push(budget, &hook);
    user.push(budget, &user_profile);
    user.push(budget, &memory);
    user.push(budget, &daily);
    developer.push(budget, &skills);
    developer.push(budget, &mcp);
    user.push(budget, &timestamp);
    user.push(budget, &dynamic);

    let mut context = Vec::with_capacity(2);
    if !developer.body.is_empty() {
        context.push(ProviderMessage::developer(developer.body.clone()));
    }
    if !user.body.is_empty() {
        context.push(ProviderMessage::user_text(user.body.clone()));
    }

    PromptContract {
        base_instructions: base.body,
        context,
        usage: PromptContractUsage {
            base: base.usage,
            developer: developer.usage,
            user: user.usage,
        },
    }
}

/// Assemble the canonical three-layer prompt contract.
///
/// Native tool schemas are deliberately absent: callers pass them independently to the
/// completion request.
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
    use providers::types::message::Role;

    fn runtime<'a>(mode: &'a str, mcp: &'a str) -> RuntimePromptLayers<'a> {
        RuntimePromptLayers {
            base_guidance: TOOL_GUIDANCE,
            developer_guidance: mode,
            timestamp: "CURRENT_TIME",
            mcp_instructions: mcp,
        }
    }

    #[test]
    fn preserves_three_layer_boundaries() {
        let static_ctx = StaticContext {
            soul: "STABLE_SOUL".into(),
            identity: "STABLE_IDENTITY".into(),
            agent_md: "PROJECT_RULES".into(),
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
            "PROJECT_MEMORY",
            "USER_PROFILE",
            "DAILY_CONTEXT",
            "HOOK_CONTEXT",
            "RECALLED_CONTEXT",
            "DEVELOPER_POLICY",
            "example-skill",
            "MCP_INSTRUCTIONS",
        ] {
            assert!(!contract.base_instructions.contains(dynamic_text));
        }
        assert_eq!(contract.context.len(), 2);
        assert_eq!(contract.context[0].role(), Role::Developer);
        assert!(contract.context[0]
            .text_content()
            .contains("DEVELOPER_POLICY"));
        assert!(contract.context[0].text_content().contains("example-skill"));
        assert!(contract.context[0]
            .text_content()
            .contains("MCP_INSTRUCTIONS"));
        assert!(!contract.context[0].text_content().contains(TOOL_GUIDANCE));
        assert_eq!(contract.context[1].role(), Role::User);
        assert!(contract.context[1].text_content().contains("PROJECT_RULES"));
        assert!(contract.context[1]
            .text_content()
            .contains("PROJECT_MEMORY"));
        assert!(contract.context[1].text_content().contains("HOOK_CONTEXT"));
        assert!(contract.context[1].text_content().contains("CURRENT_TIME"));
        assert!(contract.context[1]
            .text_content()
            .contains("RECALLED_CONTEXT"));
    }

    #[test]
    fn optional_capabilities_cannot_displace_project_or_hook_context() {
        let static_ctx = StaticContext {
            agent_md: "PROJECT_RULES".into(),
            ..Default::default()
        };
        let dynamic = DynamicContext::from_recalled(1, "RECALL_SHOULD_BE_DROPPED");
        let mode = "MODE_POLICY";
        let hook = "HOOK_CONTEXT";
        let required_chars = TOOL_GUIDANCE.chars().count()
            + mode.chars().count()
            + titled("# AGENTS.md", &static_ctx.agent_md).chars().count()
            + titled("# Hook context", hook).chars().count()
            + LAYER_SEP.chars().count();
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
        assert!(!developer.contains("huge-skill"));
        assert!(!user.contains("RECALL_SHOULD_BE_DROPPED"));
    }

    #[test]
    fn usage_matches_budgeted_contract_bodies() {
        let static_ctx = StaticContext {
            soul: "SOUL".into(),
            identity: "IDENTITY".into(),
            agent_md: "AGENTS".into(),
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
