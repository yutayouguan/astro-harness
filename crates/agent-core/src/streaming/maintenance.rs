//! 多轮循环中的上下文维护：LLM 前后的压缩/摘要、工具结果记录、hook 集成。

use std::sync::Arc;

use tokio::sync::{mpsc, Mutex};

use super::lifecycle::emit;
use super::provider::ProviderStreamer;
use super::traits::StreamingChat;
use super::types::MultiTurnStreamItem;
use crate::runtime::AgentLoop;

/// Gateway 预压安全网 + mid-run 辅模型摘要，统一进 LLM 前的上下文维护。
pub(super) async fn pre_llm_maintenance(session: &Arc<Mutex<AgentLoop>>) {
    {
        let mut agent = session.lock().await;
        let recommend_ratio = agent.compression_config().recommend_compact_ratio;
        if agent.occupancy_ratio() >= recommend_ratio {
            match agent.maintain_tool_context().await {
                Ok(report) if report.pruned + report.compressed > 0 => {
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
        let mut agent = session.lock().await;
        match crate::exec::mid_run_summary::maybe_apply_mid_run_summary(&mut agent).await {
            Ok(true) => tracing::info!("mid-run summary applied before LLM round"),
            Ok(false) => {}
            Err(e) => tracing::warn!(error = %e, "mid-run summary failed"),
        }
    }
}

/// 构建并推送上下文占用估算快照。
pub(super) async fn emit_context_usage(
    session: &Arc<Mutex<AgentLoop>>,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    history: &[types::message::Message],
    tools: &[serde_json::Value],
) {
    let agent = session.lock().await;
    let layers = agent.system_prompt_layer_breakdown();
    let recommend_compact_ratio = agent.compression_config().recommend_compact_ratio;
    let snap = crate::prompt::context_usage::build_snapshot(
        crate::prompt::context_usage::ContextUsageInput {
            system_chars: layers.system_chars,
            memory_chars: layers.memory_chars,
            skills_chars: layers.skills_chars,
            recall_chars: layers.recall_chars,
            system_items: &layers.system_items,
            memory_items: &layers.memory_items,
            skill_items: &layers.skill_items,
            tools,
            messages: history,
            context_window: agent.context_window(),
            updated_at_ms: chrono::Utc::now().timestamp_millis(),
            recommend_compact: agent.should_recommend_compact(),
            recommend_compact_ratio,
        },
    );
    drop(agent);
    let _ = emit(tx, MultiTurnStreamItem::ContextUsage(snap)).await;
}

/// 工具执行后的上下文维护（压缩 + mid-run 摘要）+ stop_after 检查。
///
/// 返回 `true` 表示 `stop_after_tool_call` 触发，主循环应跳出。
pub(super) async fn post_tool_maintenance(
    session: &Arc<Mutex<AgentLoop>>,
    calls: &[types::ParsedToolCall],
) -> bool {
    {
        let mut agent = session.lock().await;
        match agent.maintain_tool_context().await {
            Ok(report) if report.pruned + report.compressed > 0 => {
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
        match crate::exec::mid_run_summary::maybe_apply_mid_run_summary(&mut agent).await {
            Ok(true) => tracing::info!("mid-run summary applied after tool maintenance"),
            Ok(false) => {}
            Err(e) => tracing::warn!(error = %e, "mid-run summary failed"),
        }
    }

    let names: Vec<&str> = calls.iter().map(|c| c.name.as_str()).collect();
    let stop_after = {
        let agent = session.lock().await;
        agent.tool_registry().any_stop_after(&names)
    };
    if stop_after {
        tracing::info!(
            ?names,
            "stop_after_tool_call: ending run without next LLM round"
        );
    }
    stop_after
}

/// 处理工具执行结果：推送事件、解析 A2UI、记录到会话历史。
///
/// 返回 `false` 表示取消或 channel 关闭，主循环应提前退出。
pub(super) async fn record_tool_outcomes(
    session: &Arc<Mutex<AgentLoop>>,
    calls: &[types::ParsedToolCall],
    outcomes: Vec<types::ToolOutput>,
    pause: &Arc<providers::PauseControl>,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    timeline: &mut crate::timeline::TimelineBuilder,
    now_ms: impl Fn() -> i64,
) -> bool {
    for (call, result) in calls.iter().zip(outcomes) {
        if pause.is_cancelled() {
            return false;
        }

        let tool_media = result.media().to_vec();
        let result_text = result.text().to_string();
        if !emit(
            tx,
            MultiTurnStreamItem::ToolResult {
                id: call.id.clone(),
                name: call.name.clone(),
                arguments_json: call.arguments.to_string(),
                result: result_text.clone(),
                media: tool_media.clone(),
            },
        )
        .await
        {
            return false;
        }

        if matches!(call.name.as_str(), "memory")
            && !result_text.starts_with("工具错误")
            && !result_text.starts_with("工具已禁用")
            && !result_text.starts_with("工具参数 JSON 解析失败")
        {
            let preview = {
                let s = result_text.trim();
                if s.chars().count() > 240 {
                    format!("{}…", s.chars().take(240).collect::<String>())
                } else {
                    s.to_string()
                }
            };
            if !emit(
                tx,
                MultiTurnStreamItem::MemoryUpdate {
                    op: call.name.clone(),
                    content: preview,
                },
            )
            .await
            {
                return false;
            }
        }

        let info_ui = parse_astro_ui(&result_text);
        let result_for_history = if let Some(ref ui) = info_ui {
            format!("Presented info card: {}", ui.summary)
        } else {
            result_text.clone()
        };

        if let Some(ref ui) = info_ui {
            let message_id = format!("a2ui-surface-{}", call.id);
            let content_json = serde_json::json!({ "operations": ui.operations }).to_string();
            timeline.upsert_surface(
                serde_json::json!({
                    "messageId": message_id,
                    "activityType": "a2ui-surface",
                    "operations": ui.operations,
                    "status": "active",
                }),
                now_ms(),
            );
            if !emit(
                tx,
                MultiTurnStreamItem::Activity {
                    message_id,
                    activity_type: "a2ui-surface".into(),
                    content_json,
                    replace: true,
                },
            )
            .await
            {
                return false;
            }
        }

        {
            let mut agent = session.lock().await;
            let _ = agent.record_tool_result_with_id(
                Some(&call.id),
                Some(&call.name),
                &result_for_history,
            );
            if !tool_media.is_empty() {
                if let Some(last) = agent.session_messages.last_mut() {
                    if last.role == types::message::Role::Tool && last.media.is_empty() {
                        last.media = tool_media;
                    }
                }
            }
        }
    }

    // 工具循环后回写 timeline/surfaces，避免历史恢复丢 A2UI 卡片。
    {
        let agent = session.lock().await;
        if let Err(e) = agent.patch_last_assistant_timeline(timeline.reasoning_details_snapshot()) {
            tracing::warn!(error = %e, "patch assistant timeline after tools failed");
        }
    }
    true
}

/// 触发 PRE/POST_API_REQUEST hook 并发起 LLM 流式请求。
///
/// 成功返回 `Ok(stream)`；失败返回 `Err(error_string)` 并已在 hook 中记录。
pub(super) async fn stream_chat_with_hooks(
    session: &Arc<Mutex<AgentLoop>>,
    streamer: &ProviderStreamer,
    system_prompt: &str,
    history: &[types::message::Message],
    tools: Vec<serde_json::Value>,
) -> Result<super::types::AssistantContentStream, String> {
    {
        let agent = session.lock().await;
        let sid = agent.session_id().to_string();
        let turn_id = agent.current_turn_id().map(str::to_string);
        let _ = agent.fire_hook(
            ::hooks::PRE_API_REQUEST,
            ::hooks::HookPayload {
                session_id: sid,
                turn_id,
                ..Default::default()
            },
        );
    }
    match streamer.stream_chat(system_prompt, history, tools).await {
        Ok(s) => {
            let agent = session.lock().await;
            let sid = agent.session_id().to_string();
            let turn_id = agent.current_turn_id().map(str::to_string);
            let _ = agent.fire_hook(
                ::hooks::POST_API_REQUEST,
                ::hooks::HookPayload {
                    session_id: sid,
                    turn_id,
                    ..Default::default()
                },
            );
            Ok(s)
        }
        Err(err) => {
            let agent = session.lock().await;
            let sid = agent.session_id().to_string();
            let turn_id = agent.current_turn_id().map(str::to_string);
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
