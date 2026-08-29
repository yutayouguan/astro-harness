//! 非阻塞、用户可见的进度消息，用于活跃回合。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;

const ASYNC_USER_MESSAGE_MARKER: &str = "astro_async_user_message";

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SendUserMessageAsyncArgs {
    /// 简洁的确认、进度更新或阻塞性问题，展示给用户。
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AsyncUserMessagePayload {
    pub message: String,
}

pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "send_user_message_async".into(),
        toolset: "send_user_message_async".into(),
        description: "Send a concise, user-visible acknowledgment, important update, or blocking question. Returns immediately; any reply arrives asynchronously as a new user message."
            .into(),
        schema: schema_for_args::<SendUserMessageAsyncArgs>(),
        check_fn: None,
        icon: "message-circle",
        exclusive_access: true,
        ..crate::registry::ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["send_user_message_async"],
    sync_ctx: dispatch,
    args: SendUserMessageAsyncArgs,
}

fn dispatch(_ctx: &mut ToolContext<'_>, args: &SendUserMessageAsyncArgs) -> anyhow::Result<String> {
    let message = args.message.trim();
    if message.is_empty() {
        anyhow::bail!("send_user_message_async requires a non-empty message");
    }
    Ok(json!({
        ASYNC_USER_MESSAGE_MARKER: true,
        "message": message,
    })
    .to_string())
}

pub fn parse_async_user_message(result: &str) -> Option<AsyncUserMessagePayload> {
    let value: serde_json::Value = serde_json::from_str(result).ok()?;
    if value.get(ASYNC_USER_MESSAGE_MARKER)?.as_bool() != Some(true) {
        return None;
    }
    let message = value.get("message")?.as_str()?.trim();
    if message.is_empty() {
        return None;
    }
    Some(AsyncUserMessagePayload {
        message: message.into(),
    })
}

#[cfg(test)]
#[path = "send_user_message_async_tests.rs"]
mod tests;
