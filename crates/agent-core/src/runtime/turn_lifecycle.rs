//! Session turn 生命周期：输入持久化、记忆召回、prompt 组装与 hooks。

use session::{
    build_conversation_context, format_recalled_context, ConversationStore, NewResponseItem,
};

use std::collections::HashSet;
use std::sync::Arc;

use agent_protocol::{TurnInputError, TurnInputMode, TurnInputRequest, TurnInputSubmission};

use crate::tasks::{RegularTask, TaskKind, TurnInput};

use super::turn_context::QueuedTurnInput;
use super::{looks_like_user_correction, Session, StepContext, TurnContext, TurnResult};

fn coalesce_turn_inputs<I>(inputs: I) -> Option<TurnInput>
where
    I: IntoIterator<Item = TurnInput>,
{
    let mut contents = Vec::new();
    let mut image_data_urls = Vec::new();
    for input in inputs {
        let TurnInput {
            content,
            image_data_urls: images,
            client_message_id: _,
        } = input;
        contents.push(content);
        image_data_urls.extend(images);
    }
    (!contents.is_empty()).then(|| TurnInput {
        content: contents.join("\n\n"),
        image_data_urls,
        client_message_id: None,
    })
}

fn discovered_deferred_tool_names(history: &[agent_protocol::ResponseItem]) -> HashSet<&str> {
    history
        .iter()
        .filter_map(|item| match item {
            agent_protocol::ResponseItem::ToolSearchOutput { status, tools, .. }
                if status == "completed" =>
            {
                Some(tools)
            }
            _ => None,
        })
        .flatten()
        .filter_map(|tool| {
            tool.get("name")
                .or_else(|| tool.pointer("/function/name"))
                .and_then(serde_json::Value::as_str)
        })
        .collect()
}

fn response_item_for_turn_input(
    content: &str,
    image_data_urls: &[String],
    marker: Option<&str>,
) -> agent_protocol::ResponseItem {
    let mut response_content = vec![agent_protocol::ContentItem::InputText {
        text: content.to_string(),
    }];
    response_content.extend(image_data_urls.iter().map(|image_url| {
        agent_protocol::ContentItem::InputImage {
            image_url: image_url.clone(),
            detail: None,
        }
    }));
    agent_protocol::ResponseItem::Message {
        id: None,
        role: "user".into(),
        content: response_content,
        phase: None,
        internal_chat_message_metadata_passthrough: marker
            .map(|value| serde_json::json!({ "astro_memory_marker": value })),
    }
}

fn response_item_memory_marker(item: &agent_protocol::ResponseItem) -> Option<&str> {
    item.metadata()?.get("astro_memory_marker")?.as_str()
}

impl Session {
    /// 开始新的用户消息处理：重置 `tool_rounds` 与 `turn_wrote_disk`。
    ///
    /// 若上一轮工具次数达到 `learning.complex_task_tool_threshold`，为本轮挂起学习 nudge。
    pub async fn begin_user_turn(&self) {
        let memory_dir = self.memory_dir().to_path_buf();
        let compression = memory::load_compression_config(&memory_dir);
        {
            let mut state = self.lock_state();
            let prev_rounds = state.turn.begin_new_turn();
            state.compression.reset_for_new_turn(&compression);
            state.pending_learning_nudge = Self::compute_learning_nudge(&memory_dir, prev_rounds);
        }
        let context_window = self.context_window();
        *self
            .services
            .compression_policy
            .lock()
            .expect("compression policy mutex poisoned") = Box::new(
            crate::compression::StagedCompressionPolicy::from_config(&compression)
                .with_context_window(context_window),
        );
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
        &self,
        user_message: &str,
        submission_id: &str,
    ) -> anyhow::Result<TurnResult> {
        self.start_or_steer_turn_with_images(user_message, &[], submission_id)
            .await
    }

    /// 同 [`Self::start_or_steer_turn`]，附带本轮图片 data URL（`data:image/...;base64,...`）。
    ///
    /// FTS 仍只索引文本；附图写入 `messages.media_json` 并进入内存 `SessionState.history`。
    pub async fn start_or_steer_turn_with_images(
        &self,
        user_message: &str,
        image_data_urls: &[String],
        _submission_id: &str,
    ) -> anyhow::Result<TurnResult> {
        if let Some(turn_id) = self.steer_input(user_message, image_data_urls).await? {
            return Ok(TurnResult::Steered { turn_id });
        }
        self.prepare_turn(&[TurnInput {
            content: user_message.to_string(),
            image_data_urls: image_data_urls.to_vec(),
            client_message_id: None,
        }])
        .await
    }

    pub(crate) async fn submit_turn_input(
        self: &Arc<Self>,
        submission_id: String,
        request: TurnInputRequest,
        mode: TurnInputMode,
        responses_override: Option<crate::streaming::ResponsesOverride>,
    ) -> Result<TurnInputSubmission, TurnInputError> {
        let TurnInputRequest {
            input,
            thread_settings,
        } = request;
        if input.is_empty()
            || input
                .iter()
                .all(|item| item.content.trim().is_empty() && item.image_data_urls.is_empty())
        {
            return Err(TurnInputError::Invalid(
                "turn input must contain text or an image".into(),
            ));
        }
        let active_turn_id = self.active_turn_id().await;
        if active_turn_id.is_none()
            && matches!(
                mode,
                TurnInputMode::StartOrSteer | TurnInputMode::StartIfIdle
            )
            && self.terminating_turn_id().await.is_some()
        {
            return Ok(TurnInputSubmission::NotSubmitted {
                reason: "terminating".into(),
            });
        }
        match mode {
            TurnInputMode::StartOrSteer => match active_turn_id {
                Some(turn_id) => {
                    self.steer_turn(submission_id, Some(&turn_id), input, thread_settings)
                        .await
                }
                None => {
                    self.start_turn(submission_id, input, thread_settings, responses_override)
                        .await
                }
            },
            TurnInputMode::StartIfIdle => match active_turn_id {
                Some(_) => Ok(TurnInputSubmission::NotSubmitted {
                    reason: "not_idle".into(),
                }),
                None => {
                    self.start_turn(submission_id, input, thread_settings, responses_override)
                        .await
                }
            },
            TurnInputMode::Steer { expected_turn_id } => {
                self.steer_turn(
                    submission_id,
                    Some(&expected_turn_id),
                    input,
                    thread_settings,
                )
                .await
            }
        }
    }

