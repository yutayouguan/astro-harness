//! Codex-style prompt contract.
//!
//! A model request has three independent layers:
//! - stable base instructions;
//! - role-bearing dynamic context;
//! - native tool schemas (owned by the request pipeline, never rendered here).

use providers::types::message::Message as ProviderMessage;

use crate::prompt::context::{DynamicContext, StaticContext};
use crate::prompt::context_source::{assemble_from_sources, ContextBudget, RenderedSource};
use crate::prompt::prompt_builder::PromptBuilder;

/// Provider-facing prompt data excluding native tool schemas.
#[derive(Debug, Clone, Default)]
pub struct PromptContract {
    /// Stable instructions sent through the provider's dedicated instructions/system field.
    pub base_instructions: String,
    /// Dynamic context kept as explicit developer/user messages.
    pub context: Vec<ProviderMessage>,
}

impl PromptContract {
    pub fn from_base_instructions(base_instructions: impl Into<String>) -> Self {
        Self {
            base_instructions: base_instructions.into(),
            context: Vec::new(),
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
        layers.join("\n\n---\n\n")
    }
}

/// Dynamic runtime fragments assigned to explicit provider roles.
pub struct RuntimePromptLayers<'a> {
    /// Developer policy: interaction mode and tool-use behavior.
    pub guidance: &'a str,
    /// Contextual current time; follows Codex's contextual-user-message model.
    pub timestamp: &'a str,
    /// Server-provided MCP usage instructions.
    pub mcp_instructions: &'a str,
}

/// Assemble the canonical three-layer prompt contract.
///
/// Native tool schemas are deliberately absent: callers pass them independently to the
/// completion request. The shared budget is consumed in trust/priority order:
/// base instructions, developer policy/capabilities, then contextual user data.
pub fn assemble_prompt_contract(
    budget: &mut ContextBudget,
    static_ctx: &StaticContext,
    inject: Option<&str>,
    skill_index: &[(&str, &str)],
    dynamic_ctx: &DynamicContext,
    runtime: RuntimePromptLayers<'_>,
) -> PromptContract {
    let base = StaticContext {
        soul: static_ctx.soul.clone(),
        identity: static_ctx.identity.clone(),
        ..Default::default()
    };
    let base_instructions = assemble_from_sources(budget, &[&base]);

    let skills = PromptBuilder::new().with_skills_index(skill_index).build();
    let guidance = RenderedSource::new("guidance", runtime.guidance);
    let skills = RenderedSource::new("skills", skills);
    let mcp = RenderedSource::new("mcp_instructions", runtime.mcp_instructions);
    let developer = assemble_from_sources(budget, &[&guidance, &skills, &mcp]);

    let project_instructions = (!static_ctx.agent_md.trim().is_empty())
        .then(|| format!("# AGENTS.md\n{}", static_ctx.agent_md.trim()))
        .unwrap_or_default();
    let memory = StaticContext {
        memory: static_ctx.memory.clone(),
        user_profile: static_ctx.user_profile.clone(),
        daily: static_ctx.daily.clone(),
        ..Default::default()
    }
    .render();
    let injected = inject
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| format!("# Hook context\n{text}"))
        .unwrap_or_default();
    let project_instructions = RenderedSource::new("agents", project_instructions);
    let injected = RenderedSource::new("inject", injected);
    let memory = RenderedSource::new("memory", memory);
    let timestamp = RenderedSource::new("timestamp", runtime.timestamp);
    let user = assemble_from_sources(
        budget,
        &[
            &project_instructions,
            &injected,
            &memory,
            &timestamp,
            dynamic_ctx,
        ],
    );

    let mut context = Vec::with_capacity(2);
    if !developer.is_empty() {
        context.push(ProviderMessage::developer(developer));
    }
    if !user.is_empty() {
        context.push(ProviderMessage::user_text(user));
    }

    PromptContract {
        base_instructions,
        context,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use providers::types::message::Role;

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
            RuntimePromptLayers {
                guidance: "DEVELOPER_POLICY",
                timestamp: "CURRENT_TIME",
                mcp_instructions: "MCP_INSTRUCTIONS",
            },
        );

        assert!(contract.base_instructions.contains("STABLE_SOUL"));
        assert!(contract.base_instructions.contains("STABLE_IDENTITY"));
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
}
