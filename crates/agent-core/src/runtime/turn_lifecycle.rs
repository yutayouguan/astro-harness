//! AgentLoop 轮次生命周期：用户输入处理、记忆召回、system prompt 组装与 hook 触发。

use session::{build_conversation_context, format_recalled_context, NewMessage};
use types::message::Message;

use super::{looks_like_user_correction, AgentLoop, TurnResult};

impl AgentLoop {
    /// 开始新的用户消息处理：重置 `tool_rounds` 与 `turn_wrote_disk`。
    ///
    /// 若上一轮工具次数达到 `learning.complex_task_tool_threshold`，为本轮挂起学习 nudge。
    pub fn begin_user_turn(&mut self) {
        let prev_rounds = self.turn.begin_new_turn();
        let compression = memory::load_compression_config(&self.memory.base_dir);
        self.compression.reset_for_new_turn(&compression);
        self.compression_policy = Box::new(
            crate::compression::StagedCompressionPolicy::from_config(&compression)
                .with_context_window(self.context_window()),
        );
        self.pending_learning_nudge =
            Self::compute_learning_nudge(&self.memory.base_dir, prev_rounds);
    }

    /// 根据上一轮工具次数与 DecisionLog 计算本轮是否注入学习提示。
    pub(crate) fn compute_learning_nudge(
        base: &std::path::Path,
        prev_tool_rounds: usize,
    ) -> Option<String> {
        let cfg = memory::load_learning_config(base);
        if !cfg.nudge_enabled {
            return None;
        }
        if prev_tool_rounds < cfg.complex_task_tool_threshold {
            return None;
        }
        let mut text = format!(
            "上一轮使用了 {prev_tool_rounds} 次工具（≥ {}）。若流程可复用：用 `skills` manage create 或 patch 固化；若是长期偏好/事实：用 `memory` 写入。闲置技能可用 action=curate 查看建议（勿自动删除）。",
            cfg.complex_task_tool_threshold
        );
        if let Ok(recent) = memory::list_recent_decisions(base, 8) {
            if recent
                .iter()
                .any(|e| e.kind == memory::DecisionKind::ToolFailure)
            {
                text.push_str(
                    " 近期有工具失败记录：若已找到正确路径，请用 skills manage patch 写回对应 Skill。",
                );
            }
        }
        Some(text)
    }

    /// 处理一轮用户输入：记录消息、召回记忆、构建 system prompt。
    ///
    /// 返回 [`TurnResult::Continue`] 供上层发起 LLM 请求；预算耗尽或已取消时提前返回。
    /// 注意：本方法不直接调用 LLM，仅完成 Agent 侧准备工作。
    pub async fn run_turn(
        &mut self,
        user_message: &str,
        _task_id: &str,
    ) -> anyhow::Result<TurnResult> {
        self.run_turn_with_images(user_message, &[], _task_id).await
    }

    /// 同 [`Self::run_turn`]，附带本轮图片 data URL（`data:image/...;base64,...`）。
    ///
    /// FTS 仍只索引文本；附图写入 `messages.media_json` 并进入内存 `session_messages`。
    pub async fn run_turn_with_images(
        &mut self,
        user_message: &str,
        image_data_urls: &[String],
        _task_id: &str,
    ) -> anyhow::Result<TurnResult> {
        self.cancel.reset();
        if self.is_budget_exhausted() {
            return Ok(TurnResult::BudgetExhausted);
        }

        self.begin_user_turn();
        if looks_like_user_correction(user_message)
            && self
                .session_messages
                .iter()
                .any(|m| matches!(m.role, types::message::Role::Assistant))
        {
            memory::try_append_decision(
                self.memory.base_dir.as_path(),
                memory::DecisionEntry::new(
                    memory::DecisionKind::UserCorrection,
                    user_message.chars().take(200).collect::<String>(),
                )
                .with_session(self.session_id.clone()),
            );
        }
        self.reload_tools_and_mcp().await;

        self.sessions.ensure_session(&self.session_id, "tauri")?;
        let media_assets: Vec<types::MediaAsset> = image_data_urls
            .iter()
            .map(|u| u.trim())
            .filter(|u| !u.is_empty())
            .map(|u| {
                let mime = u
                    .strip_prefix("data:")
                    .and_then(|rest| rest.split(';').next())
                    .unwrap_or("image/*")
                    .to_string();
                types::MediaAsset::data_url(types::MediaKind::Image, u, mime)
            })
            .collect();
        let media_owned = if media_assets.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&media_assets)?)
        };
        self.sessions.append_message(NewMessage {
            content: Some(user_message),
            media_json: media_owned.as_deref(),
            ..NewMessage::empty(&self.session_id, "user")
        })?;

        let fts_keywords = if self.turn.current_turn >= self.config.recent_turns {
            Some(user_message)
        } else {
            None
        };
        let recalled = build_conversation_context(
            &*self.sessions,
            &self.session_id,
            self.config.recent_turns,
            fts_keywords,
        )?;
        self.compression.last_recalled_context = format_recalled_context(&recalled);

        self.session_messages
            .push(Message::user_with_images(user_message, image_data_urls));
        self.increment_turn();
        let system_prompt = self.build_system_prompt();
        let _ = self.fire_hook(
            ::hooks::ON_SESSION_START,
            ::hooks::HookPayload {
                session_id: self.session_id.clone(),
                turn_id: self.turn.current_turn_id.clone(),
                detail: format!("session={}", self.session_id),
                ..Default::default()
            },
        );
        let inject = self.fire_hook(
            ::hooks::PRE_LLM_CALL,
            ::hooks::HookPayload {
                session_id: self.session_id.clone(),
                turn_id: self.turn.current_turn_id.clone(),
                system_prompt_chars: Some(system_prompt.len()),
                detail: format!("system_prompt_chars={}", system_prompt.len()),
                ..Default::default()
            },
        );
        if let ::hooks::HookOutcome::InjectContext(ctx) = inject {
            self.pending_inject_context = Some(ctx);
        }
        if self.cancel.is_cancelled() {
            return Ok(TurnResult::Interrupted);
        }
        Ok(TurnResult::Continue {
            turn: self.turn.current_turn,
            system_prompt,
        })
    }

    /// 准备下一轮 LLM 调用所需的上下文：重载工具/MCP、构建历史、注入 hook 上下文。
    ///
    /// 返回 `(messages, tool_schemas)`，供 `ProviderStreamer::stream_chat` 或
    /// `to_provider_messages` 使用。streaming 与 headless 路径共享。
    pub(crate) async fn prepare_llm_context(&mut self) -> (Vec<Message>, Vec<serde_json::Value>) {
        self.reload_tools_and_mcp().await;
        let mut messages = self.provider_history();
        if let Some(ctx) = self.take_inject_context() {
            messages.push(Message::user(&format!("[astro:hook-context]\n{ctx}")));
        }
        let tools = self.schemas_for_api();
        (messages, tools)
    }
}
