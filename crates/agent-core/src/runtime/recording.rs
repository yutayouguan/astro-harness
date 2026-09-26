//! AgentLoop 消息记录方法：assistant / user / tool 角色消息的持久化与会话镜像维护。

use agent_protocol::{
    build_hook_prompt_message, parse_hook_prompt_message, ContentItem, HookPromptFragment,
    HookPromptItem, ResponseItem, ToolStatus,
};
use session::ConversationStore;

use super::AgentLoop;

#[derive(Default)]
pub(crate) struct ToolResultRecord<'a> {
    pub(crate) media: &'a [types::MediaAsset],
    pub(crate) file_changes: &'a [types::ToolFileChange],
    pub(crate) status: Option<&'a ToolStatus>,
    pub(crate) metadata: Option<&'a types::ToolResultMetadata>,
}

impl AgentLoop {
    pub(crate) async fn persist_response_items(
        &self,
        items: &[agent_protocol::ResponseItem],
    ) -> anyhow::Result<Vec<i64>> {
        let rollout = self.runtime_io.get().map(|bindings| &bindings.rollout);
        super::response_journal::append_canonical(rollout, items).await?;
        #[cfg(test)]
        if let Some(hook) = self
            .services
            .response_projection_write
            .lock()
            .expect("response projection hook mutex poisoned")
            .as_ref()
        {
            hook()?;
        }
        super::response_journal::project(&self.services.sessions, &self.session_id, items).await
    }

    pub(crate) async fn persist_rollout_items(
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
            .get_response_items(&self.session_id)
            .await?
            .last()
            .is_some_and(|item| item.role() == Some("user"))
        {
            let items = vec![ResponseItem::Message {
                id: None,
                role: "assistant".into(),
                content: vec![ContentItem::OutputText {
                    text: content.to_string(),
                }],
                phase: None,
                internal_chat_message_metadata_passthrough: Some(serde_json::json!({
                    "astro_finish_reason": finish_reason,
                })),
            }];
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
    /// SQLite 索引、rollout 与运行时均保存同一份原生 ResponseItem。
    pub async fn record_assistant_message_with_tools(
        &self,
        content: &str,
        tool_calls: Option<Vec<types::model_tool::ToolCall>>,
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
        _content: &str,
        _tool_calls: Option<Vec<types::model_tool::ToolCall>>,
        _reasoning: Option<&str>,
        reasoning_details: Option<serde_json::Value>,
        response_items: Vec<ResponseItem>,
    ) -> anyhow::Result<()> {
        let _write_guard = self.conversation_write_lock.lock().await;
        let response_items = attach_assistant_timeline_metadata(response_items, reasoning_details);
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
                    .map(|c| types::model_tool::ToolCall {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        namespace: c.namespace.clone(),
                        arguments: c.arguments.clone(),
                        signature: c.signature.clone(),
                    })
                    .collect(),
            )
        };
        self.record_assistant_message_with_tools(text, tc, reasoning, reasoning_details)
            .await
    }

