//! 多轮循环中的上下文维护：LLM 前后的压缩/摘要、工具结果记录、hook 集成。

use std::sync::Arc;

use agent_protocol::{
    ContextUsageEvent, ContextUsageItem, ContextUsageSegment, DeltaEvent, EventMsg, ToolStatus,
};
use providers::types::message::Role as ProviderRole;

use super::lifecycle::{
    bounded_tool_completed_event, emit, emit_context_compacted, emit_extension_completed,
    emit_hook_completed, emit_hook_started, emit_prepared, emit_subagent_activity,
    is_subagent_tool,
};
use super::provider::ProviderStreamer;
use crate::runtime::{AgentLoop, TurnContext};

/// Gateway 预压安全网 + mid-run 辅模型摘要，统一进 LLM 前的上下文维护。
pub(super) async fn pre_llm_maintenance(
    session: &Arc<AgentLoop>,
    turn_context: &TurnContext,
) -> bool {
    let mut hook_stopped = false;
    {
        let agent = session.as_ref();
        let recommend_ratio = agent.compression_config().recommend_compact_ratio;
        if agent.occupancy_ratio().await >= recommend_ratio {
            match agent.maintain_tool_context().await {
                Ok(report) if report.hook_stopped => hook_stopped = true,
                Ok(report) if report.pruned + report.compressed > 0 => {
                    emit_context_compacted(
                        session,
                        turn_context,
                        format!(
                            "pruned={} compressed={} llm_summarized={}",
                            report.pruned, report.compressed, report.llm_summarized
                        ),
                    )
                    .await;
                    tracing::info!(
                        pruned = report.pruned,
                        compressed = report.compressed,
                        llm_summarized = report.llm_summarized,
                        occupancy_before = report.occupancy_before,
                        occupancy_after = report.occupancy_after,
                        recommend_ratio,
                        "gateway pre-maintain applied"
                    );
                }
                Ok(report) if report.recommend_session_compact => {
                    tracing::warn!(
                        occupancy = report.occupancy_after,
                        "gateway pre-maintain: still critical; recommend /compact"
                    );
                }
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "gateway pre-maintain failed"),
            }
        }
    }
    {
        let agent = session.as_ref();
        match crate::exec::mid_run_summary::maybe_apply_mid_run_summary(agent).await {
            Ok(true) => tracing::info!("mid-run summary applied before LLM round"),
            Ok(false) => {}
            Err(e) => tracing::warn!(error = %e, "mid-run summary failed"),
        }
    }
    hook_stopped
}

