//! Session turn lifecycle: input persistence, memory recall, prompt assembly, and hooks.

use session::{build_conversation_context, format_recalled_context, NewMessage};
use types::message::Message;

use std::sync::Arc;

use crate::tasks::{TaskKind, TurnInput};

use super::{looks_like_user_correction, Session, StepContext, TurnContext, TurnResult};

impl Session {
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
    pub async fn start_or_steer_turn(
        &mut self,
        user_message: &str,
        submission_id: &str,
    ) -> anyhow::Result<TurnResult> {
        self.start_or_steer_turn_with_images(user_message, &[], submission_id)
            .await
    }

    /// 同 [`Self::start_or_steer_turn`]，附带本轮图片 data URL（`data:image/...;base64,...`）。
    ///
    /// FTS 仍只索引文本；附图写入 `messages.media_json` 并进入内存 `session_messages`。
    pub async fn start_or_steer_turn_with_images(
        &mut self,
        user_message: &str,
        image_data_urls: &[String],
        _submission_id: &str,
    ) -> anyhow::Result<TurnResult> {
        if let Some(turn_id) = self.steer_input(user_message, image_data_urls) {
            return Ok(TurnResult::Steered { turn_id });
        }
        self.prepare_turn(&[TurnInput::UserInput {
            content: user_message.to_string(),
            image_data_urls: image_data_urls.to_vec(),
        }])
        .await
    }

    /// Prepare initial task input for the first sampling request.
    ///
    /// Production paths call this from [`crate::tasks::RegularTask`]. The
    /// public `start_or_steer_turn*` methods remain compatibility adapters for
    /// callers that have not yet moved input ownership into `SessionTask`.
    pub(crate) async fn prepare_turn(&mut self, input: &[TurnInput]) -> anyhow::Result<TurnResult> {
        anyhow::ensure!(!input.is_empty(), "regular turn requires initial input");
        let user_message = input
            .iter()
            .map(|item| match item {
                TurnInput::UserInput { content, .. } => content.as_str(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        self.cancel.reset();
        if self.is_budget_exhausted() {
            return Ok(TurnResult::BudgetExhausted);
        }

        self.begin_user_turn();
        if looks_like_user_correction(&user_message)
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
        self.reload_tools_and_mcp().await?;

        for item in input.iter().cloned() {
            self.record_turn_input(item)?;
        }

        let fts_keywords = if self.turn.current_turn >= self.config.recent_turns {
            Some(user_message.as_str())
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

    /// Queue user input for the active regular task.
    pub fn steer_input(&self, user_message: &str, image_data_urls: &[String]) -> Option<String> {
        if user_message.trim().is_empty() && image_data_urls.is_empty() {
            return None;
        }
        let running = self.active_turn.task.as_ref()?;
        if running.kind != TaskKind::Regular {
            return None;
        }
        let accepted = running.turn_context.push_input(TurnInput::UserInput {
            content: user_message.to_string(),
            image_data_urls: image_data_urls.to_vec(),
        });
        accepted.then(|| running.turn_context.sub_id().to_string())
    }

    pub(crate) fn record_turn_input(&mut self, input: TurnInput) -> anyhow::Result<()> {
        let TurnInput::UserInput {
            content,
            image_data_urls,
        } = input;
        self.sessions.ensure_session(&self.session_id, "tauri")?;
        let media_assets: Vec<types::MediaAsset> = image_data_urls
            .iter()
            .map(|url| url.trim())
            .filter(|url| !url.is_empty())
            .map(|url| {
                let mime = url
                    .strip_prefix("data:")
                    .and_then(|rest| rest.split(';').next())
                    .unwrap_or("image/*")
                    .to_string();
                types::MediaAsset::data_url(types::MediaKind::Image, url, mime)
            })
            .collect();
        let media_json = if media_assets.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&media_assets)?)
        };
        self.sessions.append_message(NewMessage {
            content: Some(&content),
            media_json: media_json.as_deref(),
            ..NewMessage::empty(&self.session_id, "user")
        })?;
        self.session_messages
            .push(Message::user_with_images(&content, &image_data_urls));
        Ok(())
    }

    /// Compatibility adapter for callers not yet migrated to Codex naming.
    #[deprecated(note = "use start_or_steer_turn")]
    pub async fn run_turn(
        &mut self,
        user_message: &str,
        submission_id: &str,
    ) -> anyhow::Result<TurnResult> {
        self.start_or_steer_turn(user_message, submission_id).await
    }

    /// Compatibility adapter for callers not yet migrated to Codex naming.
    #[deprecated(note = "use start_or_steer_turn_with_images")]
    pub async fn run_turn_with_images(
        &mut self,
        user_message: &str,
        image_data_urls: &[String],
        submission_id: &str,
    ) -> anyhow::Result<TurnResult> {
        self.start_or_steer_turn_with_images(user_message, image_data_urls, submission_id)
            .await
    }

    /// 准备下一轮 LLM 调用所需的上下文：重载工具/MCP、构建历史、注入 hook 上下文。
    ///
    /// 返回当前 sampling request 的不可变 [`StepContext`]。foreground、background
    /// 与 Agent Thread 路径共享同一捕获入口。
    pub(crate) async fn capture_step_context(&mut self) -> anyhow::Result<Arc<StepContext>> {
        self.reload_tools_and_mcp().await?;
        let mut history = self.provider_history();
        if let Some(ctx) = self.take_inject_context() {
            history.push(Message::user(&format!("[astro:hook-context]\n{ctx}")));
        }
        let tool_specs = self.schemas_for_api();
        let turn_context = self.current_turn_context.clone().unwrap_or_else(|| {
            Arc::new(TurnContext::new(
                self.turn
                    .current_turn_id()
                    .map(str::to_owned)
                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                self.turn.current_turn(),
                self.interaction_mode,
                self.permission_profile.clone(),
                self.project_root.clone(),
            ))
        });
        let step_context = Arc::new(StepContext::new(turn_context, history, tool_specs));
        self.current_step_context = Some(Arc::clone(&step_context));
        Ok(step_context)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::TempDir;

    use super::*;

    #[tokio::test]
    async fn capture_step_context_reuses_the_turn_snapshot() {
        let dir = TempDir::new().unwrap();
        let config = crate::runtime::Config::with_defaults(dir.path().to_path_buf());
        let mut session = Session::with_session_id(config, "step-context-test".into()).unwrap();
        session.set_interaction_mode(types::InteractionMode::Plan);
        session.set_current_turn_id("turn-1");

        let first = session.capture_step_context().await.unwrap();
        let second = session.capture_step_context().await.unwrap();

        assert!(Arc::ptr_eq(&first.turn, &second.turn));
        assert_eq!(first.turn.sub_id(), "turn-1");
        assert_eq!(first.turn.mode(), types::InteractionMode::Plan);
        assert_eq!(
            serde_json::to_value(&first.history).unwrap(),
            serde_json::to_value(&second.history).unwrap()
        );
        assert_eq!(first.tool_specs, second.tool_specs);
        assert!(first.advertises_tool("file_ops"));
        assert!(!first.advertises_tool("terminal"));
    }
}