    pub(crate) async fn active_turn_id(&self) -> Option<String> {
        let active_turn = self.active_turn.lock().await;
        active_turn
            .as_ref()?
            .task
            .as_ref()
            .filter(|running| !running.cancellation_token.is_cancelled())
            .map(|running| running.turn_context.sub_id().to_string())
    }

    async fn start_turn(
        self: &Arc<Self>,
        turn_id: String,
        input: Vec<TurnInput>,
        thread_settings: agent_protocol::ThreadSettingsOverrides,
        responses_override: Option<crate::streaming::ResponsesOverride>,
    ) -> Result<TurnInputSubmission, TurnInputError> {
        let has_settings = !thread_settings.is_empty();
        let previous_settings = has_settings.then(|| self.snapshot_request_settings());
        let applied_settings = if has_settings {
            Some(
                self.apply_thread_settings(thread_settings)
                    .map_err(TurnInputError::Invalid)?,
            )
        } else {
            None
        };
        let context = self.create_turn_context(turn_id.clone()).await;
        let args = crate::streaming::multi_turn::RunTurnArgs::submitted(
            Arc::clone(self),
            Arc::clone(&context),
            responses_override,
        );
        let applied_event = async {
            if let Some(thread_settings) = applied_settings {
                self.send_event(
                    &turn_id,
                    agent_protocol::EventMsg::ThreadSettingsApplied(
                        agent_protocol::ThreadSettingsAppliedEvent { thread_settings },
                    ),
                )
                .await;
            }
        };
        if let Err(error) = self
            .spawn_task_with_install_hook(context, input, RegularTask::new(args), applied_event)
            .await
        {
            if let Some(previous_settings) = previous_settings {
                self.restore_request_settings(previous_settings);
            }
            return Err(TurnInputError::Invalid(error.to_string()));
        }
        Ok(TurnInputSubmission::Started { turn_id })
    }

    pub(crate) async fn recover_turn(
        self: &Arc<Self>,
        turn_id: String,
        responses_override: Option<crate::streaming::ResponsesOverride>,
    ) -> Result<TurnInputSubmission, TurnInputError> {
        if self.active_turn_id().await.is_some() || self.terminating_turn_id().await.is_some() {
            return Ok(TurnInputSubmission::NotSubmitted {
                reason: "not_idle".into(),
            });
        }
        if !self
            .is_recoverable_turn(&turn_id)
            .await
            .map_err(|error| TurnInputError::Invalid(error.to_string()))?
        {
            return Ok(TurnInputSubmission::NotSubmitted {
                reason: "turn_not_recoverable".into(),
            });
        }
        let context = self.create_turn_context(turn_id.clone()).await;
        let args = crate::streaming::multi_turn::RunTurnArgs::submitted(
            Arc::clone(self),
            Arc::clone(&context),
            responses_override,
        );
        self.spawn_task(context, Vec::new(), RegularTask::recovery(args))
            .await
            .map_err(|error| TurnInputError::Invalid(error.to_string()))?;
        Ok(TurnInputSubmission::Started { turn_id })
    }

    async fn is_recoverable_turn(&self, turn_id: &str) -> anyhow::Result<bool> {
        let rollout_root = self.memory_dir().join("sessions").join("rollouts");
        let Some(path) = agent_rollout::find_rollout(&rollout_root, self.session_id())? else {
            return Ok(false);
        };
        let mut started = false;
        let mut recoverable_terminal = None;
        for item in agent_rollout::read_rollout(&path).await? {
            let agent_rollout::RolloutItem::EventMsg(event) = item else {
                continue;
            };
            match event {
                agent_protocol::EventMsg::TurnStarted(event) if event.turn_id == turn_id => {
                    started = true;
                }
                agent_protocol::EventMsg::TurnAborted(event)
                    if event.turn_id.as_deref() == Some(turn_id) =>
                {
                    recoverable_terminal =
                        Some(event.reason == agent_protocol::TurnAbortReason::Interrupted);
                }
                agent_protocol::EventMsg::TurnComplete(event) if event.turn_id == turn_id => {
                    recoverable_terminal = Some(false);
                }
                _ => {}
            }
        }
        Ok(started && recoverable_terminal.unwrap_or(true))
    }

