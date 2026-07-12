//! 会话消息 → Provider 消息转换。
//!
//! 将应用内 `common::message::Message` 序列转为各 LLM Provider 统一的 `ChatMessage` 格式，
//! 并补齐 system 前缀与 tool 角色的 `name` 回溯（Provider 要求 tool 消息关联原调用名）。

use common::message::{Message, Role};
use providers::trait_::{ChatMessage as ProviderMessage, ChatToolCall};

/// 将会话历史与 system prompt 转为 Provider 可消费的聊天消息列表。
///
/// 首条固定为 `system` 角色；tool 消息会从历史中反向查找对应 `tool_call_id` 以填充 `name`。
///
/// # 参数
///
/// - `system_prompt`：已组装的 system 指令全文。
/// - `session`：按时间顺序排列的会话消息切片。
///
/// # 返回
///
/// 可直接传入 Provider `chat` / `stream` API 的消息向量。
pub fn to_provider_messages(system_prompt: &str, session: &[Message]) -> Vec<ProviderMessage> {
    let mut messages = vec![ProviderMessage::text("system", system_prompt)];

    for message in session {
        let role = match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
            Role::Tool => "tool",
        };
        let tool_calls = message.tool_calls.as_ref().map(|calls| {
            calls
                .iter()
                .map(|c| ChatToolCall {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    arguments: c.arguments.clone(),
                })
                .collect()
        });
        let tool_name = if message.role == Role::Tool {
            message.tool_call_id.as_ref().and_then(|id| {
                session.iter().rev().find_map(|m| {
                    m.tool_calls
                        .as_ref()?
                        .iter()
                        .find(|c| &c.id == id)
                        .map(|c| c.name.clone())
                })
            })
        } else {
            None
        };
        messages.push(ProviderMessage {
            role: role.to_string(),
            content: message.content_str().to_string(),
            tool_calls,
            tool_call_id: message.tool_call_id.clone(),
            name: tool_name,
        });
    }

    messages
}