/// 构建并推送上下文占用估算快照。
pub(super) async fn emit_context_usage(
    session: &Arc<AgentLoop>,
    turn_context: &TurnContext,
    prompt: &crate::prompt::PromptContract,
    prompt_context: &[providers::types::message::Message],
    history: &[types::message::Message],
    tools: &[serde_json::Value],
) {
    let agent = session.as_ref();
    let mut layers = AgentLoop::prompt_contract_layer_breakdown(prompt);
    let actual_developer_chars = prompt_context
        .iter()
        .filter(|message| message.role() == ProviderRole::Developer)
        .map(|message| message.text_content().chars().count())
        .sum::<usize>();
    let actual_user_chars = prompt_context
        .iter()
        .filter(|message| message.role() == ProviderRole::User)
        .map(|message| message.text_content().chars().count())
        .sum::<usize>();
    let current_developer_chars =
        layers.developer_chars + layers.skills_chars + layers.mcp_instruction_chars;
    let current_user_chars = layers.user_context_chars + layers.memory_chars + layers.recall_chars;
    let developer_history_chars = actual_developer_chars.saturating_sub(current_developer_chars);
    let user_history_chars = actual_user_chars.saturating_sub(current_user_chars);
    if developer_history_chars > 0 {
        layers.developer_chars += developer_history_chars;
        layers.developer_items.push((
            "context_history".into(),
            "Developer context history".into(),
            developer_history_chars,
        ));
    }
    if user_history_chars > 0 {
        layers.user_context_chars += user_history_chars;
        layers.user_context_items.push((
            "context_history".into(),
            "User context history".into(),
            user_history_chars,
        ));
    }
    let recommend_compact_ratio = agent.compression_config().recommend_compact_ratio;
    let snap = crate::prompt::context_usage::build_snapshot(
        crate::prompt::context_usage::ContextUsageInput {
            system_chars: layers.system_chars,
            developer_chars: layers.developer_chars,
            user_context_chars: layers.user_context_chars,
            memory_chars: layers.memory_chars,
            skills_chars: layers.skills_chars,
            recall_chars: layers.recall_chars,
            mcp_instruction_chars: layers.mcp_instruction_chars,
            system_items: &layers.system_items,
            developer_items: &layers.developer_items,
            user_context_items: &layers.user_context_items,
            memory_items: &layers.memory_items,
            skill_items: &layers.skill_items,
            mcp_instruction_items: &layers.mcp_instruction_items,
            tools,
            messages: history,
            context_window: agent.context_window(),
            updated_at_ms: chrono::Utc::now().timestamp_millis(),
            recommend_compact: agent.should_recommend_compact().await,
            recommend_compact_ratio,
        },
    );
    emit(
        session,
        turn_context,
        EventMsg::ContextUsage(ContextUsageEvent {
            turn_id: turn_context.sub_id().to_string(),
            context_window: snap.context_window,
            total_tokens: snap.total_tokens,
            segments: snap
                .segments
                .into_iter()
                .map(|segment| ContextUsageSegment {
                    id: segment.id,
                    tokens: segment.tokens,
                    count: segment.meta.and_then(|meta| meta.count),
                    items: segment
                        .items
                        .into_iter()
                        .map(|item| ContextUsageItem {
                            id: item.id,
                            label: item.label,
                            tokens: item.tokens,
                        })
                        .collect(),
                })
                .collect(),
            updated_at: snap.updated_at,
            recommend_compact: snap.recommend_compact,
        }),
    )
    .await;
}

/// 工具执行后的上下文维护（压缩 + mid-run 摘要）+ stop_after 检查。
///
/// 返回 `true` 表示 `stop_after_tool_call` 触发，主循环应跳出。
pub(super) async fn post_tool_maintenance(
    session: &Arc<AgentLoop>,
    step_context: &crate::runtime::StepContext,
    turn_context: &TurnContext,
    calls: &[types::ParsedToolCall],
) -> bool {
    let mut hook_stopped = false;
    {
        let agent = session.as_ref();
        match agent.maintain_tool_context().await {
            Ok(report) if report.hook_stopped => hook_stopped = true,
            Ok(report) if report.pruned + report.compressed > 0 => {
                emit_context_compacted(
                    session,
                    turn_context,
                    format!(
                        "pruned={} compressed={} llm_summarized={}",
                        report.pruned, report.compressed, report.llm_summarized
                    ),
                )
                .await;
                tracing::info!(
                    pruned = report.pruned,
                    compressed = report.compressed,
                    llm_summarized = report.llm_summarized,
                    occupancy_before = report.occupancy_before,
                    occupancy_after = report.occupancy_after,
                    stage_ratio = ?report.stage_ratio,
                    "tool context maintenance applied"
                );
            }
            Ok(report) if report.recommend_session_compact => {
                tracing::warn!(
                    occupancy = report.occupancy_after,
                    "context still critical after tool maintenance; recommend /compact"
                );
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "tool context maintenance failed"),
        }
        match crate::exec::mid_run_summary::maybe_apply_mid_run_summary(agent).await {
            Ok(true) => tracing::info!("mid-run summary applied after tool maintenance"),
            Ok(false) => {}
            Err(e) => tracing::warn!(error = %e, "mid-run summary failed"),
        }
    }

    let names: Vec<&str> = calls.iter().map(|c| c.name.as_str()).collect();
    let stop_after = step_context.tool_router.any_stop_after(&names);
    if stop_after {
        tracing::info!(
            ?names,
            "stop_after_tool_call: ending run without next LLM round"
        );
    }
    stop_after || hook_stopped
}

