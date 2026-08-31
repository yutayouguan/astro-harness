//! AgentLoop 消息记录方法：assistant / user / tool 角色消息的持久化与会话镜像维护。

use agent_protocol::{ContentItem, ResponseItem};
use session::{ConversationStore, NewMessage};

use super::AgentLoop;

impl AgentLoop {
    pub(crate) async fn persist_response_items(
        &self,
        items: &[agent_protocol::ResponseItem],
    ) -> anyhow::Result<()> {
        let Some(bindings) = self.runtime_io.get() else {
            return Ok(());
        };
        bindings
            .rollout
            .record(
                items
                    .iter()
                    .cloned()
                    .map(agent_rollout::RolloutItem::ResponseItem)
                    .collect(),
            )
            .await?;
        Ok(())
    }

    pub(crate) async fn ensure_assistant_error_boundary(&self) -> anyhow::Result<()> {
        const CONTENT: &str =
            "[astro:system]\nThe agent turn failed before producing an assistant response.";
        self.ensure_assistant_boundary(CONTENT, "error").await
    }

    pub(crate) async fn ensure_assistant_interrupted_boundary(&self) -> anyhow::Result<()> {
        const CONTENT: &str =
            "[astro:system]\nThe previous agent turn was interrupted before producing an assistant response.";
        self.ensure_assistant_boundary(CONTENT, "interrupted").await
    }