    async fn steer_turn(
        &self,
        submission_id: String,
        expected_turn_id: Option<&str>,
        input: Vec<TurnInput>,
        thread_settings: agent_protocol::ThreadSettingsOverrides,
    ) -> Result<TurnInputSubmission, TurnInputError> {
        let turn_id = self.active_turn_id().await.ok_or_else(|| {
            TurnInputError::Invalid("no active turn available for steering".into())
        })?;
        if expected_turn_id.is_some_and(|expected| expected != turn_id) {
            return Ok(TurnInputSubmission::NotSubmitted {
                reason: "turn_id_mismatch".into(),
            });
        }
        for item in input {
            let accepted = self
                .steer_input_for_turn(
                    &item.content,
                    &item.image_data_urls,
                    Some(&turn_id),
                    item.client_message_id.as_deref(),
                )
                .await
                .map_err(|error| TurnInputError::Invalid(error.to_string()))?;
            if accepted.is_none() {
                return Ok(TurnInputSubmission::NotSubmitted {
                    reason: "turn_not_accepting_input".into(),
                });
            }
        }
        if !thread_settings.is_empty() {
            let thread_settings = self
                .apply_thread_settings(thread_settings)
                .map_err(TurnInputError::Invalid)?;
            self.send_event(
                &submission_id,
                agent_protocol::EventMsg::ThreadSettingsApplied(
                    agent_protocol::ThreadSettingsAppliedEvent { thread_settings },
                ),
            )
            .await;
        }
        Ok(TurnInputSubmission::Steered { turn_id })
    }

    /// 为首次采样请求准备初始任务输入。
    ///
    /// 生产路径从 [`crate::tasks::RegularTask`] 调用此方法。公开的
    /// `start_or_steer_turn*` 方法作为兼容适配器保留，供尚未将输入
    /// 所有权迁移到 `SessionTask` 的调用方使用。
    pub(crate) async fn prepare_turn(&self, input: &[TurnInput]) -> anyhow::Result<TurnResult> {
        anyhow::ensure!(!input.is_empty(), "regular turn requires initial input");
        let client_message_ids = input
            .iter()
            .filter_map(|item| item.client_message_id.clone())
            .collect::<Vec<_>>();
        let coalesced_input =
            coalesce_turn_inputs(input.iter().cloned()).expect("non-empty input coalesces");
        let user_message = coalesced_input.content.clone();
        self.cancel.reset();
        if self.is_budget_exhausted().await {
            return Ok(TurnResult::BudgetExhausted);
        }

        let turn_id = self.current_turn_id().await;
        let admission_context = self.admit_initial_input(input, turn_id).await?;

        self.begin_user_turn().await;
        if looks_like_user_correction(&user_message)
            && self
                .clone_history()
                .await
                .iter()
                .any(|item| item.role() == Some("assistant"))
            && self.config.thread_memory_mode == types::ThreadMemoryMode::Enabled
        {
            memory::try_append_decision(
                self.memory_dir(),
                memory::DecisionEntry::new(
                    memory::DecisionKind::UserCorrection,
                    user_message.chars().take(200).collect::<String>(),
                )
                .with_session(self.session_id.clone()),
            );
        }
        self.reload_tools_and_mcp().await?;

        self.record_turn_input(coalesced_input).await?;
        let turn_id = self
            .current_turn_id()
            .await
            .unwrap_or_else(|| self.session_id.clone());
        for client_message_id in client_message_ids {
            self.send_event(
                &turn_id,
                agent_protocol::EventMsg::UserInputCommitted(
                    agent_protocol::UserInputCommittedEvent {
                        turn_id: turn_id.clone(),
                        client_message_id,
                    },
                ),
            )
            .await;
        }

        self.finish_prepared_turn(&user_message, admission_context.as_deref())
            .await
    }

    pub(crate) async fn prepare_recovery_turn(&self) -> anyhow::Result<TurnResult> {
        self.cancel.reset();
        if self.is_budget_exhausted().await {
            return Ok(TurnResult::BudgetExhausted);
        }
        self.begin_user_turn().await;
        self.reload_tools_and_mcp().await?;
        self.finish_prepared_turn("", None).await
    }

    /// 准备后续轮次，其持久化邮箱输入在采样前与序列标记一起持久化。
    /// 重试会收敛到已有标记，因此不会追加重复的用户消息。
    pub(crate) async fn prepare_mailbox_turn(&self) -> anyhow::Result<TurnResult> {
        self.cancel.reset();
        if self.is_budget_exhausted().await {
            return Ok(TurnResult::BudgetExhausted);
        }
        self.begin_user_turn().await;
        self.reload_tools_and_mcp().await?;
        let outcome = crate::exec::subagents::drain_mailbox_at_safe_boundary(self).await?;
        anyhow::ensure!(!outcome.deferred, "follow-up mailbox input was deferred");
        anyhow::ensure!(
            outcome.delivered > 0,
            "follow-up turn has no durable mailbox input"
        );
        let turn_id = self
            .current_turn_id()
            .await
            .unwrap_or_else(|| self.session_id.clone());
        for client_message_id in outcome.delivered_client_message_ids {
            self.send_event(
                &turn_id,
                agent_protocol::EventMsg::UserInputCommitted(
                    agent_protocol::UserInputCommittedEvent {
                        turn_id: turn_id.clone(),
                        client_message_id,
                    },
                ),
            )
            .await;
        }
        let user_message = self
            .clone_history()
            .await
            .iter()
            .rev()
            .find(|item| {
                item.role() == Some("user")
                    && response_item_memory_marker(item).is_some_and(|marker| {
                        marker.starts_with(crate::exec::subagents::MAILBOX_FINISH_PREFIX)
                    })
            })
            .map(agent_protocol::ResponseItem::text)
            .ok_or_else(|| {
                anyhow::anyhow!("follow-up mailbox input is missing from runtime history")
            })?;
        self.finish_prepared_turn(&user_message, None).await
    }

