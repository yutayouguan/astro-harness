//! AgentLoop 上下文维护：tool 结果压缩、provider 历史折叠与上下文占用估算。

use session::ConversationStore;
use types::message::{Message, Role};

use crate::compression::{prune_tool_view, ContextMaintenanceResult, ToolCompressionManager};

use super::AgentLoop;

impl AgentLoop {
    /// Provider 发送用历史：若有 mid-run handoff 则折叠中间轮次。
    pub async fn provider_history(&self) -> Vec<Message> {
        let (handoff, history) = {
            let state = self.lock_state();
            (
                state.compression.mid_run_handoff.clone(),
                state.clone_history(),
            )
        };
        match handoff {
            Some(handoff) => crate::exec::mid_run_summary::collapse_history_with_handoff(
                &history,
                &handoff,
                self.config_protect_first_n(),
                self.config_protect_last_n(),
            ),
            None => history,
        }
    }

    /// 当前会话占用比例（ceil chars/4 ÷ context_window）。
    pub async fn occupancy_ratio(&self) -> f32 {
        let history = self.clone_history().await;
        ToolCompressionManager::from_config(&self.compression_config())
            .with_context_window(self.context_window())
            .occupancy_ratio(&history)
    }

    /// Run 内 tool 上下文维护：委托 [`CompressionPolicy`] 生成计划，执行 prune/LLM 摘要/head-tail。
    ///
    /// 不变量：`content` 全文保留；仅改 `compressed_content`（Provider 视图）。
    pub async fn maintain_tool_context(&self) -> anyhow::Result<ContextMaintenanceResult> {
        let mut result = ContextMaintenanceResult::default();
        if !self.compression_config().enabled {
            return Ok(result);
        }
        let history = {
            let state = self.lock_state();
            if !state.compression.guard.allow_run() {
                result.thrashing_disabled = true;
                return Ok(result);
            }
            state.clone_history()
        };

        let stored = self
            .services
            .sessions
            .get_messages(&self.session_id)
            .await?;
        let protect_last_n = self.compression_config().protect_last_n.max(1);

        let plan = self
            .services
            .compression_policy
            .lock()
            .expect("compression policy mutex poisoned")
            .plan(
                &stored,
                &history,
                self.memory_dir(),
                &self.session_id,
                protect_last_n,
            );

        if plan.prune.is_empty() && plan.compress.is_empty() {
            return Ok(result);
        }

        let pre = self.fire_hook(
            ::hooks::PRE_COMPACT,
            ::hooks::HookPayload {
                turn_id: self.current_turn_id().await,
                trigger: Some("auto".into()),
                detail: format!(
                    "prune={} compress={}",
                    plan.prune.len(),
                    plan.compress.len()
                ),
                ..Default::default()
            },
        );
        if matches!(
            pre,
            ::hooks::HookOutcome::Block(_) | ::hooks::HookOutcome::Skip(_)
        ) {
            result.hook_stopped = true;
            return Ok(result);
        }

        result.stage_ratio = plan.stage_ratio;
        result.occupancy_before = plan.occupancy_before;

        // ── Prune 阶段（不含 await，复用已有 stored 快照） ──
        for target in &plan.prune {
            let view = prune_tool_view(target.tool_name.as_deref(), target.spill_rel.as_deref());
            let Some(stored_msg) = stored.iter().find(|m| m.id == target.message_id) else {
                continue;
            };
            let content = stored_msg.content.as_deref().unwrap_or_default();
            self.apply_tool_compressed_view(stored_msg, content, &view)
                .await?;
            result.pruned += 1;
        }

        // ── Compress 阶段：先尝试 LLM 摘要，失败回退 head/tail ──
        let targets = self.auxiliary_targets(types::AuxiliaryTask::Compaction);
        let llm_budget = crate::exec::tool_llm_compress::MAX_LLM_TOOL_COMPRESS_PER_PASS;

        for (i, job) in plan.compress.iter().enumerate() {
            let view = if i < llm_budget && !targets.is_empty() {
                match crate::exec::tool_llm_compress::summarize_tool_result(
                    &targets,
                    job.tool_name.as_deref(),
                    &job.content,
                    job.max_chars,
                )
                .await
                {
                    Ok(v) => {
                        result.llm_summarized += 1;
                        v
                    }
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            tool = ?job.tool_name,
                            "tool LLM compress failed; falling back to head/tail"
                        );
                        self.services
                            .compression_policy
                            .lock()
                            .expect("compression policy mutex poisoned")
                            .compress_fallback(job.tool_name.as_deref(), &job.content, job)
                            .unwrap_or_else(|| job.content.clone())
                    }
                }
            } else {
                self.services
                    .compression_policy
                    .lock()
                    .expect("compression policy mutex poisoned")
                    .compress_fallback(job.tool_name.as_deref(), &job.content, job)
                    .unwrap_or_else(|| job.content.clone())
            };