    /// 将 user 角色消息写入记忆与会话镜像。
    ///
    /// 供 `Stop` 的 `KeepGoing(msg)` 等下游控制流场景使用：与 `pending_inject_context`
    /// 的临时注入不同，本方法直接落盘并写入 `SessionState.history`，确保下一轮 API 历史与
    /// `SessionStore` 保持一致（角色交替），避免连续 assistant 触发 Provider 400。
    pub async fn record_user_message(&self, content: &str) -> anyhow::Result<()> {
        let _write_guard = self.conversation_write_lock.lock().await;
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

    /// Persist Stop-hook feedback as the native Responses user item used by Codex.
    ///
    /// The same attributed native item is written to the rollout and SQLite index.
    pub(crate) async fn record_hook_prompt(
        &self,
        fragments: Vec<HookPromptFragment>,
    ) -> anyhow::Result<Option<HookPromptItem>> {
        let Some(item) = build_hook_prompt_message(&fragments) else {
            return Ok(None);
        };
        let hook_prompt = match &item {
            ResponseItem::Message { id, content, .. } => {
                let prompt =
                    parse_hook_prompt_message(id.as_deref(), content).ok_or_else(|| {
                        anyhow::anyhow!("failed to parse the generated hook prompt message")
                    })?;
                prompt
            }
            _ => unreachable!("hook prompt builder must return a message"),
        };

        let _write_guard = self.conversation_write_lock.lock().await;
        self.persist_response_items(std::slice::from_ref(&item))
            .await?;
        self.record_response_items_unlocked(vec![item]);
        Ok(Some(hook_prompt))
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
        let tool_name = tool_name.map(types::ToolName::plain);
        self.record_tool_result_with_id_and_media(
            tool_call_id,
            tool_name.as_ref(),
            content,
            ToolResultRecord::default(),
        )
        .await
    }

    pub(crate) async fn record_tool_result_with_id_and_media(
        &self,
        tool_call_id: Option<&str>,
        tool_name: Option<&types::ToolName>,
        content: &str,
        record: ToolResultRecord<'_>,
    ) -> anyhow::Result<()> {
        // 外部上下文（联网检索 / 远端 MCP / 浏览器）会污染该线程的记忆来源。
        if let Some(name) = tool_name.filter(|name| types::is_external_context_source(name)) {
            if let Err(error) = self
                .services
                .sessions
                .mark_memory_polluted(&self.session_id)
                .await
            {
                tracing::warn!(tool = %name, %error, "failed to mark thread memory pollution");
            }
        }
        let _write_guard = self.conversation_write_lock.lock().await;
        let (_, mut media) = types::extract_tool_media(content);
        for asset in record.media {
            if !media.contains(asset) {
                media.push(asset.clone());
            }
        }
        let output = agent_protocol::FunctionCallOutputPayload::from_text(content.to_string());
        let metadata = (!media.is_empty()
            || !record.file_changes.is_empty()
            || record.status.is_some()
            || record.metadata.is_some())
        .then(|| {
            let mut metadata = serde_json::Map::new();
            if !media.is_empty() {
                metadata.insert(
                    "astro_media".into(),
                    serde_json::to_value(&media).unwrap_or_default(),
                );
            }
            if let Some(status) = record.status {
                metadata.insert(
                    "astro_tool_status".into(),
                    serde_json::to_value(status).unwrap_or_default(),
                );
            }
            if !record.file_changes.is_empty() {
                metadata.insert(
                    "astro_file_changes_v1".into(),
                    serde_json::to_value(record.file_changes).unwrap_or_default(),
                );
            }
            if let Some(tool_result_metadata) = record.metadata {
                metadata.insert(
                    "astro_tool_result_metadata_v1".into(),
                    tool_result_metadata.stored_value(),
                );
            }
            serde_json::Value::Object(metadata)
        });
        let namespace = tool_name.and_then(types::ToolName::namespace);
        let name = tool_name.map(types::ToolName::name);
        let spill_key = agent_protocol::ResponseItemId::new("tool_output");
        let mut item = match (namespace, name) {
            (None, Some("tool_search")) => ResponseItem::ToolSearchOutput {
                id: None,
                call_id: tool_call_id.map(str::to_string),
                status: "completed".into(),
                execution: "client".into(),
                tools: serde_json::from_str(content).unwrap_or_default(),
                internal_chat_message_metadata_passthrough: metadata,
            },
            (None, Some("apply_patch" | "exec"))
                if tool_call_id.is_some_and(|id| !id.is_empty()) =>
            {
                ResponseItem::CustomToolCallOutput {
                    id: None,
                    call_id: tool_call_id.expect("guarded above").to_string(),
                    name: name.map(str::to_string),
                    output,
                    internal_chat_message_metadata_passthrough: metadata,
                }
            }
            _ => ResponseItem::FunctionCallOutput {
                id: None,
                call_id: tool_call_id.map(str::to_string),
                name: name.map(str::to_string),
                namespace: namespace.map(str::to_string),
                output,
                internal_chat_message_metadata_passthrough: metadata,
            },
        };
        if content.len() >= types::DEFAULT_SPILL_THRESHOLD_BYTES {
            match types::write_tool_spill_with_key(
                self.memory_dir(),
                &self.session_id,
                spill_key.as_str(),
                content,
            ) {
                Ok(path) => {
                    let rel = types::spill_path_for_prompt(self.memory_dir(), &path);
                    let view = types::make_spill_view(name, &rel, content.len(), content);
                    attach_response_item_metadata(
                        &mut item,
                        "astro_compressed_output",
                        serde_json::Value::String(view),
                    )?;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "tool spill write failed; keeping inline content");
                }
            }
        }
        let ids = self
            .persist_response_items(std::slice::from_ref(&item))
            .await?;
        if let Some(message_id) = ids.first().copied() {
            self.register_media_artifacts(&media, message_id).await;
        }
        self.record_response_items_unlocked(vec![item]);
        Ok(())
    }
}