    async fn finish_prepared_turn(
        &self,
        user_message: &str,
        admission_context: Option<&str>,
    ) -> anyhow::Result<TurnResult> {
        let current_turn = self.lock_state().turn.current_turn;
        let fts_keywords = if current_turn >= self.config.recent_turns {
            Some(user_message)
        } else {
            None
        };
        let recalled = if let Some(keywords) = fts_keywords.filter(|value| !value.trim().is_empty())
        {
            let visible_history = self
                .provider_response_history()
                .await
                .into_iter()
                .map(|item| {
                    let role = item.role().unwrap_or_else(|| {
                        if item.is_tool_output() {
                            "tool"
                        } else {
                            "assistant"
                        }
                    });
                    (role.to_string(), item.provider_view_text())
                })
                .collect::<std::collections::HashSet<_>>();
            let mut recalled = build_conversation_context(
                &self.services.sessions,
                &self.session_id,
                self.config.recent_turns,
                Some(keywords),
            )
            .await?;
            recalled.retain(|entry| {
                !visible_history.contains(&(
                    entry.item.role().unwrap_or("item").to_string(),
                    entry.item.text(),
                ))
            });
            recalled
        } else {
            Vec::new()
        };
        self.lock_state().compression.last_recalled_context = format_recalled_context(&recalled);

        self.increment_turn().await;
        let prompt = self
            .build_prompt_contract_with_inject(admission_context)
            .await;
        self.persist_prompt_context_if_changed(&prompt).await;
        let system_prompt = prompt.flattened();
        let turn_id = self.current_turn_id().await;
        let inject = self.fire_hook(
            ::hooks::PRE_LLM_CALL,
            ::hooks::HookPayload {
                session_id: self.session_id.clone(),
                turn_id,
                system_prompt_chars: Some(system_prompt.len()),
                detail: format!("system_prompt_chars={}", system_prompt.len()),
                ..Default::default()
            },
        );
        if let ::hooks::HookOutcome::InjectContext(ctx) = inject {
            self.lock_state().pending_inject_context = Some(ctx);
        }
        if self.cancel.is_cancelled() {
            return Ok(TurnResult::Interrupted);
        }
        Ok(TurnResult::Continue {
            turn: self.session_turn().await,
            system_prompt,
            prompt,
        })
    }

    fn append_inject_context(slot: &mut Option<String>, context: String) {
        *slot = Some(match slot.take() {
            Some(existing) => format!("{existing}\n\n{context}"),
            None => context,
        });
    }

    fn apply_admission_outcome(
        event_name: &str,
        should_stop: bool,
        stop_reason: Option<String>,
        additional_contexts: Vec<String>,
    ) -> anyhow::Result<Option<String>> {
        if should_stop {
            anyhow::bail!(
                "{event_name} blocked by hook: {}",
                stop_reason.unwrap_or_else(|| "hook requested stop".into())
            );
        }
        Ok((!additional_contexts.is_empty()).then(|| additional_contexts.join("\n\n")))
    }

    async fn admit_initial_input(
        &self,
        input: &[TurnInput],
        turn_id: Option<String>,
    ) -> anyhow::Result<Option<String>> {
        let _admission_guard = self.admission_lock.lock().await;
        let mut contexts = Vec::new();
        if let Some(context) = self.admit_session_start_locked(turn_id.clone()).await? {
            contexts.push(context);
        }
        for item in input {
            if let Some(context) = self.admit_user_prompt(&item.content, turn_id.clone())? {
                contexts.push(context);
            }
        }
        Ok((!contexts.is_empty()).then(|| contexts.join("\n\n")))
    }

    #[cfg(test)]
    async fn admit_session_start(&self) -> anyhow::Result<Option<String>> {
        let _admission_guard = self.admission_lock.lock().await;
        self.admit_session_start_locked(self.current_turn_id().await)
            .await
    }

    async fn admit_session_start_locked(
        &self,
        turn_id: Option<String>,
    ) -> anyhow::Result<Option<String>> {
        let Some(source) = self.lock_state().pending_session_start_source.clone() else {
            return Ok(None);
        };
        let subagent = self.subagent_hook_context();
        let event_name = if subagent.is_some() {
            ::hooks::SUBAGENT_START
        } else {
            ::hooks::SESSION_START
        };
        // SubagentStart 仅在子 agent 启动准入时触发。恢复/后续轮次
        // 由各自的 SubagentStop 表示。
        let context = if subagent.is_some() && source != "startup" {
            None
        } else {
            let source_kind = match source.as_str() {
                "startup" => ::hooks::SessionStartSource::Startup,
                "resume" => ::hooks::SessionStartSource::Resume,
                "clear" => ::hooks::SessionStartSource::Clear,
                "compact" => ::hooks::SessionStartSource::Compact,
                other => anyhow::bail!("unsupported SessionStart source: {other}"),
            };
            let outcome = self.run_session_start_hook(source_kind, turn_id);
            Self::apply_admission_outcome(
                event_name,
                outcome.should_stop,
                outcome.stop_reason,
                outcome.additional_contexts,
            )?
        };
        let mut state = self.lock_state();
        if state.pending_session_start_source.as_deref() == Some(source.as_str()) {
            state.pending_session_start_source = None;
        }
        Ok(context)
    }

    fn admit_user_prompt(
        &self,
        prompt: &str,
        turn_id: Option<String>,
    ) -> anyhow::Result<Option<String>> {
        let outcome = self.run_user_prompt_submit_hook(turn_id, prompt.to_string());
        Self::apply_admission_outcome(
            ::hooks::USER_PROMPT_SUBMIT,
            outcome.should_stop,
            outcome.stop_reason,
            outcome.additional_contexts,
        )
    }

    /// 将用户输入排入活跃的常规任务队列。
    pub async fn steer_input(
        &self,
        user_message: &str,
        image_data_urls: &[String],
    ) -> anyhow::Result<Option<String>> {
        self.steer_input_for_turn(user_message, image_data_urls, None, None)
            .await
    }