    /// Persist a model-visible boundary before publishing `TurnAborted`.
    ///
    /// This is deliberately a developer item rather than a synthetic assistant answer: a
    /// partially streamed assistant response may already exist, while the next model request
    /// still needs an unambiguous signal that the previous turn did not complete normally.
    pub(crate) async fn record_interrupted_turn_marker(&self) -> anyhow::Result<()> {
        // Preserve Astro's alternating user/assistant projection invariant before adding the
        // richer developer marker to the canonical provider history.
        self.ensure_assistant_interrupted_boundary().await?;
        let item = ResponseItem::Message {
            id: None,
            role: "developer".into(),
            content: vec![ContentItem::InputText {
                text: "<turn_aborted>\nThe user intentionally interrupted the previous turn. Any in-flight tool work was stopped. If the user continues, do not assume the interrupted work completed.\n</turn_aborted>".into(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        };
        self.persist_response_items(std::slice::from_ref(&item))
            .await?;
        self.record_response_items_unlocked(vec![item]);
        Ok(())
    }

    async fn ensure_assistant_boundary(
        &self,
        content: &str,
        finish_reason: &str,
    ) -> anyhow::Result<()> {
        self.services
            .sessions
            .ensure_session(&self.session_id, "tauri")
            .await?;
        if self
            .services
            .sessions
            .get_messages(&self.session_id)
            .await?
            .last()
            .is_some_and(|message| message.role == "user")
        {
            self.services
                .sessions
                .append_message(NewMessage {
                    content: Some(content),
                    finish_reason: Some(finish_reason),
                    ..NewMessage::empty(&self.session_id, "assistant")
                })
                .await?;
        }
        if self
            .clone_history()
            .await
            .last()
            .is_some_and(|message| message.role == types::message::Role::User)
        {
            let items = response_items_for_assistant(content, &[], None, None)?;
            self.persist_response_items(&items).await?;
            self.record_response_items_unlocked(items);
        }
        Ok(())
    }

    /// 确保会话行存在（不存在则按 `source` 创建）。
    pub async fn ensure_session(&self, source: &str) -> anyhow::Result<()> {
        self.services
            .sessions
            .ensure_session(&self.session_id, source)
            .await
    }

    /// 将 assistant 纯文本回复写入记忆与会话镜像。
    pub async fn record_assistant_message(&self, content: &str) -> anyhow::Result<()> {
        self.record_assistant_message_with_tools(content, None, None, None)
            .await
    }

    /// 将 assistant 回复（可含 tool_calls / reasoning / reasoning_details）写入记忆与会话镜像。
    ///
    /// SQLite 仅保留 UI/检索投影；rollout 与运行时直接保存原生 ResponseItem。
    pub async fn record_assistant_message_with_tools(
        &self,
        content: &str,
        tool_calls: Option<Vec<types::message::ToolCall>>,
        reasoning: Option<&str>,
        reasoning_details: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        let response_items = response_items_for_assistant(
            content,
            tool_calls.as_deref().unwrap_or_default(),
            reasoning.filter(|value| !value.is_empty()),
            reasoning_details.clone(),
        )?;
        self.record_assistant_response_items(
            content,
            tool_calls,
            reasoning,
            reasoning_details,
            response_items,
        )
        .await
    }

    pub(crate) async fn record_assistant_response_items(
        &self,
        content: &str,
        tool_calls: Option<Vec<types::message::ToolCall>>,
        reasoning: Option<&str>,
        reasoning_details: Option<serde_json::Value>,
        response_items: Vec<ResponseItem>,
    ) -> anyhow::Result<()> {
        let _write_guard = self.conversation_write_lock.lock().await;
        let tool_calls_json = match &tool_calls {
            Some(calls) if !calls.is_empty() => Some(serde_json::to_value(calls)?),
            _ => None,
        };
        let reasoning = reasoning.filter(|r| !r.is_empty());
        self.services
            .sessions
            .ensure_session(&self.session_id, "tauri")
            .await?;
        self.services
            .sessions
            .append_message(NewMessage {
                content: Some(content),
                tool_calls: tool_calls_json,
                reasoning,
                reasoning_details: reasoning_details.clone(),
                ..NewMessage::empty(&self.session_id, "assistant")
            })
            .await?;
        self.persist_response_items(&response_items).await?;
        self.record_response_items_unlocked(response_items);
        Ok(())
    }

    /// 记录 assistant 回复并关联已解析的工具调用。
    ///
    /// 封装 `ParsedToolCall → ToolCall` 映射，消除 foreground / background 的重复代码。
    pub(crate) async fn record_assistant_with_calls(
        &self,
        text: &str,
        calls: &[types::ParsedToolCall],
        reasoning: Option<&str>,
        reasoning_details: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        let tc = if calls.is_empty() {
            None
        } else {
            Some(
                calls
                    .iter()
                    .map(|c| types::message::ToolCall {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        arguments: c.arguments.clone(),
                        signature: c.signature.clone(),
                    })
                    .collect(),
            )
        };
        self.record_assistant_message_with_tools(text, tc, reasoning, reasoning_details)
            .await
    }

    /// 工具执行后回写最近一条 assistant 的 timeline/surfaces（避免历史丢 A2UI 卡）。
    pub async fn patch_last_assistant_timeline(
        &self,
        reasoning_details: serde_json::Value,
    ) -> anyhow::Result<()> {
        self.services
            .sessions
            .patch_last_assistant_reasoning_details(&self.session_id, &reasoning_details)
            .await
    }

    /// 将 user 角色消息写入记忆与会话镜像。
    ///
    /// 供 `Stop` 的 `KeepGoing(msg)` 等下游控制流场景使用：与 `pending_inject_context`
    /// 的临时注入不同，本方法直接落盘并写入 `SessionState.history`，确保下一轮 API 历史与
    /// `SessionStore` 保持一致（角色交替），避免连续 assistant 触发 Provider 400。
    pub async fn record_user_message(&self, content: &str) -> anyhow::Result<()> {
        let _write_guard = self.conversation_write_lock.lock().await;
        self.services
            .sessions
            .ensure_session(&self.session_id, "tauri")
            .await?;
        self.services
            .sessions
            .append_message(NewMessage {
                content: Some(content),
                ..NewMessage::empty(&self.session_id, "user")
            })
            .await?;
        let item = ResponseItem::Message {
            id: None,
            role: "user".into(),
            content: vec![ContentItem::InputText {
                text: content.to_string(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        };
        self.persist_response_items(std::slice::from_ref(&item))
            .await?;
        self.record_response_items_unlocked(vec![item]);
        Ok(())
    }

    /// 将 tool 角色结果写入记忆与会话镜像（无 tool_call_id / tool_name）。
    pub async fn record_tool_result(&self, content: &str) -> anyhow::Result<()> {
        self.record_tool_result_with_id(None, None, content).await
    }

    /// 将工具生成的媒体文件实时登记到 artifacts 索引，关联当前会话与消息。
    ///
    /// 否则这些文件仅在文件空间 `reconcile` 扫盘时以 `session_id=None` 补登记，
    /// 导致「会话中生成的文件」被归入「未关联会话」。
    async fn register_media_artifacts(&self, media: &[types::MediaAsset], msg_id: i64) {
        if media.is_empty() {
            return;
        }
        let db = match artifacts::open_default(self.memory_dir()).await {
            Ok(db) => db,
            Err(e) => {
                tracing::debug!(error = %e, "open artifacts db failed; skip media register");
                return;
            }
        };
        let workspace = self.workspace_dir.clone();
        let session_id = self.session_id.clone();
        let message_id = msg_id.to_string();
        let agent_id = self.agent_id().to_string();
        for asset in media {
            let Some(rel) = asset.workspace_path() else {
                continue; // data URL / 远程 URI 不落盘，跳过
            };
            let abs = workspace.join(rel);
            let Some(path) = abs.to_str() else {
                continue;
            };
            if let Err(e) = db
                .register(
                    path,
                    artifacts::ArtifactSource::AgentWrite,
                    Some(&session_id),
                    Some(&message_id),
                    Some(&agent_id),
                )
                .await
            {
                tracing::debug!(error = %e, path, "register media artifact failed");
            }
        }
    }

    /// 将 tool 角色结果写入记忆与会话镜像，并关联 `tool_call_id` / `tool_name`。
    pub async fn record_tool_result_with_id(
        &self,
        tool_call_id: Option<&str>,
        tool_name: Option<&str>,
        content: &str,
    ) -> anyhow::Result<()> {
        self.record_tool_result_with_id_and_media(tool_call_id, tool_name, content, &[])
            .await
    }

    pub(crate) async fn record_tool_result_with_id_and_media(
        &self,
        tool_call_id: Option<&str>,
        tool_name: Option<&str>,
        content: &str,
        additional_media: &[types::MediaAsset],
    ) -> anyhow::Result<()> {
        let _write_guard = self.conversation_write_lock.lock().await;
        let (_, mut media) = types::extract_tool_media(content);
        for asset in additional_media {
            if !media.contains(asset) {
                media.push(asset.clone());
            }
        }
        let media_owned = if media.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&media)?)
        };
        self.services
            .sessions
            .ensure_session(&self.session_id, "tauri")
            .await?;
        let msg_id = self
            .services
            .sessions
            .append_message(NewMessage {
                content: Some(content),
                tool_call_id,
                tool_name,
                media_json: media_owned.as_deref(),
                ..NewMessage::empty(&self.session_id, "tool")
            })
            .await?;

        self.register_media_artifacts(&media, msg_id).await;

        let mut spill_view: Option<String> = None;
        if content.len() >= types::DEFAULT_SPILL_THRESHOLD_BYTES {
            match types::write_tool_spill(self.memory_dir(), &self.session_id, msg_id, content) {
                Ok(path) => {
                    let rel = types::spill_path_for_prompt(self.memory_dir(), &path);
                    let view = types::make_spill_view(tool_name, &rel, content.len(), content);
                    if self
                        .services
                        .sessions
                        .update_message_compressed_content(msg_id, Some(&view))
                        .await
                        .is_ok()
                    {
                        spill_view = Some(view);
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "tool spill write failed; keeping inline content");
                }
            }
        }

        let output = agent_protocol::FunctionCallOutputPayload::from_text(content.to_string());
        let metadata = (!media.is_empty() || spill_view.is_some()).then(|| {
            let mut metadata = serde_json::Map::new();
            if let Some(view) = spill_view {
                metadata.insert(
                    "astro_compressed_output".into(),
                    serde_json::Value::String(view),
                );
            }
            if !media.is_empty() {
                metadata.insert(
                    "astro_media".into(),
                    serde_json::to_value(&media).unwrap_or_default(),
                );
            }
            serde_json::Value::Object(metadata)
        });
        let item = match tool_name {
            Some("tool_search") => ResponseItem::ToolSearchOutput {
                id: None,
                call_id: tool_call_id.map(str::to_string),
                status: "completed".into(),
                execution: "client".into(),
                tools: serde_json::from_str(content).unwrap_or_default(),
                internal_chat_message_metadata_passthrough: metadata,
            },
            Some("apply_patch" | "exec") if tool_call_id.is_some_and(|id| !id.is_empty()) => {
                ResponseItem::CustomToolCallOutput {
                    id: None,
                    call_id: tool_call_id.expect("guarded above").to_string(),
                    name: tool_name.map(str::to_string),
                    output,
                    internal_chat_message_metadata_passthrough: metadata,
                }
            }
            _ => ResponseItem::FunctionCallOutput {
                id: None,
                call_id: tool_call_id.map(str::to_string),
                name: tool_name.map(str::to_string),
                namespace: None,
                output,
                internal_chat_message_metadata_passthrough: metadata,
            },
        };
        self.persist_response_items(std::slice::from_ref(&item))
            .await?;
        self.record_response_items_unlocked(vec![item]);
        Ok(())
    }
}

fn response_items_for_assistant(
    content: &str,
    calls: &[types::message::ToolCall],
    reasoning: Option<&str>,
    metadata: Option<serde_json::Value>,
) -> anyhow::Result<Vec<ResponseItem>> {
    let mut items = Vec::new();
    if let Some(reasoning) = reasoning.filter(|value| !value.is_empty()) {
        items.push(ResponseItem::Reasoning {
            id: None,
            summary: Vec::new(),
            content: Some(vec![serde_json::json!({
                "type": "reasoning_text",
                "text": reasoning,
            })]),
            encrypted_content: None,
            internal_chat_message_metadata_passthrough: metadata.clone(),
        });
    }
    if !content.is_empty() || calls.is_empty() {
        items.push(ResponseItem::Message {
            id: None,
            role: "assistant".into(),
            content: vec![ContentItem::OutputText {
                text: content.to_string(),
            }],
            phase: None,
            internal_chat_message_metadata_passthrough: metadata,
        });
    }
    for call in calls {
        let item = match call.name.as_str() {
            "tool_search" => ResponseItem::ToolSearchCall {
                id: Some(call.id.clone().into()),
                call_id: Some(call.id.clone()),
                status: Some("completed".into()),
                execution: "client".into(),
                arguments: call.arguments.clone(),
                internal_chat_message_metadata_passthrough: None,
            },
            "apply_patch" | "exec" => ResponseItem::CustomToolCall {
                id: Some(call.id.clone().into()),
                status: Some("completed".into()),
                call_id: call.id.clone(),
                name: call.name.clone(),
                namespace: None,
                input: call
                    .arguments
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| call.arguments.to_string()),
                internal_chat_message_metadata_passthrough: None,
            },
            _ => ResponseItem::FunctionCall {
                id: Some(call.id.clone().into()),
                name: call.name.clone(),
                namespace: None,
                arguments: match &call.arguments {
                    serde_json::Value::String(value) => value.clone(),
                    value => serde_json::to_string(value)?,
                },
                encrypted_function_args: None,
                call_id: call.id.clone(),
                internal_chat_message_metadata_passthrough: None,
            },
        };
        items.push(item);
    }
    Ok(items)
}