/// 处理工具执行结果：推送事件、解析 A2UI、记录到会话历史。
///
/// 返回 `false` 表示取消或 channel 关闭，主循环应提前退出。
pub(super) async fn record_tool_outcomes(
    session: &Arc<AgentLoop>,
    calls: &[types::ParsedToolCall],
    outcomes: Vec<types::ToolOutput>,
    pause: &Arc<providers::PauseControl>,
    turn_context: &TurnContext,
    timeline: &mut crate::timeline::TimelineBuilder,
    now_ms: impl Fn() -> i64,
) -> bool {
    for (call, result) in calls.iter().zip(outcomes) {
        if pause.is_cancelled() {
            return false;
        }

        let tool_media = result.media().to_vec();
        let result_text = result.text().to_string();
        let is_success = !result_text.starts_with("工具错误")
            && !result_text.starts_with("工具已禁用")
            && !result_text.starts_with("工具参数 JSON 解析失败");

        let info_ui = parse_astro_ui(&result_text);
        let result_for_history = if let Some(ref ui) = info_ui {
            format!("Presented info card: {}", ui.summary)
        } else {
            result_text.clone()
        };

        if let Some(ref ui) = info_ui {
            let message_id = format!("a2ui-surface-{}", call.id);
            timeline.upsert_surface(
                serde_json::json!({
                    "messageId": message_id,
                    "activityType": "a2ui-surface",
                    "operations": ui.operations,
                    "status": "active",
                }),
                now_ms(),
            );
        }

        let recorded = {
            let agent = session.as_ref();
            let recorded = agent
                .record_tool_result_with_id(Some(&call.id), Some(&call.name), &result_for_history)
                .await;
            if !tool_media.is_empty() {
                let mut state = agent.state.lock().expect("session state mutex poisoned");
                if let Some(last) = state.history.last_mut() {
                    if last.role == types::message::Role::Tool && last.media.is_empty() {
                        last.media = tool_media.clone();
                    }
                }
            }
            recorded
        };
        if let Err(error) = recorded {
            tracing::warn!(%error, tool_call_id = %call.id, "failed to record tool result");
            return false;
        }

        if matches!(call.name.as_str(), "terminal" | "code_exec") && !result_text.is_empty() {
            emit(
                session,
                turn_context,
                EventMsg::ExecCommandOutputDelta(DeltaEvent {
                    turn_id: turn_context.sub_id().to_string(),
                    item_id: call.id.clone(),
                    delta: result_text.clone(),
                }),
            )
            .await;
        }

        emit_prepared(
            session,
            turn_context,
            bounded_tool_completed_event(
                turn_context.sub_id(),
                &call.id,
                &call.name,
                call.arguments.clone(),
                Some(serde_json::Value::String(result_text.clone())),
                tool_media,
                if is_success {
                    ToolStatus::Completed
                } else {
                    ToolStatus::Failed
                },
            ),
        )
        .await;

        if call.name == "memory" && is_success {
            let s = result_text.trim();
            let preview = if s.chars().count() > 240 {
                format!("{}…", s.chars().take(240).collect::<String>())
            } else {
                s.to_string()
            };
            let target = call
                .arguments
                .get("target")
                .and_then(serde_json::Value::as_str)
                .filter(|target| matches!(*target, "memory" | "user" | "mixed"))
                .unwrap_or("mixed");
            emit_extension_completed(
                session,
                turn_context,
                format!("memory-{}", call.id),
                "astro.memory",
                serde_json::json!({
                    "source": "tool",
                    "target": target,
                    "summary": preview,
                    "live_written": !(result_text.contains("待审批")
                        || result_text.contains("pending")
                        || result_text.contains("入队")),
                }),
            )
            .await;
        }

        if is_subagent_tool(&call.name) {
            emit_subagent_activity(
                session,
                turn_context,
                format!("subagent-{}", call.id),
                result_text.clone(),
            )
            .await;
        }

        if let Some(ui) = info_ui {
            emit_extension_completed(
                session,
                turn_context,
                format!("a2ui-{}", call.id),
                "astro.a2ui",
                serde_json::json!({
                    "operations": ui.operations,
                    "summary": ui.summary,
                    "replace": true,
                }),
            )
            .await;
        }
    }

    // 工具循环后回写 timeline/surfaces，避免历史恢复丢 A2UI 卡片。
    {
        let agent = session.as_ref();
        if let Err(e) = agent.patch_last_assistant_timeline(timeline.reasoning_details_snapshot()) {
            tracing::warn!(error = %e, "patch assistant timeline after tools failed");
        }
    }
    true
}

