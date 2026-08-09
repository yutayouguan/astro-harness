//! AgentLoop system prompt 构建：静态/动态上下文组装与分层占用估算。

use crate::prompt::context::{DynamicContext, StaticContext};
use crate::prompt::prompt_builder::PromptBuilder;

use super::AgentLoop;

impl AgentLoop {
    /// 与 `build_system_prompt` 同源加载静态/动态上下文与技能列表（不含 env 副作用）。
    fn system_prompt_parts(&self) -> (StaticContext, DynamicContext, Vec<(String, String)>) {
        let (project_memory, user_profile, daily) = self.memory.prompt_snapshot_with_daily();
        let skill_pairs = if self.tool_registry.is_toolset_enabled("skills") {
            skills::list_enabled_for_prompt()
        } else {
            Vec::new()
        };

        let mut static_ctx = if let Some(ref over) = self.config.static_override {
            over.clone()
        } else {
            StaticContext::from_workspace_files(
                &self.config.soul,
                &project_memory,
                &user_profile,
                &daily,
            )
        };
        if static_ctx.agent_md.is_empty() {
            let ws = self.resolve_workspace_dir();
            if let Ok(content) = std::fs::read_to_string(ws.join("AGENTS.md")) {
                static_ctx.agent_md = content;
            }
        }
        let dynamic_ctx = {
            let mut dyn_ctx = DynamicContext::from_recalled(
                self.config.dynamic_max_items,
                &self.compression.last_recalled_context,
            );
            let pinned = tools::render_pinned_for_prompt(&self.memory.workspace_dir);
            if !pinned.trim().is_empty() {
                // 固定上下文优先于本轮 FTS 召回
                dyn_ctx.items.insert(0, pinned);
            }
            if let Some(ref nudge) = self.pending_learning_nudge {
                dyn_ctx.items.insert(0, format!("# 学习提示\n{nudge}"));
            }
            dyn_ctx
        };
        (static_ctx, dynamic_ctx, skill_pairs)
    }

    /// 组装完整 system prompt：静态上下文 + 动态召回 + 技能索引 + 工具指引 + 时间戳。
    ///
    /// MEMORY / USER 仅注入 **snapshot**（同会话冻结）；日记读盘后截断注入。
    /// 各层经 [`crate::prompt::ContextSource`] 共享字符预算（优先 static）。
    ///
    /// **不**把 `pending_inject_context` 编入 system：hooks / KeepGoing 注入仍走
    /// [`Self::take_inject_context`] → 消息侧 `[astro:hook-context]`（见 `multi_turn`），
    /// 避免与 system 层双重注入。
    ///
    /// 副作用：设置 workspace 目录覆盖供 skills 发现使用。
    pub fn build_system_prompt(&self) -> String {
        let (static_ctx, dynamic_ctx, skill_pairs) = self.system_prompt_parts();
        skills::set_workspace_override(&self.memory.workspace_dir);
        let skill_index: Vec<(&str, &str)> = skill_pairs
            .iter()
            .map(|(name, desc)| (name.as_str(), desc.as_str()))
            .collect();

        let (guidance, timestamp) = self.system_prompt_guidance_timestamp();
        let mut budget = crate::prompt::ContextBudget::new(self.config.context_budget_chars.max(1));
        crate::prompt::assemble_system_layers(
            &mut budget,
            &static_ctx,
            None, // inject 走 take_inject_context / user 消息，不进 system
            &skill_index,
            &dynamic_ctx,
            &guidance,
            &timestamp,
        )
    }

    /// 与 `build_system_prompt` 同源的分层字符数，供上下文占用估算。
    /// 返回 (system, memory, skills, recall)。
    pub fn system_prompt_layer_chars(&self) -> (usize, usize, usize, usize) {
        let layers = self.system_prompt_layer_breakdown();
        (
            layers.system_chars,
            layers.memory_chars,
            layers.skills_chars,
            layers.recall_chars,
        )
    }

    /// 分层占用明细（含 system / memory / skills 子项），供 `context_usage` 快照。
    pub fn system_prompt_layer_breakdown(&self) -> crate::prompt::context_usage::LayerBreakdown {
        use crate::prompt::context_usage::{estimate_tokens, LayerBreakdown, NamedChars};

        let (static_ctx, dynamic_ctx, skill_pairs) = self.system_prompt_parts();
        let skill_index: Vec<(&str, &str)> = skill_pairs
            .iter()
            .map(|(name, desc)| (name.as_str(), desc.as_str()))
            .collect();

        let (guidance, timestamp) = self.system_prompt_guidance_timestamp();
        let mode_guidance = self.interaction_mode.system_guidance();
        let tool_guidance = crate::prompt::prompt_builder::TOOL_GUIDANCE;

        let mut system_items: Vec<NamedChars> = Vec::new();
        let mut push_sys = |id: &str, label: &str, content: &str| {
            let n = content.trim().len();
            if n > 0 {
                system_items.push((id.to_string(), label.to_string(), n));
            }
        };
        push_sys("soul", "SOUL.md", &static_ctx.soul);
        push_sys("identity", "身份", &static_ctx.identity);
        push_sys("agents", "AGENTS.md", &static_ctx.agent_md);
        push_sys("mode", "交互模式引导", mode_guidance);
        push_sys("tool_guidance", "工具指引", tool_guidance);
        push_sys("timestamp", "当前时间", &timestamp);

        let system_chars = static_ctx.soul.trim().len()
            + static_ctx.identity.trim().len()
            + static_ctx.agent_md.trim().len()
            + guidance.len()
            + timestamp.len();

        let mut memory_items: Vec<NamedChars> = Vec::new();
        let mut push_mem = |id: &str, label: &str, content: &str| {
            let n = content.trim().len();
            if n > 0 {
                memory_items.push((id.to_string(), label.to_string(), n));
            }
        };
        push_mem("memory", "MEMORY.md", &static_ctx.memory);
        push_mem("user", "USER.md", &static_ctx.user_profile);
        push_mem("daily", "今日记忆", &static_ctx.daily);
        let memory_chars: usize = memory_items.iter().map(|(_, _, n)| *n).sum();

        let skills_chars = PromptBuilder::new()
            .with_skills_index(&skill_index)
            .build()
            .len();
        let skill_items: Vec<NamedChars> = skill_pairs
            .iter()
            .map(|(name, desc)| {
                let line = format!("- **{}**: {}", name, desc);
                (name.clone(), name.clone(), line.len())
            })
            .filter(|(_, _, n)| estimate_tokens(*n) > 0)
            .collect();

        let recall_chars = dynamic_ctx.render().len();

        LayerBreakdown {
            system_chars,
            memory_chars,
            skills_chars,
            recall_chars,
            system_items,
            memory_items,
            skill_items,
        }
    }

    /// guidance（mode 在前，便于预算截断时保留）+ timestamp，与 `assemble_system_layers` 顺序一致。
    fn system_prompt_guidance_timestamp(&self) -> (String, String) {
        // mode 置于 TOOL_GUIDANCE 之前：guidance 层被 take_chars 截断时优先保留模式说明。
        let guidance = format!(
            "{}\n\n{}",
            self.interaction_mode.system_guidance(),
            crate::prompt::prompt_builder::TOOL_GUIDANCE,
        );
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z");
        let timestamp = format!("# 当前时间\n{now}");
        (guidance, timestamp)
    }
}
