//! Session turn lifecycle: input persistence, memory recall, prompt assembly, and hooks.

use session::{build_conversation_context, format_recalled_context, ConversationStore, NewMessage};
use types::message::Message;

use std::sync::Arc;

use crate::tasks::{TaskKind, TurnInput};

use super::turn_context::QueuedTurnInput;
use super::{looks_like_user_correction, Session, StepContext, TurnContext, TurnResult};

fn coalesce_turn_inputs<I>(inputs: I) -> Option<TurnInput>
where
    I: IntoIterator<Item = TurnInput>,
{
    let mut contents = Vec::new();
    let mut image_data_urls = Vec::new();
    for input in inputs {
        let TurnInput::UserInput {
            content,
            image_data_urls: images,
        } = input;
        contents.push(content);
        image_data_urls.extend(images);
    }
    (!contents.is_empty()).then(|| TurnInput::UserInput {
        content: contents.join("\n\n"),
        image_data_urls,
    })
}

impl Session {
    /// 开始新的用户消息处理：重置 `tool_rounds` 与 `turn_wrote_disk`。
    ///
    /// 若上一轮工具次数达到 `learning.complex_task_tool_threshold`，为本轮挂起学习 nudge。
    pub async fn begin_user_turn(&self) {
        let memory_dir = self.memory_dir().to_path_buf();
        let compression = memory::load_compression_config(&memory_dir);
        {
            let mut state = self.state.lock().await;
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
    pub(crate) async fn prepare_turn(&self, input: &[TurnInput]) -> anyhow::Result<TurnResult> {
        anyhow::ensure!(!input.is_empty(), "regular turn requires initial input");
        let coalesced_input =
            coalesce_turn_inputs(input.iter().cloned()).expect("non-empty input coalesces");
        let TurnInput::UserInput {
            content: user_message,
            ..
        } = &coalesced_input;
        let user_message = user_message.clone();
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
                .any(|m| matches!(m.role, types::message::Role::Assistant))
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

        let current_turn = self.state.lock().await.turn.current_turn;
        let fts_keywords = if current_turn >= self.config.recent_turns {
            Some(user_message.as_str())
        } else {
            None
        };
        let recalled = build_conversation_context(
            &self.services.sessions,
            &self.session_id,
            self.config.recent_turns,
            fts_keywords,
        )?;
        self.state.lock().await.compression.last_recalled_context =
            format_recalled_context(&recalled);

        self.increment_turn().await;
        let system_prompt = self
            .build_system_prompt_with_inject(admission_context.as_deref())
            .await;
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
            self.state.lock().await.pending_inject_context = Some(ctx);
        }
        if self.cancel.is_cancelled() {
            return Ok(TurnResult::Interrupted);
        }
        Ok(TurnResult::Continue {
            turn: self.session_turn().await,
            system_prompt,
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
        outcome: ::hooks::HookOutcome,
    ) -> anyhow::Result<Option<String>> {
        match outcome {
            ::hooks::HookOutcome::Block(reason) => {
                anyhow::bail!("{event_name} blocked by hook: {reason}")
            }
            ::hooks::HookOutcome::InjectContext(context) => Ok(Some(context)),
            _ => Ok(None),
        }
    }

    async fn admit_initial_input(
        &self,
        input: &[TurnInput],
        turn_id: Option<String>,
    ) -> anyhow::Result<Option<String>> {
        let _admission_guard = self.admission_lock.lock().await;
        let mut contexts = Vec::new();
        if let Some(context) = self.admit_session_start_locked().await? {
            contexts.push(context);
        }
        for item in input {
            let TurnInput::UserInput { content, .. } = item;
            if let Some(context) = self.admit_user_prompt(content, turn_id.clone())? {
                contexts.push(context);
            }
        }
        Ok((!contexts.is_empty()).then(|| contexts.join("\n\n")))
    }

    #[cfg(test)]
    async fn admit_session_start(&self) -> anyhow::Result<Option<String>> {
        let _admission_guard = self.admission_lock.lock().await;
        self.admit_session_start_locked().await
    }

    async fn admit_session_start_locked(&self) -> anyhow::Result<Option<String>> {
        let Some(source) = self.state.lock().await.pending_session_start_source.clone() else {
            return Ok(None);
        };
        let outcome = self.fire_hook(
            ::hooks::SESSION_START,
            ::hooks::HookPayload {
                source: Some(source.clone()),
                detail: format!("session={}", self.session_id),
                ..Default::default()
            },
        );
        let context = Self::apply_admission_outcome(::hooks::SESSION_START, outcome)?;
        let mut state = self.state.lock().await;
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
        let outcome = self.fire_hook(
            ::hooks::USER_PROMPT_SUBMIT,
            ::hooks::HookPayload {
                turn_id,
                prompt: Some(prompt.to_string()),
                detail: prompt.chars().take(200).collect(),
                ..Default::default()
            },
        );
        Self::apply_admission_outcome(::hooks::USER_PROMPT_SUBMIT, outcome)
    }

    /// Queue user input for the active regular task.
    pub async fn steer_input(
        &self,
        user_message: &str,
        image_data_urls: &[String],
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
        let _admission_guard = self.admission_lock.lock().await;
        let turn_id = running.1.sub_id().to_string();
        let Some(reservation) = running.1.reserve_input() else {
            return Ok(None);
        };
        let context = self.admit_user_prompt(user_message, Some(turn_id.clone()))?;
        let input = TurnInput::UserInput {
            content: user_message.to_string(),
            image_data_urls: image_data_urls.to_vec(),
        };
        reservation.commit(input, context);
        Ok(Some(turn_id))
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
        if !contexts.is_empty() {
            let mut state = self.state.lock().await;
            for context in contexts {
                Self::append_inject_context(&mut state.pending_inject_context, context);
            }
        }
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
        let TurnInput::UserInput {
            content,
            image_data_urls,
        } = input;
        self.services
            .sessions
            .ensure_session(&self.session_id, "tauri")?;
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
        self.services.sessions.append_message(NewMessage {
            content: Some(&content),
            media_json: media_json.as_deref(),
            ..NewMessage::empty(&self.session_id, "user")
        })?;
        self.record_items_unlocked(vec![Message::user_with_images(&content, &image_data_urls)])
            .await;
        Ok(())
    }

    /// Compatibility adapter for callers not yet migrated to Codex naming.
    #[deprecated(note = "use start_or_steer_turn")]
    pub async fn run_turn(
        &self,
        user_message: &str,
        submission_id: &str,
    ) -> anyhow::Result<TurnResult> {
        self.start_or_steer_turn(user_message, submission_id).await
    }

    /// Compatibility adapter for callers not yet migrated to Codex naming.
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
        let mut history = self.provider_history().await;
        if let Some(ctx) = self.take_inject_context().await {
            history.push(Message::user(&format!("[astro:hook-context]\n{ctx}")));
        }
        let tool_router = self.build_tool_router().await;
        let session_configuration = self.session_configuration().clone();
        let turn_context = {
            let state = self.state.lock().await;
            state.current_turn_context.clone().unwrap_or_else(|| {
                Arc::new(TurnContext::new(
                    state
                        .turn
                        .current_turn_id()
                        .map(str::to_owned)
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                    state.turn.current_turn(),
                    state.interaction_mode,
                    session_configuration.permission_profile.clone(),
                    session_configuration.project_root.clone(),
                ))
            })
        };
        let step_context = Arc::new(StepContext::new(turn_context, history, tool_router));
        self.state.lock().await.current_step_context = Some(Arc::clone(&step_context));
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
        TurnInput::UserInput {
            content: content.to_string(),
            image_data_urls: Vec::new(),
        }
    }

    #[test]
    fn coalesce_turn_inputs_preserves_text_and_image_order() {
        let coalesced = coalesce_turn_inputs(vec![
            TurnInput::UserInput {
                content: "first".into(),
                image_data_urls: vec!["image-a".into()],
            },
            TurnInput::UserInput {
                content: String::new(),
                image_data_urls: vec!["image-b".into(), "image-c".into()],
            },
            input("third"),
        ])
        .unwrap();

        assert_eq!(
            coalesced,
            TurnInput::UserInput {
                content: "first\n\n\n\nthird".into(),
                image_data_urls: vec!["image-a".into(), "image-b".into(), "image-c".into()],
            }
        );
    }

    #[tokio::test]
    async fn initial_inputs_are_persisted_as_one_logical_user_message() {
        let dir = TempDir::new().unwrap();
        let config = crate::runtime::Config::with_defaults(dir.path().to_path_buf());
        let session = Session::with_session_id(config, "coalesced-initial".into()).unwrap();

        session
            .prepare_turn(&[input("first"), input("second")])
            .await
            .unwrap();

        let history = session.clone_history().await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content_str(), "first\n\nsecond");
    }

    #[tokio::test]
    async fn later_prompt_block_discards_staged_context_and_all_input() {
        let dir = TempDir::new().unwrap();
        let config = crate::runtime::Config::with_defaults(dir.path().to_path_buf());
        let session = Session::with_session_id(config, "atomic-admission".into()).unwrap();
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
        let session = Session::with_session_id(config, "retry-session-start".into()).unwrap();
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_session_start_admission_fires_once() {
        let dir = TempDir::new().unwrap();
        let config = crate::runtime::Config::with_defaults(dir.path().to_path_buf());
        let session =
            Arc::new(Session::with_session_id(config, "concurrent-session-start".into()).unwrap());
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
        let session = Session::with_session_id(config, "budgeted-admission".into()).unwrap();
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
        let session = Session::with_session_id(config, "step-context-test".into()).unwrap();
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
        assert!(first.advertises_tool("file_ops"));
        assert!(!first.advertises_tool("terminal"));
    }

    #[tokio::test]
    async fn tool_router_freezes_dynamic_handler_for_step() {
        let dir = TempDir::new().unwrap();
        let config = crate::runtime::Config::with_defaults(dir.path().to_path_buf());
        let session =
            Arc::new(Session::with_session_id(config, "tool-router-snapshot".into()).unwrap());
        let entry = || types::ToolEntry {
            name: "router_snapshot_probe".into(),
            toolset: "core".into(),
            description: "capture the handler advertised to one sampling step".into(),
            schema: serde_json::json!({"type": "object", "properties": {}}),
            check_fn: None,
            icon: "test-tube",
            ..types::ToolEntry::lifecycle_defaults()
        };
        session.tool_registry_mut().register_dynamic(
            entry(),
            Arc::new(|_name, _args| Box::pin(async { Ok(types::ToolOutput::from("first")) })),
        );
        let step = session.capture_step_context().await.unwrap();

        session.tool_registry_mut().register_dynamic(
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