    /// 仅当预期的活跃 turn 仍持有会话时，将用户输入排入队列。
    pub async fn steer_input_for_turn(
        &self,
        user_message: &str,
        image_data_urls: &[String],
        expected_turn_id: Option<&str>,
        client_message_id: Option<&str>,
    ) -> anyhow::Result<Option<String>> {
        if user_message.trim().is_empty() && image_data_urls.is_empty() {
            return Ok(None);
        }
        let running = {
            let active_turn = self.active_turn.lock().await;
            let Some(running) = active_turn.as_ref().and_then(|turn| turn.task.as_ref()) else {
                return Ok(None);
            };
            (running.kind, Arc::clone(&running.turn_context))
        };
        if running.0 != TaskKind::Regular {
            return Ok(None);
        }
        let turn_id = running.1.sub_id().to_string();
        if expected_turn_id
            .filter(|expected| !expected.is_empty())
            .is_some_and(|expected| expected != turn_id)
        {
            return Ok(None);
        }
        let Some(admission_reservation) = running.1.reserve_input().await else {
            return Ok(None);
        };
        let _admission_guard = self.admission_lock.lock().await;
        let context = self.admit_user_prompt(user_message, Some(turn_id.clone()))?;
        let input = TurnInput {
            content: user_message.to_string(),
            image_data_urls: image_data_urls.to_vec(),
            client_message_id: client_message_id.map(str::to_string),
        };
        let Some(message_id) = running.1.reserve_mailbox_input() else {
            return Ok(None);
        };
        let payload =
            match crate::exec::subagents::encode_main_steer_input_with_context(&input, context) {
                Ok(payload) => payload,
                Err(error) => {
                    running.1.retract_input(&message_id);
                    return Err(error);
                }
            };
        if let Err(error) = self
            .services
            .agent_control
            .persist_main_steer_with_id(&self.services.agent_path, message_id.clone(), payload)
            .await
        {
            running.1.retract_input(&message_id);
            return Err(error);
        }
        self.services.agent_control.notify_main_steer();
        drop(admission_reservation);
        Ok(Some(turn_id))
    }

    pub(crate) async fn queue_inject_contexts<I>(&self, contexts: I)
    where
        I: IntoIterator<Item = String>,
    {
        let mut state = self.lock_state();
        for context in contexts {
            Self::append_inject_context(&mut state.pending_inject_context, context);
        }
    }

    pub(crate) async fn record_queued_turn_inputs(
        &self,
        queued_inputs: Vec<QueuedTurnInput>,
    ) -> anyhow::Result<()> {
        if queued_inputs.is_empty() {
            return Ok(());
        }
        let mut contexts = Vec::new();
        let inputs = queued_inputs.into_iter().map(|queued| {
            if let Some(context) = queued.inject_context {
                contexts.push(context);
            }
            queued.input
        });
        self.record_turn_inputs(inputs).await?;
        self.queue_inject_contexts(contexts).await;
        Ok(())
    }

    pub(crate) async fn record_turn_inputs<I>(&self, inputs: I) -> anyhow::Result<()>
    where
        I: IntoIterator<Item = TurnInput>,
    {
        if let Some(input) = coalesce_turn_inputs(inputs) {
            self.record_turn_input(input).await?;
        }
        Ok(())
    }

    pub(crate) async fn record_turn_input(&self, input: TurnInput) -> anyhow::Result<()> {
        let _write_guard = self.conversation_write_lock.lock().await;
        self.persist_turn_input(&input, None).await?;
        self.record_turn_input_in_memory_unlocked(&input, None)
            .await;
        Ok(())
    }

    pub(crate) async fn persist_turn_input(
        &self,
        input: &TurnInput,
        memory_marker: Option<&str>,
    ) -> anyhow::Result<()> {
        let TurnInput {
            content,
            image_data_urls,
            client_message_id: _,
        } = input;
        self.services
            .sessions
            .ensure_session(&self.session_id, "tauri")
            .await?;
        let item = response_item_for_turn_input(content, image_data_urls, memory_marker);
        self.services
            .sessions
            .append_response_item(NewResponseItem {
                session_id: &self.session_id,
                item: &item,
                token_count: None,
                finish_reason: None,
            })
            .await?;
        #[cfg(test)]
        if let Some(hook) = self
            .services
            .turn_input_after_db_write
            .lock()
            .map_err(|_| anyhow::anyhow!("turn input DB-write hook mutex poisoned"))?
            .clone()
        {
            hook()?;
        }
        Ok(())
    }

    pub(crate) async fn record_turn_input_in_memory(
        &self,
        input: &TurnInput,
        marker: Option<&str>,
    ) {
        let _write_guard = self.conversation_write_lock.lock().await;
        self.record_turn_input_in_memory_unlocked(input, marker)
            .await;
    }

    async fn record_turn_input_in_memory_unlocked(&self, input: &TurnInput, marker: Option<&str>) {
        let TurnInput {
            content,
            image_data_urls,
            client_message_id: _,
        } = input;
        let item = response_item_for_turn_input(content, image_data_urls, marker);
        if let Err(error) = self
            .persist_rollout_items(std::slice::from_ref(&item))
            .await
        {
            tracing::warn!(%error, "failed to persist turn input response item");
        }
        self.record_response_items_unlocked(vec![item]);
        #[cfg(test)]
        if let Some(hook) = self
            .services
            .turn_input_after_memory_write
            .lock()
            .expect("turn input memory-write hook mutex poisoned")
            .clone()
        {
            hook();
        }
    }

    pub(crate) async fn ensure_durable_turn_input_marker(
        &self,
        marker: &str,
    ) -> anyhow::Result<bool> {
        let messages = self
            .services
            .sessions
            .get_response_items(&self.session_id)
            .await?;
        let Some(_message) = messages.iter().find(|message| {
            matches!(&message.item, agent_protocol::ResponseItem::Message { role, .. } if role == "user")
                && response_item_memory_marker(&message.item) == Some(marker)
        }) else {
            return Ok(false);
        };
        Ok(true)
    }