            let stored_again = self
                .services
                .sessions
                .get_messages(&self.session_id)
                .await?;
            let Some(stored_msg) = stored_again.iter().find(|m| m.id == job.message_id) else {
                continue;
            };
            self.apply_tool_compressed_view(stored_msg, &job.content, &view)
                .await?;
            result.compressed += 1;
        }

        // ── 防抖 + compact 建议 ──
        let mgr = ToolCompressionManager::from_config(&self.compression_config())
            .with_context_window(self.context_window());
        result.occupancy_after = mgr.occupancy_ratio(&self.clone_history().await);
        let recommend_session_compact = self
            .services
            .compression_policy
            .lock()
            .expect("compression policy mutex poisoned")
            .should_recommend_compact(result.occupancy_after);
        {
            let mut state = self.lock_state();
            state
                .compression
                .guard
                .record_outcome(result.occupancy_before, result.occupancy_after);
            result.thrashing_disabled = state.compression.guard.disabled;
            result.recommend_session_compact = recommend_session_compact;
            if result.recommend_session_compact {
                state.compression.pending_recommend_compact = true;
            }
        }
        let post = self.fire_hook(
            ::hooks::POST_COMPACT,
            ::hooks::HookPayload {
                turn_id: self.current_turn_id().await,
                trigger: Some("auto".into()),
                detail: format!(
                    "pruned={} compressed={} llm_summarized={}",
                    result.pruned, result.compressed, result.llm_summarized
                ),
                ..Default::default()
            },
        );
        result.hook_stopped = matches!(
            post,
            ::hooks::HookOutcome::Block(_) | ::hooks::HookOutcome::Skip(_)
        );
        Ok(result)
    }

    async fn apply_tool_compressed_view(
        &self,
        stored_msg: &::session::StoredMessage,
        content: &str,
        view: &str,
    ) -> anyhow::Result<()> {
        self.services
            .sessions
            .update_message_compressed_content(stored_msg.id, Some(view))
            .await?;
        let mut state = self.lock_state();
        if let Some(runtime_msg) = state.history.iter_mut().find(|m| {
            m.role == Role::Tool
                && match (&m.tool_call_id, &stored_msg.tool_call_id) {
                    (Some(a), Some(b)) => a == b,
                    (None, None) => m.content_str() == content,
                    _ => false,
                }
        }) {
            runtime_msg.compressed_content = Some(view.to_string());
        }
        Ok(())
    }

    /// 压缩本 run 中尚未压缩的 tool 结果（兼容旧调用；委托 [`Self::maintain_tool_context`]）。
    pub async fn compress_tool_results_if_needed(&self) -> anyhow::Result<usize> {
        let report = self.maintain_tool_context().await?;
        Ok(report.pruned + report.compressed)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[tokio::test]
    async fn automatic_maintenance_fires_pre_and_post_compact() {
        let dir = tempfile::tempdir().unwrap();
        let mut compression = memory::CompressionConfig::default();
        compression.soft_ratio = 0.0;
        compression.medium_ratio = 0.5;
        compression.hard_ratio = 0.9;
        compression.soft_max_chars = 12;
        compression.soft_head_chars = 5;
        compression.soft_tail_chars = 5;
        compression.protect_last_n = 1;
        compression.protect_first_messages = 0;
        memory::set_compression_config(dir.path(), &compression).unwrap();
        let session = AgentLoop::new(super::super::Config::with_defaults(
            dir.path().to_path_buf(),
        ))
        .await.unwrap();
        session.record_user_message("run tool").await.unwrap();
        session
            .record_assistant_message_with_tools(
                "",
                Some(vec![types::message::ToolCall {
                    id: "call-1".into(),
                    name: "echo".into(),
                    arguments: serde_json::json!({}),
                    signature: None,
                }]),
                None,
                None,
            )
            .await
            .unwrap();
        session
            .record_tool_result_with_id(
                Some("call-1"),
                Some("echo"),
                "a tool result that is intentionally longer than the configured soft limit",
            )
            .await
            .unwrap();
        session.record_assistant_message("done").await.unwrap();

        let events = Arc::new(Mutex::new(Vec::new()));
        for event in [::hooks::PRE_COMPACT, ::hooks::POST_COMPACT] {
            let captured = Arc::clone(&events);
            session.hook_bus().register(event, move |payload| {
                captured
                    .lock()
                    .unwrap()
                    .push((payload.hook_event_name.clone(), payload.trigger.clone()));
                ::hooks::HookOutcome::Continue
            });
        }

        let report = session.maintain_tool_context().await.unwrap();

        assert!(report.pruned + report.compressed > 0);
        assert_eq!(
            *events.lock().unwrap(),
            vec![
                (::hooks::PRE_COMPACT.into(), Some("auto".into())),
                (::hooks::POST_COMPACT.into(), Some("auto".into())),
            ]
        );
    }
}