fn attach_response_item_metadata(
    item: &mut ResponseItem,
    key: &str,
    value: serde_json::Value,
) -> anyhow::Result<()> {
    let mut encoded = serde_json::to_value(&*item)?;
    let object = encoded
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("response item must serialize as an object"))?;
    let metadata = object
        .entry("internal_chat_message_metadata_passthrough")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    metadata
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("response item metadata must be an object"))?
        .insert(key.to_string(), value);
    *item = serde_json::from_value(encoded)?;
    Ok(())
}

/// 把 Astro 时间线元数据挂到本轮 assistant 原生 item 上。
///
/// 原生 Responses 路径直接落盘 provider 返回的 items（`reasoning_details` 不会自己
/// 进 metadata），必须在这里补齐本轮 `astro_timeline_v1` / `astro_surfaces_v1`；
/// 否则历史回放只能捡到别的行上的旧时间线，气泡会错配（activity 段找不到活动，
/// 只剩一串「思考完成」）。
///
/// 目标行取本轮最后一条 assistant message；本轮没有 message（纯工具轮）时退化到
/// 最后一条 reasoning，两者都没有则不改动 items。
fn attach_assistant_timeline_metadata(
    items: Vec<ResponseItem>,
    reasoning_details: Option<serde_json::Value>,
) -> Vec<ResponseItem> {
    let Some(serde_json::Value::Object(details)) = reasoning_details else {
        return items;
    };
    if details.is_empty() {
        return items;
    }
    let mut items = items;
    let target = items
        .iter()
        .rposition(|item| matches!(item, ResponseItem::Message { role, .. } if role == "assistant"))
        .or_else(|| {
            items
                .iter()
                .rposition(|item| matches!(item, ResponseItem::Reasoning { .. }))
        });
    let Some(index) = target else {
        return items;
    };
    for (key, value) in details {
        if let Err(error) = attach_response_item_metadata(&mut items[index], &key, value) {
            tracing::warn!(error = %error, key = %key, "attach assistant timeline metadata failed");
            break;
        }
    }
    items
}

fn response_items_for_assistant(
    content: &str,
    calls: &[types::model_tool::ToolCall],
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
        let item = match (call.namespace.as_deref(), call.name.as_str()) {
            (None, "tool_search") => ResponseItem::ToolSearchCall {
                id: Some(call.id.clone().into()),
                call_id: Some(call.id.clone()),
                status: Some("completed".into()),
                execution: "client".into(),
                arguments: call.arguments.clone(),
                internal_chat_message_metadata_passthrough: None,
            },
            (None, "apply_patch" | "exec") => ResponseItem::CustomToolCall {
                id: Some(call.id.clone().into()),
                status: Some("completed".into()),
                call_id: call.id.clone(),
                name: call.name.clone(),
                namespace: call.namespace.clone(),
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
                namespace: call.namespace.clone(),
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
