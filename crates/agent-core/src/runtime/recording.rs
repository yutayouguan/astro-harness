//! AgentLoop 消息记录方法：assistant / user / tool 角色消息的持久化与会话镜像维护。

use types::message::Message;
use session::NewMessage;

use super::AgentLoop;

impl AgentLoop {
    /// 确保会话行存在（不存在则按 `source` 创建）。
    pub fn ensure_session(&self, source: &str) -> anyhow::Result<()> {
        self.sessions.ensure_session(&self.session_id, source)
    }

    /// 将 assistant 纯文本回复写入记忆与会话镜像。
    pub fn record_assistant_message(&mut self, content: &str) -> anyhow::Result<()> {
        self.record_assistant_message_with_tools(content, None, None, None)
    }

    /// 将 assistant 回复（可含 tool_calls / reasoning / reasoning_details）写入记忆与会话镜像。
    ///
    /// 非空 `tool_calls` 时使用 `Message::assistant_with_tools` 保留结构化调用信息；
    /// 落盘通过 `SessionStore::append_message` 写入富字段。
    pub fn record_assistant_message_with_tools(
        &mut self,
        content: &str,
        tool_calls: Option<Vec<types::message::ToolCall>>,
        reasoning: Option<&str>,
        reasoning_details: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        let tool_calls_json = match &tool_calls {
            Some(calls) if !calls.is_empty() => Some(serde_json::to_value(calls)?),
            _ => None,
        };
        let reasoning = reasoning.filter(|r| !r.is_empty());
        let thought_signature =
            types::message::google_thought_signature_from_details(&reasoning_details);
        self.sessions.ensure_session(&self.session_id, "tauri")?;
        self.sessions.append_message(NewMessage {
            content: Some(content),
            tool_calls: tool_calls_json,
            reasoning,
            reasoning_details: reasoning_details.clone(),
            ..NewMessage::empty(&self.session_id, "assistant")
        })?;
        let msg = match tool_calls {
            Some(calls) if !calls.is_empty() => Message::assistant_with_tools(content, calls),
            _ => Message::assistant(content),
        };
        let mut msg = msg;
        msg.reasoning = reasoning.map(str::to_string);
        msg.thought_signature = thought_signature;
        self.session_messages.push(msg);
        Ok(())
    }

    /// 记录 assistant 回复并关联已解析的工具调用。
    ///
    /// 封装 `ParsedToolCall → ToolCall` 映射，消除 streaming / headless 的重复代码。
    pub(crate) fn record_assistant_with_calls(
        &mut self,
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
    }

    /// 工具执行后回写最近一条 assistant 的 timeline/surfaces（避免历史丢 A2UI 卡）。
    pub fn patch_last_assistant_timeline(
        &self,
        reasoning_details: serde_json::Value,
    ) -> anyhow::Result<()> {
        self.sessions
            .patch_last_assistant_reasoning_details(&self.session_id, &reasoning_details)
    }

    /// 将 user 角色消息写入记忆与会话镜像。
    ///
    /// 供 `pre_verify` 的 `KeepGoing(msg)` 等下游控制流场景使用：与 `pending_inject_context`
    /// 的临时注入不同，本方法直接落盘并写入 `session_messages`，确保下一轮 API 历史与
    /// `SessionStore` 保持一致（角色交替），避免连续 assistant 触发 Provider 400。
    pub fn record_user_message(&mut self, content: &str) -> anyhow::Result<()> {
        self.sessions.ensure_session(&self.session_id, "tauri")?;
        self.sessions.append_message(NewMessage {
            content: Some(content),
            ..NewMessage::empty(&self.session_id, "user")
        })?;
        self.session_messages.push(Message::user(content));
        Ok(())
    }

    /// 将 tool 角色结果写入记忆与会话镜像（无 tool_call_id / tool_name）。
    pub fn record_tool_result(&mut self, content: &str) -> anyhow::Result<()> {
        self.record_tool_result_with_id(None, None, content)
    }

    /// 将工具生成的媒体文件实时登记到 artifacts 索引，关联当前会话与消息。
    ///
    /// 否则这些文件仅在文件空间 `reconcile` 扫盘时以 `session_id=None` 补登记，
    /// 导致「会话中生成的文件」被归入「未关联会话」。
    fn register_media_artifacts(&self, media: &[types::MediaAsset], msg_id: i64) {
        if media.is_empty() {
            return;
        }
        let db = match artifacts::open_default(self.memory_dir()) {
            Ok(db) => db,
            Err(e) => {
                tracing::debug!(error = %e, "open artifacts db failed; skip media register");
                return;
            }
        };
        let workspace = self.memory.workspace_dir.clone();
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
            if let Err(e) = db.register(
                path,
                artifacts::ArtifactSource::AgentWrite,
                Some(&session_id),
                Some(&message_id),
                Some(&agent_id),
            ) {
                tracing::debug!(error = %e, path, "register media artifact failed");
            }
        }
    }

    /// 将 tool 角色结果写入记忆与会话镜像，并关联 `tool_call_id` / `tool_name`。
    pub fn record_tool_result_with_id(
        &mut self,
        tool_call_id: Option<&str>,
        tool_name: Option<&str>,
        content: &str,
    ) -> anyhow::Result<()> {
        let (_, media) = types::extract_tool_media(content);
        let media_owned = if media.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&media)?)
        };
        self.sessions.ensure_session(&self.session_id, "tauri")?;
        let msg_id = self.sessions.append_message(NewMessage {
            content: Some(content),
            tool_call_id,
            tool_name,
            media_json: media_owned.as_deref(),
            ..NewMessage::empty(&self.session_id, "tool")
        })?;

        self.register_media_artifacts(&media, msg_id);

        let mut spill_view: Option<String> = None;
        if content.len() >= types::DEFAULT_SPILL_THRESHOLD_BYTES {
            match types::write_tool_spill(self.memory_dir(), &self.session_id, msg_id, content) {
                Ok(path) => {
                    let rel = types::spill_path_for_prompt(self.memory_dir(), &path);
                    let view = types::make_spill_view(tool_name, &rel, content.len(), content);
                    if self
                        .sessions
                        .update_message_compressed_content(msg_id, Some(&view))
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

        let mut msg = match tool_call_id {
            Some(id) if !id.is_empty() => Message::tool_with_id(id, content),
            _ => Message::tool(content),
        };
        msg.media = media;
        if let Some(view) = spill_view {
            msg.compressed_content = Some(view);
        }
        self.session_messages.push(msg);
        Ok(())
    }
}