    #[cfg(test)]
    pub(crate) fn set_turn_input_after_db_write_hook(
        &self,
        hook: Option<super::session_services::TurnInputDbWriteHook>,
    ) {
        *self
            .services
            .turn_input_after_db_write
            .lock()
            .expect("turn input DB-write hook mutex poisoned") = hook;
    }

    #[cfg(test)]
    pub(crate) fn set_turn_input_after_memory_write_hook(
        &self,
        hook: Option<super::session_services::TurnInputMemoryWriteHook>,
    ) {
        *self
            .services
            .turn_input_after_memory_write
            .lock()
            .expect("turn input memory-write hook mutex poisoned") = hook;
    }

    /// 兼容适配器，供尚未迁移到当前命名的调用方使用。
    #[deprecated(note = "use start_or_steer_turn")]
    pub async fn run_turn(
        &self,
        user_message: &str,
        submission_id: &str,
    ) -> anyhow::Result<TurnResult> {
        self.start_or_steer_turn(user_message, submission_id).await
    }

    /// 兼容适配器，供尚未迁移到当前命名的调用方使用。
    #[deprecated(note = "use start_or_steer_turn_with_images")]
    pub async fn run_turn_with_images(
        &self,
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
    pub(crate) async fn capture_step_context(&self) -> anyhow::Result<Arc<StepContext>> {
        self.reload_tools_and_mcp().await?;
        let mut history = self.provider_response_history().await;
        let prompt_context = self.prompt_context_history();
        if let Some(ctx) = self.take_inject_context().await {
            history.push(agent_protocol::ResponseItem::Message {
                id: None,
                role: "user".into(),
                content: vec![agent_protocol::ContentItem::InputText {
                    text: format!("[astro:hook-context]\n{ctx}"),
                }],
                phase: None,
                internal_chat_message_metadata_passthrough: None,
            });
        }
        let turn_context = {
            let state = self.lock_state();
            state.current_turn_context.clone().unwrap_or_else(|| {
                Arc::new(TurnContext::new_with_roots(
                    state
                        .turn
                        .current_turn_id()
                        .map(str::to_owned)
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                    state.turn.current_turn(),
                    state.interaction_mode,
                    state.permission_profile.clone(),
                    state.project_root.clone(),
                    state.workspace_roots.clone(),
                ))
            })
        };
        let interaction_mode = turn_context.mode();
        let discovered_deferred = discovered_deferred_tool_names(&history);
        let tool_router = {
            let registry = self
                .services
                .tool_registry
                .read()
                .expect("tool registry lock poisoned");
            let (visible_specs, discovered_specs) = registry.schemas_for_step(&discovered_deferred);
            let visible_specs = tools::filter_schemas(interaction_mode, visible_specs);
            let discovered_specs = tools::filter_schemas(interaction_mode, discovered_specs);
            Arc::new(crate::runtime::ToolRouter::from_registry(
                &registry,
                &discovered_specs,
                visible_specs,
            ))
        };
        let step_context = Arc::new(StepContext::new(
            turn_context,
            history,
            prompt_context,
            tool_router,
        ));
        self.lock_state().current_step_context = Some(Arc::clone(&step_context));
        Ok(step_context)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use tempfile::TempDir;

    use super::*;

    fn input(content: &str) -> TurnInput {
        TurnInput {
            content: content.to_string(),
            image_data_urls: Vec::new(),
            client_message_id: None,
        }
    }

    #[test]
    fn coalesce_turn_inputs_preserves_text_and_image_order() {
        let coalesced = coalesce_turn_inputs(vec![
            TurnInput {
                content: "first".into(),
                image_data_urls: vec!["image-a".into()],
                client_message_id: None,
            },
            TurnInput {
                content: String::new(),
                image_data_urls: vec!["image-b".into(), "image-c".into()],
                client_message_id: None,
            },
            input("third"),
        ])
        .unwrap();

        assert_eq!(
            coalesced,
            TurnInput {
                content: "first\n\n\n\nthird".into(),
                image_data_urls: vec!["image-a".into(), "image-b".into(), "image-c".into()],
                client_message_id: None,
            }
        );
    }

    #[tokio::test]
    async fn initial_inputs_are_persisted_as_one_logical_user_message() {
        let dir = TempDir::new().unwrap();
        let config = crate::runtime::Config::with_defaults(dir.path().to_path_buf());
        let session = Session::with_session_id(config, "coalesced-initial".into())
            .await
            .unwrap();

        session
            .prepare_turn(&[input("first"), input("second")])
            .await
            .unwrap();

        let history = session.clone_history().await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content_str(), "first\n\nsecond");
    }

    #[tokio::test]
    async fn initial_input_ack_is_emitted_only_after_db_and_memory_recording() {
        let dir = TempDir::new().unwrap();
        let config = crate::runtime::Config::with_defaults(dir.path().to_path_buf());
        let session = Session::with_session_id(config, "initial-input-ack".into())
            .await
            .unwrap();
        session.set_current_turn_id("turn-initial-ack").await;
        let events = session.subscribe_turn_events("turn-initial-ack").await;
        let memory_recorded = Arc::new(std::sync::atomic::AtomicBool::new(false));
        session.set_turn_input_after_memory_write_hook(Some({
            let memory_recorded = Arc::clone(&memory_recorded);
            Arc::new(move || memory_recorded.store(true, Ordering::SeqCst))
        }));

        session
            .prepare_turn(&[TurnInput {
                content: "hello".into(),
                image_data_urls: Vec::new(),
                client_message_id: Some("client-initial".into()),
            }])
            .await
            .unwrap();

        let event = events.recv().await.unwrap();
        assert!(memory_recorded.load(Ordering::SeqCst));
        assert!(matches!(
            event.msg,
            agent_protocol::EventMsg::UserInputCommitted(
                agent_protocol::UserInputCommittedEvent {
                    turn_id,
                    client_message_id,
                }
            ) if turn_id == "turn-initial-ack" && client_message_id == "client-initial"
        ));
    }

    #[tokio::test]
    async fn initial_input_write_failure_does_not_emit_ack() {
        let dir = TempDir::new().unwrap();
        let config = crate::runtime::Config::with_defaults(dir.path().to_path_buf());
        let session = Session::with_session_id(config, "initial-input-no-ack".into())
            .await
            .unwrap();
        session.set_current_turn_id("turn-initial-no-ack").await;
        let events = session.subscribe_turn_events("turn-initial-no-ack").await;
        session.set_turn_input_after_db_write_hook(Some(Arc::new(|| {
            anyhow::bail!("injected post-DB failure")
        })));

        let error = session
            .prepare_turn(&[TurnInput {
                content: "hello".into(),
                image_data_urls: Vec::new(),
                client_message_id: Some("client-failed".into()),
            }])
            .await
            .unwrap_err();

        assert!(error.to_string().contains("injected post-DB failure"));
        assert!(events.try_recv().is_err());
    }

    #[tokio::test]
    async fn later_prompt_block_discards_staged_context_and_all_input() {
        let dir = TempDir::new().unwrap();
        let config = crate::runtime::Config::with_defaults(dir.path().to_path_buf());
        let session = Session::with_session_id(config, "atomic-admission".into())
            .await
            .unwrap();
        session
            .hook_bus()
            .register(::hooks::USER_PROMPT_SUBMIT, |input| {
                match input.prompt.as_deref() {
                    Some("first") => ::hooks::HookOutcome::InjectContext("staged".into()),
                    Some("second") => ::hooks::HookOutcome::Block("deny second".into()),
                    _ => ::hooks::HookOutcome::Continue,
                }
            });

        let error = session
            .prepare_turn(&[input("first"), input("second")])
            .await
            .unwrap_err();

        assert!(error.to_string().contains("deny second"));
        assert!(session.clone_history().await.is_empty());
        assert!(session.take_inject_context().await.is_none());
    }

    #[tokio::test]
    async fn session_start_block_retries_same_source() {
        let dir = TempDir::new().unwrap();
        let config = crate::runtime::Config::with_defaults(dir.path().to_path_buf());
        let session = Session::with_session_id(config, "retry-session-start".into())
            .await
            .unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let sources = Arc::new(std::sync::Mutex::new(Vec::new()));
        let hook_hits = Arc::clone(&hits);
        let hook_sources = Arc::clone(&sources);
        session
            .hook_bus()
            .register(::hooks::SESSION_START, move |input| {
                hook_sources
                    .lock()
                    .unwrap()
                    .push(input.source.clone().unwrap());
                if hook_hits.fetch_add(1, Ordering::SeqCst) == 0 {
                    ::hooks::HookOutcome::Block("retry".into())
                } else {
                    ::hooks::HookOutcome::Continue
                }
            });

        let first = session.prepare_turn(&[input("first")]).await.unwrap_err();
        assert!(first.to_string().contains("retry"));
        assert!(session.clone_history().await.is_empty());
        session.prepare_turn(&[input("second")]).await.unwrap();

        assert_eq!(hits.load(Ordering::SeqCst), 2);
        assert_eq!(sources.lock().unwrap().as_slice(), ["startup", "startup"]);
        assert_eq!(session.clone_history().await[0].content_str(), "second");
    }

    #[tokio::test]
    async fn subagent_start_is_context_injection_only() {
        let dir = TempDir::new().unwrap();
        let config = crate::runtime::Config::with_defaults(dir.path().to_path_buf());
        let session = Session::with_session_id(config, "subagent-start-context-only".into())
            .await
            .unwrap();
        session.set_subagent_hook_context(
            "thread-child".into(),
            "researcher".into(),
            "/root/child".into(),
        );
        session.hook_bus().register(::hooks::SUBAGENT_START, |_| {
            ::hooks::HookOutcome::Block("must not cancel child admission".into())
        });
        session.hook_bus().register(::hooks::SUBAGENT_START, |_| {
            ::hooks::HookOutcome::InjectContext("child startup context".into())
        });

        let context = session.admit_session_start().await.unwrap();
        assert_eq!(context.as_deref(), Some("child startup context"));
        session.prepare_turn(&[input("first")]).await.unwrap();

        assert_eq!(session.clone_history().await[0].content_str(), "first");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_session_start_admission_fires_once() {
        let dir = TempDir::new().unwrap();
        let config = crate::runtime::Config::with_defaults(dir.path().to_path_buf());
        let session = Arc::new(
            Session::with_session_id(config, "concurrent-session-start".into())
                .await
                .unwrap(),
        );
        let hits = Arc::new(AtomicUsize::new(0));
        let first_entered = Arc::new(tokio::sync::Notify::new());
        let second_entered = Arc::new(tokio::sync::Notify::new());
        let first_release = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let hook_hits = Arc::clone(&hits);
        let hook_entered = Arc::clone(&first_entered);
        let hook_second_entered = Arc::clone(&second_entered);
        let hook_release = Arc::clone(&first_release);
        session
            .hook_bus()
            .register(::hooks::SESSION_START, move |_| {
                if hook_hits.fetch_add(1, Ordering::SeqCst) == 0 {
                    hook_entered.notify_one();
                    let (released, ready) = &*hook_release;
                    let mut released = released.lock().unwrap();
                    while !*released {
                        released = ready.wait(released).unwrap();
                    }
                } else {
                    hook_second_entered.notify_one();
                }
                ::hooks::HookOutcome::Continue
            });

        let first = tokio::spawn({
            let session = Arc::clone(&session);
            async move { session.admit_session_start().await }
        });
        first_entered.notified().await;
        let second_started = Arc::new(tokio::sync::Notify::new());
        let second = tokio::spawn({
            let session = Arc::clone(&session);
            let second_started = Arc::clone(&second_started);
            async move {
                second_started.notify_one();
                session.admit_session_start().await
            }
        });
        second_started.notified().await;
        let second_fired_before_release = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            second_entered.notified(),
        )
        .await
        .is_ok();
        {
            let (released, ready) = &*first_release;
            *released.lock().unwrap() = true;
            ready.notify_all();
        }

        first.await.unwrap().unwrap();
        second.await.unwrap().unwrap();
        assert!(!second_fired_before_release);
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn admission_context_respects_system_prompt_budget() {
        let dir = TempDir::new().unwrap();
        let mut config = crate::runtime::Config::with_defaults(dir.path().to_path_buf());
        config.context_budget_chars = 64;
        let session = Session::with_session_id(config, "budgeted-admission".into())
            .await
            .unwrap();
        session
            .hook_bus()
            .register(::hooks::USER_PROMPT_SUBMIT, |_| {
                ::hooks::HookOutcome::InjectContext("X".repeat(1_000))
            });

        let result = session.prepare_turn(&[input("hello")]).await.unwrap();
        let TurnResult::Continue { system_prompt, .. } = result else {
            panic!("expected Continue");
        };

        assert!(system_prompt.chars().count() <= 64, "{system_prompt}");
    }

    #[tokio::test]
    async fn capture_step_context_reuses_the_turn_snapshot() {
        let dir = TempDir::new().unwrap();
        let config = crate::runtime::Config::with_defaults(dir.path().to_path_buf());
        let session = Session::with_session_id(config, "step-context-test".into())
            .await
            .unwrap();
        session
            .set_interaction_mode(types::InteractionMode::Plan)
            .await;
        session.set_current_turn_id("turn-1").await;
        let first = session.capture_step_context().await.unwrap();
        let second = session.capture_step_context().await.unwrap();

        assert!(Arc::ptr_eq(&first.turn, &second.turn));
        assert_eq!(first.turn.sub_id(), "turn-1");
        assert_eq!(first.turn.mode(), types::InteractionMode::Plan);
        assert_eq!(
            serde_json::to_value(&first.history).unwrap(),
            serde_json::to_value(&second.history).unwrap()
        );
        assert_eq!(
            first.tool_router.model_visible_specs().as_ref(),
            second.tool_router.model_visible_specs().as_ref()
        );
        assert!(!first.routes_tool("web_search"));
        assert!(!first.routes_tool("exec_command"));
    }

    #[test]
    fn deferred_routes_come_only_from_completed_tool_search_outputs() {
        let history = vec![
            agent_protocol::ResponseItem::ToolSearchOutput {
                id: None,
                call_id: Some("search-ok".into()),
                status: "completed".into(),
                execution: "client".into(),
                tools: vec![
                    serde_json::json!({"type": "function", "name": "web_search"}),
                    serde_json::json!({
                        "type": "function",
                        "function": {"name": "legacy_shape"}
                    }),
                ],
                internal_chat_message_metadata_passthrough: None,
            },
            agent_protocol::ResponseItem::ToolSearchOutput {
                id: None,
                call_id: Some("search-failed".into()),
                status: "failed".into(),
                execution: "client".into(),
                tools: vec![serde_json::json!({"name": "must_not_route"})],
                internal_chat_message_metadata_passthrough: None,
            },
        ];

        assert_eq!(
            discovered_deferred_tool_names(&history),
            HashSet::from(["legacy_shape", "web_search"])
        );
    }

    #[tokio::test]
    async fn tool_router_freezes_dynamic_handler_for_step() {
        let dir = TempDir::new().unwrap();
        let config = crate::runtime::Config::with_defaults(dir.path().to_path_buf());
        let session = Arc::new(
            Session::with_session_id(config, "tool-router-snapshot".into())
                .await
                .unwrap(),
        );
        let entry = || types::ToolEntry {
            name: "router_snapshot_probe".into(),
            toolset: "core".into(),
            description: "capture the handler advertised to one sampling step".into(),
            schema: serde_json::json!({"type": "object", "properties": {}}),
            check_fn: None,
            icon: "test-tube",
            ..types::ToolEntry::lifecycle_defaults()
        };
        session
            .services
            .tool_registry
            .write()
            .expect("tool registry lock poisoned")
            .register_dynamic(
                entry(),
                Arc::new(|_name, _args| Box::pin(async { Ok(types::ToolOutput::from("first")) })),
            );
        let step = session.capture_step_context().await.unwrap();

        session
            .services
            .tool_registry
            .write()
            .expect("tool registry lock poisoned")
            .register_dynamic(
                entry(),
                Arc::new(|_name, _args| Box::pin(async { Ok(types::ToolOutput::from("second")) })),
            );

        let runtime = crate::runtime::ToolCallRuntime::new(Arc::clone(&session), step);
        let output = runtime
            .handle_tool_call(
                types::ParsedToolCall::with_id(
                    "router-snapshot-call",
                    "router_snapshot_probe",
                    serde_json::json!({}),
                ),
                tokio_util::sync::CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(output.text(), "first");
    }
}