/// 触发 PRE/POST_API_REQUEST hook 并发起 LLM 流式请求。
///
/// 成功返回 `Ok(stream)`；失败返回 `Err(error_string)` 并已在 hook 中记录。
pub(super) async fn run_sampling_request(
    session: &Arc<AgentLoop>,
    turn_context: &TurnContext,
    streamer: &ProviderStreamer,
    prompt: &crate::prompt::PromptContract,
    prompt_context: &[providers::types::message::Message],
    history: &[types::message::Message],
    tool_specs: Vec<serde_json::Value>,
) -> Result<super::types::AssistantContentStream, String> {
    {
        let agent = session.as_ref();
        let sid = agent.session_id().to_string();
        let turn_id = agent.current_turn_id().await;
        let hook_item = emit_hook_started(session, turn_context, ::hooks::PRE_API_REQUEST).await;
        let _ = agent.fire_hook(
            ::hooks::PRE_API_REQUEST,
            ::hooks::HookPayload {
                session_id: sid,
                turn_id,
                ..Default::default()
            },
        );
        emit_hook_completed(session, turn_context, hook_item, ::hooks::PRE_API_REQUEST).await;
    }
    match streamer
        .stream_chat_with_contract(prompt, prompt_context, history, tool_specs)
        .await
    {
        Ok(s) => {
            let agent = session.as_ref();
            let sid = agent.session_id().to_string();
            let turn_id = agent.current_turn_id().await;
            let hook_item =
                emit_hook_started(session, turn_context, ::hooks::POST_API_REQUEST).await;
            let _ = agent.fire_hook(
                ::hooks::POST_API_REQUEST,
                ::hooks::HookPayload {
                    session_id: sid,
                    turn_id,
                    ..Default::default()
                },
            );
            emit_hook_completed(session, turn_context, hook_item, ::hooks::POST_API_REQUEST).await;
            Ok(s)
        }
        Err(err) => {
            let agent = session.as_ref();
            let sid = agent.session_id().to_string();
            let turn_id = agent.current_turn_id().await;
            let hook_item =
                emit_hook_started(session, turn_context, ::hooks::POST_API_REQUEST).await;
            let _ = agent.fire_hook(
                ::hooks::POST_API_REQUEST,
                ::hooks::HookPayload {
                    session_id: sid,
                    turn_id,
                    error: Some(err.to_string()),
                    detail: format!("error={err}"),
                    ..Default::default()
                },
            );
            emit_hook_completed(session, turn_context, hook_item, ::hooks::POST_API_REQUEST).await;
            Err(err.to_string())
        }
    }
}

struct AstroUiPayload {
    summary: String,
    operations: serde_json::Value,
}

fn parse_astro_ui(result: &str) -> Option<AstroUiPayload> {
    let value: serde_json::Value = serde_json::from_str(result).ok()?;
    if value.get("astro_ui")?.as_bool() != Some(true) {
        return None;
    }
    if value.get("astro_hitl").and_then(|v| v.as_bool()) == Some(true) {
        return None;
    }
    let operations = value.get("operations")?.clone();
    if !operations.is_array() {
        return None;
    }
    Some(AstroUiPayload {
        summary: value
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("info")
            .to_string(),
        operations,
    })
}
